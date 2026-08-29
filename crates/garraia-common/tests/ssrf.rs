//! Integration tests for the shared SSRF guard.
//!
//! These live outside `src/ssrf.rs` rather than in a `#[cfg(test)]` module for
//! two reasons: everything under test is part of the crate's public API, so an
//! integration test exercises it exactly as a consumer would; and it keeps the
//! module itself under the repo's 700-line quality-ratchet threshold.

#![cfg(feature = "ssrf")]

use garraia_common::ssrf::*;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

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
    let local =
        UrlPolicy::http_public(Duration::from_secs(5), "test").with_ip_scope(IpScope::AllowPrivate);
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
    assert!(matches!(err, SsrfRejection::SchemeNotAllowed { ref scheme, .. } if scheme == "http"));
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

// ── NAT64 (RFC 6052) ─────────────────────────────────────────────────────

#[test]
fn blocks_nat64_wellknown_prefix() {
    // On a DNS64 network a resolver answers v4-only names with 64:ff9b::a.b.c.d.
    // Neither the v4-mapped nor the v4-compatible check catches that shape, so
    // without a dedicated arm the metadata block is bypassable there.
    assert!(is_blocked_ip(&ip("64:ff9b::169.254.169.254")));
    assert!(is_blocked_ip(&ip("64:ff9b::127.0.0.1")));
    assert!(is_blocked_ip(&ip("64:ff9b::10.0.0.1")));
    assert!(is_blocked_ip_in_scope(
        &ip("64:ff9b::169.254.169.254"),
        IpScope::AllowPrivate
    ));
    // A public v4 behind NAT64 is still reachable, and the prefix must not
    // swallow unrelated addresses that merely start with 0x0064.
    assert!(!is_blocked_ip(&ip("64:ff9b::8.8.8.8")));
    assert!(!is_blocked_ip(&ip("64:1::1")));
}

// ── SsrfCategory ─────────────────────────────────────────────────────────

#[test]
fn category_and_status_hint_agree() {
    let cases = [
        (SsrfRejection::MissingHost, SsrfCategory::BadRequest, 400),
        (
            SsrfRejection::HostNotAllowed("h".into()),
            SsrfCategory::Forbidden,
            403,
        ),
        (
            SsrfRejection::ClientBuild("x".into()),
            SsrfCategory::Upstream,
            502,
        ),
    ];
    for (rejection, category, status) in cases {
        assert_eq!(rejection.category(), category);
        assert_eq!(rejection.status_hint(), status);
    }
}
