import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../services/api_service.dart';

part 'auth_provider.g.dart';

/// Holds the authenticated user info (null = not logged in).
@riverpod
class AuthState extends _$AuthState {
  @override
  Future<MeResult?> build() async {
    final api = ref.read(apiServiceProvider);
    final token = await api.getSavedToken();
    if (token == null) return null;
    try {
      return await api.me();
    } catch (_) {
      // Token expired or invalid — treat as logged out.
      await api.logout();
      return null;
    }
  }

  Future<void> login(String email, String password) async {
    state = const AsyncLoading();
    state = await AsyncValue.guard(() async {
      final api = ref.read(apiServiceProvider);
      await api.login(email, password);
      return api.me();
    });
  }

  Future<void> register(String email, String password) async {
    state = const AsyncLoading();
    state = await AsyncValue.guard(() async {
      final api = ref.read(apiServiceProvider);
      await api.register(email, password);
      return api.me();
    });
  }

  Future<void> logout() async {
    final api = ref.read(apiServiceProvider);
    await api.logout();
    state = const AsyncData(null);
  }
}
