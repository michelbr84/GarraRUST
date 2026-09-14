import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'generated/app_localizations.dart';

export 'app_language.dart';
export 'generated/app_localizations.dart';

/// `context.l10n.settingsTitle` instead of
/// `AppLocalizations.of(context).settingsTitle` everywhere (#1178).
extension L10nContext on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);
}

/// Every delegate a `MaterialApp` (or a widget test's `MaterialApp`) needs
/// for the app's strings AND Material/Cupertino's own (back-button tooltip,
/// text-selection menu, date pickers) to follow the chosen language.
const localizationsDelegates = <LocalizationsDelegate<dynamic>>[
  AppLocalizations.delegate,
  GlobalMaterialLocalizations.delegate,
  GlobalWidgetsLocalizations.delegate,
  GlobalCupertinoLocalizations.delegate,
];
