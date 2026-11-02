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
    time::{Duration, Instant, SystemTime},
};
use tokio::sync::{Mutex, Semaphore};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CATALOG_TTL: Duration = Duration::from_secs(300);
const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
const MAX_UTTERANCE_BYTES: usize = 1024 * 1024;
const MAX_SENTENCE_BYTES: usize = 4096;
/// Concurrent STT/TTS requests per gateway (all users together).
const MAX_CONCURRENT_VOICE_REQUESTS: usize = 4;
/// Concurrent catalog discoveries per gateway.
const MAX_CONCURRENT_DISCOVERIES: usize = 2;

/// A cached provider catalog: when it was fetched (monotonic for the TTL,
/// wall clock for the UI) and the voice-capable models it listed.
struct CachedCatalog {
    at: Instant,
    fetched_at: SystemTime,
    models: Vec<Value>,
}

/// Per-gateway capacity. Permits are released when the request is cancelled.
pub struct VoiceChat {
    requests: Semaphore,
    discovery: Semaphore,
    catalogs: Mutex<HashMap<String, CachedCatalog>>,
}

impl Default for VoiceChat {
    fn default() -> Self {
        Self {
            requests: Semaphore::new(MAX_CONCURRENT_VOICE_REQUESTS),
            discovery: Semaphore::new(MAX_CONCURRENT_DISCOVERIES),
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
        // https anywhere; http only to a loopback IP literal, never a name
        // (same rule as the Signal guard; see the CodeQL ledger).
        || !credential_safe_endpoint(&url)
    {
        return Err(bad_request(
            "Voice endpoint must be HTTPS (or HTTP to 127.0.0.1/[::1]), without credentials or query",
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

async fn catalog(
    state: &SharedState,
    provider: &str,
    remote: &Remote,
) -> VoiceResult<(Vec<Value>, SystemTime)> {
    // The base URL is part of the cache identity: an operator changing an
    // endpoint must not validate against the previous server's catalog.
    let cache_key = format!("{provider}\n{}", remote.base);
    {
        let cache = state.voice_chat.catalogs.lock().await;
        if let Some(cached) = cache.get(&cache_key)
            && cached.at.elapsed() < CATALOG_TTL
        {
            return Ok((cached.models.clone(), cached.fetched_at));
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
    let fetched_at = SystemTime::now();
    let mut cache = state.voice_chat.catalogs.lock().await;
    cache.retain(|_, cached| cached.at.elapsed() < CATALOG_TTL);
    if cache.len() >= 16 {
        cache.clear();
    }
    cache.insert(
        cache_key,
        CachedCatalog {
            at: Instant::now(),
            fetched_at,
            models: models.clone(),
        },
    );
    Ok((models, fetched_at))
}

/// Prices exactly as the catalog states them. OpenRouter's speech and
/// transcription entries carry decimal USD strings without a unit (per
/// character, per second or per token depending on the model), so the unit is
/// reported as unknown instead of being assumed to be tokens (ADR 0027).
fn public_prices(model: &Value) -> Value {
    let Some(pricing) = model.get("pricing").and_then(Value::as_object) else {
        return Value::Null;
    };
    let prices: Vec<Value> = pricing
        .iter()
        .filter_map(|(field, value)| {
            let usd = value.as_str()?;
            // A decimal string, never an arbitrary provider text.
            (usd.len() <= 32 && usd.parse::<f64>().is_ok_and(|v| v.is_finite() && v >= 0.0))
                .then(|| json!({"field": field, "usd": usd, "unit": Value::Null}))
        })
        .take(16)
        .collect();
    Value::Array(prices)
}

fn utc_rfc3339(at: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(at).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn public_model(provider: &str, model: &Value) -> Value {
    json!({
        "id": model["id"], "provider": provider, "name": model.get("name").unwrap_or(&model["id"]),
        "input_modalities": model["architecture"]["input_modalities"],
        "output_modalities": model["architecture"]["output_modalities"],
        "voices": model.get("supported_voices").cloned().unwrap_or(json!([])),
        "languages": model.get("supported_languages").cloned().unwrap_or(Value::Null),
        "pricing": model.get("pricing").cloned().unwrap_or(Value::Null),
        "prices": public_prices(model),
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
    let stt_available = state.stt_client.is_some();
    let mut local_stt = json!({"id":"configured", "provider":"local", "name":"Whisper local (configurado)", "available":stt_available, "voices":[], "languages":null, "pricing":null, "prices":null, "streaming":false});
    if !stt_available {
        local_stt["reason"] =
            json!("Start the gateway with --with-voice and a configured Whisper server");
    }
    let mut stt = vec![local_stt];
    let tts_reason = if cfg.voice.tts_provider == "hibiki" {
        Some("Legacy Hibiki sample server does not synthesize speech")
    } else if state.voice_client.is_none() {
        Some("Start the gateway with --with-voice and a configured TTS server")
    } else {
        None
    };
    let mut local_tts = json!({"id":"configured", "provider":"local", "name":format!("{} local (configurado)", cfg.voice.tts_provider), "available":tts_reason.is_none(), "voices":[], "languages":null, "pricing":null, "prices":null, "streaming":false});
    if let Some(reason) = tts_reason {
        local_tts["reason"] = json!(reason);
    }
    let mut tts = vec![local_tts];
    let mut catalogs = Vec::new();
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
            Ok((found, fetched_at)) => {
                catalogs.push(json!({"provider":provider, "fetched_at":utc_rfc3339(fetched_at), "ttl_seconds":CATALOG_TTL.as_secs(), "voice_models":found.len()}));
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
            Err(_) => {
                catalogs.push(
                    json!({"provider":provider, "fetched_at":Value::Null, "available":false}),
                );
                limitations.push(format!(
                    "Catálogo de {provider} indisponível; nenhuma alternativa foi selecionada."
                ))
            }
        }
    }
    // The operator's text-model fallback chain applies to voice turns too.
    // Shown up front, and the reply frame names who actually answered.
    let llm_fallback_providers = state.agents.fallback_providers();
    if !llm_fallback_providers.is_empty() {
        limitations.push(format!(
            "Fallback de conversa configurado pelo operador: {}. Se o provedor escolhido falhar, um deles pode responder; a chamada mostra quando isso acontecer.",
            llm_fallback_providers.join(", ")
        ));
    }
    Ok(Json(
        json!({"stt":stt, "tts":tts, "speech_to_speech":speech_to_speech, "limitations":limitations, "catalogs":catalogs, "catalog_ttl_seconds":CATALOG_TTL.as_secs(), "default_language":cfg.voice.language, "llm_fallback_providers":llm_fallback_providers, "limits":{"max_utterance_seconds":30, "max_sentence_bytes":MAX_SENTENCE_BYTES, "max_concurrent_requests":MAX_CONCURRENT_VOICE_REQUESTS}}),
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

/// Output format requested from `POST /audio/speech`, per model family, as
/// documented by OpenRouter (ADR 0027). This is a protocol rule, not a model
/// catalog: unknown families ask for MP3 and a mismatch surfaces as an error,
/// never as a silent second generation in another format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpeechFormat {
    /// PCM16 LE. `default_layout` is used only when the MIME omits
    /// `rate`/`channels` and the family's layout is documented.
    Pcm {
        default_layout: Option<(u32, u16)>,
    },
    Mp3,
}

fn speech_format(model: &str) -> SpeechFormat {
    if model.starts_with("google/gemini-") {
        // Official cookbook: audio/pcm;rate=24000;channels=1.
        SpeechFormat::Pcm {
            default_layout: Some((24_000, 1)),
        }
    } else if model.starts_with("heygen/") {
        SpeechFormat::Pcm {
            default_layout: None,
        }
    } else {
        SpeechFormat::Mp3
    }
}

/// 20 ms of PCM16 mono at 16 kHz.
const GATE_FRAME_SAMPLES: usize = 320;
/// RMS (full scale = 1.0) a frame needs to count as voiced: about -44 dBFS,
/// well below the browser VAD floor (0.012), so the gate never second-guesses
/// a real start detected by the client.
const GATE_FRAME_RMS: f64 = 0.006;
/// Voiced frames an utterance needs: 80 ms.
const GATE_MIN_VOICED_FRAMES: usize = 4;

/// Energy gate run on the server before any STT request.
///
/// It is a cost guard, not a speech classifier: steady loud noise passes, and
/// that limitation is documented. `audio` must already have passed
/// [`validate_utterance`] (44-byte header, PCM16 mono 16 kHz).
fn utterance_has_speech(audio: &[u8]) -> bool {
    let Some(pcm) = audio.get(44..) else {
        return false;
    };
    let voiced = pcm
        .chunks(GATE_FRAME_SAMPLES * 2)
        .filter(|frame| {
            let samples = frame.len() / 2;
            if samples == 0 {
                return false;
            }
            let energy: f64 = frame
                .chunks_exact(2)
                .map(|b| {
                    let v = f64::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0;
                    v * v
                })
                .sum();
            (energy / samples as f64).sqrt() >= GATE_FRAME_RMS
        })
        .count();
    voiced >= GATE_MIN_VOICED_FRAMES
}

/// OpenRouter PCM is signed 16-bit little-endian. Rate/channel metadata is
/// mandatory unless the family documents a fixed layout: guessing a rate
/// changes pitch and duration (and cost estimates).
fn pcm_to_wav(
    content_type: &str,
    pcm: &[u8],
    default_layout: Option<(u32, u16)>,
) -> VoiceResult<Vec<u8>> {
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
    let (rate, channels) = match (rate, channels, default_layout) {
        (None, None, Some(layout)) => (Some(layout.0), Some(layout.1)),
        _ => (rate, channels),
    };
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
    let (models, _) = catalog(state, provider, &remote).await?;
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
        let format = speech_format(&body.model);
        let pcm = matches!(format, SpeechFormat::Pcm { .. });
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
        if let SpeechFormat::Pcm { default_layout } = format {
            let wav = pcm_to_wav(&content_type, &audio, default_layout)?;
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
        if !utterance_has_speech(&audio) {
            // Digital silence or a near-silent upload (OS-level mute, a
            // closed lid, a VAD false start): answer without contacting — and
            // paying — any STT provider.
            return Ok(Json(
                json!({"text":"", "usage":Value::Null, "skipped":"silence"}),
            ));
        }
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

/// Where an OpenRouter key may be sent: `https` anywhere, plain `http` only to
/// a loopback **IP literal** (`127.0.0.1`, `[::1]`; a local test double or
/// proxy). No host name is accepted over `http`, so no resolver or hosts file
/// can move the bearer off the machine. Redirects are disabled on the voice
/// client, and the key travels only in the `Authorization` header, never in
/// the URL.
fn credential_safe_endpoint(url: &reqwest::Url) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => url
            .host_str()
            .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
            .and_then(|h| h.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PCM16 mono 16 kHz WAV from samples in [-1, 1].
    fn wav(samples: &[f64]) -> Vec<u8> {
        let mut out = Vec::with_capacity(44 + samples.len() * 2);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + samples.len() as u32 * 2).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&16_000_u32.to_le_bytes());
        out.extend_from_slice(&32_000_u32.to_le_bytes());
        out.extend_from_slice(&2_u16.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        for s in samples {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        out
    }

    fn ms(n: usize) -> usize {
        n * 16
    }

    /// A 220 Hz tone: a stand-in for voiced speech energy.
    fn tone(len: usize, amplitude: f64) -> Vec<f64> {
        (0..len)
            .map(|i| amplitude * (i as f64 * 220.0 * std::f64::consts::TAU / 16_000.0).sin())
            .collect()
    }

    /// Deterministic pseudo-random noise (no `rand` in the gateway tests).
    fn noise(len: usize, amplitude: f64, seed: u32) -> Vec<f64> {
        let mut x = seed.max(1);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                amplitude * (f64::from(x) / f64::from(u32::MAX) * 2.0 - 1.0)
            })
            .collect()
    }

    #[test]
    fn simulated_pcm_passes_the_wav_contract() {
        assert!(validate_utterance(&wav(&tone(ms(500), 0.2))).is_ok());
        // 30 s is the ceiling; one more sample is refused.
        assert!(validate_utterance(&wav(&vec![0.0; ms(30_000)])).is_ok());
        assert!(validate_utterance(&wav(&vec![0.0; ms(30_000) + 1])).is_err());
        assert!(validate_utterance(&wav(&[])).is_err(), "no samples");
        let mut stereo = wav(&tone(ms(100), 0.2));
        stereo[22] = 2;
        assert!(validate_utterance(&stereo).is_err());
        let mut wrong_rate = wav(&tone(ms(100), 0.2));
        wrong_rate[24..28].copy_from_slice(&48_000_u32.to_le_bytes());
        assert!(validate_utterance(&wrong_rate).is_err());
    }

    #[test]
    fn digital_silence_and_low_noise_do_not_reach_stt() {
        assert!(!utterance_has_speech(&wav(&vec![0.0; ms(2_000)])));
        assert!(!utterance_has_speech(&wav(&noise(ms(2_000), 0.003, 7))));
    }

    #[test]
    fn a_click_is_not_speech_but_a_short_word_is() {
        // 20 ms burst inside silence: one voiced frame.
        let mut click = vec![0.0; ms(500)];
        click.splice(ms(200)..ms(220), tone(ms(20), 0.5));
        assert!(!utterance_has_speech(&wav(&click)));
        // 150 ms of voice ("sim") with pre-roll and trailing silence.
        let mut word = vec![0.0; ms(240)];
        word.extend(tone(ms(150), 0.1));
        word.extend(vec![0.0; ms(800)]);
        assert!(utterance_has_speech(&wav(&word)));
    }

    #[test]
    fn speech_with_pauses_counts_every_voiced_segment() {
        let mut talk = Vec::new();
        for _ in 0..3 {
            talk.extend(tone(ms(40), 0.05));
            talk.extend(vec![0.0; ms(400)]);
        }
        assert!(
            utterance_has_speech(&wav(&talk)),
            "3 x 40 ms = 120 ms voiced"
        );
    }

    /// Documented limitation: the gate measures energy, so loud steady noise
    /// passes. The browser VAD adapts to the noise floor; the server only
    /// stops silence from being billed.
    #[test]
    fn loud_noise_passes_the_energy_gate() {
        assert!(utterance_has_speech(&wav(&noise(ms(1_000), 0.2, 3))));
    }

    #[test]
    fn pcm_needs_explicit_layout_unless_the_family_documents_it() {
        let pcm = vec![0_u8; 480];
        let wav = pcm_to_wav("audio/pcm;rate=24000;channels=1", &pcm, None).expect("wav");
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            24_000
        );
        assert_eq!(wav.len(), 44 + pcm.len());
        assert!(pcm_to_wav("audio/pcm", &pcm, None).is_err(), "rate unknown");
        let gemini = pcm_to_wav("audio/pcm", &pcm, Some((24_000, 1))).expect("documented");
        assert_eq!(u16::from_le_bytes([gemini[22], gemini[23]]), 1);
        // Partial metadata never mixes with the default.
        assert!(pcm_to_wav("audio/pcm;rate=24000", &pcm, Some((24_000, 1))).is_err());
        assert!(pcm_to_wav("audio/mpeg", &pcm, Some((24_000, 1))).is_err());
        assert!(pcm_to_wav("audio/pcm;rate=24000;channels=2", &[0; 6], None).is_err());
    }

    #[test]
    fn credentials_go_only_to_https_or_a_loopback_ip_literal() {
        let ok = |u: &str| credential_safe_endpoint(&reqwest::Url::parse(u).unwrap());
        assert!(ok("https://openrouter.ai/api/v1"));
        assert!(ok("https://example.com:8443/v1"));
        assert!(ok("http://127.0.0.1:8080/v1"));
        assert!(ok("http://[::1]:8080/v1"));
        assert!(
            !ok("http://localhost:8080/v1"),
            "names are not trusted over http"
        );
        assert!(!ok("http://openrouter.ai/api/v1"));
        assert!(!ok("http://10.0.0.5/v1"));
        assert!(!ok("http://169.254.169.254/v1"));
        assert!(!ok("ftp://127.0.0.1/v1"));
    }

    #[test]
    fn speech_format_is_a_protocol_rule_not_a_catalog() {
        assert_eq!(
            speech_format("google/gemini-3.8-flash-tts"),
            SpeechFormat::Pcm {
                default_layout: Some((24_000, 1))
            }
        );
        assert_eq!(
            speech_format("heygen/heygen-voice-1-turbo"),
            SpeechFormat::Pcm {
                default_layout: None
            }
        );
        assert_eq!(
            speech_format("mistralai/voxtral-mini-tts-3-0"),
            SpeechFormat::Mp3
        );
    }

    #[test]
    fn prices_keep_the_provider_value_and_never_invent_a_unit() {
        let prices = public_prices(
            &json!({"pricing":{"prompt":"0.000015","completion":"0","weird":"ten dollars","nested":{"x":1}}}),
        );
        let prices = prices.as_array().expect("array");
        assert_eq!(prices.len(), 2, "{prices:?}");
        assert!(prices.iter().all(|p| p["unit"].is_null()));
        assert!(
            prices
                .iter()
                .any(|p| p["field"] == "prompt" && p["usd"] == "0.000015")
        );
        assert_eq!(public_prices(&json!({"id":"x"})), Value::Null);
    }
}
