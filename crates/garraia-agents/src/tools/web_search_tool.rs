use async_trait::async_trait;
use garraia_common::{Error, Result};
use serde::Deserialize;
use std::time::Duration;

use super::{Tool, ToolContext, ToolOutput};

const SEARCH_TIMEOUT_SECS: u64 = 15;
const DEFAULT_COUNT: u64 = 5;
const MAX_COUNT: u64 = 10;

/// Realiza buscas na web utilizando a API do Brave Search.
pub struct WebSearchTool {
    client: reqwest::Client,
    api_key: String,
}

impl WebSearchTool {
    pub fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();

        Self { client, api_key }
    }
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

        let response = self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha na requisição de busca: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolOutput::error(format!(
                "Erro na API Brave Search: HTTP {status}"
            )));
        }

        let body: BraveSearchResponse = response
            .json()
            .await
            .map_err(|e| Error::Agent(format!("falha ao interpretar resposta da busca: {e}")))?;

        let results = match body.web {
            Some(web) if !web.results.is_empty() => web.results,
            _ => return Ok(ToolOutput::error("Nenhum resultado encontrado.")),
        };

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

        Ok(ToolOutput::success(output.trim_end()))
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
        }
    }

    #[test]
    fn retorna_erro_quando_query_ausente() {
        let tool = WebSearchTool::new("test-key".into());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(&contexto_teste(), serde_json::json!({})));
        assert!(result.is_err());
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
            BraveWebResult {
                title: "Rust Lang".into(),
                url: "https://www.rust-lang.org".into(),
                description: "Uma linguagem de programação de sistemas.".into(),
            },
            BraveWebResult {
                title: "Crates.io".into(),
                url: "https://crates.io".into(),
                description: "O repositório de pacotes Rust.".into(),
            },
        ];

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
        let output = output.trim_end();

        assert!(output.starts_with("1. **Rust Lang**"));
        assert!(output.contains("2. **Crates.io**"));
        assert!(output.contains("https://crates.io"));
        assert!(output.contains("O repositório de pacotes Rust."));
    }
}
