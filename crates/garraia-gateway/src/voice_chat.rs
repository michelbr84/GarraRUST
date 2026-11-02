//! Voice adapters for the existing authenticated Web Console.
//!
//! Configuration, credentials and destinations belong to the server. A browser
//! can select a model, never supply a URL, credential or routing policy. Audio
//! stays in bounded memory; requests never retry on another paid provider.

use crate::state::SharedState;
use axum::{
    Json,
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CATALOG_TTL: Duration = Duration::from_secs(300);
const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
const MAX_UTTERANCE_BYTES: usize = 1024 * 1024;
const MAX_SENTENCE_BYTES: usize = 4096;

/// Per-gateway capacity. Permits are released when the request is cancelled.
pub struct VoiceChat {
    requests: Semaphore,
    discovery: Semaphore,
    catalogs: Mutex<HashMap<String, (Instant, Vec<Value>)>>,
}

impl Default for VoiceChat {
    fn default() -> Self {
        Self {
            requests: Semaphore::new(4),
            discovery: Semaphore::new(2),
            catalogs: Mutex::new(HashMap::new()),
        }
    }
}

type VoiceResult<T> = Result<T, (StatusCode, Json<Value>)>;

fn failure(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error": message})))
}

fn bad_request(message: &str) -> (StatusCode, Json<Value>) {
    failure(StatusCode::BAD_REQUEST, message)
}

fn client() -> VoiceResult<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| {
            failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Voice HTTP client unavailable",
            )
        })
}

struct Remote {
    base: String,
    key: String,
    policy: Value,
}

/// Only an already registered OpenRouter instance may use the remote adapter.
fn remote(state: &SharedState, provider: &str) -> VoiceResult<Remote> {
    let config = state.current_config();
    let cfg = config
        .llm
        .get(provider)
        .filter(|cfg| cfg.provider == "openrouter" && state.agents.get_provider(provider).is_some())
        .ok_or_else(|| bad_request("Select a configured OpenRouter instance"))?;
    let key = garraia_config::resolve_api_key(
        cfg.api_key.as_deref(),
        "OPENROUTER_API_KEY",
        "OPENROUTER_API_KEY",
    )
    .ok_or_else(|| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "OpenRouter credential unavailable",
        )
    })?;
    let base = cfg
        .base_url
        .as_deref()
        .unwrap_or("https://openrouter.ai/api/v1")
        .trim_end_matches('/');
    let url =
        reqwest::Url::parse(base).map_err(|_| bad_request("Invalid configured voice endpoint"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
    {
        return Err(bad_request(
            "Voice endpoint must use HTTPS or local HTTP, without credentials or query",
        ));
    }
    // Speech endpoints do not implement provider.only/order/ignore. Refuse
    // rather than silently disregard an operator's configured restriction.
    let policy = cfg
        .extra
        .get("voice_provider_policy")
        .cloned()
        .unwrap_or(json!({}));
    let object = policy
        .as_object()
        .ok_or_else(|| bad_request("Invalid voice provider policy"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "zdr" | "data_collection"))
        || object.get("zdr").is_some_and(|v| !v.is_boolean())
        || object
            .get("data_collection")
            .is_some_and(|v| !matches!(v.as_str(), Some("allow" | "deny")))
    {
        return Err(bad_request(
            "Voice policy supports only zdr and data_collection; upstream provider restrictions require a direct adapter",
        ));
    }
    Ok(Remote {
        base: base.to_string(),
        key,
        policy,
    })
}

async fn response_bytes(response: reqwest::Response, limit: usize) -> VoiceResult<Vec<u8>> {
    let status = response.status();
    if !status.is_success() {
        // Do not send upstream response bodies, URLs, credentials or transcripts
        // to the browser or logs. In particular, do not retry billed work.
        return Err(failure(
            if status.as_u16() == 429 {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::BAD_GATEWAY
            },
            "Voice provider rejected the request; verify model, voice, account limits and server configuration",
        ));
    }
    garraia_voice::http::bounded_body(response, limit)
        .await
        .map_err(|_| {
            failure(
                StatusCode::BAD_GATEWAY,
                "Invalid or oversized voice provider response",
            )
        })
}

fn has_modality(model: &Value, direction: &str, capability: &str) -> bool {
    model
        .get("architecture")
        .and_then(|a| a.get(direction))
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(capability)))
}

fn supports(model: &Value, capability: &str) -> bool {
    has_modality(model, "output_modalities", capability)
        && has_modality(
            model,
            "input_modalities",
            if capability == "speech" {
                "text"
            } else {
                "audio"
            },
        )
}

async fn catalog(state: &SharedState, provider: &str, remote: &Remote) -> VoiceResult<Vec<Value>> {
    // The base URL is part of the cache identity: an operator changing an
    // endpoint must not validate against the previous server's catalog.
    let cache_key = format!("{provider}\n{}", remote.base);
    {
        let cache = state.voice_chat.catalogs.lock().await;
        if let Some((at, models)) = cache.get(&cache_key)
            && at.elapsed() < CATALOG_TTL
        {
            return Ok(models.clone());
        }
    }
    let response = client()?
        .get(format!("{}/models", remote.base))
        .timeout(Duration::from_secs(8))
        .query(&[("output_modalities", "all")])
        .bearer_auth(&remote.key)
        .send()
        .await
        .map_err(|_| failure(StatusCode::BAD_GATEWAY, "Voice model discovery unavailable"))?;
    let bytes = response_bytes(response, MAX_CATALOG_BYTES).await?;
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|_| failure(StatusCode::BAD_GATEWAY, "Invalid voice model catalog"))?;
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| failure(StatusCode::BAD_GATEWAY, "Voice model catalog has no data"))?;
    let models: Vec<_> = data
        .iter()
        .filter(|m| {
            m.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty() && id.len() <= 256)
                && (supports(m, "speech")
                    || supports(m, "transcription")
                    || (has_modality(m, "input_modalities", "audio")
                        && has_modality(m, "output_modalities", "audio")))
        })
        .take(1000)
        .cloned()
        .collect();
    let mut cache = state.voice_chat.catalogs.lock().await;
    cache.retain(|_, (at, _)| at.elapsed() < CATALOG_TTL);
    if cache.len() >= 16 {
        cache.clear();
    }
    cache.insert(cache_key, (Instant::now(), models.clone()));
    Ok(models)
}

fn public_model(provider: &str, model: &Value) -> Value {
    json!({
        "id": model["id"], "provider": provider, "name": model.get("name").unwrap_or(&model["id"]),
        "input_modalities": model["architecture"]["input_modalities"],
        "output_modalities": model["architecture"]["output_modalities"],
        "voices": model.get("supported_voices").cloned().unwrap_or(json!([])),
        "languages": model.get("supported_languages").cloned().unwrap_or(Value::Null),
        "pricing": model.get("pricing").cloned().unwrap_or(Value::Null),
        "pricing_note": "Provider units; not a measured cost per call. Unknown units must not be treated as tokens.",
        "limits": model.get("per_request_limits").cloned().unwrap_or(Value::Null),
        "streaming": false, "available": true,
    })
}

/// GET /api/voice/models. Local availability is configuration, not a health or
/// acoustic quality claim. Remote discovery makes no inference request.
pub async fn models(State(state): State<SharedState>) -> VoiceResult<Json<Value>> {
    let _permit = state.voice_chat.discovery.try_acquire().map_err(|_| {
        failure(
            StatusCode::TOO_MANY_REQUESTS,
            "Voice discovery capacity reached",
        )
    })?;
    let cfg = state.current_config();
    let mut stt = vec![
        json!({"id":"configured", "provider":"local", "name":"Whisper configurado", "available":state.stt_client.is_some(), "voices":[], "languages":null, "pricing":null, "streaming":false}),
    ];
    let mut tts = vec![
        json!({"id":"configured", "provider":"local", "name":format!("{} configurado", cfg.voice.tts_provider), "available":state.voice_client.is_some() && cfg.voice.tts_provider != "hibiki", "voices":[], "languages":null, "pricing":null, "streaming":false, "reason": if cfg.voice.tts_provider == "hibiki" {"Legacy Hibiki sample server does not synthesize speech"} else {"Requires --with-voice and a working configured server"}}),
    ];
    let mut speech_to_speech = Vec::new();
    let mut limitations = vec!["Transcrição por fala e síntese por frase; sem transcrição parcial nem adaptador speech-to-speech.".to_string(), "Preços são metadados do provedor; custo total e teto monetário da chamada ainda não são garantidos.".to_string()];
    let mut providers: Vec<_> = cfg
        .llm
        .iter()
        .filter(|(_, c)| c.provider == "openrouter")
        .map(|(name, _)| name.clone())
        .collect();
    providers.sort();
    // Independent catalogs cannot consume the synthesis slots or multiply the
    // timeout by the number of configured instances.
    let discoveries = futures::future::join_all(providers.into_iter().take(8).map(|provider| {
        let state = &state;
        async move {
            let found = async {
                let remote = remote(state, &provider)?;
                catalog(state, &provider, &remote).await
            }
            .await;
            (provider, found)
        }
    }))
    .await;
    for (provider, found) in discoveries {
        match found {
            Ok(found) => {
                for model in found {
                    let mut public = public_model(&provider, &model);
                    if supports(&model, "transcription") {
                        stt.push(public.clone());
                    }
                    if supports(&model, "speech") {
                        tts.push(public.clone());
                    }
                    if has_modality(&model, "input_modalities", "audio")
                        && has_modality(&model, "output_modalities", "audio")
                    {
                        public["available"] = json!(false);
                        public["reason"] = json!("No verified realtime speech-to-speech adapter");
                        speech_to_speech.push(public);
                    }
                }
            }
            Err(_) => limitations.push(format!(
                "Catálogo de {provider} indisponível; nenhuma alternativa foi selecionada."
            )),
        }
    }
    Ok(Json(
        json!({"stt":stt, "tts":tts, "speech_to_speech":speech_to_speech, "limitations":limitations, "catalog_ttl_seconds":CATALOG_TTL.as_secs(), "default_language":cfg.voice.language}),
    ))
}

fn language(value: Option<&str>) -> VoiceResult<Option<&str>> {
    let value = value.filter(|v| !v.is_empty());
    if value
        .is_some_and(|v| v.len() > 35 || !v.chars().all(|c| c.is_ascii_alphabetic() || c == '-'))
    {
        return Err(bad_request("Invalid language code"));
    }
    Ok(value)
}

/// The browser encoder emits canonical PCM16 mono 16 kHz WAV, at most 30 s.
/// Reject malformed audio before it can trigger a paid inference request.
fn validate_utterance(audio: &[u8]) -> VoiceResult<()> {
    let invalid = || bad_request("Expected PCM16 mono 16 kHz WAV, between 1 sample and 30 seconds");
    if audio.len() < 46
        || audio.len() > MAX_UTTERANCE_BYTES
        || &audio[..4] != b"RIFF"
        || &audio[8..16] != b"WAVEfmt "
        || &audio[36..40] != b"data"
    {
        return Err(invalid());
    }
    let u32_at =
        |at| u32::from_le_bytes([audio[at], audio[at + 1], audio[at + 2], audio[at + 3]]) as usize;
    let u16_at = |at| u16::from_le_bytes([audio[at], audio[at + 1]]);
    let length = audio.len() - 44;
    if u32_at(4) != audio.len() - 8
        || u32_at(16) != 16
        || u16_at(20) != 1
        || u16_at(22) != 1
        || u32_at(24) != 16_000
        || u32_at(28) != 32_000
        || u16_at(32) != 2
        || u16_at(34) != 16
        || u32_at(40) != length
        || !length.is_multiple_of(2)
        || length > 30 * 32_000
    {
        return Err(invalid());
    }
    Ok(())
}

/// OpenRouter PCM is signed 16-bit little-endian. Rate/channel metadata is
/// mandatory: guessing a rate changes pitch and duration (and cost estimates).
fn pcm_to_wav(content_type: &str, pcm: &[u8]) -> VoiceResult<Vec<u8>> {
    let invalid = || {
        failure(
            StatusCode::BAD_GATEWAY,
            "PCM response requires explicit rate and channels metadata",
        )
    };
    let mut parts = content_type.split(';').map(str::trim);
    if parts.next() != Some("audio/pcm") {
        return Err(invalid());
    }
    let mut rate = None;
    let mut channels = None;
    for part in parts {
        if let Some((key, value)) = part.split_once('=') {
            match key {
                "rate" => rate = value.parse::<u32>().ok(),
                "channels" => channels = value.parse::<u16>().ok(),
                _ => (),
            }
        }
    }
    let rate = rate
        .filter(|r| (8_000..=192_000).contains(r))
        .ok_or_else(invalid)?;
    let channels = channels
        .filter(|c| (1..=2).contains(c))
        .ok_or_else(invalid)?;
    if pcm.is_empty()
        || !pcm.len().is_multiple_of(usize::from(channels) * 2)
        || pcm.len() > garraia_voice::http::MAX_AUDIO_BYTES
    {
        return Err(invalid());
    }
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(pcm);
    Ok(wav)
}

async fn selected_model(
    state: &SharedState,
    provider: &str,
    id: &str,
    capability: &str,
) -> VoiceResult<(Remote, Value)> {
    let remote = remote(state, provider)?;
    let models = catalog(state, provider, &remote).await?;
    let model = models
        .into_iter()
        .find(|m| m["id"].as_str() == Some(id) && supports(m, capability))
        .ok_or_else(|| bad_request("Model does not advertise the selected voice capability"))?;
    Ok((remote, model))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Synthesis {
    text: String,
    provider: String,
    model: String,
    voice: Option<String>,
    language: Option<String>,
}

/// POST /api/voice/synthesize. No text fallback: the caller must show errors.
pub async fn synthesize(
    State(state): State<SharedState>,
    Json(body): Json<Synthesis>,
) -> VoiceResult<Response> {
    let _permit = state
        .voice_chat
        .requests
        .try_acquire()
        .map_err(|_| failure(StatusCode::TOO_MANY_REQUESTS, "Voice capacity reached"))?;
    if body.text.trim().is_empty()
        || body.text.len() > MAX_SENTENCE_BYTES
        || body.provider.len() > 256
        || body.model.len() > 256
    {
        return Err(bad_request("Invalid or oversized speech request"));
    }
    let lang = language(body.language.as_deref())?;
    let work = async {
        if body.provider == "local" {
            if body.model != "configured" {
                return Err(bad_request("Unknown local voice model"));
            }
            let cfg = state.current_config();
            if cfg.voice.tts_provider == "hibiki" {
                return Err(bad_request(
                    "Legacy Hibiki sample server does not synthesize speech",
                ));
            }
            let tts = state.voice_client.as_ref().ok_or_else(|| {
                failure(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Local TTS is disabled; start with --with-voice",
                )
            })?;
            if body.voice.as_ref().is_some_and(|s| !s.is_empty()) {
                return Err(bad_request(
                    "Configured local synthesizer does not advertise selectable voices",
                ));
            }
            let audio = tts
                .synthesize_bytes(&body.text, lang.unwrap_or(&cfg.voice.language))
                .await
                .map_err(|_| {
                    failure(
                        StatusCode::BAD_GATEWAY,
                        "Local TTS failed; check server diagnostics",
                    )
                })?;
            if audio.is_empty() || audio.len() > garraia_voice::http::MAX_AUDIO_BYTES {
                return Err(failure(StatusCode::BAD_GATEWAY, "Invalid local audio size"));
            }
            return Ok((
                [("content-type", "audio/wav"), ("cache-control", "no-store")],
                audio,
            )
                .into_response());
        }
        let (remote, model) = selected_model(&state, &body.provider, &body.model, "speech").await?;
        let voice = body.voice.as_deref().filter(|s| !s.trim().is_empty());
        if voice.is_some_and(|s| s.len() > 256) {
            return Err(bad_request("Invalid voice"));
        }
        if let Some(voices) = model.get("supported_voices").and_then(Value::as_array)
            && !voices.is_empty()
            && !voice.is_some_and(|s| voices.iter().any(|v| v.as_str() == Some(s)))
        {
            return Err(bad_request("Select a voice advertised by this model"));
        }
        // These are protocol families, not a fixed model catalog. The official
        // speech contract requires PCM for Gemini TTS and HeyGen; MP3 works for
        // MiniMax/Voxtral. No failed inference is automatically retried.
        let pcm = body.model.starts_with("google/gemini-") || body.model.starts_with("heygen/");
        let mut request = json!({"model":body.model, "input":body.text, "response_format":if pcm {"pcm"} else {"mp3"}, "provider":remote.policy});
        if let Some(voice) = voice {
            request["voice"] = json!(voice);
        }
        // Language for remote TTS is determined by text/voice. Do not forward
        // undocumented fields that a provider could silently ignore.
        let response = client()?
            .post(format!("{}/audio/speech", remote.base))
            .bearer_auth(&remote.key)
            .json(&request)
            .send()
            .await
            .map_err(|_| failure(StatusCode::BAD_GATEWAY, "Remote TTS request failed"))?;
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let audio = response_bytes(response, garraia_voice::http::MAX_AUDIO_BYTES).await?;
        if pcm {
            let wav = pcm_to_wav(&content_type, &audio)?;
            return Ok((
                [("content-type", "audio/wav"), ("cache-control", "no-store")],
                wav,
            )
                .into_response());
        }
        if !matches!(
            content_type.split(';').next(),
            Some("audio/mpeg" | "audio/mp3")
        ) || audio.is_empty()
        {
            return Err(failure(
                StatusCode::BAD_GATEWAY,
                "Provider did not return supported MP3 audio",
            ));
        }
        Ok((
            [
                ("content-type", "audio/mpeg"),
                ("cache-control", "no-store"),
            ],
            audio,
        )
            .into_response())
    };
    tokio::time::timeout(REQUEST_TIMEOUT, work)
        .await
        .map_err(|_| failure(StatusCode::GATEWAY_TIMEOUT, "Speech synthesis timed out"))?
}

/// POST /api/voice/transcribe. A complete bounded WAV utterance from the
/// AudioWorklet; no file is written and no alternative provider is attempted.
pub async fn transcribe(
    State(state): State<SharedState>,
    mut multipart: Multipart,
) -> VoiceResult<Json<Value>> {
    let _permit = state
        .voice_chat
        .requests
        .try_acquire()
        .map_err(|_| failure(StatusCode::TOO_MANY_REQUESTS, "Voice capacity reached"))?;
    let work = async {
        let mut fields = HashMap::new();
        let mut audio = None;
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|_| bad_request("Invalid audio upload"))?
        {
            let name = field.name().unwrap_or("").to_string();
            if name == "file" {
                if audio.is_some() {
                    return Err(bad_request("Duplicate audio field"));
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| bad_request("Invalid audio upload"))?;
                validate_utterance(&bytes)?;
                audio = Some(bytes);
            } else if matches!(name.as_str(), "provider" | "model" | "language") {
                let value = field
                    .text()
                    .await
                    .map_err(|_| bad_request("Invalid voice selection"))?;
                if value.len() > 256 || fields.insert(name, value).is_some() {
                    return Err(bad_request("Invalid or duplicate voice selection"));
                }
            } else {
                return Err(bad_request("Unknown audio upload field"));
            }
        }
        let audio = audio.ok_or_else(|| bad_request("Missing audio"))?;
        let provider = fields
            .get("provider")
            .ok_or_else(|| bad_request("Missing STT provider"))?;
        let model = fields
            .get("model")
            .ok_or_else(|| bad_request("Missing STT model"))?;
        let lang = language(fields.get("language").map(String::as_str))?;
        let mut usage = Value::Null;
        let text = if provider == "local" {
            if model != "configured" {
                return Err(bad_request("Unknown local STT model"));
            }
            let stt = state.stt_client.as_ref().ok_or_else(|| {
                failure(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Local STT is disabled; start with --with-voice",
                )
            })?;
            stt.transcribe_bytes(&audio, "utterance.wav", "audio/wav", lang, None)
                .await
                .map_err(|_| {
                    failure(
                        StatusCode::BAD_GATEWAY,
                        "Local transcription failed; check server diagnostics",
                    )
                })?
        } else {
            let (remote, _) = selected_model(&state, provider, model, "transcription").await?;
            let mut request = json!({"model":model, "input_audio":{"data":base64::engine::general_purpose::STANDARD.encode(&audio), "format":"wav"}, "provider":remote.policy});
            if let Some(lang) = lang {
                request["language"] = json!(lang.split('-').next().unwrap_or(lang));
            }
            let response = client()?
                .post(format!("{}/audio/transcriptions", remote.base))
                .bearer_auth(&remote.key)
                .json(&request)
                .send()
                .await
                .map_err(|_| {
                    failure(
                        StatusCode::BAD_GATEWAY,
                        "Remote transcription request failed",
                    )
                })?;
            let bytes = response_bytes(response, 1024 * 1024).await?;
            let result: Value = serde_json::from_slice(&bytes)
                .map_err(|_| failure(StatusCode::BAD_GATEWAY, "Invalid transcription response"))?;
            if let Some(upstream_usage) = result.get("usage") {
                // Numeric usage only: never forward an arbitrary provider body.
                let mut public_usage = serde_json::Map::new();
                for key in [
                    "cost",
                    "input_tokens",
                    "output_tokens",
                    "total_tokens",
                    "seconds",
                ] {
                    if let Some(value) = upstream_usage.get(key).filter(|v| v.is_number()) {
                        public_usage.insert(key.to_string(), value.clone());
                    }
                }
                usage = Value::Object(public_usage);
            }
            result
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    failure(
                        StatusCode::BAD_GATEWAY,
                        "Transcription response has no text",
                    )
                })?
                .to_string()
        };
        if text.len() > garraia_voice::http::MAX_TEXT_BYTES {
            return Err(failure(
                StatusCode::BAD_GATEWAY,
                "Transcription exceeds size limit",
            ));
        }
        Ok(Json(json!({"text":text.trim(), "usage":usage})))
    };
    tokio::time::timeout(REQUEST_TIMEOUT, work)
        .await
        .map_err(|_| failure(StatusCode::GATEWAY_TIMEOUT, "Transcription timed out"))?
}
