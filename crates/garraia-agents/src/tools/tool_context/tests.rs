use super::*;
use tempfile::TempDir;

/// A recusa compartilhada (#1909): motivo certo por caso, e o caminho da
/// sessão NUNCA sai na mensagem.
#[test]
fn recusa_sessao_sem_diretorio_distingue_os_casos_sem_vazar_o_caminho() {
    let tmp = TempDir::new().unwrap();
    let projeto = tmp.path().join("projeto");
    std::fs::create_dir(&projeto).unwrap();

    assert_eq!(
        recusa_sessao_sem_diretorio(Some(&projeto), "nothing was read", "read again"),
        None,
        "diretório que existe e é diretório não é recusado"
    );

    std::fs::remove_dir(&projeto).unwrap();
    let recusa = recusa_sessao_sem_diretorio(Some(&projeto), "nothing was read", "read again")
        .expect("diretório removido é recusado");
    // A sentença inteira, não só pedaços: é o contrato que o `repo_search`
    // entrega desde a #1932 — mudar o vocabulário dele ou o desta recusa
    // tem de quebrar aqui, não em produção.
    assert_eq!(
        recusa,
        "The session's working directory is not accessible (it does not exist), so nothing \
         was read. This is not an empty result: select an existing project for this session \
         (set its working directory) and read again."
    );
    assert!(
        !recusa.contains(projeto.to_str().unwrap()),
        "o caminho da sessão não sai na recusa: {recusa}"
    );

    let arquivo = tmp.path().join("arquivo.txt");
    std::fs::write(&arquivo, "x").unwrap();
    let recusa = recusa_sessao_sem_diretorio(Some(&arquivo), "nothing was listed", "list again")
        .expect("arquivo no lugar do diretório é recusado");
    assert!(recusa.contains("it is not a directory"), "{recusa}");
    assert!(
        !recusa.contains(arquivo.to_str().unwrap()),
        "o caminho da sessão não sai na recusa: {recusa}"
    );

    assert_eq!(
        recusa_sessao_sem_diretorio(None, "nothing was read", "read again"),
        None,
        "sem working_dir não há diretório da sessão a recusar"
    );
    for em_branco in ["", "   ", "\t"] {
        assert_eq!(
            recusa_sessao_sem_diretorio(
                Some(Path::new(em_branco)),
                "nothing was read",
                "read again"
            ),
            None,
            "working_dir em branco ({em_branco:?}) não é diretório da sessão"
        );
    }
}

#[test]
fn resolve_relative_with_working_dir() {
    let tmp = TempDir::new().unwrap();
    // Create the subdirectory so path validation can canonicalize the parent.
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
    let resolved = ctx.resolve_path("src/main.rs").unwrap();
    assert!(resolved.starts_with(tmp.path()));
    assert!(resolved.ends_with("src/main.rs"));
}

#[test]
fn resolve_absolute_passes_through() {
    let tmp = TempDir::new().unwrap();
    let abs_path = tmp.path().join("foo.txt");
    // Create the file so canonicalize succeeds.
    std::fs::write(&abs_path, "").unwrap();

    let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
    let resolved = ctx.resolve_path(abs_path.to_str().unwrap()).unwrap();
    assert_eq!(
        resolved.canonicalize().unwrap(),
        abs_path.canonicalize().unwrap()
    );
}

#[test]
fn rejects_parent_dir_traversal() {
    let tmp = TempDir::new().unwrap();
    let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
    let result = ctx.resolve_path("../../../etc/passwd");
    assert!(result.is_err());
}

#[test]
fn unsandboxed_allows_anything() {
    let ctx = ProjectToolContext::unsandboxed(None, None);
    // Even `..` in the path should pass when sandbox is off.
    // (validate_path is not called)
    let result = ctx.resolve_path("../some/path");
    assert!(result.is_ok());
}

// ─── resolve_tool_path / expand_home (issue #923) ──────────────────────

#[test]
fn expands_a_leading_tilde() {
    let home = PathBuf::from("/data/data/com.termux/files/home");
    for raw in ["~/Documents/metas.md", "$HOME/Documents/metas.md"] {
        let (p, expanded) = expand_home(raw, Some(&home));
        assert!(expanded, "{raw} deveria expandir");
        assert_eq!(p, home.join("Documents/metas.md"));
    }
    let (p, expanded) = expand_home("~", Some(&home));
    assert!(expanded);
    assert_eq!(p, home);
}

/// Um `~` no meio do caminho é nome de arquivo legítimo. Reescrever seria
/// pior que não expandir.
#[test]
fn only_expands_a_leading_tilde() {
    let home = PathBuf::from("/home/u");
    for raw in ["/etc/~/passwd", "backup~", "a/~/b", "~user/file"] {
        let (p, expanded) = expand_home(raw, Some(&home));
        assert!(!expanded, "{raw} não deveria expandir");
        assert_eq!(p, PathBuf::from(raw));
    }
}

/// Sem HOME o input passa intacto: o erro depois nomeia o caminho literal
/// com `~`, que é compreensível — um palpite errado não seria.
#[test]
fn leaves_tilde_alone_without_a_home() {
    let (p, expanded) = expand_home("~/x", None);
    assert!(!expanded);
    assert_eq!(p, PathBuf::from("~/x"));
}

#[test]
fn resolve_classifies_how_it_got_there() {
    let home = PathBuf::from("/home/u");
    // "Absoluto" e decisao do host: `/etc/hosts` nao tem letra de drive e
    // no Windows e relativo. Os dois absolutos do teste vem na forma
    // nativa de cada plataforma (#1697).
    let (etc_hosts, work) = if cfg!(windows) {
        (r"C:\etc\hosts", r"C:\work\proj")
    } else {
        ("/etc/hosts", "/work/proj")
    };

    let r = resolve_tool_path(etc_hosts, None, Some(&home)).unwrap();
    assert_eq!(r.origin, PathOrigin::Absolute);
    assert_eq!(r.path, PathBuf::from(etc_hosts));

    let r = resolve_tool_path("~/notes.md", None, Some(&home)).unwrap();
    assert_eq!(r.origin, PathOrigin::HomeExpanded);
    assert_eq!(r.path, home.join("notes.md"));

    let r = resolve_tool_path("src/main.rs", Some(work), Some(&home)).unwrap();
    assert_eq!(r.origin, PathOrigin::JoinedWorkingDir);
    assert_eq!(r.path, PathBuf::from(work).join("src/main.rs"));

    // O caso que mordeu na #923: relativo, sem working_dir.
    let r = resolve_tool_path("notes.md", None, Some(&home)).unwrap();
    assert_eq!(r.origin, PathOrigin::RelativeToProcessCwd);
    assert_eq!(r.path, PathBuf::from("notes.md"));
}

/// `working_dir: Some("")` é o mesmo que ausente — um working_dir vazio
/// transformaria um caminho relativo em absoluto por acidente.
#[test]
fn empty_working_dir_counts_as_absent() {
    let r = resolve_tool_path("notes.md", Some(""), None).unwrap();
    assert_eq!(r.origin, PathOrigin::RelativeToProcessCwd);
    assert_eq!(r.path, PathBuf::from("notes.md"));
}

/// A checagem de `..` roda no input CRU, antes da expansão: um `~/../etc`
/// não pode contrabandear traversal através do home.
#[test]
fn rejects_traversal_before_expanding() {
    let home = PathBuf::from("/home/u");
    for raw in ["../etc/passwd", "~/../../etc/passwd", "a/../../b"] {
        assert!(
            resolve_tool_path(raw, Some("/work"), Some(&home)).is_err(),
            "{raw} deveria ser rejeitado"
        );
    }
}

/// A mensagem de erro tem de conter o resolvido, o pedido e o porquê —
/// era exatamente o que faltava para o agente não dizer "não existe".
#[test]
fn describe_names_resolution_and_request() {
    let r = resolve_tool_path("notes.md", None, None).unwrap();
    let d = r.describe();
    assert!(d.contains("notes.md"));
    assert!(d.contains("pedido:"));
    assert!(d.contains("working_dir"));
}

/// #1626: dentro do `working_dir`, a mensagem mostra o caminho RELATIVO —
/// nunca o prefixo absoluto do host (que, na ponte MCP HTTP, e a subarvore
/// por-orquestrador sob `<data_dir>/workspace`).
#[test]
fn describe_mostra_relativo_ao_working_dir_e_nunca_o_host() {
    let wd = "/home/dono/.local/share/garraia/workspace/abc123def456";
    let r = resolve_tool_path("notas/a.txt", Some(wd), None).unwrap();
    let d = r.describe();
    assert!(d.contains("notas/a.txt"), "{d}");
    assert!(!d.contains("/home/dono"), "o caminho do host vazou: {d}");
    assert!(
        !d.contains("workspace/abc123"),
        "o dir do orquestrador vazou: {d}"
    );
}

/// `caminho_para_exibir`: relativo quando dentro do `working_dir`, absoluto
/// quando fora ou sem `working_dir` (o modelo ja o escolheu).
#[test]
fn caminho_para_exibir_relativiza_so_dentro_do_working_dir() {
    // Sem working_dir: o proprio caminho.
    assert_eq!(
        caminho_para_exibir(Path::new("/etc/hosts"), None),
        "/etc/hosts"
    );
    // Fora do working_dir: absoluto.
    assert_eq!(
        caminho_para_exibir(Path::new("/etc/hosts"), Some(Path::new("/home/dono/ws"))),
        "/etc/hosts"
    );
    // Dentro do working_dir: relativo, sem o prefixo do host.
    assert_eq!(
        caminho_para_exibir(
            Path::new("/home/dono/ws/sub/a.txt"),
            Some(Path::new("/home/dono/ws"))
        ),
        "sub/a.txt"
    );
    // O proprio working_dir: "." em vez de vazio.
    assert_eq!(
        caminho_para_exibir(Path::new("/home/dono/ws"), Some(Path::new("/home/dono/ws"))),
        "."
    );
}

#[test]
fn is_path_allowed_basic() {
    let tmp = TempDir::new().unwrap();
    let inside = tmp.path().join("hello.txt");
    std::fs::write(&inside, "").unwrap();

    let ctx = ProjectToolContext::new(Some(tmp.path().to_path_buf()), None);
    assert!(ctx.is_path_allowed(&inside));
}

// ─── #1909: o caminho do host fora do working_dir ─────────────────────

/// A metade segura continua saindo em QUALQUER turno: o alvo dentro do
/// `working_dir` aparece relativo, e relativo nao nomeia host nenhum.
#[tokio::test]
async fn caminho_dentro_do_working_dir_sai_relativo_ate_no_turno_restrito() {
    let wd = Path::new("/home/dono/.local/share/garraia/workspace/abc");
    let alvo = Path::new("/home/dono/.local/share/garraia/workspace/abc/notas/a.txt");
    let mostrado = crate::tools::turn_tools::com_restricao_do_turno(true, async {
        caminho_para_exibir_no_turno(alvo, Some(wd))
    })
    .await;
    assert_eq!(mostrado, "notas/a.txt");
}

/// Fora do `working_dir` (ou sem `working_dir`, como uma sessao remota sem
/// projeto), o absoluto so sai quando o turno pode ver. No restrito sai o
/// marcador; fora de um turno do runtime o `None` do bit e fail-closed.
#[tokio::test]
async fn caminho_fora_do_working_dir_e_retido_no_turno_restrito() {
    // Absoluto na forma NATIVA: `/home/...` nao tem prefixo de unidade e
    // no Windows `is_absolute()` e `false` — o caso 2 (relativo) o
    // deixaria sair e o teste provaria outra coisa.
    #[cfg(windows)]
    let alvo = Path::new(r"C:\Users\dono\projetos\cliente-x\notas.txt");
    #[cfg(not(windows))]
    let alvo = Path::new("/home/dono/projetos/cliente-x/notas.txt");
    assert!(alvo.is_absolute(), "o fixture tem de ser absoluto aqui");

    let restrito = crate::tools::turn_tools::com_restricao_do_turno(true, async {
        caminho_para_exibir_no_turno(alvo, None)
    })
    .await;
    assert_eq!(restrito, CAMINHO_FORA_DO_TURNO);

    let aberto = crate::tools::turn_tools::com_restricao_do_turno(false, async {
        caminho_para_exibir_no_turno(alvo, None)
    })
    .await;
    assert_eq!(
        aberto,
        alvo.display().to_string(),
        "o operador local continua vendo o caminho inteiro"
    );

    // Fora de um turno despachado pelo runtime nao ha escopo: o lado
    // seguro e reter.
    assert_eq!(
        caminho_para_exibir_no_turno(alvo, None),
        CAMINHO_FORA_DO_TURNO
    );
}

/// A mensagem inteira do `describe` obedece ao mesmo portao: o lugar do
/// caminho RESOLVIDO passa a ser o marcador. O `pedido:` continua sendo o
/// texto que o modelo escreveu — se ele pediu um caminho absoluto, esse
/// absoluto e dele, nao uma informacao nova sobre o host.
#[tokio::test]
async fn describe_retem_o_caminho_do_host_mas_nomeia_o_pedido() {
    let tmp = TempDir::new().unwrap();
    let alvo = tmp.path().join("segredo.txt");
    std::fs::write(&alvo, "x").unwrap();
    let r = resolve_tool_path(alvo.to_str().unwrap(), None, None).unwrap();

    let restrito =
        crate::tools::turn_tools::com_restricao_do_turno(true, async { r.clone().describe() })
            .await;
    assert!(
        restrito.starts_with(&format!("'{CAMINHO_FORA_DO_TURNO}'")),
        "o caminho resolvido nao foi retido: {restrito}"
    );
    assert!(
        restrito.contains("pedido:"),
        "o pedido continua: {restrito}"
    );
    assert!(restrito.contains("caminho absoluto"), "{restrito}");

    let aberto =
        crate::tools::turn_tools::com_restricao_do_turno(false, async { r.clone().describe() })
            .await;
    assert!(
        aberto.starts_with(&format!("'{}'", alvo.display())),
        "o turno aberto perdeu o caminho que antes via: {aberto}"
    );

    let sem_escopo = r.describe();
    assert!(
        sem_escopo.starts_with(&format!("'{CAMINHO_FORA_DO_TURNO}'")),
        "sem escopo e fail-closed: {sem_escopo}"
    );
}

/// O argumento relativo sem `working_dir` (o caso da sessao remota sem
/// projeto) e o que mais importa: o modelo pede `notas.txt`, a resolucao
/// e contra o CWD do processo, e a mensagem nao pode nomear esse CWD.
#[tokio::test]
async fn pedido_relativo_sem_working_dir_nao_nomeia_o_cwd_do_processo() {
    let r = resolve_tool_path("notas.txt", None, None).unwrap();
    let restrito =
        crate::tools::turn_tools::com_restricao_do_turno(true, async { r.clone().describe() })
            .await;
    assert!(restrito.contains("pedido: 'notas.txt'"), "{restrito}");
    assert!(
        restrito.contains("a sessão não tem working_dir"),
        "{restrito}"
    );
    assert!(
        restrito.starts_with("'notas.txt'"),
        "o argumento relativo continua nomeado como o modelo o escreveu: {restrito}"
    );
}
