use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rusqlite::Connection;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// GAR-19: Token bucket per tenant
// ---------------------------------------------------------------------------

pub struct TokenBucket {
    tokens: AtomicU64,
    max_tokens: u64,
    refill_rate: u64,
    last_refill: RwLock<Instant>,
}

impl TokenBucket {
    pub fn new(max_tokens: u64, refill_rate: u64) -> Self {
        Self {
            tokens: AtomicU64::new(max_tokens),
            max_tokens,
            refill_rate,
            last_refill: RwLock::new(Instant::now()),
        }
    }

    /// Refill tokens based on elapsed time, then attempt to consume `n`.
    pub fn try_consume(&self, n: u64) -> bool {
        {
            let mut last = self.last_refill.write().unwrap();
            let elapsed_ms = last.elapsed().as_millis() as u64;
            let refill = (elapsed_ms * self.refill_rate) / 1000;
            if refill > 0 {
                let current = self.tokens.load(Ordering::Relaxed);
                let new_tokens = (current + refill).min(self.max_tokens);
                self.tokens.store(new_tokens, Ordering::Relaxed);
                *last = Instant::now();
            }
        }

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current < n {
                return false;
            }
            if self
                .tokens
                .compare_exchange(current, current - n, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn available(&self) -> u64 {
        self.tokens.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// GAR-20: Quota tracking
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct QuotaExceeded {
    pub tenant_id: String,
    pub used: u64,
    pub limit: u64,
}

impl fmt::Display for QuotaExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tenant {} exceeded quota: {}/{}",
            self.tenant_id, self.used, self.limit
        )
    }
}

pub struct TenantQuota {
    pub used: AtomicU64,
    pub limit: u64,
}

pub struct QuotaTracker {
    limits: RwLock<HashMap<String, TenantQuota>>,
}

impl QuotaTracker {
    pub fn new() -> Self {
        Self {
            limits: RwLock::new(HashMap::new()),
        }
    }

    pub fn set_limit(&self, tenant_id: &str, limit: u64) {
        let mut map = self.limits.write().unwrap();
        map.insert(
            tenant_id.to_string(),
            TenantQuota {
                used: AtomicU64::new(0),
                limit,
            },
        );
    }

    pub fn check_and_increment(&self, tenant_id: &str, tokens: u64) -> Result<(), QuotaExceeded> {
        let map = self.limits.read().unwrap();
        let Some(quota) = map.get(tenant_id) else {
            return Ok(());
        };

        loop {
            let used = quota.used.load(Ordering::Acquire);
            if used + tokens > quota.limit {
                return Err(QuotaExceeded {
                    tenant_id: tenant_id.to_string(),
                    used,
                    limit: quota.limit,
                });
            }
            if quota
                .used
                .compare_exchange(used, used + tokens, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(());
            }
        }
    }

    /// Returns (used, limit) for the given tenant.
    pub fn usage(&self, tenant_id: &str) -> (u64, u64) {
        let map = self.limits.read().unwrap();
        match map.get(tenant_id) {
            Some(q) => (q.used.load(Ordering::Relaxed), q.limit),
            None => (0, 0),
        }
    }
}

impl Default for QuotaTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GAR-21: Usage persistence table
// ---------------------------------------------------------------------------

pub struct UsageStore {
    conn: Connection,
}

impl UsageStore {
    pub fn new(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_records (
                id          TEXT PRIMARY KEY,
                tenant_id   TEXT NOT NULL,
                tokens_used INTEGER NOT NULL,
                provider    TEXT NOT NULL,
                created_at  TEXT DEFAULT (datetime('now'))
            )",
        )?;
        Ok(Self { conn })
    }

    pub fn record(&self, tenant_id: &str, tokens: u64, provider: &str) -> rusqlite::Result<()> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO usage_records (id, tenant_id, tokens_used, provider) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, tenant_id, tokens as i64, provider],
        )?;
        Ok(())
    }

    pub fn total_for_tenant(&self, tenant_id: &str) -> u64 {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(tokens_used), 0) FROM usage_records WHERE tenant_id = ?1",
                rusqlite::params![tenant_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as u64
    }
}

// ---------------------------------------------------------------------------
// GAR-22: Plan system (Free / Pro)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Free,
    Pro,
}

impl Plan {
    pub fn tokens_per_day(&self) -> u64 {
        match self {
            Plan::Free => 10_000,
            Plan::Pro => 1_000_000,
        }
    }

    pub fn requests_per_hour(&self) -> u64 {
        match self {
            Plan::Free => 100,
            Plan::Pro => 10_000,
        }
    }
}

pub struct PlanConfig {
    tenants: RwLock<HashMap<String, Plan>>,
}

impl PlanConfig {
    pub fn new() -> Self {
        Self {
            tenants: RwLock::new(HashMap::new()),
        }
    }

    pub fn set_plan(&self, tenant_id: &str, plan: Plan) {
        self.tenants
            .write()
            .unwrap()
            .insert(tenant_id.to_string(), plan);
    }

    pub fn get_plan(&self, tenant_id: &str) -> Plan {
        self.tenants
            .read()
            .unwrap()
            .get(tenant_id)
            .copied()
            .unwrap_or(Plan::Free)
    }
}

impl Default for PlanConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GAR-23: Limit enforcement middleware
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum BillingError {
    RateLimited,
    QuotaExceeded(QuotaExceeded),
}

impl fmt::Display for BillingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BillingError::RateLimited => write!(f, "rate limit exceeded"),
            BillingError::QuotaExceeded(e) => write!(f, "{e}"),
        }
    }
}

pub fn check_billing(
    bucket: &TokenBucket,
    quota: &QuotaTracker,
    tenant_id: &str,
    tokens: u64,
) -> Result<(), BillingError> {
    if !bucket.try_consume(tokens) {
        return Err(BillingError::RateLimited);
    }
    quota
        .check_and_increment(tenant_id, tokens)
        .map_err(BillingError::QuotaExceeded)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    // -- TokenBucket --------------------------------------------------------

    #[test]
    fn token_bucket_consume_within_capacity() {
        let bucket = TokenBucket::new(100, 10);
        assert!(bucket.try_consume(50));
        assert_eq!(bucket.available(), 50);
    }

    #[test]
    fn token_bucket_rejects_when_empty() {
        let bucket = TokenBucket::new(10, 0);
        assert!(bucket.try_consume(10));
        assert!(!bucket.try_consume(1));
        assert_eq!(bucket.available(), 0);
    }

    #[test]
    fn token_bucket_refills_over_time() {
        let bucket = TokenBucket::new(100, 50);
        assert!(bucket.try_consume(100));
        assert_eq!(bucket.available(), 0);

        thread::sleep(Duration::from_millis(120));

        assert!(bucket.try_consume(1));
    }

    #[test]
    fn token_bucket_does_not_exceed_max() {
        let bucket = TokenBucket::new(20, 1000);
        thread::sleep(Duration::from_millis(50));
        bucket.try_consume(0);
        assert!(bucket.available() <= 20);
    }

    // -- QuotaTracker -------------------------------------------------------

    #[test]
    fn quota_tracker_increment_and_usage() {
        let qt = QuotaTracker::new();
        qt.set_limit("t1", 1000);

        qt.check_and_increment("t1", 200).unwrap();
        qt.check_and_increment("t1", 300).unwrap();

        let (used, limit) = qt.usage("t1");
        assert_eq!(used, 500);
        assert_eq!(limit, 1000);
    }

    #[test]
    fn quota_tracker_rejects_when_exceeded() {
        let qt = QuotaTracker::new();
        qt.set_limit("t1", 100);
        qt.check_and_increment("t1", 90).unwrap();

        let err = qt.check_and_increment("t1", 20).unwrap_err();
        assert_eq!(err.used, 90);
        assert_eq!(err.limit, 100);

        let (used, _) = qt.usage("t1");
        assert_eq!(used, 90, "failed increment must not change usage");
    }

    #[test]
    fn quota_tracker_allows_unknown_tenant() {
        let qt = QuotaTracker::new();
        assert!(qt.check_and_increment("unknown", 999).is_ok());
        assert_eq!(qt.usage("unknown"), (0, 0));
    }

    // -- UsageStore ---------------------------------------------------------

    #[test]
    fn usage_store_record_and_total() {
        let conn = Connection::open_in_memory().unwrap();
        let store = UsageStore::new(conn).unwrap();

        store.record("tenant-a", 100, "openai").unwrap();
        store.record("tenant-a", 250, "anthropic").unwrap();
        store.record("tenant-b", 50, "openai").unwrap();

        assert_eq!(store.total_for_tenant("tenant-a"), 350);
        assert_eq!(store.total_for_tenant("tenant-b"), 50);
        assert_eq!(store.total_for_tenant("tenant-c"), 0);
    }

    // -- Plan & PlanConfig --------------------------------------------------

    #[test]
    fn plan_limits() {
        assert_eq!(Plan::Free.tokens_per_day(), 10_000);
        assert_eq!(Plan::Free.requests_per_hour(), 100);
        assert_eq!(Plan::Pro.tokens_per_day(), 1_000_000);
        assert_eq!(Plan::Pro.requests_per_hour(), 10_000);
    }

    #[test]
    fn plan_config_defaults_to_free() {
        let pc = PlanConfig::new();
        assert_eq!(pc.get_plan("new-tenant"), Plan::Free);

        pc.set_plan("pro-tenant", Plan::Pro);
        assert_eq!(pc.get_plan("pro-tenant"), Plan::Pro);
    }

    // -- check_billing (integration) ----------------------------------------

    #[test]
    fn check_billing_rate_limited() {
        let bucket = TokenBucket::new(10, 0);
        let quota = QuotaTracker::new();
        quota.set_limit("t1", 10_000);

        assert!(check_billing(&bucket, &quota, "t1", 10).is_ok());
        assert!(matches!(
            check_billing(&bucket, &quota, "t1", 1),
            Err(BillingError::RateLimited)
        ));
    }

    #[test]
    fn check_billing_quota_exceeded() {
        let bucket = TokenBucket::new(100_000, 0);
        let quota = QuotaTracker::new();
        quota.set_limit("t1", 50);

        assert!(check_billing(&bucket, &quota, "t1", 50).is_ok());
        assert!(matches!(
            check_billing(&bucket, &quota, "t1", 1),
            Err(BillingError::QuotaExceeded(_))
        ));
    }
}
