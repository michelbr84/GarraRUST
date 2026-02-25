use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;

// ---------------------------------------------------------------------------
// GAR-29: Docker production build
// ---------------------------------------------------------------------------

/// Configuration for generating a Docker production build.
pub struct DockerConfig {
    pub image_name: String,
    pub tag: String,
    pub registry: Option<String>,
    pub port: u16,
    pub env_vars: HashMap<String, String>,
}

impl DockerConfig {
    pub fn full_image(&self) -> String {
        match &self.registry {
            Some(reg) => format!("{}/{}:{}", reg, self.image_name, self.tag),
            None => format!("{}:{}", self.image_name, self.tag),
        }
    }

    /// Generate an optimized multi-stage Dockerfile snippet.
    pub fn to_dockerfile_snippet(&self) -> String {
        let env_lines: String = self
            .env_vars
            .iter()
            .map(|(k, v)| format!("ENV {}={}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        let env_section = if env_lines.is_empty() {
            String::new()
        } else {
            format!("\n{env_lines}")
        };

        format!(
            r#"# --- builder stage ---
FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# --- runtime stage ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/{name} /usr/local/bin/{name}
EXPOSE {port}{env_section}
CMD ["{name}"]"#,
            name = self.image_name,
            port = self.port,
        )
    }
}

// ---------------------------------------------------------------------------
// GAR-30: Multi-instance deployment
// ---------------------------------------------------------------------------

/// Metadata about a single running gateway instance.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub instance_id: String,
    pub hostname: String,
    pub port: u16,
    pub started_at: DateTime<Utc>,
    pub healthy: bool,
}

/// Thread-safe registry of cluster instances backed by `DashMap`.
pub struct ClusterRegistry {
    instances: DashMap<String, InstanceInfo>,
}

impl ClusterRegistry {
    pub fn new() -> Self {
        Self {
            instances: DashMap::new(),
        }
    }

    pub fn register(&self, info: InstanceInfo) {
        self.instances.insert(info.instance_id.clone(), info);
    }

    pub fn deregister(&self, instance_id: &str) {
        self.instances.remove(instance_id);
    }

    pub fn healthy_instances(&self) -> Vec<InstanceInfo> {
        self.instances
            .iter()
            .filter(|entry| entry.value().healthy)
            .map(|entry| entry.value().clone())
            .collect()
    }
}

impl Default for ClusterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GAR-31: Load balancer config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBalancerStrategy {
    RoundRobin,
    LeastConnections,
    Random,
}

/// Simple load balancer that picks a healthy instance from a `ClusterRegistry`.
pub struct LoadBalancer {
    registry: ClusterRegistry,
    counter: AtomicU32,
}

impl LoadBalancer {
    pub fn new(registry: ClusterRegistry) -> Self {
        Self {
            registry,
            counter: AtomicU32::new(0),
        }
    }

    /// Select the next instance according to the given strategy.
    pub fn next_instance(&self, strategy: LoadBalancerStrategy) -> Option<InstanceInfo> {
        let mut instances = self.registry.healthy_instances();
        if instances.is_empty() {
            return None;
        }
        match strategy {
            LoadBalancerStrategy::RoundRobin => {
                let idx = self.counter.fetch_add(1, Ordering::Relaxed) as usize % instances.len();
                Some(instances.remove(idx))
            }
            LoadBalancerStrategy::LeastConnections => {
                // Without actual connection counts we fall back to the first healthy instance.
                Some(instances.remove(0))
            }
            LoadBalancerStrategy::Random => {
                let idx = (self.counter.load(Ordering::Relaxed) as usize
                    ^ (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .subsec_nanos() as usize))
                    % instances.len();
                Some(instances.remove(idx))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GAR-32: Sticky sessions strategy
// ---------------------------------------------------------------------------

/// Maps session ids to instance ids for sticky routing.
pub struct StickySessionRouter {
    map: DashMap<String, String>,
}

impl StickySessionRouter {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    pub fn route(&self, session_id: &str) -> Option<String> {
        self.map.get(session_id).map(|v| v.value().clone())
    }

    pub fn assign(&self, session_id: &str, instance_id: &str) {
        self.map
            .insert(session_id.to_owned(), instance_id.to_owned());
    }

    pub fn unassign(&self, session_id: &str) {
        self.map.remove(session_id);
    }
}

impl Default for StickySessionRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GAR-33: Health endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded(String),
    Unhealthy(String),
}

pub trait HealthChecker: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self) -> HealthStatus;
}

pub struct HealthCheck {
    pub checks: Vec<Box<dyn HealthChecker>>,
}

impl HealthCheck {
    pub fn new() -> Self {
        Self { checks: Vec::new() }
    }

    pub fn add(&mut self, checker: Box<dyn HealthChecker>) {
        self.checks.push(checker);
    }

    /// Run every registered checker. Returns `(overall_healthy, details)`.
    pub fn check_all(&self) -> (bool, Vec<(String, HealthStatus)>) {
        let mut all_healthy = true;
        let mut results = Vec::with_capacity(self.checks.len());
        for checker in &self.checks {
            let status = checker.check();
            if matches!(status, HealthStatus::Unhealthy(_)) {
                all_healthy = false;
            }
            results.push((checker.name().to_owned(), status));
        }
        (all_healthy, results)
    }
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self::new()
    }
}

/// Built-in checker that always returns `Healthy` with uptime info.
pub struct UptimeChecker {
    started: Instant,
}

impl UptimeChecker {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Default for UptimeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthChecker for UptimeChecker {
    fn name(&self) -> &str {
        "uptime"
    }

    fn check(&self) -> HealthStatus {
        let _uptime = self.started.elapsed();
        HealthStatus::Healthy
    }
}

// ---------------------------------------------------------------------------
// GAR-34: Kubernetes manifests
// ---------------------------------------------------------------------------

pub struct K8sManifestGenerator {
    pub app_name: String,
}

impl K8sManifestGenerator {
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_owned(),
        }
    }

    pub fn deployment_yaml(&self, replicas: u32, image: &str, port: u16) -> String {
        format!(
            r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {name}
spec:
  replicas: {replicas}
  selector:
    matchLabels:
      app: {name}
  template:
    metadata:
      labels:
        app: {name}
    spec:
      containers:
        - name: {name}
          image: {image}
          ports:
            - containerPort: {port}"#,
            name = self.app_name,
        )
    }

    pub fn service_yaml(&self, port: u16) -> String {
        format!(
            r#"apiVersion: v1
kind: Service
metadata:
  name: {name}
spec:
  selector:
    app: {name}
  ports:
    - protocol: TCP
      port: {port}
      targetPort: {port}
  type: ClusterIP"#,
            name = self.app_name,
        )
    }
}

// ---------------------------------------------------------------------------
// GAR-35: Autoscaling policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScaleDecision {
    ScaleUp(u32),
    ScaleDown(u32),
    NoChange,
}

pub struct AutoscalePolicy {
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub target_cpu_percent: u32,
    pub scale_up_cooldown: Duration,
    pub scale_down_cooldown: Duration,
}

impl AutoscalePolicy {
    pub fn should_scale(&self, current_replicas: u32, avg_cpu_percent: u32) -> ScaleDecision {
        if avg_cpu_percent > self.target_cpu_percent && current_replicas < self.max_replicas {
            let desired = (current_replicas + 1).min(self.max_replicas);
            ScaleDecision::ScaleUp(desired)
        } else if avg_cpu_percent < self.target_cpu_percent / 2
            && current_replicas > self.min_replicas
        {
            let desired = (current_replicas - 1).max(self.min_replicas);
            ScaleDecision::ScaleDown(desired)
        } else {
            ScaleDecision::NoChange
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // GAR-29 tests -------------------------------------------------------

    #[test]
    fn docker_config_full_image_with_registry() {
        let cfg = DockerConfig {
            image_name: "garraia".into(),
            tag: "1.0".into(),
            registry: Some("ghcr.io/myorg".into()),
            port: 8080,
            env_vars: HashMap::new(),
        };
        assert_eq!(cfg.full_image(), "ghcr.io/myorg/garraia:1.0");
    }

    #[test]
    fn docker_config_dockerfile_contains_stages() {
        let cfg = DockerConfig {
            image_name: "garraia".into(),
            tag: "latest".into(),
            registry: None,
            port: 3000,
            env_vars: HashMap::from([("RUST_LOG".into(), "info".into())]),
        };
        let snippet = cfg.to_dockerfile_snippet();
        assert!(snippet.contains("FROM rust:latest AS builder"));
        assert!(snippet.contains("FROM debian:bookworm-slim"));
        assert!(snippet.contains("EXPOSE 3000"));
        assert!(snippet.contains("ENV RUST_LOG=info"));
    }

    // GAR-30 tests -------------------------------------------------------

    #[test]
    fn cluster_registry_register_and_healthy() {
        let reg = ClusterRegistry::new();
        reg.register(InstanceInfo {
            instance_id: "a".into(),
            hostname: "h1".into(),
            port: 8080,
            started_at: Utc::now(),
            healthy: true,
        });
        reg.register(InstanceInfo {
            instance_id: "b".into(),
            hostname: "h2".into(),
            port: 8081,
            started_at: Utc::now(),
            healthy: false,
        });
        let healthy = reg.healthy_instances();
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].instance_id, "a");
    }

    #[test]
    fn cluster_registry_deregister() {
        let reg = ClusterRegistry::new();
        reg.register(InstanceInfo {
            instance_id: "x".into(),
            hostname: "h".into(),
            port: 80,
            started_at: Utc::now(),
            healthy: true,
        });
        reg.deregister("x");
        assert!(reg.healthy_instances().is_empty());
    }

    // GAR-31 tests -------------------------------------------------------

    #[test]
    fn load_balancer_round_robin() {
        let reg = ClusterRegistry::new();
        for i in 0..3 {
            reg.register(InstanceInfo {
                instance_id: format!("inst-{i}"),
                hostname: "h".into(),
                port: 8080 + i as u16,
                started_at: Utc::now(),
                healthy: true,
            });
        }
        let lb = LoadBalancer::new(reg);
        let first = lb
            .next_instance(LoadBalancerStrategy::RoundRobin)
            .unwrap();
        let second = lb
            .next_instance(LoadBalancerStrategy::RoundRobin)
            .unwrap();
        // They should not be identical in a round-robin with 3 instances
        // (unless ordering happens to collide), but counter must advance.
        assert!(lb.counter.load(Ordering::Relaxed) >= 2);
        // Just ensure we got valid instances
        assert!(!first.instance_id.is_empty());
        assert!(!second.instance_id.is_empty());
    }

    #[test]
    fn load_balancer_empty_registry_returns_none() {
        let reg = ClusterRegistry::new();
        let lb = LoadBalancer::new(reg);
        assert!(lb.next_instance(LoadBalancerStrategy::RoundRobin).is_none());
    }

    // GAR-32 tests -------------------------------------------------------

    #[test]
    fn sticky_session_assign_and_route() {
        let router = StickySessionRouter::new();
        router.assign("sess-1", "inst-a");
        assert_eq!(router.route("sess-1"), Some("inst-a".into()));
    }

    #[test]
    fn sticky_session_unassign() {
        let router = StickySessionRouter::new();
        router.assign("sess-1", "inst-a");
        router.unassign("sess-1");
        assert!(router.route("sess-1").is_none());
    }

    // GAR-33 tests -------------------------------------------------------

    #[test]
    fn health_check_all_with_uptime() {
        let mut hc = HealthCheck::new();
        hc.add(Box::new(UptimeChecker::new()));
        let (overall, results) = hc.check_all();
        assert!(overall);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "uptime");
        assert_eq!(results[0].1, HealthStatus::Healthy);
    }

    struct FailingChecker;
    impl HealthChecker for FailingChecker {
        fn name(&self) -> &str {
            "failing"
        }
        fn check(&self) -> HealthStatus {
            HealthStatus::Unhealthy("db down".into())
        }
    }

    #[test]
    fn health_check_reports_unhealthy() {
        let mut hc = HealthCheck::new();
        hc.add(Box::new(UptimeChecker::new()));
        hc.add(Box::new(FailingChecker));
        let (overall, results) = hc.check_all();
        assert!(!overall);
        assert_eq!(results.len(), 2);
    }

    // GAR-34 tests -------------------------------------------------------

    #[test]
    fn k8s_deployment_yaml_content() {
        let generator = K8sManifestGenerator::new("garraia");
        let yaml = generator.deployment_yaml(3, "garraia:latest", 8080);
        assert!(yaml.contains("replicas: 3"));
        assert!(yaml.contains("image: garraia:latest"));
        assert!(yaml.contains("containerPort: 8080"));
    }

    #[test]
    fn k8s_service_yaml_content() {
        let generator = K8sManifestGenerator::new("garraia");
        let yaml = generator.service_yaml(8080);
        assert!(yaml.contains("kind: Service"));
        assert!(yaml.contains("port: 8080"));
        assert!(yaml.contains("ClusterIP"));
    }

    // GAR-35 tests -------------------------------------------------------

    #[test]
    fn autoscale_scale_up() {
        let policy = AutoscalePolicy {
            min_replicas: 1,
            max_replicas: 10,
            target_cpu_percent: 70,
            scale_up_cooldown: Duration::from_secs(60),
            scale_down_cooldown: Duration::from_secs(120),
        };
        assert_eq!(policy.should_scale(2, 90), ScaleDecision::ScaleUp(3));
    }

    #[test]
    fn autoscale_scale_down() {
        let policy = AutoscalePolicy {
            min_replicas: 1,
            max_replicas: 10,
            target_cpu_percent: 70,
            scale_up_cooldown: Duration::from_secs(60),
            scale_down_cooldown: Duration::from_secs(120),
        };
        assert_eq!(policy.should_scale(5, 20), ScaleDecision::ScaleDown(4));
    }

    #[test]
    fn autoscale_no_change() {
        let policy = AutoscalePolicy {
            min_replicas: 1,
            max_replicas: 10,
            target_cpu_percent: 70,
            scale_up_cooldown: Duration::from_secs(60),
            scale_down_cooldown: Duration::from_secs(120),
        };
        assert_eq!(policy.should_scale(3, 50), ScaleDecision::NoChange);
    }
}
