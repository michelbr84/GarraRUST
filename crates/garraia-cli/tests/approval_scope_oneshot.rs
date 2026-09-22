//! Quem chama o runtime na CLI decidiu quem aprova (#1343 S4).
//!
//! `garraia chat` e o unico caminho da CLI com um humano digitando turno a
//! turno, e e o unico que opta pelo escopo de aprovacao (via
//! `exec_do_turno`). `garraia ask` e o `garra_agent` do `garraia mcp-server`
//! sao one-shot — sessao nova a cada chamada — e quem chama e muitas vezes
//! outro agente: um "ok" dele nunca pode aprovar uma ferramenta perigosa.
//! Sem escopo, a aprovacao so viria do historico, que ali e vazio, e a pausa
//! e terminal.
//!
//! O teste varre `src/` com o MESMO detector do guarda do gateway
//! (`crates/garraia-gateway/tests/varredura_de_escopo`) e decide POR
//! CHAMADA: nos arquivos de [`ESCOPADOS`], toda chamada tem de passar um
//! contexto montado por `exec_do_turno(..)` (inline ou num `let` da mesma
//! `fn`); nos de [`SEM_ESCOPO`], cada chamada fica presa por `fn` e ponto de
//! entrada; em qualquer outro arquivo, chamar o runtime reprova.

#[path = "../../garraia-gateway/tests/varredura_de_escopo/mod.rs"]
mod varredura_de_escopo;

use std::collections::BTreeMap;
use std::path::Path;

use varredura_de_escopo::{Construtor, Fonte, chamadas, varrer};

const CONSTRUTOR: Construtor = Construtor {
    nome: "exec_do_turno",
    caminhos: &["exec_do_turno"],
};

/// Arquivos que chamam o runtime COM escopo: toda chamada deles passa pelo
/// `exec_do_turno`.
const ESCOPADOS: &[&str] = &["chat.rs"];

/// Chamadas SEM escopo, de proposito: `(arquivo, fn, entrada, quantas,
/// motivo)`.
const SEM_ESCOPO: &[(&str, &str, &str, usize, &str)] = &[
    (
        "ask.rs",
        "ask_oneshot",
        "process_message_streaming",
        1,
        "one-shot (`garraia ask`): sessao nova por chamada, frequentemente dirigida por script ou agente",
    ),
    (
        "mcp_agent.rs",
        "agent_oneshot",
        "process_message_streaming_with_events",
        1,
        "`garra_agent` do `garraia mcp-server`: quem chama e outro agente, e o bash dele nem pede confirmacao",
    ),
];

fn src() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Todo chamador do runtime na CLI esta decidido, chamada a chamada.
#[test]
fn todo_chamador_do_runtime_na_cli_esta_decidido() {
    let arquivos = varrer(&src());
    assert!(
        arquivos.iter().any(|(a, _)| a == "chat.rs"),
        "a varredura nao achou src/chat.rs; ela quebrou?"
    );
    let mut erros = Vec::new();
    let mut escopadas = 0usize;
    let mut sem_escopo: BTreeMap<(String, String, String), Vec<usize>> = BTreeMap::new();
    for (arq, fonte) in &arquivos {
        let (achadas, desconhecidas) = chamadas(fonte, &CONSTRUTOR);
        erros.extend(desconhecidas.into_iter().map(|e| format!("{arq}: {e}")));
        let escopado = ESCOPADOS.contains(&arq.as_str());
        for c in achadas {
            if escopado {
                if c.escopada() {
                    escopadas += 1;
                } else {
                    erros.push(format!(
                        "{arq}:{} (`{}`): `{}` sem `exec_do_turno(..)` no contexto — o turno \
                         humano do `garraia chat` tem de montar o ExecContext por ele (#1343)",
                        c.linha, c.funcao, c.entrada
                    ));
                }
            } else if c.escopada() {
                erros.push(format!(
                    "{arq}:{} (`{}`): `{}` opta pelo escopo de aprovacao fora do `garraia chat`",
                    c.linha, c.funcao, c.entrada
                ));
            } else {
                sem_escopo
                    .entry((arq.clone(), c.funcao.clone(), c.entrada.clone()))
                    .or_default()
                    .push(c.linha);
            }
        }
    }
    assert!(
        escopadas >= 1,
        "o `garraia chat` nao chama mais o runtime pelo `exec_do_turno`? {erros:?}"
    );
    for ((arq, funcao, entrada), linhas) in &sem_escopo {
        match SEM_ESCOPO
            .iter()
            .find(|(a, f, e, _, _)| a == arq && f == funcao && e == entrada)
        {
            None => erros.push(format!(
                "{arq}: `{entrada}` sem decisao de escopo de aprovacao em `{funcao}` (linhas \
                 {linhas:?}). Humano turno a turno: use `exec_do_turno`; one-shot/agente: \
                 acrescente (arquivo, fn, entrada) a SEM_ESCOPO."
            )),
            Some((_, _, _, n, _)) if *n != linhas.len() => erros.push(format!(
                "{arq}: {} chamada(s) `{entrada}` em `{funcao}` (linhas {linhas:?}), a lista diz {n}.",
                linhas.len()
            )),
            Some(_) => {}
        }
    }
    for (arq, funcao, entrada, _, _) in SEM_ESCOPO {
        let chave = (arq.to_string(), funcao.to_string(), entrada.to_string());
        if !sem_escopo.contains_key(&chave) {
            erros.push(format!(
                "{arq}: SEM_ESCOPO lista `{entrada}` em `{funcao}`, mas ela nao chama mais o \
                 runtime ali; tire da lista."
            ));
        }
    }
    assert!(erros.is_empty(), "{}", erros.join("\n"));
}

#[test]
fn caminhos_one_shot_nao_optam_pelo_escopo() {
    let arquivos = varrer(&src());
    for (arq, _, _, _, motivo) in SEM_ESCOPO {
        let (_, fonte) = arquivos
            .iter()
            .find(|(a, _)| a == arq)
            .unwrap_or_else(|| panic!("{arq} sumiu de src/"));
        for proibido in ["approval_scope", "ApprovalScope", "exec_do_turno"] {
            assert!(
                !fonte.codigo.contains(proibido),
                "{arq} nao pode optar pelo escopo de aprovacao ({motivo}), mas contem `{proibido}`"
            );
        }
    }
}

#[test]
fn o_chat_opta_pelo_escopo() {
    let arquivos = varrer(&src());
    let (_, chat) = arquivos
        .iter()
        .find(|(a, _)| a == "chat.rs")
        .expect("src/chat.rs");
    assert!(
        chat.codigo
            .contains("ApprovalScope::new(CANAL_CLI, session_id, REMETENTE_CLI)"),
        "o escopo do CLI e (cli, sessao, local-tty)"
    );
}

/// #1343 A4: a decisao e por chamada, nao por arquivo. Um arquivo escopado
/// que tem `exec_do_turno(` em algum lugar, mas cuja chamada nova monta o
/// contexto na mao, reprova.
#[test]
fn o_detector_da_cli_decide_por_chamada() {
    let fonte = r#"
        fn exec_do_turno(cwd: &str, s: &str) -> ExecContext { todo!() }
        pub async fn run_chat() {
            let exec = exec_do_turno(&cwd, &sessao);
            let call = runtime.process_message_streaming_with_events(
                &sessao, &input, &h, tx, None, None, None, None, None, None, &exec);
        }
        async fn turno_novo() {
            let exec = ExecContext::with_working_dir(Some(cwd.clone()));
            runtime.process_message_streaming_with_events(
                &sessao, &input, &h, tx, None, None, None, None, None, None, &exec).await;
        }
        async fn herdaria_de_outra_fn() {
            runtime.process_message_streaming_with_events(
                &sessao, &input, &h, tx, None, None, None, None, None, None, &exec).await;
        }
    "#;
    let f = Fonte::nova(fonte);
    let (achadas, erros) = chamadas(&f, &CONSTRUTOR);
    assert!(erros.is_empty(), "{erros:?}");
    let decisao: Vec<(&str, bool)> = achadas
        .iter()
        .map(|c| (c.funcao.as_str(), c.escopada()))
        .collect();
    assert_eq!(
        decisao,
        vec![
            ("run_chat", true),
            ("turno_novo", false),
            ("herdaria_de_outra_fn", false)
        ]
    );
}
