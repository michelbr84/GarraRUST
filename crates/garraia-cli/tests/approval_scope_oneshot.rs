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
//! O teste varre `src/`: todo arquivo que chama `process_message*` precisa
//! estar numa das duas listas abaixo, e as listas sao verificadas nos dois
//! sentidos.

use std::path::Path;

/// Arquivos que chamam o runtime COM escopo, e o marcador que o prova.
const ESCOPADOS: &[(&str, &str)] = &[("chat.rs", "exec_do_turno(")];

/// Arquivos que chamam o runtime SEM escopo, de proposito.
const SEM_ESCOPO: &[(&str, &str)] = &[
    (
        "ask.rs",
        "one-shot (`garraia ask`): sessao nova por chamada, frequentemente dirigida por script ou agente",
    ),
    (
        "mcp_agent.rs",
        "`garra_agent` do `garraia mcp-server`: quem chama e outro agente, e o bash dele nem pede confirmacao",
    ),
];

fn fonte(nome: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(nome);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("ler {}: {e}", p.display()))
}

/// O codigo de producao: corta no primeiro `#[cfg(test)]` seguido de `mod`.
fn producao(fonte: &str) -> String {
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
    linhas[..fim].join("\n")
}

fn chama_o_runtime(prod: &str) -> bool {
    prod.contains(".process_message(") || prod.contains(".process_message_")
}

#[test]
fn caminhos_one_shot_nao_optam_pelo_escopo() {
    for (arq, motivo) in SEM_ESCOPO {
        let prod = producao(&fonte(arq));
        assert!(
            chama_o_runtime(&prod),
            "{arq} saiu da lista sem escopo? ele nao chama mais o runtime"
        );
        for proibido in ["approval_scope", "ApprovalScope", "exec_do_turno("] {
            assert!(
                !prod.contains(proibido),
                "{arq} nao pode optar pelo escopo de aprovacao ({motivo}), mas contem `{proibido}`"
            );
        }
    }
}

#[test]
fn o_chat_opta_pelo_escopo() {
    for (arq, marcador) in ESCOPADOS {
        let prod = producao(&fonte(arq));
        assert!(chama_o_runtime(&prod), "{arq} nao chama mais o runtime?");
        assert!(
            prod.contains(marcador),
            "{arq}: o turno tem de montar o contexto por `{marcador}` (#1343)"
        );
    }
    let chat = producao(&fonte("chat.rs"));
    assert!(
        chat.contains("ApprovalScope::new(CANAL_CLI, session_id, REMETENTE_CLI)"),
        "o escopo do CLI e (cli, sessao, local-tty)"
    );
}

/// Arquivo novo da CLI que chame o runtime precisa entrar numa das listas.
#[test]
fn todo_chamador_do_runtime_na_cli_esta_decidido() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pendentes = Vec::new();
    let mut pilha = vec![dir.clone()];
    while let Some(d) = pilha.pop() {
        for e in std::fs::read_dir(&d).expect("ler src").flatten() {
            let p = e.path();
            if p.is_dir() {
                pilha.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let prod = producao(&std::fs::read_to_string(&p).expect("ler"));
            if !chama_o_runtime(&prod) {
                continue;
            }
            let rel = p
                .strip_prefix(&dir)
                .expect("em src")
                .to_string_lossy()
                .replace('\\', "/");
            let decidido = ESCOPADOS.iter().any(|(a, _)| *a == rel)
                || SEM_ESCOPO.iter().any(|(a, _)| *a == rel);
            if !decidido {
                pendentes.push(rel);
            }
        }
    }
    assert!(
        pendentes.is_empty(),
        "chamam o runtime sem decisao de escopo de aprovacao: {pendentes:?}. \
         Humano turno a turno: use `exec_do_turno`; one-shot/agente: acrescente a SEM_ESCOPO."
    );
}
