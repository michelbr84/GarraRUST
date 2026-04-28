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

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
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
    // 1) URL parse + scheme gate. Use reqwest's re-exported `Url` so we
    //    don't need to add a direct dep on the `url` crate.
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| InstallError::bad_request(format!("invalid URL: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(InstallError::bad_request(
            "manifest URL must use https scheme",
        ));
    }

    // 2) Allowlist gate (empty by default → remote URL install disabled).
    let host = parsed
        .host_str()
        .ok_or_else(|| InstallError::bad_request("URL is missing host"))?
        .to_lowercase();
    if !host_in_allowlist(&host, INSTALL_URL_ALLOWLIST) {
        return Err(InstallError::forbidden(
            "remote URL install disabled (host not in allowlist; \
             empty allowlist == disabled by default)",
        ));
    }

    // 3) DNS resolve + IP block gate. Resolves ONCE here; the resolved
    //    addrs are pinned into the reqwest client below via
    //    `resolve_to_addrs` so the connect does NOT re-resolve `host`
    //    (defense against DNS rebinding — GAR-461).
    let port = parsed.port_or_known_default().unwrap_or(443);
    let resolved = resolve_manifest_addrs(&host, port)?;
    validate_manifest_addrs(&resolved, &host)?;

    // 4) HTTP fetch with redirect=none + timeout + bounded body. Client
    //    is built with `resolve_to_addrs(&host, &resolved)` so reqwest's
    //    `.send()` skips DNS resolution entirely for `host` and connects
    //    to the IPs we already validated in step (3).
    let client = build_pinned_manifest_client(&host, &resolved)?;

    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| InstallError::upstream(format!("download failed: {e}")))?;

    if !response.status().is_success() {
        return Err(InstallError::upstream(format!(
            "upstream returned HTTP {}",
            response.status()
        )));
    }

    // Optional Content-Length sanity check (some upstreams omit this header).
    if let Some(len) = response.content_length()
        && (len as usize) > MANIFEST_BODY_CAP_BYTES
    {
        return Err(InstallError::bad_request(format!(
            "manifest exceeds {} byte cap (declared {} bytes)",
            MANIFEST_BODY_CAP_BYTES, len
        )));
    }

    // Bounded read — accumulate up to the cap, refuse if exceeded.
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

/// Resolve `host:port` to the full set of `SocketAddr`s via the system
/// resolver. Returns an `InstallError::bad_request` if resolution fails
/// or returns zero addresses. Pure helper — no IO besides DNS, easy to
/// reason about and replace.
fn resolve_manifest_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, InstallError> {
    let socket_str = format!("{host}:{port}");
    let resolved: Vec<SocketAddr> = socket_str
        .to_socket_addrs()
        .map_err(|e| InstallError::bad_request(format!("DNS resolve failed: {e}")))?
        .collect();
    if resolved.is_empty() {
        return Err(InstallError::bad_request(format!(
            "host '{host}' resolved to no addresses"
        )));
    }
    Ok(resolved)
}

/// Reject the entire `addrs` list if ANY address falls in a blocked
/// IP range (per [`is_blocked_ip`]). Returning `Ok(())` guarantees every
/// entry is publicly routable. The `host` parameter only flavors the
/// error message.
fn validate_manifest_addrs(addrs: &[SocketAddr], host: &str) -> Result<(), InstallError> {
    for addr in addrs {
        if is_blocked_ip(&addr.ip()) {
            return Err(InstallError::forbidden(format!(
                "host '{host}' resolves to blocked address {} \
                 (loopback/private/link-local/multicast/unspecified)",
                addr.ip()
            )));
        }
    }
    Ok(())
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
    reqwest::Client::builder()
        .resolve_to_addrs(host, addrs)
        .timeout(MANIFEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .user_agent(concat!(
            "GarraIA/",
            env!("CARGO_PKG_VERSION"),
            " plugin-installer"
        ))
        .build()
        .map_err(|e| InstallError::upstream(format!("client build failed: {e}")))
}

/// Read at most `cap` bytes from a response body, aborting with a 400 if
/// the body would exceed the cap. Streams the body to keep peak memory
/// bounded even on misbehaving upstreams that lie about Content-Length.
async fn read_capped(response: reqwest::Response, cap: usize) -> Result<Vec<u8>, InstallError> {
    use futures::StreamExt;

    let mut buf: Vec<u8> = Vec::with_capacity(cap.min(8192));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| InstallError::upstream(format!("body stream error: {e}")))?;
        if buf.len() + chunk.len() > cap {
            return Err(InstallError::bad_request(format!(
                "manifest exceeds {cap} byte cap (streamed)"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

// ── SSRF helpers (kept defensive duplicates; see crate docstring) ──────────

/// Returns `true` if `ip` is loopback, private, link-local, multicast,
/// unspecified, or otherwise unsafe to use as an HTTP target from the
/// gateway (SSRF defense). Covers IPv4 + IPv6.
///
/// IPv4 ranges blocked:
///   * 127.0.0.0/8       (loopback)
///   * 10.0.0.0/8        (RFC 1918)
///   * 172.16.0.0/12     (RFC 1918)
///   * 192.168.0.0/16    (RFC 1918)
///   * 169.254.0.0/16    (link-local incl. AWS/GCP IMDS at 169.254.169.254)
///   * 0.0.0.0/8         (unspecified / "this network")
///   * 224.0.0.0/4       (multicast)
///   * 100.64.0.0/10     (CGNAT)
///
/// IPv6 ranges blocked:
///   * ::1/128           (loopback)
///   * ::/128            (unspecified)
///   * fc00::/7          (unique local)
///   * fe80::/10         (link-local)
///   * ff00::/8          (multicast)
///   * ::ffff:0:0/96     (IPv4-mapped — inspect inner v4)
///   * ::a.b.c.d         (IPv4-compatible legacy RFC 4291 §2.5.5.1 — inspect inner v4)
fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0                  // 0.0.0.0/8
                || (o[0] == 100 && (64..=127).contains(&o[1])) // 100.64.0.0/10 CGNAT
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return true;
            }
            let segs = v6.segments();
            // fc00::/7 (unique local).
            if segs[0] & 0xfe00 == 0xfc00 {
                return true;
            }
            // fe80::/10 (link-local).
            if segs[0] & 0xffc0 == 0xfe80 {
                return true;
            }
            // IPv4-mapped IPv6 (::ffff:0:0/96): inspect the inner v4.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_ip(&IpAddr::V4(v4));
            }
            // IPv4-compatible IPv6 (RFC 4291 §2.5.5.1, deprecated):
            // ::a.b.c.d where the high 96 bits are zero and the low 32
            // bits encode an IPv4 address. Distinguished from v4-mapped
            // (::ffff:a.b.c.d, handled above by to_ipv4_mapped) and from
            // pure ::1/:: (caught by is_loopback/is_unspecified guards
            // ABOVE this branch — those guards run first, so the cases
            // segs[6] == 0 && segs[7] in (0, 1) never reach here). GAR-460.
            if segs[0..6] == [0, 0, 0, 0, 0, 0] {
                let v4 = Ipv4Addr::new(
                    (segs[6] >> 8) as u8,
                    (segs[6] & 0xff) as u8,
                    (segs[7] >> 8) as u8,
                    (segs[7] & 0xff) as u8,
                );
                return is_blocked_ip(&IpAddr::V4(v4));
            }
            false
        }
    }
}

/// Suffix-match a host against an allowlist of host suffixes. An empty
/// allowlist returns `false` (the empty-by-default disabled state).
/// Suffixes match either the full host or a `.suffix` boundary — so
/// `"plugins.example.com"` matches `"plugins.example.com"` and
/// `"v2.plugins.example.com"`, but NOT `"evilplugins.example.com"`.
fn host_in_allowlist(host: &str, allowlist: &[&str]) -> bool {
    if allowlist.is_empty() {
        return false;
    }
    let host = host.trim_end_matches('.').to_lowercase();
    allowlist.iter().any(|allowed| {
        let allowed = allowed.trim_end_matches('.').to_lowercase();
        host == allowed || host.ends_with(&format!(".{allowed}"))
    })
}

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

    // ── SSRF defense — IP block ────────────────────────────────────────────

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("test ip parse")
    }

    #[test]
    fn is_blocked_ip_loopback_v4() {
        assert!(is_blocked_ip(&ip("127.0.0.1")));
        assert!(is_blocked_ip(&ip("127.255.255.254")));
    }

    #[test]
    fn is_blocked_ip_rfc1918_v4() {
        assert!(is_blocked_ip(&ip("10.0.0.1")));
        assert!(is_blocked_ip(&ip("10.255.255.255")));
        assert!(is_blocked_ip(&ip("172.16.0.1")));
        assert!(is_blocked_ip(&ip("172.31.255.255")));
        assert!(is_blocked_ip(&ip("192.168.0.1")));
        assert!(is_blocked_ip(&ip("192.168.255.255")));
    }

    #[test]
    fn is_blocked_ip_link_local_and_imds_v4() {
        assert!(is_blocked_ip(&ip("169.254.0.1")));
        // AWS / GCP / Azure IMDS endpoint.
        assert!(is_blocked_ip(&ip("169.254.169.254")));
    }

    #[test]
    fn is_blocked_ip_unspecified_and_multicast_and_cgnat_v4() {
        assert!(is_blocked_ip(&ip("0.0.0.0")));
        assert!(is_blocked_ip(&ip("0.1.2.3")));
        assert!(is_blocked_ip(&ip("224.0.0.1")));
        assert!(is_blocked_ip(&ip("239.255.255.255")));
        // CGNAT 100.64.0.0/10.
        assert!(is_blocked_ip(&ip("100.64.0.1")));
        assert!(is_blocked_ip(&ip("100.127.255.254")));
    }

    #[test]
    fn is_blocked_ip_ipv6_loopback_and_unspecified() {
        assert!(is_blocked_ip(&ip("::1")));
        assert!(is_blocked_ip(&ip("::")));
    }

    #[test]
    fn is_blocked_ip_ipv6_unique_local_and_link_local() {
        assert!(is_blocked_ip(&ip("fc00::1")));
        assert!(is_blocked_ip(&ip("fd00::1")));
        assert!(is_blocked_ip(&ip("fe80::1")));
        assert!(is_blocked_ip(&ip("febf::1")));
    }

    #[test]
    fn is_blocked_ip_ipv6_multicast_and_v4_mapped() {
        assert!(is_blocked_ip(&ip("ff02::1")));
        // ::ffff:127.0.0.1 — IPv4-mapped IPv6 of loopback.
        assert!(is_blocked_ip(&ip("::ffff:127.0.0.1")));
        // ::ffff:169.254.169.254 — IPv4-mapped IPv6 of IMDS.
        assert!(is_blocked_ip(&ip("::ffff:169.254.169.254")));
    }

    #[test]
    fn is_blocked_ip_ipv6_v4_compatible_legacy() {
        // GAR-460 — IPv4-compatible IPv6 (RFC 4291 §2.5.5.1, deprecated):
        // ::a.b.c.d. NOT captured by Ipv6Addr::to_ipv4_mapped() (which
        // only handles ::ffff:a.b.c.d). The new branch in is_blocked_ip
        // walks segs[0..6] == [0;6] and decodes segs[6..8] as a v4 addr.
        assert!(is_blocked_ip(&ip("::127.0.0.1"))); // loopback wrapped
        assert!(is_blocked_ip(&ip("::169.254.169.254"))); // IMDS wrapped
        assert!(is_blocked_ip(&ip("::10.0.0.1"))); // RFC1918 wrapped
        assert!(is_blocked_ip(&ip("::192.168.1.1"))); // RFC1918 wrapped
        // Public v4 wrapped in v4-compatible should NOT be blocked.
        assert!(!is_blocked_ip(&ip("::8.8.8.8")));
        assert!(!is_blocked_ip(&ip("::1.1.1.1")));
        // Boundary: ::1 stays IPv6 loopback (caught by is_loopback BEFORE
        // the v4-compat branch fires — guards run in declared order).
        assert!(is_blocked_ip(&ip("::1")));
        // Boundary: :: stays IPv6 unspecified (caught by is_unspecified
        // BEFORE the v4-compat branch). Would also collapse to 0.0.0.0
        // which is blocked, but for the right reason.
        assert!(is_blocked_ip(&ip("::")));
    }

    #[test]
    fn is_blocked_ip_allows_public_addresses() {
        assert!(!is_blocked_ip(&ip("8.8.8.8")));
        assert!(!is_blocked_ip(&ip("1.1.1.1")));
        assert!(!is_blocked_ip(&ip("203.0.113.1"))); // documentation prefix, but not blocked
        assert!(!is_blocked_ip(&ip("2606:4700::1111"))); // Cloudflare public
        assert!(!is_blocked_ip(&ip("2001:4860:4860::8888"))); // Google public
    }

    // ── SSRF defense — host allowlist ──────────────────────────────────────

    #[test]
    fn empty_allowlist_blocks_everything() {
        assert!(!host_in_allowlist("example.com", &[]));
        assert!(!host_in_allowlist("plugins.example.com", &[]));
    }

    #[test]
    fn allowlist_exact_match_only_when_no_subdomain_left() {
        let list = &["plugins.example.com"];
        assert!(host_in_allowlist("plugins.example.com", list));
        assert!(host_in_allowlist("v2.plugins.example.com", list));
        // Boundary check: no trailing-substring spoofing.
        assert!(!host_in_allowlist("evilplugins.example.com", list));
        assert!(!host_in_allowlist("example.com", list));
    }

    #[test]
    fn allowlist_is_case_insensitive_and_trims_trailing_dot() {
        let list = &["Plugins.Example.Com"];
        assert!(host_in_allowlist("plugins.example.com", list));
        assert!(host_in_allowlist("PLUGINS.EXAMPLE.COM", list));
        assert!(host_in_allowlist("plugins.example.com.", list));
    }

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
    fn validate_manifest_addrs_accepts_empty() {
        // Trivially total. `resolve_manifest_addrs` would have returned
        // an error before reaching this; we still want a no-panic on [].
        assert!(validate_manifest_addrs(&[], "example.com").is_ok());
    }

    #[test]
    fn validate_manifest_addrs_rejects_loopback_v4() {
        let addrs = vec![SocketAddr::from(([127, 0, 0, 1], 443))];
        let err =
            validate_manifest_addrs(&addrs, "evil.example").expect_err("loopback must be rejected");
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert!(err.message.contains("blocked address"));
    }

    #[test]
    fn validate_manifest_addrs_rejects_imds() {
        let addrs = vec![SocketAddr::from(([169, 254, 169, 254], 443))];
        assert!(validate_manifest_addrs(&addrs, "evil.example").is_err());
    }

    #[test]
    fn validate_manifest_addrs_rejects_mixed_public_private() {
        // Critical case: an attacker's resolver returns a public IP
        // first (would pass alone) followed by RFC1918. The helper MUST
        // walk the entire list and trip on ANY blocked entry.
        let addrs = vec![
            SocketAddr::from(([8, 8, 8, 8], 443)), // public, would pass alone
            SocketAddr::from(([10, 0, 0, 1], 443)), // RFC1918 — must trip
        ];
        let err = validate_manifest_addrs(&addrs, "evil.example")
            .expect_err("mixed public+private must be rejected");
        assert_eq!(err.status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn validate_manifest_addrs_accepts_all_public_v4_v6() {
        let addrs = vec![
            SocketAddr::from(([8, 8, 8, 8], 443)),
            SocketAddr::from(([1, 1, 1, 1], 443)),
            SocketAddr::from((
                std::net::Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111),
                443,
            )),
        ];
        assert!(validate_manifest_addrs(&addrs, "ok.example").is_ok());
    }

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
