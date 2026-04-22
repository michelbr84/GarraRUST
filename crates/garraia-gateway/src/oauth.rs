//! OAuth2 / OIDC login support for GarraIA mobile and web clients.
//!
//! Supported providers: Google, GitHub, Azure AD (generic OIDC).
//!
//! Routes (registered in router.rs):
//!   GET  /auth/oauth/{provider}           — redirect to provider authorization URL
//!   GET  /auth/oauth/{provider}/callback  — exchange code for token, create/link user

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::mobile_auth::issue_jwt_pub;
use crate::state::AppState;

// ── Provider config ───────────────────────────────────────────────────────────

/// Supported OAuth2 / OIDC provider identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OAuthProvider {
    Google,
    GitHub,
    /// Generic OIDC — used for Azure AD and custom providers.
    Oidc,
}

impl OAuthProvider {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "google" => Some(OAuthProvider::Google),
            "github" => Some(OAuthProvider::GitHub),
            "oidc" | "azure" | "azuread" => Some(OAuthProvider::Oidc),
            _ => None,
        }
    }

    fn authorization_endpoint(&self, config: &OAuthConfig) -> String {
        match self {
            OAuthProvider::Google => "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            OAuthProvider::GitHub => "https://github.com/login/oauth/authorize".to_string(),
            OAuthProvider::Oidc => config.authorization_endpoint.clone().unwrap_or_default(),
        }
    }

    fn token_endpoint(&self, config: &OAuthConfig) -> String {
        match self {
            OAuthProvider::Google => "https://oauth2.googleapis.com/token".to_string(),
            OAuthProvider::GitHub => "https://github.com/login/oauth/access_token".to_string(),
            OAuthProvider::Oidc => config.token_endpoint.clone().unwrap_or_default(),
        }
    }

    fn userinfo_endpoint(&self, config: &OAuthConfig) -> Option<String> {
        match self {
            OAuthProvider::Google => {
                Some("https://www.googleapis.com/oauth2/v3/userinfo".to_string())
            }
            OAuthProvider::GitHub => Some("https://api.github.com/user".to_string()),
            OAuthProvider::Oidc => config.userinfo_endpoint.clone(),
        }
    }

    fn default_scopes(&self) -> Vec<String> {
        match self {
            OAuthProvider::Google => {
                vec!["openid".into(), "email".into(), "profile".into()]
            }
            OAuthProvider::GitHub => {
                vec!["user:email".into()]
            }
            OAuthProvider::Oidc => {
                vec!["openid".into(), "email".into()]
            }
        }
    }
}

/// Per-provider OAuth2 configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub provider: OAuthProvider,
    pub client_id: String,
    /// Never logged or included in API responses.
    #[serde(skip_serializing)]
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    /// For generic OIDC: authorization endpoint URL.
    pub authorization_endpoint: Option<String>,
    /// For generic OIDC: token endpoint URL.
    pub token_endpoint: Option<String>,
    /// For generic OIDC: userinfo endpoint URL.
    pub userinfo_endpoint: Option<String>,
}

impl OAuthConfig {
    pub fn google(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            provider: OAuthProvider::Google,
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
            scopes: OAuthProvider::Google.default_scopes(),
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
        }
    }

    pub fn github(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            provider: OAuthProvider::GitHub,
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
            scopes: OAuthProvider::GitHub.default_scopes(),
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
        }
    }

    pub fn oidc(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
        authorization_endpoint: impl Into<String>,
        token_endpoint: impl Into<String>,
        userinfo_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            provider: OAuthProvider::Oidc,
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
            scopes: OAuthProvider::Oidc.default_scopes(),
            authorization_endpoint: Some(authorization_endpoint.into()),
            token_endpoint: Some(token_endpoint.into()),
            userinfo_endpoint: Some(userinfo_endpoint.into()),
        }
    }
}

// ── OAuth state store (in-memory, short-lived) ────────────────────────────────

/// Minimal in-memory CSRF state store for OAuth flows.
/// Each state value maps to the originating provider name and creation time.
/// In a production cluster this should be backed by Redis / DB.
///
/// Entries older than `TTL` (10 minutes) are automatically evicted during
/// validation to prevent unbounded memory growth from abandoned flows.
#[derive(Debug, Default)]
pub struct OAuthStateStore {
    pending: std::sync::Mutex<HashMap<String, OAuthPendingEntry>>,
}

/// A pending OAuth state entry with creation timestamp for TTL enforcement.
#[derive(Debug, Clone)]
struct OAuthPendingEntry {
    provider: String,
    created_at: std::time::Instant,
}

/// Maximum age for an OAuth state token (10 minutes).
const OAUTH_STATE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

impl OAuthStateStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate and store a new CSRF state token. Returns the token.
    pub fn create(&self, provider: &str) -> String {
        let token = Uuid::new_v4().to_string().replace('-', "");
        let entry = OAuthPendingEntry {
            provider: provider.to_string(),
            created_at: std::time::Instant::now(),
        };
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(token.clone(), entry);
        token
    }

    /// Validate and consume a state token. Returns the provider name if valid.
    ///
    /// Also evicts any entries older than `OAUTH_STATE_TTL` to prevent
    /// unbounded growth from abandoned OAuth flows.
    pub fn validate_and_consume(&self, token: &str) -> Option<String> {
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());

        // Evict expired entries
        let now = std::time::Instant::now();
        pending.retain(|_, entry| now.duration_since(entry.created_at) < OAUTH_STATE_TTL);

        // Consume the requested token (if it survived eviction)
        pending.remove(token).map(|entry| entry.provider)
    }
}

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthTokenResponse {
    pub token: String,
    pub user_id: String,
    pub email: String,
    pub provider: String,
    pub is_new_user: bool,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenExchangeResponse {
    access_token: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    /// Google / OIDC: sub claim or id
    sub: Option<String>,
    /// GitHub: numeric id as string
    id: Option<serde_json::Value>,
    email: Option<String>,
    login: Option<String>, // GitHub username
    #[allow(dead_code)]
    name: Option<String>,
}

impl UserInfo {
    fn provider_id(&self) -> Option<String> {
        self.sub.clone().or_else(|| {
            self.id.as_ref().map(|v| match v {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        })
    }

    fn email_or_login(&self) -> Option<String> {
        self.email
            .clone()
            .or_else(|| self.login.as_ref().map(|l| format!("{l}@github.noreply")))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_oauth_configs() -> HashMap<String, OAuthConfig> {
    let mut configs = HashMap::new();

    // Google
    if let (Ok(id), Ok(secret)) = (
        std::env::var("GARRAIA_GOOGLE_CLIENT_ID"),
        std::env::var("GARRAIA_GOOGLE_CLIENT_SECRET"),
    ) {
        let redirect = std::env::var("GARRAIA_GOOGLE_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:3888/auth/oauth/google/callback".into());
        configs.insert("google".into(), OAuthConfig::google(id, secret, redirect));
    }

    // GitHub
    if let (Ok(id), Ok(secret)) = (
        std::env::var("GARRAIA_GITHUB_CLIENT_ID"),
        std::env::var("GARRAIA_GITHUB_CLIENT_SECRET"),
    ) {
        let redirect = std::env::var("GARRAIA_GITHUB_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:3888/auth/oauth/github/callback".into());
        configs.insert("github".into(), OAuthConfig::github(id, secret, redirect));
    }

    // Generic OIDC (e.g. Azure AD)
    if let (Ok(id), Ok(secret), Ok(auth_ep), Ok(token_ep)) = (
        std::env::var("GARRAIA_OIDC_CLIENT_ID"),
        std::env::var("GARRAIA_OIDC_CLIENT_SECRET"),
        std::env::var("GARRAIA_OIDC_AUTH_ENDPOINT"),
        std::env::var("GARRAIA_OIDC_TOKEN_ENDPOINT"),
    ) {
        let redirect = std::env::var("GARRAIA_OIDC_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:3888/auth/oauth/oidc/callback".into());
        let userinfo =
            std::env::var("GARRAIA_OIDC_USERINFO_ENDPOINT").unwrap_or_else(|_| "".into());
        configs.insert(
            "oidc".into(),
            OAuthConfig::oidc(id, secret, redirect, auth_ep, token_ep, userinfo),
        );
    }

    configs
}

fn build_authorization_url(config: &OAuthConfig, state: &str) -> String {
    let base = config.provider.authorization_endpoint(config);
    let scopes = config.scopes.join(" ");
    format!(
        "{base}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
        urlencoding_simple(&config.client_id),
        urlencoding_simple(&config.redirect_uri),
        urlencoding_simple(&scopes),
        state,
    )
}

/// Minimal percent-encoding for OAuth URL parameters (encodes space as %20).
fn urlencoding_simple(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                vec![c]
            }
            ' ' => vec!['%', '2', '0'],
            c => {
                let encoded = format!("%{:02X}", c as u32);
                encoded.chars().collect()
            }
        })
        .collect()
}

async fn exchange_code_for_token(
    config: &OAuthConfig,
    code: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let token_url = config.provider.token_endpoint(config);
    if token_url.is_empty() {
        return Err("no token endpoint configured".into());
    }

    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", code);
    params.insert("redirect_uri", &config.redirect_uri);
    params.insert("client_id", &config.client_id);
    params.insert("client_secret", &config.client_secret);

    let resp = client
        .post(&token_url)
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("token request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("token endpoint returned {}", resp.status()));
    }

    let body: TokenExchangeResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse token response: {e}"))?;

    body.access_token
        .ok_or_else(|| "no access_token in response".into())
}

async fn fetch_userinfo(
    config: &OAuthConfig,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<UserInfo, String> {
    let userinfo_url = config
        .provider
        .userinfo_endpoint(config)
        .ok_or_else(|| "no userinfo endpoint".to_string())?;

    let resp = client
        .get(&userinfo_url)
        .bearer_auth(access_token)
        .header("User-Agent", "GarraIA/1.0")
        .send()
        .await
        .map_err(|e| format!("userinfo request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("userinfo endpoint returned {}", resp.status()));
    }

    resp.json::<UserInfo>()
        .await
        .map_err(|e| format!("failed to parse userinfo: {e}"))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Global in-memory OAuth state store (CSRF protection).
static OAUTH_STATE_STORE: std::sync::OnceLock<OAuthStateStore> = std::sync::OnceLock::new();

fn oauth_state_store() -> &'static OAuthStateStore {
    OAUTH_STATE_STORE.get_or_init(OAuthStateStore::new)
}

/// GET /auth/oauth/{provider}
///
/// Redirects the user to the provider's authorization endpoint.
/// Returns 404 if the provider is not configured.
pub async fn oauth_redirect(
    Path(provider): Path<String>,
    State(_state): State<Arc<AppState>>,
) -> Response {
    let configs = load_oauth_configs();
    let config = match configs.get(&provider) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("oauth provider '{provider}' not configured")})),
            )
                .into_response();
        }
    };

    let state_token = oauth_state_store().create(&provider);
    let url = build_authorization_url(config, &state_token);

    info!("oauth redirect for provider={provider}");
    Redirect::temporary(&url).into_response()
}

/// GET /auth/oauth/{provider}/callback
///
/// Exchanges the authorization code for an access token, fetches user info,
/// and creates or links a GarraIA mobile account. Returns a JWT bearer token.
pub async fn oauth_callback(
    Path(provider): Path<String>,
    Query(params): Query<OAuthCallbackParams>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Handle provider-side errors
    if let Some(err) = &params.error {
        let desc = params
            .error_description
            .as_deref()
            .unwrap_or("no description");
        warn!("oauth callback error from {provider}: {err} — {desc}");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("oauth error: {err}")})),
        );
    }

    // Validate CSRF state
    let state_token = params.state.as_deref().unwrap_or("");
    if oauth_state_store()
        .validate_and_consume(state_token)
        .as_deref()
        != Some(&provider)
    {
        warn!("oauth callback: invalid or expired state token for provider={provider}");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid oauth state"})),
        );
    }

    let code = match &params.code {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing authorization code"})),
            );
        }
    };

    let configs = load_oauth_configs();
    let config = match configs.get(&provider) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({"error": format!("oauth provider '{provider}' not configured")}),
                ),
            );
        }
    };

    let http_client = reqwest::Client::new();

    // Exchange code for access token
    let access_token = match exchange_code_for_token(config, &code, &http_client).await {
        Ok(t) => t,
        Err(e) => {
            warn!("oauth token exchange failed for provider={provider}: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "token exchange failed"})),
            );
        }
    };

    // Fetch user info
    let user_info = match fetch_userinfo(config, &access_token, &http_client).await {
        Ok(u) => u,
        Err(e) => {
            warn!("oauth userinfo fetch failed for provider={provider}: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "failed to fetch user info"})),
            );
        }
    };

    let provider_user_id = match user_info.provider_id() {
        Some(id) => id,
        None => {
            warn!("oauth userinfo missing user id for provider={provider}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "user id missing from provider response"})),
            );
        }
    };

    let email = match user_info.email_or_login() {
        Some(e) => e,
        None => {
            warn!("oauth userinfo missing email for provider={provider}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "email missing from provider response"})),
            );
        }
    };

    // Create or find existing user
    let (user_id, is_new_user) = match &state.session_store {
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        }
        Some(store_arc) => {
            let store = store_arc.lock().await;
            let oauth_email = format!("{provider}:{provider_user_id}@oauth.garraia");

            // Try to find by oauth synthetic email first
            match store.find_mobile_user_by_email(&oauth_email) {
                Ok(Some(u)) => (u.id, false),
                Ok(None) => {
                    // Create a new account — password is a random UUID (not usable for login)
                    let new_id = Uuid::new_v4().to_string();
                    let placeholder_hash = Uuid::new_v4().to_string();
                    let placeholder_salt = Uuid::new_v4().to_string();
                    match store.create_mobile_user(
                        &new_id,
                        &oauth_email,
                        &placeholder_hash,
                        &placeholder_salt,
                    ) {
                        Ok(()) => {
                            info!(
                                "oauth: created new user for provider={provider} email={}",
                                // Use sanitized form — never log the actual provider email
                                &email[..email.find('@').unwrap_or(email.len()).min(16)]
                            );
                            (new_id, true)
                        }
                        Err(e) => {
                            warn!("oauth: failed to create user: {e}");
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({"error": "internal error"})),
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!("oauth: db lookup failed: {e}");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "internal error"})),
                    );
                }
            }
        }
    };

    // Issue JWT. Plan 0046 slice 3: pass `&AppState` so the handler
    // reads the secret via `state.jwt_signing_secret()` rather than
    // `std::env::var`. Fail-closed to 503 when unconfigured.
    let token = match issue_jwt_pub(&state, &user_id, &email) {
        Ok(t) => t,
        Err(crate::mobile_auth::JwtIssueError::AuthUnconfigured) => {
            warn!("oauth: AuthConfig unavailable; returning 503");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "auth not configured"})),
            );
        }
        Err(crate::mobile_auth::JwtIssueError::Jwt(e)) => {
            warn!("oauth: JWT issue failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    let status = if is_new_user {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    (
        status,
        Json(serde_json::json!(OAuthTokenResponse {
            token,
            user_id,
            email,
            provider,
            is_new_user,
        })),
    )
}

// ── GET /auth/oauth/providers — list configured OAuth providers ───────────────

pub async fn list_oauth_providers(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let configs = load_oauth_configs();
    let providers: Vec<serde_json::Value> = configs
        .keys()
        .map(|name| {
            serde_json::json!({
                "provider": name,
                "enabled": true,
                "login_url": format!("/auth/oauth/{name}"),
            })
        })
        .collect();

    Json(serde_json::json!({"providers": providers}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_google_auth_url_contains_required_params() {
        let config = OAuthConfig::google("client123", "secret", "https://example.com/cb");
        let url = build_authorization_url(&config, "statetoken");
        assert!(url.contains("client_id=client123"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=statetoken"));
        assert!(url.contains("accounts.google.com"));
    }

    #[test]
    fn oauth_state_store_lifecycle() {
        let store = OAuthStateStore::new();
        let token = store.create("google");
        assert!(!token.is_empty());

        // Valid consumption
        let provider = store.validate_and_consume(&token);
        assert_eq!(provider.as_deref(), Some("google"));

        // Second consumption returns None (token is spent)
        let provider2 = store.validate_and_consume(&token);
        assert!(provider2.is_none());
    }

    #[test]
    fn oauth_state_store_wrong_provider_is_none() {
        let store = OAuthStateStore::new();
        let _token = store.create("github");
        // Different token entirely
        assert!(store.validate_and_consume("nonexistent").is_none());
    }

    #[test]
    fn urlencoding_simple_encodes_spaces() {
        let encoded = urlencoding_simple("openid email profile");
        assert!(encoded.contains("%20"));
        assert!(!encoded.contains(' '));
    }

    #[test]
    fn provider_from_str_roundtrip() {
        assert_eq!(
            OAuthProvider::from_str("google"),
            Some(OAuthProvider::Google)
        );
        assert_eq!(
            OAuthProvider::from_str("github"),
            Some(OAuthProvider::GitHub)
        );
        assert_eq!(OAuthProvider::from_str("azure"), Some(OAuthProvider::Oidc));
        assert!(OAuthProvider::from_str("unknown").is_none());
    }
}
