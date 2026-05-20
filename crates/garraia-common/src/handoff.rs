/// Auto Dream / handoff state persisted to `.garra-estado.md` (TOML).
///
/// The file uses a `.md` extension for human discoverability but its content is
/// TOML, loaded and saved by this module.  The schema contains only allow-listed
/// fields — no free-form message bodies and no user PII.
///
/// The [`RedactedString`] newtype enforces that any human-supplied description
/// passes through [`redact`] before it can be stored, providing both compile-time
/// proof and runtime guarantee that the field is scrubbed.
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced by [`load`] and [`save`].
#[derive(Debug, Error)]
pub enum HandoffError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Decode(#[from] toml::de::Error),
    #[error("TOML encode error: {0}")]
    Encode(#[from] toml::ser::Error),
}

/// A string that has been through [`redact`].
///
/// Can only be constructed via [`RedactedString::new`], which applies redaction
/// automatically.  You cannot assign a raw `String` or `&str` to this field
/// without explicitly calling `new()`, which makes it clear redaction is
/// happening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct RedactedString(String);

impl RedactedString {
    /// Apply [`redact`] to `raw` and wrap the result.
    pub fn new(raw: &str) -> Self {
        Self(redact(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<RedactedString> for String {
    fn from(r: RedactedString) -> Self {
        r.0
    }
}

/// Scrubs sensitive patterns from a string before storing.
///
/// Rules applied in order:
/// 1. Replace email-shaped tokens with `<email>`.
/// 2. Replace JWT-shaped tokens (three base64url segments) with `<token>`.
/// 3. Replace Unix home paths (`/home/…` or `~`) with `<path>`.
/// 4. Truncate to at most 500 characters (appending `…` if cut).
pub fn redact(s: &str) -> String {
    let s = redact_emails(s);
    let s = redact_jwts(&s);
    let s = redact_unix_paths(&s);
    truncate_str(&s, 500)
}

fn redact_emails(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        // Detect a plausible email: non-space run containing '@' with a '.' after.
        if chars[i] != ' ' && chars[i] != '\n' && chars[i] != '\t' {
            let start = i;
            while i < n && chars[i] != ' ' && chars[i] != '\n' && chars[i] != '\t' {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if let Some(at) = token.find('@') {
                let after = &token[at + 1..];
                if after.contains('.') && !after.starts_with('.') {
                    out.push_str("<email>");
                    continue;
                }
            }
            out.push_str(&token);
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn redact_jwts(s: &str) -> String {
    // JWT pattern: three base64url-safe segments separated by dots, each ≥4 chars.
    let mut result = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        let parts: Vec<&str> = word.splitn(4, '.').collect();
        if parts.len() >= 3
            && parts[0].len() >= 4
            && parts[1].len() >= 4
            && parts[2].len() >= 4
            && parts.iter().take(3).all(|p| {
                p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '=')
            })
        {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str("<token>");
        } else {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(word);
        }
    }
    // Preserve leading/trailing whitespace structure (just do a simple replace for
    // the common inline case).
    result
}

fn redact_unix_paths(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        let is_home_path = s[i..].starts_with("/home/");
        let is_tilde = bytes[i] == b'~' && (i + 1 >= n || bytes[i + 1] == b'/');
        if is_home_path || is_tilde {
            out.push_str("<path>");
            while i < n && bytes[i] != b' ' && bytes[i] != b'\n' && bytes[i] != b'\t' {
                i += 1;
            }
        } else {
            // Consume one char at a time for correct UTF-8 handling.
            let ch = s[i..].chars().next().unwrap_or('\0');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max_chars).collect();
        t.push('…');
        t
    }
}

/// Which pipeline stage was last active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HandoffActionKind {
    #[default]
    Other,
    Brainstorm,
    Spec,
    Plan,
    Implement,
    Review,
    Verify,
    Merge,
}

/// A single pipeline action entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffAction {
    pub kind: HandoffActionKind,
    /// Short, redacted description of what was done / what to do next.
    pub description: RedactedString,
    /// ISO 8601 UTC timestamp.
    pub timestamp: Option<String>,
}

impl HandoffAction {
    pub fn new(kind: HandoffActionKind, description: &str) -> Self {
        Self {
            kind,
            description: RedactedString::new(description),
            timestamp: None,
        }
    }

    pub fn with_timestamp(mut self, ts: &str) -> Self {
        self.timestamp = Some(ts.to_owned());
        self
    }
}

/// Current schema version — bump when fields are added/removed.
pub const SCHEMA_VERSION: u32 = 1;

/// Serialisable handoff state written to `.garra-estado.md`.
///
/// All fields are optional except `schema_version` so that a future reader can
/// gracefully handle a partial file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffState {
    pub schema_version: u32,
    pub branch: Option<String>,
    pub linear_issue: Option<String>,
    pub current_plan: Option<String>,
    pub last_action: Option<HandoffAction>,
    pub next_action: Option<HandoffAction>,
    /// ISO 8601 UTC.
    pub updated_at: Option<String>,
}

impl Default for HandoffState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            branch: None,
            linear_issue: None,
            current_plan: None,
            last_action: None,
            next_action: None,
            updated_at: None,
        }
    }
}

impl HandoffState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a one-line summary for display at session start.
    pub fn summary(&self) -> String {
        let last = self
            .last_action
            .as_ref()
            .map(|a| format!("{:?}: {}", a.kind, a.description.as_str()))
            .unwrap_or_else(|| "none".to_owned());
        let next = self
            .next_action
            .as_ref()
            .map(|a| format!("{:?}: {}", a.kind, a.description.as_str()))
            .unwrap_or_else(|| "none".to_owned());
        format!("Última ação: {last} | Próxima: {next}")
    }
}

/// Load [`HandoffState`] from `path`.
///
/// Returns `Err` if the file exists but cannot be parsed; returns
/// `Ok(HandoffState::default())` if the file does not exist.
pub fn load(path: &Path) -> Result<HandoffState, HandoffError> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let state: HandoffState = toml::from_str(&content)?;
            Ok(state)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HandoffState::default()),
        Err(e) => Err(HandoffError::Io(e)),
    }
}

/// Save [`HandoffState`] to `path` atomically (write to `.tmp`, then rename).
pub fn save(state: &HandoffState, path: &Path) -> Result<(), HandoffError> {
    let content = toml::to_string_pretty(state)?;
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Helper: create a tmp dir for file I/O tests.
    fn tmp_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    // ── redact() ──────────────────────────────────────────────────────────────

    #[test]
    fn redact_leaves_plain_text_untouched() {
        assert_eq!(
            redact("implement the auth module"),
            "implement the auth module"
        );
    }

    #[test]
    fn redact_strips_email() {
        let r = redact("contact user@example.com for details");
        assert!(!r.contains('@'), "email should be replaced: {r}");
        assert!(r.contains("<email>"), "placeholder missing: {r}");
    }

    #[test]
    fn redact_strips_jwt_shaped_token() {
        // Three base64url-safe segments ≥4 chars each — clearly a test fixture.
        let jwt_like = "aaaa1111.bbbb2222.cccc3333";
        let r = redact(jwt_like);
        assert_eq!(r, "<token>");
    }

    #[test]
    fn redact_strips_unix_home_path() {
        let r = redact("config at /home/alice/.garraia/config.toml");
        assert!(!r.contains("/home/"), "path not redacted: {r}");
        assert!(r.contains("<path>"), "placeholder missing: {r}");
    }

    #[test]
    fn redact_strips_tilde_path() {
        let r = redact("see ~/config");
        assert!(!r.contains("~/"), "tilde path not redacted: {r}");
        assert!(r.contains("<path>"), "placeholder missing: {r}");
    }

    #[test]
    fn redact_truncates_long_strings() {
        let long: String = "x".repeat(600);
        let r = redact(&long);
        // 500 chars + ellipsis
        assert!(r.chars().count() <= 502, "too long after truncation");
        assert!(r.ends_with('…'), "truncation marker missing");
    }

    #[test]
    fn redact_preserves_short_ascii() {
        let s = "fix login crash in auth module (GAR-500)";
        assert_eq!(redact(s), s);
    }

    // ── RedactedString ────────────────────────────────────────────────────────

    #[test]
    fn redacted_string_new_applies_redaction() {
        let rs = RedactedString::new("user@example.com sent a request");
        assert!(!rs.as_str().contains('@'));
    }

    #[test]
    fn redacted_string_roundtrip_serde() {
        let rs = RedactedString::new("plain text");
        let json = serde_json::to_string(&rs).unwrap();
        let back: RedactedString = serde_json::from_str(&json).unwrap();
        assert_eq!(rs, back);
    }

    // ── HandoffState ──────────────────────────────────────────────────────────

    #[test]
    fn default_state_has_schema_version_1() {
        let s = HandoffState::default();
        assert_eq!(s.schema_version, 1);
        assert!(s.branch.is_none());
        assert!(s.last_action.is_none());
    }

    #[test]
    fn summary_with_no_actions_says_none() {
        let s = HandoffState::default();
        let sum = s.summary();
        assert!(sum.contains("none"), "expected 'none' in summary: {sum}");
    }

    #[test]
    fn summary_shows_last_and_next() {
        let s = HandoffState {
            last_action: Some(HandoffAction::new(
                HandoffActionKind::Plan,
                "wrote plan 0157",
            )),
            next_action: Some(HandoffAction::new(
                HandoffActionKind::Implement,
                "implement handoff module",
            )),
            ..HandoffState::default()
        };
        let sum = s.summary();
        assert!(sum.contains("Plan"), "kind missing: {sum}");
        assert!(sum.contains("Implement"), "next kind missing: {sum}");
    }

    // ── load / save ───────────────────────────────────────────────────────────

    #[test]
    fn load_returns_default_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let path = tmp_path(&dir, ".garra-estado.md");
        let state = load(&path).unwrap();
        assert_eq!(state, HandoffState::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = tmp_path(&dir, ".garra-estado.md");

        let mut original = HandoffState::new();
        original.branch = Some("routine/test".to_owned());
        original.linear_issue = Some("GAR-500".to_owned());
        original.current_plan = Some("plans/0157-gar-500-auto-dream-handoff.md".to_owned());
        original.last_action = Some(HandoffAction::new(
            HandoffActionKind::Plan,
            "wrote plan 0157",
        ));
        original.next_action = Some(HandoffAction::new(
            HandoffActionKind::Implement,
            "implement the module",
        ));

        save(&original, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(original, loaded);
    }

    #[test]
    fn save_creates_file_and_load_parses_it() {
        let dir = TempDir::new().unwrap();
        let path = tmp_path(&dir, ".garra-estado.md");

        let state = HandoffState {
            schema_version: 1,
            branch: Some("feat/test".to_owned()),
            linear_issue: Some("GAR-1".to_owned()),
            current_plan: None,
            last_action: None,
            next_action: None,
            updated_at: Some("2026-05-20T06:00:00Z".to_owned()),
        };

        save(&state, &path).unwrap();
        assert!(path.exists(), "file should be created by save");

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.branch.as_deref(), Some("feat/test"));
        assert_eq!(loaded.updated_at.as_deref(), Some("2026-05-20T06:00:00Z"));
    }

    #[test]
    fn load_returns_error_on_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let path = tmp_path(&dir, ".garra-estado.md");
        std::fs::write(&path, "[[not valid toml}}}").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn handoff_action_with_timestamp() {
        let action = HandoffAction::new(HandoffActionKind::Merge, "merged PR #443")
            .with_timestamp("2026-05-20T06:16:00Z");
        assert_eq!(action.timestamp.as_deref(), Some("2026-05-20T06:16:00Z"));
        assert_eq!(action.kind, HandoffActionKind::Merge);
    }
}
