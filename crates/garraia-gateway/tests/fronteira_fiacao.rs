//! Guarda de fiação da fronteira de autorizacao (follow-up do #1666).
//!
//! Os testes comportamentais (`garraia-agents`, `runtime/tests/fronteira_wrappers.rs`)
//! provam o MECANISMO; esta guarda cobre a LIGACAO: nenhum ponto de entrada de
//! turno pode voltar a despachar sem contexto provado, e nenhum
//! `ExecContext::default()` novo aparece em producao fora da lista declarada —
//! cada entrada da lista carrega a justificativa da confianca que a autoriza.
//!
//! Busca textual e COMPLEMENTO, nao substituto: o comportamento e provado no
//! runtime com ferramentas simuladas, em ambiente isolado.

use std::path::{Path, PathBuf};

/// Metade de producao: tudo antes do primeiro `#[cfg(test)]`.
fn producao(fonte: &str) -> &str {
    fonte.split("#[cfg(test)]").next().unwrap_or(fonte)
}

/// Codigo sem comentarios de linha — comentario citando um padrao nao e uso.
fn sem_comentarios(txt: &str) -> String {
    txt.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// (caminho relativo a `crates/`, fonte) de todo `.rs` de producao do workspace.
fn fontes_de_producao() -> Vec<(String, String)> {
    let crates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("diretorio crates");
    let mut out = Vec::new();
    varrer(&crates_dir, &crates_dir, &mut out);
    out.sort();
    out
}

fn varrer(raiz: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for entrada in std::fs::read_dir(dir).expect("lista diretorio") {
        let entrada = entrada.expect("entrada");
        let caminho = entrada.path();
        if caminho.is_dir() {
            // `tests/` de crate e modulos de teste nao sao producao.
            if caminho.file_name().and_then(|n| n.to_str()) != Some("tests") {
                varrer(raiz, &caminho, out);
            }
        } else if caminho.extension().and_then(|e| e.to_str()) == Some("rs") {
            // `tests.rs` e modulo de teste declarado pelo pai
            // (`#[cfg(test)] mod tests;`) — o gate esta FORA do arquivo.
            if caminho.file_name().and_then(|n| n.to_str()) == Some("tests.rs") {
                continue;
            }
            let rel = caminho
                .strip_prefix(raiz)
                .expect("prefixo")
                .to_string_lossy()
                .to_string();
            let fonte = std::fs::read_to_string(&caminho).expect("le fonte");
            out.push((rel, fonte));
        }
    }
}

/// Wrappers de conveniencia do `AgentRuntime` (contexto implicito). Chamadas
/// com contexto explicito (`*_with_exec`, `*_with_agent_config`,
/// `*_with_events`) nao entram aqui por desenho.
const WRAPPERS: [&str; 5] = [
    ".process_message(",
    ".process_message_with_context(",
    ".process_heartbeat(",
    ".process_message_streaming(",
    ".process_message_streaming_with_context(",
];

/// Uso declarado de wrapper de conveniencia em producao, com a confianca que o
/// autoriza. Novo call-site so entra aqui com revisao — e o `runtime.rs` (as
/// proprias definicoes) esta fora da varredura.
fn wrappers_declarados() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "garraia-ask/src/ask.rs",
            "ask nao registra NENHUMA tool (invariante GAR-579, guarda propria); \
             o wrapper fail-closed nao tem o que conceder",
        ),
        (
            "garraia-channels/src/discord/handler.rs",
            "`self.process_message(` e o metodo PROPRIO do DiscordHandler \
             (conversor de mensagem do Discord), nao o do AgentRuntime",
        ),
    ]
}

/// Uso declarado de `ExecContext::default()` em producao, com a confianca que o
/// autoriza. O padrao e: quem monta contexto SEM prova de operador usa
/// `ExecContext::sem_prova_de_operador()` (fail-closed).
fn contextos_default_declarados() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "garraia-gateway/src/a2a.rs",
            "rota `/a2a/*` gateada pelo bearer `api_key` (credencial do \
             administrador); os usos levam `.com_credencial_de_operador()` na \
             mesma cadeia",
        ),
        (
            "garraia-gateway/src/state.rs",
            "`exec_context_base` monta a base; a camada da superficie e \
             sobrescrita INCONDICIONALMENTE pelo construtor central logo abaixo",
        ),
    ]
}

fn arquivos_com(ocorrencias: &[&str], fontes: &[(String, String)]) -> Vec<String> {
    let mut achados: Vec<String> = fontes
        .iter()
        .filter(|(rel, _)| rel != "garraia-agents/src/runtime.rs")
        .filter(|(_, fonte)| {
            let codigo = sem_comentarios(producao(fonte));
            ocorrencias.iter().any(|padrao| codigo.contains(padrao))
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    achados.sort();
    achados.dedup();
    achados
}

/// Nenhum ponto de entrada de turno volta a despachar sem contexto: todo uso
/// de wrapper de conveniencia em producao e da lista declarada — nem mais, nem
/// menos (a lista e um espelho, como `TOOLS_SANDBOXAVEIS`).
#[test]
fn wrappers_de_conveniencia_sao_so_os_declarados() {
    let fontes = fontes_de_producao();
    let mut declarados: Vec<String> = wrappers_declarados()
        .into_iter()
        .map(|(arquivo, _)| arquivo.to_string())
        .collect();
    declarados.sort();
    assert_eq!(
        arquivos_com(&WRAPPERS, &fontes),
        declarados,
        "call-site de wrapper de conveniencia fora da lista declarada em \
         `wrappers_declarados()` (ou declarado sem mais existir). Novo caminho \
         de turno usa `*_with_exec` com o contexto do construtor central."
    );
}

/// Nenhum `ExecContext::default()` novo em producao fora da lista declarada.
#[test]
fn contexto_default_em_producao_e_so_o_declarado() {
    let fontes = fontes_de_producao();
    let mut declarados: Vec<String> = contextos_default_declarados()
        .into_iter()
        .map(|(arquivo, _)| arquivo.to_string())
        .collect();
    declarados.sort();
    assert_eq!(
        arquivos_com(&["ExecContext::default()"], &fontes),
        declarados,
        "uso de `ExecContext::default()` em producao fora da lista declarada em \
         `contextos_default_declarados()`. Caminho sem prova de operador usa \
         `ExecContext::sem_prova_de_operador()` (fail-closed)."
    );
}

/// A ligacao: construtor central sobrescreve a camada da superficie SEMPRE, e
/// os pontos de turno que antes despachavam sem contexto passam por ele.
#[test]
fn construtor_central_e_caminhos_de_turno_estao_ligados() {
    let crates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let le = |rel: &str| {
        std::fs::read_to_string(crates_dir.join(rel)).unwrap_or_else(|_| panic!("le {rel}"))
    };

    let estado = le("garraia-gateway/src/state.rs");
    assert!(
        estado.contains(
            "exec.teto_da_superficie = crate::channels_view::teto_da_superficie(operador)"
        ),
        "state.rs: o construtor central nao preenche o teto da superficie"
    );
    assert!(
        !estado.contains("if exec.teto.is_none()"),
        "state.rs: voltou o salto condicional — a camada e incondicional"
    );

    let mensagens = le("garraia-gateway/src/rest_v1/messages.rs");
    assert!(
        mensagens.contains("process_message_with_exec(")
            && mensagens.contains("exec_context_for_msg("),
        "messages.rs: o bot do workspace nao passa pelo construtor central"
    );

    let servidor = le("garraia-gateway/src/server.rs");
    assert!(
        servidor.contains("process_heartbeat_with_exec(")
            && servidor.contains("exec_context_for_msg("),
        "server.rs: a tarefa agendada nao passa pelo construtor central"
    );

    let runtime = le("garraia-agents/src/runtime.rs");
    assert_eq!(
        runtime
            .matches("&ExecContext::sem_prova_de_operador()")
            .count(),
        3,
        "runtime.rs: os 3 wrappers sem contexto constroem o contexto fail-closed"
    );

    let modos = le("garraia-agents/src/modes.rs");
    assert!(
        modos
            .matches(".com_teto_de_superficie(exec.teto_da_superficie.clone())")
            .count()
            >= 2,
        "modes.rs: `from_exec`/`para_o_turno` precisam compor as DUAS camadas de teto"
    );
}
