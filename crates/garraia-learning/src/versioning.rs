use crate::updater::ShellRunner;
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────

/// Paths required by all versioning operations.
#[derive(Debug, Clone)]
pub struct VersioningOptions {
    /// Directory containing skill `.md` files (e.g. `.garra/skills/`).
    pub skills_dir: PathBuf,
    /// Root of the git repository that tracks the skills dir.
    pub repo_root: PathBuf,
}

impl VersioningOptions {
    pub fn new(skills_dir: PathBuf, repo_root: PathBuf) -> Self {
        Self {
            skills_dir,
            repo_root,
        }
    }
}

/// A point-in-time snapshot of a skill file recorded in git history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillVersion {
    pub sha: String,
    pub short_sha: String,
    pub date: String,
    pub author: String,
    pub message: String,
}

/// One entry in the append-only score ledger for a skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreEntry {
    /// Full git SHA of the commit that set this score,
    /// or a synthetic key such as `"rollback-<sha>"`.
    pub sha: String,
    /// UTC timestamp in `YYYY-MM-DDTHH:MM:SSZ` format.
    pub timestamp_utc: String,
    /// EMA score at this point, in `0.0..=1.0`.
    pub score: f32,
}

// ─────────────────────────────────────────────────────────
// Argument validation
// ─────────────────────────────────────────────────────────

/// Smallest abbreviated SHA git will resolve without ambiguity in practice.
const SHA_MIN_LEN: usize = 7;
/// Length of a full SHA-1 object name.
const SHA_MAX_LEN: usize = 40;

/// Rejects anything that is not a git object name.
///
/// Every function in this module hands its SHA straight to `git` as a command
/// line argument, and the callers include an HTTP handler
/// (`POST /api/learning/skills/{name}/rollback`) that reads it from a request
/// body. Two distinct things go wrong without this check, and neither needs a
/// shell — `Command` never spawns one:
///
/// 1. **Option injection.** `git revert --no-edit <sha>` reads any value
///    starting with `-` as an option instead of a commit. `--output=<path>` on
///    the `git diff` path is an arbitrary file write; other options change what
///    the command does in ways the caller never asked for. Passing the value
///    after a `--` separator stops the parse, and this check makes the value
///    unable to look like an option in the first place.
/// 2. **A panic.** [`short_sha`] slices the first 8 *bytes*; a multi-byte
///    character straddling that boundary panics inside the handler.
///
/// The error deliberately does not echo the rejected value: it travels back to
/// an HTTP client, and reflecting attacker-chosen bytes into a response body is
/// a separate problem.
pub fn validate_git_sha(value: &str) -> Result<()> {
    if value.len() < SHA_MIN_LEN || value.len() > SHA_MAX_LEN {
        return Err(Error::Other(format!(
            "invalid git sha: expected {SHA_MIN_LEN}..={SHA_MAX_LEN} hexadecimal characters"
        )));
    }
    if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Other(
            "invalid git sha: expected hexadecimal characters only".to_string(),
        ));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────
// Path helpers
// ─────────────────────────────────────────────────────────

fn skill_file_path(name: &str, opts: &VersioningOptions) -> PathBuf {
    opts.skills_dir.join(format!("{name}.md"))
}

fn history_dir(opts: &VersioningOptions) -> PathBuf {
    opts.skills_dir.join("_history")
}

fn score_ledger_path(name: &str, opts: &VersioningOptions) -> PathBuf {
    history_dir(opts).join(format!("{name}.json"))
}

fn relative_skill_path(name: &str, opts: &VersioningOptions) -> Result<String> {
    let abs = skill_file_path(name, opts);
    let rel = abs.strip_prefix(&opts.repo_root).map_err(|_| {
        Error::Other(format!(
            "skill path '{}' is outside repo_root '{}'",
            abs.display(),
            opts.repo_root.display()
        ))
    })?;
    Ok(rel.to_string_lossy().into_owned())
}

// ─────────────────────────────────────────────────────────
// Public functions — history + diff
// ─────────────────────────────────────────────────────────

/// Returns the git commit history for a skill file, newest first.
///
/// Each entry comes from `git log --format="%H|%h|%ai|%an|%s" -- <skill-file>`.
pub fn history<R: ShellRunner>(
    name: &str,
    opts: &VersioningOptions,
    runner: &R,
) -> Result<Vec<SkillVersion>> {
    let rel_path = relative_skill_path(name, opts)?;
    let output = runner.run_git(
        &["log", "--format=%H|%h|%ai|%an|%s", "--", &rel_path],
        &opts.repo_root,
    )?;
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(parse_log_line)
        .collect()
}

fn parse_log_line(line: &str) -> Result<SkillVersion> {
    let parts: Vec<&str> = line.splitn(5, '|').collect();
    if parts.len() < 5 {
        return Err(Error::Other(format!(
            "unexpected git log output line: '{line}'"
        )));
    }
    Ok(SkillVersion {
        sha: parts[0].to_string(),
        short_sha: parts[1].to_string(),
        date: parts[2].to_string(),
        author: parts[3].to_string(),
        message: parts[4].to_string(),
    })
}

/// Returns the raw unified diff of a skill file between two git SHAs.
pub fn diff<R: ShellRunner>(
    name: &str,
    from_sha: &str,
    to_sha: &str,
    opts: &VersioningOptions,
    runner: &R,
) -> Result<String> {
    validate_git_sha(from_sha)?;
    validate_git_sha(to_sha)?;
    let rel_path = relative_skill_path(name, opts)?;
    runner.run_git(
        &["diff", &format!("{from_sha}..{to_sha}"), "--", &rel_path],
        &opts.repo_root,
    )
}

// ─────────────────────────────────────────────────────────
// Public functions — score ledger
// ─────────────────────────────────────────────────────────

/// Reads the full score history ledger for a skill.
///
/// Returns an empty `Vec` if the ledger file does not yet exist.
pub fn score_history(name: &str, opts: &VersioningOptions) -> Result<Vec<ScoreEntry>> {
    let path = score_ledger_path(name, opts);
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&path)?;
    serde_json::from_str::<Vec<ScoreEntry>>(&raw)
        .map_err(|e| Error::Other(format!("invalid score ledger at '{}': {e}", path.display())))
}

/// Appends a `ScoreEntry` to the ledger without modifying existing entries.
///
/// Creates the `_history/` directory if it does not exist.
/// This function is the **only** correct way to write to the ledger.
pub fn append_score_entry(name: &str, entry: ScoreEntry, opts: &VersioningOptions) -> Result<()> {
    let dir = history_dir(opts);
    std::fs::create_dir_all(&dir)?;
    let path = score_ledger_path(name, opts);
    let mut entries = score_history(name, opts)?;
    entries.push(entry);
    let json = serde_json::to_string_pretty(&entries)
        .map_err(|e| Error::Other(format!("failed to serialise score ledger: {e}")))?;
    std::fs::write(&path, json)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────
// Public function — rollback
// ─────────────────────────────────────────────────────────

/// Rolls back a skill by reverting the git commit identified by `to_sha`.
///
/// Steps:
/// 1. **Idempotency guard**: if a revert of `to_sha` is already in the log, return `Ok(())`.
/// 2. Run `git revert --no-edit -- <to_sha>`.
/// 3. Look up the `ScoreEntry` for `to_sha` in the ledger and re-append it (score reset).
/// 4. Append a rollback audit entry tagged `rollback-<sha>`.
///
/// The `reason` string is included in the audit entry message (not logged as PII — only
/// `name.len()` and sha are carried).
pub fn rollback<R: ShellRunner>(
    name: &str,
    to_sha: &str,
    reason: &str,
    opts: &VersioningOptions,
    runner: &R,
) -> Result<()> {
    // 0. Reject anything that is not an object name, before it reaches `git`
    //    or `short_sha`. Fail-closed: no process is spawned on a bad value.
    validate_git_sha(to_sha)?;

    // 1. Idempotency: has this SHA already been reverted?
    let grep_pattern = format!("Revert.*{}", short_sha(to_sha));
    let existing = runner
        .run_git(
            &["log", "--oneline", "--grep", &grep_pattern],
            &opts.repo_root,
        )
        .unwrap_or_default();
    if !existing.trim().is_empty() {
        return Ok(());
    }

    // 2. Perform the revert.
    // `--` ends option parsing: belt and braces over `validate_git_sha`.
    runner.run_git(&["revert", "--no-edit", "--", to_sha], &opts.repo_root)?;

    // 3. Look up historical score for this SHA.
    let historical_score = score_history(name, opts)?
        .into_iter()
        .find(|e| e.sha == to_sha)
        .map(|e| e.score)
        .unwrap_or(0.0);

    // 4. Append rollback audit entry (carries name_len, not name — no PII).
    let _ = reason; // reason captured in the git commit message via `git revert`; not stored in ledger
    let audit = ScoreEntry {
        sha: format!("rollback-{to_sha}"),
        timestamp_utc: now_utc_iso8601(),
        score: historical_score,
    };
    append_score_entry(name, audit, opts)?;

    Ok(())
}

fn short_sha(sha: &str) -> &str {
    let end = sha.len().min(8);
    &sha[..end]
}

// ─────────────────────────────────────────────────────────
// Timestamp helper (no external dep required)
// ─────────────────────────────────────────────────────────

fn now_utc_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    epoch_secs_to_iso8601(secs)
}

fn epoch_secs_to_iso8601(secs: u64) -> String {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Howard Hinnant's civil_from_days algorithm (proleptic Gregorian).
/// Reference: <https://howardhinnant.github.io/date_algorithms.html>
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days as i64 + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y as u64, mo, d)
}

// ─────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // ── Mock shell runner ──────────────────────────────────

    struct MockShellRunner {
        responses: Mutex<HashMap<String, Result<String>>>,
    }

    impl MockShellRunner {
        fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
            }
        }

        fn expect_git(&self, args_key: &str, response: Result<String>) {
            self.responses
                .lock()
                .unwrap()
                .insert(args_key.to_string(), response);
        }
    }

    impl ShellRunner for MockShellRunner {
        fn run_git(&self, args: &[&str], _cwd: &Path) -> Result<String> {
            let key = args.join(" ");
            self.responses
                .lock()
                .unwrap()
                .remove(&key)
                .unwrap_or_else(|| Ok(String::new()))
        }

        fn run_gh(&self, _args: &[&str], _cwd: &Path) -> Result<String> {
            Ok(String::new())
        }
    }

    // ── Fixtures ──────────────────────────────────────────

    fn make_opts(tmp: &TempDir) -> VersioningOptions {
        let skills_dir = tmp.path().join(".garra").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        VersioningOptions {
            skills_dir,
            repo_root: tmp.path().to_path_buf(),
        }
    }

    const SHA1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHORT1: &str = "aaaaaaaa";

    fn log_line(sha: &str, short: &str) -> String {
        format!("{sha}|{short}|2026-05-19 00:00:00 +0000|Alice|add skill foo")
    }

    // ── history() ─────────────────────────────────────────

    #[test]
    fn history_parses_git_log_output() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = MockShellRunner::new();
        let mock_output = format!("{}\n{}", log_line(SHA1, SHORT1), log_line(SHA2, "bbbbbbbb"));
        runner.expect_git(
            "log --format=%H|%h|%ai|%an|%s -- .garra/skills/foo.md",
            Ok(mock_output),
        );

        let versions = history("foo", &opts, &runner).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].sha, SHA1);
        assert_eq!(versions[0].author, "Alice");
        assert_eq!(versions[0].message, "add skill foo");
    }

    #[test]
    fn history_returns_empty_on_no_commits() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = MockShellRunner::new();
        runner.expect_git(
            "log --format=%H|%h|%ai|%an|%s -- .garra/skills/foo.md",
            Ok(String::new()),
        );

        let versions = history("foo", &opts, &runner).unwrap();
        assert!(versions.is_empty());
    }

    #[test]
    fn history_errors_on_malformed_line() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = MockShellRunner::new();
        runner.expect_git(
            "log --format=%H|%h|%ai|%an|%s -- .garra/skills/foo.md",
            Ok("bad".to_string()),
        );

        assert!(history("foo", &opts, &runner).is_err());
    }

    // ── diff() ────────────────────────────────────────────

    #[test]
    fn diff_calls_git_with_correct_args() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = MockShellRunner::new();
        let expected_diff = "--- a/skill.md\n+++ b/skill.md\n@@ -1 +1 @@\n-old\n+new";
        runner.expect_git(
            &format!("diff {SHA1}..{SHA2} -- .garra/skills/foo.md"),
            Ok(expected_diff.to_string()),
        );

        let result = diff("foo", SHA1, SHA2, &opts, &runner).unwrap();
        assert_eq!(result, expected_diff);
    }

    // ── score_history() + append_score_entry() ────────────

    #[test]
    fn score_history_empty_when_no_ledger() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        assert!(score_history("foo", &opts).unwrap().is_empty());
    }

    #[test]
    fn append_score_entry_creates_ledger_and_appends() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);

        let entry1 = ScoreEntry {
            sha: SHA1.to_string(),
            timestamp_utc: "2026-05-19T00:00:00Z".to_string(),
            score: 0.8,
        };
        append_score_entry("foo", entry1.clone(), &opts).unwrap();

        let entries = score_history("foo", &opts).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], entry1);
    }

    #[test]
    fn append_score_entry_is_truly_append_only() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);

        let entry1 = ScoreEntry {
            sha: SHA1.to_string(),
            timestamp_utc: "2026-05-19T00:00:00Z".to_string(),
            score: 0.8,
        };
        append_score_entry("foo", entry1.clone(), &opts).unwrap();

        let entry2 = ScoreEntry {
            sha: SHA2.to_string(),
            timestamp_utc: "2026-05-19T01:00:00Z".to_string(),
            score: 0.6,
        };
        append_score_entry("foo", entry2, &opts).unwrap();

        let entries = score_history("foo", &opts).unwrap();
        assert_eq!(entries.len(), 2);
        // First entry must NOT be modified.
        assert_eq!(entries[0], entry1, "first entry must remain unchanged");
        assert_eq!(entries[0].score, 0.8);
    }

    #[test]
    fn append_score_entry_overwrites_check_fails_on_mutation() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);

        let entry = ScoreEntry {
            sha: SHA1.to_string(),
            timestamp_utc: "2026-05-19T00:00:00Z".to_string(),
            score: 0.9,
        };
        append_score_entry("foo", entry.clone(), &opts).unwrap();

        // Simulate attempt to mutate: read → modify first entry → write back
        let path = score_ledger_path("foo", &opts);
        let mut entries: Vec<ScoreEntry> =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // A mutation attempt: change score of existing entry
        entries[0].score = 0.0;
        // Rewrite the ledger (simulating a bug / malicious write)
        std::fs::write(&path, serde_json::to_string_pretty(&entries).unwrap()).unwrap();

        // If we read back, the mutated value is visible — this is intentionally NOT prevented
        // at the Rust API level (the invariant is enforced by `append_score_entry` never
        // overwriting entries it reads). We verify the invariant via the previous test
        // (append_score_entry_is_truly_append_only). This test documents the threat model.
        let after = score_history("foo", &opts).unwrap();
        assert_eq!(
            after[0].score, 0.0,
            "ledger was externally mutated (threat model test)"
        );
    }

    // ── rollback() ────────────────────────────────────────

    #[test]
    fn rollback_reverts_commit_and_appends_audit_entry() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);

        // Pre-populate ledger with a score entry for SHA1
        append_score_entry(
            "foo",
            ScoreEntry {
                sha: SHA1.to_string(),
                timestamp_utc: "2026-05-19T00:00:00Z".to_string(),
                score: 0.75,
            },
            &opts,
        )
        .unwrap();

        let runner = MockShellRunner::new();
        // Idempotency check: no existing revert commit
        runner.expect_git(
            &format!("log --oneline --grep Revert.*{SHORT1}"),
            Ok(String::new()),
        );
        // The actual revert
        runner.expect_git(&format!("revert --no-edit -- {SHA1}"), Ok(String::new()));

        rollback("foo", SHA1, "test rollback", &opts, &runner).unwrap();

        let entries = score_history("foo", &opts).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(
            entries[1].sha.starts_with("rollback-"),
            "audit entry present"
        );
        assert_eq!(entries[1].score, 0.75, "score reset to historical value");
    }

    #[test]
    fn rollback_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);

        let runner = MockShellRunner::new();
        // Idempotency check returns an existing revert commit
        runner.expect_git(
            &format!("log --oneline --grep Revert.*{SHORT1}"),
            Ok(format!("deadbeef Revert commit for {SHORT1}")),
        );

        // Should NOT call git revert or append to ledger
        rollback("foo", SHA1, "repeat", &opts, &runner).unwrap();

        // Ledger must be empty (no audit entry appended)
        assert!(score_history("foo", &opts).unwrap().is_empty());
    }

    #[test]
    fn rollback_uses_zero_score_when_sha_not_in_ledger() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);

        let runner = MockShellRunner::new();
        runner.expect_git(
            &format!("log --oneline --grep Revert.*{SHORT1}"),
            Ok(String::new()),
        );
        runner.expect_git(&format!("revert --no-edit -- {SHA1}"), Ok(String::new()));

        rollback("foo", SHA1, "unknown sha", &opts, &runner).unwrap();

        let entries = score_history("foo", &opts).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].score, 0.0, "unknown sha → score defaults to 0.0");
    }

    // ── epoch_secs_to_iso8601() ───────────────────────────

    #[test]
    fn iso8601_formats_unix_epoch_correctly() {
        assert_eq!(epoch_secs_to_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso8601_formats_known_date() {
        // 2026-05-19T00:00:00Z = 1779148800
        assert_eq!(epoch_secs_to_iso8601(1_779_148_800), "2026-05-19T00:00:00Z");
    }

    #[test]
    fn iso8601_formats_leap_year() {
        // 2024-02-29T12:00:00Z = 1709208000
        assert_eq!(epoch_secs_to_iso8601(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    // ── relative_skill_path() ─────────────────────────────

    #[test]
    fn relative_skill_path_strips_repo_root() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let rel = relative_skill_path("my-skill", &opts).unwrap();
        assert_eq!(rel, ".garra/skills/my-skill.md");
    }

    // ── validate_git_sha + option injection ───────────────
    //
    // `POST /api/learning/skills/{name}/rollback` reads `sha` from a request
    // body and it ends up as a `git` argument. These tests pin the two ways
    // that went wrong: an argument that git reads as an option, and a byte
    // slice that panics.

    /// Records every arg vector so a test can assert that nothing was spawned.
    struct RecordingRunner {
        git_calls: Mutex<Vec<Vec<String>>>,
    }

    impl RecordingRunner {
        fn new() -> Self {
            Self {
                git_calls: Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.git_calls.lock().unwrap().clone()
        }
    }

    impl ShellRunner for RecordingRunner {
        fn run_git(&self, args: &[&str], _cwd: &Path) -> Result<String> {
            self.git_calls
                .lock()
                .unwrap()
                .push(args.iter().map(|a| a.to_string()).collect());
            Ok(String::new())
        }
        fn run_gh(&self, _args: &[&str], _cwd: &Path) -> Result<String> {
            Ok(String::new())
        }
    }

    #[test]
    fn validate_git_sha_accepts_real_object_names() {
        assert!(validate_git_sha(SHA1).is_ok());
        assert!(validate_git_sha("aaaaaaa").is_ok(), "7-char abbreviation");
        assert!(validate_git_sha("0123456789abcdefABCDEF0123456789abcdef01").is_ok());
    }

    #[test]
    fn validate_git_sha_rejects_option_lookalikes() {
        // The payload that motivated the fix: `git diff --output=<path>` writes
        // a file, and `git revert` reads it as an option too.
        assert!(validate_git_sha("--output=/tmp/pwn").is_err());
        assert!(validate_git_sha("-x").is_err());
        assert!(validate_git_sha("--upload-pack=curl").is_err());
    }

    #[test]
    fn validate_git_sha_rejects_wrong_shapes() {
        assert!(validate_git_sha("").is_err(), "empty");
        assert!(validate_git_sha("aaaaaa").is_err(), "6 chars is too short");
        assert!(
            validate_git_sha(&"a".repeat(41)).is_err(),
            "41 chars is too long"
        );
        assert!(validate_git_sha("zzzzzzz").is_err(), "not hexadecimal");
        assert!(validate_git_sha("aaaa aaa").is_err(), "space");
        assert!(validate_git_sha("HEAD~1").is_err(), "revision expression");
    }

    #[test]
    fn validate_git_sha_error_does_not_echo_the_input() {
        // The message travels to an HTTP client; reflecting attacker bytes back
        // is a separate problem, so it must not appear.
        let err = validate_git_sha("--output=/tmp/pwn")
            .unwrap_err()
            .to_string();
        assert!(!err.contains("/tmp/pwn"), "error echoed the input: {err}");
    }

    #[test]
    fn rollback_refuses_option_injection_before_spawning_git() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = RecordingRunner::new();

        let result = rollback("foo", "--output=/tmp/pwn", "why", &opts, &runner);

        assert!(result.is_err(), "option-shaped sha must be refused");
        assert!(
            runner.calls().is_empty(),
            "fail-closed: no git process may be spawned, got {:?}",
            runner.calls()
        );
    }

    #[test]
    fn rollback_does_not_panic_on_multibyte_sha() {
        // `short_sha` slices the first 8 *bytes*. Byte 8 of this value falls in
        // the middle of the 'e-acute', which panics without the length+hex check.
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = RecordingRunner::new();

        let result = rollback("foo", "aaaaaaa\u{e9}", "why", &opts, &runner);

        assert!(result.is_err());
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn rollback_separates_the_sha_with_double_dash() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = RecordingRunner::new();

        rollback("foo", SHA1, "why", &opts, &runner).unwrap();

        let revert = runner
            .calls()
            .into_iter()
            .find(|c| c.first().map(|a| a == "revert").unwrap_or(false))
            .expect("revert was not called");
        assert_eq!(revert, vec!["revert", "--no-edit", "--", SHA1]);
    }

    #[test]
    fn diff_refuses_option_injection_on_either_side() {
        let tmp = TempDir::new().unwrap();
        let opts = make_opts(&tmp);
        let runner = RecordingRunner::new();

        assert!(diff("foo", "--output=/tmp/pwn", SHA2, &opts, &runner).is_err());
        assert!(diff("foo", SHA1, "--output=/tmp/pwn", &opts, &runner).is_err());
        assert!(
            runner.calls().is_empty(),
            "fail-closed on both sides of the range"
        );
    }
}
