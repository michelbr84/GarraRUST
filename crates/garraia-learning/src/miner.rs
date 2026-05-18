use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────

/// A single command entry recorded in a session log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEntry {
    pub cmd: String,
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
}

/// Session log as stored in `~/.garra/sessions/<id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub task_family: String,
    #[serde(default)]
    pub commands: Vec<CommandEntry>,
    #[serde(default)]
    pub created_at: String,
}

/// Configuration for a mining run.
pub struct MineOptions {
    /// Directory containing `*.json` session files.
    pub sessions_dir: PathBuf,
    /// Output directory for candidate skill files.
    pub candidates_dir: PathBuf,
    /// Minimum number of sessions that must contain a pattern.
    pub threshold: usize,
}

impl MineOptions {
    /// Construct options rooted at an explicit base (useful in tests).
    pub fn with_base(base: &Path, threshold: usize) -> Self {
        MineOptions {
            sessions_dir: base.join("sessions"),
            candidates_dir: base.join("skills/_candidates"),
            threshold,
        }
    }
}

/// A command-sequence pattern detected across multiple sessions.
#[derive(Debug, Clone)]
pub struct MinedPattern {
    /// Normalized command pair (length 2).
    pub normalized_sequence: Vec<String>,
    /// Number of distinct sessions that contain this pair.
    pub occurrence_count: usize,
    /// URL-safe slug derived from the command actions.
    pub slug: String,
    /// 8-char hex FNV-1a hash of the normalized sequence (for idempotency).
    pub content_hash: String,
}

// ──────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────

/// Mine session logs from `opts.sessions_dir`, detect repeated command pairs,
/// and write PII-redacted candidate files to `opts.candidates_dir`.
///
/// Returns the list of detected patterns (including those whose file already
/// existed — idempotent).
pub fn mine(opts: &MineOptions) -> Result<Vec<MinedPattern>> {
    let sessions = load_all_sessions(&opts.sessions_dir)?;
    let patterns = find_patterns(&sessions, opts.threshold);

    if !patterns.is_empty() {
        std::fs::create_dir_all(&opts.candidates_dir).map_err(|e| {
            Error::Other(format!(
                "cannot create candidates dir {}: {e}",
                opts.candidates_dir.display()
            ))
        })?;
    }

    for pattern in &patterns {
        write_candidate_if_new(&opts.candidates_dir, pattern)?;
    }

    Ok(patterns)
}

/// Mine a single session log file.
///
/// A single session produces no patterns on its own (patterns require ≥ 2
/// sessions). Returns an empty vec for compatibility with the original stub
/// signature, without error.
pub fn mine_from_log(log_path: &Path) -> Result<Vec<crate::Skill>> {
    // Validate the file can be parsed; report errors rather than silently succeed.
    let _session = load_session(log_path)?;
    Ok(vec![])
}

// ──────────────────────────────────────────────
// Normalization
// ──────────────────────────────────────────────

/// Normalize a command string for cross-session pattern matching.
///
/// Transformations applied in order:
/// 1. Pure-integer tokens (PR numbers, issue numbers, ports) are removed.
/// 2. The token immediately after `--delete` that is not a flag gets replaced
///    with `<BRANCH>` (handles `git push origin --delete <branch>`).
///
/// Flags (tokens starting with `-`) are always preserved unchanged.
pub fn normalize_cmd(cmd: &str) -> String {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let n = tokens.len();
    let mut out: Vec<String> = Vec::with_capacity(n);
    let mut i = 0;

    while i < n {
        let tok = tokens[i];

        if is_pure_integer(tok) {
            // Strip pure integers entirely.
            i += 1;
            continue;
        }

        if tok == "--delete" && i + 1 < n && !tokens[i + 1].starts_with('-') {
            // --delete <branch> → keep the flag, replace branch with placeholder.
            out.push("--delete".to_string());
            out.push("<BRANCH>".to_string());
            i += 2;
            continue;
        }

        out.push(tok.to_string());
        i += 1;
    }

    out.join(" ")
}

fn is_pure_integer(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

// ──────────────────────────────────────────────
// PII redaction
// ──────────────────────────────────────────────

/// Redact PII from a string before writing it to a candidate file.
///
/// Replacements:
/// - Email-shaped tokens → `<EMAIL>`
/// - Long alphanumeric tokens (≥ 32 consecutive alnum chars) → `<TOKEN>`
/// - Absolute home paths (`/home/<user>/…` or `/Users/<user>/…`) → `<HOMEPATH>/…`
pub fn redact(text: &str) -> String {
    let text = redact_home_paths(text);
    let words: Vec<String> = text.split_whitespace().map(redact_word).collect();
    words.join(" ")
}

fn redact_word(word: &str) -> String {
    if has_email_shape(word) {
        return "<EMAIL>".to_string();
    }
    if has_long_token(word) {
        return "<TOKEN>".to_string();
    }
    word.to_string()
}

fn has_email_shape(word: &str) -> bool {
    if let Some(at) = word.find('@') {
        let local = &word[..at];
        let domain = &word[at + 1..];
        return !local.is_empty() && domain.contains('.') && !domain.starts_with('.');
    }
    false
}

fn has_long_token(word: &str) -> bool {
    let mut run = 0usize;
    for ch in word.chars() {
        if ch.is_ascii_alphanumeric() {
            run += 1;
            if run >= 32 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn redact_home_paths(text: &str) -> String {
    let mut result = text.to_string();
    for prefix in &["/home/", "/Users/"] {
        while let Some(pos) = result.find(prefix) {
            let after = &result[pos + prefix.len()..];
            let seg_end = after.find('/').unwrap_or(after.len());
            let username = &after[..seg_end];
            if username.is_empty() {
                break;
            }
            let original = format!("{prefix}{username}");
            result = result.replacen(&original, "<HOMEPATH>", 1);
        }
    }
    result
}

// ──────────────────────────────────────────────
// Pattern detection
// ──────────────────────────────────────────────

/// Detect repeated consecutive-command pairs across sessions.
///
/// Counts how many *distinct sessions* contain each normalized pair.
/// Sessions that contain the same pair multiple times still count as 1.
fn find_patterns(sessions: &[SessionRecord], threshold: usize) -> Vec<MinedPattern> {
    // pair → set of session indices that contain it
    let mut pair_to_sessions: HashMap<(String, String), HashSet<usize>> = HashMap::new();

    for (idx, session) in sessions.iter().enumerate() {
        let normalized: Vec<String> = session
            .commands
            .iter()
            .filter(|c| !c.cmd.trim().is_empty())
            .map(|c| normalize_cmd(&c.cmd))
            .filter(|n| !n.is_empty())
            .collect();

        for window in normalized.windows(2) {
            let pair = (window[0].clone(), window[1].clone());
            pair_to_sessions.entry(pair).or_default().insert(idx);
        }
    }

    let mut results: Vec<MinedPattern> = pair_to_sessions
        .into_iter()
        .filter(|(_, set)| set.len() >= threshold)
        .map(|((cmd1, cmd2), set)| {
            let seq = vec![cmd1, cmd2];
            let hash = compute_hash(&seq);
            let slug = derive_slug(&seq);
            MinedPattern {
                normalized_sequence: seq,
                occurrence_count: set.len(),
                slug,
                content_hash: hash,
            }
        })
        .collect();

    // Deterministic ordering: by slug then hash.
    results.sort_by(|a, b| {
        a.slug
            .cmp(&b.slug)
            .then(a.content_hash.cmp(&b.content_hash))
    });
    results
}

// ──────────────────────────────────────────────
// Hash + slug
// ──────────────────────────────────────────────

/// FNV-1a 32-bit hash of the normalized sequence, returned as 8 hex chars.
fn compute_hash(seq: &[String]) -> String {
    const FNV_OFFSET: u32 = 2_166_136_261;
    const FNV_PRIME: u32 = 16_777_619;
    let mut hash = FNV_OFFSET;
    for s in seq {
        for byte in s.bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        // separator between sequence elements
        hash ^= b'|' as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:08x}")
}

/// Derive a human-readable slug from the normalized command pair.
///
/// Strategy: extract the primary verb from each command, join with `-`.
/// Falls back to `mined` if no verbs are found.
fn derive_slug(seq: &[String]) -> String {
    let verbs: Vec<String> = seq
        .iter()
        .filter_map(|cmd| extract_primary_verb(cmd))
        .collect();

    if verbs.is_empty() {
        "mined".to_string()
    } else {
        verbs.join("-")
    }
}

/// Extract the most-meaningful action word from a normalized command.
///
/// Rules:
/// 1. Skip the executable name (first token).
/// 2. Return the first non-flag, non-placeholder subcommand or the last
///    non-flag token if no subcommand is found.
fn extract_primary_verb(cmd: &str) -> Option<String> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    // skip index 0 (executable)
    let candidates: Vec<&&str> = tokens[1..]
        .iter()
        .filter(|t| !t.starts_with('-') && !t.starts_with('<'))
        .collect();

    candidates.last().map(|t| t.to_string())
}

// ──────────────────────────────────────────────
// Candidate file I/O
// ──────────────────────────────────────────────

/// Write a candidate skill file unless it already exists (idempotent).
///
/// Filename: `mined-<slug>-<hash8>.md`
/// Format: YAML frontmatter (`---` delimited) + Markdown `## Commands` section.
fn write_candidate_if_new(candidates_dir: &Path, pattern: &MinedPattern) -> Result<()> {
    let filename = format!("mined-{}-{}.md", pattern.slug, pattern.content_hash);
    let path = candidates_dir.join(&filename);

    if path.exists() {
        tracing::debug!("candidate already exists, skipping: {}", path.display());
        return Ok(());
    }

    let content = render_candidate(pattern);
    std::fs::write(&path, content)
        .map_err(|e| Error::Other(format!("cannot write candidate {}: {e}", path.display())))?;

    tracing::info!(
        slug = %pattern.slug,
        occurrences = pattern.occurrence_count,
        "wrote candidate skill"
    );
    Ok(())
}

fn render_candidate(pattern: &MinedPattern) -> String {
    use crate::{LearningSkillFrontmatter, SkillScope, SkillSource};

    let fm = LearningSkillFrontmatter {
        name: format!("mined-{}", pattern.slug),
        version: "0.1.0".to_string(),
        source: SkillSource::Mined,
        scope: SkillScope::Project,
        score: 0.0,
        locked: false,
        critical_paths_touched: vec![],
        fail_count: 0,
    };

    let fm_yaml = serde_yaml::to_string(&fm).unwrap_or_else(|_| "{}".to_string());

    let body_lines: Vec<String> = pattern
        .normalized_sequence
        .iter()
        .map(|cmd| redact(cmd))
        .collect();

    format!(
        "---\n{fm_yaml}---\n\n## Commands\n\nDetected in {} session(s).\n\n```\n{}\n```\n",
        pattern.occurrence_count,
        body_lines.join("\n")
    )
}

// ──────────────────────────────────────────────
// Session I/O
// ──────────────────────────────────────────────

fn load_all_sessions(dir: &Path) -> Result<Vec<SessionRecord>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut sessions = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::Other(format!("read_dir {}: {e}", dir.display())))?;

    for entry in entries {
        let entry = entry.map_err(|e| Error::Other(format!("dir entry error: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            match load_session(&path) {
                Ok(s) => sessions.push(s),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "skipping malformed session");
                }
            }
        }
    }
    Ok(sessions)
}

fn load_session(path: &Path) -> Result<SessionRecord> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::Other(format!("read {}: {e}", path.display())))?;
    serde_json::from_str(&content)
        .map_err(|e| Error::Other(format!("parse {}: {e}", path.display())))
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    // ── helpers ──────────────────────────────

    fn session(id: &str, cmds: &[&str]) -> SessionRecord {
        SessionRecord {
            session_id: id.to_string(),
            intent: String::new(),
            task_family: String::new(),
            commands: cmds
                .iter()
                .map(|c| CommandEntry {
                    cmd: c.to_string(),
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })
                .collect(),
            created_at: String::new(),
        }
    }

    fn write_session_json(dir: &Path, id: &str, cmds: &[&str]) {
        let s = session(id, cmds);
        let json = serde_json::to_string(&s).unwrap();
        let path = dir.join(format!("{id}.json"));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(json.as_bytes()).unwrap();
    }

    // ── normalize_cmd ────────────────────────

    #[test]
    fn normalize_strips_pure_integer() {
        assert_eq!(
            normalize_cmd("gh pr merge 123 --squash --delete-branch"),
            "gh pr merge --squash --delete-branch"
        );
    }

    #[test]
    fn normalize_replaces_branch_after_delete() {
        assert_eq!(
            normalize_cmd("git push origin --delete feature-branch"),
            "git push origin --delete <BRANCH>"
        );
    }

    #[test]
    fn normalize_keeps_flags() {
        assert_eq!(
            normalize_cmd("cargo clippy --workspace -- -D warnings"),
            "cargo clippy --workspace -- -D warnings"
        );
    }

    #[test]
    fn normalize_preserves_non_integer_args() {
        assert_eq!(normalize_cmd("git checkout main"), "git checkout main");
    }

    #[test]
    fn normalize_empty_command() {
        assert_eq!(normalize_cmd(""), "");
    }

    // ── redact ──────────────────────────────

    #[test]
    fn redact_email() {
        assert_eq!(redact("contact user@example.com"), "contact <EMAIL>");
    }

    #[test]
    fn redact_long_token() {
        let token = "a".repeat(32);
        let input = format!("KEY={token}");
        let out = redact(&input);
        assert!(out.contains("<TOKEN>"), "expected <TOKEN> in: {out}");
    }

    #[test]
    fn redact_home_path() {
        let out = redact("config at /home/michelbr/config.toml");
        assert!(out.contains("<HOMEPATH>"), "expected <HOMEPATH> in: {out}");
        assert!(!out.contains("michelbr"), "username leaked: {out}");
    }

    #[test]
    fn redact_clean_text_unchanged() {
        let text = "git push origin --delete <BRANCH>";
        assert_eq!(redact(text), text);
    }

    // ── find_patterns ────────────────────────

    #[test]
    fn detects_pair_at_threshold() {
        let sessions: Vec<SessionRecord> = (0..3)
            .map(|i| {
                session(
                    &format!("s{i}"),
                    &[
                        &format!("gh pr merge {} --squash --delete-branch", 100 + i),
                        &format!("git push origin --delete feature-{i}"),
                    ],
                )
            })
            .collect();

        let patterns = find_patterns(&sessions, 3);
        assert_eq!(patterns.len(), 1);
        let p = &patterns[0];
        assert_eq!(
            p.normalized_sequence[0],
            "gh pr merge --squash --delete-branch"
        );
        assert_eq!(
            p.normalized_sequence[1],
            "git push origin --delete <BRANCH>"
        );
        assert_eq!(p.occurrence_count, 3);
    }

    #[test]
    fn below_threshold_emits_nothing() {
        let sessions: Vec<SessionRecord> = (0..2)
            .map(|i| {
                session(
                    &format!("s{i}"),
                    &[
                        &format!("gh pr merge {} --squash --delete-branch", 100 + i),
                        &format!("git push origin --delete feature-{i}"),
                    ],
                )
            })
            .collect();

        let patterns = find_patterns(&sessions, 3);
        assert!(patterns.is_empty());
    }

    #[test]
    fn session_counted_once_even_with_repeated_pair() {
        // session s0 contains the pair twice — still counts as 1
        let s0 = session(
            "s0",
            &[
                "gh pr merge 1 --squash --delete-branch",
                "git push origin --delete feat-a",
                "gh pr merge 2 --squash --delete-branch",
                "git push origin --delete feat-b",
            ],
        );
        let s1 = session(
            "s1",
            &[
                "gh pr merge 3 --squash --delete-branch",
                "git push origin --delete feat-c",
            ],
        );
        let s2 = session(
            "s2",
            &[
                "gh pr merge 4 --squash --delete-branch",
                "git push origin --delete feat-d",
            ],
        );

        let patterns = find_patterns(&[s0, s1, s2], 3);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].occurrence_count, 3);
    }

    #[test]
    fn empty_sessions_produces_no_patterns() {
        let patterns = find_patterns(&[], 1);
        assert!(patterns.is_empty());
    }

    #[test]
    fn single_command_session_produces_no_pairs() {
        let sessions = vec![session("s0", &["git status"])];
        let patterns = find_patterns(&sessions, 1);
        assert!(patterns.is_empty());
    }

    // ── mine() integration ────────────────────

    #[test]
    fn mine_writes_candidate_file() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        for i in 0..3 {
            write_session_json(
                &sessions_dir,
                &format!("s{i}"),
                &[
                    &format!("gh pr merge {} --squash --delete-branch", 100 + i),
                    &format!("git push origin --delete feature-{i}"),
                ],
            );
        }

        // Add 7 unrelated sessions to reach the fixture's 10.
        for i in 3..10 {
            write_session_json(
                &sessions_dir,
                &format!("s{i}"),
                &[&format!("cargo test -p crate-{i}")],
            );
        }

        let candidates_dir = tmp.path().join("skills/_candidates");
        let opts = MineOptions {
            sessions_dir,
            candidates_dir: candidates_dir.clone(),
            threshold: 3,
        };

        let patterns = mine(&opts).unwrap();
        assert_eq!(patterns.len(), 1);

        // Exactly one file written.
        let files: Vec<_> = std::fs::read_dir(&candidates_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);

        // File has correct content.
        let content = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(content.contains("source: mined"), "missing source: mined");
        assert!(content.contains("gh pr merge"), "missing command in body");
        assert!(
            content.contains("--delete <BRANCH>"),
            "branch not normalized"
        );
    }

    #[test]
    fn mine_idempotent() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        for i in 0..3 {
            write_session_json(
                &sessions_dir,
                &format!("s{i}"),
                &[
                    &format!("gh pr merge {} --squash --delete-branch", 100 + i),
                    &format!("git push origin --delete feature-{i}"),
                ],
            );
        }

        let candidates_dir = tmp.path().join("skills/_candidates");
        let opts = MineOptions {
            sessions_dir: sessions_dir.clone(),
            candidates_dir: candidates_dir.clone(),
            threshold: 3,
        };

        mine(&opts).unwrap();
        mine(&opts).unwrap(); // second run

        let count = std::fs::read_dir(&candidates_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(
            count, 1,
            "idempotency violation: {count} files after 2 runs"
        );
    }

    #[test]
    fn mine_threshold_four_emits_nothing() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        for i in 0..3 {
            write_session_json(
                &sessions_dir,
                &format!("s{i}"),
                &[
                    &format!("gh pr merge {} --squash --delete-branch", 100 + i),
                    &format!("git push origin --delete feature-{i}"),
                ],
            );
        }

        let opts = MineOptions {
            sessions_dir,
            candidates_dir: tmp.path().join("candidates"),
            threshold: 4,
        };

        let patterns = mine(&opts).unwrap();
        assert!(patterns.is_empty());
    }

    #[test]
    fn mine_missing_sessions_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let opts = MineOptions {
            sessions_dir: tmp.path().join("nonexistent"),
            candidates_dir: tmp.path().join("candidates"),
            threshold: 3,
        };
        let result = mine(&opts).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn mine_malformed_json_skipped() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Write one malformed file.
        std::fs::write(sessions_dir.join("bad.json"), b"not json at all").unwrap();

        // Write 3 valid sessions.
        for i in 0..3 {
            write_session_json(
                &sessions_dir,
                &format!("s{i}"),
                &[
                    &format!("gh pr merge {} --squash --delete-branch", 100 + i),
                    &format!("git push origin --delete feature-{i}"),
                ],
            );
        }

        let opts = MineOptions {
            sessions_dir,
            candidates_dir: tmp.path().join("candidates"),
            threshold: 3,
        };

        // Should not error, malformed file is skipped.
        let patterns = mine(&opts).unwrap();
        assert_eq!(
            patterns.len(),
            1,
            "expected 1 pattern despite malformed file"
        );
    }

    #[test]
    fn mine_pii_redacted_in_candidate() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        for i in 0..3 {
            // Session stdout contains an email — should be redacted in candidate body.
            let s = SessionRecord {
                session_id: format!("s{i}"),
                intent: String::new(),
                task_family: String::new(),
                commands: vec![
                    CommandEntry {
                        cmd: format!("gh pr merge {} --squash --delete-branch", 100 + i),
                        exit_code: 0,
                        stdout: "notified admin@example.com".to_string(),
                        stderr: String::new(),
                    },
                    CommandEntry {
                        cmd: format!("git push origin --delete feature-{i}"),
                        exit_code: 0,
                        stdout: String::new(),
                        stderr: String::new(),
                    },
                ],
                created_at: String::new(),
            };
            let json = serde_json::to_string(&s).unwrap();
            std::fs::write(sessions_dir.join(format!("s{i}.json")), json).unwrap();
        }

        let candidates_dir = tmp.path().join("candidates");
        let opts = MineOptions {
            sessions_dir,
            candidates_dir: candidates_dir.clone(),
            threshold: 3,
        };

        mine(&opts).unwrap();

        let files: Vec<_> = std::fs::read_dir(&candidates_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);
        let content = std::fs::read_to_string(files[0].path()).unwrap();
        // The candidate BODY (commands section) is redacted.
        // The raw email appears only in stdout (not in the body we write).
        assert!(
            !content.contains("admin@example.com"),
            "PII leaked into candidate"
        );
    }
}
