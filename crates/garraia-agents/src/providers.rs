use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};

/// Trait para integrações com provedores de LLM (Anthropic, OpenAI, Ollama, etc.).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Identificador do provedor (ex: "anthropic", "openai", "ollama").
    fn provider_id(&self) -> &str;

    /// Envia uma requisição de completion e retorna a resposta.
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse>;

    /// Envia uma requisição de completion em modo streaming,
    /// retornando eventos conforme são recebidos.
    /// A implementação padrão retorna erro indicando que streaming não é suportado.
    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Err(garraia_common::Error::Agent(format!(
            "provedor {} não suporta streaming",
            self.provider_id()
        )))
    }

    /// Retorna o modelo padrão configurado para o provedor, se conhecido.
    fn configured_model(&self) -> Option<&str> {
        None
    }

    /// Retorna a lista de modelos disponíveis para este provedor.
    async fn available_models(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Verifica se o provedor está disponível e corretamente configurado.
    async fn health_check(&self) -> Result<bool>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: MessagePart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessagePart {
    Text(String),
    Parts(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { url: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Eventos emitidos durante uma completion em modo streaming.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Um trecho incremental de texto gerado.
    TextDelta(String),
    /// Início de um bloco de uso de ferramenta.
    ToolUseStart {
        index: usize,
        id: String,
        name: String,
    },
    /// Fragmento parcial de JSON de entrada para um bloco de ferramenta.
    InputJsonDelta(String),
    /// Finalização de um bloco de conteúdo.
    ContentBlockStop { index: usize },
    /// Indica que a mensagem está sendo finalizada, incluindo metadados.
    MessageDelta {
        stop_reason: Option<String>,
        usage: Option<Usage>,
    },
    /// Indica que o streaming foi concluído.
    MessageStop,
}

// ── #1249: classificacao de falha de envio ───────────────────────────────────

/// `true` quando o `reqwest::Error` diz que a requisicao **nao chegou a ter
/// resposta**: conexao recusada, DNS que nao resolve, timeout, falha ao
/// enviar.
///
/// Fora de proposito:
/// - `is_builder()` — URL/cliente mal formado e bug de configuracao nosso, e
///   tentar outro provider esconderia o bug;
/// - `is_status()` — ja houve resposta, e a politica dela (429/5xx com
///   backoff) e a de sempre;
/// - `is_body()` / `is_decode()` — a conexao existiu e quebrou no meio do
///   corpo, caso do #1176, tratado no consumidor do stream.
///
/// A leitura e feita aqui, onde o tipo do `reqwest` ainda existe. Depois do
/// `format!` sobra texto, e texto de erro de dependencia muda de versao para
/// versao — era exatamente esse o defeito que a issue descreve.
pub(crate) fn falha_de_transporte(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || e.is_request()
}

/// Erro de envio de requisicao a um provider, com a **classe** preservada.
///
/// `contexto` e o prefixo que o provider ja usava ("openai request failed"),
/// mantido palavra por palavra: o cartao de erro da CLI classifica pela frase
/// interna do `reqwest`, e nao pelo prefixo do enum.
pub(crate) fn erro_de_envio(contexto: &str, e: &reqwest::Error) -> Error {
    let msg = format!("{contexto}: {e}");
    if falha_de_transporte(e) {
        Error::Transport(msg)
    } else {
        Error::Agent(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Porta fechada em loopback: o unico erro de rede que um teste pode
    /// provocar sem depender de rede de verdade.
    async fn erro_de_conexao_recusada() -> reqwest::Error {
        // Abre e fecha para descobrir uma porta que ninguem esta servindo.
        let porta = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind efemero");
            l.local_addr().expect("addr").port()
        };
        reqwest::Client::new()
            .get(format!("http://127.0.0.1:{porta}/"))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .expect_err("ninguem esta escutando nessa porta")
    }

    #[tokio::test]
    async fn conexao_recusada_e_transporte() {
        let e = erro_de_conexao_recusada().await;
        assert!(
            falha_de_transporte(&e),
            "connect/timeout tem de ser transporte; veio: {e}"
        );
        assert!(
            matches!(
                erro_de_envio("openai request failed", &e),
                Error::Transport(_)
            ),
            "a classe precisa sobreviver ao format!"
        );
    }

    #[test]
    fn url_invalida_nao_e_transporte() {
        // `is_builder()`: nada saiu da maquina, e cair para outro provider
        // esconderia um bug de configuracao nosso.
        let e = reqwest::Client::new()
            .get("http://[::1")
            .build()
            .expect_err("url invalida");
        assert!(!falha_de_transporte(&e), "builder nao e transporte: {e}");
        assert!(matches!(
            erro_de_envio("openai request failed", &e),
            Error::Agent(_)
        ));
    }

    #[tokio::test]
    async fn mensagem_do_provider_sobrevive_inteira() {
        let e = erro_de_conexao_recusada().await;
        let texto = erro_de_envio("ollama request failed", &e).to_string();
        assert!(
            texto.contains("ollama request failed"),
            "o prefixo do provider fica; veio: {texto}"
        );
        assert!(
            texto.starts_with("transport error: "),
            "e o prefixo da classe entra na frente; veio: {texto}"
        );
    }
}
