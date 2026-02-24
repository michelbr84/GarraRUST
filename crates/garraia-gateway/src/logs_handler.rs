use axum::{
    http::StatusCode,
    response::IntoResponse,
    Json,
};

/// GET /api/logs
pub async fn get_logs() -> impl IntoResponse {
    let garraia_dir = garraia_config::ConfigLoader::default_config_dir();
    let log_path = garraia_dir.join("garraia.log");

    if !log_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Log file not found at {}", log_path.display()) })),
        )
            .into_response();
    }

    match std::fs::read_to_string(&log_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > 1000 { lines.len() - 1000 } else { 0 };
            let recent_lines = lines[start..].join("\n");
            
            (
                StatusCode::OK,
                Json(serde_json::json!({ "logs": recent_lines })),
            )
                .into_response()
        },
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}
