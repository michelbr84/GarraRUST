import 'dart:ui' show Locale, PlatformDispatcher;

import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'package:shared_preferences/shared_preferences.dart';

part 'app_language.g.dart';

/// The UI language the user picked in Settings (#1178).
///
/// `system` follows the device locale (resolved by [resolveLocale] to one of
/// [supportedLocales]); the others force a language regardless of the device.
/// Persisted in SharedPreferences under [AppLanguageState.prefsKey] so the
/// choice survives restarts — the device may be in English while the user
/// wants the app in Portuguese, or the other way round.
enum AppLanguage {
  system,
  en,
  ptBR;

  /// The locale to hand to `MaterialApp.locale`; `null` means "system".
  Locale? get locale => switch (this) {
    AppLanguage.system => null,
    AppLanguage.en => const Locale('en'),
    AppLanguage.ptBR => const Locale('pt', 'BR'),
  };

  /// Stable string stored in prefs (never the enum index — reordering the
  /// enum must not flip anyone's language).
  String get storageValue => switch (this) {
    AppLanguage.system => 'system',
    AppLanguage.en => 'en',
    AppLanguage.ptBR => 'pt-BR',
  };

  static AppLanguage fromStorage(String? raw) => switch (raw) {
    'en' => AppLanguage.en,
    'pt-BR' => AppLanguage.ptBR,
    _ => AppLanguage.system,
  };
}

/// Locales the app ships. Order matters: the first entry is the fallback
/// when the device locale matches none of them.
const supportedLocales = [Locale('en'), Locale('pt', 'BR')];

/// Maps a device locale onto one of [supportedLocales]: any `pt` variant
/// (pt-BR, pt-PT, pt) becomes pt-BR, everything else falls back to English.
/// Used as `MaterialApp.localeResolutionCallback` and by [effectiveLocale].
Locale resolveLocale(Locale? device, Iterable<Locale> supported) {
  if (device != null && device.languageCode == 'pt') {
    return const Locale('pt', 'BR');
  }
  return const Locale('en');
}

/// The locale actually in effect for [language]: the forced one, or the
/// device locale resolved onto the supported set.
Locale effectiveLocale(AppLanguage language) =>
    language.locale ??
    resolveLocale(PlatformDispatcher.instance.locale, supportedLocales);

/// The `SharedPreferences` instance, loaded once in `main()` before
/// `runApp` and injected through a `ProviderScope` override — so the
/// language is known synchronously on the first frame (no flash of the
/// wrong language while an async load resolves).
@Riverpod(keepAlive: true)
SharedPreferences sharedPreferences(Ref ref) => throw UnimplementedError(
  'sharedPreferencesProvider must be overridden in main() / tests',
);

/// The persisted UI language.
@Riverpod(keepAlive: true)
class AppLanguageState extends _$AppLanguageState {
  static const prefsKey = 'app_language';

  @override
  AppLanguage build() {
    final prefs = ref.watch(sharedPreferencesProvider);
    return AppLanguage.fromStorage(prefs.getString(prefsKey));
  }

  Future<void> set(AppLanguage language) async {
    state = language;
    final prefs = ref.read(sharedPreferencesProvider);
    if (language == AppLanguage.system) {
      await prefs.remove(prefsKey);
    } else {
      await prefs.setString(prefsKey, language.storageValue);
    }
  }
}
