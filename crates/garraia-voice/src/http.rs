//! Bounded HTTP bodies shared by the voice adapters.

/// Maximum audio upload or generated audio: 16 MiB.
pub const MAX_AUDIO_BYTES: usize = 16 * 1024 * 1024;
/// Maximum text sent to TTS or accepted from STT: 32 KiB.
pub const MAX_TEXT_BYTES: usize = 32 * 1024;
/// Maximum JSON/metadata accepted from a voice server: 1 MiB.
pub const MAX_METADATA_BYTES: usize = 1024 * 1024;
pub const VOICE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Cliente HTTP dos adaptadores de voz: timeout de conexao e total, sem
/// redirects (um redirect levaria o audio a um destino fora da config).
pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(VOICE_REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| {
            // So falha se o backend TLS nao inicializar; o padrao do crate
            // antes do #1754 era o mesmo fallback.
            tracing::warn!("voice HTTP client builder failed; using defaults");
            reqwest::Client::new()
        })
}

/// Read incrementally, including when a server omits Content-Length.
pub async fn bounded_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        return Err("voice response exceeds the size limit".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "failed to read voice response")?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err("voice response exceeds the size limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) fn api_url(endpoint: &str, path: &str) -> String {
    let path = if endpoint.ends_with("/v1") {
        path.strip_prefix("/v1").unwrap_or(path)
    } else {
        path
    };
    format!("{endpoint}{path}")
}
