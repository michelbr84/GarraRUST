// Shared plumbing for widget tests after #1178: every screen reads its
// strings from `context.l10n`, so a `MaterialApp` under test needs the
// localizations delegates, and providers that read the persisted language
// need `sharedPreferencesProvider` overridden.
//
// Tests assert on the pt-BR copy by default (the project's primary language);
// pass `locale: const Locale('en')` to assert on English.

import 'package:flutter/material.dart';
import 'package:garraia_mobile/l10n/l10n.dart';
import 'package:shared_preferences/shared_preferences.dart';

export 'package:garraia_mobile/l10n/l10n.dart';

const testLocalePt = Locale('pt', 'BR');
const testLocaleEn = Locale('en');

/// A `MaterialApp` with the app's delegates, pinned to [locale].
Widget localizedApp(Widget home, {Locale locale = testLocalePt}) {
  return MaterialApp(
    locale: locale,
    supportedLocales: supportedLocales,
    localizationsDelegates: localizationsDelegates,
    home: home,
  );
}

/// In-memory `SharedPreferences` for `sharedPreferencesProvider`. Call inside
/// the test body (it is async) and hand it to the container:
///
/// ```dart
/// sharedPreferencesProvider.overrideWithValue(await mockSharedPreferences())
/// ```
Future<SharedPreferences> mockSharedPreferences({
  Map<String, Object> initial = const {},
}) async {
  SharedPreferences.setMockInitialValues(Map<String, Object>.of(initial));
  return SharedPreferences.getInstance();
}

/// The strings for [locale] without a widget tree — for asserting on the
/// exact copy (`l10n.settingsTitle`) instead of repeating literals in tests.
Future<AppLocalizations> loadL10n([Locale locale = testLocalePt]) =>
    AppLocalizations.delegate.load(locale);
