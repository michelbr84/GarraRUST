import 'chat_event.dart';
import 'models.dart';
import 'runtime_config.dart';

/// Everything the UI needs from a Garra runtime, independent of where it
/// runs. `GatewayConnection` implements it over HTTP for the local (Termux)
/// and LAN cases; `CloudConnection` adapts the hosted API. A future embedded
/// runtime (Rust in the APK, ADR 0016 v2) plugs in here without touching a
/// single screen.
abstract interface class GarraConnection {
  RuntimeMode get mode;
  String get baseUrl;

  Future<GarraHealth> health();
  Future<GarraCapabilities> capabilities();

  // Chat
  Future<List<GarraSession>> listSessions();
  Future<String> createSession();
  Future<List<ChatMessage>> history(String? sessionId);

  /// Returns the assistant reply. [sessionId] is null only in cloud mode.
  ///
  /// Still the fallback path: [sendMessageStreaming] drops to it whenever the
  /// stream cannot be opened.
  Future<String> sendMessage(String text, {String? sessionId});

  /// Runs one turn and reports it as it happens.
  ///
  /// The stream always terminates with exactly one of [ChatCompleted],
  /// [ChatStopped] or [ChatFailed]. A runtime with no streaming endpoint is
  /// not an error: the implementation falls back to [sendMessage] and emits
  /// the whole reply as a single [ChatCompleted], so a caller never has to ask
  /// which mode it is talking to.
  Stream<ChatEvent> sendMessageStreaming(String text, {String? sessionId});

  /// Cancels the turn in flight, if there is one and the transport can carry
  /// the request. A no-op otherwise — a reply already on its way over HTTP
  /// cannot be recalled.
  Future<void> stopStreaming();

  // Memory
  Future<List<MemoryEntry>> recentMemory({int limit = 50});
  Future<List<MemoryEntry>> searchMemory(String query, {int limit = 30});

  /// `DELETE /api/memory/{id}`. `true` when it was there, `false` when the
  /// runtime no longer had it (already gone) — not an error for the user.
  Future<bool> deleteMemory(String id);

  // Skills / commands
  Future<List<SkillSummary>> skills();
  Future<List<SlashCommandInfo>> slashCommands();

  // Providers / agents / files
  Future<List<ProviderInfo>> providers();
  Future<List<ModeInfo>> modes();
  Future<List<McpServerInfo>> mcpServers();
  Future<List<ProjectInfo>> projects();
  Future<List<String>> projectFiles(String projectId);

  // Ops
  Future<List<String>> logs();

  // Voice
  Future<String> transcribe(String audioPath);
}

/// Thrown when a call needs a runtime and none is configured yet.
class NoRuntimeConfigured implements Exception {
  const NoRuntimeConfigured();

  @override
  String toString() => 'No Garra runtime configured';
}
