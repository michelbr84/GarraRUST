use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::middleware::AuthenticatedAdmin;
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;

// ═══════════════════════════════════════════════════════════════════════
// Phase 6: Observability/UI
// ═══════════════════════════════════════════════════════════════════════

/// GET /admin/api/logs — stream recent log entries
pub async fn admin_logs(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Sessions, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100);
    let log_path = dirs::home_dir()
        .map(|h| h.join(".garraia").join("garraia.log"))
        .unwrap_or_default();

    if !log_path.exists() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"lines": [], "count": 0})),
        )
            .into_response();
    }

    match ultimas_linhas_do_log(&log_path, limit) {
        Ok(lines) => (
            StatusCode::OK,
            Json(serde_json::json!({"lines": lines, "count": lines.len()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}

/// As ultimas `limit` linhas do `garraia.log`, lidas so da cauda.
///
/// O log cresce sem rotacao desde que o daemon passou a abrir em append
/// (#1371), e o descritor que ele herda como stdout/stderr recebe escrita
/// crua (panic, filho com stderr herdado). Um `read_to_string` do arquivo
/// inteiro custava memoria e CPU proporcionais a toda a historia do daemon a
/// cada GET e respondia 500 no primeiro byte que nao fosse UTF-8. A leitura
/// agora e a mesma do `GET /api/logs`: no maximo `MAX_TAIL_BYTES` do fim, em
/// UTF-8 lossy.
fn ultimas_linhas_do_log(path: &std::path::Path, limit: usize) -> std::io::Result<Vec<String>> {
    let cauda = crate::logs_handler::read_log_tail(path, crate::logs_handler::MAX_TAIL_BYTES)?;
    let linhas: Vec<&str> = cauda.lines().collect();
    let inicio = linhas.len().saturating_sub(limit);
    Ok(linhas[inicio..].iter().map(|l| (*l).to_owned()).collect())
}

/// GET /admin/api/metrics — current metrics snapshot
pub async fn admin_metrics(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Metrics, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        )
            .into_response();
    }

    let metrics = crate::observability::global_metrics();
    let active_sessions = state.app_state.sessions.len();
    let active_providers = state.app_state.agents.provider_ids();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "requests_total": metrics.requests_total.load(std::sync::atomic::Ordering::Relaxed),
            "active_sessions": active_sessions,
            "active_providers": active_providers,
            "provider_count": active_providers.len(),
        })),
    )
        .into_response()
}

/// GET /admin/api/metrics/prometheus — raw prometheus format
pub async fn admin_prometheus(
    State(_state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Metrics, Action::Read) {
        return (StatusCode::FORBIDDEN, "insufficient permissions").into_response();
    }

    let body = crate::observability::global_metrics().render_prometheus();
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
        .into_response()
}

/// GET /admin/api/alerts — basic alerts (provider down, high error rate)
pub async fn admin_alerts(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Alerts, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let mut alerts: Vec<serde_json::Value> = Vec::new();

    let active_ids = state.app_state.agents.provider_ids();
    if active_ids.is_empty() {
        alerts.push(serde_json::json!({
            "level": "warning",
            "source": "providers",
            "message": "No LLM providers are active",
        }));
    }

    let config = state.app_state.current_config();
    if !config.memory.enabled {
        alerts.push(serde_json::json!({
            "level": "info",
            "source": "memory",
            "message": "Memory system is disabled",
        }));
    }

    if let Some(alerta) = alerta_sem_credencial(&config.gateway) {
        alerts.push(alerta);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"alerts": alerts, "count": alerts.len()})),
    )
}

/// The "no gateway credential" alert, or `None` when there is one.
///
/// #1261: the credential may come from `GARRAIA_GATEWAY_API_KEY`
/// (`api_key_env`), and a blank file key leaves the gate off (#1241). Both
/// cases go through `api_key_configurada`, the predicate the gate itself uses
/// — reading the raw `api_key` field warned about an env-only key and stayed
/// quiet about a blank one.
fn alerta_sem_credencial(gateway: &garraia_config::GatewayConfig) -> Option<serde_json::Value> {
    if gateway.api_key_configurada() {
        return None;
    }
    Some(serde_json::json!({
        "level": "warning",
        "source": "security",
        "message": "No API key configured for the gateway",
    }))
}

/// GET /admin/api/themes — available UI themes
pub async fn list_themes() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "themes": [
            {"id": "dark", "name": "Dark", "description": "Dark theme"},
            {"id": "light", "name": "Light", "description": "Light theme"},
            {"id": "brasil", "name": "Brasil", "description": "Green and gold accent"},
        ],
        "current": "dark",
    }))
}

/// GET /admin/api/layout — layout preferences
pub async fn get_layout_preferences() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "sidebar_compact": false,
        "density": "comfortable",
        "shortcuts": {
            "toggle_sidebar": "Ctrl+B",
            "search": "Ctrl+K",
            "settings": "Ctrl+,",
        }
    }))
}

/// GET /admin/api/templates — list prompt/persona templates
pub async fn list_templates(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Config, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();
    let mut templates: Vec<serde_json::Value> = Vec::new();

    if let Some(prompt) = &config.agent.system_prompt {
        templates.push(serde_json::json!({
            "id": "default",
            "name": "Default Agent",
            "system_prompt_preview": if prompt.len() > 100 { &prompt[..100] } else { prompt },
            "provider": config.agent.default_provider,
        }));
    }

    for (name, agent) in &config.agents {
        templates.push(serde_json::json!({
            "id": name,
            "name": name,
            "system_prompt_preview": agent.system_prompt.as_ref()
                .map(|p| if p.len() > 100 { &p[..100] } else { p }),
            "provider": agent.provider,
            "model": agent.model,
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"templates": templates})),
    )
}

/// GET /admin/api/about — build info, version, uptime
pub async fn about(State(state): State<AdminState>) -> Json<serde_json::Value> {
    let active_providers = state.app_state.agents.provider_ids();
    let session_count = state.app_state.sessions.len();

    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": "GarraIA",
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "repository": "https://github.com/michelbr84/GarraRUST",
        "license": "MIT",
        "rust_version": "1.85+",
        "active_providers": active_providers.len(),
        "active_sessions": session_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::GatewayConfig;

    #[test]
    fn alerta_de_credencial_segue_o_predicado_do_gate() {
        let env_only = GatewayConfig {
            api_key_env: garraia_config::auth::gateway_api_key_de(Some("k-env-1261".into())),
            ..GatewayConfig::default()
        };
        assert!(
            alerta_sem_credencial(&env_only).is_none(),
            "chave de env ignorada"
        );

        let arquivo = GatewayConfig {
            api_key: Some("k-arquivo".into()),
            ..GatewayConfig::default()
        };
        assert!(alerta_sem_credencial(&arquivo).is_none());

        let branco = GatewayConfig {
            api_key: Some("   ".into()),
            ..GatewayConfig::default()
        };
        assert!(
            alerta_sem_credencial(&branco).is_some(),
            "chave em branco nao liga o gate"
        );
        assert!(alerta_sem_credencial(&GatewayConfig::default()).is_some());
    }

    /// #1371: o log do daemon cresce entre restarts. O GET le so a cauda
    /// (`MAX_TAIL_BYTES`), nunca o arquivo inteiro, e a linha partida pelo
    /// corte nao aparece.
    #[test]
    fn logs_do_admin_leem_so_a_cauda_de_um_log_maior_que_o_teto() {
        use crate::logs_handler::MAX_TAIL_BYTES;
        use std::io::Write as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("garraia.log");
        let mut f = std::fs::File::create(&path).expect("cria o log");
        let mut escrito = 0u64;
        let mut n = 0u32;
        while escrito <= 2 * MAX_TAIL_BYTES {
            let linha = format!("linha {n:08} do daemon\n");
            f.write_all(linha.as_bytes()).expect("escreve");
            escrito += linha.len() as u64;
            n += 1;
        }
        drop(f);

        let linhas = ultimas_linhas_do_log(&path, usize::MAX).expect("le a cauda");
        let bytes: usize = linhas.iter().map(|l| l.len() + 1).sum();
        assert!(
            bytes as u64 <= MAX_TAIL_BYTES,
            "leu {bytes} bytes, acima do teto de {MAX_TAIL_BYTES}"
        );
        assert!(
            !linhas.iter().any(|l| l == "linha 00000000 do daemon"),
            "a cabeca do arquivo nao pode ser lida"
        );
        assert_eq!(
            linhas.last().map(String::as_str),
            Some(format!("linha {:08} do daemon", n - 1).as_str()),
            "a ultima linha e a mais recente"
        );
        for l in &linhas {
            assert!(
                l.starts_with("linha ") && l.ends_with(" do daemon") && l.len() == 24,
                "linha partida pelo corte: {l:?}"
            );
        }

        let tres = ultimas_linhas_do_log(&path, 3).expect("le 3");
        assert_eq!(tres.len(), 3);
        assert_eq!(tres[2], format!("linha {:08} do daemon", n - 1));
    }

    /// #1371: escrita crua pelo descritor herdado nao e necessariamente
    /// UTF-8. Um byte invalido vira U+FFFD em vez de 500.
    #[test]
    fn logs_do_admin_sobrevivem_a_byte_que_nao_e_utf8() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("garraia.log");
        std::fs::write(&path, b"antes\nfilho cru \xff aqui\ndepois\n").expect("semeia");

        let linhas = ultimas_linhas_do_log(&path, 100).expect("byte invalido nao derruba");
        assert_eq!(
            linhas,
            vec![
                "antes".to_owned(),
                "filho cru \u{FFFD} aqui".to_owned(),
                "depois".to_owned(),
            ]
        );
    }
}
