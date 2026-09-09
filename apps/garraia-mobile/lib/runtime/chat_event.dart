import 'dart:convert';

/// One event of an assistant turn, as it happens.
///
/// The gateway streams a turn frame by frame over `/ws`
/// (`crates/garraia-gateway/src/ws.rs`): `delta` for each chunk of text,
/// `tool_started` / `tool_finished` around a tool call, then exactly one
/// terminator — `message` when the turn completed, `stopped` when it was
/// cancelled, `error` when the agent failed.
///
/// The hierarchy is sealed so a `switch` over it is exhaustive at compile
/// time: a frame type added here cannot be forgotten at a consumer.
sealed class ChatEvent {
  const ChatEvent();

  /// Parses one server frame, or returns `null` for anything this client does
  /// not understand.
  ///
  /// Returning `null` rather than throwing is the point: the gateway may grow
  /// a frame type before the app ships support for it, and a stranger on the
  /// wire must never take the turn down. Handshake frames (`connected`,
  /// `resumed`) are consumed by the transport and are unknown here too.
  static ChatEvent? tryParse(String raw) {
    final Object? decoded;
    try {
      decoded = jsonDecode(raw);
    } on FormatException {
      return null;
    }
    if (decoded is! Map<String, dynamic>) return null;

    switch (_string(decoded['type'])) {
      case 'delta':
        final content = _string(decoded['content']);
        return content == null ? null : ChatDelta(content);

      case 'tool_started':
        final name = _string(decoded['name']);
        return name == null
            ? null
            : ChatToolStarted(name: name, detail: _string(decoded['detail']));

      case 'tool_finished':
        final name = _string(decoded['name']);
        return name == null
            ? null
            : ChatToolFinished(
                name: name,
                success: decoded['success'] == true,
                summary: _string(decoded['summary']),
              );

      case 'message':
        final content = _string(decoded['content']);
        return content == null ? null : ChatCompleted(content);

      case 'stopped':
        return const ChatStopped();

      case 'error':
        return ChatFailed(
          _string(decoded['message']) ?? 'the runtime reported an error',
        );

      default:
        return null;
    }
  }

  /// `null` unless the value really is a string. A frame carrying a number
  /// where a string belongs is a malformed frame, not a cast to force.
  static String? _string(Object? value) => value is String ? value : null;
}

/// A chunk of assistant text. Chunks concatenate in arrival order.
final class ChatDelta extends ChatEvent {
  final String content;
  const ChatDelta(this.content);
}

/// A tool call started. [detail] is a short human-readable argument summary.
final class ChatToolStarted extends ChatEvent {
  final String name;
  final String? detail;
  const ChatToolStarted({required this.name, this.detail});
}

/// A tool call finished. The tool's full output is deliberately not on the
/// wire — the frame says it ended, it does not carry the result.
final class ChatToolFinished extends ChatEvent {
  final String name;
  final bool success;
  final String? summary;
  const ChatToolFinished({
    required this.name,
    required this.success,
    this.summary,
  });
}

/// The turn completed. [content] is the whole reply, authoritative over the
/// concatenated deltas.
final class ChatCompleted extends ChatEvent {
  final String content;
  const ChatCompleted(this.content);
}

/// The turn was cancelled. Whatever arrived before this is what the user saw,
/// and it is kept — the server does not persist a cancelled turn, so dropping
/// it here would erase text the user already read.
final class ChatStopped extends ChatEvent {
  const ChatStopped();
}

/// The runtime failed mid-turn, or the transport did.
final class ChatFailed extends ChatEvent {
  final String message;
  const ChatFailed(this.message);
}

/// The gateway answered the handshake with a different session than the one
/// asked for — it does that when a resume fails, creating a fresh session
/// instead. The app has to follow, otherwise the next history load reads a
/// session the reply never went to.
final class ChatSessionChanged extends ChatEvent {
  final String sessionId;
  const ChatSessionChanged(this.sessionId);
}
