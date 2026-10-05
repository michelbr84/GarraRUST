//! `GET /admin/api/permissions/overview` (#1433): a visao global de agentes e
//! permissoes do Web Console — principais, suas capacidades efetivas por
//! canal e o que e sensivel, num lugar so.
//!
//! Esta rota NAO tem logica de autorizacao propria. Ela le a MESMA politica
//! que a pagina WhatsApp Access edita (ADR 0025): o efetivo de cada principal
//! vem de [`impacto::matriz`] — o `ToolGate` real do turno (piso de modo +
//! teto do principal) —, as classes de cada linha de [`Capacidades::classes`],
//! e "sensivel" de [`Capacidade::e_mutante`]. O perfil de execucao e o do
//! PROCESSO (ADR 0024). Nenhuma matriz `role×resource×action` chumbada entra
//! aqui: `GET /admin/api/permissions` (o RBAC do painel) e outra coisa e
//! continua onde esta.
//!
//! Hoje so o `whatsapp_linked` tem motor de politica por principal. Os demais
//! canais sao listados com honestidade (`policy_engine: false`,
//! `policy_source: "defaults"`): o piso de modo da sessao e o portao de
//! ferramentas valem, mas nao ha teto por principal — nao se finge o que nao
//! existe.
//!
//! Edicao: a pagina global NAO duplica o fluxo de edicao. Para o
//! `whatsapp_linked` ela aponta para `edit_endpoint`/`edit_page`, que e o
//! caminho unico de `POST /admin/api/whatsapp/access` (preview/confirm, com a
//! confirmacao de elevacao sensivel enforcada no servidor). Secret-free e
//! PII-free como a pagina WhatsApp Access: identidades so por `…1234`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use garraia_agents::capacidades::Capacidade;
use garraia_config::{AppConfig, ConfigLoader, ExecutionProfile};
use serde_json::{Value, json};

use super::middleware::AuthenticatedAdmin;
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;
use crate::bootstrap::WHATSAPP_LINKED_CONFIG_KEY;
use crate::bootstrap::whatsapp_linked_politica::presets::rotulo_efetivo;
use crate::bootstrap::whatsapp_linked_politica::{Principal, impacto, visao};
use crate::bootstrap::whatsapp_linked_settings;
use crate::channels_view::KNOWN_CHANNELS;

/// O canal cujo `id` casa com `WHATSAPP_LINKED_CONFIG_KEY` tem motor de
/// politica; o resto, nao. Uma lista fechada deixa explicito o que falta.
fn tem_motor_de_politica(id: &str) -> bool {
    id == WHATSAPP_LINKED_CONFIG_KEY
}

/// As classes do registro (#1385) com a marca de sensivel. "Sensivel" = muta
/// algo fora da conversa ([`Capacidade::e_mutante`]): e o que a elevacao
/// precisa confirmar. A ordem e a do vocabulario.
fn legenda_de_classes() -> Vec<Value> {
    Capacidade::TODAS
        .iter()
        .map(|c| {
            json!({
                "class": c.as_str(),
                "sensitive": c.e_mutante(),
            })
        })
        .collect()
}

/// Os principais do `whatsapp_linked` com as classes efetivas, pelo MESMO
/// motor da matriz. Cada linha ja vem com o alvo mascarado (`…1234`).
fn principais_com_classes(
    settings: &crate::bootstrap::WhatsAppLinkedSettings,
    perfil: ExecutionProfile,
) -> Vec<Value> {
    impacto::matriz(settings, perfil)
        .into_iter()
        .map(|l| {
            let admitido = l.principal != Principal::Bloqueado.as_str()
                && l.principal != Principal::Estranho.as_str();
            let classes = l.efetivo.capacidades.classes();
            let sensitive: Vec<&str> = classes
                .iter()
                .filter(|c| c.e_mutante())
                .map(|c| c.as_str())
                .collect();
            let all: Vec<&str> = classes.iter().map(|c| c.as_str()).collect();
            let (level, write) = match l.efetivo.alcance {
                Some(a) => (json!(a.nivel.as_str()), json!(a.write)),
                None => (Value::Null, Value::Null),
            };
            json!({
                "principal": l.principal,
                "last4": l.alvo,
                "mode": l.efetivo.modo,
                "level": level,
                "write": write,
                "preset": match l.efetivo.alcance {
                    Some(a) => json!(rotulo_efetivo(a)),
                    None => Value::Null,
                },
                "admitted": admitido,
                "classes": all,
                "sensitive_classes": sensitive,
            })
        })
        .collect()
}

/// O bloco do `whatsapp_linked`: resumo da politica (o mesmo de
/// [`visao::documento`]) e os principais com as classes efetivas.
fn bloco_whatsapp(config: &AppConfig, perfil: ExecutionProfile, enabled: bool) -> Value {
    let settings = whatsapp_linked_settings(config);
    let doc = visao::documento(config, perfil, false);
    json!({
        "id": WHATSAPP_LINKED_CONFIG_KEY,
        "label": "WhatsApp (linked device)",
        "policy_engine": true,
        "policy_source": "channels.whatsapp_linked.access",
        "enabled": enabled,
        "admission": doc["admission"].clone(),
        "defaults": {
            "owner_floor_mode": doc["owner_floor_mode"].clone(),
            "default_mode": doc["default_mode"].clone(),
            "unknown_sender": doc["default"].clone(),
            "groups": doc["groups"].clone(),
        },
        "overrides": doc["counts"].clone(),
        "warnings": doc["warnings"].clone(),
        // A edicao e o caminho unico da pagina WhatsApp Access — nao se
        // duplica aqui.
        "edit_page": "whatsapp_access",
        "edit_endpoint": "/admin/api/whatsapp/access",
        "principals": principais_com_classes(&settings, perfil),
    })
}

/// Um canal sem motor de politica por principal: listado com honestidade.
fn bloco_sem_motor(id: &str, label: &str, enabled: bool) -> Value {
    json!({
        "id": id,
        "label": label,
        "policy_engine": false,
        "policy_source": "defaults",
        "enabled": enabled,
        "note": "per-principal access policy not supported yet; the session mode floor and the tool gate apply to every sender",
        "principals": Value::Array(Vec::new()),
    })
}

pub async fn admin_permissions_overview(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    // Mesma guarda da pagina WhatsApp Access: viewer le.
    if !check_permission(admin.role, Resource::Channels, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient permissions" })),
        );
    }
    let config = match ConfigLoader::new().and_then(|l| l.load_sem_env()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(erro = %e, "admin: nao consegui ler o config.yml para a visao de permissoes");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "failed to read config" })),
            );
        }
    };
    // O perfil de execucao e o do PROCESSO (env vence o arquivo, ADR 0024).
    let perfil = state.app_state.config.execution.perfil();

    let channels: Vec<Value> = KNOWN_CHANNELS
        .iter()
        .map(|(id, label, _needs_secret, _kind)| {
            let enabled = config
                .channels
                .get(*id)
                .is_some_and(|c| c.enabled.unwrap_or(false));
            if tem_motor_de_politica(id) {
                bloco_whatsapp(&config, perfil, enabled)
            } else {
                bloco_sem_motor(id, label, enabled)
            }
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "global": {
                "execution_profile": visao::nome_do_perfil(perfil),
            },
            "capability_classes": legenda_de_classes(),
            "channels": channels,
            "hot_reload": state.app_state.has_config_watcher(),
        })),
    )
}
