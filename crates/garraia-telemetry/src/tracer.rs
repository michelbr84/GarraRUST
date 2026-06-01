//! OTLP gRPC tracer pipeline.

use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    trace::{Sampler, SdkTracerProvider},
};

use crate::{Error, config::TelemetryConfig};

pub fn init_tracer(config: &TelemetryConfig) -> Result<Option<SdkTracerProvider>, Error> {
    if !config.enabled {
        return Ok(None);
    }

    let endpoint = config
        .otlp_endpoint
        .clone()
        .unwrap_or_else(|| "http://localhost:4317".to_string());

    let resource = Resource::builder_empty()
        .with_service_name(config.service_name.clone())
        .build();

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| Error::Init(format!("failed to build OTLP exporter: {e}")))?;

    let provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::TraceIdRatioBased(config.sample_ratio))
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

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
