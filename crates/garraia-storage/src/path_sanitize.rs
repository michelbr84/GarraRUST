//! Central path-sanitization policy for all storage backends.
//!
//! Every object key passing through any [`crate::ObjectStore`] implementation
//! MUST be validated via [`sanitise_key`] first. Centralising the rules here
//! prevents drift between backends (LocalFs / S3 / MinIO).

use thiserror::Error;

use crate::error::StorageError;

/// Reason the key failed validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SanitizeError {
    #[error("key is empty")]
    Empty,
    #[error("key contains a NUL byte")]
    ContainsNul,
    #[error("key contains a parent-directory segment (`..`)")]
    ParentSegment,
    #[error("key contains a current-directory segment (`.`)")]
    CurrentSegment,
    #[error("key contains an empty segment (e.g. `a//b` or trailing `/`)")]
    EmptySegment,
    #[error("key starts with a path separator — absolute paths are rejected")]
    Absolute,
    #[error("key uses a backslash; forward-slash-only is required")]
    Backslash,
    #[error("key starts with a Windows drive letter (`C:/` or `C:foo`)")]
    WindowsDrive,
    #[error("key contains a reserved Windows name (`{0}`)")]
    ReservedWindowsName(&'static str),
    #[error("key exceeds {max} bytes (got {got})")]
    TooLong { got: usize, max: usize },
    #[error("key contains a control character (0x{byte:02x})")]
    ControlChar { byte: u8 },
}

impl From<SanitizeError> for StorageError {
    fn from(e: SanitizeError) -> Self {
        StorageError::InvalidKey(e.to_string())
    }
}

/// Per-segment maximum length (a single component between `/`). 1024 is a
/// conservative upper bound that most POSIX and NTFS setups accept.
pub const MAX_KEY_BYTES: usize = 1024;

/// Names reserved by Windows even on case-insensitive comparison; rejected
/// for portability between LocalFs on Linux dev workstations and Windows CI.
const RESERVED_WINDOWS_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate an object key. Returns the trimmed key on success.
///
/// # Policy
///
/// - Must be non-empty.
/// - Must be ≤ [`MAX_KEY_BYTES`] bytes long.
/// - Must not contain NUL (`0x00`) nor control characters (< 0x20, != `\t`).
/// - Must not contain `..` as any segment (strict — even inside like `a..b` is allowed; only `..` as full segment is rejected).
/// - Must not start with `/` (no absolute paths).
/// - Must not contain `\` (backslash) — forward-slash only, for cross-platform portability.
/// - Must not start with a Windows drive letter (`C:/`, `D:\`).
/// - Each segment must not match a reserved Windows name (case-insensitive).
pub fn sanitise_key(key: &str) -> Result<&str, SanitizeError> {
    if key.is_empty() {
        return Err(SanitizeError::Empty);
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(SanitizeError::TooLong {
            got: key.len(),
            max: MAX_KEY_BYTES,
        });
    }
    if key.as_bytes().contains(&0u8) {
        return Err(SanitizeError::ContainsNul);
    }
    for &b in key.as_bytes() {
        if (b < 0x20 && b != b'\t') || b == 0x7f {
            return Err(SanitizeError::ControlChar { byte: b });
        }
    }
    if key.contains('\\') {
        return Err(SanitizeError::Backslash);
    }
    if key.starts_with('/') {
        return Err(SanitizeError::Absolute);
    }
    // Windows drive letters: `C:/`, `Z:/`, or `C:foo` (drive-relative, which
    // resolves against the per-drive current working directory on Windows).
    // SEC-F-02 (plan 0037 audit): even without a separator, `C:` prefix must
    // be rejected because Windows interprets it as a drive-scoped path.
    if key.len() >= 2 {
        let bytes = key.as_bytes();
        if bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(SanitizeError::WindowsDrive);
        }
    }
    for seg in key.split('/') {
        if seg.is_empty() {
            // SEC-F-03 (plan 0037 audit): rejects `a//b`, trailing `/`, and `/a`.
            return Err(SanitizeError::EmptySegment);
        }
        if seg == ".." {
            return Err(SanitizeError::ParentSegment);
        }
        if seg == "." {
            // SEC-F-03: rejects `./foo` and `foo/.` — equivalent but
            // distinct-looking keys confuse downstream caches.
            return Err(SanitizeError::CurrentSegment);
        }
        // Strip extension for reserved-name comparison: CON.txt is also reserved on Windows.
        let base = seg.split('.').next().unwrap_or(seg);
        for reserved in RESERVED_WINDOWS_NAMES {
            if base.eq_ignore_ascii_case(reserved) {
                return Err(SanitizeError::ReservedWindowsName(reserved));
            }
        }
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical_keys() {
        for k in [
            "group-abc/file-123/v1",
            "a/b/c/d/e.txt",
            "single",
            "nested/with.dots",
            "a..b", // `..` inside a segment (not a full segment) — allowed
            "foo/a-b-c.pdf",
        ] {
            assert_eq!(sanitise_key(k), Ok(k), "should accept `{k}`");
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(sanitise_key(""), Err(SanitizeError::Empty));
    }

    #[test]
    fn rejects_parent_segment() {
        for k in ["../secret", "a/../b", "foo/bar/..", ".."] {
            assert_eq!(
                sanitise_key(k),
                Err(SanitizeError::ParentSegment),
                "key `{k}` should be rejected"
            );
        }
    }

    #[test]
    fn rejects_absolute_paths() {
        assert_eq!(sanitise_key("/etc/passwd"), Err(SanitizeError::Absolute));
        assert_eq!(sanitise_key("/"), Err(SanitizeError::Absolute));
    }

    #[test]
    fn rejects_backslash() {
        assert_eq!(sanitise_key("a\\b"), Err(SanitizeError::Backslash));
    }

    #[test]
    fn rejects_windows_drive_letter() {
        assert_eq!(sanitise_key("C:/foo"), Err(SanitizeError::WindowsDrive));
        assert_eq!(sanitise_key("z:/foo"), Err(SanitizeError::WindowsDrive));
    }

    #[test]
    fn rejects_nul_byte() {
        let k = "a\0b";
        assert_eq!(sanitise_key(k), Err(SanitizeError::ContainsNul));
    }

    #[test]
    fn rejects_control_chars() {
        // Newline 0x0A
        let k = "line1\nline2";
        assert!(matches!(
            sanitise_key(k),
            Err(SanitizeError::ControlChar { byte: 0x0a })
        ));
        // DEL 0x7f
        let k = "a\x7fb";
        assert!(matches!(
            sanitise_key(k),
            Err(SanitizeError::ControlChar { byte: 0x7f })
        ));
    }

    #[test]
    fn rejects_drive_relative_without_separator() {
        // SEC-F-02: `C:foo` on Windows is drive-current-dir relative.
        assert_eq!(sanitise_key("C:foo"), Err(SanitizeError::WindowsDrive));
        assert_eq!(sanitise_key("z:file"), Err(SanitizeError::WindowsDrive));
        // But `a:b` is accepted if first char is not alphabetic? First check is
        // `is_ascii_alphabetic`, so `1:b` would pass — that's fine because `1:`
        // is not a valid Windows drive.
        // `a:` alone (2 bytes) is rejected.
        assert_eq!(sanitise_key("a:"), Err(SanitizeError::WindowsDrive));
    }

    #[test]
    fn rejects_empty_segment() {
        // SEC-F-03: trailing slash, consecutive slashes.
        for k in ["a/", "a//b", "foo///bar"] {
            assert_eq!(
                sanitise_key(k),
                Err(SanitizeError::EmptySegment),
                "key `{k}` should be rejected"
            );
        }
    }

    #[test]
    fn rejects_current_segment() {
        for k in ["./foo", "foo/.", "foo/./bar", "."] {
            assert_eq!(
                sanitise_key(k),
                Err(SanitizeError::CurrentSegment),
                "key `{k}` should be rejected"
            );
        }
    }

    #[test]
    fn rejects_reserved_windows_names() {
        for k in ["CON", "con.txt", "COM1", "prn/file", "foo/NUL", "lpt9.bin"] {
            assert!(
                matches!(sanitise_key(k), Err(SanitizeError::ReservedWindowsName(_))),
                "key `{k}` should be rejected"
            );
        }
    }

    #[test]
    fn rejects_too_long() {
        let k = "a".repeat(MAX_KEY_BYTES + 1);
        match sanitise_key(&k) {
            Err(SanitizeError::TooLong { got, max }) => {
                assert_eq!(got, MAX_KEY_BYTES + 1);
                assert_eq!(max, MAX_KEY_BYTES);
            }
            other => panic!("expected TooLong, got {other:?}"),
        }
    }
}
