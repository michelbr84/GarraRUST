//! Issue #1346: a corrupted `npx` cache entry must be cleared once, safely,
//! and an exhausted server must stop spamming the log.
//!
//! Drives a real child: `tests/fixtures/fake_npx.py`, copied into a temp dir
//! under the name `npx`, which behaves like npx over a fake npm cache in
//! `<tmp>/home/.npm/_npx/0123456789abcdef` and execs `fake_mcp_server.py`
//! once the entry is complete. Every test has an outer timeout.
#![cfg(all(unix, feature = "mcp"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use garraia_agents::{McpFailureCause, McpManager, McpServerState};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

const HASH: &str = "0123456789abcdef";
const PKG: &str = "@modelcontextprotocol/server-filesystem";

/// Records `(level, message)` of every event.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<(Level, String)>>>);

struct MsgVisitor(String);
impl Visit for MsgVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: Subscriber> Layer<S> for Recorder {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut v = MsgVisitor(String::new());
        event.record(&mut v);
        if let Ok(mut g) = self.0.lock() {
            g.push((*event.metadata().level(), v.0));
        }
    }
}

impl Recorder {
    fn count(&self, level: Level, needle: &str) -> usize {
        self.0
            .lock()
            .map(|g| {
                g.iter()
                    .filter(|(l, m)| *l == level && m.contains(needle))
                    .count()
            })
            .unwrap_or(0)
    }
    fn any_at_or_above_warn(&self, needle: &str) -> bool {
        self.0
            .lock()
            .map(|g| {
                g.iter()
                    .any(|(l, m)| *l <= Level::WARN && m.contains(needle))
            })
            .unwrap_or(false)
    }
}

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    npx: String,
    log: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        Self::with_home("home")
    }

    /// `home` is the directory name used as `$HOME` (it may contain spaces).
    fn with_home(home: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir bin");
        std::fs::create_dir_all(root.join(home)).expect("mkdir home");
        let npx = bin.join("npx");
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_npx.py"),
            &npx,
        )
        .expect("copy fake npx");
        std::fs::set_permissions(&npx, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        Self {
            home: root.join(home),
            log: root.join("npx.log"),
            npx: npx.to_string_lossy().into_owned(),
            root,
            _tmp: tmp,
        }
    }

    fn home(&self) -> PathBuf {
        self.home.clone()
    }

    fn entry(&self) -> PathBuf {
        self.home().join(".npm").join("_npx").join(HASH)
    }

    fn env(&self, extra: &[(&str, &str)]) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = [
            ("HOME", self.home().to_string_lossy().into_owned()),
            (
                "FAKE_MCP_SERVER",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/fake_mcp_server.py"
                )
                .into(),
            ),
            ("FAKE_NPX_LOG", self.log.to_string_lossy().into_owned()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        for (k, v) in extra {
            env.insert(k.to_string(), v.to_string());
        }
        env
    }

    fn invocations(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    /// A half-installed entry: package.json present, module missing, plus a
    /// sentinel so the test can tell whether the dir was removed.
    fn corrupt_entry_at(dir: &Path) {
        std::fs::create_dir_all(dir.join("node_modules").join("zod")).expect("mkdir");
        std::fs::write(
            dir.join("package.json"),
            serde_json::json!({"dependencies": {PKG: "^2026.8.31"}}).to_string(),
        )
        .expect("write package.json");
        std::fs::write(dir.join("SENTINEL"), b"x").expect("sentinel");
    }
}

fn args() -> Vec<String> {
    vec!["-y".into(), format!("{PKG}@2026.8.31"), "/tmp".into()]
}

async fn connect(
    m: &McpManager,
    sb: &Sandbox,
    name: &str,
    env: &HashMap<String, String>,
) -> garraia_common::Result<()> {
    m.connect(name, &sb.npx, &args(), env, 10, vec![], None, 5, 1, false)
        .await
}

fn timeout<F: std::future::Future>(f: F) -> tokio::time::Timeout<F> {
    tokio::time::timeout(Duration::from_secs(60), f)
}

/// Acceptance (1) + (2): the corrupt entry is removed exactly once, the
/// connect retries once and succeeds; a second corruption in the same
/// process is NOT cleared again and fails with a classified cause, which
/// `server_statuses` reports for the pending server.
#[tokio::test]
async fn corrupt_npx_entry_is_cleared_once_then_reported() {
    let rec = Recorder::default();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(rec.clone()));
    timeout(async {
        let sb = Sandbox::new();
        Sandbox::corrupt_entry_at(&sb.entry());
        let env = sb.env(&[]);
        let m = Arc::new(McpManager::new());

        connect(&m, &sb, "filesystem", &env)
            .await
            .expect("connect must succeed after one cache clear");
        assert!(m.is_connected("filesystem").await);
        assert_eq!(sb.invocations(), 2, "one failed spawn + exactly one retry");
        assert!(
            !sb.entry().join("SENTINEL").exists(),
            "the corrupt entry must have been removed"
        );
        assert!(
            sb.entry().join("package.json").exists(),
            "reinstalled by npx"
        );
        // The child's stack trace never reaches warn/error.
        assert!(!rec.any_at_or_above_warn("finalizeResolution"));
        assert!(!rec.any_at_or_above_warn("Node.js v22"));
        assert!(
            rec.count(Level::DEBUG, "ERR_MODULE_NOT_FOUND") >= 1,
            "stderr is forwarded at debug level"
        );

        // Second corruption, same process: no second clear.
        m.disconnect("filesystem").await;
        Sandbox::corrupt_entry_at(&sb.entry());
        std::fs::remove_file(sb.entry().join("node_modules/zod/v4/mini/external.js"))
            .expect("break it again");
        let err = connect(&m, &sb, "filesystem", &env)
            .await
            .expect_err("second corruption must not be auto-cleared");
        assert!(
            err.to_string().contains("npx cache entry"),
            "error names the classified cause: {err}"
        );
        assert!(
            sb.entry().join("SENTINEL").exists(),
            "one-shot: the entry survives the second failure"
        );
        assert_eq!(sb.invocations(), 3, "no retry on the second failure");

        // Boot parks it in pending; health shows it with the cause.
        m.register_pending_stdio(
            "filesystem",
            &sb.npx,
            &args(),
            &env,
            10,
            vec![],
            None,
            5,
            1,
            false,
        )
        .await;
        let st = m.server_statuses().await;
        let fs = st
            .iter()
            .find(|s| s.name == "filesystem")
            .expect("pending server listed");
        assert_eq!(fs.state, McpServerState::Retrying);
        assert_eq!(
            fs.cause.as_ref().map(|c| c.as_str()),
            Some("npx_cache_corrupt")
        );
        let dir = fs.cause.as_ref().and_then(McpFailureCause::npx_dir);
        assert!(dir.is_some_and(|d| d.ends_with(format!("_npx/{HASH}"))));
        assert!(fs.last_error.as_deref().is_some_and(|e| e.len() <= 200));

        // A manual admin restart re-arms the recovery.
        m.reset_restart_state("filesystem").await;
        connect(&m, &sb, "filesystem", &env)
            .await
            .expect("after a manual restart the recovery runs again");
        m.disconnect_all().await;
    })
    .await
    .expect("test must not hang");
}

/// Acceptance (3): stderr naming a directory OUTSIDE the resolved npm cache
/// is never deleted, even when it looks exactly like an npx entry of the
/// configured package.
#[tokio::test]
async fn stderr_pointing_outside_the_cache_deletes_nothing() {
    timeout(async {
        let sb = Sandbox::new();
        let victim = sb.root.join("victim").join("_npx").join(HASH);
        Sandbox::corrupt_entry_at(&victim);
        let victim_s = victim.to_string_lossy().into_owned();
        let env = sb.env(&[
            ("FAKE_NPX_MODE", "point-at"),
            ("FAKE_NPX_POINT_AT", &victim_s),
        ]);
        // The real cache exists too, so only the path check can refuse.
        std::fs::create_dir_all(sb.home().join(".npm").join("_npx")).expect("mkdir");
        let m = McpManager::new();

        let err = connect(&m, &sb, "filesystem", &env)
            .await
            .expect_err("must fail");
        assert!(err.to_string().contains("npx cache entry"), "{err}");
        assert!(victim.join("SENTINEL").exists(), "victim must survive");
        assert_eq!(sb.invocations(), 1, "no retry when nothing was cleared");

        // Review MCP-1/3/4: the path the child made up reaches neither the
        // error (which becomes `last_error` on the auth-free health endpoint)
        // nor the cause (which diagnostics turns into "delete this
        // directory"): the gateway refused it, so it does not repeat it.
        assert!(!err.to_string().contains("victim"), "{err}");
        m.register_pending_stdio(
            "filesystem",
            &sb.npx,
            &args(),
            &env,
            10,
            vec![],
            None,
            5,
            1,
            false,
        )
        .await;
        let st = m.server_statuses().await;
        let fs = st
            .iter()
            .find(|s| s.name == "filesystem")
            .expect("pending server listed");
        assert_eq!(
            fs.cause,
            Some(McpFailureCause::NpxCacheCorrupt { dir: None }),
            "an entry the gateway refused is never reported as the corrupt one"
        );
        assert!(
            fs.last_error
                .as_deref()
                .is_some_and(|e| !e.contains("victim") && !e.contains("_npx")),
            "{:?}",
            fs.last_error
        );
    })
    .await
    .expect("test must not hang");
}

/// A symlinked cache entry is never followed: the victim it points at
/// survives, and nothing is retried.
#[tokio::test]
async fn symlinked_entry_is_never_followed() {
    timeout(async {
        let sb = Sandbox::new();
        let victim = sb.root.join("victim").join("_npx").join(HASH);
        Sandbox::corrupt_entry_at(&victim);
        std::fs::create_dir_all(sb.home().join(".npm").join("_npx")).expect("mkdir");
        std::os::unix::fs::symlink(&victim, sb.entry()).expect("symlink");
        let env = sb.env(&[]);
        let m = McpManager::new();

        connect(&m, &sb, "filesystem", &env)
            .await
            .expect_err("must fail");
        assert!(victim.join("SENTINEL").exists(), "victim must survive");
        assert!(
            std::fs::symlink_metadata(sb.entry())
                .map(|md| md.file_type().is_symlink())
                .unwrap_or(false),
            "the symlink itself is left alone"
        );
        assert_eq!(sb.invocations(), 1);
    })
    .await
    .expect("test must not hang");
}

/// Recovery only applies to `npx` itself: the same trace from a command
/// named anything else clears nothing.
#[tokio::test]
async fn non_npx_command_never_clears() {
    timeout(async {
        use std::os::unix::fs::PermissionsExt;
        let sb = Sandbox::new();
        Sandbox::corrupt_entry_at(&sb.entry());
        let other = sb.root.join("bin").join("not-npx");
        std::fs::copy(&sb.npx, &other).expect("copy");
        std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let m = McpManager::new();
        let err = m
            .connect(
                "fs",
                &other.to_string_lossy(),
                &args(),
                &sb.env(&[]),
                10,
                vec![],
                None,
                5,
                1,
                false,
            )
            .await
            .expect_err("must fail");
        assert!(err.to_string().contains("npx cache entry"), "{err}");
        assert!(sb.entry().join("SENTINEL").exists());
    })
    .await
    .expect("test must not hang");
}

/// Acceptance (4): once `max_restarts` is exhausted the error is logged
/// exactly once, however many ticks follow; the server is reported
/// `Failed`; and a manual restart re-arms both retries and the log line.
#[tokio::test]
async fn exhausted_restarts_are_logged_exactly_once() {
    let rec = Recorder::default();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(rec.clone()));
    timeout(async {
        let sb = Sandbox::new();
        let env = sb.env(&[("FAKE_NPX_MODE", "fail")]);
        let m = McpManager::new();
        // max_restarts = 2, base delay 0 → every tick may retry.
        m.register_pending_stdio(
            "broken",
            &sb.npx,
            &args(),
            &env,
            10,
            vec![],
            None,
            2,
            0,
            false,
        )
        .await;

        for _ in 0..8 {
            m.health_tick().await;
        }
        assert_eq!(sb.invocations(), 2, "exactly max_restarts attempts");
        assert_eq!(
            rec.count(Level::ERROR, "max restarts"),
            1,
            "the exhausted error is logged once, not per tick"
        );
        assert_eq!(rec.count(Level::INFO, "waiting for backoff"), 0);
        let st = m.server_statuses().await;
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].state, McpServerState::Failed);
        assert_eq!(st[0].attempts, 2);
        assert_eq!(st[0].cause.as_ref().map(|c| c.as_str()), Some("other"));

        // Manual restart: retries resume, and exhausting again logs again.
        m.reset_restart_state("broken").await;
        for _ in 0..8 {
            m.health_tick().await;
        }
        assert_eq!(sb.invocations(), 4);
        assert_eq!(rec.count(Level::ERROR, "max restarts"), 2);
    })
    .await
    .expect("test must not hang");
}

/// Review MCP-5: a `$HOME` with a space in it (`C:\Users\John Smith` on
/// Windows is the common case) used to cut the stderr path in half, so the
/// entry was never found and the recovery silently never ran. Only the entry
/// hash is read from stderr now; the directory is rebuilt from the cache root.
#[tokio::test]
async fn a_space_in_home_does_not_defeat_the_recovery() {
    timeout(async {
        let sb = Sandbox::with_home("John Smith");
        Sandbox::corrupt_entry_at(&sb.entry());
        let env = sb.env(&[]);
        let m = McpManager::new();

        connect(&m, &sb, "filesystem", &env)
            .await
            .expect("connect must succeed after one cache clear");
        assert_eq!(sb.invocations(), 2, "one failed spawn + exactly one retry");
        assert!(!sb.entry().join("SENTINEL").exists());
        m.disconnect_all().await;
    })
    .await
    .expect("test must not hang");
}
