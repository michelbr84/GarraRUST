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
