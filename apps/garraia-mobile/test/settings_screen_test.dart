// Plan 0029 / GAR-358 — widget tests for SettingsScreen, updated for the
// v0.4.0 runtime model: the account card and the logout button only exist
// in cloud mode, so the test pins a cloud RuntimeConfig and stubs the
// health probe (no network in widget tests).

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/models.dart';
import 'package:garraia_mobile/runtime/runtime_config.dart';
import 'package:garraia_mobile/runtime/runtime_providers.dart';
import 'package:garraia_mobile/screens/settings_screen.dart';
import 'package:garraia_mobile/services/api_service.dart';
import 'package:go_router/go_router.dart';

class _StubApiService implements ApiService {
  final MeResult _me;
  bool logoutCalled = false;

  _StubApiService(this._me);

  @override
  Future<MeResult> me() async => _me;

  @override
  Future<void> logout() async {
    logoutCalled = true;
  }

  @override
  Future<AuthResult> register(String email, String password) =>
      throw UnimplementedError('register not used in SettingsScreen tests');

  @override
  Future<AuthResult> login(String email, String password) =>
      throw UnimplementedError('login not used in SettingsScreen tests');

  @override
  Future<String> sendMessage(String message) =>
      throw UnimplementedError('sendMessage not used in SettingsScreen tests');

  @override
  Future<List<ChatMessage>> getHistory() =>
      throw UnimplementedError('getHistory not used in SettingsScreen tests');

  @override
  Future<String> transcribeAudio(String audioPath) => throw UnimplementedError(
    'transcribeAudio not used in SettingsScreen tests',
  );

  @override
  Future<String?> getSavedToken() async => 'stub-jwt';
}

class _CloudConfig extends RuntimeConfigState {
  @override
  Future<RuntimeConfig?> build() async => const RuntimeConfig(
    mode: RuntimeMode.cloud,
    baseUrl: 'https://cloud.test',
    ownerName: 'Alice',
  );
}

const _health = GarraHealth(
  status: 'healthy',
  version: '0.4.0-test',
  gatewayUrl: 'https://cloud.test',
  uptimeSecs: 1,
  activeSessions: 0,
  provider: 'openrouter',
  model: 'auto',
);

ProviderContainer _container(_StubApiService api) {
  return ProviderContainer(
    // Riverpod 3 retries throwing providers with a backoff Timer; widget
    // tests must end with no pending timers.
    retry: (_, __) => null,
    overrides: [
      apiServiceProvider.overrideWithValue(api),
      runtimeConfigStateProvider.overrideWith(_CloudConfig.new),
      runtimeHealthProvider.overrideWith((ref) async => _health),
    ],
  );
}

Widget _wrap(ProviderContainer container, Widget child) {
  final router = GoRouter(
    initialLocation: '/',
    routes: [
      GoRoute(path: '/', builder: (_, __) => child),
      GoRoute(
        path: '/login',
        builder: (_, __) => const Scaffold(body: Text('LOGIN')),
      ),
      GoRoute(
        path: '/onboarding',
        builder: (_, __) => const Scaffold(body: Text('ONBOARDING')),
      ),
    ],
  );
  return UncontrolledProviderScope(
    container: container,
    child: MaterialApp.router(routerConfig: router),
  );
}

void main() {
  final me = MeResult(
    userId: '8f2c7e1a-1234-4abc-9def-0123456789ab',
    email: 'alice@example.com',
    createdAt: '2026-03-10T12:34:56Z',
  );

  testWidgets('renders runtime card, account info and logout (cloud mode)', (
    tester,
  ) async {
    final api = _StubApiService(me);
    final container = _container(api);
    addTearDown(container.dispose);

    await tester.pumpWidget(_wrap(container, const SettingsScreen()));
    await tester.pumpAndSettle();

    expect(find.text('Runtime'), findsOneWidget);
    expect(find.text('Garra Cloud'), findsOneWidget);
    expect(find.textContaining('0.4.0-test'), findsOneWidget);

    expect(find.text('alice@example.com'), findsOneWidget);
    expect(find.textContaining('8f2c7e1a'), findsOneWidget);
    expect(find.textContaining('89ab'), findsOneWidget);
    expect(find.text('2026-03-10'), findsOneWidget);
    expect(find.text('Sair da conta'), findsOneWidget);
    await tester.scrollUntilVisible(find.textContaining('Garra Mobile v'), 200);
    expect(find.textContaining('Garra Mobile v'), findsOneWidget);
  });

  testWidgets(
    'logout asks for confirmation; cancel keeps session, confirm logs out',
    (tester) async {
      final api = _StubApiService(me);
      final container = _container(api);
      addTearDown(container.dispose);

      await tester.pumpWidget(_wrap(container, const SettingsScreen()));
      await tester.pumpAndSettle();

      await tester.ensureVisible(find.text('Sair da conta'));
      await tester.tap(find.text('Sair da conta'));
      await tester.pumpAndSettle();
      expect(find.byType(AlertDialog), findsOneWidget);

      await tester.tap(find.text('Cancelar'));
      await tester.pumpAndSettle();
      expect(api.logoutCalled, isFalse);

      await tester.tap(find.text('Sair da conta'));
      await tester.pumpAndSettle();
      await tester.tap(
        find.descendant(
          of: find.byType(AlertDialog),
          matching: find.text('Sair'),
        ),
      );
      await tester.pumpAndSettle();

      expect(api.logoutCalled, isTrue);
      expect(find.text('LOGIN'), findsOneWidget);
    },
  );
}
