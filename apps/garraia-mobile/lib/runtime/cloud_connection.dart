import 'package:dio/dio.dart';

import '../services/api_service.dart';
import 'gateway_connection.dart';
import 'models.dart';
import 'runtime_config.dart';

/// The hosted Garra Cloud is the same Rust gateway, so every read endpoint
/// (`/api/health`, `/api/capabilities`, memory, skills, …) is inherited from
/// [GatewayConnection]. Chat is the exception: Cloud Alpha talks through the
/// JWT-protected `/chat` + `/chat/history` (`mobile_chat.rs`) instead of
/// `/api/sessions`, and is single-threaded per account.
class CloudConnection extends GatewayConnection {
  final ApiService _api;

  CloudConnection({required ApiService api, String? baseUrl})
    : _api = api,
      super(
        mode: RuntimeMode.cloud,
        baseUrl: baseUrl ?? kCloudBaseUrl,
        dio: _cloudDio(baseUrl ?? kCloudBaseUrl, api),
      );

  static Dio _cloudDio(String baseUrl, ApiService api) {
    final dio = Dio(
      BaseOptions(
        baseUrl: baseUrl,
        connectTimeout: const Duration(seconds: 10),
        receiveTimeout: const Duration(seconds: 120),
        headers: {'Content-Type': 'application/json'},
      ),
    );
    dio.interceptors.add(_JwtInterceptor(api));
    return dio;
  }

  @override
  Future<List<GarraSession>> listSessions() async => const [
    GarraSession(sessionId: 'cloud', connected: true),
  ];

  @override
  Future<String> createSession() async => 'cloud';

  @override
  Future<List<ChatMessage>> history(String? sessionId) => _api.getHistory();

  @override
  Future<String> sendMessage(String text, {String? sessionId}) =>
      _api.sendMessage(text);
}

class _JwtInterceptor extends Interceptor {
  final ApiService _api;
  _JwtInterceptor(this._api);

  @override
  void onRequest(
    RequestOptions options,
    RequestInterceptorHandler handler,
  ) async {
    final token = await _api.getSavedToken();
    if (token != null) options.headers['Authorization'] = 'Bearer $token';
    handler.next(options);
  }
}
