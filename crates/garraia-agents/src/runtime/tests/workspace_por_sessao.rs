use super::*;

/// O working_dir do #980 chega ao `ToolContext`.
#[test]
pub(super) fn o_working_dir_chega_ao_contexto_de_ferramenta() {
    use crate::exec_context::ExecContext;

    let exec = ExecContext::with_working_dir(Some("/tmp/projeto".to_string()));
    assert_eq!(exec.working_dir.as_deref(), Some("/tmp/projeto"));
    assert_eq!(exec.agent_mode, None, "working_dir nao pode implicar modo");
}

// ─── #1449: o workspace padrao escopado por sessao ─────────────────────

/// Sem workspace padrao ligado, nada muda: o `working_dir` da sessao passa
/// como esta, e a ausencia dele continua sendo ausencia (fail-closed da
/// #1244). E o caso da CLI, que nunca liga o workspace.
#[test]
pub(super) fn sem_workspace_padrao_o_working_dir_passa_como_esta() {
    use crate::exec_context::ExecContext;

    let rt = AgentRuntime::new();
    assert_eq!(
        rt.working_dir_efetivo(
            &ExecContext::with_working_dir(Some("/tmp/projeto".to_string())),
            "sessao-1"
        )
        .as_deref(),
        Some("/tmp/projeto")
    );
    assert_eq!(
        rt.working_dir_efetivo(&ExecContext::default(), "sessao-1"),
        None,
        "sem workspace padrao a sessao sem projeto nao ganha raiz nenhuma"
    );
}

/// **O achado R4 da #1449.** Com o workspace padrao ligado, duas sessoes
/// sem projeto recebem diretorios DIFERENTES — e nenhuma recebe o pai.
#[test]
pub(super) fn com_workspace_padrao_cada_sessao_ganha_o_seu_diretorio() {
    use crate::exec_context::ExecContext;

    let tmp = tempfile::tempdir().expect("tempdir");
    let raiz = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let mut rt = AgentRuntime::new();
    rt.set_workspace_padrao(Some(crate::tools::SessionWorkspace::nova(raiz.clone())));

    let a = rt
        .working_dir_efetivo(&ExecContext::default(), "sessao-A")
        .expect("A");
    let b = rt
        .working_dir_efetivo(&ExecContext::default(), "sessao-B")
        .expect("B");

    assert_ne!(a, b, "duas sessoes no mesmo diretorio (#1449)");
    for dir in [&a, &b] {
        let p = std::path::Path::new(dir);
        assert!(p.is_dir(), "{dir} nao foi criado");
        assert_eq!(p.parent(), Some(raiz.as_path()));
        assert_ne!(p, raiz.as_path(), "a sessao recebeu o PAI como raiz");
    }
}

/// O `working_dir` declarado pela sessao continua vencendo o workspace
/// padrao: uma sessao com projeto nao muda de lugar por causa da #1449.
#[test]
pub(super) fn o_working_dir_declarado_vence_o_workspace_padrao() {
    use crate::exec_context::ExecContext;

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut rt = AgentRuntime::new();
    rt.set_workspace_padrao(Some(crate::tools::SessionWorkspace::nova(
        tmp.path().to_path_buf(),
    )));

    assert_eq!(
        rt.working_dir_efetivo(
            &ExecContext::with_working_dir(Some("/tmp/projeto".to_string())),
            "sessao-1"
        )
        .as_deref(),
        Some("/tmp/projeto")
    );
    // E um `working_dir` so de espaco conta como ausente, como no jail.
    assert_ne!(
        rt.working_dir_efetivo(
            &ExecContext::with_working_dir(Some("   ".to_string())),
            "sessao-1"
        )
        .as_deref(),
        Some("   ")
    );
}

/// O `ToolContext` que o runtime monta carrega o diretorio da sessao.
#[test]
pub(super) fn o_contexto_de_ferramenta_carrega_o_diretorio_da_sessao() {
    use crate::exec_context::ExecContext;

    let tmp = tempfile::tempdir().expect("tempdir");
    let raiz = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let mut rt = AgentRuntime::new();
    rt.set_workspace_padrao(Some(crate::tools::SessionWorkspace::nova(raiz.clone())));

    let ctx = rt.contexto_de_ferramenta(
        &ExecContext::default(),
        "sessao-1",
        Some("u1"),
        crate::tools::approval::ToolApproval::None,
        false,
    );
    assert_eq!(ctx.session_id, "sessao-1");
    assert_eq!(ctx.user_id.as_deref(), Some("u1"));
    let dir = std::path::PathBuf::from(ctx.working_dir.expect("working_dir"));
    assert_eq!(dir.parent(), Some(raiz.as_path()));
}

/// **A fiacao.** O escopo por sessao so vale se TODO laco de tool-call
/// passar por `contexto_de_ferramenta`. Sao quatro lacos neste arquivo, e
/// um deles montando o `ToolContext` a mao reabriria a #1449 em silencio —
/// nenhum dos testes acima ficaria vermelho. Varredura do proprio fonte,
/// na disciplina dos guards de `spinner.rs` e `detect.rs`.
#[test]
pub(super) fn nenhum_tool_context_montado_a_mao_neste_runtime() {
    // O arquivo inteiro e producao: os testes moram em `runtime/tests/`, e
    // e por isso que montar `ToolContext` a mao AQUI (tool por tool) nao
    // entra na conta.
    let producao = include_str!("../../runtime.rs");

    let literais: Vec<usize> = producao
        .lines()
        .enumerate()
        .filter(|(_, l)| parece_construtor_de_tool_context(l))
        .map(|(n, _)| n + 1)
        .collect();

    assert_eq!(
        literais.len(),
        1,
        "ha {} literais `ToolContext {{` no codigo de producao deste arquivo (linhas {:?}), e \
         tem de haver exatamente 1 — o de `contexto_de_ferramenta`. Os quatro lacos de \
         tool-call passam por ele; um montado a mao reabriria a #1449 (workspace padrao \
         compartilhado entre sessoes) sem deixar nenhum outro teste vermelho",
        literais.len(),
        literais
    );
    assert!(
        producao.contains("working_dir: self.working_dir_efetivo(exec, session_id)"),
        "o construtor deixou de escopar o workspace padrao por sessao (#1449)"
    );
}

/// Uma linha de fonte monta um `ToolContext`? (#1464)
///
/// Substring, e nao igualdade da linha inteira: a forma anterior
/// (`t == "ToolContext {"`) deixava passar `let c = ToolContext { .. };`
/// numa linha so, `garraia_agents::ToolContext {` com caminho e
/// `ToolContext { ..base }` — exatamente as formas que um contribuidor
/// escreveria ao reabrir a #1449 sem querer. O que nao monta nada fica
/// fora: comentario, assinatura que devolve o tipo, `impl`/`struct`.
pub(super) fn parece_construtor_de_tool_context(linha: &str) -> bool {
    let t = linha.trim();
    if t.starts_with("//") || t.starts_with("impl ") || t.contains("struct ToolContext") {
        return false;
    }
    // Assinatura que devolve o tipo — `) -> ToolContext {`, com ou sem
    // caminho de modulo na frente. O que vem depois da seta e SO o tipo.
    if let Some((_, depois)) = t.split_once("->") {
        let tipo = depois.trim().trim_end_matches('{').trim();
        if tipo.rsplit("::").next() == Some("ToolContext") {
            return false;
        }
    }
    t.contains("ToolContext {")
}

/// O filtro do guard acima (#1464). Casa o literal em qualquer forma que
/// o rustfmt — ou um contribuidor — produza: numa linha so, com caminho de
/// modulo, com `..base`. E NAO casa o que nao monta nada: comentario,
/// assinatura que devolve o tipo, `impl`/`struct` do tipo.
#[test]
pub(super) fn o_filtro_do_guard_casa_qualquer_forma_do_construtor() {
    for monta in [
        "ToolContext {",
        "crate::tools::ToolContext {",
        "let context = ToolContext { session_id, user_id, working_dir: None };",
        "garraia_agents::ToolContext { ..base }",
        "    Some(ToolContext { ..ctx })",
    ] {
        assert!(parece_construtor_de_tool_context(monta), "{monta:?}");
    }
    for nao_monta in [
        "// ToolContext { ... }",
        "/// monta um `ToolContext { .. }` por sessao",
        "//! ToolContext {",
        "ToolContext::new(",
        "    ) -> ToolContext {",
        "    ) -> crate::tools::ToolContext {",
        "fn contexto_de_ferramenta(&self) -> ToolContext {",
        "pub fn ctx() -> garraia_agents::ToolContext {",
        "impl ToolContext {",
        "pub struct ToolContext {",
    ] {
        assert!(
            !parece_construtor_de_tool_context(nao_monta),
            "{nao_monta:?}"
        );
    }
}
