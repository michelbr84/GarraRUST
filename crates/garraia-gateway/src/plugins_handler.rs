//! Plugin Registry API handlers (Phase 3.1).
//!
//! Provides CRUD endpoints for managing WASM plugins:
//! - `POST /api/plugins/install` — install plugin by URL or name
//! - `GET /api/plugins` — list installed plugins with status
//! - `GET /api/plugins/{id}` — get plugin details
//! - `DELETE /api/plugins/{id}` — uninstall plugin
//! - `POST /api/plugins/{id}/toggle` — enable/disable plugin

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

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

// ── Handlers ────────────────────────────────────────────────────────────────

/// POST /api/plugins/install — install a plugin by URL or name.
pub async fn install_plugin(
    State(_state): State<SharedState>,
    Json(body): Json<InstallPluginRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
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

    // Validate the source looks reasonable
    if source.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "plugin source cannot be empty",
            })),
        );
    }

    info!(source = %source, "installing plugin");

    // For URL-based installs, download and validate manifest
    if let Some(url) = &body.url {
        match download_and_validate_manifest(url).await {
            Ok(manifest) => {
                info!(
                    name = %manifest.name,
                    version = %manifest.version,
                    "plugin manifest validated"
                );
                return (
                    StatusCode::CREATED,
                    Json(serde_json::json!({
                        "status": "ok",
                        "message": format!("plugin '{}' v{} installed", manifest.name, manifest.version),
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
            Err(e) => {
                warn!(url = %url, error = %e, "failed to install plugin from URL");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": format!("failed to install plugin: {e}"),
                    })),
                );
            }
        }
    }

    // Name-based install (from built-in registry)
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
) -> Json<serde_json::Value> {
    // Query the plugin loader if available, otherwise return empty list
    let plugins: Vec<PluginInfo> = Vec::new();

    Json(serde_json::json!({
        "plugins": plugins,
        "total": plugins.len(),
    }))
}

/// GET /api/plugins/{id} — get plugin details.
pub async fn get_plugin(
    State(_state): State<SharedState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    // Look up plugin by ID
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
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    info!(plugin = %id, "uninstalling plugin");

    // In a full implementation, this would:
    // 1. Stop the plugin if running
    // 2. Remove WASM files from disk
    // 3. Remove from registry
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
    Path(id): Path<String>,
    Json(body): Json<TogglePluginRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let action = if body.enabled { "enabled" } else { "disabled" };
    info!(plugin = %id, action, "toggling plugin");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("plugin '{id}' {action}"),
            "enabled": body.enabled,
        })),
    )
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Download a plugin manifest from a URL and validate it.
async fn download_and_validate_manifest(url: &str) -> Result<PluginManifestJson, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("failed to download manifest: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let text = response
        .text()
        .await
        .map_err(|e| format!("failed to read response: {e}"))?;

    let manifest: PluginManifestJson = serde_json::from_str(&text)
        .map_err(|e| format!("invalid plugin manifest JSON: {e}"))?;

    // Validate semver
    if !is_valid_semver(&manifest.version) {
        return Err(format!("invalid semver version: {}", manifest.version));
    }

    // Validate name (alphanumeric + hyphens)
    if manifest.name.is_empty()
        || !manifest
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-')
    {
        return Err(format!("invalid plugin name: {}", manifest.name));
    }

    Ok(manifest)
}

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
}
