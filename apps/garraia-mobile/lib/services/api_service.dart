import 'package:dio/dio.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'api_service.g.dart';

/// Base URL for the Garra Cloud Alpha backend.
/// Override with env var at build time: --dart-define=API_BASE_URL=https://api.garraia.org
const String kApiBaseUrl = String.fromEnvironment(
  'API_BASE_URL',
  defaultValue: 'https://api.garraia.org',
);

const _kTokenKey = 'garraia_jwt';

@riverpod
ApiService apiService(Ref ref) => ApiService();

class ApiService {
  final Dio _dio;
  final FlutterSecureStorage _storage;

  ApiService()
      : _dio = Dio(BaseOptions(
          baseUrl: kApiBaseUrl,
          connectTimeout: const Duration(seconds: 10),
          receiveTimeout: const Duration(seconds: 30),
          headers: {'Content-Type': 'application/json'},
        )),
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

  // ── Token ────────────────────────────────────────────────────────────────

  Future<String?> getSavedToken() => _storage.read(key: _kTokenKey);
}

// ── Interceptor ──────────────────────────────────────────────────────────────

class _AuthInterceptor extends Interceptor {
  final FlutterSecureStorage _storage;
  _AuthInterceptor(this._storage);

  @override
  void onRequest(RequestOptions options, RequestInterceptorHandler handler) async {
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

  MeResult({required this.userId, required this.email, required this.createdAt});

  factory MeResult.fromJson(Map<String, dynamic> json) => MeResult(
        userId: json['user_id'] as String,
        email: json['email'] as String,
        createdAt: json['created_at'] as String,
      );
}

class ChatMessage {
  final String role;   // "user" | "assistant"
  final String content;
  final String timestamp;

  ChatMessage({required this.role, required this.content, required this.timestamp});

  factory ChatMessage.fromJson(Map<String, dynamic> json) => ChatMessage(
        role: json['role'] as String,
        content: json['content'] as String,
        timestamp: json['timestamp'] as String,
      );
}
