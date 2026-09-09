import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../runtime/chat_event.dart';
import '../runtime/models.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';
import 'streaming_reply_provider.dart';

part 'chat_provider.g.dart';

/// The runtime reported a failed turn (an `error` frame, or a transport that
/// died after the message was already submitted).
class ChatTurnFailed implements Exception {
  final String message;
  const ChatTurnFailed(this.message);

  @override
  String toString() => message;
}

/// In-memory list of chat messages for the current session (loaded from
/// history, appended on send). Runtime-agnostic: every call goes through
/// `GarraConnection`.
@riverpod
class ChatMessages extends _$ChatMessages {
  @override
  Future<List<ChatMessage>> build() async {
    final conn = requireConnection(ref);
    if (conn.mode == RuntimeMode.cloud) return conn.history(null);
    final sessionId = await ref.watch(currentSessionProvider.future);
    return conn.history(sessionId);
  }

  /// Runs one turn, showing it as it arrives.
  ///
  /// The reply is streamed into [StreamingReplyState] rather than into this
  /// list: appending here per token would rebuild every bubble on screen. Only
  /// when the turn ends does the finished text become a real message.
  Future<void> send(String text) async {
    final conn = requireConnection(ref);
    final streaming = ref.read(streamingReplyStateProvider.notifier);

    // Optimistically add user message
    final current = state.value ?? const [];
    state = AsyncData([
      ...current,
      ChatMessage(
        role: 'user',
        content: text,
        timestamp: DateTime.now().toIso8601String(),
      ),
    ]);

    ref.read(mascotStateProvider.notifier).set(MascotState.thinking);
    streaming.start();

    try {
      final sessionId = conn.mode == RuntimeMode.cloud
          ? null
          : await ref.read(currentSessionProvider.notifier).ensure();

      var reply = '';
      String? failure;

      await for (final event in conn.sendMessageStreaming(
        text,
        sessionId: sessionId,
      )) {
        switch (event) {
          case ChatDelta(:final content):
            streaming.appendDelta(content);
          case ChatToolStarted(:final name):
            streaming.toolStarted(name);
          case ChatToolFinished():
            streaming.toolFinished();
          case ChatCompleted(:final content):
            reply = content;
          case ChatStopped():
            // A cancelled turn is never persisted by the gateway, so the text
            // the user already read exists only here. Keeping it is the whole
            // difference between "stopped" and "lost".
            reply = ref.read(streamingReplyStateProvider).text;
          case ChatFailed(:final message):
            failure = message;
          case ChatSessionChanged(sessionId: final rotated):
            // The resume did not take and the gateway opened a fresh session.
            // Follow it, or the next history load reads the wrong one.
            ref.read(currentSessionProvider.notifier).select(rotated);
        }
      }

      if (failure != null) throw ChatTurnFailed(failure);

      if (reply.isNotEmpty) {
        final updated = state.value ?? const [];
        state = AsyncData([
          ...updated,
          ChatMessage(
            role: 'assistant',
            content: reply,
            timestamp: DateTime.now().toIso8601String(),
          ),
        ]);
      }
      streaming.finish();

      ref.read(mascotStateProvider.notifier).set(MascotState.talking);
      await Future<void>.delayed(const Duration(seconds: 2));
      ref.read(mascotStateProvider.notifier).set(MascotState.idle);
    } catch (e) {
      streaming.finish();
      final withoutOptimistic = (state.value ?? const [])
          .where((m) => m.content != text || m.role != 'user')
          .toList();
      state = AsyncData(withoutOptimistic);
      ref.read(mascotStateProvider.notifier).set(MascotState.idle);
      rethrow;
    }
  }

  /// Asks the runtime to cancel the turn in flight. What is on screen stays:
  /// the `stopped` frame ends the stream and [send] commits the partial text.
  Future<void> stop() => requireConnection(ref).stopStreaming();
}

/// Mascot animation state machine.
enum MascotState { idle, thinking, talking, happy }

/// Generated provider name: `mascotStateProvider` (riverpod_generator 4 drops
/// the `Notifier` suffix — the old `mascotStateNotifierProvider` alias is gone).
@riverpod
class MascotStateNotifier extends _$MascotStateNotifier {
  @override
  MascotState build() => MascotState.idle;

  void set(MascotState s) => state = s;
}
