//! Free functions for the signal-cli REST API.
//!
//! Same shape as [`crate::whatsapp::api`] and [`crate::slack::api`]: the two
//! other channels that speak HTTP+JSON directly instead of through an SDK put
//! their requests in free functions taking `&Client` plus the config they need,
//! so both the `Channel::send_message` path and the polling/webhook task can
//! call one implementation.
//!
//! Signal was the odd one out. `SignalChannel::connect` takes `&mut self` and
//! `SignalChannel` is neither `Clone` nor stored behind an `Arc`
//! (`ChannelRegistry` holds `Box<dyn Channel>`), so its reply task could not
//! reach `self.send_text` and open-coded the POST instead. The two copies then
//! drifted: different error types, different variable names, and — the part
//! that mattered — **only one of them ran the URL guard**.
//!
//! Moving the request here fixes that by construction. `vetted` is an
//! `Arc<OnceLock<()>>` the task can clone, so [`ensure_url_vetted`] is
//! reachable from inside `tokio::spawn` and every caller pays the same gate.

use std::sync::OnceLock;

use reqwest::Client;
use serde_json::json;

use super::{SignalConfig, vet_signal_cli_url};
use garraia_common::{Error, Result};

/// Run [`vet_signal_cli_url`] at most once for a given `vetted` cell, then
/// remember the verdict.
///
/// The guard resolves the host (`to_socket_addrs`, a blocking syscall), so
/// paying it on every send would be both slow and pointless: `SignalConfig` is
/// immutable after construction, so the verdict cannot change.
///
/// A racing caller may vet concurrently; the guard is pure and the verdict
/// identical, so losing the race is harmless.
pub fn ensure_url_vetted(config: &SignalConfig, vetted: &OnceLock<()>) -> Result<()> {
    if vetted.get().is_some() {
        return Ok(());
    }
    vet_signal_cli_url(&config.signal_cli_url)?;
    let _ = vetted.set(());
    Ok(())
}

fn send_url(config: &SignalConfig) -> String {
    format!("{}/v2/send", config.signal_cli_url.trim_end_matches('/'))
}

/// POST a prepared body to `/v2/send`, gating on the URL guard first.
///
/// `what` names the operation in error messages ("send", "group send",
/// "reply") so the caller keeps the wording it had before the extraction.
async fn post_send(
    client: &Client,
    config: &SignalConfig,
    vetted: &OnceLock<()>,
    what: &str,
    body: serde_json::Value,
) -> Result<()> {
    ensure_url_vetted(config, vetted)?;

    let resp = client
        .post(send_url(config))
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::Channel(format!("signal {what} failed: {e}")))?;

    // `send()` only errors on transport failure: a 4xx/5xx from signal-cli
    // comes back as `Ok(Response)`, so the status needs its own check.
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = resp.text().await.unwrap_or_default();
        return Err(Error::Channel(format!(
            "signal {what} error {status}: {detail}"
        )));
    }

    Ok(())
}

/// Send a text message to a single recipient.
pub async fn send_text(
    client: &Client,
    config: &SignalConfig,
    vetted: &OnceLock<()>,
    recipient: &str,
    text: &str,
) -> Result<()> {
    let body = json!({
        "message": text,
        "number": config.phone_number,
        "recipients": [recipient],
    });
    post_send(client, config, vetted, "send", body).await
}

/// Send a text message to a Signal group.
///
/// signal-cli wants an empty `recipients` alongside `group_id`, not the group
/// id inside `recipients`.
pub async fn send_to_group(
    client: &Client,
    config: &SignalConfig,
    vetted: &OnceLock<()>,
    group_id: &str,
    text: &str,
) -> Result<()> {
    let body = json!({
        "message": text,
        "number": config.phone_number,
        "recipients": [],
        "group_id": group_id,
    });
    post_send(client, config, vetted, "group send", body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(url: &str) -> SignalConfig {
        SignalConfig {
            signal_cli_url: url.to_string(),
            phone_number: "+15550001111".to_string(),
        }
    }

    /// The gap this module exists to close: the reply task inside `connect`'s
    /// `tokio::spawn` used to POST without running the guard, relying only on
    /// `connect` having run it earlier. Now the guard is reachable from there,
    /// so an unvetted cell fails closed even though nothing called `connect`.
    #[tokio::test]
    async fn send_text_refuses_cleartext_with_an_unvetted_cell() {
        let vetted = OnceLock::new();
        let err = send_text(
            &Client::new(),
            &cfg("http://10.0.0.5:8080"),
            &vetted,
            "+15550002222",
            "segredo",
        )
        .await
        .expect_err("http:// to a non-loopback host must fail closed");
        assert!(
            err.to_string().contains("non-loopback"),
            "unexpected error: {err}"
        );
        assert!(
            vetted.get().is_none(),
            "a rejected URL must not be remembered as vetted"
        );
    }

    #[tokio::test]
    async fn send_to_group_refuses_cleartext_with_an_unvetted_cell() {
        let vetted = OnceLock::new();
        let err = send_to_group(
            &Client::new(),
            &cfg("http://10.0.0.5:8080"),
            &vetted,
            "group.abc",
            "segredo",
        )
        .await
        .expect_err("http:// to a non-loopback host must fail closed");
        assert!(
            err.to_string().contains("non-loopback"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn a_rejected_scheme_never_marks_the_cell_vetted() {
        let vetted = OnceLock::new();
        assert!(ensure_url_vetted(&cfg("ftp://example.test"), &vetted).is_err());
        assert!(vetted.get().is_none());
        // And a later good URL on the same cell still gets its own verdict.
        assert!(ensure_url_vetted(&cfg("https://signal.example"), &vetted).is_ok());
        assert!(vetted.get().is_some());
    }
}
