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

/// O portao de uma sessao fora do WhatsApp pessoal: o modo escolhido (se
/// houver) composto com o **teto de superficie** — o mesmo que
/// `AppState::exec_context_for` poe no turno de uma sessao que nao e do
/// operador (`channels_view::teto_da_superficie`).
///
/// #1915: sem isto o painel atribuia a recusa so ao modo (ou mostrava a
/// ferramenta como liberada) numa sessao remota cujo turno real tambem e
/// barrado pelo teto — a camada relatada ficava errada. So leitura: o
/// enforcement continua no turno. Sessao do operador sem modo escolhido
/// segue sem portao (`None`).
async fn portao_com_teto_de_superficie(
    app: &crate::state::AppState,
    session_id: &str,
    escolhido: Option<String>,
) -> Option<garraia_agents::modes::ToolGate> {
    use garraia_agents::modes::ToolGate;
    let operador = crate::channels_view::sessao_do_operador(app, session_id).await;
    let teto = crate::channels_view::teto_da_superficie(operador);
    match (escolhido, teto) {
        (None, None) => None,
        // Na mesma camada em que o turno o aplica (`exec_context_for_inner`
        // grava `teto_da_superficie`, nao o teto do principal).
        (modo, teto) => Some(
            modo.map(|m| ToolGate::for_mode_name(&m))
                .unwrap_or_else(ToolGate::sem_politica)
                .com_teto_de_superficie(teto),
        ),
    }
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
    // sessao composta com o teto de superficie (#1915), como o turno faz.
    // Sem `session_id`, portao aberto: so estados de runtime.
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
                    // As duas camadas que o turno do WhatsApp vinculado
                    // aplica (`GatewaySink`): o teto do principal e o da
                    // superficie resolvido pela prova de identidade. Sem a
                    // segunda, o painel mostraria como liberado o que o turno
                    // de um remetente sem identidade provada recusa.
                    use crate::bootstrap::whatsapp_linked_politica as politica;
                    Some(
                        garraia_agents::modes::ToolGate::for_mode_name(&modo)
                            .com_teto(politica::teto_do_principal(&principal))
                            .com_teto_de_superficie(politica::teto_da_superficie_do_principal(
                                &principal,
                            )),
                    )
                }
                None => portao_com_teto_de_superficie(app, sid, escolhido).await,
            }
        }
        (Some(sid), None) => {
            let escolhido = app.chosen_agent_mode_for(sid).await;
            portao_com_teto_de_superficie(app, sid, escolhido).await
        }
        (None, _) => None,
    };
    let permite = |n: &str| {
        portao
            .as_ref()
            .is_none_or(|p| app.agents.portao_permite(p, n))
    };
    // #1915: o mesmo portao, por camada — o console mostra se quem barra e
    // o modo da sessao ou o teto do principal.
    let recusa = |n: &str| portao.as_ref().and_then(|p| app.agents.portao_recusa(p, n));
    let disponibilidade = |n: &str| app.agents.disponibilidade_de(n);
    let mcp: Vec<garraia_agents::McpServerStatus> = match &app.mcp_manager_arc {
        Some(mgr) => mgr.server_statuses().await,
        None => Vec::new(),
    };
    let politica = crate::bootstrap::politica_de_execucao(&app.config);
    let exposicao = crate::bootstrap::exposicao_do_bash(
        politica.perfil,
        &crate::bootstrap::sandbox_policy_from(&app.config.agent.sandbox),
        !app.config.agent.bash_allowlist.is_empty(),
    );
    let bash_desligado = match exposicao {
        crate::bootstrap::ExposicaoDoBash::Desligado { .. } => Some((
            exposicao.descricao(),
            crate::bootstrap::COMO_LIGAR_O_BASH.to_string(),
        )),
        _ => None,
    };
    // #1416: com `session_id`, o contexto e o DAQUELA sessao (tem
    // `working_dir`?); sem sessao, so o que a instalacao garante (fonte das
    // raizes) e nada sobre repositorio.
    // Resolvido no boot, nunca por request.
    let fonte = app.raizes_das_file_tools.fonte;
    let contexto = match &resumo {
        Some(r) => crate::capacidades_registro::contexto_de_arquivos(
            fonte,
            r.has_workspace.then_some(std::path::Path::new(".")),
            garraia_agents::tools::repo_search_tool::processo_em_repositorio,
        ),
        None => crate::capacidades_registro::ContextoDaSessao {
            tem_raiz: Some(!matches!(
                fonte,
                crate::bootstrap::FonteDasRaizesDasFileTools::SomenteSessao
            )),
            tem_repositorio: None,
        },
    };
    let linhas = registro(&Entradas {
        inventario: &inventario,
        permite: &permite,
        recusa: &recusa,
        disponibilidade: &disponibilidade,
        mcp: &mcp,
        bash_desligado,
        restrito: false,
        contexto,
    });
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "session_id": query.session_id,
            "session": sessao_json,
            "counts": contagens(&linhas),
            "capabilities": linhas,
            // #1417: as ferramentas em pausa NESTA conversa (circuit breaker),
            // com codigo e motivo — texto constante do modulo, sem caminho.
            "breaker": query
                .session_id
                .as_deref()
                .map(|sid| app.agents.estado_do_breaker(sid))
                .unwrap_or_default(),
        })),
    )
}
