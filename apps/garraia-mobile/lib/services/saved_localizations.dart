import 'package:shared_preferences/shared_preferences.dart';

import '../l10n/l10n.dart';

/// The app's strings in the language the user picked in Settings (#1178),
/// for code that has no `BuildContext`: services that start before `runApp`
/// (notification channels), the chat transport running under a provider, the
/// OS biometric prompt.
///
/// Reads the SharedPreferences key `AppLanguageState` persists, so it agrees
/// with what `MaterialApp.locale` shows; "system" resolves through the device
/// locale exactly like the app does. Never throws: when preferences are
/// unreachable (a test without a mock store, a plugin not registered yet) or
/// the locale is unknown, the answer is English — the first supported locale
/// is the documented fallback.
Future<AppLocalizations> savedLocalizations() async {
  try {
    final prefs = await SharedPreferences.getInstance();
    final language = AppLanguage.fromStorage(
      prefs.getString(AppLanguageState.prefsKey),
    );
    return lookupAppLocalizations(effectiveLocale(language));
  } catch (_) {
    // Deliberately broad: a missing plugin is an Exception, an uninitialised
    // binding or an unsupported locale is an Error, and none of them may take
    // a notification or a chat turn down over a label.
    return lookupAppLocalizations(supportedLocales.first);
  }
}
