use async_trait::async_trait;
use garraia_common::ssrf::{self, UrlPolicy};
use garraia_common::{Error, Result};
use std::time::Duration;

use super::{Tool, ToolContext, ToolOutput};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024; // 1MB

/// Política de saída da tool `web_fetch`.
///
/// A URL vem de `input["url"]`, ou seja de uma tool call do LLM — dirigível
/// pela mensagem do usuário ou por prompt injection em conteúdo ingerido. É uma
/// fonte remota para todos os efeitos.
///
/// Até 2026-08-29 a única defesa era `esta_bloqueado`, um
/// `url.contains(dominio)` sobre `dominios_bloqueados` — e
/// `bootstrap/mod.rs:454` registra a tool com `None`, então a lista estava
/// **vazia**: sem checagem de esquema, sem checagem de IP, redirects seguidos.
/// `http://169.254.169.254/latest/meta-data/` passava direto. CodeQL classifica
/// como `rust/request-forgery` (9.1, Critical).
///
/// http em texto claro continua permitido (buscar uma página http pública é uso
/// legítimo); quem faz o trabalho é o bloqueio de IP, que recusa loopback,
/// RFC 1918, link-local, CGNAT e afins.
fn fetch_policy() -> UrlPolicy {
    UrlPolicy::http_public(
        Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        concat!("GarraIA/", env!("CARGO_PKG_VERSION"), " web-fetch"),
    )
}

/// Busca conteúdo de uma URL com timeout, limite de tamanho e bloqueio de domínios.
///
/// Não guarda um `reqwest::Client`: cada requisição usa um cliente construído
/// por [`ssrf::pinned_client`], pinado nos endereços já validados para aquela
/// URL. Um cliente compartilhado não teria como carregar esse pinning, que é o
/// que fecha a janela de DNS rebinding.
pub struct WebFetchTool {
    dominios_bloqueados: Vec<String>,
}

impl WebFetchTool {
    pub fn new(dominios_bloqueados: Option<Vec<String>>) -> Self {
        Self {
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

        // Guard de SSRF compartilhado: esquema, bloqueio de IP e DNS pinning.
        // Roda ANTES de qualquer conexão. O cliente pinado também não segue
        // redirects — senão um host permitido redirecionaria para um bloqueado
        // depois da checagem.
        let vetted = match ssrf::vet_url(url, &fetch_policy()) {
            Ok(v) => v,
            Err(e) => return Ok(ToolOutput::error(format!("URL recusada: {e}"))),
        };
        let client = match ssrf::pinned_client(&vetted, &fetch_policy()) {
            Ok(c) => c,
            Err(e) => return Ok(ToolOutput::error(format!("URL recusada: {e}"))),
        };

        let response = client
            .get(vetted.url.clone())
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

    /// Regressão do SSRF: o guard tem que recusar antes de qualquer conexão.
    /// Nenhum destes casos abre socket — `vet_url` falha no parse, no esquema ou
    /// no bloqueio de IP, então o teste é determinístico e offline.
    #[test]
    fn recusa_alvos_internos_e_esquemas_perigosos() {
        let tool = WebFetchTool::new(None);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        for url in [
            "http://169.254.169.254/latest/meta-data/",
            "http://127.0.0.1:11434/api/tags",
            "http://[::1]/x",
            "http://[::ffff:127.0.0.1]/x",
            "http://10.0.0.1/x",
            "file:///etc/passwd",
            "gopher://evil.test/_x",
            "nao e uma url",
        ] {
            let out = rt
                .block_on(tool.execute(&ctx, serde_json::json!({ "url": url })))
                .expect("tool must not error out");
            assert!(
                out.is_error,
                "{url} deveria ter sido recusada, veio: {}",
                out.content
            );
        }
    }

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
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };
        let result = rt.block_on(tool.execute(&ctx, serde_json::json!({})));
        assert!(result.is_err());
    }
}
