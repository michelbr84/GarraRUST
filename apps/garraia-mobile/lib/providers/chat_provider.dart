import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../services/api_service.dart';

part 'chat_provider.g.dart';

/// Holds the in-memory list of chat messages (loaded from history + appended on send).
@riverpod
class ChatMessages extends _$ChatMessages {
  @override
  Future<List<ChatMessage>> build() async {
    final api = ref.read(apiServiceProvider);
    return api.getHistory();
  }

  Future<void> send(String text) async {
    // Optimistically add user message
    final current = state.valueOrNull ?? [];
    state = AsyncData([
      ...current,
      ChatMessage(
        role: 'user',
        content: text,
        timestamp: DateTime.now().toIso8601String(),
      ),
    ]);

    ref.read(mascotStateNotifierProvider.notifier).set(MascotState.thinking);

    try {
      final api = ref.read(apiServiceProvider);
      final reply = await api.sendMessage(text);

      final updated = state.valueOrNull ?? [];
      state = AsyncData([
        ...updated,
        ChatMessage(
          role: 'assistant',
          content: reply,
          timestamp: DateTime.now().toIso8601String(),
        ),
      ]);

      ref.read(mascotStateNotifierProvider.notifier).set(MascotState.talking);
      await Future.delayed(const Duration(seconds: 2));
      ref.read(mascotStateNotifierProvider.notifier).set(MascotState.idle);
    } catch (e) {
      final withoutOptimistic = (state.valueOrNull ?? [])
          .where((m) => m.content != text || m.role != 'user')
          .toList();
      state = AsyncData(withoutOptimistic);
      ref.read(mascotStateNotifierProvider.notifier).set(MascotState.idle);
      rethrow;
    }
  }
}

/// Mascot animation state machine.
enum MascotState { idle, thinking, talking, happy }

@riverpod
class MascotStateNotifier extends _$MascotStateNotifier {
  @override
  MascotState build() => MascotState.idle;

  void set(MascotState s) => state = s;
}

/// Shorthand alias used throughout the UI.
final mascotStateProvider = mascotStateNotifierProvider;
