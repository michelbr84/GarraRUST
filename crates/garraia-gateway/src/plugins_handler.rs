//! Plugin Registry API handlers (Phase 3.1).
//!
//! Provides CRUD endpoints for managing WASM plugins, all gated behind
//! the admin authentication middleware (`require_admin_auth`) +
//! `Permission::ManagePlugins` permission check:
//!
//! - `POST /api/plugins/install` — install plugin by URL (admin only)
//! - `GET  /api/plugins`         — list installed plugins
//! - `GET  /api/plugins/{id}`    — get plugin details
//! - `DELETE /api/plugins/{id}`  — uninstall plugin
//! - `POST /api/plugins/{id}/toggle` — enable/disable plugin
//!
//! GAR-459 (PR-A of GAR-454, plan `purrfect-lantern` 2026-04-27): hardened
//! the surface as a prerequisite of the wasmtime 28→44 bump (GAR-454/PR-B).
//! Threat model: anonymous SSRF via `download_and_validate_manifest`
//! reaching arbitrary URLs (cloud metadata services, RFC1918, link-local,
//! loopback). Mitigations:
//!   1. `require_admin_auth` cookie + `Permission::ManagePlugins` gate.
//!   2. `require_csrf` on POST/DELETE.
//!   3. Empty-by-default URL allowlist — remote install disabled until
//!      operator opts into specific domains via `INSTALL_URL_ALLOWLIST`.
//!   4. HTTPS-only, redirect=none, 10s timeout, 64KiB body cap, blocked
//!      IPs (loopback/private/link-local/multicast/unspecified) for
//!      IPv4 + IPv6 (including v4-mapped and v4-compatible legacy via
//!      GAR-460).
//!   5. **DNS pinning (GAR-461):** the IPs vetted in step (3) are
//!      handed to `reqwest::ClientBuilder::resolve_to_addrs(&host, …)`
//!      so the connect-time DNS lookup is skipped entirely for `host`
//!      and `.send()` connects to the IPs we already validated. This
//!      eliminates the TOCTOU window an attacker controlling the
//!      upstream resolver could otherwise exploit (DNS rebinding).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::admin::middleware::{
    AuthenticatedAdmin, require_admin_auth, require_csrf, security_headers,
};
use crate::admin::rbac::{Permission, has_permission};
use crate::admin::store::AdminStore;
use crate::state::SharedState;

// ── Plugin Manifest (JSON format for API) ───────────────────────────────────

/// JSON plugin manifest used by the marketplace/API layer.
/// This is distinct from the on-disk TOML manifest in garraia-plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifestJson {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tools_provided: Vec<String>,
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
}

/// Plugin status in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    Active,
    Inactive,
    Error,
}

/// Full plugin info returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub status: PluginStatus,
    pub tools_provided: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_version: Option<String>,
    pub update_available: bool,
}

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InstallPluginRequest {
    /// URL to download the plugin manifest/package from.
    #[serde(default)]
    pub url: Option<String>,
    /// Plugin name to install from a known registry.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TogglePluginRequest {
    pub enabled: bool,
}

// ── SSRF defense constants ──────────────────────────────────────────────────

/// Allowlist of host suffixes from which `install_plugin` may fetch a
/// manifest URL. **Empty by default** — remote URL installs are refused
/// until an operator extends this list. Future move to
/// `AppConfig.plugins.install_url_allowlist` is tracked under
/// `GAR-454.a.config`. Match is suffix-based (`endswith`) so that a
/// trailing-dot canonical comparison yields the expected result; entries
/// SHOULD be lowercase domain strings (e.g. `"plugins.example.com"`).
const INSTALL_URL_ALLOWLIST: &[&str] = &[];

/// Maximum plugin manifest body size accepted from a remote URL.
const MANIFEST_BODY_CAP_BYTES: usize = 64 * 1024;

/// Per-request timeout for the manifest download.
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(10);

// ── Router builder ──────────────────────────────────────────────────────────

/// Build the `/api/plugins/*` sub-router with admin auth + CSRF + admin-store
/// extension wired exactly like the `/admin` nested router. Mounted via
/// `.merge(...)` from the main `build_router` in `router.rs`.
///
/// Layer order (axum applies in reverse: last `.layer(...)` runs first on
/// the incoming request, mirroring `tower::ServiceBuilder` semantics):
///   1. `Extension<Arc<Mutex<AdminStore>>>` — sets the extension consumed by
///      `require_admin_auth` to validate the session cookie.
///   2. `require_admin_auth`               — validates cookie, injects
///      `AuthenticatedAdmin` into request extensions; rejects 401 otherwise.
///   3. `require_csrf`                     — on POST/DELETE/PUT/PATCH only,
///      validates `x-csrf-token` matches `AuthenticatedAdmin.csrf_token`;
///      403 otherwise. (GET/HEAD/OPTIONS pass through.)
pub fn build_plugin_routes(state: SharedState, admin_store: Arc<Mutex<AdminStore>>) -> Router {
    Router::new()
        .route("/api/plugins/install", post(install_plugin))
        .route("/api/plugins", get(list_plugins))
        .route(
            "/api/plugins/{id}",
            get(get_plugin).delete(uninstall_plugin),
        )
        .route("/api/plugins/{id}/toggle", post(toggle_plugin))
        .layer(axum::middleware::from_fn(require_csrf))
        .layer(axum::middleware::from_fn(require_admin_auth))
        .layer(Extension(admin_store))
        // Security-auditor LOW finding (GAR-459): mirror the `security_headers`
        // middleware applied to the /admin nested router so /api/plugins/*
        // responses also carry CSP, X-Content-Type-Options, X-Frame-Options,
        // referrer-policy, permissions-policy, cache-control: no-store.
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// Permission gate shared by every plugin handler. Returns `Err(403)` if
/// the caller's role lacks `Permission::ManagePlugins`. `Role::Admin` and
/// `Role::Operator` carry it; `Role::Viewer` does not. Defined in
/// `crate::admin::rbac`.
fn check_manage_plugins(
    admin: &AuthenticatedAdmin,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if has_permission(admin.role, Permission::ManagePlugins) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "status": "error",
                "message": "missing permission: manage_plugins",
            })),
        ))
    }
}

/// POST /api/plugins/install — install a plugin by URL (admin/operator only).
pub async fn install_plugin(
    State(_state): State<SharedState>,
    Extension(admin): Extension<AuthenticatedAdmin>,
    Json(body): Json<InstallPluginRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err((code, json)) = check_manage_plugins(&admin) {
        return (code, json);
    }

    let source = match (&body.url, &body.name) {
        (Some(url), _) => url.clone(),
        (_, Some(name)) => name.clone(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "status": "error",
                    "message": "either 'url' or 'name' must be provided",
                })),
            );
        }
    };

    if source.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "plugin source cannot be empty",
            })),
        );
    }

    info!(actor = %admin.username, source = %source, "installing plugin");

    if let Some(url) = &body.url {
        match download_and_validate_manifest(url).await {
            Ok(manifest) => {
                info!(
                    actor = %admin.username,
                    name = %manifest.name,
                    version = %manifest.version,
                    "plugin manifest validated"
                );
                return (
                    StatusCode::CREATED,
                    Json(serde_json::json!({
                        "status": "ok",
                        "message": format!(
                            "plugin '{}' v{} installed",
                            manifest.name, manifest.version
                        ),
                        "plugin": {
                            "id": manifest.name,
                            "name": manifest.name,
                            "version": manifest.version,
                            "description": manifest.description,
                            "status": "active",
                        },
                    })),
                );
            }
            Err(InstallError { status, message }) => {
                warn!(
                    actor = %admin.username,
                    url = %url,
                    status = status.as_u16(),
                    error = %message,
                    "remote plugin install rejected"
                );
                return (
                    status,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": message,
                    })),
                );
            }
        }
    }

    // Name-based install (from built-in registry) — no remote network call.
    let name = body.name.as_deref().unwrap_or_default();
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("plugin '{name}' installed"),
            "plugin": {
                "id": name,
                "name": name,
                "version": "0.1.0",
                "status": "active",
            },
        })),
    )
}

/// GET /api/plugins — list installed plugins with status.
pub async fn list_plugins(
    State(_state): State<SharedState>,
    Extension(admin): Extension<AuthenticatedAdmin>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err((code, json)) = check_manage_plugins(&admin) {
        return (code, json);
    }
    let plugins: Vec<PluginInfo> = Vec::new();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "plugins": plugins,
            "total": plugins.len(),
        })),
    )
}

/// GET /api/plugins/{id} — get plugin details.
pub async fn get_plugin(
    State(_state): State<SharedState>,
    Extension(admin): Extension<AuthenticatedAdmin>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err((code, json)) = check_manage_plugins(&admin) {
        return (code, json);
    }
    let _ = &id;
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "status": "error",
            "message": format!("plugin '{id}' not found"),
        })),
    )
}

/// DELETE /api/plugins/{id} — uninstall a plugin.
pub async fn uninstall_plugin(
    State(_state): State<SharedState>,
    Extension(admin): Extension<AuthenticatedAdmin>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err((code, json)) = check_manage_plugins(&admin) {
        return (code, json);
    }
    info!(actor = %admin.username, plugin = %id, "uninstalling plugin");
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("plugin '{id}' uninstalled"),
        })),
    )
}

/// POST /api/plugins/{id}/toggle — enable or disable a plugin.
pub async fn toggle_plugin(
    State(_state): State<SharedState>,
    Extension(admin): Extension<AuthenticatedAdmin>,
    Path(id): Path<String>,
    Json(body): Json<TogglePluginRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err((code, json)) = check_manage_plugins(&admin) {
        return (code, json);
    }
    let action = if body.enabled { "enabled" } else { "disabled" };
    info!(
        actor = %admin.username,
        plugin = %id,
        action,
        "toggling plugin"
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("plugin '{id}' {action}"),
            "enabled": body.enabled,
        })),
    )
}

// ── SSRF-hardened manifest download ─────────────────────────────────────────

/// Internal install error carrying a precise HTTP status. Layered to keep
/// the handler call site short and to make the test cases explicit.
struct InstallError {
    status: StatusCode,
    message: String,
}

impl InstallError {
    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    fn upstream(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: message.into(),
        }
    }
}

/// Download a plugin manifest from `url` and validate it. Hardened against
/// SSRF, redirect amplification, slow-loris, and oversize bodies — see the
/// crate-level docstring above for the threat model.
async fn download_and_validate_manifest(url: &str) -> Result<PluginManifestJson, InstallError> {
    // 1-3) Scheme gate, host allow-list, single DNS resolve and IP block, all
    //      in the shared guard (`garraia_common::ssrf`). Extracted from this
    //      module on 2026-08-29 so the skill importer, the `web_fetch` tool and
    //      the MCP transport get the same defenses instead of near-copies.
    let vetted = garraia_common::ssrf::vet_url(url, &manifest_url_policy())
        .map_err(ssrf_to_install_error)?;

    // 4) HTTP fetch with redirect=none + timeout + bounded body. The client is
    //    pinned to the addresses vetted above, so `.send()` skips DNS entirely
    //    and cannot be rebound between the check and the connect (GAR-461).
    let client = build_pinned_manifest_client(&vetted.host, &vetted.addrs)?;

    let response = client
        .get(vetted.url.clone())
        .send()
        .await
        .map_err(|e| InstallError::upstream(format!("download failed: {e}")))?;

    if !response.status().is_success() {
        return Err(InstallError::upstream(format!(
            "upstream returned HTTP {}",
            response.status()
        )));
    }

    // Bounded read. The shared helper also honours a declared Content-Length
    // over the cap, so the previous separate pre-check is no longer needed.
    let bytes = read_capped(response, MANIFEST_BODY_CAP_BYTES).await?;

    let text = std::str::from_utf8(&bytes)
        .map_err(|e| InstallError::bad_request(format!("non-utf8 manifest body: {e}")))?;

    let manifest: PluginManifestJson = serde_json::from_str(text)
        .map_err(|e| InstallError::bad_request(format!("invalid plugin manifest JSON: {e}")))?;

    if !is_valid_semver(&manifest.version) {
        return Err(InstallError::bad_request(format!(
            "invalid semver version: {}",
            manifest.version
        )));
    }
    if manifest.name.is_empty()
        || !manifest
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-')
    {
        return Err(InstallError::bad_request(format!(
            "invalid plugin name: {}",
            manifest.name
        )));
    }

    Ok(manifest)
}

/// Policy for plugin-manifest fetches: https only, host allow-list enforced
/// (empty by default == remote URL install disabled), 10s timeout.
///
/// The allow-list is `Some(INSTALL_URL_ALLOWLIST)` rather than `None` on
/// purpose — that is what keeps remote install off unless an operator opts in.
fn manifest_url_policy() -> garraia_common::ssrf::UrlPolicy {
    garraia_common::ssrf::UrlPolicy::https_public(
        MANIFEST_TIMEOUT,
        concat!("GarraIA/", env!("CARGO_PKG_VERSION"), " plugin-installer"),
    )
    .with_host_allowlist(INSTALL_URL_ALLOWLIST)
}

/// Map the shared guard's rejection onto this module's error type, preserving
/// the status split the pre-existing tests assert on: malformed input is a
/// 400, a policy refusal is a 403, our own client failure is an upstream error.
fn ssrf_to_install_error(rejection: garraia_common::ssrf::SsrfRejection) -> InstallError {
    use garraia_common::ssrf::SsrfCategory;
    // Match on the category, not on the status code: a new `SsrfRejection`
    // variant then forces this mapping to be revisited instead of silently
    // landing in a catch-all arm.
    match rejection.category() {
        SsrfCategory::Forbidden => InstallError::forbidden(rejection.to_string()),
        SsrfCategory::Upstream => InstallError::upstream(rejection.to_string()),
        SsrfCategory::BadRequest => InstallError::bad_request(rejection.to_string()),
    }
}

/// Build a `reqwest::Client` that connects ONLY to the pre-validated
/// `addrs` for `host`, bypassing reqwest's internal DNS resolver. This
/// closes the TOCTOU window for DNS rebinding: without
/// `resolve_to_addrs`, reqwest would resolve `host` again at `.send()`
/// time, allowing an attacker controlling the upstream resolver to swap
/// the IP between our IP-block gate and the actual connect. Cf.
/// `reqwest::ClientBuilder::resolve_to_addrs` (GAR-461).
fn build_pinned_manifest_client(
    host: &str,
    addrs: &[SocketAddr],
) -> Result<reqwest::Client, InstallError> {
    garraia_common::ssrf::pinned_client_for(host, addrs, &manifest_url_policy())
        .map_err(ssrf_to_install_error)
}

/// Read at most `cap` bytes from a response body, aborting with a 400 if
/// the body would exceed the cap. Streams the body to keep peak memory
/// bounded even on misbehaving upstreams that lie about Content-Length.
async fn read_capped(response: reqwest::Response, cap: usize) -> Result<Vec<u8>, InstallError> {
    garraia_common::ssrf::read_capped(response, cap)
        .await
        .map_err(ssrf_to_install_error)
}

// ── SSRF adapters (thin delegates to `garraia_common::ssrf`) ──────────────

// ── Manifest helpers (unchanged from pre-PR-A) ──────────────────────────────

/// Basic semver validation (major.minor.patch).
fn is_valid_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u64>().is_ok())
}

/// Compare two semver strings. Returns true if `available` > `installed`.
pub fn semver_newer(installed: &str, available: &str) -> bool {
    let parse = |v: &str| -> Option<(u64, u64, u64)> {
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    };

    match (parse(installed), parse(available)) {
        (Some(i), Some(a)) => a > i,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison() {
        assert!(semver_newer("0.1.0", "0.2.0"));
        assert!(semver_newer("1.0.0", "1.0.1"));
        assert!(semver_newer("0.9.9", "1.0.0"));
        assert!(!semver_newer("1.0.0", "1.0.0"));
        assert!(!semver_newer("2.0.0", "1.0.0"));
    }

    #[test]
    fn valid_semver() {
        assert!(is_valid_semver("0.1.0"));
        assert!(is_valid_semver("1.0.0"));
        assert!(is_valid_semver("12.34.56"));
        assert!(!is_valid_semver("1.0"));
        assert!(!is_valid_semver("abc"));
        assert!(!is_valid_semver("1.0.0-beta"));
    }
    // ── SSRF defense — host allowlist ──────────────────────────────────────

    // ── End-to-end: scheme + allowlist gates ───────────────────────────────

    #[tokio::test]
    async fn download_rejects_http_scheme() {
        let err = download_and_validate_manifest("http://plugins.example.com/m.json")
            .await
            .expect_err("http should be rejected");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("https"));
    }

    #[tokio::test]
    async fn download_rejects_when_allowlist_empty() {
        // INSTALL_URL_ALLOWLIST is empty by default; any host must be 403.
        let err = download_and_validate_manifest("https://plugins.example.com/m.json")
            .await
            .expect_err("empty allowlist should reject");
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert!(err.message.contains("allowlist"));
    }

    #[tokio::test]
    async fn download_rejects_invalid_url() {
        let err = download_and_validate_manifest("not a url at all")
            .await
            .expect_err("invalid URL should be 400");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    // ── DNS-pinning helpers (GAR-461) ──────────────────────────────────────
    //
    // These exercise the pure helpers `validate_manifest_addrs` +
    // `build_pinned_manifest_client` without touching the network. The
    // structural guarantee that `.send()` skips the connect-time DNS
    // lookup comes from `reqwest::ClientBuilder::resolve_to_addrs`,
    // which we plug in `build_pinned_manifest_client`. We do not test
    // `resolve_manifest_addrs` directly because it depends on the system
    // resolver (covered indirectly by the existing e2e harness).

    #[test]
    fn build_pinned_manifest_client_smoke() {
        // Smoke test: builder accepts a host + pinned addrs and returns
        // a usable Client. The behavioral guarantee that `.send()` on
        // this client skips DNS resolution comes from reqwest's docs;
        // we verify the wiring compiles and constructs successfully.
        let addrs = vec![SocketAddr::from(([8, 8, 8, 8], 443))];
        let client = build_pinned_manifest_client("example.invalid", &addrs);
        assert!(client.is_ok(), "expected pinned client to build cleanly");
    }
}
