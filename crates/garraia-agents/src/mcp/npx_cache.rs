//! Issue #1346: diagnosis and one-shot recovery of a corrupted `npx` cache.
//!
//! `npx -y <pkg>` installs the package into `<npm cache>/_npx/<16 hex>/` and
//! runs it from there. An interrupted install (a killed boot, a full disk, a
//! registry hiccup) can leave that directory half-populated; from then on every
//! spawn dies with `ERR_MODULE_NOT_FOUND` (the field case was
//! `zod/v4/mini/external.js`) and npx never repairs it, because the directory
//! exists. Before this module the gateway retried five times, dumped the full
//! Node stack trace into its log each time and then gave up.
//!
//! Everything here is **pure or filesystem-read-only**, except
//! [`spawn_stderr_drain`], so the whole decision table is unit-tested without a
//! child process. The single destructive step (`remove_dir_all`) lives in the
//! manager and only ever receives a path returned by [`validate_candidate`].
//!
//! # Why so many conditions before deleting anything
//!
//! The candidate directory comes from the **child's stderr**, which is
//! untrusted output: a hostile or buggy MCP server can print any path it
//! likes. So the path from stderr is never used as-is. It is only accepted when
//! it canonicalizes to exactly `<canonical npm cache root>/_npx/<16 hex>`,
//! where the cache root is resolved from the environment the gateway itself
//! built for the child (not from anything the child said), the entry is a real
//! directory (never a symlink, and `_npx` itself is not one either), and its
//! `package.json` declares the package the server was configured to run. The
//! worst an adversarial stderr can achieve is a re-download of one npx cache
//! entry of the configured package.

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncReadExt;
use tokio::process::ChildStderr;
use tokio::task::JoinHandle;

/// How many bytes of a child's stderr are kept for classification.
pub(crate) const STDERR_TAIL_BYTES: usize = 16 * 1024;

/// How many stderr lines of one spawn are forwarded to `debug!`. A chatty
/// server must not turn debug logging into a firehose either.
const STDERR_DEBUG_LINE_BUDGET: usize = 200;

/// Longest single stderr line forwarded to `debug!`.
const STDERR_DEBUG_LINE_MAX: usize = 512;

/// Largest `package.json` we are willing to read from an npx entry.
const PACKAGE_JSON_MAX_BYTES: u64 = 64 * 1024;

/// Why an MCP stdio server failed to come up, as far as the gateway can tell.
///
/// Secret-free by construction: the only payload is a filesystem path inside
/// the npm cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpFailureCause {
    /// The npx cache entry the server runs from is incomplete or failed its
    /// integrity check. `dir` is the `_npx/<16 hex>` entry named by the
    /// child's stderr, when there was one — it is a *claim*, not yet
    /// validated.
    NpxCacheCorrupt { dir: Option<PathBuf> },
    /// The child reported `ENOSPC`.
    DiskFull,
    /// Anything else (including "no stderr at all").
    Other,
}

impl McpFailureCause {
    /// Stable machine label, used by `/api/mcp/health` and diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            McpFailureCause::NpxCacheCorrupt { .. } => "npx_cache_corrupt",
            McpFailureCause::DiskFull => "disk_full",
            McpFailureCause::Other => "other",
        }
    }

    /// The cache entry named by the child, if any.
    pub fn npx_dir(&self) -> Option<&Path> {
        match self {
            McpFailureCause::NpxCacheCorrupt { dir } => dir.as_deref(),
            _ => None,
        }
    }
}

/// Classify a failed spawn from the tail of its stderr.
///
/// `ENOSPC` wins over everything: a full disk also produces missing-module
/// errors, and clearing a cache entry would only make it worse.
pub(crate) fn classify_npx_failure(stderr_tail: &str) -> McpFailureCause {
    if stderr_tail.contains("ENOSPC") {
        return McpFailureCause::DiskFull;
    }
    let dir = find_npx_entry(stderr_tail);
    let missing_module = [
        "ERR_MODULE_NOT_FOUND",
        "MODULE_NOT_FOUND",
        "Cannot find module",
        "Cannot find package",
    ]
    .iter()
    .any(|m| stderr_tail.contains(m));
    if missing_module && dir.is_some() {
        return McpFailureCause::NpxCacheCorrupt { dir };
    }
    if stderr_tail.contains("EINTEGRITY") {
        return McpFailureCause::NpxCacheCorrupt { dir };
    }
    McpFailureCause::Other
}

fn is_sep(c: u8) -> bool {
    c == b'/' || c == b'\\'
}

fn is_lower_hex16(s: &str) -> bool {
    s.len() == 16
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// First `<...>/_npx/<16 hex>` directory in `text` that is followed by
/// `/node_modules`, i.e. a module path inside an npx entry.
fn find_npx_entry(text: &str) -> Option<PathBuf> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(off) = text[from..].find("_npx") {
        let i = from + off;
        from = i + 4;
        let hex_start = i + 5;
        let hex_end = hex_start + 16;
        let after = hex_end + 1;
        if i == 0
            || !is_sep(bytes[i - 1])
            || bytes.len() <= after
            || !is_sep(bytes[i + 4])
            || !text.is_char_boundary(hex_start)
            || !text.is_char_boundary(hex_end)
            || !is_lower_hex16(&text[hex_start..hex_end])
            || !is_sep(bytes[hex_end])
            || !text[after..].starts_with("node_modules")
        {
            continue;
        }
        // Walk back to the start of the path token.
        let start = text[..i]
            .rfind(|c: char| c.is_whitespace() || "'\"`(<[".contains(c))
            .map(|p| p + 1)
            .unwrap_or(0);
        let mut raw = &text[start..hex_end];
        if let Some(rest) = raw.strip_prefix("file://") {
            raw = rest;
            // `file:///C:/Users/...` → `C:/Users/...`
            let b = raw.as_bytes();
            if b.len() > 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
                raw = &raw[1..];
            }
        }
        let path = PathBuf::from(raw);
        if path.is_absolute() || looks_like_windows_absolute(raw) {
            return Some(path);
        }
    }
    None
}

fn looks_like_windows_absolute(raw: &str) -> bool {
    let b = raw.as_bytes();
    b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && is_sep(b[2])
}

/// Whether the configured command is `npx` itself (the only case in which the
/// gateway knows where the package lives).
pub(crate) fn is_npx_command(command: &str) -> bool {
    let base = Path::new(command)
        .file_name()
        .and_then(|b| b.to_str())
        .unwrap_or(command);
    base == "npx" || base.eq_ignore_ascii_case("npx.cmd")
}

/// The npm cache root the child **actually** uses, resolved from the
/// environment the gateway built for it (never from the child's output).
///
/// `npm_config_cache` (any case, as npm reads it) wins; otherwise the
/// platform default — `%LOCALAPPDATA%\npm-cache` on Windows, `$HOME/.npm`
/// elsewhere. `None` (and therefore "report, do not delete") when nothing
/// resolves, when the value is relative, or when two spellings of
/// `npm_config_cache` disagree. A cache moved by `.npmrc` is invisible here,
/// which only means its stderr path will not match and nothing is deleted.
pub(crate) fn npm_cache_root(child_env: &[(String, String)], windows: bool) -> Option<PathBuf> {
    let absolute = |v: &str| {
        let p = PathBuf::from(v);
        (p.is_absolute() || (windows && looks_like_windows_absolute(v))).then_some(p)
    };
    let explicit: Vec<&str> = child_env
        .iter()
        .filter(|(k, v)| k.eq_ignore_ascii_case("npm_config_cache") && !v.is_empty())
        .map(|(_, v)| v.as_str())
        .collect();
    if let Some(first) = explicit.first() {
        if explicit.iter().any(|v| v != first) {
            return None;
        }
        return absolute(first);
    }
    let (key, suffix) = if windows {
        ("LOCALAPPDATA", "npm-cache")
    } else {
        ("HOME", ".npm")
    };
    let base = child_env
        .iter()
        .find(|(k, v)| {
            !v.is_empty()
                && if windows {
                    k.eq_ignore_ascii_case(key)
                } else {
                    k == key
                }
        })
        .map(|(_, v)| v.as_str())?;
    absolute(base).map(|b| b.join(suffix))
}

/// The npm package name an `npx` invocation runs (`-p`/`--package` or the
/// first positional argument), version suffix stripped.
pub(crate) fn package_from_args(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let spec = if arg == "-p" || arg == "--package" {
            iter.next()?.as_str()
        } else if let Some(v) = arg.strip_prefix("--package=") {
            v
        } else if arg.starts_with('-') {
            continue;
        } else {
            arg.as_str()
        };
        return strip_version(spec).map(str::to_string);
    }
    None
}

fn strip_version(spec: &str) -> Option<&str> {
    let name = match spec.strip_prefix('@') {
        Some(rest) => match rest.find('@') {
            Some(at) => &spec[..at + 1],
            None => spec,
        },
        None => spec.split('@').next().unwrap_or(spec),
    };
    (!name.is_empty() && name != "@").then_some(name)
}

/// Why a candidate npx entry was **not** cleared. Every variant is a refusal;
/// the messages carry paths only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecoveryRefusal {
    Traversal,
    NotAnNpxEntry,
    CacheRootUnresolved,
    NpxDirNotARealDirectory,
    EntryMissingOrSymlink,
    OutsideCache,
    PackageJsonUnreadable,
    PackageMismatch,
}

impl std::fmt::Display for RecoveryRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RecoveryRefusal::Traversal => "path has '.' or '..' components",
            RecoveryRefusal::NotAnNpxEntry => "path is not '<cache>/_npx/<16 hex>'",
            RecoveryRefusal::CacheRootUnresolved => "npm cache root could not be resolved",
            RecoveryRefusal::NpxDirNotARealDirectory => "'_npx' is not a real directory",
            RecoveryRefusal::EntryMissingOrSymlink => "entry is missing or is a symlink",
            RecoveryRefusal::OutsideCache => "path does not resolve inside the npm cache",
            RecoveryRefusal::PackageJsonUnreadable => "entry has no readable package.json",
            RecoveryRefusal::PackageMismatch => "entry does not belong to the configured package",
        };
        f.write_str(s)
    }
}

/// Accept `candidate` for deletion only when it is exactly
/// `<canonical cache_root>/_npx/<16 hex>`, a real directory reached without
/// any symlink, whose `package.json` depends on `package`. Returns the
/// canonical path to delete.
pub(crate) fn validate_candidate(
    cache_root: &Path,
    candidate: &Path,
    package: &str,
) -> Result<PathBuf, RecoveryRefusal> {
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(RecoveryRefusal::Traversal);
    }
    let name = candidate
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| is_lower_hex16(n))
        .ok_or(RecoveryRefusal::NotAnNpxEntry)?;
    let parent_is_npx = candidate
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "_npx");
    if !parent_is_npx {
        return Err(RecoveryRefusal::NotAnNpxEntry);
    }

    let root =
        std::fs::canonicalize(cache_root).map_err(|_| RecoveryRefusal::CacheRootUnresolved)?;
    let npx_dir = root.join("_npx");
    match std::fs::symlink_metadata(&npx_dir) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        _ => return Err(RecoveryRefusal::NpxDirNotARealDirectory),
    }
    let expected = npx_dir.join(name);
    match std::fs::symlink_metadata(&expected) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        _ => return Err(RecoveryRefusal::EntryMissingOrSymlink),
    }
    // The candidate itself must resolve to the same place: this is what
    // rejects a path outside the cache and any symlinked component on the
    // way (a symlink anywhere changes the canonical form).
    match std::fs::canonicalize(candidate) {
        Ok(c) if c == expected => {}
        _ => return Err(RecoveryRefusal::OutsideCache),
    }

    let pkg_json = expected.join("package.json");
    match std::fs::symlink_metadata(&pkg_json) {
        Ok(m) if m.is_file() && m.len() <= PACKAGE_JSON_MAX_BYTES => {}
        _ => return Err(RecoveryRefusal::PackageJsonUnreadable),
    }
    let raw = std::fs::read(&pkg_json).map_err(|_| RecoveryRefusal::PackageJsonUnreadable)?;
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|_| RecoveryRefusal::PackageJsonUnreadable)?;
    let declared = value
        .get("dependencies")
        .and_then(|d| d.as_object())
        .is_some_and(|d| d.contains_key(package));
    if !declared {
        return Err(RecoveryRefusal::PackageMismatch);
    }
    Ok(expected)
}

/// Bounded tail of a byte stream: keeps the last `cap` bytes.
#[derive(Debug)]
pub(crate) struct StderrTail {
    buf: VecDeque<u8>,
    cap: usize,
}

impl StderrTail {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap.min(4096)),
            cap,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let bytes = if bytes.len() > self.cap {
            &bytes[bytes.len() - self.cap..]
        } else {
            bytes
        };
        let overflow = (self.buf.len() + bytes.len()).saturating_sub(self.cap);
        self.buf.drain(..overflow);
        self.buf.extend(bytes);
    }

    pub(crate) fn snapshot(&self) -> String {
        let (a, b) = self.buf.as_slices();
        let mut v = Vec::with_capacity(a.len() + b.len());
        v.extend_from_slice(a);
        v.extend_from_slice(b);
        String::from_utf8_lossy(&v).into_owned()
    }
}

/// Shared handle to a drained stderr tail.
pub(crate) type SharedTail = Arc<Mutex<StderrTail>>;

/// Read snapshot of a shared tail, tolerating a poisoned lock.
pub(crate) fn tail_snapshot(tail: &SharedTail) -> String {
    match tail.lock() {
        Ok(t) => t.snapshot(),
        Err(poisoned) => poisoned.into_inner().snapshot(),
    }
}

/// Drain a child's stderr without ever blocking it: every chunk goes into a
/// bounded tail (for classification) and complete lines are forwarded to
/// `debug!` up to a per-spawn budget. Nothing is logged above `debug`; stderr
/// is untrusted child output.
pub(crate) fn spawn_stderr_drain(
    server: &str,
    mut stderr: ChildStderr,
) -> (SharedTail, JoinHandle<()>) {
    let tail: SharedTail = Arc::new(Mutex::new(StderrTail::new(STDERR_TAIL_BYTES)));
    let tail_task = Arc::clone(&tail);
    let server = server.to_string();
    let handle = tokio::spawn(async move {
        let mut chunk = [0u8; 4096];
        let mut line: Vec<u8> = Vec::new();
        let mut budget = STDERR_DEBUG_LINE_BUDGET;
        let emit = |line: &mut Vec<u8>, budget: &mut usize| {
            if *budget > 0 {
                *budget -= 1;
                let text = String::from_utf8_lossy(line);
                let text = text.trim_end();
                let shown: String = text.chars().take(STDERR_DEBUG_LINE_MAX).collect();
                tracing::debug!(server = %server, "mcp child stderr: {shown}");
                if *budget == 0 {
                    tracing::debug!(server = %server, "mcp child stderr: line budget reached, further lines not forwarded");
                }
            }
            line.clear();
        };
        loop {
            let n = match stderr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            match tail_task.lock() {
                Ok(mut t) => t.push(&chunk[..n]),
                Err(poisoned) => poisoned.into_inner().push(&chunk[..n]),
            }
            for &b in &chunk[..n] {
                if b == b'\n' {
                    emit(&mut line, &mut budget);
                } else if line.len() < STDERR_DEBUG_LINE_MAX * 4 {
                    line.push(b);
                }
            }
        }
        if !line.is_empty() {
            emit(&mut line, &mut budget);
        }
    });
    (tail, handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field case (issue #1346), trimmed from a real Node 22 trace.
    const ZOD_TRACE: &str = "node:internal/modules/esm/resolve:275\n    throw new ERR_MODULE_NOT_FOUND(\n          ^\n\nError [ERR_MODULE_NOT_FOUND]: Cannot find module '/home/u/.npm/_npx/a1b2c3d4e5f60718/node_modules/zod/v4/mini/external.js' imported from /home/u/.npm/_npx/a1b2c3d4e5f60718/node_modules/zod/v4/mini/index.js\n    at finalizeResolution (node:internal/modules/esm/resolve:275:11) {\n  code: 'ERR_MODULE_NOT_FOUND',\n  url: 'file:///home/u/.npm/_npx/a1b2c3d4e5f60718/node_modules/zod/v4/mini/external.js'\n}\n\nNode.js v22.22.3\n";

    #[test]
    fn classifies_the_real_zod_trace_as_corrupt_npx_cache() {
        assert_eq!(
            classify_npx_failure(ZOD_TRACE),
            McpFailureCause::NpxCacheCorrupt {
                dir: Some(PathBuf::from("/home/u/.npm/_npx/a1b2c3d4e5f60718"))
            }
        );
    }

    #[test]
    fn classification_table() {
        let cases: &[(&str, &str)] = &[
            (
                "Error: Cannot find package 'zod' imported from /c/_npx/0123456789abcdef/node_modules/x/i.js",
                "npx_cache_corrupt",
            ),
            (
                "url: 'file:///c/_npx/0123456789abcdef/node_modules/x.js'\n code: 'ERR_MODULE_NOT_FOUND'",
                "npx_cache_corrupt",
            ),
            (
                "npm error code EINTEGRITY\nnpm error sha512-... integrity checksum failed",
                "npx_cache_corrupt",
            ),
            (
                "npm error code ENOSPC\nnpm error syscall write",
                "disk_full",
            ),
            // ENOSPC wins even when a module is also missing.
            (
                "ENOSPC\nCannot find module '/c/_npx/0123456789abcdef/node_modules/a.js'",
                "disk_full",
            ),
            // Missing module outside any npx entry is the server's own bug.
            ("Error: Cannot find module '/opt/srv/index.js'", "other"),
            ("", "other"),
            ("Error: EACCES: permission denied, open '/x'", "other"),
            // 15 hex / uppercase hex / no node_modules → not an npx entry.
            (
                "Cannot find module '/c/_npx/0123456789abcde/node_modules/a.js'",
                "other",
            ),
            (
                "Cannot find module '/c/_npx/0123456789ABCDEF/node_modules/a.js'",
                "other",
            ),
            (
                "Cannot find module '/c/_npx/0123456789abcdef/other/a.js'",
                "other",
            ),
        ];
        for (text, want) in cases {
            assert_eq!(classify_npx_failure(text).as_str(), *want, "{text}");
        }
        assert_eq!(
            classify_npx_failure("npm error code EINTEGRITY"),
            McpFailureCause::NpxCacheCorrupt { dir: None }
        );
    }

    #[test]
    fn extracts_windows_paths() {
        let t = "Error [ERR_MODULE_NOT_FOUND]: Cannot find module 'C:\\Users\\u\\AppData\\Local\\npm-cache\\_npx\\0123456789abcdef\\node_modules\\zod\\a.js'";
        assert_eq!(
            classify_npx_failure(t).npx_dir(),
            Some(Path::new(
                "C:\\Users\\u\\AppData\\Local\\npm-cache\\_npx\\0123456789abcdef"
            ))
        );
        let u = "url: 'file:///C:/Users/u/AppData/Local/npm-cache/_npx/0123456789abcdef/node_modules/a.js' ERR_MODULE_NOT_FOUND";
        assert_eq!(
            classify_npx_failure(u).npx_dir(),
            Some(Path::new(
                "C:/Users/u/AppData/Local/npm-cache/_npx/0123456789abcdef"
            ))
        );
    }

    #[test]
    fn relative_npx_paths_are_ignored() {
        let t = "Cannot find module 'cache/_npx/0123456789abcdef/node_modules/a.js'";
        assert_eq!(classify_npx_failure(t), McpFailureCause::Other);
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn cache_root_resolution_order() {
        assert_eq!(
            npm_cache_root(&env(&[("HOME", "/h"), ("npm_config_cache", "/c")]), false),
            Some(PathBuf::from("/c"))
        );
        assert_eq!(
            npm_cache_root(&env(&[("HOME", "/h"), ("NPM_CONFIG_CACHE", "/c")]), false),
            Some(PathBuf::from("/c"))
        );
        assert_eq!(
            npm_cache_root(&env(&[("HOME", "/h")]), false),
            Some(PathBuf::from("/h/.npm"))
        );
        assert_eq!(npm_cache_root(&env(&[]), false), None);
        assert_eq!(npm_cache_root(&env(&[("HOME", "")]), false), None);
        // Relative values never resolve (fail-closed).
        assert_eq!(npm_cache_root(&env(&[("HOME", "rel")]), false), None);
        assert_eq!(
            npm_cache_root(&env(&[("npm_config_cache", "rel")]), false),
            None
        );
        // Two spellings that disagree → ambiguous → nothing.
        assert_eq!(
            npm_cache_root(
                &env(&[("npm_config_cache", "/a"), ("NPM_CONFIG_CACHE", "/b")]),
                false
            ),
            None
        );
        // Windows default lives under LOCALAPPDATA, not HOME.
        assert_eq!(
            npm_cache_root(
                &env(&[
                    ("HOME", "/h"),
                    ("LocalAppData", "C:\\Users\\u\\AppData\\Local")
                ]),
                true
            ),
            Some(PathBuf::from("C:\\Users\\u\\AppData\\Local").join("npm-cache"))
        );
        assert_eq!(npm_cache_root(&env(&[("HOME", "/h")]), true), None);
    }

    #[test]
    fn package_names_from_npx_args() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            package_from_args(&a(&[
                "-y",
                "@modelcontextprotocol/server-filesystem@2026.8.31",
                "/r"
            ])),
            Some("@modelcontextprotocol/server-filesystem".into())
        );
        assert_eq!(
            package_from_args(&a(&["--yes", "@scope/pkg", "/r"])),
            Some("@scope/pkg".into())
        );
        assert_eq!(
            package_from_args(&a(&["-y", "pkg@1.2.3"])),
            Some("pkg".into())
        );
        assert_eq!(
            package_from_args(&a(&["-p", "@s/p@1", "bin"])),
            Some("@s/p".into())
        );
        assert_eq!(
            package_from_args(&a(&["--package=pkg@1", "bin"])),
            Some("pkg".into())
        );
        assert_eq!(package_from_args(&a(&["-y"])), None);
    }

    #[test]
    fn npx_command_detection() {
        assert!(is_npx_command("npx"));
        assert!(is_npx_command("/usr/local/bin/npx"));
        assert!(is_npx_command("npx.cmd"));
        assert!(is_npx_command("NPX.CMD"));
        assert!(!is_npx_command("node"));
        assert!(!is_npx_command("npx-wrapper"));
        assert!(!is_npx_command("python3"));
    }

    const PKG: &str = "@modelcontextprotocol/server-filesystem";
    const HASH: &str = "0123456789abcdef";

    /// `<tmp>/cache/_npx/<HASH>/package.json` depending on `pkg`.
    fn make_entry(root: &Path, pkg: &str) -> PathBuf {
        let entry = root.join("_npx").join(HASH);
        std::fs::create_dir_all(entry.join("node_modules")).expect("mkdir");
        std::fs::write(
            entry.join("package.json"),
            serde_json::json!({"dependencies": {pkg: "^2026.8.31"}}).to_string(),
        )
        .expect("write");
        entry
    }

    #[test]
    fn accepts_exactly_the_entry_of_the_configured_package() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("cache");
        let entry = make_entry(&root, PKG);
        let got = validate_candidate(&root, &entry, PKG).expect("valid entry");
        assert_eq!(got, std::fs::canonicalize(&entry).expect("canon"));
    }

    #[test]
    fn rejects_everything_else() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("cache");
        let entry = make_entry(&root, PKG);

        // Another package's entry.
        assert_eq!(
            validate_candidate(&root, &entry, "@other/pkg"),
            Err(RecoveryRefusal::PackageMismatch)
        );
        // `_npx` itself, the cache root, a non-hex name.
        assert_eq!(
            validate_candidate(&root, &root.join("_npx"), PKG),
            Err(RecoveryRefusal::NotAnNpxEntry)
        );
        assert_eq!(
            validate_candidate(&root, &root, PKG),
            Err(RecoveryRefusal::NotAnNpxEntry)
        );
        assert_eq!(
            validate_candidate(&root, &root.join("_npx").join("not-a-hash-xxxxx"), PKG),
            Err(RecoveryRefusal::NotAnNpxEntry)
        );
        // Traversal.
        let trav = root.join("_npx").join(HASH).join("..").join(HASH);
        assert_eq!(
            validate_candidate(&root, &trav, PKG),
            Err(RecoveryRefusal::Traversal)
        );
        // Same shape, outside the cache root.
        let victim_root = tmp.path().join("victim");
        let victim = make_entry(&victim_root, PKG);
        assert_eq!(
            validate_candidate(&root, &victim, PKG),
            Err(RecoveryRefusal::OutsideCache)
        );
        assert!(victim.exists());
        // Missing package.json.
        std::fs::remove_file(entry.join("package.json")).expect("rm");
        assert_eq!(
            validate_candidate(&root, &entry, PKG),
            Err(RecoveryRefusal::PackageJsonUnreadable)
        );
        // Unresolvable cache root.
        assert_eq!(
            validate_candidate(&tmp.path().join("nope"), &entry, PKG),
            Err(RecoveryRefusal::CacheRootUnresolved)
        );
    }

    #[cfg(unix)]
    #[test]
    fn never_follows_a_symlinked_entry_or_npx_dir() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().expect("tempdir");

        // Entry is a symlink to a victim dir that looks exactly right.
        let victim = make_entry(&tmp.path().join("victim"), PKG);
        let root = tmp.path().join("cache");
        std::fs::create_dir_all(root.join("_npx")).expect("mkdir");
        let link = root.join("_npx").join(HASH);
        symlink(&victim, &link).expect("symlink");
        assert_eq!(
            validate_candidate(&root, &link, PKG),
            Err(RecoveryRefusal::EntryMissingOrSymlink)
        );
        assert!(victim.join("package.json").exists(), "victim survives");

        // `_npx` itself is a symlink.
        let root2 = tmp.path().join("cache2");
        std::fs::create_dir_all(&root2).expect("mkdir");
        symlink(tmp.path().join("victim").join("_npx"), root2.join("_npx")).expect("symlink");
        assert_eq!(
            validate_candidate(&root2, &root2.join("_npx").join(HASH), PKG),
            Err(RecoveryRefusal::NpxDirNotARealDirectory)
        );

        // package.json is a symlink (to a file that would pass).
        let root3 = tmp.path().join("cache3");
        let entry3 = make_entry(&root3, PKG);
        std::fs::remove_file(entry3.join("package.json")).expect("rm");
        symlink(victim.join("package.json"), entry3.join("package.json")).expect("symlink");
        assert_eq!(
            validate_candidate(&root3, &entry3, PKG),
            Err(RecoveryRefusal::PackageJsonUnreadable)
        );

        // A candidate reached through a symlinked parent resolves elsewhere.
        let root4 = tmp.path().join("cache4");
        make_entry(&root4, PKG);
        let alias = tmp.path().join("alias");
        symlink(&root4, &alias).expect("symlink");
        let via_alias = alias.join("_npx").join(HASH);
        // The canonical form equals the real entry, so it is the same dir —
        // accepted, and the path returned is the canonical one, not the alias.
        let got = validate_candidate(&root4, &via_alias, PKG).expect("same dir");
        assert_eq!(
            got,
            std::fs::canonicalize(root4.join("_npx").join(HASH)).expect("canon")
        );
    }

    #[test]
    fn stderr_tail_keeps_only_the_last_bytes() {
        let mut t = StderrTail::new(8);
        t.push(b"abc");
        t.push(b"defghij");
        assert_eq!(t.snapshot(), "cdefghij");
        t.push(b"0123456789XYZ");
        assert_eq!(t.snapshot(), "56789XYZ");
    }
}
