import 'package:dio/dio.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../runtime/models.dart';
import '../runtime/runtime_config.dart';

export '../runtime/models.dart' show ChatMessage;

part 'api_service.g.dart';

/// Base URL of the hosted Garra Cloud API. Kept for callers that still read
/// it; new code should go through `RuntimeConfig.baseUrl`.
const String kApiBaseUrl = kCloudBaseUrl;

const _kTokenKey = 'garraia_jwt';

@riverpod
ApiService apiService(Ref ref) => ApiService();

/// Garra Cloud Alpha account + chat client (`mobile_auth.rs`,
/// `mobile_chat.rs`). Local and LAN runtimes never touch this class — they go
/// through `GatewayConnection`.
class ApiService {
  final Dio _dio;
  final FlutterSecureStorage _storage;

  ApiService({String? baseUrl})
    : _dio = Dio(
        BaseOptions(
          baseUrl: baseUrl ?? kCloudBaseUrl,
          connectTimeout: const Duration(seconds: 10),
          receiveTimeout: const Duration(seconds: 30),
          headers: {'Content-Type': 'application/json'},
        ),
      ),
      _storage = const FlutterSecureStorage() {
    _dio.interceptors.add(_AuthInterceptor(_storage));
  }

  // ── Auth ─────────────────────────────────────────────────────────────────

  Future<AuthResult> register(String email, String password) async {
    final resp = await _dio.post<Map<String, dynamic>>(
      '/auth/register',
      data: {'email': email, 'password': password},
    );
    final result = AuthResult.fromJson(resp.data!);
    await _storage.write(key: _kTokenKey, value: result.token);
    return result;
  }

  Future<AuthResult> login(String email, String password) async {
    final resp = await _dio.post<Map<String, dynamic>>(
      '/auth/login',
      data: {'email': email, 'password': password},
    );
    final result = AuthResult.fromJson(resp.data!);
    await _storage.write(key: _kTokenKey, value: result.token);
    return result;
  }

  Future<void> logout() async {
    await _storage.delete(key: _kTokenKey);
  }

  Future<MeResult> me() async {
    final resp = await _dio.get<Map<String, dynamic>>('/me');
    return MeResult.fromJson(resp.data!);
  }

  // ── Chat ─────────────────────────────────────────────────────────────────

  Future<String> sendMessage(String message) async {
    final resp = await _dio.post<Map<String, dynamic>>(
      '/chat',
      data: {'message': message},
    );
    return resp.data!['reply'] as String;
  }

  Future<List<ChatMessage>> getHistory() async {
    final resp = await _dio.get<Map<String, dynamic>>('/chat/history');
    final list = resp.data!['messages'] as List<dynamic>;
    return list
        .map((e) => ChatMessage.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  // ── Voice ─────────────────────────────────────────────────────────────────

  /// `POST /api/stt` (`voice_handler.rs`). The old client called
  /// `/api/voice/transcribe`, a path the gateway never exposed.
  Future<String> transcribeAudio(String audioPath) async {
    final resp = await _dio.post<Map<String, dynamic>>(
      '/api/stt',
      data: FormData.fromMap({
        'audio': await MultipartFile.fromFile(audioPath),
      }),
    );
    return resp.data?['text'] as String? ?? 'Transcricao indisponivel';
  }

  // ── Token ────────────────────────────────────────────────────────────────

  Future<String?> getSavedToken() => _storage.read(key: _kTokenKey);
}

// ── Interceptor ──────────────────────────────────────────────────────────────

class _AuthInterceptor extends Interceptor {
  final FlutterSecureStorage _storage;
  _AuthInterceptor(this._storage);

  @override
  void onRequest(
    RequestOptions options,
    RequestInterceptorHandler handler,
  ) async {
    final token = await _storage.read(key: _kTokenKey);
    if (token != null) {
      options.headers['Authorization'] = 'Bearer $token';
    }
    handler.next(options);
  }
}

// ── Models ───────────────────────────────────────────────────────────────────

class AuthResult {
  final String token;
  final String userId;
  final String email;

  AuthResult({required this.token, required this.userId, required this.email});

  factory AuthResult.fromJson(Map<String, dynamic> json) => AuthResult(
    token: json['token'] as String,
    userId: json['user_id'] as String,
    email: json['email'] as String,
  );
}

class MeResult {
  final String userId;
  final String email;
  final String createdAt;

  MeResult({
    required this.userId,
    required this.email,
    required this.createdAt,
  });

  factory MeResult.fromJson(Map<String, dynamic> json) => MeResult(
    userId: json['user_id'] as String,
    email: json['email'] as String,
    createdAt: json['created_at'] as String,
  );
}
