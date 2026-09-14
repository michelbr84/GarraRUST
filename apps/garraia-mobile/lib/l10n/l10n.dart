import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import '../runtime/garra_connection.dart' show NoRuntimeConfigured;
import 'generated/app_localizations.dart';

export 'app_language.dart';
export 'generated/app_localizations.dart';

/// `context.l10n.settingsTitle` instead of
/// `AppLocalizations.of(context).settingsTitle` everywhere (#1178).
extension L10nContext on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);
}

/// The one app-side exception that screens show as-is gets the localized
/// copy; everything else (Dio, gateway JSON, Dart errors) is transport text
/// and is shown verbatim. Single place, so the three error surfaces (chat
/// SnackBar, conversation loader, `AsyncBody`) cannot disagree.
String describeError(AppLocalizations l10n, Object error) =>
    error is NoRuntimeConfigured
    ? l10n.errorNoRuntimeConfigured
    : error.toString();

/// Every delegate a `MaterialApp` (or a widget test's `MaterialApp`) needs
/// for the app's strings AND Material/Cupertino's own (back-button tooltip,
/// text-selection menu, date pickers) to follow the chosen language.
const localizationsDelegates = <LocalizationsDelegate<dynamic>>[
  AppLocalizations.delegate,
  GlobalMaterialLocalizations.delegate,
  GlobalWidgetsLocalizations.delegate,
  GlobalCupertinoLocalizations.delegate,
];
