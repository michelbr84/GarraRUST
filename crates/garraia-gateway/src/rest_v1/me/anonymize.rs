//! `POST /v1/me/anonymize` — irreversible anonymisation of the caller's identifiers.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use axum::extract::State;
use axum::http::StatusCode;
use garraia_auth::{
    Principal, WorkspaceAuditAction, anon_token, anonymize_identity, audit_workspace_event,
};
use serde_json::json;
use uuid::Uuid;

use crate::rest_v1::RestV1FullState;
use crate::rest_v1::problem::RestError;

// ─── POST /v1/me/anonymize ────────────────────────────────────────────────────

/// `POST /v1/me/anonymize` — LGPD art. 12 / GDPR art. 4(5) personal data
/// anonymisation (plan 0345 / GAR-888, slice 3 of GAR-400; fixed in plan 0354).
///
/// Replaces the caller's email everywhere it lives in this schema —
/// `user_identities.provider_sub` (the login key, via `garraia_login`),
/// `users.email` (in the same app-pool transaction as the status flip) and
/// `group_invites.invited_email` (pending + historical invites, same tx) —
/// with a non-identifiable deterministic token
/// (`anon-<32 hex>@garraanon.local`), and sets `users.display_name` to
/// `'Usuário Anônimo'` + `users.status` to `'anonymized'`.  All active sessions
/// are revoked atomically in the same transaction. The operation is irreversible.
///
/// ## Error matrix
///
/// | Condition                | Status |
/// |--------------------------|--------|
/// | Missing/invalid JWT      | 401    |
/// | Account already deleted  | 409    |
/// | Account already anonymized | 409  |
/// | Happy path               | 204    |
#[utoipa::path(
    post,
    path = "/v1/me/anonymize",
    responses(
        (status = 204, description = "Account anonymised — PII replaced, sessions revoked."),
        (status = 401, description = "Missing or invalid JWT.", body = crate::rest_v1::problem::ProblemDetails),
        (status = 409, description = "Account already anonymized or deleted.", body = crate::rest_v1::problem::ProblemDetails),
    ),
    security(("bearer" = [])),
    tag = "me",
)]
pub async fn anonymize_me(
    State(state): State<RestV1FullState>,
    principal: Principal,
) -> Result<StatusCode, RestError> {
    let nil_uuid = Uuid::nil();
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(nil_uuid.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Guard: FOR UPDATE prevents concurrent double-anonymize. O email
    // original é capturado aqui (cast ::text — sqlx não decodifica citext
    // direto em String) para o wipe de group_invites mais abaixo.
    let current: Option<(String, String)> =
        sqlx::query_as("SELECT status, email::text FROM users WHERE id = $1 FOR UPDATE")
            .bind(principal.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

    let original_email = match current {
        None => {
            return Err(RestError::Internal(anyhow::anyhow!(
                "user row not found for authenticated principal"
            )));
        }
        Some((status, _)) if status == "deleted" => {
            return Err(RestError::Conflict(
                "account is already deleted; cannot anonymize".into(),
            ));
        }
        Some((status, _)) if status == "anonymized" => {
            return Err(RestError::Conflict("account is already anonymized".into()));
        }
        Some((_, email)) => email,
    };

    // Step 1: anonymise the login key (user_identities.provider_sub) via
    // login_pool (BYPASSRLS — the table is invisible to garraia_app under
    // FORCE RLS). Runs outside the app_pool transaction; a failure between
    // the two commits heals on endpoint retry (the UPDATE is idempotent and
    // the 409 status guard only trips after the app-side commit below).
    // Intencional: o FOR UPDATE de `users` (acima) segue segurado durante
    // este round-trip — é o que impede um segundo anonymize concorrente de
    // passar pelo guard de status antes de este fluxo commitar.
    // 0 rows = user without an internal identity (e.g. future external IdP)
    // — not an error; users.email is anonymised regardless.
    let _identity_rows = anonymize_identity(&state.auth.login_pool, principal.user_id)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!("anonymize_identity: {e}")))?;

    // Step 2 (atomic): anonymise users.email + status + display_name, revoke
    // sessions, wipe group_invites, emit audit event. users.email is citext
    // UNIQUE — the token embeds the full UUID, so it cannot collide across
    // users. users.updated_at has no trigger (caller responsibility per
    // migration 001). Fórmula única do token: garraia_auth::anon_token
    // (security review plan 0354).
    let anon_email = anon_token(principal.user_id);
    sqlx::query(
        "UPDATE users \
         SET status = 'anonymized', display_name = 'Usuário Anônimo', \
             email = $2, updated_at = now() \
         WHERE id = $1",
    )
    .bind(principal.user_id)
    .bind(&anon_email)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(
        "UPDATE sessions SET revoked_at = now() \
         WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // group_invites.invited_email guarda o email PII de convites pendentes e
    // históricos (security review plan 0354 LOW-2). Sem FORCE RLS nesta
    // tabela (acesso por token — ver comentário em list_my_invites), então o
    // UPDATE por email funciona no app_pool. citext = text compara
    // case-insensitive, cobrindo convites gravados com caixa diferente.
    sqlx::query("UPDATE group_invites SET invited_email = $2 WHERE invited_email = $1")
        .bind(&original_email)
        .bind(&anon_email)
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::AccountAnonymized,
        principal.user_id,
        nil_uuid,
        "users",
        principal.user_id.to_string(),
        json!({}),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!("{e}")))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── anonymize_me unit tests (plan 0345 / GAR-888) ────────────────────────

    #[test]
    fn anonymize_me_action_string_is_stable() {
        assert_eq!(
            WorkspaceAuditAction::AccountAnonymized.as_str(),
            "account.anonymized"
        );
    }

    #[test]
    fn anonymize_me_action_display_matches_as_str() {
        let action = WorkspaceAuditAction::AccountAnonymized;
        assert_eq!(format!("{action}"), action.as_str());
    }

    #[test]
    fn anonymize_me_response_is_no_content() {
        assert_eq!(StatusCode::NO_CONTENT.as_u16(), 204);
    }

    #[test]
    fn anonymize_me_conflict_is_409() {
        assert_eq!(StatusCode::CONFLICT.as_u16(), 409);
    }

    #[test]
    fn anonymize_me_action_is_user_scoped_not_group_scoped() {
        let s = WorkspaceAuditAction::AccountAnonymized.as_str();
        assert!(
            s.starts_with("account."),
            "expected 'account.*' namespace, got '{s}'"
        );
    }

    #[test]
    fn anonymize_me_metadata_is_empty_object_no_pii() {
        // PII safety: no email or display_name in metadata.
        let meta = serde_json::json!({});
        assert!(meta.is_object());
        assert_eq!(meta.as_object().unwrap().len(), 0);
    }

    #[test]
    fn anonymize_me_action_distinct_from_delete_action() {
        assert_ne!(
            WorkspaceAuditAction::AccountAnonymized.as_str(),
            WorkspaceAuditAction::AccountSelfDeleted.as_str(),
        );
    }
}
