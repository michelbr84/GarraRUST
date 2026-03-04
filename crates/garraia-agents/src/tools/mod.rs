pub mod bash_tool;
pub mod file_read_tool;
pub mod file_write_tool;
pub mod git_diff_tool;
pub mod schedule;
pub mod web_fetch_tool;
pub mod web_search_tool;

pub use bash_tool::BashTool;
pub use file_read_tool::FileReadTool;
pub use file_write_tool::FileWriteTool;
pub use git_diff_tool::GitDiffTool;
pub use schedule::ScheduleHeartbeat;
pub use web_fetch_tool::WebFetchTool;
pub use web_search_tool::WebSearchTool;

use async_trait::async_trait;
use garraia_common::Result;
use serde::{Deserialize, Serialize};

/// Contexto passado para as ferramentas durante a execução.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    /// Identificador da sessão atual.
    pub session_id: String,

    /// Identificador opcional do usuário.
    pub user_id: Option<String>,

    /// Quando verdadeiro, indica que a execução veio de um heartbeat agendado.
    /// Usado para evitar autoagendamento recursivo.
    #[serde(default)]
    pub is_heartbeat: bool,

    /// GAR-187: Quando verdadeiro, o usuário já aprovou uma confirmação pendente
    /// para esta invocação. Ferramentas com `CONFIRM_LIST` podem pular o pedido de
    /// confirmação neste caso e executar diretamente.
    #[serde(default)]
    pub is_confirmation_approved: bool,
}

/// Trait para ferramentas que os agentes podem invocar
/// (bash, navegador, operações de arquivo, etc.).
#[async_trait]
pub trait Tool: Send + Sync {
    /// Nome único da ferramenta.
    fn name(&self) -> &str;

    /// Descrição textual da ferramenta.
    fn description(&self) -> &str;

    /// JSON Schema que define os parâmetros de entrada.
    fn input_schema(&self) -> serde_json::Value;

    /// Executa a ferramenta com o contexto e entrada fornecidos.
    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput>;
}

/// Resultado retornado por uma ferramenta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Conteúdo textual retornado pela ferramenta.
    pub content: String,

    /// Indica se a execução resultou em erro.
    pub is_error: bool,

    /// GAR-187: Quando verdadeiro, a ferramenta requer confirmação do usuário antes
    /// de executar. O runtime deve interromper o loop de tools e aguardar aprovação.
    #[serde(default)]
    pub requires_confirmation: bool,
}

impl ToolOutput {
    /// Cria um resultado de sucesso.
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            requires_confirmation: false,
        }
    }

    /// Cria um resultado de erro.
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            requires_confirmation: false,
        }
    }

    /// GAR-187: Cria um resultado que requer confirmação do usuário antes de prosseguir.
    ///
    /// A mensagem deve explicar o que será executado e como aprovar.
    pub fn confirmation_request(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            requires_confirmation: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ToolOutput;

    #[test]
    fn helper_success_define_estado_sem_erro() {
        let output = ToolOutput::success("done");
        assert_eq!(output.content, "done");
        assert!(!output.is_error);
        assert!(!output.requires_confirmation);
    }

    #[test]
    fn helper_error_define_estado_com_erro() {
        let output = ToolOutput::error("failed");
        assert_eq!(output.content, "failed");
        assert!(output.is_error);
        assert!(!output.requires_confirmation);
    }

    #[test]
    fn helper_confirmation_request() {
        let output = ToolOutput::confirmation_request("Confirmar: rm -rf /tmp");
        assert!(output.is_error);
        assert!(output.requires_confirmation);
    }
}
