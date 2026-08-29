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

/// How a caller should classify a rejection. An enum rather than a bare status
/// code so `match` stays exhaustive: adding a variant to [`SsrfRejection`] then
/// forces every call site to decide where it belongs, instead of silently
/// falling into a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsrfCategory {
    /// The caller sent something malformed or oversized. HTTP 400.
    BadRequest,
    /// The target is off limits by policy. HTTP 403.
    Forbidden,
    /// We or the upstream failed. HTTP 502.
    Upstream,
}

impl SsrfRejection {
    /// Which class of failure this is.
    pub fn category(&self) -> SsrfCategory {
        match self {
            Self::InvalidUrl(_)
            | Self::SchemeNotAllowed { .. }
            | Self::MissingHost
            | Self::ResolveFailed(_)
            | Self::BodyTooLarge { .. } => SsrfCategory::BadRequest,
            Self::HostNotAllowed(_) | Self::BlockedAddress { .. } => SsrfCategory::Forbidden,
            Self::ClientBuild(_) | Self::Transport(_) => SsrfCategory::Upstream,
        }
    }

    /// [`Self::category`] as an HTTP status code, for callers that just need a
    /// number to serialize.
    pub fn status_hint(&self) -> u16 {
        match self.category() {
            SsrfCategory::BadRequest => 400,
            SsrfCategory::Forbidden => 403,
            SsrfCategory::Upstream => 502,
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
    #[must_use = "returns a new UrlPolicy; the receiver is unchanged"]
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
    #[must_use = "returns a new UrlPolicy; the receiver is unchanged"]
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
    #[must_use = "returns a new UrlPolicy; the receiver is unchanged"]
    pub const fn with_host_allowlist(mut self, allowlist: &'static [&'static str]) -> Self {
        self.host_allowlist = Some(allowlist);
        self
    }

    /// Widen the address scope. Use only where local targets are the point —
    /// see [`IpScope::AllowPrivate`] for what stays blocked regardless.
    #[must_use = "returns a new UrlPolicy; the receiver is unchanged"]
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
                return is_blocked_ip_in_scope(&IpAddr::V4(embedded_v4(&segs)), scope);
            }
            // NAT64 well-known prefix 64:ff9b::/96 (RFC 6052). On a network with
            // DNS64 — mobile carriers, IPv6-only cloud subnets — a resolver
            // answers with `64:ff9b::169.254.169.254` for a v4-only name, and
            // neither the v4-mapped nor the v4-compatible check above matches
            // it. Without this arm the instance-metadata block is bypassable
            // wherever DNS64 is in play.
            if segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2..6] == [0, 0, 0, 0] {
                return is_blocked_ip_in_scope(&IpAddr::V4(embedded_v4(&segs)), scope);
            }
            false
        }
    }
}

/// The IPv4 address encoded in the low 32 bits of an IPv6 address. Shared by
/// the v4-compatible (`::a.b.c.d`) and NAT64 (`64:ff9b::a.b.c.d`) forms.
fn embedded_v4(segs: &[u16; 8]) -> Ipv4Addr {
    Ipv4Addr::new(
        (segs[6] >> 8) as u8,
        (segs[6] & 0xff) as u8,
        (segs[7] >> 8) as u8,
        (segs[7] & 0xff) as u8,
    )
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
