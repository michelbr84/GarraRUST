//! In-process smoke tests for the telemetry pipeline.
//!
//! These tests deliberately avoid any network I/O. Strategy A uses
//! `InMemorySpanExporter` from `opentelemetry_sdk`'s `testing` module
//! (enabled via the `testing` feature in dev-dependencies) to verify that
//! a span produced against a `TracerProvider` is captured end-to-end.

use opentelemetry::trace::{Span as _, Tracer, TracerProvider as _};
use opentelemetry_sdk::testing::trace::InMemorySpanExporter;
use opentelemetry_sdk::trace::TracerProvider;

#[test]
fn smoke_in_memory_exporter_captures_span() {
    let exporter = InMemorySpanExporter::default();
    let provider = TracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();

    let tracer = provider.tracer("garraia-test");
    let mut span = tracer.start("test.span");
    span.end();

    // SimpleSpanProcessor exports on span end, but force_flush is safe and
    // cheap. We must read spans BEFORE `provider.shutdown()` because the
    // in-memory exporter clears its backing store on shutdown.
    for result in provider.force_flush() {
        result.expect("force_flush must succeed for simple exporter");
    }

    let spans = exporter
        .get_finished_spans()
        .expect("in-memory exporter must return finished spans");
    assert!(!spans.is_empty(), "expected at least one finished span");
    assert!(
        spans.iter().any(|s| s.name == "test.span"),
        "expected to find 'test.span' in captured spans, got: {:?}",
        spans.iter().map(|s| s.name.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn disabled_init_is_noop() {
    // Plan 0026 (GAR-411 M3): `init()` no longer returns `Result` — any
    // failure mode is logged internally and converted to an empty guard.
    let guard = garraia_telemetry::init(garraia_telemetry::TelemetryConfig::default());
    drop(guard);
}
