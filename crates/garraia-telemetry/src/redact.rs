//! PII redaction helpers for headers and structured fields.

/// Headers whose values must never appear in structured logs or trace
/// attributes.
///
/// Plan 0025 (GAR-411 M5): expanded to cover reverse-proxy variants.
/// nginx, AWS ALB, Traefik, and corporate SSO gateways commonly rewrite
/// the original `Authorization` into `X-Forwarded-Authorization` or
/// `X-Original-Authorization` before forwarding. Without redaction those
/// survive into tracing spans and Prometheus labels.
///
/// `X-Amzn-Trace-Id` is not credential data, but it leaks **infrastructure
/// topology**: the value embeds a creation timestamp (`1-{hex-seconds}-
/// {random}` format used by AWS X-Ray / ALB) and a sub-ID that correlates
/// back to load-balancer / backend identifiers. Stripping it keeps internal
/// topology out of third-party observability pipelines that the operator
/// does not control.
///
/// # Consumption (plan 0025 / security audit M-B)
///
/// This list is **pre-protective**, not reactive. `layers::http_trace_layer`
/// today uses `include_headers(false)` (see `layers.rs`), so request /
/// response headers never flow into spans — `REDACT_HEADERS` is currently
/// a dormant defense. It kicks in when a future caller (or a v2 of the
/// trace layer) sets `include_headers(true)` or captures headers via a
/// custom `MakeSpan`. Callers that *do* capture headers MUST route them
/// through [`redact_header_value`] before emitting.
///
/// # Known gaps (plan 0026+)
///
/// Cloud-LB IAP headers carry full JWT assertions and are NOT yet covered,
/// because the project does not declare support for those deploy targets.
/// When added, extend this list with: `x-goog-iap-jwt-assertion` (GCP IAP),
/// `cf-access-jwt-assertion` (Cloudflare Access), `x-ms-client-principal`
/// (Azure Front Door), and `x-forwarded-user` (generic SSO via oauth2-proxy
/// or nginx auth_request).
pub const REDACT_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "proxy-authorization",
    "x-forwarded-authorization",
    "x-original-authorization",
    "x-amzn-trace-id",
    // Plan 0026 (GAR-411 SA-L-A) — cloud-LB IAP variants.
    // These carry full JWT assertions or serialized identity tokens that
    // would leak into span attributes if `include_headers(true)` is ever
    // enabled on the trace layer.
    "x-goog-iap-jwt-assertion",
    "cf-access-jwt-assertion",
    "x-ms-client-principal",
    "x-forwarded-user",
];

pub fn redact_header_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if REDACT_HEADERS.contains(&lower.as_str()) {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

pub fn is_sensitive_field(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "jwt",
        "passphrase",
        "private_key",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_authorization_header_case_insensitive() {
        assert_eq!(
            redact_header_value("Authorization", "Bearer abc"),
            "[REDACTED]"
        );
        assert_eq!(redact_header_value("AUTHORIZATION", "x"), "[REDACTED]");
        assert_eq!(redact_header_value("authorization", "x"), "[REDACTED]");
        assert_eq!(redact_header_value("Cookie", "s=1"), "[REDACTED]");
    }

    #[test]
    fn strips_reverse_proxy_authorization_variants() {
        // Plan 0025 (GAR-411 M5). These headers are what nginx/ALB/Traefik
        // typically inject when they rewrite the original Authorization —
        // missing them here would leak JWT/session cookies to logs.
        for raw in [
            "X-Forwarded-Authorization",
            "x-forwarded-authorization",
            "X-FORWARDED-AUTHORIZATION",
            "X-Original-Authorization",
            "x-original-authorization",
            "X-Amzn-Trace-Id",
            "x-amzn-trace-id",
        ] {
            assert_eq!(
                redact_header_value(raw, "Bearer leaked-token"),
                "[REDACTED]",
                "header {raw} must redact"
            );
        }
    }

    #[test]
    fn strips_cloud_iap_jwt_headers() {
        // Plan 0026 (GAR-411 SA-L-A). These headers carry full JWT
        // assertions or serialized identity tokens when the gateway sits
        // behind GCP IAP, Cloudflare Access, Azure Front Door, or a
        // generic oauth2-proxy / nginx auth_request fronting SSO.
        for raw in [
            "X-Goog-IAP-JWT-Assertion",
            "x-goog-iap-jwt-assertion",
            "CF-Access-JWT-Assertion",
            "cf-access-jwt-assertion",
            "X-Ms-Client-Principal",
            "x-ms-client-principal",
            "X-Forwarded-User",
            "x-forwarded-user",
        ] {
            assert_eq!(
                redact_header_value(raw, "eyJhbGciOi...leaked"),
                "[REDACTED]",
                "cloud IAP header {raw} must redact"
            );
        }
    }

    #[test]
    fn passes_innocent_headers() {
        assert_eq!(
            redact_header_value("content-type", "application/json"),
            "application/json"
        );
    }

    #[test]
    fn is_sensitive_field_detects_variants() {
        for v in [
            "password",
            "user_password",
            "secret",
            "api_secret",
            "token",
            "access_token",
            "api_key",
            "apikey",
            "X-ApiKey",
            "jwt",
            "jwt_secret",
            "passphrase",
            "private_key",
        ] {
            assert!(is_sensitive_field(v), "expected sensitive: {v}");
        }
        assert!(!is_sensitive_field("username"));
        assert!(!is_sensitive_field("email"));
    }
}
