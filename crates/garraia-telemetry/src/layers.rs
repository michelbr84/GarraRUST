//! Axum/tower-http integration layers.

use axum::http::HeaderName;
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultMakeSpan, TraceLayer},
};

// v1: headers fully excluded from spans for PII safety. v2 can selectively
// include headers with redaction via `redact::redact_header_value`.
pub fn http_trace_layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, DefaultMakeSpan> {
    TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::new().include_headers(false))
}

pub fn request_id_layer() -> SetRequestIdLayer<MakeRequestUuid> {
    SetRequestIdLayer::new(HeaderName::from_static("x-request-id"), MakeRequestUuid)
}

pub fn propagate_request_id_layer() -> PropagateRequestIdLayer {
    PropagateRequestIdLayer::new(HeaderName::from_static("x-request-id"))
}
