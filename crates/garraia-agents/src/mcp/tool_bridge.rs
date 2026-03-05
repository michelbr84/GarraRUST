use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use garraia_common::{Error, Result};
use rmcp::model::{CallToolRequestParams, RawContent};
use rmcp::service::{Peer, RoleClient};
use serde_json::Value;
use tracing::info;

use crate::tools::{Tool, ToolContext, ToolOutput};

/// Faz a ponte entre uma ferramenta exposta por um servidor MCP
/// e o trait `Tool` utilizado pelo Garraia.
pub struct McpTool {
    /// Nome com namespace: "nome_servidor.nome_ferramenta"
    nome_completo: String,

    /// Nome original da ferramenta registrada no servidor MCP
    nome_original: String,

    /// Descrição da ferramenta (vinda do servidor MCP)
    descricao: String,

    /// JSON Schema de entrada da ferramenta
    schema_entrada: Value,

    /// Referência compartilhada para o peer MCP
    peer: Arc<Peer<RoleClient>>,

    /// Timeout máximo para execução da ferramenta
    timeout: Duration,
}

impl McpTool {
    pub fn new(
        nome_servidor: &str,
        nome_original: String,
        descricao: Option<String>,
        schema_entrada: Value,
        peer: Arc<Peer<RoleClient>>,
        timeout: Duration,
    ) -> Self {
        Self {
            // Use "__" instead of "." — OpenAI/Anthropic APIs reject dots in tool names
            // (pattern: ^[a-zA-Z0-9_-]+$). The MCP call itself uses `nome_original`.
            nome_completo: format!("{nome_servidor}__{nome_original}"),
            descricao: descricao.unwrap_or_else(|| {
                format!("Ferramenta MCP {nome_original} do servidor {nome_servidor}")
            }),
            nome_original,
            schema_entrada,
            peer,
            timeout,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.nome_completo
    }

    fn description(&self) -> &str {
        &self.descricao
    }

    fn input_schema(&self) -> Value {
        self.schema_entrada.clone()
    }

    async fn execute(&self, context: &ToolContext, input: Value) -> Result<ToolOutput> {
        // GAR-190: audit log — every MCP tool invocation is recorded.
        let input_keys: Vec<&str> = match &input {
            Value::Object(m) => m.keys().map(|k| k.as_str()).collect(),
            _ => vec![],
        };
        info!(
            tool = %self.nome_completo,
            session = %context.session_id,
            input_keys = ?input_keys,
            "mcp tool call"
        );

        // Converte a entrada para o formato esperado pelo MCP
        let argumentos = match input {
            Value::Object(map) => Some(map),
            Value::Null => None,
            outro => {
                let mut map = serde_json::Map::new();
                map.insert("input".to_string(), outro);
                Some(map)
            }
        };

        let params = CallToolRequestParams {
            name: Cow::Owned(self.nome_original.clone()),
            arguments: argumentos,
            meta: None,
            task: None,
        };

        // Executa com timeout
        let resultado = tokio::time::timeout(self.timeout, self.peer.call_tool(params))
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "ferramenta {} excedeu o tempo limite após {:?}",
                    self.nome_completo, self.timeout
                ))
            })?
            .map_err(|e| Error::Mcp(format!("falha ao chamar call_tool: {e}")))?;

        // Converte conteúdos retornados pelo MCP em texto único
        let mut partes_texto = Vec::new();
        for content in &resultado.content {
            match &content.raw {
                RawContent::Text(text_content) => {
                    partes_texto.push(text_content.text.to_string());
                }
                _ => {
                    // Conteúdo não textual recebe placeholder
                    partes_texto.push("[conteúdo não textual]".to_string());
                }
            }
        }

        let texto_saida = partes_texto.join("\n");
        let eh_erro = resultado.is_error.unwrap_or(false);

        if eh_erro {
            Ok(ToolOutput::error(texto_saida))
        } else {
            Ok(ToolOutput::success(texto_saida))
        }
    }
}
