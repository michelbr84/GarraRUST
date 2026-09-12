use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use garraia_common::{Error, Result};
use rmcp::model::{CallToolRequestParams, CallToolResponse, ContentBlock};
use serde_json::Value;
use tracing::info;

use super::manager::McpManager;
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

    /// Nome do servidor MCP que expõe esta ferramenta.
    nome_servidor: String,

    /// Manager consultado a cada chamada para obter o peer ATUAL.
    ///
    /// Guardar um `Arc<Peer>` capturado no registro (como antes) quebrava a
    /// ferramenta para sempre após qualquer reconexão: o reconnect troca a
    /// `McpConnection` inteira por uma com peer novo, e o `AgentRuntime` é
    /// imutável depois do boot, então ninguém atualizava a cópia antiga.
    manager: Arc<McpManager>,

    /// Timeout máximo para execução da ferramenta
    timeout: Duration,
}

impl McpTool {
    pub fn new(
        manager: Arc<McpManager>,
        nome_servidor: &str,
        nome_original: String,
        descricao: Option<String>,
        schema_entrada: Value,
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
            nome_servidor: nome_servidor.to_string(),
            manager,
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

        let mut params = CallToolRequestParams::new(self.nome_original.clone());
        if let Some(a) = argumentos {
            params = params.with_arguments(a);
        }

        // Resolve o peer ATUAL a cada chamada (sobrevive a reconexões) e solta
        // o lock de conexões antes de aguardar a resposta.
        let peer = self
            .manager
            .peer_for(&self.nome_servidor)
            .await
            .ok_or_else(|| {
                Error::Mcp(format!(
                    "servidor MCP '{}' desconectado; reconexão automática em andamento",
                    self.nome_servidor
                ))
            })?;

        // Executa com timeout.
        //
        // rmcp 3.x: `Peer::call_tool_once` manda UM `tools/call` e devolve o
        // enum MRTR-aware `CallToolResponse`. Os braços `InputRequired`
        // (SEP-2322: o servidor pede input do usuário antes de completar) e
        // `Task` (SEP-2663: o servidor materializou uma task e quer polling
        // em `tasks/get`) são fail-closed — este bridge não dirige rounds
        // interativos nem ciclos de task; o LLM vê o motivo e decide.
        let resultado = tokio::time::timeout(self.timeout, peer.call_tool_once(params))
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "ferramenta {} excedeu o tempo limite após {:?}",
                    self.nome_completo, self.timeout
                ))
            })?
            .map_err(|e| {
                Error::Mcp(format!(
                    "falha ao chamar '{}' no servidor MCP '{}': {e}",
                    self.nome_original, self.nome_servidor
                ))
            })?;

        let resultado = match resultado {
            CallToolResponse::Complete(resultado) => resultado,
            CallToolResponse::InputRequired(_) => {
                return Err(Error::Mcp(format!(
                    "servidor MCP '{}' pediu input do usuário (SEP-2322 input_required) para '{}'; \
                     este runtime não conduz rodadas interativas — chame a ferramenta com \
                     argumentos completos ou negocie fora do MCP",
                    self.nome_servidor, self.nome_original
                )));
            }
            CallToolResponse::Task(_) => {
                return Err(Error::Mcp(format!(
                    "servidor MCP '{}' materializou a chamada como task (SEP-2663) para '{}'; \
                     polling de tasks/get não é suportado neste runtime",
                    self.nome_servidor, self.nome_original
                )));
            }
            // `CallToolResponse` é `#[non_exhaustive]` (o mesmo contrato do
            // `ContentBlock`): variantes novas da spec caem aqui, fail-closed.
            _ => {
                return Err(Error::Mcp(format!(
                    "resposta desconhecida do servidor MCP '{}' para '{}'; fail-closed",
                    self.nome_servidor, self.nome_original
                )));
            }
        };

        // Converte conteúdos retornados pelo MCP em texto único.
        //
        // rmcp 2.2 removeu a camada `Annotated<RawContent>` (o campo `.raw`) em
        // favor do enum achatado `ContentBlock`, alinhado à spec MCP
        // 2025-11-25. O braço `_` é obrigatório: `ContentBlock` é
        // `#[non_exhaustive]` e ganha variantes novas a cada revisão da spec.
        let mut partes_texto = Vec::new();
        for content in &resultado.content {
            match content {
                ContentBlock::Text(text_content) => {
                    partes_texto.push(text_content.text.clone());
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
