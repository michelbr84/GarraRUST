//! OTLP gRPC tracer pipeline.

use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    trace::{self as sdktrace, Sampler, TracerProvider},
};

use crate::{Error, config::TelemetryConfig};

pub fn init_tracer(config: &TelemetryConfig) -> Result<Option<TracerProvider>, Error> {
    if !config.enabled {
        return Ok(None);
    }

    let endpoint = config
        .otlp_endpoint
        .clone()
        .unwrap_or_else(|| "http://localhost:4317".to_string());

    let resource = Resource::new(vec![KeyValue::new(
        "service.name",
        config.service_name.clone(),
    )]);

    let trace_config = sdktrace::Config::default()
        .with_sampler(Sampler::TraceIdRatioBased(config.sample_ratio))
        .with_resource(resource);

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint);

    let provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(trace_config)
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .map_err(|e| Error::Init(format!("failed to install OTLP tracer: {e}")))?;

    global::set_tracer_provider(provider.clone());

    Ok(Some(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_returns_none() {
        let cfg = TelemetryConfig::default();
        let provider = init_tracer(&cfg).expect("disabled path must not error");
        assert!(provider.is_none());
    }
}
