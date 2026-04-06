use axum::{Json, http::StatusCode, response::IntoResponse};

/// GET /api/logs
pub async fn get_logs() -> impl IntoResponse {
    let garraia_dir = garraia_config::ConfigLoader::default_config_dir();
    let log_path = garraia_dir.join("garraia.log");

    if !log_path.exists() {
        // Return 200 with a hint — 404 causes the frontend to show a generic "server unavailable" error.
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "logs": format!(
                    "Nenhum arquivo de log encontrado em {}.\n\
                     Para habilitar logs em arquivo, configure RUST_LOG e redirecione a saída:\n\
                     RUST_LOG=garraia=info garraia start 2>&1 | tee ~/.garraia/garraia.log",
                    log_path.display()
                )
            })),
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

        // Read as bytes and convert lossily — prevents 500 when the log contains
        // non-UTF-8 bytes (e.g. from terminal colour codes or binary tool output).
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        let mut buf = String::from_utf8_lossy(&raw).into_owned();

        if file_len > MAX_TAIL_BYTES
            && let Some(pos) = buf.find('\n') {
                buf = buf[pos + 1..].to_string();
            }

        let lines: Vec<&str> = buf.lines().collect();
        let start = if lines.len() > 1000 {
            lines.len() - 1000
        } else {
            0
        };
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
