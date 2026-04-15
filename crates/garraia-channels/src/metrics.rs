//! Per-channel latency and throughput metrics.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Metrics for a single channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMetrics {
    /// Total messages sent through this channel.
    pub sent: u64,
    /// Total messages received through this channel.
    pub received: u64,
    /// Average latency in milliseconds for send operations.
    pub avg_latency_ms: f64,
    /// Total errors encountered.
    pub errors: u64,
}

impl Default for ChannelMetrics {
    fn default() -> Self {
        Self {
            sent: 0,
            received: 0,
            avg_latency_ms: 0.0,
            errors: 0,
        }
    }
}

/// Thread-safe, atomic metrics tracker for a single channel.
#[derive(Debug)]
pub struct AtomicChannelMetrics {
    sent: AtomicU64,
    received: AtomicU64,
    total_latency_us: AtomicU64,
    errors: AtomicU64,
}

impl AtomicChannelMetrics {
    /// Create a new zeroed metrics tracker.
    pub fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            total_latency_us: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    /// Record a successful send with latency in microseconds.
    pub fn record_send(&self, latency_us: u64) {
        self.sent.fetch_add(1, Ordering::Relaxed);
        self.total_latency_us
            .fetch_add(latency_us, Ordering::Relaxed);
    }

    /// Record a received message.
    pub fn record_receive(&self) {
        self.received.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an error.
    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot the current metrics into a serializable struct.
    pub fn snapshot(&self) -> ChannelMetrics {
        let sent = self.sent.load(Ordering::Relaxed);
        let total_latency_us = self.total_latency_us.load(Ordering::Relaxed);
        let avg_latency_ms = if sent > 0 {
            (total_latency_us as f64) / (sent as f64) / 1000.0
        } else {
            0.0
        };

        ChannelMetrics {
            sent,
            received: self.received.load(Ordering::Relaxed),
            avg_latency_ms,
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

impl Default for AtomicChannelMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of metrics for all channels.
#[derive(Debug, Clone)]
pub struct MetricsRegistry {
    channels: Arc<RwLock<HashMap<String, Arc<AtomicChannelMetrics>>>>,
}

impl MetricsRegistry {
    /// Create a new empty metrics registry.
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create metrics for a channel type.
    pub async fn get_or_create(&self, channel_type: &str) -> Arc<AtomicChannelMetrics> {
        // Fast path: check if already exists
        {
            let channels = self.channels.read().await;
            if let Some(metrics) = channels.get(channel_type) {
                return Arc::clone(metrics);
            }
        }

        // Slow path: create
        let mut channels = self.channels.write().await;
        let metrics = channels
            .entry(channel_type.to_string())
            .or_insert_with(|| Arc::new(AtomicChannelMetrics::new()));
        Arc::clone(metrics)
    }

    /// Snapshot all channel metrics.
    pub async fn snapshot_all(&self) -> HashMap<String, ChannelMetrics> {
        let channels = self.channels.read().await;
        channels
            .iter()
            .map(|(name, metrics)| (name.clone(), metrics.snapshot()))
            .collect()
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_metrics_default() {
        let m = AtomicChannelMetrics::new();
        let snap = m.snapshot();
        assert_eq!(snap.sent, 0);
        assert_eq!(snap.received, 0);
        assert_eq!(snap.avg_latency_ms, 0.0);
        assert_eq!(snap.errors, 0);
    }

    #[test]
    fn record_send_updates_metrics() {
        let m = AtomicChannelMetrics::new();
        m.record_send(1000); // 1ms
        m.record_send(3000); // 3ms
        let snap = m.snapshot();
        assert_eq!(snap.sent, 2);
        assert!((snap.avg_latency_ms - 2.0).abs() < 0.001);
    }

    #[test]
    fn record_receive_and_error() {
        let m = AtomicChannelMetrics::new();
        m.record_receive();
        m.record_receive();
        m.record_error();
        let snap = m.snapshot();
        assert_eq!(snap.received, 2);
        assert_eq!(snap.errors, 1);
    }

    #[tokio::test]
    async fn metrics_registry_get_or_create() {
        let registry = MetricsRegistry::new();
        let m1 = registry.get_or_create("telegram").await;
        m1.record_send(500);
        let m2 = registry.get_or_create("telegram").await;
        assert_eq!(m2.snapshot().sent, 1);

        let all = registry.snapshot_all().await;
        assert!(all.contains_key("telegram"));
        assert_eq!(all["telegram"].sent, 1);
    }
}
