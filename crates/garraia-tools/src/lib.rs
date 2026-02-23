use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use thiserror::Error;
use tokio::time;

/// Contexto de execução da ferramenta.
/// Contém informações sobre a requisição atual.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub request_id: String,
    // TODO: adicionar user_id, tenant_id, permissions, etc.
}

/// Entrada para uma ferramenta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    pub name: String,
    pub payload: serde_json::Value,
}

/// Saída de uma ferramenta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub name: String,
    pub payload: serde_json::Value,
}

/// Erros possíveis durante execução de ferramenta.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("ferramenta expirou após {0:?}")]
    Timeout(Duration),

    #[error("ferramenta falhou: {0}")]
    Failed(String),
}

/// Trait principal para todas as ferramentas do GarraIA.
/// Cada ferramenta implementa este trait.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Nome único da ferramenta.
    fn name(&self) -> &'static str;

    /// Executa a ferramenta com o contexto e entrada fornecidos.
    async fn execute(&self, ctx: &ToolContext, input: ToolInput) -> Result<ToolOutput, ToolError>;
}

/// Registry centralizado de ferramentas.
/// Permite registrar e buscar ferramentas por nome.
pub struct ToolRegistry {
    tools: HashMap<&'static str, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Cria um registry vazio.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Registra uma ferramenta no registry (builder pattern).
    pub fn register<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.insert(tool.name(), Box::new(tool));
        self
    }

    /// Busca uma ferramenta pelo nome.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Retorna os nomes de todas as ferramentas registradas.
    pub fn list_names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    /// Retorna o número de ferramentas registradas.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Verifica se o registry está vazio.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Executa uma ferramenta com timeout.
/// Se a ferramenta não responder dentro do prazo, retorna `ToolError::Timeout`.
/// Importante: `tokio::time::timeout` cancela a future internamente (drop).
pub async fn execute_with_timeout(
    tool: &dyn Tool,
    ctx: &ToolContext,
    input: ToolInput,
    timeout_duration: Duration,
) -> Result<ToolOutput, ToolError> {
    match time::timeout(timeout_duration, tool.execute(ctx, input)).await {
        Ok(result) => result,
        Err(_elapsed) => Err(ToolError::Timeout(timeout_duration)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FerramentaEco;

    #[async_trait]
    impl Tool for FerramentaEco {
        fn name(&self) -> &'static str {
            "eco"
        }

        async fn execute(&self, _ctx: &ToolContext, input: ToolInput) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput {
                name: input.name,
                payload: input.payload,
            })
        }
    }

    struct FerramentaLenta;

    #[async_trait]
    impl Tool for FerramentaLenta {
        fn name(&self) -> &'static str {
            "lenta"
        }

        async fn execute(&self, _ctx: &ToolContext, input: ToolInput) -> Result<ToolOutput, ToolError> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(ToolOutput {
                name: input.name,
                payload: serde_json::json!({"resultado": "nunca chega aqui"}),
            })
        }
    }

    fn criar_contexto() -> ToolContext {
        ToolContext {
            request_id: "teste-001".into(),
        }
    }

    #[tokio::test]
    async fn registry_registra_e_busca() {
        let registry = ToolRegistry::new().register(FerramentaEco);
        assert_eq!(registry.len(), 1);
        assert!(registry.get("eco").is_some());
        assert!(registry.get("inexistente").is_none());
    }

    #[tokio::test]
    async fn ferramenta_eco_retorna_payload() {
        let tool = FerramentaEco;
        let ctx = criar_contexto();
        let input = ToolInput {
            name: "eco".into(),
            payload: serde_json::json!({"msg": "olá"}),
        };

        let output = tool.execute(&ctx, input).await.unwrap();
        assert_eq!(output.payload, serde_json::json!({"msg": "olá"}));
    }

    #[tokio::test]
    async fn timeout_e_aplicado() {
        let tool = FerramentaLenta;
        let ctx = criar_contexto();
        let input = ToolInput {
            name: "lenta".into(),
            payload: serde_json::json!({}),
        };

        let resultado = execute_with_timeout(
            &tool,
            &ctx,
            input,
            Duration::from_millis(100),
        )
        .await;

        assert!(resultado.is_err());
        match resultado.unwrap_err() {
            ToolError::Timeout(d) => assert_eq!(d, Duration::from_millis(100)),
            _ => panic!("esperava ToolError::Timeout"),
        }
    }

    #[tokio::test]
    async fn execucao_com_timeout_sucesso() {
        let tool = FerramentaEco;
        let ctx = criar_contexto();
        let input = ToolInput {
            name: "eco".into(),
            payload: serde_json::json!({"ok": true}),
        };

        let resultado = execute_with_timeout(
            &tool,
            &ctx,
            input,
            Duration::from_secs(5),
        )
        .await;

        assert!(resultado.is_ok());
    }
}
