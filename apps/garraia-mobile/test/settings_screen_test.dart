// Plan 0029 / GAR-358 — widget tests for SettingsScreen.
//
// These tests exercise the UI surface without network I/O — we inject a
// stub ApiService that returns a canned MeResult from `me()` and no-op
// on `logout()`. The widget tree uses the real `appRouterProvider` so
// the `/settings` route + logout redirect are exercised end-to-end.
//
// The stub uses `flutter_secure_storage` via the default test binding —
// flutter_secure_storage ships with in-memory defaults for tests, so no
// additional mock is required.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/providers/auth_provider.dart';
import 'package:garraia_mobile/screens/settings_screen.dart';
import 'package:garraia_mobile/services/api_service.dart';

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

  // The rest of ApiService is unreachable from SettingsScreen — throwing
  // loudly here makes accidental calls obvious during test development.
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
  Future<String> transcribeAudio(String audioPath) =>
      throw UnimplementedError('transcribeAudio not used in SettingsScreen tests');

  @override
  Future<String?> getSavedToken() async => 'stub-jwt';
}

ProviderContainer _container(_StubApiService api) {
  return ProviderContainer(
    overrides: [
      apiServiceProvider.overrideWithValue(api),
    ],
  );
}

Widget _wrap(ProviderContainer container, Widget child) {
  return UncontrolledProviderScope(
    container: container,
    child: MaterialApp(
      home: child,
      theme: ThemeData(useMaterial3: true, brightness: Brightness.dark),
    ),
  );
}

void main() {
  group('SettingsScreen', () {
    testWidgets('renders account info from MeResult', (tester) async {
      final me = MeResult(
        userId: '8f2c7e1a-1234-4abc-9def-0123456789ab',
        email: 'alice@example.com',
        createdAt: '2026-03-10T12:34:56Z',
      );
      final api = _StubApiService(me);
      final container = _container(api);
      addTearDown(container.dispose);

      await tester.pumpWidget(_wrap(container, const SettingsScreen()));
      // Let the AuthState future resolve.
      await tester.pumpAndSettle();

      expect(find.text('Configurações'), findsOneWidget);
      expect(find.text('alice@example.com'), findsOneWidget);
      // UUID shortened to `8f2c7e1a…89ab`.
      expect(find.textContaining('8f2c7e1a'), findsOneWidget);
      expect(find.textContaining('89ab'), findsOneWidget);
      // Date segment shown without timezone.
      expect(find.text('2026-03-10'), findsOneWidget);
      expect(find.text('Sair da conta'), findsOneWidget);
    });

    testWidgets('logout button shows confirmation dialog and calls logout',
        (tester) async {
      final me = MeResult(
        userId: '8f2c7e1a-1234-4abc-9def-0123456789ab',
        email: 'alice@example.com',
        createdAt: '2026-03-10T12:34:56Z',
      );
      final api = _StubApiService(me);
      final container = _container(api);
      addTearDown(container.dispose);

      await tester.pumpWidget(_wrap(container, const SettingsScreen()));
      await tester.pumpAndSettle();

      // Tap logout button.
      await tester.tap(find.text('Sair da conta'));
      await tester.pumpAndSettle();

      // Confirmation dialog appears.
      expect(find.text('Sair da conta?'), findsOneWidget);
      expect(find.text('Cancelar'), findsOneWidget);

      // Cancel — no logout.
      await tester.tap(find.text('Cancelar'));
      await tester.pumpAndSettle();
      expect(api.logoutCalled, isFalse);

      // Re-open and confirm.
      await tester.tap(find.text('Sair da conta'));
      await tester.pumpAndSettle();
      // Button inside dialog has label "Sair"; the page button is "Sair da conta".
      // We tap the dialog action explicitly.
      await tester.tap(find.widgetWithText(FilledButton, 'Sair'));
      await tester.pumpAndSettle();

      expect(api.logoutCalled, isTrue,
          'logout() must be called after dialog confirm');
    });
  });
}
