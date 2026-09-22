//! Toda chamada do runtime no gateway decidiu quem aprova (#1343 S5).
//!
//! O pedido de confirmacao pausado so e retomado no turno seguinte quando o
//! turno chega com `ExecContext::approval_scope` — e o escopo so pode vir de
//! um remetente derivado pelo servidor. Um caminho novo que chame o runtime
//! sem decidir isso passaria despercebido: sem escopo, a pausa e terminal
//! (fail-closed, mas o humano nunca consegue aprovar), e com um escopo mal
//! montado um "sim" de outra pessoa poderia valer.
//!
//! Este teste varre `src/` atras de cada chamada a `process_message*` e exige
//! uma de duas coisas:
//!
//! - o contexto de execucao da chamada passa por
//!   `approval_scope::com_escopo(` (inline, ou no `let` do identificador que
//!   vai como ultimo argumento); ou
//! - o arquivo esta em [`SEM_ESCOPO`] com o numero exato de chamadas sem
//!   escopo e o motivo por escrito.
//!
//! Chamada nova sem escopo e fora da lista reprova o CI ate alguem decidir.
//! Chamada escopada que perdeu o escopo tambem reprova (a contagem por
//! arquivo e exata).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// O marcador de uma chamada escopada.
const TOKEN_ESCOPO: &str = "approval_scope::com_escopo(";

/// Arquivos com chamadas que ficam SEM escopo de proposito, quantas, e por
/// que. Sem escopo, a aprovacao so viria do historico — que nestes caminhos
/// ou e vazio ou e texto — e a pausa e terminal.
const SEM_ESCOPO: &[(&str, usize, &str)] = &[
    (
        "a2a.rs",
        2,
        "trafego de outro agente (A2A): nao ha prova de que um humano disse o \"sim\"",
    ),
    (
        "api.rs",
        3,
        "POST /api/sessions/{id}/messages: Web Console auth-free, a sessao vem do \
         path e nao ha remetente derivado pelo servidor",
    ),
    (
        "bootstrap/openclaw.rs",
        1,
        "mensagens repassadas por outro agente (OpenClaw), nao por um humano",
    ),
    (
        "rest_v1/messages.rs",
        1,
        "resposta do agente a uma mensagem de chat do workspace, one-shot e com \
         historico vazio",
    ),
];

const ENTRADAS: &[&str] = &[
    "process_message",
    "process_message_with_context",
    "process_message_with_agent_config",
    "process_message_streaming",
    "process_message_streaming_with_context",
    "process_message_streaming_with_agent_config",
    "process_message_streaming_with_events",
];

fn arquivos_rs(dir: &Path, fora: &mut Vec<PathBuf>) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entradas.flatten() {
        let p = e.path();
        if p.is_dir() {
            arquivos_rs(&p, fora);
        } else if p.extension().is_some_and(|x| x == "rs") {
            fora.push(p);
        }
    }
}

/// O codigo de producao do arquivo: corta no primeiro `#[cfg(test)]` seguido
/// de `mod`, que e onde o modulo de teste comeca por convencao da arvore.
/// Arquivos inteiros de teste (`tests.rs`) ficam de fora.
fn producao(caminho: &Path, fonte: &str) -> Option<String> {
    if caminho.file_name().is_some_and(|n| n == "tests.rs") {
        return None;
    }
    let linhas: Vec<&str> = fonte.lines().collect();
    let mut fim = linhas.len();
    for (i, l) in linhas.iter().enumerate() {
        if l.trim() == "#[cfg(test)]"
            && linhas[i + 1..]
                .iter()
                .find(|x| !x.trim().is_empty())
                .is_some_and(|x| x.trim_start().starts_with("mod "))
        {
            fim = i;
            break;
        }
    }
    Some(linhas[..fim].join("\n"))
}

/// Indice do `)` que fecha o `(` em `abre`.
fn fecha(fonte: &str, abre: usize) -> Option<usize> {
    let mut prof = 0i32;
    for (i, c) in fonte[abre..].char_indices() {
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => {
                prof -= 1;
                if prof == 0 {
                    return Some(abre + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// O ultimo argumento de topo de `args` (sem os parenteses).
fn ultimo_argumento(args: &str) -> &str {
    let mut prof = 0i32;
    let mut inicio = 0;
    let mut ultimo = "";
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => prof -= 1,
            ',' if prof == 0 => {
                let pedaco = args[inicio..i].trim();
                if !pedaco.is_empty() {
                    ultimo = pedaco;
                }
                inicio = i + 1;
            }
            _ => {}
        }
    }
    let resto = args[inicio..].trim();
    if resto.is_empty() { ultimo } else { resto }
}

/// O `let <nome> = ...;` mais proximo antes de `ate`.
fn binding(fonte: &str, nome: &str, ate: usize) -> Option<String> {
    let antes = &fonte[..ate];
    let alvo = [format!("let {nome} ="), format!("let mut {nome} =")];
    let pos = alvo.iter().filter_map(|a| antes.rfind(a.as_str())).max()?;
    let resto = &fonte[pos..];
    let mut prof = 0i32;
    for (i, c) in resto.char_indices() {
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => prof -= 1,
            ';' if prof == 0 => return Some(resto[..i].to_string()),
            _ => {}
        }
    }
    None
}

#[derive(Debug)]
struct Chamada {
    linha: usize,
    escopada: bool,
}

fn chamadas(fonte: &str) -> Vec<Chamada> {
    let mut fora = Vec::new();
    let mut i = 0;
    while let Some(rel) = fonte[i..].find(".process_message") {
        let pos = i + rel;
        let nome_ini = pos + 1;
        let nome_fim = fonte[nome_ini..]
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(fonte.len(), |n| nome_ini + n);
        let nome = &fonte[nome_ini..nome_fim];
        let depois = fonte[nome_fim..].trim_start();
        i = nome_fim;
        if !ENTRADAS.contains(&nome) || !depois.starts_with('(') {
            continue;
        }
        let abre = nome_fim + (fonte[nome_fim..].len() - depois.len());
        let Some(fim) = fecha(fonte, abre) else {
            continue;
        };
        let args = &fonte[abre + 1..fim];
        let exec = ultimo_argumento(args);
        let escopada = if exec.contains(TOKEN_ESCOPO) {
            true
        } else if let Some(nome) = exec
            .strip_prefix('&')
            .filter(|n| n.chars().all(|c| c.is_alphanumeric() || c == '_'))
        {
            binding(fonte, nome, pos).is_some_and(|b| b.contains(TOKEN_ESCOPO))
        } else {
            false
        };
        fora.push(Chamada {
            linha: fonte[..pos].lines().count(),
            escopada,
        });
    }
    fora
}

#[test]
fn toda_chamada_do_runtime_no_gateway_decidiu_quem_aprova() {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut arquivos = Vec::new();
    arquivos_rs(&raiz, &mut arquivos);
    arquivos.sort();

    let mut sem_escopo: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut escopadas = 0usize;
    for arq in &arquivos {
        let fonte = std::fs::read_to_string(arq).expect("ler fonte");
        let Some(prod) = producao(arq, &fonte) else {
            continue;
        };
        let rel = arq
            .strip_prefix(&raiz)
            .expect("dentro de src")
            .to_string_lossy()
            .replace('\\', "/");
        for c in chamadas(&prod) {
            if c.escopada {
                escopadas += 1;
            } else {
                sem_escopo.entry(rel.clone()).or_default().push(c.linha);
            }
        }
    }

    // Sanidade do detector: se o regex quebrar, o teste nao pode passar
    // "por nao achar nada". Hoje: 11 canais bootstrap (10 com dois ramos,
    // mais o de voz do Telegram), whatsapp_linked, ws, parrot, openai (2) e
    // mobile.
    assert!(
        escopadas >= 25,
        "o detector achou so {escopadas} chamadas escopadas; ele quebrou?"
    );

    let mut erros = Vec::new();
    for (arq, linhas) in &sem_escopo {
        match SEM_ESCOPO.iter().find(|(a, _, _)| a == arq) {
            None => erros.push(format!(
                "{arq}: chamada(s) do runtime sem escopo de aprovacao nas linhas {linhas:?}. \
                 Passe o ExecContext por `crate::approval_scope::com_escopo(..)` com um \
                 remetente derivado pelo servidor, ou, se nao ha humano verificavel, \
                 acrescente o arquivo a SEM_ESCOPO com o motivo."
            )),
            Some((_, n, _)) if *n != linhas.len() => erros.push(format!(
                "{arq}: {} chamada(s) sem escopo (linhas {linhas:?}), a lista diz {n}. \
                 Decida a chamada nova ou atualize SEM_ESCOPO.",
                linhas.len()
            )),
            Some(_) => {}
        }
    }
    for (arq, n, _) in SEM_ESCOPO {
        if !sem_escopo.contains_key(*arq) {
            erros.push(format!(
                "{arq}: esta em SEM_ESCOPO ({n}) mas nao tem chamada sem escopo; \
                 tire da lista."
            ));
        }
    }
    assert!(erros.is_empty(), "{}", erros.join("\n"));
}

/// O detector enxerga as tres formas que a arvore usa: escopo inline, escopo
/// no `let` do identificador, e chamada sem contexto nenhum.
#[test]
fn o_detector_distingue_escopada_de_nao_escopada() {
    let fonte = r#"
        let exec = crate::approval_scope::com_escopo(a, "x", &s, &u);
        rt.process_message_with_agent_config(&s, &t, &h, None, &exec).await;
        rt.process_message_with_agent_config(&s, &t, &h, None,
            &crate::approval_scope::com_escopo(state.exec().await, "x", &s, &u)).await;
        let outro = state.exec_context_for(&s, None).await;
        rt.process_message_with_agent_config(&s, &t, &h, None, &outro).await;
        rt.process_message(&s, &t, &[]).await;
        rt.process_message_streaming_with_events(&s, &t, &h, tx,
            &state.exec_context_for(&s, None).await).await;
        self.process_message_impl(&s);
    "#;
    let achadas: Vec<bool> = chamadas(fonte).iter().map(|c| c.escopada).collect();
    assert_eq!(achadas, vec![true, true, false, false, false]);
}
