//! Canonical content-type allow-list for object uploads.
//!
//! Plan 0038 §3 + ADR 0004 §Security 3 mandate a fail-closed allow-list for
//! `put` operations — any type outside this list is rejected unless the
//! caller sets `PutOptions::allow_unsafe_mime = true` (which should emit an
//! audit event at the caller layer).

/// Exact-match and wildcard rules for the default allow-list.
///
/// Entries ending in `/*` match any subtype; exact strings match verbatim.
/// The matcher is **case-insensitive on the top-level type** (MIME types
/// are case-insensitive per RFC 6838 §4.2) and strips any `;parameter`
/// suffix before matching.
pub const DEFAULT_ALLOWED: &[&str] = &[
    // image/*: png, jpeg, webp, gif, svg+xml
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/svg+xml",
    // documents
    "application/pdf",
    "application/json",
    "application/zip",
    // video / audio
    "video/mp4",
    "audio/mpeg",
    "audio/ogg",
    "audio/wav",
    // text/* core
    "text/plain",
    "text/csv",
    "text/markdown",
    // Office Open XML family (docx, xlsx, pptx, …)
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
];

/// Return true when `content_type` is allowed by the default list.
///
/// The match strips any `;parameter` suffix (e.g. `text/csv; charset=utf-8`
/// matches `text/csv`) and lowercases the input before comparison.
pub fn is_mime_allowed(content_type: &str) -> bool {
    is_allowed_against(content_type, DEFAULT_ALLOWED)
}

/// Match `content_type` against an arbitrary allow-list. Useful for tests
/// and for slice 3 when `garraia-config::storage.allowed_mime_types`
/// supplies an override.
pub fn is_allowed_against(content_type: &str, allowed: &[&str]) -> bool {
    let normalized = normalize(content_type);
    allowed.iter().any(|entry| matches_rule(&normalized, entry))
}

fn normalize(ct: &str) -> String {
    ct.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn matches_rule(normalized: &str, rule: &str) -> bool {
    let rule = rule.to_ascii_lowercase();
    if let Some(prefix) = rule.strip_suffix("/*") {
        // Prefix match on the type portion.
        normalized
            .split_once('/')
            .map(|(top, _)| top == prefix)
            .unwrap_or(false)
    } else {
        normalized == rule
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_common_image_types() {
        for ct in [
            "image/png",
            "image/jpeg",
            "image/webp",
            "image/gif",
            "image/svg+xml",
        ] {
            assert!(is_mime_allowed(ct), "{ct} should be allowed");
        }
    }

    #[test]
    fn allows_docs_and_archives() {
        for ct in ["application/pdf", "application/json", "application/zip"] {
            assert!(is_mime_allowed(ct), "{ct} should be allowed");
        }
    }

    #[test]
    fn allows_text_family_with_charset_parameter() {
        assert!(is_mime_allowed("text/plain; charset=utf-8"));
        assert!(is_mime_allowed("text/csv ; charset=utf-8"));
        assert!(is_mime_allowed("text/markdown"));
    }

    #[test]
    fn allows_office_openxml() {
        assert!(is_mime_allowed(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
    }

    #[test]
    fn case_insensitive_match() {
        assert!(is_mime_allowed("IMAGE/PNG"));
        assert!(is_mime_allowed("Image/Jpeg"));
    }

    #[test]
    fn rejects_dangerous_types() {
        for ct in [
            "application/x-msdownload",
            "application/x-sh",
            "application/octet-stream",
            "text/html",
            "application/x-httpd-php",
        ] {
            assert!(!is_mime_allowed(ct), "{ct} should be rejected");
        }
    }

    #[test]
    fn rejects_empty_string() {
        assert!(!is_mime_allowed(""));
    }

    #[test]
    fn custom_allowlist_overrides_default() {
        assert!(is_allowed_against(
            "application/x-tar",
            &["application/x-tar"]
        ));
        assert!(!is_allowed_against("image/png", &["application/pdf"]));
    }

    #[test]
    fn wildcard_rule_matches_subtypes() {
        assert!(is_allowed_against("image/heic", &["image/*"]));
        assert!(!is_allowed_against("video/mp4", &["image/*"]));
    }
}
