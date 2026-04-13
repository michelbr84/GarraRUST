//! PII redaction helpers for headers and structured fields.

pub const REDACT_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "proxy-authorization",
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
        assert_eq!(redact_header_value("Authorization", "Bearer abc"), "[REDACTED]");
        assert_eq!(redact_header_value("AUTHORIZATION", "x"), "[REDACTED]");
        assert_eq!(redact_header_value("authorization", "x"), "[REDACTED]");
        assert_eq!(redact_header_value("Cookie", "s=1"), "[REDACTED]");
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
