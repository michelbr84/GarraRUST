use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

use super::middleware::{AuthenticatedAdmin, extract_ip};
use super::rbac::{Action, Resource, check_permission};
use super::shared::AdminState;

// ── Request types ────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct SetSecretRequest {
    pub provider: String,
    pub key_name: String,
    pub value: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
}

fn default_tenant() -> String {
    "default".to_string()
}

#[derive(serde::Deserialize)]
pub struct RotateSecretRequest {
    pub provider: String,
    pub key_name: String,
    pub new_value: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
}

// ── CRUD endpoints ───────────────────────────────────────────────────

/// POST /admin/api/secrets — create or update a secret (never returns the value)
pub async fn set_secret(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<SetSecretRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let (encrypted, nonce) = match encrypt_value(body.value.as_bytes(), &state.encryption_key) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("encryption failed: {e}")})),
            );
        }
    };

    let guard = state.store.lock().await;
    match guard.set_secret(
        &body.tenant_id,
        &body.provider,
        &body.key_name,
        &encrypted,
        &nonce,
        Some(&admin.username),
    ) {
        Ok(id) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "set",
                "secret",
                Some(&format!("{}/{}", body.provider, body.key_name)),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"id": id, "is_set": true})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// GET /admin/api/secrets — list secrets (only metadata, NEVER values)
pub async fn list_secrets(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let tenant_id = params
        .get("tenant_id")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let guard = state.store.lock().await;
    let secrets = guard.list_secrets(tenant_id);

    let result: Vec<serde_json::Value> = secrets
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "tenant_id": s.tenant_id,
                "provider": s.provider,
                "key_name": s.key_name,
                "is_set": s.is_set,
                "version": s.version,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({"secrets": result})))
}

/// DELETE /admin/api/secrets/{provider}/{key_name}
pub async fn delete_secret(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path((provider, key_name)): axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Delete) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let tenant_id = params
        .get("tenant_id")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let guard = state.store.lock().await;
    match guard.delete_secret(tenant_id, &provider, &key_name) {
        Ok(()) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "delete",
                "secret",
                Some(&format!("{provider}/{key_name}")),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (StatusCode::OK, Json(serde_json::json!({"ok": true})))
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))),
    }
}

/// GET /admin/api/secrets/{provider}/{key_name}/test — test a stored secret
pub async fn test_secret(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path((provider, key_name)): axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let tenant_id = params
        .get("tenant_id")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let guard = state.store.lock().await;

    let raw = guard.get_secret_raw(tenant_id, &provider, &key_name);
    drop(guard);

    let Some((encrypted, nonce)) = raw else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "secret not found"})),
        );
    };

    let decrypted = match decrypt_value(&encrypted, &nonce, &state.encryption_key) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("decryption failed: {e}")})),
            );
        }
    };

    let is_valid = !decrypted.is_empty();
    let value_len = decrypted.len();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "provider": provider,
            "key_name": key_name,
            "is_valid": is_valid,
            "value_length": value_len,
        })),
    )
}

/// GET /admin/api/secrets/{id}/versions — list secret versions
pub async fn list_secret_versions(
    State(state): State<AdminState>,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    axum::extract::Path(secret_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Read) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let guard = state.store.lock().await;
    let versions = guard.list_secret_versions(&secret_id);
    (
        StatusCode::OK,
        Json(serde_json::json!({"versions": versions})),
    )
}

// ── Rotation + migration ─────────────────────────────────────────────

/// POST /admin/api/secrets/rotate — rotate a secret (archives current version)
pub async fn rotate_secret(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    Json(body): Json<RotateSecretRequest>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Update) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let (encrypted, nonce) = match encrypt_value(body.new_value.as_bytes(), &state.encryption_key) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
            );
        }
    };

    let guard = state.store.lock().await;
    match guard.set_secret(
        &body.tenant_id,
        &body.provider,
        &body.key_name,
        &encrypted,
        &nonce,
        Some(&admin.username),
    ) {
        Ok(id) => {
            let _ = guard.append_audit(
                Some(&admin.user_id),
                Some(&admin.username),
                "rotate",
                "secret",
                Some(&format!("{}/{}", body.provider, body.key_name)),
                None,
                extract_ip(&headers, None).as_deref(),
                "success",
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({"id": id, "rotated": true})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        ),
    }
}

/// POST /admin/api/secrets/migrate — migrate secrets from config.yml/env to the secrets store
pub async fn migrate_secrets(
    State(state): State<AdminState>,
    headers: HeaderMap,
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
) -> impl IntoResponse {
    if !check_permission(admin.role, Resource::Secrets, Action::Create) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "insufficient permissions"})),
        );
    }

    let config = state.app_state.current_config();
    let mut migrated = Vec::new();

    let env_keys = [
        ("anthropic", "ANTHROPIC_API_KEY"),
        ("openai", "OPENAI_API_KEY"),
        ("openrouter", "OPENROUTER_API_KEY"),
        ("deepseek", "DEEPSEEK_API_KEY"),
        ("mistral", "MISTRAL_API_KEY"),
        ("gemini", "GEMINI_API_KEY"),
        ("cohere", "COHERE_API_KEY"),
        ("falcon", "FALCON_API_KEY"),
        ("jais", "JAIS_API_KEY"),
        ("qwen", "QWEN_API_KEY"),
        ("yi", "YI_API_KEY"),
        ("minimax", "MINIMAX_API_KEY"),
        ("moonshot", "MOONSHOT_API_KEY"),
        ("sansa", "SANSA_API_KEY"),
    ];

    let guard = state.store.lock().await;

    for (provider, env_var) in &env_keys {
        let api_key = config
            .llm
            .get(*provider)
            .and_then(|c| c.api_key.clone())
            .or_else(|| std::env::var(env_var).ok());

        if let Some(key) = api_key {
            if key.is_empty() || key == "***REDACTED***" {
                continue;
            }
            match encrypt_value(key.as_bytes(), &state.encryption_key) {
                Ok((encrypted, nonce)) => {
                    if guard
                        .set_secret(
                            "default",
                            provider,
                            "api_key",
                            &encrypted,
                            &nonce,
                            Some(&admin.username),
                        )
                        .is_ok()
                    {
                        migrated.push(format!("{provider}/api_key"));
                    }
                }
                Err(_) => continue,
            }
        }
    }

    for (name, ch) in &config.channels {
        for (key, val) in &ch.settings {
            let lower = key.to_lowercase();
            if (lower.contains("token") || lower.contains("key") || lower.contains("secret"))
                && let Some(s) = val.as_str()
            {
                if s.is_empty() || s == "***REDACTED***" {
                    continue;
                }
                if let Ok((encrypted, nonce)) = encrypt_value(s.as_bytes(), &state.encryption_key)
                    && guard
                        .set_secret(
                            "default",
                            &format!("channel:{name}"),
                            key,
                            &encrypted,
                            &nonce,
                            Some(&admin.username),
                        )
                        .is_ok()
                {
                    migrated.push(format!("channel:{name}/{key}"));
                }
            }
        }
    }

    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "migrate",
        "secret",
        None,
        Some(&format!("migrated {} secrets", migrated.len())),
        extract_ip(&headers, None).as_deref(),
        "success",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "migrated": migrated,
            "count": migrated.len(),
        })),
    )
}

// ── Encryption helpers (private) ─────────────────────────────────────

fn encrypt_value(plaintext: &[u8], key: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let unbound = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| "failed to create encryption key".to_string())?;
    let aead_key = LessSafeKey::new(unbound);

    let rng = SystemRandom::new();
    let mut nonce_bytes = vec![0u8; 12];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| "failed to generate nonce".to_string())?;

    let nonce =
        Nonce::try_assume_unique_for_key(&nonce_bytes).map_err(|_| "invalid nonce".to_string())?;

    let mut in_out = plaintext.to_vec();
    aead_key
        .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "encryption failed".to_string())?;

    Ok((in_out, nonce_bytes))
}

fn decrypt_value(ciphertext: &[u8], nonce_bytes: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    let unbound = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| "failed to create decryption key".to_string())?;
    let aead_key = LessSafeKey::new(unbound);

    let nonce =
        Nonce::try_assume_unique_for_key(nonce_bytes).map_err(|_| "invalid nonce".to_string())?;

    let mut in_out = ciphertext.to_vec();
    let plaintext = aead_key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| "decryption failed".to_string())?;

    Ok(plaintext.to_vec())
}

// ── Config secret redaction (pub(super) — used by config handlers in handlers.rs) ──

pub(super) fn redact_config_secrets(config: &mut garraia_config::AppConfig) {
    config.gateway.api_key = config
        .gateway
        .api_key
        .as_ref()
        .map(|_| "***REDACTED***".to_string());

    for (_, llm) in config.llm.iter_mut() {
        llm.api_key = llm.api_key.as_ref().map(|_| "***REDACTED***".to_string());
    }

    for (_, emb) in config.embeddings.iter_mut() {
        emb.api_key = emb.api_key.as_ref().map(|_| "***REDACTED***".to_string());
    }

    for (_, ch) in config.channels.iter_mut() {
        for (key, val) in ch.settings.iter_mut() {
            let lower = key.to_lowercase();
            if lower.contains("token")
                || lower.contains("key")
                || lower.contains("secret")
                || lower.contains("password")
            {
                *val = serde_json::json!("***REDACTED***");
            }
        }
    }
}
