import 'dart:async';
import 'dart:convert';

import 'package:dio/dio.dart';

import 'chat_event.dart';
import 'chat_socket.dart';
import 'garra_connection.dart';
import 'models.dart';
import 'runtime_config.dart';

/// `GarraConnection` over the gateway's `/api/*` surface. One class serves
/// both the on-device runtime (`http://127.0.0.1:<port>`, Termux) and a Garra
/// on the LAN — they differ only by base URL, optional API key and label.
///
/// Auth: when the gateway has `gateway.api_key` set, the key goes out as
/// `Authorization: Bearer` (what `session_auth.rs` and `ws.rs` read). Session
/// tokens (GAR-202) arrive as a `garraia_session` cookie on
/// `POST /api/sessions`; Dio has no cookie jar on mobile, so the cookie is
/// captured once and replayed as a `Cookie` header — only when
/// [replaySessionCookie] is on. The cloud subclass turns it off: its chat
/// is JWT-authenticated and never creates a gateway session, so a cookie
/// there could only be something a hostile response planted.
class GatewayConnection implements GarraConnection {
  @override
  final RuntimeMode mode;
  @override
  final String baseUrl;

  final Dio _dio;
  String? _sessionCookie;

  /// Kept for the WebSocket URL: `WebSocketChannel.connect` cannot set headers
  /// on the web target, so the gateway key rides in the query string.
  final String? _apiKey;

  /// How the streaming transport is built. Swapped in tests.
  final ChatSocketFactory _socketFactory;

  /// The socket carrying the current turn, if any. `stopStreaming` needs it,
  /// and it is the only mutable handle the class keeps.
  ChatSocket? _activeSocket;

  /// Session the gateway acknowledged for the current turn — not necessarily
  /// the one we asked for, since a failed resume rotates it.
  String? _activeSessionId;

  GatewayConnection({
    required this.mode,
    required this.baseUrl,
    String? apiKey,
    Dio? dio,
    bool replaySessionCookie = true,
    ChatSocketFactory socketFactory = connectWebSocket,
  }) : _apiKey = apiKey,
       _socketFactory = socketFactory,
       _dio =
           dio ??
           Dio(
             BaseOptions(
               baseUrl: baseUrl,
               connectTimeout: const Duration(seconds: 6),
               receiveTimeout: const Duration(seconds: 120),
               headers: {
                 'Content-Type': 'application/json',
                 if (apiKey != null && apiKey.isNotEmpty)
                   'Authorization': 'Bearer $apiKey',
               },
             ),
           ) {
    if (replaySessionCookie) _dio.interceptors.add(_CookieReplay(this));
  }

  // ── Health / capabilities ────────────────────────────────────────────────

  @override
  Future<GarraHealth> health() async {
    final r = await _dio.get<Map<String, dynamic>>(
      '/api/health',
      options: Options(receiveTimeout: const Duration(seconds: 6)),
    );
    return GarraHealth.fromJson(r.data ?? const {});
  }

  @override
  Future<GarraCapabilities> capabilities() async {
    final r = await _dio.get<Map<String, dynamic>>(
      '/api/capabilities',
      options: Options(receiveTimeout: const Duration(seconds: 6)),
    );
    return GarraCapabilities.fromJson(r.data ?? const {});
  }

  // ── Chat ─────────────────────────────────────────────────────────────────

  @override
  Future<List<GarraSession>> listSessions() async {
    final r = await _dio.get<Object>('/api/sessions');
    return unwrapList(r.data, const [
      'sessions',
    ]).map(GarraSession.fromJson).toList();
  }

  @override
  Future<String> createSession() async {
    final r = await _dio.post<Map<String, dynamic>>(
      '/api/sessions',
      data: const {},
    );
    final id = r.data?['session_id'] as String?;
    if (id == null || id.isEmpty) {
      throw DioException(
        requestOptions: r.requestOptions,
        message: 'session_id missing',
      );
    }
    return id;
  }

  @override
  Future<List<ChatMessage>> history(String? sessionId) async {
    if (sessionId == null || sessionId.isEmpty) return const [];
    try {
      final r = await _dio.get<Object>('/api/sessions/$sessionId/history');
      return unwrapList(r.data, const [
        'messages',
      ]).map(ChatMessage.fromJson).toList();
    } on DioException catch (e) {
      // A session the gateway no longer knows (restart, eviction) is not an
      // error for the user — the next send creates a fresh one.
      if (e.response?.statusCode == 404) return const [];
      rethrow;
    }
  }

  @override
  Future<String> sendMessage(String text, {String? sessionId}) async {
    if (sessionId == null || sessionId.isEmpty) {
      throw ArgumentError('sessionId is required for gateway chat');
    }
    final r = await _dio.post<Map<String, dynamic>>(
      '/api/sessions/$sessionId/messages',
      data: {'content': text},
    );
    // `SendMessageResponse.content` is a required String on the Rust side
    // (api.rs). A 200 without it is a contract break, not an empty reply —
    // surface it instead of appending a blank assistant bubble.
    final content = r.data?['content'] as String?;
    if (content == null) {
      throw DioException(
        requestOptions: r.requestOptions,
        response: r,
        message: 'reply content missing from the gateway response',
      );
    }
    return content;
  }

  /// Opening budget for the socket. Short on purpose: when there is no `/ws`
  /// to talk to, the POST fallback still has to feel immediate.
  static const _openTimeout = Duration(seconds: 6);

  /// Longest silence tolerated inside a turn. Every frame resets it, so it
  /// only fires when the runtime stops talking altogether. Same order as the
  /// Dio `receiveTimeout` the POST path already uses.
  static const _silenceTimeout = Duration(seconds: 120);

  @override
  Stream<ChatEvent> sendMessageStreaming(
    String text, {
    String? sessionId,
  }) async* {
    if (sessionId == null || sessionId.isEmpty) {
      throw ArgumentError('sessionId is required for gateway chat');
    }

    // The socket never opening is the ordinary case on an older gateway, a
    // proxy that refuses the upgrade, or a LAN that dropped. Nothing was
    // submitted yet, so the POST path can still run the turn and the user sees
    // a reply rather than an error. This is the *only* safe place to fall
    // back: once the content frame goes out, retrying over HTTP would send the
    // same message twice.
    final ChatSocket socket;
    try {
      socket = _socketFactory(chatSocketUri(baseUrl, apiKey: _apiKey));
    } catch (_) {
      yield* _sendOverPost(text, sessionId);
      return;
    }
    try {
      await socket.ready.timeout(_openTimeout);
    } catch (_) {
      unawaited(socket.close());
      yield* _sendOverPost(text, sessionId);
      return;
    }

    _activeSocket = socket;
    _activeSessionId = sessionId;
    var submitted = false;

    try {
      // Resume the session the app already has, so the turn lands in the same
      // history the HTTP path reads. A resume the gateway cannot honour is not
      // refused: it answers `connected` with a *different* session.
      socket.send(jsonEncode({'type': 'resume', 'session_id': sessionId}));

      await for (final raw in socket.incoming.timeout(_silenceTimeout)) {
        if (!submitted) {
          final acked = _handshakeSessionId(raw, requested: sessionId);
          if (acked == null) {
            // An error during the handshake means no turn ever started, so the
            // POST fallback is still safe. Anything else: keep waiting.
            if (ChatEvent.tryParse(raw) is ChatFailed) break;
            continue;
          }
          if (acked != sessionId) {
            _activeSessionId = acked;
            yield ChatSessionChanged(acked);
          }
          socket.send(jsonEncode({'content': text}));
          submitted = true;
          continue;
        }

        final event = ChatEvent.tryParse(raw);
        // An unknown frame is skipped, never fatal: the gateway may grow a
        // frame type before the app learns it.
        if (event == null) continue;
        yield event;
        if (event is ChatCompleted ||
            event is ChatStopped ||
            event is ChatFailed) {
          return;
        }
      }

      // The stream ended without a terminator.
      if (!submitted) {
        yield* _sendOverPost(text, sessionId);
      } else {
        yield const ChatFailed('a conexao caiu antes de a resposta terminar');
      }
    } catch (e) {
      if (!submitted) {
        yield* _sendOverPost(text, sessionId);
      } else {
        yield ChatFailed('$e');
      }
    } finally {
      _activeSocket = null;
      _activeSessionId = null;
      unawaited(socket.close());
    }
  }

  @override
  Future<void> stopStreaming() async {
    final socket = _activeSocket;
    final sessionId = _activeSessionId;
    if (socket == null || sessionId == null) return;
    // Always addressed: an unaddressed stop cancels whatever turn the socket
    // happens to be running, and the acknowledged id is the only one the
    // gateway matches against (`ws.rs`).
    socket.send(jsonEncode({'type': 'stop', 'session_id': sessionId}));
  }

  /// The session id a handshake frame acknowledges, or `null` when [raw] is
  /// not a handshake frame.
  static String? _handshakeSessionId(String raw, {required String requested}) {
    final Object? decoded;
    try {
      decoded = jsonDecode(raw);
    } on FormatException {
      return null;
    }
    if (decoded is! Map<String, dynamic>) return null;
    final type = decoded['type'];
    if (type != 'connected' && type != 'resumed') return null;
    final acked = decoded['session_id'];
    return acked is String && acked.isNotEmpty ? acked : requested;
  }

  /// One turn over `POST /api/sessions/{id}/messages`, shaped like a stream.
  Stream<ChatEvent> _sendOverPost(String text, String sessionId) async* {
    yield ChatCompleted(await sendMessage(text, sessionId: sessionId));
  }

  // ── Memory ───────────────────────────────────────────────────────────────

  @override
  Future<List<MemoryEntry>> recentMemory({int limit = 50}) async {
    final r = await _dio.get<Object>(
      '/api/memory/recent',
      queryParameters: {'limit': limit},
    );
    return unwrapList(r.data, const [
      'memories',
    ]).map(MemoryEntry.fromJson).toList();
  }

  @override
  Future<List<MemoryEntry>> searchMemory(String query, {int limit = 30}) async {
    final r = await _dio.get<Object>(
      '/api/memory/search',
      queryParameters: {'q': query, 'query': query, 'limit': limit},
    );
    return unwrapList(r.data, const [
      'memories',
      'results',
    ]).map(MemoryEntry.fromJson).toList();
  }

  @override
  Future<bool> deleteMemory(String id) async {
    try {
      await _dio.delete<void>('/api/memory/${Uri.encodeComponent(id)}');
      return true;
    } on DioException catch (e) {
      if (e.response?.statusCode == 404) return false;
      rethrow;
    }
  }

  // ── Skills / commands ────────────────────────────────────────────────────

  @override
  Future<List<SkillSummary>> skills() async {
    final r = await _dio.get<Object>('/api/learning/skills');
    return unwrapList(r.data, const [
      'skills',
    ]).map(SkillSummary.fromJson).toList();
  }

  @override
  Future<List<SlashCommandInfo>> slashCommands() async {
    final r = await _dio.get<Object>('/api/slash-commands');
    return unwrapList(r.data, const [
      'commands',
    ]).map(SlashCommandInfo.fromJson).toList();
  }

  // ── Providers / agents / files ───────────────────────────────────────────

  @override
  Future<List<ProviderInfo>> providers() async {
    final r = await _dio.get<Object>('/api/providers');
    return unwrapList(r.data, const [
      'providers',
    ]).map(ProviderInfo.fromJson).toList();
  }

  @override
  Future<List<ModeInfo>> modes() async {
    final r = await _dio.get<Object>('/api/modes');
    return unwrapList(r.data, const ['modes']).map(ModeInfo.fromJson).toList();
  }

  @override
  Future<List<McpServerInfo>> mcpServers() async {
    final r = await _dio.get<Object>('/api/mcp');
    return unwrapList(r.data, const [
      'servers',
      'mcp',
    ]).map(McpServerInfo.fromJson).toList();
  }

  @override
  Future<List<ProjectInfo>> projects() async {
    final r = await _dio.get<Object>('/api/projects');
    return unwrapList(r.data, const [
      'projects',
    ]).map(ProjectInfo.fromJson).toList();
  }

  @override
  Future<List<String>> projectFiles(String projectId) async {
    final r = await _dio.get<Object>('/api/projects/$projectId/files');
    return unwrapNames(r.data, const ['files']);
  }

  // ── Ops ──────────────────────────────────────────────────────────────────

  @override
  Future<List<String>> logs() async {
    final r = await _dio.get<Object>('/api/logs');
    final data = r.data;
    if (data is Map && data['logs'] is String) {
      return (data['logs'] as String).split('\n');
    }
    return unwrapNames(data, const ['logs']);
  }

  // ── Voice ────────────────────────────────────────────────────────────────

  @override
  Future<String> transcribe(String audioPath) async {
    final r = await _dio.post<Map<String, dynamic>>(
      '/api/stt',
      data: FormData.fromMap({
        'audio': await MultipartFile.fromFile(audioPath),
      }),
      options: Options(contentType: 'multipart/form-data'),
    );
    return r.data?['text'] as String? ?? '';
  }
}

/// Captures `Set-Cookie: garraia_session=…` and replays it on every request.
class _CookieReplay extends Interceptor {
  final GatewayConnection _owner;
  _CookieReplay(this._owner);

  @override
  void onRequest(RequestOptions options, RequestInterceptorHandler handler) {
    final c = _owner._sessionCookie;
    if (c != null) options.headers['Cookie'] = c;
    handler.next(options);
  }

  @override
  void onResponse(Response response, ResponseInterceptorHandler handler) {
    final raw = response.headers['set-cookie'];
    if (raw != null) {
      for (final line in raw) {
        final pair = line.split(';').first.trim();
        if (pair.startsWith('garraia_session=')) {
          _owner._sessionCookie = pair;
        }
      }
    }
    handler.next(response);
  }
}
