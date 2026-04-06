//! Audit log helpers and query types for Phase 7.1.
//!
//! The actual persistence is delegated to [`AdminStore::append_audit`] and
//! [`AdminStore::list_audit_log`] via the convenience wrappers below.

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

use super::store::{AdminStore, AuditEntry};

// ── Public re-export for callers that want the full entry type ────────────────

pub use super::store::AuditEntry as AuditLogEntry;

// ── Filter struct used by query_audit ────────────────────────────────────────

/// Optional filters for querying the audit log via `GET /api/admin/audit`.
#[derive(Debug, Default, serde::Deserialize)]
pub struct AuditFilter {
    /// Filter by user_id (exact match).
    pub user_id: Option<String>,
    /// Filter by action string (exact match).
    pub action: Option<String>,
    /// Filter by resource_type (exact match).
    pub resource_type: Option<String>,
    /// ISO-8601 lower bound for timestamp (inclusive).
    pub from: Option<String>,
    /// ISO-8601 upper bound for timestamp (inclusive).
    pub to: Option<String>,
    /// Maximum number of rows to return (default 100, max 1000).
    pub limit: Option<usize>,
    /// Row offset for pagination (default 0).
    pub offset: Option<usize>,
}

// ── High-level helpers ────────────────────────────────────────────────────────

/// Append a single audit entry. Failures are logged as warnings and swallowed
/// so that a failed audit write never blocks the primary operation.
pub async fn log_action(
    store: &Arc<Mutex<AdminStore>>,
    user_id: Option<&str>,
    username: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    details: Option<&str>,
    ip_address: Option<&str>,
    outcome: &str,
) {
    let guard = store.lock().await;
    if let Err(e) = guard.append_audit(
        user_id,
        username,
        action,
        resource_type,
        resource_id,
        details,
        ip_address,
        outcome,
    ) {
        warn!("failed to write audit log: {e}");
    }
}

/// Query the audit log with optional filters.
///
/// Returns at most `filter.limit` entries (default 100, capped at 1000),
/// ordered newest-first.
pub async fn query_audit(
    store: &Arc<Mutex<AdminStore>>,
    filter: &AuditFilter,
) -> Vec<AuditEntry> {
    let limit = filter.limit.unwrap_or(100).min(1000);
    let offset = filter.offset.unwrap_or(0);

    let guard = store.lock().await;
    guard.list_audit_log_filtered(
        limit,
        offset,
        filter.user_id.as_deref(),
        filter.action.as_deref(),
        filter.resource_type.as_deref(),
        filter.from.as_deref(),
        filter.to.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::store::AdminStore;

    fn make_store() -> Arc<Mutex<AdminStore>> {
        Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory store"),
        ))
    }

    #[tokio::test]
    async fn log_and_query_roundtrip() {
        let store = make_store();

        log_action(
            &store,
            Some("uid-1"),
            Some("alice"),
            "delete",
            "secret",
            Some("OPENAI_KEY"),
            None,
            Some("10.0.0.1"),
            "success",
        )
        .await;

        log_action(
            &store,
            Some("uid-2"),
            Some("bob"),
            "login",
            "session",
            None,
            None,
            Some("10.0.0.2"),
            "failure",
        )
        .await;

        let all = query_audit(&store, &AuditFilter::default()).await;
        assert_eq!(all.len(), 2);

        let filtered = query_audit(
            &store,
            &AuditFilter {
                action: Some("login".into()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].username.as_deref(), Some("bob"));
    }

    #[tokio::test]
    async fn filter_by_user_id() {
        let store = make_store();

        for i in 0..5u32 {
            log_action(
                &store,
                Some(&format!("uid-{i}")),
                None,
                "read",
                "config",
                None,
                None,
                None,
                "success",
            )
            .await;
        }

        let filtered = query_audit(
            &store,
            &AuditFilter {
                user_id: Some("uid-3".into()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].user_id.as_deref(), Some("uid-3"));
    }
}
