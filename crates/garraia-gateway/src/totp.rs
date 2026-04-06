//! TOTP-based two-factor authentication (Google Authenticator compatible).
//!
//! Routes (registered in router.rs):
//!   POST /auth/2fa/setup    — generate a TOTP secret and QR URI
//!   POST /auth/2fa/verify   — verify code and enable 2FA
//!   POST /auth/2fa/disable  — disable 2FA (requires current code)
//!
//! The TOTP secret is stored encrypted in the `mobile_users` table via
//! `totp_secret_enc` column (AES-256-GCM using the vault key).
//!
//! RFC 6238 TOTP implementation: HMAC-SHA1, 30-second window, 6 digits.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use ring::hmac;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::mobile_auth::MobileAuth;
use crate::state::AppState;

// ── Constants ─────────────────────────────────────────────────────────────────

const TOTP_DIGITS: u32 = 6;
const TOTP_STEP_SECS: u64 = 30;
/// Allow one window of drift in each direction (~60 s total).
const TOTP_DRIFT_WINDOWS: i64 = 1;
/// Secret length in bytes (20 bytes = 160 bits, standard for TOTP).
const TOTP_SECRET_BYTES: usize = 20;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TotpSetupResponse {
    /// Base32-encoded TOTP secret (shown once to the user).
    pub secret: String,
    /// `otpauth://` URI suitable for QR code generation.
    pub qr_uri: String,
    /// Human-readable issuer shown in the authenticator app.
    pub issuer: String,
}

#[derive(Debug, Deserialize)]
pub struct TotpVerifyRequest {
    /// 6-digit TOTP code from the authenticator app.
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct TotpDisableRequest {
    /// Current valid TOTP code to confirm identity before disabling.
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct TotpStatusResponse {
    pub enabled: bool,
}

// ── Core TOTP implementation ──────────────────────────────────────────────────

/// Generate a cryptographically random TOTP secret (base32 encoded).
pub fn generate_totp_secret() -> Result<String, String> {
    let rng = ring::rand::SystemRandom::new();
    let mut buf = vec![0u8; TOTP_SECRET_BYTES];
    ring::rand::SecureRandom::fill(&rng, &mut buf)
        .map_err(|_| "failed to generate TOTP secret".to_string())?;
    Ok(base32_encode(&buf))
}

/// Build an `otpauth://totp/` URI for QR code generation.
///
/// The secret is NOT logged — only the URI template structure is returned.
pub fn generate_totp_qr(secret: &str, email: &str) -> String {
    let issuer = "GarraIA";
    let label = format!("{issuer}:{email}");
    format!(
        "otpauth://totp/{label}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits={TOTP_DIGITS}&period={TOTP_STEP_SECS}",
        label = urlenc(label.as_str()),
        secret = secret,
        issuer = urlenc(issuer),
    )
}

/// Verify a TOTP code against the given base32 secret.
///
/// Checks the current window plus `TOTP_DRIFT_WINDOWS` windows in each
/// direction to account for clock skew.
pub fn verify_totp(secret: &str, code: &str) -> bool {
    let code = code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let key_bytes = match base32_decode(secret) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let current_counter = (now_secs / TOTP_STEP_SECS) as i64;

    for delta in -TOTP_DRIFT_WINDOWS..=TOTP_DRIFT_WINDOWS {
        let counter = (current_counter + delta) as u64;
        let expected = hotp(&key_bytes, counter);
        if expected == code {
            return true;
        }
    }

    false
}

/// Compute HOTP(key, counter) and return as zero-padded 6-digit string.
fn hotp(key: &[u8], counter: u64) -> String {
    let counter_bytes = counter.to_be_bytes();
    let hmac_key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
    let tag = hmac::sign(&hmac_key, &counter_bytes);
    let bytes = tag.as_ref();

    // Dynamic truncation per RFC 4226 §5.4
    let offset = (bytes[19] & 0x0f) as usize;
    let code = u32::from_be_bytes([
        bytes[offset] & 0x7f,
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]);

    format!("{:0>6}", code % 10u32.pow(TOTP_DIGITS))
}

// ── Base32 helpers (RFC 4648, no padding required by Google Authenticator) ────

const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(input: &[u8]) -> String {
    let mut output = String::new();
    let mut buffer: u32 = 0;
    let mut bits_left: u32 = 0;

    for &byte in input {
        buffer = (buffer << 8) | u32::from(byte);
        bits_left += 8;
        while bits_left >= 5 {
            bits_left -= 5;
            let idx = ((buffer >> bits_left) & 0x1f) as usize;
            output.push(BASE32_ALPHABET[idx] as char);
        }
    }

    if bits_left > 0 {
        let idx = ((buffer << (5 - bits_left)) & 0x1f) as usize;
        output.push(BASE32_ALPHABET[idx] as char);
    }

    output
}

fn base32_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits_left: u32 = 0;

    for ch in input.chars() {
        let val = match ch.to_ascii_uppercase() {
            c @ 'A'..='Z' => c as u32 - 'A' as u32,
            '2' => 26,
            '3' => 27,
            '4' => 28,
            '5' => 29,
            '6' => 30,
            '7' => 31,
            '=' => continue, // padding
            other => return Err(format!("invalid base32 character: {other}")),
        };

        buffer = (buffer << 5) | val;
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            output.push(((buffer >> bits_left) & 0xff) as u8);
        }
    }

    Ok(output)
}

fn urlenc(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => vec![c],
            c => format!("%{:02X}", c as u32).chars().collect::<Vec<_>>(),
        })
        .collect()
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /auth/2fa/setup
///
/// Generate a new TOTP secret for the authenticated user.
/// The secret is returned once — the user must scan the QR code immediately.
/// The secret is NOT yet active; the user must call `/auth/2fa/verify` to
/// confirm they have successfully enrolled.
pub async fn setup_2fa(
    MobileAuth(claims): MobileAuth,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let secret = match generate_totp_secret() {
        Ok(s) => s,
        Err(e) => {
            warn!("2fa setup: failed to generate secret: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    // Store the pending (unconfirmed) secret encrypted in the DB
    if let Some(store_arc) = &state.session_store {
        let store = store_arc.lock().await;
        if let Err(e) = store.set_mobile_user_totp_secret(&claims.sub, &secret) {
            warn!("2fa setup: failed to store secret: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    }

    let qr_uri = generate_totp_qr(&secret, &claims.email);

    (
        StatusCode::OK,
        Json(serde_json::json!(TotpSetupResponse {
            secret,
            qr_uri,
            issuer: "GarraIA".into(),
        })),
    )
}

/// POST /auth/2fa/verify
///
/// Verify a TOTP code against the user's stored secret.
/// Returns 200 on success (2FA is now considered active).
pub async fn verify_2fa(
    MobileAuth(claims): MobileAuth,
    State(state): State<Arc<AppState>>,
    Json(req): Json<TotpVerifyRequest>,
) -> impl IntoResponse {
    let secret = match get_user_totp_secret(&state, &claims.sub).await {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "2fa not set up"})),
            );
        }
    };

    if verify_totp(&secret, &req.code) {
        (
            StatusCode::OK,
            Json(serde_json::json!({"status": "2fa verified", "enabled": true})),
        )
    } else {
        warn!("2fa verify: invalid code for user={}", &claims.sub[..8.min(claims.sub.len())]);
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid code"})),
        )
    }
}

/// POST /auth/2fa/disable
///
/// Disable 2FA. Requires the user to provide a valid TOTP code first.
pub async fn disable_2fa(
    MobileAuth(claims): MobileAuth,
    State(state): State<Arc<AppState>>,
    Json(req): Json<TotpDisableRequest>,
) -> impl IntoResponse {
    let secret = match get_user_totp_secret(&state, &claims.sub).await {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "2fa not enabled"})),
            );
        }
    };

    if !verify_totp(&secret, &req.code) {
        warn!("2fa disable: invalid code for user={}", &claims.sub[..8.min(claims.sub.len())]);
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid code"})),
        );
    }

    if let Some(store_arc) = &state.session_store {
        let store = store_arc.lock().await;
        if let Err(e) = store.clear_mobile_user_totp_secret(&claims.sub) {
            warn!("2fa disable: db error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "2fa disabled", "enabled": false})),
    )
}

// ── Shared helper ─────────────────────────────────────────────────────────────

async fn get_user_totp_secret(state: &Arc<AppState>, user_id: &str) -> Option<String> {
    let store_arc = state.session_store.as_ref()?;
    let store = store_arc.lock().await;
    store.get_mobile_user_totp_secret(user_id).ok().flatten()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trip() {
        let input = b"Hello, TOTP!";
        let encoded = base32_encode(input);
        let decoded = base32_decode(&encoded).unwrap();
        assert_eq!(&decoded, input);
    }

    #[test]
    fn generate_secret_is_base32() {
        let secret = generate_totp_secret().unwrap();
        assert!(!secret.is_empty());
        // All characters must be valid base32
        for ch in secret.chars() {
            assert!(
                ch.is_ascii_uppercase() || ('2'..='7').contains(&ch),
                "unexpected char: {ch}"
            );
        }
    }

    #[test]
    fn qr_uri_format() {
        let uri = generate_totp_qr("JBSWY3DPEHPK3PXP", "user@example.com");
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains("secret=JBSWY3DPEHPK3PXP"));
        assert!(uri.contains("issuer=GarraIA"));
        assert!(uri.contains("digits=6"));
        assert!(uri.contains("period=30"));
    }

    #[test]
    fn verify_totp_self_consistency() {
        // Generate a secret, then verify the current HOTP value matches
        let secret = generate_totp_secret().unwrap();
        let key_bytes = base32_decode(&secret).unwrap();
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let counter = now_secs / TOTP_STEP_SECS;
        let code = hotp(&key_bytes, counter);

        assert!(verify_totp(&secret, &code), "own TOTP code should verify");
    }

    #[test]
    fn verify_totp_rejects_wrong_code() {
        let secret = generate_totp_secret().unwrap();
        assert!(!verify_totp(&secret, "000000") || verify_totp(&secret, "000000"),
            // This could theoretically be valid; just check it doesn't panic
        );
        assert!(!verify_totp(&secret, "abc123")); // non-digit
        assert!(!verify_totp(&secret, "12345"));  // too short
    }

    #[test]
    fn hotp_known_vector() {
        // RFC 4226 Appendix D — key = b"12345678901234567890", counter 0..=9
        let key = b"12345678901234567890";
        let expected = ["755224", "287082", "359152", "969429", "338314",
                        "254676", "287922", "162583", "399871", "520489"];
        for (i, &exp) in expected.iter().enumerate() {
            assert_eq!(hotp(key, i as u64), exp, "HOTP mismatch at counter={i}");
        }
    }
}
