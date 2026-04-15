//! Phase 1.3 — Skins API: list, create, retrieve, and delete UI skins.
//!
//! Skins are JSON files stored in a `skins/` directory (configurable via
//! `GARRAIA_SKINS_DIR` env var, defaulting to `skins/` relative to CWD).
//! Each skin file contains CSS variable overrides that the desktop/web
//! frontend consumes to re-theme the UI.

use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

/// Resolve the skins directory path.
fn skins_dir() -> PathBuf {
    std::env::var("GARRAIA_SKINS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("skins"))
}

/// A skin definition — name plus arbitrary CSS variable overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skin {
    pub name: String,
    #[serde(flatten)]
    pub variables: serde_json::Value,
}

/// GET /api/skins — list all available skins from the skins directory.
pub async fn list_skins() -> impl IntoResponse {
    let dir = skins_dir();
    let skins = match read_skins(&dir).await {
        Ok(s) => s,
        Err(e) => {
            warn!("failed to read skins directory: {e}");
            Vec::new()
        }
    };
    Json(serde_json::json!({ "skins": skins }))
}

/// POST /api/skins — save a custom skin as a JSON file.
pub async fn create_skin(Json(body): Json<Skin>) -> impl IntoResponse {
    let dir = skins_dir();

    // Validate name (no path traversal).
    if body.name.contains('/') || body.name.contains('\\') || body.name.contains("..") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid skin name" })),
        );
    }

    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        warn!("failed to create skins directory: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to create skins dir: {e}") })),
        );
    }

    let file_path = dir.join(format!("{}.json", body.name));
    let json = match serde_json::to_string_pretty(&body) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid JSON: {e}") })),
            );
        }
    };

    match tokio::fs::write(&file_path, json.as_bytes()).await {
        Ok(_) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "skin": body })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to write skin: {e}") })),
        ),
    }
}

/// GET /api/skins/{name} — get a specific skin by name.
pub async fn get_skin(Path(name): Path<String>) -> impl IntoResponse {
    let dir = skins_dir();
    let file_path = dir.join(format!("{name}.json"));

    if !file_path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "skin not found" })),
        );
    }

    match tokio::fs::read_to_string(&file_path).await {
        Ok(contents) => match serde_json::from_str::<Skin>(&contents) {
            Ok(skin) => (StatusCode::OK, Json(serde_json::json!({ "skin": skin }))),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("invalid skin JSON: {e}") })),
            ),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to read skin: {e}") })),
        ),
    }
}

/// DELETE /api/skins/{name} — delete a custom skin.
pub async fn delete_skin(Path(name): Path<String>) -> impl IntoResponse {
    let dir = skins_dir();
    let file_path = dir.join(format!("{name}.json"));

    if !file_path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "skin not found" })),
        );
    }

    match tokio::fs::remove_file(&file_path).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to delete skin: {e}") })),
        ),
    }
}

/// Read all `.json` files in the skins directory.
async fn read_skins(dir: &std::path::Path) -> std::result::Result<Vec<Skin>, String> {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut skins = Vec::new();
        if !dir.is_dir() {
            return Ok(skins);
        }
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("read_dir: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Ok(contents) = std::fs::read_to_string(&path)
                && let Ok(skin) = serde_json::from_str::<Skin>(&contents)
            {
                skins.push(skin);
            }
        }
        Ok(skins)
    })
    .await
    .map_err(|e| format!("join: {e}"))?
}
