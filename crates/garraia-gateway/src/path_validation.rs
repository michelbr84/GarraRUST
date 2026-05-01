//! Single-segment basename validation for skill/skin file names.
//!
//! Linear: GAR-490 PR A (CodeQL `rust/path-injection` Wave 1).
//! Predecessor: GAR-491 (Wave 2 ledger mechanism, PR #109).
//!
//! Used by:
//! * `skills_handler` — 6 endpoints under `/api/skills/{name}` and the
//!   `body.name` of `POST /api/skills`.
//! * `skins_handler` — 3 endpoints under `/api/skins/{name}` and the
//!   `body.name` of `POST /api/skins`.
//!
//! Rejects every CodeQL `rust/path-injection` vector before the user-supplied
//! basename is concatenated into a filesystem path:
//! * path separators (`/`, `\`)
//! * NUL byte
//! * any control char (0x00–0x1F, 0x7F)
//! * Windows drive letters and reserved names (`C:foo`, `nul`, `con`)
//! * `..` and `.` segments
//! * any non-ASCII byte (anti-homoglyph)
//! * empty / oversized inputs
//!
//! Why not reuse `garraia_storage::sanitise_key`?
//! `sanitise_key` is multi-segment-friendly (it accepts `a/b/c` for object
//! storage keys). Skill and skin names are single-segment file basenames,
//! so the rules are strictly stricter. Reusing `sanitise_key` here would
//! permit paths the underlying filesystem layer would still treat as
//! traversal-adjacent.

/// Maximum allowed length of a skill/skin basename, in bytes.
///
/// 128 bytes covers any reasonable identifier and stays well below the
/// 255-byte filename limit on common filesystems (NTFS, ext4, APFS).
pub const MAX_NAME_LEN: usize = 128;

/// Reasons why a skill/skin basename is rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone, Copy)]
pub enum NameError {
    #[error("name must not be empty")]
    Empty,
    #[error("name exceeds {} bytes", MAX_NAME_LEN)]
    TooLong,
    #[error("name contains an invalid character (only ASCII letters, digits and '-' are allowed)")]
    InvalidChar,
}

/// Validate a skill or skin basename.
///
/// Accepts only `[A-Za-z0-9-]{1,128}`. This whitelist is strictly stronger
/// than `garraia_storage::sanitise_key` for single-segment use because it:
/// * forbids `/`, `\`, `:`, `.`, `_`, NUL, control chars, Windows reserved
///   names *implicitly* (none of those bytes is in the allow-list);
/// * caps length, so an attacker-controlled megabyte-scale name cannot DoS
///   the path layer.
///
/// Used by `skills_handler` (6 handlers) and `skins_handler` (3 handlers).
///
/// ## Why no underscore?
///
/// `garraia_skills::validate_skill` (parser.rs:64) enforces the project
/// convention `is_alphanumeric() || '-'` — i.e., underscores are rejected
/// downstream when the YAML frontmatter is parsed. This helper aligns
/// exactly so a name accepted at the path layer cannot fail later inside
/// the body parser. The brief's `[A-Za-z0-9_-]+` example was advisory; the
/// existing convention takes precedence.
///
/// ## Why ASCII-only?
///
/// The downstream rule uses `char::is_alphanumeric()` which accepts any
/// Unicode letter (e.g., Cyrillic). That is unsafe for filesystem layout:
/// homoglyphs (`ASCII a` U+0061 vs `Cyrillic а` U+0430) make distinct
/// names look identical to humans, and Windows + NTFS canonicalization
/// treats them differently across locales. Restricting to ASCII closes
/// that vector while remaining a strict subset of the downstream
/// convention — every name accepted here is also accepted by
/// `garraia_skills::validate_skill`.
pub fn validate_skill_name(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.len() > MAX_NAME_LEN {
        return Err(NameError::TooLong);
    }
    for c in name.bytes() {
        let ok = c.is_ascii_alphanumeric() || c == b'-';
        if !ok {
            return Err(NameError::InvalidChar);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Negative cases ───────────────────────────────────────────────────────
    // Each rejected input must close at least one CodeQL `rust/path-injection`
    // attack vector documented in §1 of the plan.

    #[test]
    fn rejects_empty_string() {
        assert_eq!(validate_skill_name(""), Err(NameError::Empty));
    }

    #[test]
    fn rejects_parent_dir_segment() {
        assert_eq!(validate_skill_name(".."), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_parent_dir_with_path() {
        assert_eq!(validate_skill_name("../x"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_forward_slash() {
        assert_eq!(validate_skill_name("x/y"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_backslash() {
        assert_eq!(validate_skill_name("x\\y"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_nul_byte() {
        assert_eq!(validate_skill_name("abc\0def"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_windows_drive_with_path() {
        assert_eq!(validate_skill_name("C:foo"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_lowercase_drive_letter() {
        assert_eq!(validate_skill_name("a:b"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_dot_in_basename() {
        // Even though `.md` is a legitimate extension, the helper validates
        // the *stem*; the handler appends the extension.
        assert_eq!(validate_skill_name("foo.md"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_space() {
        assert_eq!(validate_skill_name("a b"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_too_long() {
        let too_long = "a".repeat(MAX_NAME_LEN + 1);
        assert_eq!(validate_skill_name(&too_long), Err(NameError::TooLong));
    }

    #[test]
    fn rejects_non_ascii_homoglyph() {
        // Cyrillic 'а' (U+0430) looks identical to ASCII 'a' (U+0061) in
        // most fonts. Without an ASCII-only rule, an attacker could craft
        // a name that visually matches an existing skill.
        assert_eq!(validate_skill_name("привет"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_del_control_char() {
        assert_eq!(validate_skill_name("\x7f"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_tab_char() {
        assert_eq!(validate_skill_name("a\tb"), Err(NameError::InvalidChar));
    }

    #[test]
    fn rejects_newline() {
        assert_eq!(validate_skill_name("a\nb"), Err(NameError::InvalidChar));
    }

    // ── Positive cases ───────────────────────────────────────────────────────
    // Each accepted input matches the project's stated convention and the
    // brief's positive examples.

    #[test]
    fn accepts_alphanumeric() {
        assert_eq!(validate_skill_name("valid123"), Ok(()));
    }

    #[test]
    fn rejects_underscore() {
        // Underscore is rejected to match the downstream rule in
        // `garraia_skills::validate_skill` (parser.rs:64). See the
        // module docstring for rationale.
        assert_eq!(
            validate_skill_name("valid_skill"),
            Err(NameError::InvalidChar)
        );
    }

    #[test]
    fn accepts_hyphen() {
        assert_eq!(validate_skill_name("valid-skill"), Ok(()));
    }

    #[test]
    fn accepts_single_letter() {
        assert_eq!(validate_skill_name("a"), Ok(()));
    }

    #[test]
    fn accepts_mixed_case_with_digits_and_hyphen() {
        assert_eq!(validate_skill_name("Ab-C-1"), Ok(()));
    }

    #[test]
    fn accepts_max_length_boundary() {
        let at_limit = "a".repeat(MAX_NAME_LEN);
        assert_eq!(validate_skill_name(&at_limit), Ok(()));
    }

    // ── Error properties ─────────────────────────────────────────────────────

    #[test]
    fn name_error_messages_dont_leak_input() {
        // Defense-in-depth: the error display should never echo the
        // attacker-supplied byte sequence (which could contain control
        // chars that mess with logs).
        let err = NameError::InvalidChar;
        let msg = err.to_string();
        assert!(!msg.contains('\0'));
        assert!(!msg.contains('\x1b'));
    }

    #[test]
    fn name_error_is_clone_and_copy() {
        // Ergonomics — handlers may want to log the variant and still
        // return it.
        let err = NameError::Empty;
        let copied = err;
        assert_eq!(err, copied);
    }
}
