//! Admin password recovery assisted by the CLI (#1122).
//!
//! The code **never** travels in an HTTP response: `/recovery/start` mints a
//! one-time code, keeps only its PBKDF2 hash, and writes the plaintext to a
//! `0600` file inside the data dir. Reading it therefore requires shell access
//! on the host — the same bar as `garra admin recovery`, which is the intended
//! reader. Nothing in this module logs a code.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Router, middleware as axum_mw};

use super::middleware::extract_ip;
use super::shared::AdminState;
use super::store::AdminStore;
use crate::rate_limiter::RateLimiter;

/// Name of the handoff file, inside the resolved data dir. Public so
/// `garraia-cli` reads the exact path the gateway writes — two independent
/// copies of this string would drift.
pub const RECOVERY_CODE_FILE: &str = "admin-recovery.code";

/// Codes live 10 minutes: long enough for an operator to `cat` the file and
/// paste it, short enough to bound the brute-force window.
const RECOVERY_TTL_SECS: i64 = 600;

/// Same floor as `admin/users.rs` (setup + create user).
const MIN_PASSWORD_LEN: usize = 8;

/// Byte-identical for every `/start` outcome — existing user, unknown user,
/// panel not provisioned. Any difference would make the endpoint a user
/// enumeration oracle.
const GENERIC_START_MESSAGE: &str = "Se o usuario existir, um codigo de uso unico foi gerado \
     no host. Rode `garra admin recovery` no servidor para le-lo.";

/// Unauthenticated sub-router. Merged into the admin `public_routes`, so it
/// gets `security_headers` but never `require_admin_auth` / `require_csrf`.
pub fn recovery_router(admin_state: AdminState) -> Router {
    Router::new()
        .route("/api/recovery/start", post(start_recovery))
        .route("/api/recovery/complete", post(complete_recovery))
        // Rate limiting is the only thing standing between a 256-bit code and
        // a hostile caller, so it wraps the handlers as a layer rather than
        // living inside them — easy to forget otherwise.
        .layer(axum_mw::from_fn(recovery_rate_limit))
        .layer(Extension(RateLimiter::auth_limiter()))
        .with_state(admin_state)
}

/// Per-IP sliding window, reusing the gateway's auth limiter (10/min).
async fn recovery_rate_limit(
    Extension(limiter): Extension<Arc<RateLimiter>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    // TODO(plan-0023+): `extract_ip` trusts `X-Forwarded-For` without checking
    // whether the peer is an allowlisted proxy, so a spoofed header buys a
    // fresh bucket per fake IP. Same known gap as the rest of the admin
    // surface; lifting `real_client_ip` there fixes it here too.
    let key = format!(
        "admin-recovery:{}",
        extract_ip(&headers, None).unwrap_or_else(|| "unknown".to_string())
    );

    if !limiter.check(&key).allowed {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "too many requests"})),
        )
            .into_response();
    }

    next.run(request).await
}

#[derive(serde::Deserialize)]
pub struct RecoveryStartRequest {
    pub username: String,
}

/// POST /admin/api/recovery/start — mint a code and hand it off out-of-band.
pub async fn start_recovery(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<RecoveryStartRequest>,
) -> impl IntoResponse {
    let ip = extract_ip(&headers, None);
    let username = body.username.trim();
    let mut guard = state.store.lock().await;

    // Nothing to recover. Refusing here is what keeps `/start` from becoming
    // an account-creation path on a panel that is still unprovisioned.
    if guard.user_count() == 0 {
        let _ = guard.append_audit(
            None,
            Some(username),
            "recovery.start",
            "auth",
            None,
            Some("refused: admin panel not provisioned"),
            ip.as_deref(),
            "failure",
        );
        return generic_start_response();
    }

    let Some(user) = guard.get_user_by_username(username) else {
        // Spend the same PBKDF2 work as the found branch so the response time
        // does not split the two cases.
        let _ = guard.burn_recovery_work();
        let _ = guard.append_audit(
            None,
            Some(username),
            "recovery.start",
            "auth",
            None,
            Some("unknown user"),
            ip.as_deref(),
            "failure",
        );
        return generic_start_response();
    };

    let (token_id, code) = match guard.create_recovery_token(&user.id, RECOVERY_TTL_SECS) {
        Ok(pair) => pair,
        Err(e) => {
            let _ = guard.append_audit(
                Some(&user.id),
                Some(&user.username),
                "recovery.start",
                "auth",
                None,
                Some(&format!("failed to mint code: {e}")),
                ip.as_deref(),
                "failure",
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "could not start recovery"})),
            )
                .into_response();
        }
    };

    // Fail-closed: a code the operator cannot read must not stay live.
    let write_result = safe_data_dir(&state.app_state.config)
        .and_then(|data_dir| write_code_file(&data_dir, &user.username, &code));
    if let Err(e) = write_result {
        let _ = guard.discard_recovery_token(token_id);
        let _ = guard.append_audit(
            Some(&user.id),
            Some(&user.username),
            "recovery.start",
            "auth",
            None,
            Some(&format!("failed to write handoff file: {e}")),
            ip.as_deref(),
            "failure",
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "could not start recovery"})),
        )
            .into_response();
    }

    let _ = guard.append_audit(
        Some(&user.id),
        Some(&user.username),
        "recovery.start",
        "auth",
        None,
        Some("one-time code handed off out-of-band"),
        ip.as_deref(),
        "success",
    );

    generic_start_response()
}

#[derive(serde::Deserialize)]
pub struct RecoveryCompleteRequest {
    pub username: String,
    pub code: String,
    pub new_password: String,
}

/// POST /admin/api/recovery/complete — consume `{username, code, new_password}`.
pub async fn complete_recovery(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<RecoveryCompleteRequest>,
) -> impl IntoResponse {
    let ip = extract_ip(&headers, None);
    let username = body.username.trim();

    if body.new_password.len() < MIN_PASSWORD_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "password >=8 chars"})),
        )
            .into_response();
    }

    let guard = state.store.lock().await;

    let Some(user) = guard.get_user_by_username(username) else {
        let _ = guard.append_audit(
            None,
            Some(username),
            "recovery.complete",
            "auth",
            None,
            Some("unknown user"),
            ip.as_deref(),
            "failure",
        );
        return invalid_code_response();
    };

    let matched = guard
        .live_recovery_secrets(&user.id)
        .into_iter()
        .find(|(_, hash, salt)| AdminStore::recovery_code_matches(&body.code, hash, salt))
        .map(|(id, _, _)| id);

    let Some(token_id) = matched else {
        let _ = guard.append_audit(
            Some(&user.id),
            Some(&user.username),
            "recovery.complete",
            "auth",
            None,
            Some("invalid or expired code"),
            ip.as_deref(),
            "failure",
        );
        return invalid_code_response();
    };

    // Claim before touching the password: `used_at IS NULL` in the UPDATE is
    // what makes the code single-use, so a racing second request loses here
    // instead of both writers resetting the account.
    match guard.claim_recovery_token(token_id) {
        Ok(true) => {}
        Ok(false) => {
            let _ = guard.append_audit(
                Some(&user.id),
                Some(&user.username),
                "recovery.complete",
                "auth",
                None,
                Some("code already used"),
                ip.as_deref(),
                "failure",
            );
            return invalid_code_response();
        }
        Err(e) => return server_error(&e),
    }

    if let Err(e) = guard.update_user_password(&user.id, &body.new_password) {
        return server_error(&e);
    }
    let _ = guard.delete_user_sessions(&user.id);

    // The handoff file outlived its code; leaving a dead code on disk is how
    // an operator ends up pasting yesterday's code and blaming the gateway.
    if let Ok(data_dir) = safe_data_dir(&state.app_state.config) {
        let _ = std::fs::remove_file(recovery_code_path(&data_dir));
    }

    let _ = guard.append_audit(
        Some(&user.id),
        Some(&user.username),
        "recovery.complete",
        "auth",
        Some(&user.id),
        Some("password reset via one-time code; sessions revoked"),
        ip.as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({"ok": true, "username": user.username})),
    )
        .into_response()
}

/// The only body `/start` ever returns. Kept as a `Value` (not a `Response`)
/// so a test can assert the shape without driving an async body.
fn generic_start_body() -> serde_json::Value {
    serde_json::json!({
        "status": "pending",
        "message": GENERIC_START_MESSAGE,
    })
}

fn generic_start_response() -> Response {
    (StatusCode::ACCEPTED, Json(generic_start_body())).into_response()
}

fn invalid_code_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "invalid or expired recovery code"})),
    )
        .into_response()
}

fn server_error(detail: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": detail})),
    )
        .into_response()
}

/// Resolve the data dir that will hold the handoff file, refusing any path
/// that walks out of itself.
///
/// `resolved_data_dir()` is operator-controlled — `data_dir` in the config
/// file, or `GARRAIA_CONFIG_DIR` in the environment — so this is defense in
/// depth, not a fix for a live bug: nothing in the HTTP request reaches the
/// path, and the file name is a constant. It still earns its place, because a
/// data dir carrying `..` writes the `0600` file somewhere the operator did
/// not point at, and a non-UTF-8 dir would make the CLI's copy of this path
/// disagree with the gateway's.
///
/// The `..` check is deliberately over the whole string, not per component:
/// it costs a data dir literally named `foo..bar` (which nobody has) and buys
/// the exact shape CodeQL's `rust/path-injection` models as a sanitizer, so
/// the three `std::fs` calls downstream stop being reported as tainted sinks.
fn safe_data_dir(config: &garraia_config::AppConfig) -> Result<PathBuf, String> {
    vet_data_dir(config.resolved_data_dir())
}

/// The check itself, split out so a test can drive it without an `AppConfig`.
fn vet_data_dir(resolved: PathBuf) -> Result<PathBuf, String> {
    let dir = resolved
        .to_str()
        .ok_or_else(|| "data dir is not valid UTF-8".to_string())?;

    if dir.contains("..") {
        return Err("data dir must not contain a `..` component".to_string());
    }

    Ok(PathBuf::from(dir))
}

/// Absolute path of the handoff file for a given data dir.
pub fn recovery_code_path(data_dir: &Path) -> PathBuf {
    data_dir.join(RECOVERY_CODE_FILE)
}

/// Write `code` where only the host's operator can read it.
///
/// `0600` is set explicitly (not just on create) so a pre-existing file with
/// looser permissions is tightened rather than reused as-is.
fn write_code_file(data_dir: &Path, username: &str, code: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("failed to create {}: {e}", data_dir.display()))?;

    let path = recovery_code_path(data_dir);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(&path)
        .map_err(|e| format!("failed to open {}: {e}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("failed to chmod {}: {e}", path.display()))?;
    }

    writeln!(file, "username={username}")
        .and_then(|_| writeln!(file, "code={code}"))
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_body_is_generic_and_leaks_nothing() {
        let body = generic_start_body();
        let object = body.as_object().expect("object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["message", "status"]);

        let rendered = body.to_string();
        for leak in ["code", "username", "exists", "user_id", "not_found"] {
            assert!(!rendered.contains(leak), "body leaked {leak}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn handoff_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("garra-recovery-{}", uuid::Uuid::new_v4()));
        let path = recovery_code_path(&dir);

        // Pre-existing file with looser permissions: the write must tighten
        // it, not reuse it as-is.
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(&path, "stale").expect("seed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");

        let written = write_code_file(&dir, "admin", "code-abc").expect("write");
        let mode = std::fs::metadata(&written)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);

        let contents = std::fs::read_to_string(&written).expect("read");
        assert!(contents.contains("code=code-abc"));
        assert!(contents.contains("username=admin"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn data_dir_with_parent_component_is_refused() {
        assert!(vet_data_dir(PathBuf::from("/var/lib/garraia/../../etc")).is_err());
        assert!(vet_data_dir(PathBuf::from("..")).is_err());

        let ok = vet_data_dir(PathBuf::from("/var/lib/garraia/data")).expect("clean dir");
        assert_eq!(ok, PathBuf::from("/var/lib/garraia/data"));
    }

    #[test]
    fn wrong_code_never_matches() {
        use super::super::rbac::Role;
        let mut store = AdminStore::in_memory().expect("store");
        let user = store
            .create_user("admin", "pass", Role::Admin)
            .expect("user");
        let (_id, code) = store.create_recovery_token(&user.id, 600).expect("mint");
        let live = store.live_recovery_secrets(&user.id);
        assert_eq!(live.len(), 1);
        assert!(!AdminStore::recovery_code_matches(
            &format!("{code}x"),
            &live[0].1,
            &live[0].2
        ));
    }
}
