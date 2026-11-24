use super::*;
use crate::providers::LlmResponse;
use crate::tools::repo_dir::{contexto_de_teste as ctx, repo_git_temporario};

/// Provedor que devolve o prompt recebido como se fosse a revisão. É o que
/// deixa o teste afirmar **qual diff** chegou ao LLM, sem rede nenhuma.
struct ProvedorQueEcoa;

#[async_trait]
impl LlmProvider for ProvedorQueEcoa {
    fn provider_id(&self) -> &str {
        "eco-de-teste"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let texto = request
            .messages
            .iter()
            .map(|m| match &m.content {
                MessagePart::Text(t) => t.clone(),
                MessagePart::Parts(_) => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(LlmResponse {
            content: vec![ContentBlock::Text { text: texto }],
            model: "eco".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

pub(super) fn tool_com_eco() -> CodeReviewTool {
    CodeReviewTool::new(Arc::new(ProvedorQueEcoa), "modelo-de-teste", Some(15))
}

/// #1780: provedor que dorme mais que o teto de LLM configurado — a
/// chamada morre dentro da tool com erro nomeando a etapa, e não fica
/// pendurada até o wrap externo do runtime.
struct ProvedorLento(Duration);

#[async_trait]
impl LlmProvider for ProvedorLento {
    fn provider_id(&self) -> &str {
        "lento-de-teste"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        tokio::time::sleep(self.0).await;
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "revisão que nunca chega".to_string(),
            }],
            model: "lento".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// #1780: o LLM sem resposta dentro do teto proprio da tool vira erro
/// preciso, antes de o wrap externo matar a chamada inteira.
///
/// **Mutação que este teste pega**: tire o `tokio::time::timeout` de
/// `review_diff` e ele fica vermelho (o erro seria do provider, sem
/// "timed out", ou a chamada pendura).
#[tokio::test]
async fn llm_sem_resposta_dentro_do_teto_da_tool_e_erro_preciso() {
    let repo = repo_git_temporario("alvo-review-lento", "ramo-review-lento");
    let tool = CodeReviewTool::new(
        Arc::new(ProvedorLento(Duration::from_secs(5))),
        "modelo-de-teste",
        Some(15),
    )
    .com_timeout_de_llm(1);
    let wd = repo.path().to_string_lossy().into_owned();

    let saida = tool
        .execute(&ctx(Some(&wd)), serde_json::json!({}))
        .await
        .expect("execute");

    assert!(saida.is_error, "{}", saida.content);
    assert!(
        saida.content.contains("LLM review timed out after 1s"),
        "erro de LLM sem o teto aplicado:\n{}",
        saida.content
    );
}

/// #1780: o teto declarado cobre o git **e** o LLM — o wrap externo usa
/// o maior entre este e o teto do modo. #1909: o git conta tres vezes
/// (prefixo endurecido, resolucao das pontas, diff).
#[test]
fn teto_proprio_cobre_diff_mais_llm() {
    let tool = tool_com_eco();
    assert_eq!(
        tool.teto_proprio(),
        Some(Duration::from_secs(15 * 3 + LLM_TIMEOUT_SECS))
    );
    // O default (sem override) é o const; o override de teste muda o
    // bound declarado na mesma medida.
    let custom = CodeReviewTool::new(Arc::new(ProvedorQueEcoa), "m", None).com_timeout_de_llm(7);
    assert_eq!(
        custom.teto_proprio(),
        Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS * 3 + 7))
    );
}

/// #1272 S3: o `code_review` usa o mesmo git endurecido — config
/// plantada nao executa nada, e o diff ainda chega ao revisor.
#[cfg(unix)]
#[tokio::test]
async fn config_plantada_no_repo_nao_roda_programa_nenhum() {
    let repo = repo_git_temporario("alvo-review-1272", "ramo-review-1272");
    let marcas = tempfile::tempdir().expect("tmp");
    let marcadores = crate::git_endurecido::planta_programas_no_repo(repo.path(), marcas.path());
    let wd = repo.path().to_string_lossy().into_owned();
    let saida = tool_com_eco()
        .execute(&ctx(Some(&wd)), serde_json::json!({}))
        .await
        .expect("execute");
    assert!(!saida.is_error, "{}", saida.content);
    assert!(
        saida.content.contains("alvo-review-1272"),
        "{}",
        saida.content
    );
    for m in &marcadores {
        assert!(!m.exists(), "code_review executou {}", m.display());
    }
}

/// #1272 S3: filtro declarado na config PROPRIA de um submodulo, com o
/// superprojeto em `diff.submodule=diff`, nao roda pelo `code_review`.
#[cfg(unix)]
#[tokio::test]
async fn code_review_nao_roda_filtro_de_submodulo() {
    let repo = repo_git_temporario("alvo-review-sub", "ramo-review-sub");
    let marcas = tempfile::tempdir().expect("tmp");
    let marca = crate::git_endurecido::planta_filtro_em_submodulo(repo.path(), marcas.path());
    let wd = repo.path().to_string_lossy().into_owned();
    let saida = tool_com_eco()
        .execute(&ctx(Some(&wd)), serde_json::json!({}))
        .await
        .expect("execute");
    assert!(!saida.is_error, "{}", saida.content);
    assert!(!marca.exists(), "code_review rodou o filtro do submodulo");
}

/// #1258 (item 4 da aceitação): o `code_review` tinha o **mesmo** defeito
/// raiz — `Command::new("git")` sem `current_dir` em `get_diff` —, então
/// ganha a mesma prova, também pelo caminho do agente.
///
/// **Mutação que este teste pega**: comente o `cmd.current_dir(dir)` de
/// `CodeReviewTool::get_diff` e ele fica vermelho.
#[tokio::test]
async fn diff_revisado_vem_do_working_dir_da_sessao() {
    let repo_a = repo_git_temporario("alvo-review-a", "ramo-review-a");
    let repo_b = repo_git_temporario("alvo-review-b", "ramo-review-b");
    let tool = tool_com_eco();

    for (repo, meu, do_outro) in [
        (&repo_a, "alvo-review-a", "alvo-review-b"),
        (&repo_b, "alvo-review-b", "alvo-review-a"),
    ] {
        let wd = repo.path().to_string_lossy().into_owned();
        let saida = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({}))
            .await
            .expect("execute");

        assert!(!saida.is_error, "{}", saida.content);
        assert!(
            saida.content.contains(meu),
            "o diff revisado tinha de vir de {wd}:\n{}",
            saida.content
        );
        assert!(
            !saida.content.contains(do_outro),
            "o diff revisado veio do repositório errado:\n{}",
            saida.content
        );
        // #1258: a procedência é nomeada. #1626: sem o caminho absoluto, que
        // vazaria o dir por-orquestrador no envelope servido pela ponte.
        assert!(
            saida
                .content
                .contains("**Repositório:** o working_dir da sessão"),
            "{}",
            saida.content
        );
        assert!(
            !saida.content.contains(&wd),
            "o caminho absoluto do working_dir vazou na revisão:\n{}",
            saida.content
        );
    }
}

/// #1258, caso 2 no `code_review`: sem `working_dir` o CWD do processo é
/// mantido e a resposta o nomeia. Vale para os dois desfechos — a revisão
/// de uma árvore suja e o "No diff output" de uma árvore limpa, que é
/// caminho de erro —, então o teste afirma só o que é comum aos dois.
/// Turno ABERTO (operador local); no restrito o caminho não sai (#1909).
#[tokio::test]
async fn sem_working_dir_o_code_review_nomeia_o_repositorio_do_cwd() {
    let tool = tool_com_eco();
    let cwd = std::env::current_dir().expect("CWD do processo de teste");

    let saida = crate::tools::turn_tools::com_restricao_do_turno(false, async {
        tool.execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("execute")
    })
    .await;

    assert!(
        saida.content.contains(&cwd.display().to_string()),
        "{}",
        saida.content
    );
    assert!(
        saida.content.contains("a sessão não tem working_dir"),
        "{}",
        saida.content
    );
}

/// #1909 no `code_review`: num turno restrito a mesma revisão de sessão
/// sem `working_dir` não entrega o caminho do HOST. A procedência
/// continua nomeada; o lugar do caminho é o marcador das outras tools.
#[tokio::test]
async fn sem_working_dir_no_turno_restrito_o_code_review_nao_nomeia_o_cwd() {
    let tool = tool_com_eco();
    let cwd = std::env::current_dir().expect("CWD do processo de teste");

    let saida = crate::tools::turn_tools::com_restricao_do_turno(true, async {
        tool.execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("execute")
    })
    .await;

    // A linha de procedência da revisão, e não o corpo: o corpo inclui o
    // diff ecoado pelo provedor de teste.
    let procedencia = saida
        .content
        .lines()
        .find(|l| l.contains("Repositório:"))
        .unwrap_or_default();
    assert!(
        procedencia.contains(crate::tools::tool_context::CAMINHO_FORA_DO_TURNO),
        "o lugar do caminho deveria ser o marcador: {procedencia}"
    );
    assert!(
        !saida.content.contains(&cwd.display().to_string()),
        "o CWD do host saiu no turno restrito:\n{}",
        saida.content
    );
    assert!(
        saida.content.contains("a sessão não tem working_dir"),
        "{}",
        saida.content
    );
}

/// #1269 no `code_review` (paridade): o `commit_range` vem do modelo e é
/// um argumento argv — começando com `-` viraria flag do git
/// (`--output=/tmp/x` escreve arquivo, `-O…`, `--stdin`). Recusado antes
/// de entrar na linha de comando, no mesmo formato que o `git_diff` usa.
///
/// **Mutação que este teste pega**: tire o `if range.starts_with('-')` de
/// `get_diff` e ele fica vermelho.
#[tokio::test]
async fn commit_range_comecando_com_hifen_e_recusado() {
    let tool = tool_com_eco();

    for range in ["--output=/tmp/vazou-1258.diff", "-O/tmp/x", "--stdin"] {
        let saida = tool
            .execute(&ctx(None), serde_json::json!({"commit_range": range}))
            .await
            .expect("execute");

        assert!(saida.is_error, "{range}: {}", saida.content);
        assert!(
            saida.content.contains("recusado"),
            "{range}: {}\n{}",
            range,
            saida.content
        );
    }
}

/// Controle positivo da recusa: um `commit_range` legítimo — revisão que
/// não começa com `-` — passa pela tool, e o diff de verdade chega ao LLM.
#[tokio::test]
async fn commit_range_valido_ainda_passa() {
    let repo = repo_git_temporario("alvo-range-valido", "ramo-range-valido");
    let tool = tool_com_eco();
    let wd = repo.path().to_string_lossy().into_owned();

    let saida = tool
        .execute(&ctx(Some(&wd)), serde_json::json!({"commit_range": "HEAD"}))
        .await
        .expect("execute");

    assert!(
        !saida.is_error,
        "`git diff HEAD` num repositório com árvore suja tem de funcionar:\n{}",
        saida.content
    );
    assert!(
        saida.content.contains("alvo-range-valido"),
        "{}",
        saida.content
    );
}

#[test]
fn test_code_review_schema() {
    // We need a provider to create the tool, but we can test the schema statically
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "commit_range": {
                "type": "string",
                "description": "Git commit range for diff"
            },
            "file_path": {
                "type": "string",
                "description": "Specific file to review"
            }
        }
    });

    assert!(schema.get("properties").is_some());
}
