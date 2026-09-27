use super::*;

/// #1266 (P0): a `repo_search` **como o runtime a registra** nao pode
/// executar comando nenhum quando o modelo escolhe uma query que e flag do
/// ripgrep.
///
/// O registro aqui e o mesmo do boot (`bootstrap::mod` e `cli::chat`:
/// `register_tool(Box::new(RepoSearchTool::new(..)))`) e a chamada passa
/// pelo `find_tool`, que e por onde o loop de tool call do LLM acha a tool
/// — nao por uma copia montada no teste.
///
/// A carga e um `.txt` comum, do tipo que a `file_write` pode gravar
/// dentro do jail: com `--pre=/bin/sh` o proprio ripgrep o executa como
/// script para cada arquivo varrido, e o marcador aparece. Tirar o `--` do
/// `rg_args` deixa este teste vermelho — foi assim que a falha foi
/// reproduzida antes da correcao.
///
/// Numa maquina sem `rg` o teste exercita o fallback `grep`, que tambem
/// nao pode executar nada; so a demonstracao da mutacao depende do `rg`.
#[cfg(not(windows))]
#[tokio::test]
pub(super) async fn repo_search_registrada_nao_executa_pre_do_ripgrep() {
    use crate::tools::RepoSearchTool;

    let dir = std::env::temp_dir().join(format!(
        "garra-repo-search-1266-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    let marcador = dir.join("EXECUTADO");
    std::fs::write(
        dir.join("carga.txt"),
        format!("#!/bin/sh\ntouch {}\n", marcador.display()),
    )
    .expect("write");

    let rt = Arc::new(AgentRuntime::new());
    rt.register_tool(Box::new(RepoSearchTool::new(Some(10), None)));
    let tool = rt.find_tool("repo_search").expect("tool registrada");

    let ctx = ToolContext {
        session_id: "test-1266".into(),
        user_id: None,
        is_heartbeat: false,
        approval: crate::tools::approval::ToolApproval::None,
        working_dir: Some(dir.to_string_lossy().into_owned()),
        project_id: None,
    };

    let saida = tool
        .execute(&ctx, serde_json::json!({"query": "--pre=/bin/sh"}))
        .await
        .expect("a tool devolve saida, nao erro de runtime");

    let executou = marcador.exists();
    let conteudo = saida.content.clone();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        !executou,
        "a query do modelo virou flag e o ripgrep executou a carga: {conteudo}"
    );
    // Busca literal por um texto que nao esta em lugar nenhum: sem match.
    assert!(
        conteudo.contains("No matches found") || saida.is_error,
        "esperava busca literal sem match ou erro controlado, veio: {conteudo}"
    );
}

/// Repositório git com um arquivo rastreado **modificado**. O `--output`
/// cria o arquivo mesmo com diff vazio, e o repo plantado mantém a prova
/// de efeito em pé agora que a tool honra o `working_dir` da sessão
/// (#1258): sem ele o git roda num diretório que não é repositório e
/// morre antes de qualquer efeito observável. Usado pelos dois testes de
/// regressão da #1269.
#[cfg(not(windows))]
pub(super) fn repo_git_com_arquivo_modificado(repo: &std::path::Path) {
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git disponível no ambiente de teste");
        assert!(status.success(), "git {args:?} falhou no setup");
    };
    run(&["init", "-q"]);
    std::fs::write(repo.join("carga.txt"), "conteudo inicial\n").expect("write");
    run(&["add", "carga.txt"]);
    run(&[
        "-c",
        "user.name=garra-test",
        "-c",
        "user.email=garra@test.local",
        "commit",
        "-q",
        "-m",
        "init",
    ]);
    // Mudança não commitada: é ela que o `git diff` mostra.
    std::fs::write(repo.join("carga.txt"), "conteudo inicial\nmudanca\n").expect("write");
}

/// #1269 (P1): a `git_diff` **como o runtime a registra** não pode deixar
/// o `file_path` do modelo virar flag do git.
///
/// No git, quando a mesma flag aparece mais de uma vez, a última vence: o
/// argv vulnerável era `["diff", "--no-ext-diff", "-U3", "--ext-diff"]`,
/// que com um `.git/config` plantado (`diff.external`) executa comando
/// externo escolhido pelo prompt — reproduzido manualmente nesta rodada.
/// O registro aqui é o mesmo do boot (`cli::chat`:
/// `register_tool(Box::new(GitDiffTool::new(None, None)))`) e a chamada
/// passa pelo `find_tool`, que é por onde o loop de tool call do LLM acha
/// a tool — não por uma cópia montada no teste.
///
/// A carga automatizada é `--output=<caminho>`, a mesma classe de opção
/// com efeito observável e determinístico: o git cria o arquivo mesmo
/// quando o diff é vazio, então o teste funciona em clone limpo. Tirar o
/// `--` do `git_diff_args` deixa este teste vermelho.
///
/// E o controle positivo (chamada benigna sem `STDERR:`) guarda contra o
/// mascaramento que esta rodada revelou: com o `-U 3` separado que o git
/// recusa (`bad revision '3'`), o git morria antes de qualquer efeito
/// observável — nem o argv injetado chegava a agir, e a prova do
/// terminador não conseguia falhar. O `-U{context}` colado em
/// `git_diff_args` é o que mantém a prova viva.
///
/// Premissa do ambiente (atualizada pela #1258): o git roda no
/// `working_dir` da sessão, então o repositório é **plantado no tempdir**
/// da sessão em vez de emprestado do CWD do processo de teste. Antes da
/// #1258 este teste passava porque a tool ignorava o `working_dir` e caía
/// no checkout do próprio GarraRUST — verde por acidente, e por um acidente
/// que dependia de onde a suite rodava.
#[cfg(not(windows))]
#[tokio::test]
pub(super) async fn git_diff_registrada_nao_reabre_ext_diff_pelo_file_path() {
    use crate::tools::GitDiffTool;

    let dir = std::env::temp_dir().join(format!(
        "garra-git-diff-1269-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    // #1258: o git desta chamada roda aqui, então é aqui que o
    // repositório (e o `carga.txt` do controle positivo) tem de existir.
    repo_git_com_arquivo_modificado(&dir);
    let marcador = dir.join("EXT-WRITO");

    let rt = Arc::new(AgentRuntime::new());
    rt.register_tool(Box::new(GitDiffTool::new(None, None)));
    let tool = rt.find_tool("git_diff").expect("tool registrada");

    let ctx = ToolContext {
        session_id: "test-1269".into(),
        user_id: None,
        is_heartbeat: false,
        approval: crate::tools::approval::ToolApproval::None,
        working_dir: Some(dir.to_string_lossy().into_owned()),
        project_id: None,
    };

    let saida = tool
        .execute(
            &ctx,
            serde_json::json!({
                "operation": "diff",
                "file_path": format!("--output={}", marcador.display()),
            }),
        )
        .await
        .expect("a tool devolve saida, nao erro de runtime");

    // Controle positivo: uma chamada benigna com o mesmo registro prova
    // que o git roda de verdade — nenhum STDERR no caminho de saída. É o
    // que impede o teste de ficar verde por acidente (ver doc comment).
    let benigna = tool
        .execute(
            &ctx,
            serde_json::json!({
                "operation": "diff",
                "file_path": "carga.txt",
            }),
        )
        .await
        .expect("chamada benigna devolve saida, nao erro de runtime");

    let executou = marcador.exists();
    let conteudo = saida.content.clone();
    let conteudo_benigno = benigna.content.clone();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        !executou,
        "o file_path do modelo virou flag e o git escreveu a saida do diff onde ele escolheu: {conteudo}"
    );
    // Pathspec "--output=..." casa com nada: diff vazio e controle.
    assert!(
        !saida.is_error,
        "esperava diff vazio bem-comportado, veio erro: {conteudo}"
    );
    assert!(
        !conteudo_benigno.contains("STDERR:"),
        "a chamada benigna revelou o git quebrado no argv do diff: {conteudo_benigno}"
    );
}

/// #1269 (segundo achado): `from_commit` começando com `-` põe o token
/// `{from}..{to}` inteiro em posição de opção — `--output=<caminho>`
/// escreve o diff onde o modelo escolher. A validação fail-closed recusa
/// antes de subir o git.
///
/// Remover a checagem de `starts_with('-')` do `git_diff_args` deixa este
/// teste vermelho: o git executa e o arquivo do modelo aparece.
#[cfg(not(windows))]
#[tokio::test]
pub(super) async fn git_diff_registrada_nao_deixa_range_do_modelo_virar_flag() {
    use crate::tools::GitDiffTool;

    let dir = std::env::temp_dir().join(format!(
        "garra-git-range-1269-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    repo_git_com_arquivo_modificado(&dir);

    // Sem correção: token = "--output={dir}/..marker-escrito" — o git
    // escreve a saída do diff nesse caminho, escolhido pelo modelo.
    let marcador = dir.join("..marker-escrito");

    let rt = Arc::new(AgentRuntime::new());
    rt.register_tool(Box::new(GitDiffTool::new(None, None)));
    let tool = rt.find_tool("git_diff").expect("tool registrada");

    let ctx = ToolContext {
        session_id: "test-1269-range".into(),
        user_id: None,
        is_heartbeat: false,
        approval: crate::tools::approval::ToolApproval::None,
        working_dir: Some(dir.to_string_lossy().into_owned()),
        project_id: None,
    };

    let saida = tool
        .execute(
            &ctx,
            serde_json::json!({
                "operation": "diff",
                "from_commit": format!("--output={}/", dir.display()),
                "to_commit": "marker-escrito",
            }),
        )
        .await
        .expect("a tool devolve saida, nao erro de runtime");

    let escreveu = marcador.exists();
    let conteudo = saida.content.clone();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        !escreveu,
        "o from_commit do modelo virou opção e o git escreveu o diff onde ele escolheu: {conteudo}"
    );
    assert!(
        saida.is_error,
        "esperava recusa controlada da revisão, veio: {conteudo}"
    );
}

/// Test that AgentRuntime can be created with an empty/default config without crashing.
/// This test verifies the "empty config" scenario is handled safely.
#[test]
pub(super) fn build_agent_runtime_empty_config_no_crash() {
    // Create a runtime with default/empty configuration
    let runtime = AgentRuntime::new();

    // Verify basic state is correct for empty config
    assert!(runtime.providers.read().unwrap().is_empty());
    assert!(runtime.default_provider.read().unwrap().is_none());
    assert!(runtime.memory.is_none());
    assert!(runtime.embeddings.is_none());
    assert!(runtime.tool_names().is_empty());
    assert!(runtime.system_prompt.is_none());
    assert!(runtime.max_tokens.is_none());
    assert!(runtime.max_context_tokens.is_none());

    // Verify methods that could crash with empty config don't panic
    let _ = runtime.provider_ids();
    let _ = runtime.default_provider_id();
    let _ = runtime.has_memory_provider();
    let _ = runtime.has_embedding_provider();
    let _ = runtime.list_tool_info();
    let _ = runtime.system_prompt();

    // Verify getting a non-existent provider returns None, not a crash
    let _ = runtime.get_provider("nonexistent");
    let _ = runtime.default_provider();

    // Test setting values on empty runtime doesn't panic
    let mut runtime = runtime;
    runtime.set_system_prompt("test prompt".to_string());
    runtime.set_max_tokens(1000);
    runtime.set_max_context_tokens(8000);

    assert_eq!(runtime.system_prompt(), Some("test prompt"));
    assert_eq!(runtime.max_tokens, Some(1000));
    assert_eq!(runtime.max_context_tokens, Some(8000));
}

/// Test that AgentRuntime Default trait works correctly.
#[test]
pub(super) fn agent_runtime_default_is_empty() {
    let runtime = AgentRuntime::default();

    // Same checks as above but using Default
    assert!(runtime.providers.read().unwrap().is_empty());
    assert!(runtime.default_provider.read().unwrap().is_none());
}

// ─── issue #1042: continuity_key substitui o escopo de sessao ──────────
//
// Antes destes testes nenhum caso passava `session_id: Some` **e**
// `continuity_key: Some` ao mesmo tempo, que e exatamente o que os tres
// call sites de producao fazem. O AND do store tornava a flag
// `shared_continuity` um no-op entre sessoes.

/// Grava na sessao A com a chave compartilhada e recupera na sessao B.
/// Com o AND (o comportamento anterior), a linha da sessao A ficava fora
/// do resultado porque `session_id = "sessao-b"` nunca casa com ela.
#[tokio::test]
pub(super) async fn recall_com_continuity_key_atravessa_sessoes() {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);

    rt.remember_turn(
        "sessao-a",
        Some("bus:shared-global"),
        None,
        "o gato da Maria se chama Frajola",
        "",
    )
    .await
    .expect("remember_turn");

    let achados = rt
        .recall_context("gato", Some("sessao-b"), Some("bus:shared-global"), 10)
        .await
        .expect("recall_context");

    assert!(
        achados.iter().any(|m| m.content.contains("Frajola")),
        "a memoria da sessao A nao atravessou para a sessao B: {achados:?}"
    );
}

/// O irmao: sem chave de continuidade o escopo de sessao continua valendo,
/// e nada atravessa. E o que prova que a mudanca nao abriu a memoria de
/// todo mundo para todo mundo.
#[tokio::test]
pub(super) async fn recall_sem_continuity_key_nao_atravessa_sessoes() {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);

    rt.remember_turn(
        "sessao-a",
        None,
        None,
        "o gato da Maria se chama Frajola",
        "",
    )
    .await
    .expect("remember_turn");

    let achados = rt
        .recall_context("gato", Some("sessao-b"), None, 10)
        .await
        .expect("recall_context");

    assert!(
        !achados.iter().any(|m| m.content.contains("Frajola")),
        "sem continuity_key a memoria da sessao A vazou para a sessao B: {achados:?}"
    );

    // E na propria sessao A ela continua visivel.
    let na_propria = rt
        .recall_context("gato", Some("sessao-a"), None, 10)
        .await
        .expect("recall_context");
    assert!(
        na_propria.iter().any(|m| m.content.contains("Frajola")),
        "a memoria sumiu da propria sessao que a gravou: {na_propria:?}"
    );
}

/// Com a chave ligada, a memoria da **propria** sessao continua visivel —
/// ela tambem e gravada com a chave, entao trocar o escopo nao esconde
/// nada de quem esta conversando agora.
#[tokio::test]
pub(super) async fn recall_com_continuity_key_ainda_ve_a_propria_sessao() {
    let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
    let mut rt = AgentRuntime::new();
    rt.set_memory_provider(store);

    rt.remember_turn(
        "sessao-a",
        Some("bus:shared-global"),
        None,
        "o gato da Maria se chama Frajola",
        "",
    )
    .await
    .expect("remember_turn");

    let achados = rt
        .recall_context("gato", Some("sessao-a"), Some("bus:shared-global"), 10)
        .await
        .expect("recall_context");

    assert!(
        achados.iter().any(|m| m.content.contains("Frajola")),
        "a memoria da propria sessao sumiu com a chave ligada: {achados:?}"
    );
}
