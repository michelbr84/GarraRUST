use async_trait::async_trait;
use garraia_common::ssrf::{self, IpScope, UrlPolicy};
use garraia_common::{Error, Result};
use serde::Deserialize;
use std::time::Duration;

use super::{Tool, ToolContext, ToolOutput};

const SEARCH_TIMEOUT_SECS: u64 = 15;
const DEFAULT_COUNT: u64 = 5;
const MAX_COUNT: u64 = 10;

/// De onde a `web_search` tira os resultados (#1034).
///
/// Ate o #1034 so havia o Brave, e a tool so era registrada com a chave dele
/// — sem chave, o agente nao tinha busca nenhuma. O SearXNG e o caminho sem
/// chave: um meta-buscador self-hosted que agrega DDG, Google, Bing e afins.
#[derive(Debug, Clone)]
pub enum SearchBackend {
    /// Brave Search API: host fixo, chave no header.
    Brave { api_key: String },
    /// SearXNG (`GET {base_url}/search?format=json`). Sem chave. A URL e do
    /// operador, e por isso passa pelo guard de SSRF com [`IpScope::AllowPrivate`]:
    /// loopback e LAN sao o caso de uso, link-local e metadata de nuvem nao.
    Searxng { base_url: String },
}

impl SearchBackend {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Brave { .. } => "brave",
            Self::Searxng { .. } => "searxng",
        }
    }
}

/// Realiza buscas na web pelo backend configurado.
pub struct WebSearchTool {
    client: reqwest::Client,
    backend: SearchBackend,
}

impl WebSearchTool {
    /// Brave, como sempre foi. Mantido para os chamadores existentes.
    pub fn new(api_key: String) -> Self {
        Self::with_backend(SearchBackend::Brave { api_key })
    }

    pub fn with_backend(backend: SearchBackend) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();

        Self { client, backend }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }
}

/// Um resultado ja no formato comum aos backends — o modelo ve o mesmo
/// schema seja qual for a origem.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResult {
    title: String,
    url: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct BraveSearchResponse {
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    results: Vec<BraveWebResult>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResult {
    title: String,
    url: String,
    description: String,
}

impl From<BraveWebResult> for SearchResult {
    fn from(r: BraveWebResult) -> Self {
        Self {
            title: r.title,
            url: r.url,
            description: r.description,
        }
    }
}

/// Resposta do `/search?format=json` do SearXNG — so os campos usados.
/// `content` e a descricao; falta em resultado de imagem ou mapa, por isso
/// `default`.
#[derive(Debug, Deserialize)]
struct SearxngResponse {
    #[serde(default)]
    results: Vec<SearxngResult>,
}

#[derive(Debug, Deserialize)]
struct SearxngResult {
    #[serde(default)]
    title: String,
    url: String,
    #[serde(default)]
    content: String,
}

impl From<SearxngResult> for SearchResult {
    fn from(r: SearxngResult) -> Self {
        Self {
            title: r.title,
            url: r.url,
            description: r.content,
        }
    }
}

/// O que uma busca devolveu: resultados, ou um erro que o modelo deve ver
/// como resposta da tool (HTTP fora de 2xx, URL recusada) em vez de como
/// falha do turno.
enum Fetched {
    Results(Vec<SearchResult>),
    Failed(String),
}

/// Politica do SearXNG: http e https (self-hosted na LAN costuma ser http),
/// loopback e faixas privadas liberadas — sao o caso de uso —, link-local,
/// CGNAT, multicast e unspecified continuam bloqueados (threat-model §5.6).
/// A URL vem da config, mas a config pode vir de `PATCH /api/settings`, e o
/// guard e o que impede um `169.254.169.254` de virar alvo de busca.
fn searxng_policy() -> UrlPolicy {
    UrlPolicy::http_public(
        Duration::from_secs(SEARCH_TIMEOUT_SECS),
        concat!("GarraIA/", env!("CARGO_PKG_VERSION"), " web-search"),
    )
    .with_ip_scope(IpScope::AllowPrivate)
}

/// A lista numerada que o modelo recebe — igual para os dois backends.
fn format_results(results: &[SearchResult]) -> String {
    let mut output = String::new();
    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!(
            "{}. **{}** — {}\n   {}\n\n",
            i + 1,
            result.title,
            result.url,
            result.description,
        ));
    }
    output.trim_end().to_string()
}

impl WebSearchTool {
    async fn brave(&self, api_key: &str, query: &str, count: u64) -> Result<Fetched> {
        let response = self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", api_key)
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha na requisição de busca: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Ok(Fetched::Failed(format!(
                "Erro na API Brave Search: HTTP {status}"
            )));
        }

        let body: BraveSearchResponse = response
            .json()
            .await
            .map_err(|e| Error::Agent(format!("falha ao interpretar resposta da busca: {e}")))?;

        Ok(Fetched::Results(
            body.web
                .map(|w| w.results.into_iter().map(Into::into).collect())
                .unwrap_or_default(),
        ))
    }

    /// Nao usa `self.client`: cada busca constroi um cliente pinado nos
    /// enderecos que `vet_url` validou, como o `web_fetch` — e o que fecha a
    /// janela de DNS rebinding para uma URL que o operador pode trocar.
    async fn searxng(&self, base_url: &str, query: &str, count: u64) -> Result<Fetched> {
        let endpoint = format!("{}/search", base_url.trim_end_matches('/'));
        let policy = searxng_policy();
        let vetted = match ssrf::vet_url(&endpoint, &policy) {
            Ok(v) => v,
            Err(e) => return Ok(Fetched::Failed(format!("URL do SearXNG recusada: {e}"))),
        };
        let client = match ssrf::pinned_client(&vetted, &policy) {
            Ok(c) => c,
            Err(e) => return Ok(Fetched::Failed(format!("URL do SearXNG recusada: {e}"))),
        };

        let response = client
            .get(vetted.url.clone())
            .header("Accept", "application/json")
            .query(&[("q", query), ("format", "json")])
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha na requisição de busca: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            // 403 e o que o SearXNG devolve quando `json` nao esta em
            // `search.formats` — a causa mais comum de "nao funciona".
            let hint = if status.as_u16() == 403 {
                " — a instancia precisa de `json` em `search.formats` (settings.yml)"
            } else {
                ""
            };
            return Ok(Fetched::Failed(format!(
                "Erro no SearXNG: HTTP {status}{hint}"
            )));
        }

        let body: SearxngResponse = response
            .json()
            .await
            .map_err(|e| Error::Agent(format!("falha ao interpretar resposta da busca: {e}")))?;

        // O SearXNG nao aceita `count`; devolve a pagina inteira e o corte e
        // aqui, para o modelo receber o mesmo tamanho dos dois backends.
        Ok(Fetched::Results(
            body.results
                .into_iter()
                .take(usize::try_from(count).unwrap_or(DEFAULT_COUNT as usize))
                .map(Into::into)
                .collect(),
        ))
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Realiza uma busca na web e retorna os principais resultados com título, URL e descrição."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Consulta de busca"
                },
                "count": {
                    "type": "number",
                    "description": "Quantidade de resultados (1-10, padrão 5)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'query' ausente".into()))?;

        let count = input
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_COUNT)
            .clamp(1, MAX_COUNT);

        let fetched = match &self.backend {
            SearchBackend::Brave { api_key } => self.brave(api_key, query, count).await?,
            SearchBackend::Searxng { base_url } => self.searxng(base_url, query, count).await?,
        };

        let results = match fetched {
            Fetched::Failed(msg) => return Ok(ToolOutput::error(msg)),
            Fetched::Results(r) if r.is_empty() => {
                return Ok(ToolOutput::error("Nenhum resultado encontrado."));
            }
            Fetched::Results(r) => r,
        };

        Ok(ToolOutput::success(format_results(&results)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contexto_teste() -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        }
    }

    #[test]
    fn retorna_erro_quando_query_ausente() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        for tool in [
            WebSearchTool::new("test-key".into()),
            WebSearchTool::with_backend(SearchBackend::Searxng {
                base_url: "http://127.0.0.1:8081".into(),
            }),
        ] {
            let result = rt.block_on(tool.execute(&contexto_teste(), serde_json::json!({})));
            assert!(result.is_err(), "{}", tool.backend_name());
        }
    }

    #[test]
    fn count_limita_ao_maximo() {
        let count: u64 = 50;
        assert_eq!(count.clamp(1, MAX_COUNT), MAX_COUNT);
    }

    #[test]
    fn count_limita_ao_minimo() {
        let count: u64 = 0;
        assert_eq!(count.clamp(1, MAX_COUNT), 1);
    }

    #[test]
    fn formata_resultados_corretamente() {
        let results = [
            SearchResult {
                title: "Rust Lang".into(),
                url: "https://www.rust-lang.org".into(),
                description: "Uma linguagem de programação de sistemas.".into(),
            },
            SearchResult {
                title: "Crates.io".into(),
                url: "https://crates.io".into(),
                description: "O repositório de pacotes Rust.".into(),
            },
        ];
        let output = format_results(&results);

        assert!(output.starts_with("1. **Rust Lang**"));
        assert!(output.contains("2. **Crates.io**"));
        assert!(output.contains("https://crates.io"));
        assert!(output.contains("O repositório de pacotes Rust."));
        assert!(!output.ends_with('\n'));
    }

    /// #1034: o JSON do SearXNG (`title`, `url`, `content`, mais campos que
    /// nao usamos) vira o mesmo `SearchResult` do Brave — `content` e a
    /// descricao, e um resultado sem `content` nao derruba o parse.
    #[test]
    fn searxng_json_mapeia_content_para_description() {
        let body = r#"{
            "query": "rust",
            "number_of_results": 2,
            "results": [
                {"title": "Rust", "url": "https://www.rust-lang.org", "content": "A language.", "engines": ["duckduckgo"], "score": 9.1},
                {"title": "Sem descricao", "url": "https://example.org"}
            ],
            "suggestions": []
        }"#;
        let parsed: SearxngResponse = serde_json::from_str(body).expect("json do searxng");
        let results: Vec<SearchResult> = parsed.results.into_iter().map(Into::into).collect();
        assert_eq!(
            results,
            vec![
                SearchResult {
                    title: "Rust".into(),
                    url: "https://www.rust-lang.org".into(),
                    description: "A language.".into(),
                },
                SearchResult {
                    title: "Sem descricao".into(),
                    url: "https://example.org".into(),
                    description: String::new(),
                },
            ]
        );
    }

    /// A URL do SearXNG e do operador, entao loopback e LAN passam; o que
    /// nunca e alvo legitimo (metadata de nuvem em link-local, esquema que
    /// nao e http) continua recusado antes de qualquer conexao. Offline:
    /// `vet_url` decide no parse ou no bloqueio de IP.
    #[test]
    fn searxng_policy_libera_loopback_e_bloqueia_link_local() {
        let policy = searxng_policy();
        assert!(ssrf::vet_url("http://127.0.0.1:8081/search", &policy).is_ok());
        assert!(ssrf::vet_url("http://192.168.1.10:8080/search", &policy).is_ok());
        assert!(ssrf::vet_url("http://169.254.169.254/search", &policy).is_err());
        assert!(ssrf::vet_url("file:///etc/passwd", &policy).is_err());
        assert!(ssrf::vet_url("ftp://127.0.0.1/search", &policy).is_err());
    }

    /// Uma base recusada vira resposta de erro da tool, nao erro do turno —
    /// o modelo fica sabendo e segue sem busca.
    #[test]
    fn base_url_recusada_vira_erro_da_tool() {
        let tool = WebSearchTool::with_backend(SearchBackend::Searxng {
            base_url: "http://169.254.169.254".into(),
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let out = rt
            .block_on(tool.execute(&contexto_teste(), serde_json::json!({"query": "x"})))
            .expect("erro da tool, nao do turno");
        assert!(out.is_error, "{out:?}");
        assert!(out.content.contains("recusada"), "{}", out.content);
    }

    #[test]
    fn backend_name_diz_qual_esta_ativo() {
        assert_eq!(WebSearchTool::new("k".into()).backend_name(), "brave");
        assert_eq!(
            WebSearchTool::with_backend(SearchBackend::Searxng {
                base_url: "http://127.0.0.1:8081".into()
            })
            .backend_name(),
            "searxng"
        );
    }
}
