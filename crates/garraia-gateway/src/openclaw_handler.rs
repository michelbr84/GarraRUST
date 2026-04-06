use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::state::SharedState;

/// GET /api/openclaw/status — connection status of the OpenClaw bridge.
pub async fn openclaw_status(
    State(state): State<SharedState>,
) -> Json<serde_json::Value> {
    let status = if let Some(ref client) = state.openclaw_client {
        let s = client.status();
        serde_json::json!({
            "enabled": true,
            "status": format!("{s:?}"),
            "ws_url": state.openclaw_config.as_ref().map(|c| &c.ws_url),
        })
    } else {
        serde_json::json!({
            "enabled": false,
            "status": "not_configured",
        })
    };

    Json(status)
}

/// POST /api/openclaw/connect — (re)connect to the OpenClaw daemon.
///
/// In the current design the connection loop runs automatically when the
/// client is constructed, so this endpoint is a no-op status check.
/// Future versions may support on-demand connect/disconnect.
pub async fn openclaw_connect(
    State(state): State<SharedState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if state.openclaw_client.is_some() {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "message": "OpenClaw client is active (auto-reconnect enabled)",
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": "OpenClaw is not configured. Set OPENCLAW_ENABLED=true in .env",
            })),
        )
    }
}

/// POST /api/openclaw/disconnect — placeholder for graceful disconnection.
pub async fn openclaw_disconnect(
    State(state): State<SharedState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if state.openclaw_client.is_some() {
        // Future: signal the connection loop to stop.
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "message": "Disconnect is not yet implemented; the client auto-reconnects",
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": "OpenClaw is not configured",
            })),
        )
    }
}

/// GET /api/openclaw/channels — list platforms available through OpenClaw.
pub async fn openclaw_channels(
    State(state): State<SharedState>,
) -> Json<serde_json::Value> {
    let channels: Vec<&str> = if let Some(ref client) = state.openclaw_client {
        client
            .available_channels()
            .iter()
            .map(|s| s.as_str())
            .collect()
    } else {
        Vec::new()
    };

    Json(serde_json::json!({ "channels": channels }))
}
