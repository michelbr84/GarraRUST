//! `RedactedStorageError` — newtype wrapper around `sqlx::Error` whose
//! `Display` and `Debug` impls strip credentials that `sqlx` may embed in
//! connection error messages.
//!
//! ## Background
//!
//! Some `sqlx::Error` variants — notably `Io` and `Configuration` originating
//! from `PgConnectOptions` — embed the connection string (with password)
//! inside their error message. Logging such errors at `{:?}` or even `{}`
//! can leak credentials. This module provides a redacting wrapper so the
//! gateway can log the error via `Display` and know for certain that no
//! `postgres://user:pass@host/db` URL, bare `host=...` segment, or
//! `password=...` segment survives.
//!
//! The redaction is string-based (no regex dependency) and targets the three
//! shapes that show up in practice:
//!
//! - URL form:   `postgres://user:pass@host:5432/db`
//! - Key/value:  `host=db.example.com`
//! - Key/value:  `password=hunter2`
//!
//! Chain printing (`source()`) is preserved so downstream error formatters
//! that follow the chain still get the original `sqlx::Error` — BUT such
//! formatters MUST use `Display` (not `Debug`) to avoid leaking via the
//! inner `{:?}` of `sqlx::Error`. The `Debug` impl of `RedactedStorageError`
//! itself performs the same redaction as `Display`.

use std::error::Error;
use std::fmt;

/// Newtype wrapper around `sqlx::Error` with credential-redacting `Display`
/// and `Debug`. See module docs for the redaction rules.
pub struct RedactedStorageError(sqlx::Error);

impl RedactedStorageError {
    /// Borrow the inner `sqlx::Error`. Callers MUST redact before logging.
    pub fn inner(&self) -> &sqlx::Error {
        &self.0
    }
}

impl From<sqlx::Error> for RedactedStorageError {
    fn from(err: sqlx::Error) -> Self {
        Self(err)
    }
}

/// Apply all redaction rules to a raw error string. Exposed (crate-private)
/// for direct unit testing without having to construct synthetic `sqlx::Error`
/// values.
pub(crate) fn redact(raw: &str) -> String {
    let mut out = redact_urls(raw);
    out = redact_key_value(&out, "host=");
    out = redact_key_value(&out, "password=");
    // GAR-391c security review M-2: also redact `user=` segments. The
    // username itself is not strictly secret, but it confirms the role
    // (e.g., `user=garraia_login` reveals architecture). Minimal-exposure
    // principle: redact it too. Tests cover all 3 keys.
    out = redact_key_value(&out, "user=");
    out
}

/// Replace any `postgres://...@...` or `postgresql://...@...` substring with
/// a fixed `postgres://[REDACTED]@[REDACTED]` placeholder. The redaction
/// consumes characters up to the next whitespace, quote, or delimiter so
/// that the surrounding message remains readable.
fn redact_urls(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &input[i..];
        let matched = if rest.starts_with("postgres://") {
            Some("postgres://".len())
        } else if rest.starts_with("postgresql://") {
            Some("postgresql://".len())
        } else {
            None
        };

        if let Some(prefix_len) = matched {
            // Find end of the URL: first whitespace, quote, or common
            // delimiter that would reasonably terminate a URL in a message.
            let tail = &rest[prefix_len..];
            let end = tail
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | ';' | ')' | ']')
                })
                .unwrap_or(tail.len());
            out.push_str("postgres://[REDACTED]@[REDACTED]");
            i += prefix_len + end;
        } else {
            // Copy one character. The `unwrap_or` is defensive: the loop
            // condition `i < bytes.len()` already guarantees `rest` is
            // non-empty, so `.next()` returns `Some` in practice. Code
            // review 391c #D: replaced `.unwrap()` with `.unwrap_or('\0')`
            // to keep `unwrap()` strictly out of production paths per
            // CLAUDE.md rule 4.
            let ch = rest.chars().next().unwrap_or('\0');
            if ch == '\0' {
                break;
            }
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Replace any `<key>=VALUE` segment with `<key>=[REDACTED]` where VALUE
/// runs until the next whitespace, quote, or common delimiter.
fn redact_key_value(input: &str, key: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut remaining = input;
    while let Some(idx) = remaining.find(key) {
        out.push_str(&remaining[..idx]);
        out.push_str(key);
        out.push_str("[REDACTED]");
        let after = &remaining[idx + key.len()..];
        let end = after
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | ';' | ')' | ']')
            })
            .unwrap_or(after.len());
        remaining = &after[end..];
    }
    out.push_str(remaining);
    out
}

impl fmt::Display for RedactedStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let raw = format!("{}", self.0);
        f.write_str(&redact(&raw))
    }
}

impl fmt::Debug for RedactedStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // NEVER `{:?}` the inner sqlx::Error directly — some variants embed
        // the connection URL in their Debug output. Use the Display-based
        // redacted text instead.
        let raw = format!("{}", self.0);
        f.debug_tuple("RedactedStorageError")
            .field(&redact(&raw))
            .finish()
    }
}

impl Error for RedactedStorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_postgres_url() {
        let raw = "connect failed: postgres://alice:hunter2@db.example.com:5432/app oops";
        let out = redact(raw);
        assert!(!out.contains("alice"), "leaked user: {out}");
        assert!(!out.contains("hunter2"), "leaked password: {out}");
        assert!(!out.contains("db.example.com"), "leaked host: {out}");
        assert!(out.contains("postgres://[REDACTED]@[REDACTED]"));
        assert!(out.contains("oops"), "tail preserved: {out}");
    }

    #[test]
    fn redacts_postgresql_scheme() {
        let raw = "postgresql://bob:sekret@h:5432/db";
        let out = redact(raw);
        assert!(!out.contains("bob"));
        assert!(!out.contains("sekret"));
        assert!(out.contains("postgres://[REDACTED]@[REDACTED]"));
    }

    #[test]
    fn redacts_host_kv_segment() {
        let raw = "io error: host=db.internal port=5432 user=app";
        let out = redact(raw);
        assert!(!out.contains("db.internal"), "leaked host: {out}");
        assert!(out.contains("host=[REDACTED]"));
        // M-2 (391c): user= is also redacted now.
        assert!(!out.contains("user=app"), "leaked user: {out}");
        assert!(out.contains("user=[REDACTED]"));
        // non-sensitive key/values remain
        assert!(out.contains("port=5432"));
    }

    #[test]
    fn redacts_password_kv_segment() {
        let raw = "configuration error: user=app password=hunter2 dbname=app";
        let out = redact(raw);
        assert!(!out.contains("hunter2"), "leaked password: {out}");
        assert!(out.contains("password=[REDACTED]"));
        // M-2 (391c): user= is now redacted as well.
        assert!(!out.contains("user=app"), "leaked user: {out}");
        assert!(out.contains("user=[REDACTED]"));
    }

    #[test]
    fn redacts_multiple_segments() {
        let raw = "error: postgres://u:p@h/db host=other password=xyz";
        let out = redact(raw);
        assert!(!out.contains("u:p"));
        assert!(!out.contains("other"));
        assert!(!out.contains("xyz"));
        assert!(out.contains("postgres://[REDACTED]@[REDACTED]"));
        assert!(out.contains("host=[REDACTED]"));
        assert!(out.contains("password=[REDACTED]"));
    }

    #[test]
    fn passthrough_when_nothing_sensitive() {
        let raw = "connection timed out after 30s";
        assert_eq!(redact(raw), raw);
    }

    #[test]
    fn from_sqlx_error_wraps_and_display_redacts() {
        // Use a synthetic sqlx::Error::Protocol (carries an arbitrary String
        // via Display) so we can test the full newtype path end-to-end.
        let raw = "protocol error near postgres://u:p@h:5432/db while handshaking".to_string();
        let err = sqlx::Error::Protocol(raw);
        let wrapped: RedactedStorageError = err.into();
        let disp = format!("{wrapped}");
        assert!(!disp.contains("u:p"), "Display leaked: {disp}");
        assert!(disp.contains("postgres://[REDACTED]@[REDACTED]"));
        let dbg = format!("{wrapped:?}");
        assert!(!dbg.contains("u:p"), "Debug leaked: {dbg}");
        assert!(dbg.contains("postgres://[REDACTED]@[REDACTED]"));
    }

    #[test]
    fn source_chain_is_present() {
        let err = sqlx::Error::Protocol("x".to_string());
        let wrapped: RedactedStorageError = err.into();
        assert!(wrapped.source().is_some());
    }
}
