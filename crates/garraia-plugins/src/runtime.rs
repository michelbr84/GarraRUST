use crate::manifest::PluginManifest;
use crate::traits::{Capability, Plugin, PluginInput, PluginOutput};
use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};
// GAR-454 (sessão `purrfect-lantern`, 2026-04-27): wasmtime 28 → 44 bump.
// The `preview1` module was renamed to `p1` in wasmtime-wasi 30+ (Preview 1
// is now referenced by its protocol version short name `p1`). The functional
// surface — `add_to_linker_async`, `WasiP1Ctx`, `build_p1()` — is identical;
// only the path changed. `build_p1()` is feature-gated under `features = ["p1"]`
// declared in the workspace Cargo.toml since wasmtime-wasi 44.
use wasmtime_wasi::p1::{self, WasiP1Ctx};
// 2026-08-29 (PR #848, Dependabot): wasmtime-wasi 47 → 48. `DirPerms` and
// `FilePerms` bitflags were collapsed into a single `FsPerms { ReadOnly,
// ReadWrite }` enum, and `preopened_dir` lost its fourth argument
// accordingly. `WasiCtxBuilder` still lives at the crate root (shared by the
// p1 and p2 contexts). `SocketAddrUse` and the `pipe::*` types moved into
// `wasmtime_wasi::p2::*` in v30+ when WASIp2 became the canonical namespace.
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::sockets::SocketAddrUse;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

pub struct WasmRuntime {
    manifest: PluginManifest,
    engine: Engine,
    module: Module,
    plugin_root: PathBuf,
    ticker_handle: tokio::task::JoinHandle<()>,
}

struct WasmState {
    ctx: WasiP1Ctx,
    limits: StoreLimits,
}

impl Drop for WasmRuntime {
    fn drop(&mut self) {
        self.ticker_handle.abort();
    }
}

impl WasmRuntime {
    pub fn new(manifest: PluginManifest, wasm_path: PathBuf) -> Result<Self> {
        let mut config = Config::new();
        // GAR-454: `Config::async_support(true)` is deprecated in wasmtime 44+
        // ("no longer has any effect" — async support is enabled implicitly
        // by the async APIs we use such as `Linker::instantiate_async` and
        // `Func::call_async`). Removed to avoid the deprecation warning.
        config.epoch_interruption(true);

        let engine =
            Engine::new(&config).map_err(|e| Error::Plugin(format!("engine error: {e}")))?;

        let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| {
            Error::Plugin(format!("failed to read wasm {}: {e}", wasm_path.display()))
        })?;
        let module = Module::new(&engine, &wasm_bytes)
            .map_err(|e| Error::Plugin(format!("module error: {e}")))?;
        let plugin_root = wasm_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let ticker_engine = engine.clone();
        let ticker_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                ticker_engine.increment_epoch();
            }
        });

        Ok(Self {
            manifest,
            engine,
            module,
            plugin_root,
            ticker_handle,
        })
    }

    fn configure_filesystem(&self, builder: &mut WasiCtxBuilder) -> Result<()> {
        let read_paths = &self.manifest.permissions.filesystem_read_paths;
        let write_paths = &self.manifest.permissions.filesystem_write_paths;

        if !self.manifest.permissions.filesystem {
            if !read_paths.is_empty() || !write_paths.is_empty() {
                return Err(Error::Plugin(
                    "filesystem paths were provided but filesystem=false in plugin permissions"
                        .to_string(),
                ));
            }
            return Ok(());
        }

        // Filesystem enabled: always scope access to explicit preopened dirs.
        // If none are configured, default to plugin root as read-only.
        let effective_read_paths = if read_paths.is_empty() && write_paths.is_empty() {
            vec![self.plugin_root.display().to_string()]
        } else {
            read_paths.clone()
        };

        let mut mounts: BTreeMap<PathBuf, bool> = BTreeMap::new();
        for raw in effective_read_paths {
            let host_path = normalize_scoped_path(&self.plugin_root, &raw, false)?;
            mounts.entry(host_path).or_insert(false);
        }
        for raw in write_paths {
            let host_path = normalize_scoped_path(&self.plugin_root, raw, true)?;
            mounts.insert(host_path, true);
        }

        for (idx, (host_path, writable)) in mounts.into_iter().enumerate() {
            let guest_path = format!("mnt{idx}");
            // wasmtime-wasi 48 folded the dir/file bitflag pair into one enum:
            // `READ` alone is `ReadOnly`, `READ | MUTATE` plus `READ | WRITE`
            // is `ReadWrite`. Same intent, fewer ways to get it wrong.
            let fs_perms = if writable {
                FsPerms::ReadWrite
            } else {
                FsPerms::ReadOnly
            };

            builder
                .preopened_dir(&host_path, &guest_path, fs_perms)
                .map_err(|e| {
                    Error::Plugin(format!(
                        "failed to preopen filesystem path {}: {e}",
                        host_path.display()
                    ))
                })?;
        }

        Ok(())
    }

    fn configure_network(&self, builder: &mut WasiCtxBuilder) -> Result<()> {
        if self.manifest.permissions.network.is_empty() {
            return Ok(());
        }

        let allowed_ips = Arc::new(resolve_allowlisted_ips(&self.manifest.permissions.network)?);
        builder.allow_ip_name_lookup(true);
        builder.allow_tcp(true);
        builder.allow_udp(true);
        builder.socket_addr_check(move |addr, reason| {
            let allowed_ips = Arc::clone(&allowed_ips);
            Box::pin(async move { socket_use_allowed(reason, addr, &allowed_ips) })
        });

        Ok(())
    }
}

#[async_trait]
impl Plugin for WasmRuntime {
    fn name(&self) -> &str {
        &self.manifest.plugin.name
    }

    fn description(&self) -> &str {
        &self.manifest.plugin.description
    }

    fn capabilities(&self) -> Vec<Capability> {
        let mut caps = Vec::new();
        if self.manifest.permissions.filesystem {
            caps.push(Capability::Filesystem {
                read_paths: self.manifest.permissions.filesystem_read_paths.clone(),
                write_paths: self.manifest.permissions.filesystem_write_paths.clone(),
            });
        }
        if !self.manifest.permissions.network.is_empty() {
            caps.push(Capability::Network(
                self.manifest.permissions.network.clone(),
            ));
        }
        if !self.manifest.permissions.env_vars.is_empty() {
            caps.push(Capability::EnvVars(
                self.manifest.permissions.env_vars.clone(),
            ));
        }
        caps
    }

    async fn execute(&self, input: PluginInput) -> Result<PluginOutput> {
        let mut linker = Linker::new(&self.engine);
        p1::add_to_linker_async(&mut linker, |s: &mut WasmState| &mut s.ctx)
            .map_err(|e| Error::Plugin(format!("linker error: {e}")))?;

        let mut builder = WasiCtxBuilder::new();
        builder.args(&input.args);
        self.configure_filesystem(&mut builder)?;
        self.configure_network(&mut builder)?;

        for (k, v) in &input.env {
            if self.manifest.permissions.env_vars.contains(k) {
                builder.env(k, v);
            }
        }

        // Output capture via bounded pipes.
        let max_output_bytes = self.manifest.limits.max_output_bytes.max(1);
        let stdout = MemoryOutputPipe::new(max_output_bytes);
        let stderr = MemoryOutputPipe::new(max_output_bytes);
        builder.stdout(stdout.clone());
        builder.stderr(stderr.clone());

        // Input
        if !input.stdin.is_empty() {
            let stdin = MemoryInputPipe::new(input.stdin.clone());
            builder.stdin(stdin);
        }

        let ctx = builder.build_p1();
        let max_memory_bytes = self
            .manifest
            .limits
            .max_memory_mb
            .saturating_mul(1024 * 1024)
            .min(usize::MAX as u64) as usize;
        let limits = StoreLimitsBuilder::new()
            .memory_size(max_memory_bytes)
            .build();

        let state = WasmState { ctx, limits };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);

        // Timeout
        // We set the deadline to the current engine epoch + timeout_secs.
        // The background ticker increments the epoch every second.
        let timeout_secs = self.manifest.limits.timeout_secs.max(1);
        store.set_epoch_deadline(timeout_secs);

        let instance = linker
            .instantiate_async(&mut store, &self.module)
            .await
            .map_err(|e| Error::Plugin(format!("instantiation error: {e}")))?;

        let func = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(|e| Error::Plugin(format!("missing _start: {e}")))?;

        let res = func.call_async(&mut store, ()).await;

        let stdout_data = stdout.contents().into();
        let stderr_data = stderr.contents().into();

        let status = match res {
            Ok(_) => 0,
            Err(e) => {
                let root = e.root_cause().to_string();
                if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                    exit.0
                } else if root.contains("interrupted") {
                    return Err(Error::Plugin("execution timed out".into()));
                } else if root.contains("write beyond capacity of MemoryOutputPipe") {
                    return Err(Error::Plugin(format!(
                        "plugin output exceeded limit ({} bytes per stream)",
                        max_output_bytes
                    )));
                } else {
                    return Err(Error::Plugin(format!("execution error: {e}")));
                }
            }
        };

        Ok(PluginOutput {
            stdout: stdout_data,
            stderr: stderr_data,
            status,
        })
    }
}

fn normalize_scoped_path(
    plugin_root: &Path,
    raw: &str,
    create_if_missing: bool,
) -> Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Error::Plugin(
            "filesystem path entries cannot be empty".to_string(),
        ));
    }

    let path = PathBuf::from(raw);

    // Reject absolute paths — plugins must stay within their root.
    if path.is_absolute() {
        return Err(Error::Plugin(format!(
            "absolute filesystem paths are not allowed in plugin permissions: {}",
            path.display()
        )));
    }

    let path = plugin_root.join(path);

    if create_if_missing {
        std::fs::create_dir_all(&path).map_err(|e| {
            Error::Plugin(format!(
                "failed to create writable filesystem path {}: {e}",
                path.display()
            ))
        })?;
    }
    if !path.exists() {
        return Err(Error::Plugin(format!(
            "filesystem path does not exist: {}",
            path.display()
        )));
    }
    let canonical = path.canonicalize().map_err(|e| {
        Error::Plugin(format!(
            "failed to canonicalize path {}: {e}",
            path.display()
        ))
    })?;

    // Boundary check: the canonical path must still be inside plugin_root.
    let canonical_root = plugin_root.canonicalize().map_err(|e| {
        Error::Plugin(format!(
            "failed to canonicalize plugin root {}: {e}",
            plugin_root.display()
        ))
    })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(Error::Plugin(format!(
            "path escapes plugin root: {} is outside {}",
            canonical.display(),
            canonical_root.display()
        )));
    }

    Ok(canonical)
}

/// Returns `true` if the IP address is private, loopback, or link-local.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()                          // 127.0.0.0/8
                || v4.octets()[0] == 10                // 10.0.0.0/8
                || (v4.octets()[0] == 172 && (16..=31).contains(&v4.octets()[1])) // 172.16.0.0/12
                || (v4.octets()[0] == 192 && v4.octets()[1] == 168)              // 192.168.0.0/16
                || (v4.octets()[0] == 169 && v4.octets()[1] == 254)              // 169.254.0.0/16
                || v4.is_unspecified() // 0.0.0.0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()                           // ::1
                || v6.is_unspecified()                 // ::
                || v6.segments()[0] & 0xfe00 == 0xfc00 // fc00::/7 (unique local)
                || v6.segments()[0] & 0xffc0 == 0xfe80 // fe80::/10 (link-local)
        }
    }
}

fn resolve_allowlisted_ips(domains: &[String]) -> Result<HashSet<IpAddr>> {
    let mut ips = HashSet::new();
    for domain in domains {
        let domain = domain.trim();
        if domain.is_empty() {
            continue;
        }

        let query = format!("{domain}:0");
        let resolved = query.to_socket_addrs().map_err(|e| {
            Error::Plugin(format!(
                "failed to resolve allowlisted domain '{domain}': {e}"
            ))
        })?;

        let mut resolved_any = false;
        for addr in resolved {
            if is_private_ip(&addr.ip()) {
                return Err(Error::Plugin(format!(
                    "allowlisted domain '{domain}' resolved to private/loopback address {}, \
                     which is blocked to prevent SSRF",
                    addr.ip()
                )));
            }
            ips.insert(addr.ip());
            resolved_any = true;
        }
        if !resolved_any {
            return Err(Error::Plugin(format!(
                "allowlisted domain '{domain}' resolved to no addresses"
            )));
        }
    }

    if ips.is_empty() {
        return Err(Error::Plugin(
            "network permission enabled but no allowlisted domains were resolved".to_string(),
        ));
    }

    Ok(ips)
}

/// Decides whether a sandboxed plugin may use `addr` for `reason`.
///
/// The policy is: a plugin may reach out to addresses its manifest
/// allowlisted, and may never act as a server.
///
/// The bind arms deserve an explanation, because the obvious `false` is wrong
/// on wasmtime-wasi 48. When a guest calls `connect` on a socket it never
/// bound, v48 asks this check about an *implicit* bind to the wildcard
/// address before it asks about the connect itself (`sockets/tcp.rs:208` and
/// `sockets/udp.rs:123` in 48.0.1). Refusing every bind therefore refuses
/// every outbound connection, turning the allowlist into a silent deny-all.
/// v47 had no such implicit check, which is why the port is not mechanical.
///
/// So we permit exactly the shape wasmtime uses for an implicit bind —
/// `0.0.0.0:0` or `[::]:0` (`sockets/mod.rs:573-579`) — and keep refusing an
/// explicit bind to a concrete local address. Listening and accepting, the
/// operations that actually expose a plugin to the network, stay refused
/// outright.
///
/// The match is deliberately exhaustive with no `_` arm: a variant added by a
/// future wasmtime should fail the build and force a decision here, rather
/// than silently inherit network access.
fn socket_use_allowed(
    reason: SocketAddrUse,
    addr: std::net::SocketAddr,
    allowed_ips: &HashSet<IpAddr>,
) -> bool {
    match reason {
        // Outbound traffic — gated by the manifest allowlist.
        SocketAddrUse::TcpConnect | SocketAddrUse::UdpSend | SocketAddrUse::UdpReceive => {
            allowed_ips.contains(&addr.ip())
        }

        // Implicit bind only (see above); an explicit bind stays refused.
        SocketAddrUse::TcpBind | SocketAddrUse::UdpBind => {
            addr.ip().is_unspecified() && addr.port() == 0
        }

        // A plugin is never a server.
        SocketAddrUse::TcpListen | SocketAddrUse::TcpAccept => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_private_ip, normalize_scoped_path, resolve_allowlisted_ips, socket_use_allowed,
    };
    use std::net::IpAddr;
    use std::path::Path;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "garraia-plugin-test-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn resolve_allowlisted_ips_rejects_localhost() {
        let result = resolve_allowlisted_ips(&["localhost".to_string()]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("private/loopback") || err.contains("SSRF"));
    }

    #[test]
    fn normalize_scoped_path_creates_writable_path() {
        let root = temp_root("writable");
        let scoped = normalize_scoped_path(Path::new(&root), "rw-data", true).unwrap();
        assert!(scoped.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn path_traversal_blocked() {
        let root = temp_root("traversal");
        let result = normalize_scoped_path(&root, "../../../etc/passwd", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("escapes plugin root") || err.contains("does not exist"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn absolute_path_blocked() {
        let root = temp_root("absolute");

        // Use a platform-specific absolute path to ensure `is_absolute()` returns true
        let abs_path = if cfg!(windows) {
            "C:\\Windows\\System32"
        } else {
            "/etc/passwd"
        };

        let result = normalize_scoped_path(&root, abs_path, false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("absolute filesystem paths are not allowed"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn private_ip_rejected() {
        assert!(is_private_ip(&"127.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip(&"10.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip(&"169.254.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip(&"::1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip(&"fd00::1".parse::<IpAddr>().unwrap()));
        assert!(is_private_ip(&"fe80::1".parse::<IpAddr>().unwrap()));
        // Public IP should NOT be private
        assert!(!is_private_ip(&"8.8.8.8".parse::<IpAddr>().unwrap()));
        assert!(!is_private_ip(
            &"2001:4860:4860::8888".parse::<IpAddr>().unwrap()
        ));
    }

    // ── socket_use_allowed (wasmtime-wasi 48 policy) ──────────────────────
    //
    // These pin the semantics of the 47 → 48 migration. The bind cases are
    // the ones that matter: on 48 a `connect` on an unbound socket is checked
    // as an implicit wildcard bind *first*, so a blanket `false` on the bind
    // arms would silently refuse every outbound connection.

    use super::SocketAddrUse;
    use std::collections::HashSet;
    use std::net::SocketAddr;

    fn allowlist() -> HashSet<IpAddr> {
        let mut set = HashSet::new();
        set.insert("93.184.216.34".parse::<IpAddr>().unwrap());
        set
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn outbound_to_allowlisted_ip_is_permitted() {
        let allowed = allowlist();
        for reason in [
            SocketAddrUse::TcpConnect,
            SocketAddrUse::UdpSend,
            SocketAddrUse::UdpReceive,
        ] {
            assert!(
                socket_use_allowed(reason, addr("93.184.216.34:443"), &allowed),
                "allowlisted peer must be reachable"
            );
        }
    }

    #[test]
    fn outbound_to_unlisted_ip_is_refused() {
        let allowed = allowlist();
        for reason in [
            SocketAddrUse::TcpConnect,
            SocketAddrUse::UdpSend,
            SocketAddrUse::UdpReceive,
        ] {
            assert!(
                !socket_use_allowed(reason, addr("1.1.1.1:443"), &allowed),
                "peer outside the manifest allowlist must be refused"
            );
        }
    }

    #[test]
    fn implicit_wildcard_bind_is_permitted() {
        // wasmtime 48 passes SocketAddr::new(UNSPECIFIED, 0) for the implicit
        // bind it checks before TcpConnect / UdpSend. Refusing this would
        // break every outbound connection, so it must be allowed.
        let allowed = allowlist();
        for reason in [SocketAddrUse::TcpBind, SocketAddrUse::UdpBind] {
            assert!(
                socket_use_allowed(reason, addr("0.0.0.0:0"), &allowed),
                "implicit IPv4 bind precedes every outbound connect"
            );
            assert!(
                socket_use_allowed(reason, addr("[::]:0"), &allowed),
                "implicit IPv6 bind precedes every outbound connect"
            );
        }
    }

    #[test]
    fn explicit_bind_to_a_concrete_address_is_refused() {
        let allowed = allowlist();
        for reason in [SocketAddrUse::TcpBind, SocketAddrUse::UdpBind] {
            assert!(
                !socket_use_allowed(reason, addr("127.0.0.1:8080"), &allowed),
                "binding a concrete local address is not the implicit form"
            );
            // Wildcard IP but a fixed port is still an explicit bind.
            assert!(
                !socket_use_allowed(reason, addr("0.0.0.0:8080"), &allowed),
                "a pinned port means the guest asked for this bind"
            );
        }
    }

    #[test]
    fn listening_and_accepting_are_always_refused() {
        let allowed = allowlist();
        // Even an allowlisted address must not let a plugin serve.
        for reason in [SocketAddrUse::TcpListen, SocketAddrUse::TcpAccept] {
            assert!(!socket_use_allowed(
                reason,
                addr("93.184.216.34:443"),
                &allowed
            ));
            assert!(!socket_use_allowed(reason, addr("0.0.0.0:0"), &allowed));
        }
    }
}
