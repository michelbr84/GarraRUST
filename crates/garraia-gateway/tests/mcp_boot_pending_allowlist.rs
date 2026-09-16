//! Issue #1242, path 2: a *boot* failure must not lose the allowlist either.
//!
//! `bootstrap::build_mcp_tools` parked only **stdio** servers that failed
//! their handshake in `McpManager::pending`. An HTTP server configured with a
//! GAR-190 allowlist whose handshake failed therefore left no trace of that
//! allowlist anywhere: `connections` never got an entry, `pending` was
//! skipped, and the gateway's registry type has no `allowed_tools` field to
//! carry it (issue #1260). The first admin restart of that server resolved
//! `None` and reconnected it with no allowlist at all.
//!
//! This drives the real `build_mcp_tools`, not a reimplementation of it. The
//! binary is declared with `required-features = ["mcp-http"]` in `Cargo.toml`
//! on purpose: the HTTP arm it exercises is `#[cfg(feature = "mcp-http")]`, so
//! without the feature this file would compile to zero tests and report a
//! green run that proved nothing. Cargo now refuses instead.

use std::collections::HashMap;

use garraia_config::AppConfig;

const SERVER: &str = "http-allowlisted";

/// A port nobody is listening on, so the handshake fails for a boring reason
/// (connection refused) rather than a timeout. `connect_http` vets the URL
/// with `IpScope::AllowPrivate`, so loopback reaches the socket.
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

#[tokio::test]
async fn a_failed_http_boot_still_remembers_the_allowlist() {
    // Point the loader at an empty directory so the developer's real
    // `mcp.json` cannot join the run and connect actual servers.
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: this is a dedicated test binary with a single test, so no other
    // thread is reading the environment concurrently.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
    }

    let mut config = AppConfig::default();
    config.mcp.insert(
        SERVER.to_string(),
        garraia_config::McpServerConfig {
            command: String::new(),
            args: vec![],
            env: HashMap::new(),
            transport: "http".to_string(),
            url: Some(format!("http://127.0.0.1:{}/mcp", closed_port())),
            enabled: Some(true),
            timeout: Some(2),
            allowed_tools: vec!["read_file".to_string()],
            memory_limit_mb: None,
            max_restarts: Some(5),
            restart_delay_secs: Some(1),
        },
    );

    let (manager, tools, failures) = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        garraia_gateway::bootstrap::build_mcp_tools(&config),
    )
    .await
    .expect("boot must not hang");

    assert!(
        failures.iter().any(|(name, _)| name == SERVER),
        "precondition: the handshake against a closed port must fail, got: {failures:?}"
    );
    assert!(
        tools.is_empty(),
        "a server that never handshook contributes no tools"
    );

    // The assertion that matters. `Some(vec![])` would be just as wrong as
    // `None` here: it is how `is_tool_allowed` spells "allow everything", and
    // a restart would hand exactly that to the reconnect.
    assert_eq!(
        manager.allowed_tools_for(SERVER).await,
        Some(vec!["read_file".to_string()]),
        "the allowlist of an HTTP server that failed at boot must survive in \
         `pending`, or the first admin restart reconnects it wide open"
    );
}
