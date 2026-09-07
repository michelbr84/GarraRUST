import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../runtime/models.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';

part 'chat_provider.g.dart';

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

  Future<void> send(String text) async {
    final conn = requireConnection(ref);

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

    try {
      final sessionId = conn.mode == RuntimeMode.cloud
          ? null
          : await ref.read(currentSessionProvider.notifier).ensure();
      final reply = await conn.sendMessage(text, sessionId: sessionId);

      final updated = state.value ?? const [];
      state = AsyncData([
        ...updated,
        ChatMessage(
          role: 'assistant',
          content: reply,
          timestamp: DateTime.now().toIso8601String(),
        ),
      ]);

      ref.read(mascotStateProvider.notifier).set(MascotState.talking);
      await Future<void>.delayed(const Duration(seconds: 2));
      ref.read(mascotStateProvider.notifier).set(MascotState.idle);
    } catch (e) {
      final withoutOptimistic = (state.value ?? const [])
          .where((m) => m.content != text || m.role != 'user')
          .toList();
      state = AsyncData(withoutOptimistic);
      ref.read(mascotStateProvider.notifier).set(MascotState.idle);
      rethrow;
    }
  }
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
