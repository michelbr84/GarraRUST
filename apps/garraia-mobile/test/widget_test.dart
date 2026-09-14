import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/l10n/l10n.dart';
import 'package:garraia_mobile/main.dart';
import 'package:garraia_mobile/screens/onboarding_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  testWidgets('GarraApp smoke test — first launch lands on onboarding', (
    WidgetTester tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    FlutterSecureStorage.setMockInitialValues({});
    // main() loads prefs before runApp and injects them (#1178); the smoke
    // test does the same, otherwise the language provider has no store.
    final prefs = await SharedPreferences.getInstance();

    await tester.pumpWidget(
      ProviderScope(
        overrides: [sharedPreferencesProvider.overrideWithValue(prefs)],
        child: const GarraApp(),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byType(MaterialApp), findsOneWidget);
    // No runtime configured → the router gate sends a fresh install to onboarding.
    expect(find.byType(OnboardingScreen), findsOneWidget);
  });
}
