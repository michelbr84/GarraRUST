//! Guarda de fiação da fronteira de autorizacao (follow-up do #1666).
//!
//! Os testes comportamentais (`garraia-agents`, `runtime/tests/fronteira_wrappers.rs`)
//! provam o MECANISMO; esta guarda cobre a LIGACAO: nenhum ponto de entrada de
//! turno pode voltar a despachar sem contexto provado, e nenhuma construcao de
//! `ExecContext` sem teto de superficie (`default()`, `with_*` ou literal)
//! aparece em producao fora da lista declarada — cada entrada da lista carrega
//! a contagem e a justificativa da confianca que a autoriza.
//!
//! Busca textual e COMPLEMENTO, nao substituto: o comportamento e provado no
//! runtime com ferramentas simuladas, em ambiente isolado.

mod varredura_de_escopo;

use std::path::PathBuf;

use varredura_de_escopo::varrer;

/// (caminho relativo a `crates/`, codigo) de todo `.rs` de producao do
/// workspace.
///
/// O detector e o de `varredura_de_escopo` (o mesmo do
/// `approval_scope_coverage.rs`): comentarios e conteudo de literais apagados,
/// os `#[cfg(test)] mod x { .. }` inline apagados SEM esconder a producao que
/// vem depois deles, e o arquivo dos `#[cfg(test)] mod x;` fora da lista. A
/// primeira versao desta guarda cortava o fonte no primeiro `#[cfg(test)]` — o
/// que viesse depois de um modulo de teste no meio do arquivo ficava invisivel.
fn fontes_de_producao() -> Vec<(String, String)> {
    let crates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("diretorio crates");
    varrer(&crates_dir)
        .into_iter()
        // `tests/` de crate (testes de integracao) nao e producao.
        .filter(|(rel, _)| !rel.split('/').any(|parte| parte == "tests"))
        .map(|(rel, fonte)| (rel, fonte.codigo))
        .collect()
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

/// Construtores que devolvem um `ExecContext` SEM teto de superficie: o
/// `default()` e os `with_*`, que partem dele. Quem os usa em producao tem de
/// declarar a confianca na mesma cadeia (`.operador_local()`,
/// `.com_credencial_de_operador()`, `.sem_credencial_de_operador()`) ou passar
/// pelo construtor central — e entra na lista abaixo.
const CONSTRUTORES_IMPLICITOS: [&str; 4] = [
    "ExecContext::default()",
    "ExecContext::with_mode(",
    "ExecContext::with_working_dir(",
    "ExecContext::with_custom_profile(",
];

/// Quantos contextos implicitos o codigo monta: os construtores acima mais os
/// literais `ExecContext { .. }` (que tambem nascem sem teto de superficie).
/// `-> ExecContext {`, `impl ExecContext {`, `struct ExecContext {` e
/// `for ExecContext {` sao assinatura/definicao, nao construcao — com o tipo
/// solto ou qualificado (`-> crate::exec_context::ExecContext {`).
fn contextos_implicitos(codigo: &str) -> usize {
    let construtores: usize = CONSTRUTORES_IMPLICITOS
        .iter()
        .map(|padrao| codigo.matches(padrao).count())
        .sum();
    let literais = codigo
        .match_indices("ExecContext {")
        .filter(|(pos, _)| {
            let antes = sem_caminho(&codigo[..*pos]).trim_end();
            !["->", "impl", "struct", "for"]
                .iter()
                .any(|palavra| antes.ends_with(palavra))
        })
        .count();
    construtores + literais
}

/// Tira do fim de `s` o caminho que qualifica um tipo (`crate::exec_context::`),
/// para o que sobra ser o que vem ANTES do tipo.
fn sem_caminho(s: &str) -> &str {
    let mut resto = s;
    while let Some(sem_dois_pontos) = resto.strip_suffix("::") {
        resto = sem_dois_pontos.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    }
    resto
}

/// Uso declarado de contexto implicito em producao: arquivo, QUANTAS
/// construcoes e a confianca que as autoriza. A contagem e parte do espelho —
/// uma construcao nova num arquivo ja declarado tambem reprova. O padrao e:
/// quem monta contexto SEM prova de operador usa
/// `ExecContext::sem_prova_de_operador()` (fail-closed).
fn contextos_implicitos_declarados() -> Vec<(&'static str, usize, &'static str)> {
    vec![
        (
            "garraia-cli/src/chat.rs",
            1,
            "`exec_do_turno`: `with_working_dir(..).operador_local()` — o \
             operador da propria maquina, sem superficie remota",
        ),
        (
            "garraia-gateway/src/a2a.rs",
            2,
            "`exec_do_a2a`: as duas bases (`with_custom_profile` e `default()`) \
             saem sempre por `.com_credencial_de_operador()` quando o gate da \
             rota exigiu o bearer `api_key`, ou por \
             `.sem_credencial_de_operador()` quando o gateway esta sem chave",
        ),
        (
            "garraia-gateway/src/agente_mcp.rs",
            1,
            "`agent_oneshot`: `with_working_dir(..).com_credencial_de_operador()` \
             — a ponte MCP HTTP nao monta sem `api_key` (`decidir_montagem`)",
        ),
        (
            "garraia-gateway/src/state.rs",
            4,
            "`exec_context_base` monta a base (literal + `default()`, \
             `with_custom_profile`, `with_mode`); a camada da superficie e \
             sobrescrita INCONDICIONALMENTE pelo construtor central logo abaixo",
        ),
    ]
}

fn arquivos_com(ocorrencias: &[&str], fontes: &[(String, String)]) -> Vec<String> {
    let mut achados: Vec<String> = fontes
        .iter()
        .filter(|(rel, _)| rel != "garraia-agents/src/runtime.rs")
        .filter(|(_, codigo)| ocorrencias.iter().any(|padrao| codigo.contains(padrao)))
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

/// Nenhum contexto implicito novo em producao fora da lista declarada — nem em
/// arquivo novo, nem a mais num arquivo ja declarado.
#[test]
fn contexto_implicito_em_producao_e_so_o_declarado() {
    let achados: Vec<(String, usize)> = fontes_de_producao()
        .into_iter()
        .map(|(rel, codigo)| (rel, contextos_implicitos(&codigo)))
        .filter(|(_, n)| *n > 0)
        .collect();
    let mut declarados: Vec<(String, usize)> = contextos_implicitos_declarados()
        .into_iter()
        .map(|(arquivo, n, _)| (arquivo.to_string(), n))
        .collect();
    declarados.sort();
    assert_eq!(
        achados, declarados,
        "construcao de `ExecContext` sem teto de superficie (`default()`, \
         `with_*` ou literal) fora da lista declarada em \
         `contextos_implicitos_declarados()`, ou em quantidade diferente da \
         declarada. Caminho sem prova de operador usa \
         `ExecContext::sem_prova_de_operador()` (fail-closed); caminho com \
         confianca declara-a na mesma cadeia."
    );
}

/// O detector de literal distingue construcao de assinatura.
#[test]
fn literal_de_contexto_nao_confunde_assinatura() {
    assert_eq!(
        contextos_implicitos(
            "fn a() -> ExecContext { ExecContext { x: 1, ..Default::default() } }"
        ),
        1
    );
    assert_eq!(contextos_implicitos("impl ExecContext { }"), 0);
    assert_eq!(contextos_implicitos("pub struct ExecContext { }"), 0);
    assert_eq!(contextos_implicitos("impl Default for ExecContext { }"), 0);
    assert_eq!(
        contextos_implicitos("let e = ExecContext::with_working_dir(None);"),
        1
    );
    // Tipo qualificado: assinatura continua nao sendo construcao, e literal
    // continua sendo.
    assert_eq!(
        contextos_implicitos("fn a(e: E) -> crate::exec_context::ExecContext { e }"),
        0
    );
    assert_eq!(
        contextos_implicitos("impl Default for garraia_agents::ExecContext { }"),
        0
    );
    assert_eq!(
        contextos_implicitos("let e = crate::exec_context::ExecContext { x: 1 };"),
        1
    );
}

/// A guarda enxerga producao que vem DEPOIS de um modulo de teste inline —
/// o ponto cego do corte no primeiro `#[cfg(test)]`.
#[test]
fn producao_depois_de_modulo_de_teste_continua_visivel() {
    let fonte = "fn antes() {}\n\
                 #[cfg(test)]\nmod tests { fn t() { let _ = ExecContext::default(); } }\n\
                 fn depois() { let _ = ExecContext::default(); }\n";
    let codigo = varredura_de_escopo::Fonte::nova(fonte).codigo;
    assert_eq!(
        contextos_implicitos(&codigo),
        1,
        "so a construcao de producao conta; a do modulo de teste some"
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

    // A2A (#1985): a confianca sai do MESMO gate do middleware, e a
    // credencial de operador so e declarada dentro de `exec_do_a2a`.
    let a2a = varredura_de_escopo::Fonte::nova(&le("garraia-gateway/src/a2a.rs")).codigo;
    assert!(
        a2a.contains("ApiKeyGate::from_config(&state.config.gateway).is_enabled()"),
        "a2a.rs: a confianca do chamador nao sai do gate da rota"
    );
    assert_eq!(
        a2a.matches(".com_credencial_de_operador()").count(),
        1,
        "a2a.rs: credencial de operador declarada fora de `exec_do_a2a`"
    );
    assert_eq!(
        a2a.matches(".sem_credencial_de_operador()").count(),
        1,
        "a2a.rs: sem `api_key`, o A2A precisa cair no teto de leitura"
    );
    assert_eq!(
        a2a.matches("exec_do_a2a(").count(),
        3,
        "a2a.rs: os dois ramos do dispatch passam por `exec_do_a2a`"
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
