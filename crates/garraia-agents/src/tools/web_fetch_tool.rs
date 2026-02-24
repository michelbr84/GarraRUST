use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::time::Duration;

use super::{Tool, ToolContext, ToolOutput};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024; // 1MB

/// Busca conteúdo de uma URL com timeout, limite de tamanho e bloqueio de domínios.
pub struct WebFetchTool {
    client: reqwest::Client,
    dominios_bloqueados: Vec<String>,
}

impl WebFetchTool {
    pub fn new(dominios_bloqueados: Option<Vec<String>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();

        Self {
            client,
            dominios_bloqueados: dominios_bloqueados.unwrap_or_default(),
        }
    }

    fn esta_bloqueado(&self, url: &str) -> bool {
        self.dominios_bloqueados
            .iter()
            .any(|dominio| url.contains(dominio))
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Busca o conteúdo de uma página web na URL informada. Retorna o corpo da resposta como texto."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "A URL a ser buscada"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'url' ausente".into()))?;

        if self.esta_bloqueado(url) {
            return Ok(ToolOutput::error("domínio bloqueado".to_string()));
        }

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("falha ao buscar URL: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolOutput::error(format!("HTTP {status}")));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| Error::Agent(format!("falha ao ler corpo da resposta: {e}")))?;

        if bytes.len() > MAX_RESPONSE_BYTES {
            let truncado = String::from_utf8_lossy(&bytes[..MAX_RESPONSE_BYTES]);
            return Ok(ToolOutput::success(format!(
                "{}\n... (resposta truncada em {} bytes)",
                truncado, MAX_RESPONSE_BYTES
            )));
        }

        let texto = String::from_utf8_lossy(&bytes);
        Ok(ToolOutput::success(texto.into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloqueia_dominios() {
        let tool = WebFetchTool::new(Some(vec!["evil.com".to_string()]));
        assert!(tool.esta_bloqueado("https://evil.com/path"));
        assert!(!tool.esta_bloqueado("https://good.com/path"));
    }

    #[test]
    fn retorna_erro_quando_url_ausente() {
        let tool = WebFetchTool::new(None);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
        };
        let result = rt.block_on(tool.execute(&ctx, serde_json::json!({})));
        assert!(result.is_err());
    }
}