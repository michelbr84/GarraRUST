use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::{info, warn};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Per-provider circuit breaker that tracks failures and prevents
/// cascading calls to an unhealthy provider.
#[derive(Debug)]
pub struct CircuitBreaker {
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure: RwLock<Option<Instant>>,
    is_open: AtomicBool,
    /// How many consecutive failures before opening the circuit.
    pub failure_threshold: u64,
    /// How long the circuit stays open before moving to half-open.
    pub recovery_timeout: Duration,
    /// How many successes in half-open before closing.
    pub half_open_successes: u64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u64, recovery_timeout: Duration) -> Self {
        Self {
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure: RwLock::new(None),
            is_open: AtomicBool::new(false),
            failure_threshold,
            recovery_timeout,
            half_open_successes: 2,
        }
    }

    pub async fn state(&self) -> CircuitState {
        if !self.is_open.load(Ordering::Relaxed) {
            return CircuitState::Closed;
        }

        let guard = self.last_failure.read().await;
        if let Some(last) = *guard {
            if last.elapsed() >= self.recovery_timeout {
                return CircuitState::HalfOpen;
            }
        }
        CircuitState::Open
    }

    pub async fn allow_request(&self) -> bool {
        match self.state().await {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => false,
        }
    }

    pub async fn record_success(&self) {
        let state = self.state().await;
        if state == CircuitState::HalfOpen {
            let count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
            if count >= self.half_open_successes {
                self.is_open.store(false, Ordering::Relaxed);
                self.failure_count.store(0, Ordering::Relaxed);
                self.success_count.store(0, Ordering::Relaxed);
                info!("circuit breaker closed after recovery");
            }
        } else {
            self.failure_count.store(0, Ordering::Relaxed);
            self.success_count.store(0, Ordering::Relaxed);
        }
    }

    pub async fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        self.success_count.store(0, Ordering::Relaxed);
        if count >= self.failure_threshold {
            self.is_open.store(true, Ordering::Relaxed);
            let mut guard = self.last_failure.write().await;
            *guard = Some(Instant::now());
            warn!(
                "circuit breaker opened after {} failures",
                self.failure_threshold
            );
        }
    }

    pub fn reset(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        self.is_open.store(false, Ordering::Relaxed);
    }
}

/// Retry policy with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
        }
    }
}

impl RetryPolicy {
    /// Compute the delay for a given attempt (0-based).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.initial_delay.as_millis() as f64;
        let delay_ms = base * self.backoff_factor.powi(attempt as i32);
        let capped = delay_ms.min(self.max_delay.as_millis() as f64);
        Duration::from_millis(capped as u64)
    }
}

/// Priority-ordered fallback list for providers.
#[derive(Debug, Clone)]
pub struct FallbackConfig {
    /// Provider IDs in priority order (first = highest priority).
    pub provider_order: Vec<String>,
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            provider_order: Vec::new(),
        }
    }
}

impl FallbackConfig {
    pub fn new(order: Vec<String>) -> Self {
        Self {
            provider_order: order,
        }
    }
}

/// Cached model list for a provider.
#[derive(Debug, Clone)]
struct CachedModels {
    models: Vec<String>,
    fetched_at: Instant,
}

/// Per-provider health status.
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub provider_id: String,
    pub healthy: bool,
    pub last_check: Option<Instant>,
    pub latency_ms: Option<u64>,
}

/// Central resilience manager for all providers.
pub struct ResilienceManager {
    circuit_breakers: RwLock<HashMap<String, Arc<CircuitBreaker>>>,
    model_cache: RwLock<HashMap<String, CachedModels>>,
    pub retry_policy: RetryPolicy,
    pub fallback_config: RwLock<FallbackConfig>,
    health_status: RwLock<HashMap<String, ProviderHealth>>,
    cache_ttl: Duration,
}

impl ResilienceManager {
    pub fn new() -> Self {
        Self {
            circuit_breakers: RwLock::new(HashMap::new()),
            model_cache: RwLock::new(HashMap::new()),
            retry_policy: RetryPolicy::default(),
            fallback_config: RwLock::new(FallbackConfig::default()),
            health_status: RwLock::new(HashMap::new()),
            cache_ttl: Duration::from_secs(300),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Get or create a circuit breaker for a provider.
    pub async fn circuit_breaker(&self, provider_id: &str) -> Arc<CircuitBreaker> {
        let breakers = self.circuit_breakers.read().await;
        if let Some(cb) = breakers.get(provider_id) {
            return Arc::clone(cb);
        }
        drop(breakers);

        let mut breakers = self.circuit_breakers.write().await;
        let cb = breakers
            .entry(provider_id.to_string())
            .or_insert_with(|| {
                Arc::new(CircuitBreaker::new(5, Duration::from_secs(60)))
            });
        Arc::clone(cb)
    }

    /// Record that a request to a provider succeeded.
    pub async fn record_success(&self, provider_id: &str) {
        let cb = self.circuit_breaker(provider_id).await;
        cb.record_success().await;
    }

    /// Record that a request to a provider failed.
    pub async fn record_failure(&self, provider_id: &str) {
        let cb = self.circuit_breaker(provider_id).await;
        cb.record_failure().await;
    }

    /// Check whether a provider is allowed to receive requests.
    pub async fn is_provider_available(&self, provider_id: &str) -> bool {
        let cb = self.circuit_breaker(provider_id).await;
        cb.allow_request().await
    }

    /// Update the health status for a provider.
    pub async fn update_health(
        &self,
        provider_id: &str,
        healthy: bool,
        latency_ms: Option<u64>,
    ) {
        let mut statuses = self.health_status.write().await;
        statuses.insert(
            provider_id.to_string(),
            ProviderHealth {
                provider_id: provider_id.to_string(),
                healthy,
                last_check: Some(Instant::now()),
                latency_ms,
            },
        );
    }

    /// Get the health status of all tracked providers.
    pub async fn all_health(&self) -> Vec<ProviderHealth> {
        let statuses = self.health_status.read().await;
        statuses.values().cloned().collect()
    }

    /// Cache a list of models for a provider.
    pub async fn cache_models(&self, provider_id: &str, models: Vec<String>) {
        let mut cache = self.model_cache.write().await;
        cache.insert(
            provider_id.to_string(),
            CachedModels {
                models,
                fetched_at: Instant::now(),
            },
        );
    }

    /// Get cached models if they haven't expired.
    pub async fn get_cached_models(&self, provider_id: &str) -> Option<Vec<String>> {
        let cache = self.model_cache.read().await;
        cache.get(provider_id).and_then(|cached| {
            if cached.fetched_at.elapsed() < self.cache_ttl {
                Some(cached.models.clone())
            } else {
                None
            }
        })
    }

    /// Determine the next available provider based on fallback priority
    /// and circuit breaker state.
    pub async fn next_available_provider(&self, exclude: &[&str]) -> Option<String> {
        let config = self.fallback_config.read().await;
        for provider_id in &config.provider_order {
            if exclude.contains(&provider_id.as_str()) {
                continue;
            }
            if self.is_provider_available(provider_id).await {
                return Some(provider_id.clone());
            }
        }
        None
    }

    /// Set the fallback provider order.
    pub async fn set_fallback_order(&self, order: Vec<String>) {
        let mut config = self.fallback_config.write().await;
        config.provider_order = order;
    }
}

impl Default for ResilienceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn circuit_breaker_stays_closed_on_success() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        assert_eq!(cb.state().await, CircuitState::Closed);
        cb.record_success().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_threshold_failures() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);
        assert!(!cb.allow_request().await);
    }

    #[tokio::test]
    async fn circuit_breaker_half_open_after_recovery_timeout() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(10));
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);
        tokio::time::sleep(Duration::from_millis(15)).await;
        assert_eq!(cb.state().await, CircuitState::HalfOpen);
        assert!(cb.allow_request().await);
    }

    #[tokio::test]
    async fn circuit_breaker_closes_after_half_open_successes() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(10));
        cb.record_failure().await;
        cb.record_failure().await;
        tokio::time::sleep(Duration::from_millis(15)).await;
        assert_eq!(cb.state().await, CircuitState::HalfOpen);
        cb.record_success().await;
        cb.record_success().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[test]
    fn retry_policy_exponential_backoff() {
        let policy = RetryPolicy {
            max_retries: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_factor: 2.0,
        };
        assert_eq!(policy.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(800));
    }

    #[test]
    fn retry_policy_caps_at_max_delay() {
        let policy = RetryPolicy {
            max_retries: 10,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            backoff_factor: 10.0,
        };
        assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(5));
    }

    #[tokio::test]
    async fn resilience_manager_caches_models() {
        let mgr = ResilienceManager::new();
        assert!(mgr.get_cached_models("openai").await.is_none());
        mgr.cache_models("openai", vec!["gpt-4o".into(), "gpt-4".into()])
            .await;
        let cached = mgr.get_cached_models("openai").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn resilience_manager_fallback_skips_unavailable() {
        let mgr = ResilienceManager::new();
        mgr.set_fallback_order(vec![
            "anthropic".into(),
            "openai".into(),
            "ollama".into(),
        ])
        .await;

        // Open anthropic circuit breaker
        let cb = mgr.circuit_breaker("anthropic").await;
        for _ in 0..5 {
            cb.record_failure().await;
        }

        let next = mgr.next_available_provider(&[]).await;
        assert_eq!(next, Some("openai".to_string()));
    }

    #[tokio::test]
    async fn resilience_manager_health_tracking() {
        let mgr = ResilienceManager::new();
        mgr.update_health("openai", true, Some(150)).await;
        mgr.update_health("anthropic", false, None).await;

        let health = mgr.all_health().await;
        assert_eq!(health.len(), 2);
    }
}
