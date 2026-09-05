//! Proactive outbound sends (issue #921).
//!
//! Garra could answer a Telegram message but never *start* one. The gap was
//! not the Bot API — `TelegramChannel::send_message` has always existed and is
//! reachable through `AppState.channels`. The gap was addressing: that method
//! reads `message.metadata["telegram_chat_id"]`, and **nothing in the
//! repository ever wrote that key**. A repo-wide grep found exactly two hits,
//! both reads.
//!
//! So the proactive path was not missing, it was dead: `execute_scheduled_task`
//! already builds an outgoing `Message` and hands it to the channel adapter,
//! and every scheduled Telegram heartbeat has been failing at that line with
//! `"missing telegram_chat_id in metadata"` — swallowed by a `tracing::error!`
//! that nobody was reading. Fixing the addressing fixes the scheduler and
//! makes the new tool a thin wrapper rather than a new subsystem.
//!
//! # Security posture
//!
//! An outbound send is a different risk from an inbound reply: the model
//! chooses the recipient. The two cases are separated deliberately.
//!
//! * **Replying into the session's own chat** needs no allowlist. The chat is
//!   the one this conversation already belongs to — the human on the other end
//!   started it, and they are receiving a message they can already receive.
//! * **Sending to an explicit `chat_id`** is deny-by-default and consults
//!   [`ProactiveTargets`], an allowlist of chat ids the *operator* configured.
//!
//! [`ProactiveTargets`] is deliberately **not** the existing
//! `garraia_security::Allowlist`. That one is keyed on Telegram *user* ids,
//! while a chat id is a different namespace (negative for groups and
//! supergroups, equal to the user id only in private chats), and its
//! `AllowlistMode::Open` means "allow everyone" — a sane default for deciding
//! who may talk *to* the bot, and a dangerous one for deciding who the bot may
//! talk to unprompted. An empty list here means "refuse", never "allow all".

use std::collections::HashSet;

use garraia_config::AppConfig;

/// Chat ids a proactive send may target when the model names one explicitly.
///
/// Read from each Telegram channel's `proactive_chat_ids` setting. Empty means
/// no explicit-target send is permitted at all, which is the default.
#[derive(Debug, Clone, Default)]
pub struct ProactiveTargets {
    allowed: HashSet<i64>,
}

impl ProactiveTargets {
    /// Collect `proactive_chat_ids` across every configured Telegram channel.
    ///
    /// Accepts JSON numbers and numeric strings: a chat id exceeds 2^53 in no
    /// realistic case, but operators write these by hand and YAML/TOML round
    /// trips make quoting easy to get wrong. A value that is neither is
    /// ignored rather than silently coerced — a typo must not widen the set.
    pub fn from_config(config: &AppConfig) -> Self {
        let mut allowed = HashSet::new();

        for channel in config.channels.values() {
            if channel.channel_type != "telegram" {
                continue;
            }
            let Some(list) = channel
                .settings
                .get("proactive_chat_ids")
                .and_then(|v| v.as_array())
            else {
                continue;
            };
            for entry in list {
                if let Some(id) = entry.as_i64() {
                    allowed.insert(id);
                } else if let Some(id) = entry.as_str().and_then(|s| s.trim().parse::<i64>().ok()) {
                    allowed.insert(id);
                }
            }
        }

        Self { allowed }
    }

    /// Build from an explicit set. Tests and callers that already have ids.
    pub fn from_ids(ids: impl IntoIterator<Item = i64>) -> Self {
        Self {
            allowed: ids.into_iter().collect(),
        }
    }

    /// Deny-by-default membership check.
    pub fn allows(&self, chat_id: i64) -> bool {
        self.allowed.contains(&chat_id)
    }

    /// True when no explicit target is permitted.
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// How many ids are configured. For diagnostics only — the ids themselves
    /// are not logged, since a chat id identifies a person or a group.
    pub fn len(&self) -> usize {
        self.allowed.len()
    }
}

/// Per-session ceiling on proactive sends.
///
/// Security audit finding (MEDIUM), issue #921: the agent's generic execution
/// budget allows ~50 tool calls per task, and the loop detector only catches
/// calls with an *identical* signature. A prompt-injected heartbeat could
/// therefore call `telegram_send` fifty times with fifty different texts and
/// the runtime would let it — the user gets spammed by their own assistant.
///
/// `ScheduleHeartbeat` solves its own recursion problem by refusing to run
/// during a heartbeat; this tool cannot, because running during a heartbeat is
/// its entire purpose. So it carries its own ceiling instead.
///
/// The clock is a parameter, not a call to `Instant::now()` inside: the
/// interesting cases are the window boundary and the reset, and neither is
/// testable against a real clock.
#[derive(Debug, Default)]
pub struct SendBudget {
    inner: std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, u32)>>,
}

/// Sends allowed per session per window.
pub const MAX_SENDS_PER_WINDOW: u32 = 5;
/// Length of the rolling window.
pub const SEND_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

impl SendBudget {
    /// Charge one send against `session`. `Err(used)` when the ceiling is hit.
    ///
    /// Fails **open** if the mutex is poisoned — a panic elsewhere must not
    /// permanently silence the assistant's ability to reach its user. The
    /// budget is anti-amplification, not an authorization boundary; the
    /// allowlist is the boundary, and that one fails closed.
    pub fn try_consume(&self, session: &str, now: std::time::Instant) -> Result<u32, u32> {
        let Ok(mut map) = self.inner.lock() else {
            return Ok(0);
        };

        // Opportunistic sweep: without it a long-lived gateway accumulates one
        // entry per session that ever sent, forever.
        map.retain(|_, (start, _)| now.duration_since(*start) < SEND_WINDOW);

        let entry = map.entry(session.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= SEND_WINDOW {
            *entry = (now, 0);
        }

        if entry.1 >= MAX_SENDS_PER_WINDOW {
            return Err(entry.1);
        }
        entry.1 += 1;
        Ok(MAX_SENDS_PER_WINDOW - entry.1)
    }
}

/// Attach the channel-specific address an outbound `Message` needs.
///
/// Today only Telegram requires one (`telegram_chat_id`). Returning the
/// enriched value rather than mutating in place keeps the caller's stored
/// session metadata untouched: the address belongs to the *message*, not to
/// the session row.
pub fn with_channel_address(
    metadata: &serde_json::Value,
    channel_type: &str,
    external_id: Option<&str>,
) -> serde_json::Value {
    let mut out = metadata.clone();

    if channel_type != "telegram" {
        return out;
    }

    // Already addressed (a caller that knew the chat id) — leave it alone.
    if out
        .get("telegram_chat_id")
        .and_then(|v| v.as_i64())
        .is_some()
    {
        return out;
    }

    let Some(chat_id) = external_id.and_then(|s| s.trim().parse::<i64>().ok()) else {
        return out;
    };

    if let Some(obj) = out.as_object_mut() {
        obj.insert(
            "telegram_chat_id".to_string(),
            serde_json::Value::from(chat_id),
        );
    } else {
        out = serde_json::json!({ "telegram_chat_id": chat_id });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telegram_channel(setting: serde_json::Value) -> garraia_config::model::ChannelConfig {
        let mut settings = std::collections::HashMap::new();
        settings.insert("proactive_chat_ids".to_string(), setting);
        garraia_config::model::ChannelConfig {
            channel_type: "telegram".into(),
            enabled: Some(true),
            settings,
        }
    }

    fn config_with(setting: serde_json::Value) -> AppConfig {
        let mut config = AppConfig::default();
        config
            .channels
            .insert("tg".to_string(), telegram_channel(setting));
        config
    }

    /// The whole point: no configuration means no explicit-target send.
    #[test]
    fn default_is_deny_all() {
        let targets = ProactiveTargets::from_config(&AppConfig::default());
        assert!(targets.is_empty());
        assert!(!targets.allows(12345));
        assert!(!targets.allows(-100987));
    }

    #[test]
    fn reads_numbers_and_numeric_strings() {
        let targets = ProactiveTargets::from_config(&config_with(serde_json::json!([
            12345,
            "-1009876543210",
            " 777 "
        ])));
        assert!(targets.allows(12345));
        assert!(targets.allows(-1009876543210));
        assert!(targets.allows(777));
        assert_eq!(targets.len(), 3);
    }

    /// A typo must narrow the set, never widen it — the failure mode of a
    /// permissive parse here is messaging a stranger.
    #[test]
    fn garbage_entries_are_ignored_not_coerced() {
        let targets = ProactiveTargets::from_config(&config_with(serde_json::json!([
            "not-a-number",
            null,
            true,
            {"chat_id": 5},
            42
        ])));
        assert_eq!(targets.len(), 1);
        assert!(targets.allows(42));
    }

    /// Non-Telegram channels do not contribute ids.
    #[test]
    fn ignores_other_channel_types() {
        let mut config = AppConfig::default();
        let mut settings = std::collections::HashMap::new();
        settings.insert("proactive_chat_ids".to_string(), serde_json::json!([12345]));
        config.channels.insert(
            "dc".to_string(),
            garraia_config::model::ChannelConfig {
                channel_type: "discord".into(),
                enabled: Some(true),
                settings,
            },
        );
        assert!(ProactiveTargets::from_config(&config).is_empty());
    }

    // ─── SendBudget (finding MEDIUM da auditoria) ──────────────────────────

    #[test]
    fn budget_allows_up_to_the_ceiling_then_refuses() {
        let b = SendBudget::default();
        let t0 = std::time::Instant::now();
        for i in 0..MAX_SENDS_PER_WINDOW {
            assert!(b.try_consume("s", t0).is_ok(), "envio {i} deveria passar");
        }
        assert_eq!(b.try_consume("s", t0), Err(MAX_SENDS_PER_WINDOW));
    }

    #[test]
    fn budget_resets_after_the_window() {
        let b = SendBudget::default();
        let t0 = std::time::Instant::now();
        for _ in 0..MAX_SENDS_PER_WINDOW {
            let _ = b.try_consume("s", t0);
        }
        assert!(b.try_consume("s", t0).is_err());
        assert!(b.try_consume("s", t0 + SEND_WINDOW).is_ok());
    }

    /// Uma sessão barulhenta não pode consumir a cota de outra.
    #[test]
    fn budget_is_per_session() {
        let b = SendBudget::default();
        let t0 = std::time::Instant::now();
        for _ in 0..MAX_SENDS_PER_WINDOW {
            let _ = b.try_consume("noisy", t0);
        }
        assert!(b.try_consume("noisy", t0).is_err());
        assert!(b.try_consume("other", t0).is_ok());
    }

    /// Entradas velhas não podem crescer sem limite num gateway longevo.
    #[test]
    fn budget_sweeps_expired_sessions() {
        let b = SendBudget::default();
        let t0 = std::time::Instant::now();
        for i in 0..50 {
            let _ = b.try_consume(&format!("s{i}"), t0);
        }
        assert_eq!(b.inner.lock().unwrap().len(), 50);
        let _ = b.try_consume("fresh", t0 + SEND_WINDOW);
        assert_eq!(b.inner.lock().unwrap().len(), 1);
    }

    #[test]
    fn addresses_a_telegram_message_from_the_session_key() {
        let out = with_channel_address(
            &serde_json::json!({"continuity_key": "k"}),
            "telegram",
            Some("-1001"),
        );
        assert_eq!(out["telegram_chat_id"], serde_json::json!(-1001));
        assert_eq!(out["continuity_key"], serde_json::json!("k"));
    }

    /// The scheduler writes `{}` or `{"continuity_key": …}` into session
    /// metadata; neither carries an address, which is the #921 bug.
    #[test]
    fn addresses_an_empty_metadata_object() {
        let out = with_channel_address(&serde_json::json!({}), "telegram", Some("42"));
        assert_eq!(out["telegram_chat_id"], serde_json::json!(42));
    }

    #[test]
    fn leaves_an_existing_address_alone() {
        let out = with_channel_address(
            &serde_json::json!({"telegram_chat_id": 7}),
            "telegram",
            Some("9"),
        );
        assert_eq!(out["telegram_chat_id"], serde_json::json!(7));
    }

    #[test]
    fn non_telegram_channels_are_untouched() {
        let before = serde_json::json!({"a": 1});
        let out = with_channel_address(&before, "discord", Some("42"));
        assert_eq!(out, before);
    }

    /// An unmapped session must not produce a half-addressed message: the send
    /// fails with "missing telegram_chat_id", which is the honest error.
    #[test]
    fn unresolvable_external_id_adds_nothing() {
        for external in [None, Some("not-a-number"), Some("")] {
            let out = with_channel_address(&serde_json::json!({}), "telegram", external);
            assert!(
                out.get("telegram_chat_id").is_none(),
                "external = {external:?}"
            );
        }
    }
}
