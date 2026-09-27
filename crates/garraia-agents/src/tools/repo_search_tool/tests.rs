//! Testes de [`RepoSearchTool`]. Arquivo proprio pelo teto de 700 linhas do
//! Quality Ratchet (`.quality/`, plan 0064): o modulo cresceu com a #1329 e
//! levou `repo_search_tool.rs` a 840 linhas.

use super::*;

#[test]
fn test_repo_search_schema() {
    let tool = RepoSearchTool::new(None, None);
    let schema = tool.input_schema();
    assert!(schema.get("properties").is_some());
    assert_eq!(schema["required"].as_array().map(|a| a.len()), Some(1));
}

/// A busca acontece no diretorio da sessao, nao no CWD do gateway.
#[cfg(not(windows))]
#[tokio::test]
async fn searches_in_the_session_dir() {
    let dir = std::env::temp_dir().join(format!("garra-repo-search-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    std::fs::write(dir.join("notas.txt"), "agulha_unica_xyz\n").expect("write");

    let tool = RepoSearchTool::new(Some(10), None);
    let ctx = ToolContext {
        session_id: "test".into(),
        user_id: None,
        is_heartbeat: false,
        approval: crate::tools::approval::ToolApproval::None,
        working_dir: Some(dir.to_string_lossy().into_owned()),
        project_id: None,
    };
    let result = tool
        .execute(&ctx, serde_json::json!({"query": "agulha_unica_xyz"}))
        .await
        .expect("executa");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.contains("notas.txt"), "{}", result.content);
}

#[tokio::test]
async fn test_repo_search_empty_query() {
    let tool = RepoSearchTool::new(None, None);
    let ctx = ToolContext {
        session_id: "test".into(),
        user_id: None,
        is_heartbeat: false,
        approval: crate::tools::approval::ToolApproval::None,
        working_dir: None,
        project_id: None,
    };

    let result = tool
        .execute(&ctx, serde_json::json!({"query": ""}))
        .await
        .expect("should not error");

    assert!(result.is_error);
}

#[tokio::test]
async fn test_repo_search_missing_query() {
    let tool = RepoSearchTool::new(None, None);
    let ctx = ToolContext {
        session_id: "test".into(),
        user_id: None,
        is_heartbeat: false,
        approval: crate::tools::approval::ToolApproval::None,
        working_dir: None,
        project_id: None,
    };

    let result = tool.execute(&ctx, serde_json::json!({})).await;
    assert!(result.is_err());
}

// ─── #1380: recusa rapida quando nao ha repositorio ativo ─────────────

/// O contexto que a #1380 descreve: sessao sem `working_dir` (celular,
/// canal remoto, gateway sem projeto selecionado). Vem do helper que o
/// `repo_dir` ja expoe aos testes, para nao haver duas nocoes de
/// "contexto sem diretorio" na crate.
fn ctx_sem_working_dir() -> ToolContext {
    crate::tools::repo_dir::contexto_de_teste(None)
}

/// O programa esta no `PATH`? Sonda de metadado, sem executar nada.
#[cfg(not(windows))]
fn existe_no_path(programa: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(programa).is_file()))
}

/// Um diretorio fundo SEM nenhuma marca de repositorio acima dele: o
/// `tempdir` do sistema nao esta dentro de um checkout.
fn dir_sem_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join("a/b/c/d")).expect("subdirs");
    tmp
}

/// O caso (b): nada de repositorio no diretorio herdado. A tool recusa, e
/// a decisao custa alguns `stat` — nao os 15s de varredura da issue.
#[test]
fn sem_repositorio_no_cwd_herdado_recusa_e_e_barato() {
    let tmp = dir_sem_repo();
    let fundo = tmp.path().join("a/b/c/d");
    assert!(!dentro_de_repositorio(&fundo), "{}", fundo.display());

    let repo = RepoDir::ProcessoCwd(Some(fundo.clone()));
    let antes = std::time::Instant::now();
    let motivo = recusa_sem_repositorio(&repo, Duration::from_secs(15), true)
        .expect("sem repositorio, a tool tem de recusar");
    let gasto = antes.elapsed();

    assert!(
        gasto < Duration::from_millis(500),
        "a recusa levou {gasto:?} — a issue pede resposta imediata, nao o timeout"
    );
    assert!(motivo.contains("No active repository"), "{motivo}");
    // A mensagem diz ONDE ele olhou e O QUE fazer: sem isso o operador
    // nao tem como distinguir isto de "a busca nao encontrou nada".
    assert!(motivo.contains(&fundo.display().to_string()), "{motivo}");
    assert!(motivo.contains("working directory"), "{motivo}");
    assert!(motivo.contains(".git"), "{motivo}");

    // CWD ilegivel tambem recusa, e sem citar caminho nenhum.
    let motivo = recusa_sem_repositorio(&RepoDir::ProcessoCwd(None), Duration::from_secs(15), true)
        .expect("sem CWD legivel nao ha onde buscar");
    assert!(motivo.contains("No active repository"), "{motivo}");
}

/// Revisao da onda B: o turno restrito recebe a MESMA recusa, sem o
/// caminho absoluto do host. `repo_search` vive no modo `search`, o mesmo
/// turno em que o `garra_status` retem `session.working_dir` (#1347) —
/// sem este portao a mensagem de erro entregaria pela porta dos fundos o
/// caminho que a outra tool nega.
#[test]
fn turno_restrito_recusa_sem_entregar_o_caminho_do_host() {
    let tmp = dir_sem_repo();
    let fundo = tmp.path().join("a/b/c/d");
    let repo = RepoDir::ProcessoCwd(Some(fundo.clone()));

    let motivo = recusa_sem_repositorio(&repo, Duration::from_secs(15), false)
        .expect("o turno restrito recusa igual: muda o que a mensagem conta, nao a decisao");
    // Espelho do caso aberto: mesma recusa acionavel, sem o caminho.
    assert!(motivo.contains("No active repository"), "{motivo}");
    assert!(motivo.contains("working directory"), "{motivo}");
    assert!(motivo.contains("the process directory"), "{motivo}");
    assert!(
        !motivo.contains(&fundo.display().to_string()),
        "o caminho do host vazou para um turno restrito: {motivo}"
    );
    // Nem o caminho inteiro, nem um pedaco dele que ja localize o host.
    //
    // So o componente que ESTE teste criou e que identifica o host: o
    // nome sorteado do tempdir. Varrer o caminho inteiro arrastaria junto
    // os componentes do `$TMPDIR` herdado do runner, e um `TMPDIR` que
    // por acaso contivesse "search" ou "project" derrubaria o teste por
    // um motivo que nao e o desta issue; os diretorios `a/b/c/d` sao
    // letras soltas, que casam com qualquer prosa.
    let nome_do_tempdir = tmp
        .path()
        .file_name()
        .expect("tempdir tem nome")
        .to_string_lossy()
        .to_string();
    assert!(
        !motivo.contains(&nome_do_tempdir),
        "o componente {nome_do_tempdir:?} do caminho vazou: {motivo}"
    );
}

/// O portao que liga o bit do turno a mensagem, com o fail-closed que o
/// resto nao cobre: sem turno (chamada fora de escopo) o caminho NAO sai.
#[tokio::test]
async fn caminho_so_sai_no_turno_aberto_e_fail_closed_sem_turno() {
    use crate::tools::turn_tools::com_ferramentas_do_turno;

    assert!(
        !pode_revelar_caminho(),
        "fora de um turno o bit e desconhecido, e o fail-closed e esconder"
    );
    let restrito = com_ferramentas_do_turno(vec!["repo_search".to_string()], true, async {
        pode_revelar_caminho()
    })
    .await;
    assert!(!restrito, "turno restrito nao pode ver o caminho do host");
    let aberto = com_ferramentas_do_turno(vec!["repo_search".to_string()], false, async {
        pode_revelar_caminho()
    })
    .await;
    assert!(aberto, "o operador local ve onde a tool olhou");
}

/// O caso (a), que a recusa NAO pode pegar: sem `working_dir`, mas com o
/// CWD do processo dentro de um repositorio — o `garra chat` rodando na
/// raiz do projeto do usuario. Aqui a tool segue buscando, como sempre.
#[test]
fn sem_working_dir_com_repositorio_no_cwd_nao_recusa() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for (marca, como_diretorio) in [
        (".git", true),
        // Worktree e submodulo: `.git` e ARQUIVO, e continua sendo
        // repositorio.
        (".git", false),
        (".hg", true),
        (".svn", true),
        (".jj", true),
    ] {
        let raiz = tmp.path().join(format!("{marca}-{como_diretorio}"));
        let fundo = raiz.join("src/tools");
        std::fs::create_dir_all(&fundo).expect("subdirs");
        if como_diretorio {
            std::fs::create_dir_all(raiz.join(marca)).expect("marca");
        } else {
            std::fs::write(raiz.join(marca), "gitdir: /outro/lugar\n").expect("marca");
        }

        // Tanto na raiz quanto la no fundo: a marca vale para a arvore.
        for dir in [&raiz, &fundo] {
            assert!(dentro_de_repositorio(dir), "{}", dir.display());
            let repo = RepoDir::ProcessoCwd(Some(dir.clone()));
            assert_eq!(
                recusa_sem_repositorio(&repo, Duration::from_secs(15), true),
                None,
                "{marca} em {} foi recusado: e o caso do `garra chat` local",
                dir.display()
            );
        }
    }
}

/// A sessao que ESCOLHEU um diretorio nunca passa pela recusa, tenha ele
/// repositorio ou nao: quem escolheu decide o que ha la, e recusar
/// mudaria o contrato de quem ja usa a tool assim.
#[test]
fn working_dir_da_sessao_nunca_e_recusado() {
    let tmp = dir_sem_repo();
    let repo = RepoDir::decidir(Some(&tmp.path().to_string_lossy()));
    assert!(matches!(repo, RepoDir::Sessao(_)), "{repo:?}");
    assert_eq!(
        recusa_sem_repositorio(&repo, Duration::from_secs(15), true),
        None
    );
}

/// Regressao de ponta a ponta do caso (a), pela `execute` de verdade: o
/// processo de teste roda no diretorio do crate, dentro deste
/// repositorio, e a sessao nao tem `working_dir` — exatamente a forma que
/// a correcao nao pode quebrar.
///
/// A agulha e a propria string literal desta linha: ela existe neste
/// arquivo fonte, entao um resultado com `repo_search_tool` prova que a
/// busca rodou no repositorio herdado do CWD, e nao que a tool respondeu
/// qualquer coisa.
#[cfg(not(windows))]
#[tokio::test]
async fn regressao_1380_sem_working_dir_no_repo_do_cwd_ainda_busca() {
    let cwd = std::env::current_dir().expect("CWD do processo de teste");
    assert!(
        dentro_de_repositorio(&cwd),
        "este teste precisa rodar dentro do checkout ({}): e o caso (a) da #1380",
        cwd.display()
    );

    // A decisao em si nao depende de programa nenhum, e vale sempre.
    assert_eq!(
        recusa_sem_repositorio(&RepoDir::decidir(None), Duration::from_secs(15), true),
        None,
        "a sessao sem working_dir dentro do checkout nao pode ser recusada"
    );
    // A busca de verdade so com o `rg`: sem ele o fallback e um `grep -r`
    // que nao le `.gitignore` e desceria em `target/`, o que torna o teste
    // lento por um motivo que nao e o desta issue.
    if !existe_no_path("rg") {
        eprintln!("rg ausente no PATH: parte da execucao deste teste foi pulada");
        return;
    }

    let tool = RepoSearchTool::new(Some(20), None);
    let saida = tool
        .execute(
            &ctx_sem_working_dir(),
            serde_json::json!({
                "query": "agulha_da_regressao_1380",
                "file_pattern": "*.rs",
                "max_results": 5,
                "context_lines": 0,
            }),
        )
        .await
        .expect("executa");

    assert!(!saida.is_error, "{}", saida.content);
    assert!(
        !saida.content.contains("No active repository"),
        "a recusa da #1380 pegou o caso legitimo: {}",
        saida.content
    );
    // O nome do diretorio, e nao do arquivo: a agulha mora neste
    // `repo_search_tool/tests.rs`, e o separador de caminho muda por sistema.
    assert!(
        saida.content.contains("repo_search_tool"),
        "a busca tinha de achar a agulha neste arquivo: {}",
        saida.content
    );
}

// ─── #1266: a query do modelo nunca pode cair em posicao de flag ──────
//
// Os tres call sites (`rg`, `grep`, `findstr`) sao cobertos aqui pelos
// construtores puros de argumento; o caminho do `rg` ganha ainda um teste
// de execucao pela tool que o runtime registra, em `runtime.rs`
// (`repo_search_registrada_nao_executa_pre_do_ripgrep`), porque e nele que
// a injecao virava execucao de comando.

/// Posicao do `--` no `rg`: tudo que vem do modelo fica depois dele.
#[test]
fn rg_args_poem_a_query_depois_do_terminador() {
    let args = rg_args("--pre=/bin/sh", None, 2, 50);
    let term = args
        .iter()
        .position(|a| a == "--")
        .expect("o terminador tem de existir");
    let query = args
        .iter()
        .position(|a| a == "--pre=/bin/sh")
        .expect("a query tem de estar na linha");
    assert!(
        term < query,
        "query antes do terminador vira flag: {args:?}"
    );
    // E nada de padrao de busca duplicado antes do terminador.
    assert!(
        !args[..term].iter().any(|a| a == "--pre=/bin/sh"),
        "{args:?}"
    );
}

/// O `file_pattern` tambem vem do modelo: fica colado no `--glob=` para
/// nao depender de como o parser da vez resolve um valor com `-`.
#[test]
fn rg_args_colam_o_glob_no_nome_da_opcao() {
    let args = rg_args("agulha", Some("--pre=/bin/sh"), 2, 50);
    assert!(
        args.contains(&"--glob=--pre=/bin/sh".to_string()),
        "{args:?}"
    );
    assert!(!args.iter().any(|a| a == "--glob"), "{args:?}");
}

/// Mesma regra no fallback Unix.
#[test]
fn grep_args_poem_a_query_depois_do_terminador() {
    let args = grep_args("-f/etc/passwd", 2);
    let term = args.iter().position(|a| a == "--").expect("terminador");
    let query = args
        .iter()
        .position(|a| a == "-f/etc/passwd")
        .expect("query");
    assert!(term < query, "{args:?}");
}

/// O `findstr` nao tem `--`; o equivalente e `/C:`, e a query nunca pode
/// aparecer como argumento solto (ai um `/` inicial viraria opcao).
#[test]
fn findstr_args_embrulham_a_query_em_barra_c() {
    for adversarial in ["/OFF", "-f/etc/passwd", "--pre=/bin/sh"] {
        let args = findstr_args(adversarial);
        assert!(args.contains(&format!("/C:{adversarial}")), "{args:?}");
        assert!(
            !args.iter().any(|a| a == adversarial),
            "query solta na linha do findstr: {args:?}"
        );
    }
}

/// Fallback Unix de ponta a ponta: os argumentos que a tool monta, dados
/// ao `grep` de verdade. O caminho de fallback so dispara quando o `rg`
/// nao existe na maquina, o que nao da para forcar de dentro do teste —
/// entao o que se exercita e exatamente a linha de comando que a tool
/// produz, sem reescreve-la a mao.
///
/// Sem o `--`, o grep leria `/etc/passwd` como arquivo de padroes (`-f`) e
/// nao acharia a linha; com ele, a busca e literal e acha.
#[cfg(not(windows))]
#[test]
fn grep_trata_query_com_traco_como_texto_literal() {
    let dir = std::env::temp_dir().join(format!(
        "garra-grep-flag-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("tempdir");
    std::fs::write(dir.join("notas.txt"), "antes -f/etc/passwd depois\n").expect("write");

    let saida = std::process::Command::new("grep")
        .args(grep_args("-f/etc/passwd", 0))
        .current_dir(&dir)
        .output();
    let _ = std::fs::remove_dir_all(&dir);

    let saida = saida.expect("grep tem de existir em Unix");
    let stdout = String::from_utf8_lossy(&saida.stdout);
    assert!(
        stdout.contains("notas.txt"),
        "a query devia ser padrao literal; stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&saida.stderr)
    );
}
