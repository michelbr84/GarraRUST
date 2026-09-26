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
    // #1415: com `session_id`, o portao e o do TURNO daquela conversa — no
    // WhatsApp pessoal, o piso do canal (ou a escolha da sessao) composto com
    // o teto do principal (ADR 0025); nas outras superficies, a escolha da
    // sessao. Sem `session_id`, portao aberto: so estados de runtime.
    let resumo = query.session_id.as_deref().and_then(|sid| {
        app.sessions
            .get(sid)
            .map(|s| super::sessoes::ResumoDaSessao::de(s.value()))
    });
    let sessao_json = match resumo.clone() {
        Some(r) => Some(super::sessoes::sessao_em_json(app, r).await),
        None => None,
    };
    let portao = match (&query.session_id, &resumo) {
        (Some(sid), Some(r)) => {
            let escolhido = app.chosen_agent_mode_for(sid).await;
            match super::sessoes::principal_e_piso(app, r) {
                Some((principal, piso)) => {
                    let modo = escolhido.unwrap_or(piso);
                    Some(
                        garraia_agents::modes::ToolGate::for_mode_name(&modo).com_teto(
                            crate::bootstrap::whatsapp_linked_politica::teto_do_principal(
                                &principal,
                            ),
                        ),
                    )
                }
                None => escolhido.map(|m| garraia_agents::modes::ToolGate::for_mode_name(&m)),
            }
        }
        (Some(sid), None) => app
            .chosen_agent_mode_for(sid)
            .await
            .map(|m| garraia_agents::modes::ToolGate::for_mode_name(&m)),
        (None, _) => None,
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
            "session": sessao_json,
            "counts": contagens(&linhas),
            "capabilities": linhas,
        })),
    )
}
