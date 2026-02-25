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

    const MAX_TAIL_BYTES: u64 = 512 * 1024;

    let read_result = (|| -> std::io::Result<String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&log_path)?;
        let metadata = file.metadata()?;
        let file_len = metadata.len();

        if file_len > MAX_TAIL_BYTES {
            file.seek(SeekFrom::End(-(MAX_TAIL_BYTES as i64)))?;
        }

        let mut buf = String::new();
        file.read_to_string(&mut buf)?;

        if file_len > MAX_TAIL_BYTES {
            if let Some(pos) = buf.find('\n') {
                buf = buf[pos + 1..].to_string();
            }
        }

        let lines: Vec<&str> = buf.lines().collect();
        let start = if lines.len() > 1000 { lines.len() - 1000 } else { 0 };
        Ok(lines[start..].join("\n"))
    })();

    match read_result {
        Ok(recent_lines) => (
            StatusCode::OK,
            Json(serde_json::json!({ "logs": recent_lines })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
