//! `GET /admin/api/capabilities` (#1381): o registro de capacidades do
//! runtime para o console — a mesma funcao que o `garra_status` e o
//! `/api/diagnostics` usam, sem portao de sessao (nao ha conversa aqui) e
//! com `?session_id=` opcional para aplicar o modo escolhido daquela sessao.
//! `Resource::Tools` + `Action::Read` (viewer le). Secret-free por
//! construcao: os motivos vem das proprias ferramentas e de constantes.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::middleware::AuthenticatedAdmin;
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;
use crate::capacidades_registro::{Entradas, contagens, registro};

#[derive(Debug, Clone, Deserialize)]
pub struct CapabilitiesQuery {
    /// Aplica o portao do modo escolhido desta sessao (`denied` passa a
    /// existir). Sem ele, o portao e aberto: so estados de runtime.
    pub session_id: Option<String>,
}

pub async fn admin_capabilities(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Query(query): Query<CapabilitiesQuery>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Tools, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }
    let app = &state.app_state;
    let inventario = app.agents.tool_inventory();
    let portao = match &query.session_id {
        Some(sid) => app
            .chosen_agent_mode_for(sid)
            .await
            .map(|m| garraia_agents::modes::ToolGate::for_mode_name(&m)),
        None => None,
    };
    let permite = |n: &str| {
        portao
            .as_ref()
            .is_none_or(|p| app.agents.portao_permite(p, n))
    };
    let disponibilidade = |n: &str| app.agents.disponibilidade_de(n);
    let mcp: Vec<garraia_agents::McpServerStatus> = match &app.mcp_manager_arc {
        Some(mgr) => mgr.server_statuses().await,
        None => Vec::new(),
    };
    let politica = crate::bootstrap::politica_de_execucao(&app.config);
    let exposicao = crate::bootstrap::exposicao_do_bash(
        politica.perfil,
        &crate::bootstrap::sandbox_policy_from(&app.config.agent.sandbox),
    );
    let bash_desligado = match exposicao {
        crate::bootstrap::ExposicaoDoBash::Desligado { .. } => Some((
            exposicao.descricao(),
            crate::bootstrap::COMO_LIGAR_O_BASH.to_string(),
        )),
        _ => None,
    };
    let linhas = registro(&Entradas {
        inventario: &inventario,
        permite: &permite,
        disponibilidade: &disponibilidade,
        mcp: &mcp,
        bash_desligado,
        restrito: false,
    });
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "session_id": query.session_id,
            "counts": contagens(&linhas),
            "capabilities": linhas,
        })),
    )
}
