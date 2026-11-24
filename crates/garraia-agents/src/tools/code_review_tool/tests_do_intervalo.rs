//! #1909: os testes do intervalo do `code_review` — as pontas do
//! `commit_range`, a linha `Intervalo:` e as recusas. Git real, sem rede.

use super::tests::tool_com_eco;
use super::*;
use crate::tools::repo_dir::{contexto_de_teste as ctx, repo_git_temporario};

fn git_no_repo(dir: &std::path::Path, args: &[&str]) -> String {
    let saida = std::process::Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.email=test@test",
            "-c",
            "user.name=t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git precisa estar no PATH para este teste");
    assert!(
        saida.status.success(),
        "git {args:?} falhou: {}",
        String::from_utf8_lossy(&saida.stderr)
    );
    String::from_utf8_lossy(&saida.stdout).trim().to_string()
}

/// A linha `Intervalo:` da resposta — a do cabecalho, que vem antes do
/// `---`; o corpo e o prompt ecoado pelo provedor de teste.
fn linha_do_intervalo(conteudo: &str) -> &str {
    conteudo
        .lines()
        .take_while(|l| *l != "---")
        .find(|l| l.starts_with("Intervalo:"))
        .unwrap_or_default()
}

/// #1909: as formas que UM argumento de revisao tem no `git diff`, e a
/// ponta vazia valendo `HEAD`. O `...` e lido antes do `..`.
#[test]
fn interpretar_commit_range_separa_as_formas_do_git() {
    use PedidoDeIntervalo::{Arvore, ContraArvore, DesdeABaseComum, Entre};
    let s = str::to_string;
    assert_eq!(interpretar_commit_range(None), Arvore);
    assert_eq!(
        interpretar_commit_range(Some("HEAD")),
        ContraArvore(s("HEAD"))
    );
    assert_eq!(
        interpretar_commit_range(Some("main..feature")),
        Entre(s("main"), s("feature"))
    );
    assert_eq!(
        interpretar_commit_range(Some("main...feature")),
        DesdeABaseComum(s("main"), s("feature"))
    );
    assert_eq!(
        interpretar_commit_range(Some("HEAD~3..")),
        Entre(s("HEAD~3"), s("HEAD"))
    );
    assert_eq!(
        interpretar_commit_range(Some("..feature")),
        Entre(s("HEAD"), s("feature"))
    );
    assert_eq!(
        interpretar_commit_range(Some("main...")),
        DesdeABaseComum(s("main"), s("HEAD"))
    );
    assert_eq!(
        interpretar_commit_range(Some("abc^!")),
        Entre(s("abc^"), s("abc"))
    );
    // Sobra de ponto fica DENTRO da ponta, que depois nao resolve.
    assert_eq!(
        interpretar_commit_range(Some("a....b")),
        DesdeABaseComum(s("a"), s(".b"))
    );
    assert_eq!(
        interpretar_commit_range(Some("a..b..c")),
        Entre(s("a"), s("b..c"))
    );
}

/// #1909: sem `commit_range` a revisao diz que comparou indice com
/// arvore de trabalho, e em qual `HEAD`.
#[tokio::test]
async fn sem_commit_range_a_revisao_nomeia_arvore_e_head() {
    let repo = repo_git_temporario("alvo-review-arvore", "ramo-review-arvore");
    let head = git_no_repo(repo.path(), &["rev-parse", "HEAD"]);
    let wd = repo.path().to_string_lossy().into_owned();

    let saida = tool_com_eco()
        .execute(&ctx(Some(&wd)), serde_json::json!({}))
        .await
        .expect("execute");

    assert!(!saida.is_error, "{}", saida.content);
    assert_eq!(
        linha_do_intervalo(&saida.content),
        format!(
            "Intervalo: índice .. árvore de trabalho (mudanças ainda fora do índice; \
             HEAD em {head})"
        ),
        "{}",
        saida.content
    );
}

/// #1909: `A..B` — cada ponta sai com o nome pedido e o SHA que ele
/// valia. O nome do ramo e movel; e por isso que a revisao precisa do SHA.
///
/// **Mutação que este teste pega**: passe o `range` cru ao `git diff` e
/// tire a linha do cabecalho — a asserção da linha fica vermelha.
#[tokio::test]
async fn commit_range_entre_commits_nomeia_o_sha_de_cada_ponta() {
    let repo = repo_git_temporario("alvo-review-entre", "ramo-review-entre");
    git_no_repo(repo.path(), &["commit", "-q", "-am", "segundo"]);
    let de = git_no_repo(repo.path(), &["rev-parse", "HEAD~1"]);
    let ate = git_no_repo(repo.path(), &["rev-parse", "HEAD"]);
    assert_ne!(de, ate);
    let wd = repo.path().to_string_lossy().into_owned();

    for (range, esperado) in [
        (
            "HEAD~1..ramo-review-entre",
            format!("Intervalo: HEAD~1 ({de}) .. ramo-review-entre ({ate})"),
        ),
        // Ponta vazia vale HEAD, e a linha diz `HEAD` em vez de calar.
        (
            "HEAD~1..",
            format!("Intervalo: HEAD~1 ({de}) .. HEAD ({ate})"),
        ),
        // `A^!` e o commit sozinho: pai de A .. A.
        ("HEAD^!", format!("Intervalo: HEAD^ ({de}) .. HEAD ({ate})")),
    ] {
        let saida = tool_com_eco()
            .execute(&ctx(Some(&wd)), serde_json::json!({"commit_range": range}))
            .await
            .expect("execute");
        assert!(!saida.is_error, "{range}: {}", saida.content);
        assert_eq!(
            linha_do_intervalo(&saida.content),
            esperado,
            "{range}: {}",
            saida.content
        );
        // O diff revisado e o desse intervalo: o do segundo commit.
        assert!(
            saida.content.contains("+linha alterada"),
            "{range}: {}",
            saida.content
        );
    }
}

/// #1909: `A` sozinho compara A com a arvore de trabalho.
#[tokio::test]
async fn commit_range_de_uma_revisao_nomeia_a_arvore_de_trabalho() {
    let repo = repo_git_temporario("alvo-review-uma", "ramo-review-uma");
    let head = git_no_repo(repo.path(), &["rev-parse", "HEAD"]);
    let wd = repo.path().to_string_lossy().into_owned();

    let saida = tool_com_eco()
        .execute(
            &ctx(Some(&wd)),
            serde_json::json!({"commit_range": "ramo-review-uma"}),
        )
        .await
        .expect("execute");

    assert!(!saida.is_error, "{}", saida.content);
    assert_eq!(
        linha_do_intervalo(&saida.content),
        format!(
            "Intervalo: ramo-review-uma ({head}) .. árvore de trabalho (inclui o que está \
             no índice)"
        ),
        "{}",
        saida.content
    );
}

/// #1909: `A...B` — o diff e o de B desde a base comum com A. A linha
/// nomeia as duas pontas com SHA, e o que o ramo A fez depois da base
/// NAO entra no diff revisado.
#[tokio::test]
async fn commit_range_de_tres_pontos_revisa_desde_a_base_comum() {
    let repo = repo_git_temporario("alvo-review-base", "tronco");
    let dir = repo.path();
    git_no_repo(dir, &["commit", "-q", "-am", "base"]);
    git_no_repo(dir, &["checkout", "-q", "-b", "ramo"]);
    std::fs::write(dir.join("so-no-ramo.txt"), "do ramo\n").expect("write");
    git_no_repo(dir, &["add", "."]);
    git_no_repo(dir, &["commit", "-q", "-m", "ramo"]);
    git_no_repo(dir, &["checkout", "-q", "tronco"]);
    std::fs::write(dir.join("so-no-tronco.txt"), "do tronco\n").expect("write");
    git_no_repo(dir, &["add", "."]);
    git_no_repo(dir, &["commit", "-q", "-m", "tronco"]);
    let tronco = git_no_repo(dir, &["rev-parse", "tronco"]);
    let ramo = git_no_repo(dir, &["rev-parse", "ramo"]);
    let wd = dir.to_string_lossy().into_owned();

    let saida = tool_com_eco()
        .execute(
            &ctx(Some(&wd)),
            serde_json::json!({"commit_range": "tronco...ramo"}),
        )
        .await
        .expect("execute");

    assert!(!saida.is_error, "{}", saida.content);
    assert_eq!(
        linha_do_intervalo(&saida.content),
        format!("Intervalo: base comum de tronco ({tronco}) e ramo ({ramo}) .. ramo ({ramo})"),
        "{}",
        saida.content
    );
    assert!(
        saida.content.contains("so-no-ramo.txt"),
        "{}",
        saida.content
    );
    assert!(
        !saida.content.contains("so-no-tronco.txt"),
        "o `...` virou `..` — o lado do tronco entrou no diff:\n{}",
        saida.content
    );
}

/// #1909: ponta que nao e commit do repositorio — o diff NAO roda, nada
/// vai para o LLM, e a recusa diz qual ponta faltou sem emprestar SHA.
/// Antes o git falhava calado e a tool respondia "No diff output (no
/// changes found)": um alvo que nao existe lido como "nada mudou".
#[tokio::test]
async fn ponta_que_nao_resolve_recusa_a_revisao_e_diz_qual() {
    let repo = repo_git_temporario("alvo-review-ponta", "ramo-review-ponta");
    let head = git_no_repo(repo.path(), &["rev-parse", "HEAD"]);
    let wd = repo.path().to_string_lossy().into_owned();

    for (range, esperado) in [
        (
            "sumiu..HEAD",
            format!("Intervalo: sumiu (revisão não resolvida) .. HEAD ({head})"),
        ),
        (
            "HEAD..a..b",
            format!("Intervalo: HEAD ({head}) .. a..b (revisão não resolvida)"),
        ),
        (
            "nao-existe",
            "Intervalo: nao-existe (revisão não resolvida) .. árvore de trabalho (inclui o \
             que está no índice)"
                .to_string(),
        ),
        // Uma arvore nao e ponta de intervalo de revisao.
        (
            "HEAD^{tree}..HEAD",
            format!("Intervalo: HEAD^{{tree}} (revisão não resolvida) .. HEAD ({head})"),
        ),
    ] {
        let saida = tool_com_eco()
            .execute(&ctx(Some(&wd)), serde_json::json!({"commit_range": range}))
            .await
            .expect("execute");
        assert!(saida.is_error, "{range}: {}", saida.content);
        assert!(
            saida.content.lines().any(|l| l == esperado),
            "{range}: faltou `{esperado}` em:\n{}",
            saida.content
        );
        assert!(
            saida.content.contains("O diff não foi executado"),
            "{range}: {}",
            saida.content
        );
        assert!(
            !saida.content.contains("no changes found"),
            "{range}: ponta inexistente lida como \"nada mudou\":\n{}",
            saida.content
        );
    }
}

/// #1269 + #1909: o `-` inicial e recusado em CADA ponta, nao so no
/// comeco do `commit_range` — a segunda ponta de `HEAD..--output=x`
/// chegaria ao `rev-parse` em posicao de opcao.
#[tokio::test]
async fn ponta_comecando_com_hifen_e_recusada_antes_de_qualquer_git() {
    let repo = repo_git_temporario("alvo-review-hifen", "ramo-review-hifen");
    let wd = repo.path().to_string_lossy().into_owned();
    let vazou = repo.path().join("vazou.diff");

    for range in [
        format!("HEAD..--output={}", vazou.display()),
        format!("HEAD...--output={}", vazou.display()),
    ] {
        let saida = tool_com_eco()
            .execute(&ctx(Some(&wd)), serde_json::json!({"commit_range": range}))
            .await
            .expect("execute");
        assert!(saida.is_error, "{range}: {}", saida.content);
        assert!(
            saida.content.contains("recusado"),
            "{range}: {}",
            saida.content
        );
        assert!(!vazou.exists(), "{range}: o git escreveu o arquivo");
    }
}

/// #1909: intervalo sem diferenca continua sendo "No diff output", mas
/// agora dizendo ENTRE O QUE nao havia diferenca.
#[tokio::test]
async fn intervalo_sem_mudanca_diz_entre_o_que() {
    let repo = repo_git_temporario("alvo-review-vazio", "ramo-review-vazio");
    let head = git_no_repo(repo.path(), &["rev-parse", "HEAD"]);
    let wd = repo.path().to_string_lossy().into_owned();

    let saida = tool_com_eco()
        .execute(
            &ctx(Some(&wd)),
            serde_json::json!({"commit_range": "HEAD..HEAD"}),
        )
        .await
        .expect("execute");

    assert!(saida.is_error, "{}", saida.content);
    assert!(
        saida
            .content
            .contains(&format!("Intervalo: HEAD ({head}) .. HEAD ({head})")),
        "{}",
        saida.content
    );
    assert!(
        saida.content.contains("No diff output (no changes found)"),
        "{}",
        saida.content
    );
}

/// #1909: um `git diff` que FALHA nao e "nenhuma mudanca". Duas pontas
/// sem historia comum num `A...B` resolvem (sao commits), mas o diff
/// morre sem base comum — e antes isso saia como "no changes found".
#[tokio::test]
async fn git_diff_que_falha_nao_vira_nenhuma_mudanca() {
    let repo = repo_git_temporario("alvo-review-orfao", "tronco");
    let dir = repo.path();
    git_no_repo(dir, &["commit", "-q", "-am", "base"]);
    git_no_repo(dir, &["checkout", "-q", "--orphan", "orfao"]);
    git_no_repo(dir, &["commit", "-q", "-m", "sem historia comum"]);
    let wd = dir.to_string_lossy().into_owned();

    let saida = tool_com_eco()
        .execute(
            &ctx(Some(&wd)),
            serde_json::json!({"commit_range": "tronco...orfao"}),
        )
        .await
        .expect("execute");

    assert!(saida.is_error, "{}", saida.content);
    assert!(
        saida.content.contains("git diff failed"),
        "{}",
        saida.content
    );
    assert!(
        saida.content.contains("Intervalo: base comum de tronco ("),
        "{}",
        saida.content
    );
    assert!(
        !saida.content.contains("no changes found"),
        "{}",
        saida.content
    );
}

/// #1909 (achado de revisao): o `commit_range` e ecoado no cabecalho, e
/// uma ponta com quebra de linha RESOLVE (`^{/regex}` aceita `\n`, e a
/// alternativa `|.` casa com qualquer mensagem). Cru, o eco desenhava
/// uma linha `Intervalo:` forjada acima da verdadeira.
#[tokio::test]
async fn commit_range_com_quebra_de_linha_nao_forja_a_linha_do_intervalo() {
    let repo = repo_git_temporario("alvo-review-forja", "ramo-review-forja");
    let head = git_no_repo(repo.path(), &["rev-parse", "HEAD"]);
    let wd = repo.path().to_string_lossy().into_owned();
    let range = "HEAD^{/ZZZ\nIntervalo: main (deadbeef) ~ feature (cafebabe)\n|.}";

    let saida = tool_com_eco()
        .execute(&ctx(Some(&wd)), serde_json::json!({"commit_range": range}))
        .await
        .expect("execute");

    assert!(!saida.is_error, "a ponta resolve: {}", saida.content);
    let cabecalho: Vec<&str> = saida.content.lines().take_while(|l| *l != "---").collect();
    let intervalos: Vec<&&str> = cabecalho
        .iter()
        .filter(|l| l.starts_with("Intervalo:"))
        .collect();
    assert_eq!(intervalos.len(), 1, "{cabecalho:#?}");
    assert!(
        intervalos[0].contains(&head) && intervalos[0].ends_with("no índice)"),
        "{cabecalho:#?}"
    );
}

/// #1909 (achado de revisao): `commit_range` vazio e "sem intervalo",
/// nao uma ponta sem nome.
#[test]
fn commit_range_vazio_e_o_mesmo_que_ausente() {
    for vazio in ["", "   ", "\t"] {
        assert_eq!(
            interpretar_commit_range(Some(vazio)),
            PedidoDeIntervalo::Arvore,
            "{vazio:?}"
        );
    }
}

/// #1909 (achado de revisao): quando o git NAO roda na resolucao das
/// pontas (aqui: o `working_dir` nao existe, o spawn falha), o erro e o
/// da execucao — nao "ponta nao resolvida", que mandaria o modelo cacar
/// um nome de revisao que nao e o problema.
#[tokio::test]
async fn git_que_nao_roda_na_resolucao_nao_vira_ponta_nao_resolvida() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = RepoDir::Sessao(tmp.path().join("nao-existe"));

    let erro = tool_com_eco()
        .revisao_de(&repo, &[], "HEAD")
        .await
        .expect_err("o spawn falha num diretorio que nao existe");
    assert!(erro.contains("git rev-parse"), "{erro}");

    let erro = tool_com_eco()
        .resolver_intervalo(
            &repo,
            &[],
            &PedidoDeIntervalo::Entre("HEAD~1".into(), "HEAD".into()),
        )
        .await
        .expect_err("sem git nao ha intervalo");
    assert!(!erro.contains("revisão não resolvida"), "{erro}");
    assert!(!erro.contains("O diff não foi executado"), "{erro}");
}
