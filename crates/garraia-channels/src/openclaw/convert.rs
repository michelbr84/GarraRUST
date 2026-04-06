use garraia_common::{ChannelId, Message, MessageContent, MessageDirection, SessionId, UserId};

/// Convert an OpenClaw JSON message into a GarraIA `Message`.
///
/// Expected OpenClaw format:
/// ```json
/// {
///   "platform": "whatsapp",
///   "channel_id": "chat-123",
///   "user_id": "user-456",
///   "text": "Hello",
///   "thread_id": "optional-thread"
/// }
/// ```
pub fn from_openclaw_message(value: &serde_json::Value) -> Option<Message> {
    let platform = value.get("platform")?.as_str()?;
    let channel_id_raw = value
        .get("channel_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let user_id_raw = value
        .get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");

    let session_id = SessionId::from_string(format!("openclaw-{platform}-{channel_id_raw}"));
    let channel_id = ChannelId::from_string(format!("openclaw-{platform}"));
    let user_id = UserId::from_string(user_id_raw);

    let mut msg = Message::text(
        session_id,
        channel_id,
        user_id,
        MessageDirection::Incoming,
        text,
    );

    // Preserve OpenClaw metadata for routing the reply back.
    msg.metadata = serde_json::json!({
        "openclaw": true,
        "platform": platform,
        "original_channel_id": channel_id_raw,
        "original_user_id": user_id_raw,
        "thread_id": value.get("thread_id").and_then(|v| v.as_str()),
    });

    Some(msg)
}

/// Convert a GarraIA `Message` into OpenClaw JSON for sending a reply.
pub fn to_openclaw_message(msg: &Message) -> serde_json::Value {
    let text = match &msg.content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::System(t) => t.clone(),
        other => format!("[unsupported content type: {other:?}]"),
    };

    let platform = msg
        .metadata
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let channel_id = msg
        .metadata
        .get("original_channel_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let user_id = msg
        .metadata
        .get("original_user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let mut obj = serde_json::json!({
        "platform": platform,
        "channel_id": channel_id,
        "user_id": user_id,
        "text": text,
    });

    if let Some(thread_id) = msg.metadata.get("thread_id").and_then(|v| v.as_str()) {
        obj["thread_id"] = serde_json::Value::String(thread_id.to_string());
    }

    obj
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_openclaw_message() {
        let incoming = serde_json::json!({
            "platform": "telegram",
            "channel_id": "chat-42",
            "user_id": "user-7",
            "text": "Hello Garra!",
            "thread_id": "thread-1"
        });

        let msg = from_openclaw_message(&incoming).expect("should parse");
        assert_eq!(msg.session_id.as_str(), "openclaw-telegram-chat-42");
        assert!(matches!(msg.content, MessageContent::Text(ref t) if t == "Hello Garra!"));
        assert_eq!(
            msg.metadata.get("platform").unwrap().as_str().unwrap(),
            "telegram"
        );

        let outgoing = to_openclaw_message(&msg);
        assert_eq!(outgoing["platform"], "telegram");
        assert_eq!(outgoing["channel_id"], "chat-42");
        assert_eq!(outgoing["text"], "Hello Garra!");
        assert_eq!(outgoing["thread_id"], "thread-1");
    }

    #[test]
    fn from_openclaw_missing_text_defaults_empty() {
        let incoming = serde_json::json!({
            "platform": "discord",
            "channel_id": "ch-1",
        });

        let msg = from_openclaw_message(&incoming).expect("should parse");
        assert!(matches!(msg.content, MessageContent::Text(ref t) if t.is_empty()));
    }

    #[test]
    fn from_openclaw_missing_platform_returns_none() {
        let incoming = serde_json::json!({ "text": "hello" });
        assert!(from_openclaw_message(&incoming).is_none());
    }
}
