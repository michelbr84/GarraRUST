//! Shared SSRF guard for every outbound HTTP request whose URL is influenced
//! by a remote party.
//!
//! # Why this lives in `garraia-common`
//!
//! The pattern was first implemented inline in
//! `garraia-gateway/src/plugins_handler.rs` (GAR-460 / GAR-461) and proved
//! itself there. The 2026-08-29 CodeQL wave — the Rust extractor went from 118
//! to 422 files covered when the runner bundle moved 2.26.3 -> 2.26.4 — showed
//! that the same shape exists in at least four other places, spread across
//! three crates: the skill importer (`garraia-skills`), the `web_fetch` LLM
//! tool and the MCP HTTP transport (`garraia-agents`), and the provider
//! registration handlers (`garraia-gateway`). `rust/request-forgery` has
//! security-severity 9.1, i.e. Critical.
//!
//! `garraia-common` is the one crate all of those already depend on, so the
//! guard lives here rather than being copied per crate. A guard that exists in
//! four slightly different copies is a guard that will drift.
//!
//! # What it defends against
//!
//! 1. **Scheme abuse** — `file://`, `gopher://`, `ftp://` and friends never
//!    reach a client. Only the schemes the caller opts into.
//! 2. **Internal address reach** — the host is resolved *once*, and the request
//!    is refused if *any* resolved address is loopback, RFC 1918, link-local
//!    (which is where cloud instance-metadata lives at `169.254.169.254`),
//!    CGNAT, unique-local, multicast or unspecified. IPv4 and IPv6, including
//!    v4-mapped and the deprecated v4-compatible form.
//! 3. **DNS rebinding** — the vetted addresses are pinned into the client with
//!    [`reqwest::ClientBuilder::resolve_to_addrs`], so `.send()` does not
//!    re-resolve the host and cannot be pointed somewhere else between the
//!    check and the connect.
//! 4. **Redirect laundering** — the built client follows no redirects, so an
//!    allowed host cannot bounce the request to a blocked one.
//! 5. **Unbounded bodies** — [`read_capped`] streams with a hard byte cap.
//!
//! # What it does *not* do
//!
//! It does not authenticate or authorise the caller. Both concerns are
//! independent: `plugins_handler` requires `Permission::ManagePlugins` *and*
//! vets the URL, because an authorised operator should not be able to make the
//! gateway probe its own internal network either.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

/// Why a URL was refused. Callers map these onto their own error type; the
/// `status_hint` distinguishes "you sent something malformed" (400) from "that
/// target is off limits" (403) and "we could not build the client" (502).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsrfRejection {
    /// The string is not a URL at all.
    InvalidUrl(String),
    /// Scheme outside the caller's allowlist (e.g. `file:`, `gopher:`). Carries
    /// the accepted set so the message can name it — operators need to know
    /// what *would* have worked, and callers assert on that text.
    SchemeNotAllowed {
        scheme: String,
        allowed: &'static [&'static str],
    },
    /// URL has no host component (e.g. `file:///etc/passwd`).
    MissingHost,
    /// Host is not in the caller's host allowlist.
    HostNotAllowed(String),
    /// DNS resolution failed or returned nothing.
    ResolveFailed(String),
    /// Host resolves to an address the gateway must not reach.
    BlockedAddress { host: String, ip: IpAddr },
    /// `reqwest` refused to build the client.
    ClientBuild(String),
    /// Response body exceeded the caller's cap.
    BodyTooLarge { cap: usize },
    /// Transport-level failure while streaming the body.
    Transport(String),
}

impl SsrfRejection {
    /// 400 for caller mistakes, 403 for policy refusals, 502 for our own or the
    /// upstream's failure.
    pub fn status_hint(&self) -> u16 {
        match self {
            Self::InvalidUrl(_)
            | Self::SchemeNotAllowed { .. }
            | Self::MissingHost
            | Self::ResolveFailed(_)
            | Self::BodyTooLarge { .. } => 400,
            Self::HostNotAllowed(_) | Self::BlockedAddress { .. } => 403,
            Self::ClientBuild(_) | Self::Transport(_) => 502,
        }
    }
}

impl fmt::Display for SsrfRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(e) => write!(f, "invalid URL: {e}"),
            Self::SchemeNotAllowed { scheme, allowed } => write!(
                f,
                "URL scheme '{scheme}' is not allowed (allowed schemes: {})",
                allowed.join(", ")
            ),
            Self::MissingHost => write!(f, "URL is missing a host"),
            Self::HostNotAllowed(h) => write!(f, "host '{h}' is not in the allowlist"),
            Self::ResolveFailed(e) => write!(f, "DNS resolve failed: {e}"),
            Self::BlockedAddress { host, ip } => write!(
                f,
                "host '{host}' resolves to blocked address {ip} \
                 (loopback/private/link-local/CGNAT/multicast/unspecified)"
            ),
            Self::ClientBuild(e) => write!(f, "HTTP client build failed: {e}"),
            Self::BodyTooLarge { cap } => write!(f, "response body exceeds the {cap} byte cap"),
            Self::Transport(e) => write!(f, "body stream error: {e}"),
        }
    }
}

impl std::error::Error for SsrfRejection {}

/// Which address ranges a call site may reach.
///
/// GarraIA is a local-first gateway: some outbound targets are *supposed* to be
/// on the loopback interface or the LAN — an Ollama server on
/// `http://127.0.0.1:11434`, a self-hosted MCP server on the office network.
/// Blanket-blocking private ranges there would break the product's offline
/// story, so those call sites opt into [`IpScope::AllowPrivate`], which still
/// blocks the ranges that are never a legitimate target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpScope {
    /// Publicly routable addresses only. The default, and the right choice for
    /// anything fetching a resource named by a remote party.
    PublicOnly,
    /// Also permit loopback and private/LAN ranges (RFC 1918, IPv6 unique
    /// local). Link-local — which is where cloud instance metadata lives at
    /// `169.254.169.254` — plus CGNAT, multicast and the unspecified address
    /// stay blocked: none of those is ever a legitimate HTTP target, and
    /// link-local is the single highest-value SSRF prize in a cloud deployment.
    AllowPrivate,
}

/// What a given call site is willing to talk to.
#[derive(Debug, Clone)]
pub struct UrlPolicy {
    /// Schemes accepted, lowercase. `["https"]` for anything crossing the
    /// public internet; `["http", "https"]` where plaintext is a legitimate
    /// operator choice (a self-hosted Ollama or MCP server on the LAN).
    pub allowed_schemes: &'static [&'static str],
    /// Host suffix allow-list. `None` means "any host permitted by `ip_scope`";
    /// `Some(&[])` means "nothing" (a feature disabled by default).
    pub host_allowlist: Option<&'static [&'static str]>,
    /// Address ranges this call site may reach.
    pub ip_scope: IpScope,
    /// Connect + read timeout for the client built from this policy.
    pub timeout: Duration,
    /// `User-Agent` sent upstream.
    pub user_agent: &'static str,
}

impl UrlPolicy {
    /// https-only, any publicly routable host. The default for fetching
    /// attacker-nameable resources off the internet.
    pub const fn https_public(timeout: Duration, user_agent: &'static str) -> Self {
        Self {
            allowed_schemes: &["https"],
            host_allowlist: None,
            ip_scope: IpScope::PublicOnly,
            timeout,
            user_agent,
        }
    }

    /// http+https, any publicly routable host. For call sites where plaintext
    /// is a legitimate operator choice. The IP block still applies, so this is
    /// *not* a way to reach localhost.
    pub const fn http_public(timeout: Duration, user_agent: &'static str) -> Self {
        Self {
            allowed_schemes: &["http", "https"],
            host_allowlist: None,
            ip_scope: IpScope::PublicOnly,
            timeout,
            user_agent,
        }
    }

    /// Restrict to a host suffix allowlist. An empty list denies everything.
    pub const fn with_host_allowlist(mut self, allowlist: &'static [&'static str]) -> Self {
        self.host_allowlist = Some(allowlist);
        self
    }

    /// Widen the address scope. Use only where local targets are the point —
    /// see [`IpScope::AllowPrivate`] for what stays blocked regardless.
    pub const fn with_ip_scope(mut self, scope: IpScope) -> Self {
        self.ip_scope = scope;
        self
    }
}

/// A URL that passed [`vet_url`], together with the addresses it resolved to.
///
/// Holding one of these is the proof obligation: build the client from it with
/// [`pinned_client`] so the connect cannot resolve anywhere else.
#[derive(Debug, Clone)]
pub struct VettedUrl {
    pub url: url::Url,
    pub host: String,
    pub addrs: Vec<SocketAddr>,
}

/// Parse, gate by scheme and host allow-list, resolve once, and refuse if any
/// resolved address is non-public.
///
/// Resolution is synchronous (`ToSocketAddrs`); call sites in async code should
/// treat it as they would any other short blocking syscall, exactly as
/// `plugins_handler` has since GAR-461.
pub fn vet_url(raw: &str, policy: &UrlPolicy) -> Result<VettedUrl, SsrfRejection> {
    let parsed = url::Url::parse(raw).map_err(|e| SsrfRejection::InvalidUrl(e.to_string()))?;

    let scheme = parsed.scheme().to_lowercase();
    if !policy.allowed_schemes.contains(&scheme.as_str()) {
        return Err(SsrfRejection::SchemeNotAllowed {
            scheme,
            allowed: policy.allowed_schemes,
        });
    }

    let host = parsed
        .host_str()
        .ok_or(SsrfRejection::MissingHost)?
        .to_lowercase();
    if host.is_empty() {
        return Err(SsrfRejection::MissingHost);
    }

    if let Some(allowlist) = policy.host_allowlist
        && !host_in_allowlist(&host, allowlist)
    {
        return Err(SsrfRejection::HostNotAllowed(host));
    }

    let port = parsed
        .port_or_known_default()
        .unwrap_or(if scheme == "http" { 80 } else { 443 });
    let addrs = resolve_addrs(&host, port)?;
    validate_addrs_in_scope(&addrs, &host, policy.ip_scope)?;

    Ok(VettedUrl {
        url: parsed,
        host,
        addrs,
    })
}

/// Resolve `host:port`, refusing an empty answer.
pub fn resolve_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, SsrfRejection> {
    let resolved: Vec<SocketAddr> = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| SsrfRejection::ResolveFailed(e.to_string()))?
        .collect();
    if resolved.is_empty() {
        return Err(SsrfRejection::ResolveFailed(format!(
            "host '{host}' resolved to no addresses"
        )));
    }
    Ok(resolved)
}

/// Reject the whole list if *any* address is blocked. All-or-nothing on
/// purpose: a host that resolves to both a public and a private address must
/// not be reachable, because which one the connect picks is not ours to decide.
pub fn validate_addrs(addrs: &[SocketAddr], host: &str) -> Result<(), SsrfRejection> {
    validate_addrs_in_scope(addrs, host, IpScope::PublicOnly)
}

/// [`validate_addrs`] against an explicit [`IpScope`].
pub fn validate_addrs_in_scope(
    addrs: &[SocketAddr],
    host: &str,
    scope: IpScope,
) -> Result<(), SsrfRejection> {
    for addr in addrs {
        if is_blocked_ip_in_scope(&addr.ip(), scope) {
            return Err(SsrfRejection::BlockedAddress {
                host: host.to_string(),
                ip: addr.ip(),
            });
        }
    }
    Ok(())
}

/// Build a client pinned to the vetted addresses, with redirects disabled.
///
/// Pinning closes the TOCTOU window: without `resolve_to_addrs`, reqwest would
/// resolve the host again at `.send()` time and an attacker controlling the
/// resolver could swap the IP between our check and the connect.
pub fn pinned_client(
    vetted: &VettedUrl,
    policy: &UrlPolicy,
) -> Result<reqwest::Client, SsrfRejection> {
    pinned_client_for(&vetted.host, &vetted.addrs, policy)
}

/// [`pinned_client`] for callers that already hold a host and its vetted
/// addresses without a [`VettedUrl`] wrapper.
pub fn pinned_client_for(
    host: &str,
    addrs: &[SocketAddr],
    policy: &UrlPolicy,
) -> Result<reqwest::Client, SsrfRejection> {
    let https_only = policy.allowed_schemes == ["https"];
    reqwest::Client::builder()
        .resolve_to_addrs(host, addrs)
        .timeout(policy.timeout)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(https_only)
        .user_agent(policy.user_agent)
        .build()
        .map_err(|e| SsrfRejection::ClientBuild(e.to_string()))
}

/// Read at most `cap` bytes from a response body, streaming so peak memory
/// stays bounded even when the upstream lies about `Content-Length`.
pub async fn read_capped(
    response: reqwest::Response,
    cap: usize,
) -> Result<Vec<u8>, SsrfRejection> {
    use futures::StreamExt;

    if let Some(len) = response.content_length()
        && (len as usize) > cap
    {
        return Err(SsrfRejection::BodyTooLarge { cap });
    }

    let mut buf: Vec<u8> = Vec::with_capacity(cap.min(8192));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| SsrfRejection::Transport(e.to_string()))?;
        if buf.len() + chunk.len() > cap {
            return Err(SsrfRejection::BodyTooLarge { cap });
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// `true` if `ip` must never be used as an HTTP target from a GarraIA process.
///
/// IPv4 blocked: `127.0.0.0/8` (loopback), `10.0.0.0/8`, `172.16.0.0/12`,
/// `192.168.0.0/16` (RFC 1918), `169.254.0.0/16` (link-local — this is where
/// AWS/GCP instance metadata sits, at `169.254.169.254`), `0.0.0.0/8`
/// (unspecified), `224.0.0.0/4` (multicast), `100.64.0.0/10` (CGNAT).
///
/// IPv6 blocked: `::1/128` (loopback), `::/128` (unspecified), `fc00::/7`
/// (unique local), `fe80::/10` (link-local), `ff00::/8` (multicast),
/// `::ffff:0:0/96` (v4-mapped — the inner v4 is inspected), and the deprecated
/// v4-compatible `::a.b.c.d` form of RFC 4291 §2.5.5.1 (likewise inspected).
pub fn is_blocked_ip(ip: &IpAddr) -> bool {
    is_blocked_ip_in_scope(ip, IpScope::PublicOnly)
}

/// [`is_blocked_ip`] against an explicit [`IpScope`].
///
/// Under [`IpScope::AllowPrivate`] loopback and private/unique-local ranges are
/// permitted; link-local, CGNAT, multicast and unspecified are not.
pub fn is_blocked_ip_in_scope(ip: &IpAddr, scope: IpScope) -> bool {
    let allow_private = matches!(scope, IpScope::AllowPrivate);
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            let never_legitimate = v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0                                  // 0.0.0.0/8
                || (o[0] == 100 && (64..=127).contains(&o[1])); // 100.64.0.0/10 CGNAT
            if never_legitimate {
                return true;
            }
            if allow_private {
                return false;
            }
            v4.is_loopback() || v4.is_private()
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() || v6.is_multicast() {
                return true;
            }
            let segs = v6.segments();
            // fe80::/10 (link-local) — never legitimate, blocked in any scope.
            if segs[0] & 0xffc0 == 0xfe80 {
                return true;
            }
            if v6.is_loopback() {
                return !allow_private;
            }
            // fc00::/7 (unique local) — the IPv6 analogue of RFC 1918.
            if segs[0] & 0xfe00 == 0xfc00 {
                return !allow_private;
            }
            // v4-mapped (::ffff:0:0/96): inspect the inner v4.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_ip_in_scope(&IpAddr::V4(v4), scope);
            }
            // v4-compatible (RFC 4291 §2.5.5.1, deprecated): ::a.b.c.d, high 96
            // bits zero. Distinguished from v4-mapped (handled just above) and
            // from pure ::1 / :: (caught by the loopback/unspecified guards
            // above, which run first).
            if segs[0..6] == [0, 0, 0, 0, 0, 0] {
                let v4 = Ipv4Addr::new(
                    (segs[6] >> 8) as u8,
                    (segs[6] & 0xff) as u8,
                    (segs[7] >> 8) as u8,
                    (segs[7] & 0xff) as u8,
                );
                return is_blocked_ip_in_scope(&IpAddr::V4(v4), scope);
            }
            false
        }
    }
}

/// Suffix-match a host against an allow-list. An empty list denies everything.
/// A suffix matches the host itself or a `.suffix` boundary, so
/// `plugins.example.com` matches `plugins.example.com` and
/// `v2.plugins.example.com`, but not `evilplugins.example.com`.
pub fn host_in_allowlist(host: &str, allowlist: &[&str]) -> bool {
    if allowlist.is_empty() {
        return false;
    }
    let host = host.trim_end_matches('.').to_lowercase();
    allowlist.iter().any(|allowed| {
        let allowed = allowed.trim_end_matches('.').to_lowercase();
        host == allowed || host.ends_with(&format!(".{allowed}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("valid ip")
    }

    // ── is_blocked_ip: IPv4 ──────────────────────────────────────────────

    #[test]
    fn blocks_ipv4_loopback() {
        assert!(is_blocked_ip(&ip("127.0.0.1")));
        assert!(is_blocked_ip(&ip("127.255.255.254")));
    }

    #[test]
    fn blocks_ipv4_rfc1918() {
        assert!(is_blocked_ip(&ip("10.0.0.1")));
        assert!(is_blocked_ip(&ip("10.255.255.255")));
        assert!(is_blocked_ip(&ip("172.16.0.1")));
        assert!(is_blocked_ip(&ip("172.31.255.255")));
        assert!(is_blocked_ip(&ip("192.168.0.1")));
        assert!(is_blocked_ip(&ip("192.168.255.255")));
    }

    #[test]
    fn blocks_cloud_instance_metadata() {
        // The single most valuable SSRF target in a cloud deployment.
        assert!(is_blocked_ip(&ip("169.254.169.254")));
        assert!(is_blocked_ip(&ip("169.254.0.1")));
    }

    #[test]
    fn blocks_ipv4_unspecified_multicast_cgnat() {
        assert!(is_blocked_ip(&ip("0.0.0.0")));
        assert!(is_blocked_ip(&ip("0.1.2.3")));
        assert!(is_blocked_ip(&ip("224.0.0.1")));
        assert!(is_blocked_ip(&ip("239.255.255.255")));
        assert!(is_blocked_ip(&ip("100.64.0.1")));
        assert!(is_blocked_ip(&ip("100.127.255.255")));
    }

    #[test]
    fn allows_public_ipv4() {
        assert!(!is_blocked_ip(&ip("1.1.1.1")));
        assert!(!is_blocked_ip(&ip("8.8.8.8")));
        assert!(!is_blocked_ip(&ip("140.82.121.4"))); // github.com
        // Just outside CGNAT and RFC 1918 on both sides.
        assert!(!is_blocked_ip(&ip("100.63.255.255")));
        assert!(!is_blocked_ip(&ip("100.128.0.0")));
        assert!(!is_blocked_ip(&ip("172.15.255.255")));
        assert!(!is_blocked_ip(&ip("172.32.0.0")));
    }

    // ── is_blocked_ip: IPv6 ──────────────────────────────────────────────

    #[test]
    fn blocks_ipv6_loopback_unspecified_multicast() {
        assert!(is_blocked_ip(&ip("::1")));
        assert!(is_blocked_ip(&ip("::")));
        assert!(is_blocked_ip(&ip("ff02::1")));
    }

    #[test]
    fn blocks_ipv6_unique_local_and_link_local() {
        assert!(is_blocked_ip(&ip("fc00::1")));
        assert!(is_blocked_ip(&ip("fd12:3456::1")));
        assert!(is_blocked_ip(&ip("fe80::1")));
        assert!(is_blocked_ip(&ip("febf::1")));
    }

    #[test]
    fn blocks_ipv4_mapped_and_compatible_bypasses() {
        // ::ffff:127.0.0.1 — the classic filter bypass.
        assert!(is_blocked_ip(&ip("::ffff:127.0.0.1")));
        assert!(is_blocked_ip(&ip("::ffff:169.254.169.254")));
        assert!(is_blocked_ip(&ip("::ffff:10.0.0.1")));
        // Deprecated v4-compatible form.
        assert!(is_blocked_ip(&ip("::127.0.0.1")));
        assert!(is_blocked_ip(&ip("::169.254.169.254")));
    }

    #[test]
    fn allows_public_ipv6() {
        assert!(!is_blocked_ip(&ip("2606:4700:4700::1111")));
        assert!(!is_blocked_ip(&ip("::ffff:1.1.1.1")));
    }

    // ── validate_addrs ───────────────────────────────────────────────────

    #[test]
    fn validate_addrs_is_all_or_nothing_on_a_mixed_list() {
        // The case that matters: an attacker's resolver answers with a public
        // IP first (which would pass on its own) and an RFC 1918 address after
        // it. The check must walk the whole list, because which address the
        // connect picks is not ours to decide.
        let addrs = vec![
            SocketAddr::from(([8, 8, 8, 8], 443)),
            SocketAddr::from(([10, 0, 0, 1], 443)),
        ];
        let err = validate_addrs(&addrs, "evil.example").expect_err("must reject");
        assert!(matches!(err, SsrfRejection::BlockedAddress { .. }));
    }

    #[test]
    fn validate_addrs_accepts_all_public_and_the_empty_list() {
        // Empty is trivially fine — resolve_addrs would have errored first.
        assert!(validate_addrs(&[], "example.com").is_ok());
        let addrs = vec![
            SocketAddr::from(([8, 8, 8, 8], 443)),
            SocketAddr::from(([1, 1, 1, 1], 443)),
            SocketAddr::from((
                std::net::Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111),
                443,
            )),
        ];
        assert!(validate_addrs(&addrs, "ok.example").is_ok());
    }

    // ── IpScope::AllowPrivate ────────────────────────────────────────────

    #[test]
    fn allow_private_permits_local_targets() {
        // The whole point: a local Ollama and a LAN MCP server must stay
        // reachable, or the offline story breaks.
        for addr in ["127.0.0.1", "10.0.0.5", "192.168.1.50", "172.16.3.4"] {
            assert!(
                !is_blocked_ip_in_scope(&ip(addr), IpScope::AllowPrivate),
                "{addr} must be reachable under AllowPrivate"
            );
            assert!(
                is_blocked_ip_in_scope(&ip(addr), IpScope::PublicOnly),
                "{addr} must stay blocked under PublicOnly"
            );
        }
        assert!(!is_blocked_ip_in_scope(&ip("::1"), IpScope::AllowPrivate));
        assert!(!is_blocked_ip_in_scope(
            &ip("fd12:3456::1"),
            IpScope::AllowPrivate
        ));
    }

    #[test]
    fn allow_private_still_blocks_what_is_never_legitimate() {
        // Cloud instance metadata is the highest-value SSRF target there is;
        // widening the scope for local targets must never widen it to this.
        for addr in [
            "169.254.169.254",
            "169.254.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "100.64.0.1",
        ] {
            assert!(
                is_blocked_ip_in_scope(&ip(addr), IpScope::AllowPrivate),
                "{addr} must stay blocked even under AllowPrivate"
            );
        }
        assert!(is_blocked_ip_in_scope(
            &ip("fe80::1"),
            IpScope::AllowPrivate
        ));
        assert!(is_blocked_ip_in_scope(
            &ip("ff02::1"),
            IpScope::AllowPrivate
        ));
        assert!(is_blocked_ip_in_scope(&ip("::"), IpScope::AllowPrivate));
        // And the v4-mapped bypass must follow the same scope.
        assert!(is_blocked_ip_in_scope(
            &ip("::ffff:169.254.169.254"),
            IpScope::AllowPrivate
        ));
        assert!(!is_blocked_ip_in_scope(
            &ip("::ffff:127.0.0.1"),
            IpScope::AllowPrivate
        ));
    }

    #[test]
    fn vet_url_honours_the_policy_scope() {
        let local = UrlPolicy::http_public(Duration::from_secs(5), "test")
            .with_ip_scope(IpScope::AllowPrivate);
        assert!(vet_url("http://127.0.0.1:11434/api/tags", &local).is_ok());
        // Metadata stays refused on the widened policy.
        let err = vet_url("http://169.254.169.254/latest/meta-data/", &local)
            .expect_err("metadata must stay blocked");
        assert!(matches!(err, SsrfRejection::BlockedAddress { .. }));
        // And a bad scheme is still a bad scheme.
        assert!(matches!(
            vet_url("file:///etc/passwd", &local),
            Err(SsrfRejection::SchemeNotAllowed { .. }) | Err(SsrfRejection::MissingHost)
        ));
    }

    // ── host_in_allowlist ────────────────────────────────────────────────

    #[test]
    fn empty_allowlist_denies_everything() {
        assert!(!host_in_allowlist("example.com", &[]));
    }

    #[test]
    fn allowlist_matches_on_dot_boundary_only() {
        let list = &["plugins.example.com"];
        assert!(host_in_allowlist("plugins.example.com", list));
        assert!(host_in_allowlist("v2.plugins.example.com", list));
        assert!(host_in_allowlist("PLUGINS.EXAMPLE.COM", list));
        assert!(host_in_allowlist("plugins.example.com.", list));
        // The bypass an unanchored `ends_with` would let through.
        assert!(!host_in_allowlist("evilplugins.example.com", list));
        assert!(!host_in_allowlist("plugins.example.com.evil.net", list));
    }

    // ── vet_url ──────────────────────────────────────────────────────────

    fn policy() -> UrlPolicy {
        UrlPolicy::http_public(Duration::from_secs(5), "test")
    }

    #[test]
    fn rejects_non_http_schemes() {
        for raw in [
            "file:///etc/passwd",
            "gopher://evil.test/_x",
            "ftp://evil.test/x",
            "data:text/plain,hi",
        ] {
            let err = vet_url(raw, &policy()).expect_err(raw);
            assert!(
                matches!(
                    err,
                    SsrfRejection::SchemeNotAllowed { .. } | SsrfRejection::MissingHost
                ),
                "{raw} produced {err:?}"
            );
        }
    }

    #[test]
    fn rejects_https_only_policy_on_plaintext() {
        let p = UrlPolicy::https_public(Duration::from_secs(5), "test");
        let err = vet_url("http://example.com/x", &p).expect_err("http must be refused");
        assert!(
            matches!(err, SsrfRejection::SchemeNotAllowed { ref scheme, .. } if scheme == "http")
        );
        assert_eq!(err.status_hint(), 400);
        // The message must name what would have worked — plugins_handler's
        // pre-existing test asserts the operator sees "https".
        assert!(err.to_string().contains("https"), "{err}");
    }

    #[test]
    fn rejects_literal_internal_addresses() {
        // Literal IPs skip DNS but still go through validate_addrs.
        for raw in [
            "http://127.0.0.1/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.1/x",
            "http://[::1]/x",
            "http://[::ffff:127.0.0.1]/x",
        ] {
            let err = vet_url(raw, &policy()).expect_err(raw);
            assert!(
                matches!(err, SsrfRejection::BlockedAddress { .. }),
                "{raw} produced {err:?}"
            );
            assert_eq!(err.status_hint(), 403);
        }
    }

    #[test]
    fn rejects_garbage_urls() {
        assert!(matches!(
            vet_url("not a url", &policy()),
            Err(SsrfRejection::InvalidUrl(_))
        ));
        assert!(matches!(
            vet_url("", &policy()),
            Err(SsrfRejection::InvalidUrl(_))
        ));
    }

    #[test]
    fn empty_host_allowlist_refuses_before_dns() {
        let p = policy().with_host_allowlist(&[]);
        let err = vet_url("http://127.0.0.1/x", &p).expect_err("must refuse");
        assert!(err.to_string().contains("allowlist"), "{err}");
        // Host gate runs before resolution, so this is HostNotAllowed rather
        // than BlockedAddress — that ordering is what makes an empty allow-list
        // a true kill switch.
        assert!(matches!(err, SsrfRejection::HostNotAllowed(_)));
    }

    #[test]
    fn status_hints_split_client_error_from_policy_refusal() {
        assert_eq!(SsrfRejection::MissingHost.status_hint(), 400);
        assert_eq!(SsrfRejection::HostNotAllowed("h".into()).status_hint(), 403);
        assert_eq!(SsrfRejection::ClientBuild("x".into()).status_hint(), 502);
    }
}
