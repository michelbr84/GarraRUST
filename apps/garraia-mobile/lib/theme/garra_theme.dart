import 'package:flutter/material.dart';

import 'garra_tokens.dart';

/// Inter is shipped as a single variable font. Flutter selects the weight
/// axis through [FontVariation], not through [FontWeight] alone, so every
/// text style built here sets both — `fontWeight` for widgets that read it
/// (e.g. `TextStyle.merge`) and `fontVariations` for the actual rendering.
TextStyle garraText({
  double? size,
  FontWeight weight = FontWeight.w400,
  Color color = GarraColors.text,
  double? height,
  double? letterSpacing,
  bool mono = false,
}) {
  final w = _weightValue(weight);
  return TextStyle(
    fontFamily: mono ? 'JetBrainsMono' : 'Inter',
    fontFamilyFallback: const ['Roboto', 'sans-serif'],
    fontSize: size,
    fontWeight: weight,
    fontVariations: mono ? null : [FontVariation('wght', w)],
    color: color,
    height: height,
    letterSpacing: letterSpacing,
  );
}

double _weightValue(FontWeight w) => switch (w) {
  FontWeight.w100 => 100,
  FontWeight.w200 => 200,
  FontWeight.w300 => 300,
  FontWeight.w400 => 400,
  FontWeight.w500 => 500,
  FontWeight.w600 => 600,
  FontWeight.w700 => 700,
  FontWeight.w800 => 800,
  FontWeight.w900 => 900,
  _ => 400,
};

/// Explicit dark colour scheme. The previous theme derived everything from a
/// single seed through `ColorScheme.fromSeed`; the neon design needs exact
/// surfaces and accents, so every slot is set on purpose.
const ColorScheme garraColorScheme = ColorScheme(
  brightness: Brightness.dark,
  primary: GarraColors.violet,
  onPrimary: Colors.white,
  primaryContainer: Color(0xFF2A1B4D),
  onPrimaryContainer: GarraColors.violetLight,
  secondary: GarraColors.cyan,
  onSecondary: Color(0xFF00202B),
  secondaryContainer: Color(0xFF0E2A3A),
  onSecondaryContainer: GarraColors.cyan,
  tertiary: GarraColors.gold,
  onTertiary: Color(0xFF2B2300),
  tertiaryContainer: Color(0xFF2C2610),
  onTertiaryContainer: GarraColors.gold,
  error: GarraColors.danger,
  onError: Colors.white,
  errorContainer: Color(0xFF3B1414),
  onErrorContainer: Color(0xFFFECACA),
  surface: GarraColors.bg,
  onSurface: GarraColors.text,
  surfaceContainerLowest: GarraColors.bg,
  surfaceContainerLow: GarraColors.bgElevated,
  surfaceContainer: GarraColors.panel,
  surfaceContainerHigh: Color(0xFF181A33),
  surfaceContainerHighest: Color(0xFF1F2140),
  onSurfaceVariant: GarraColors.textMuted,
  outline: GarraColors.panelBorder,
  outlineVariant: GarraColors.panelBorderStrong,
  shadow: Colors.black,
  scrim: Colors.black,
  inverseSurface: Colors.white,
  onInverseSurface: GarraColors.bg,
  inversePrimary: GarraColors.violetDeep,
  surfaceTint: GarraColors.violet,
);

ThemeData garraTheme() {
  final base = ThemeData(
    useMaterial3: true,
    colorScheme: garraColorScheme,
    scaffoldBackgroundColor: GarraColors.bg,
    fontFamily: 'Inter',
  );

  final textTheme = TextTheme(
    displayLarge: garraText(size: 40, weight: FontWeight.w800, height: 1.1),
    displayMedium: garraText(size: 34, weight: FontWeight.w800, height: 1.1),
    headlineLarge: garraText(size: 30, weight: FontWeight.w800, height: 1.15),
    headlineMedium: garraText(size: 26, weight: FontWeight.w700, height: 1.2),
    headlineSmall: garraText(size: 22, weight: FontWeight.w700, height: 1.2),
    titleLarge: garraText(size: 20, weight: FontWeight.w700),
    titleMedium: garraText(size: 16, weight: FontWeight.w600),
    titleSmall: garraText(size: 14, weight: FontWeight.w600),
    bodyLarge: garraText(size: 16, height: 1.45),
    bodyMedium: garraText(size: 14, height: 1.45),
    bodySmall: garraText(size: 12, height: 1.4, color: GarraColors.textMuted),
    labelLarge: garraText(size: 14, weight: FontWeight.w600),
    labelMedium: garraText(size: 12, weight: FontWeight.w600),
    labelSmall: garraText(
      size: 11,
      weight: FontWeight.w600,
      letterSpacing: 0.6,
      color: GarraColors.textMuted,
    ),
  );

  return base.copyWith(
    textTheme: textTheme,
    primaryTextTheme: textTheme,
    appBarTheme: AppBarTheme(
      backgroundColor: Colors.transparent,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      centerTitle: true,
      foregroundColor: GarraColors.text,
      titleTextStyle: garraText(size: 18, weight: FontWeight.w700),
    ),
    cardTheme: CardThemeData(
      color: GarraColors.panel,
      elevation: 0,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(GarraRadius.card),
        side: const BorderSide(color: GarraColors.panelBorder),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: Colors.white.withValues(alpha: 0.06),
      hintStyle: garraText(size: 14, color: GarraColors.textDim),
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(GarraRadius.input),
        borderSide: const BorderSide(color: GarraColors.panelBorder),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(GarraRadius.input),
        borderSide: const BorderSide(color: GarraColors.panelBorder),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(GarraRadius.input),
        borderSide: const BorderSide(color: GarraColors.cyan, width: 1.4),
      ),
      contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
    ),
    elevatedButtonTheme: ElevatedButtonThemeData(
      style: ElevatedButton.styleFrom(
        minimumSize: const Size.fromHeight(50),
        backgroundColor: GarraColors.violet,
        foregroundColor: Colors.white,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(GarraRadius.input),
        ),
        textStyle: garraText(size: 16, weight: FontWeight.w600),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        minimumSize: const Size.fromHeight(50),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(GarraRadius.input),
        ),
        textStyle: garraText(size: 16, weight: FontWeight.w600),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        foregroundColor: GarraColors.text,
        side: const BorderSide(color: GarraColors.panelBorderStrong),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(GarraRadius.input),
        ),
        textStyle: garraText(size: 14, weight: FontWeight.w600),
      ),
    ),
    chipTheme: ChipThemeData(
      backgroundColor: GarraColors.panel,
      side: const BorderSide(color: GarraColors.panelBorder),
      labelStyle: garraText(size: 13, weight: FontWeight.w500),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(GarraRadius.input),
      ),
    ),
    dividerTheme: const DividerThemeData(color: GarraColors.panelBorder),
    snackBarTheme: SnackBarThemeData(
      backgroundColor: GarraColors.panel,
      contentTextStyle: garraText(size: 14),
      behavior: SnackBarBehavior.floating,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(GarraRadius.input),
        side: const BorderSide(color: GarraColors.panelBorder),
      ),
    ),
    bottomNavigationBarTheme: const BottomNavigationBarThemeData(
      backgroundColor: GarraColors.bgElevated,
      selectedItemColor: GarraColors.violetLight,
      unselectedItemColor: GarraColors.textMuted,
    ),
    listTileTheme: const ListTileThemeData(
      iconColor: GarraColors.textMuted,
      textColor: GarraColors.text,
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: GarraColors.panel,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(GarraRadius.tile),
        side: const BorderSide(color: GarraColors.panelBorder),
      ),
    ),
    progressIndicatorTheme: const ProgressIndicatorThemeData(
      color: GarraColors.cyan,
    ),
  );
}
