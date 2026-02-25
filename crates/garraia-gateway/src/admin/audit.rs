use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

use super::store::AdminStore;

/// Convenience wrapper for appending audit entries from async handlers.
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
