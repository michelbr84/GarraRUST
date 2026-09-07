import 'package:flutter/material.dart';

/// Garra Neon design tokens — the mobile extension of the Garra Glass palette
/// (ADR 0009). Gold (`#FFD400`) and cyan (`#16D9FF`) are *the same* values the
/// web console uses; violet and magenta are the mobile additions that carry
/// the "local-first, on-device" identity of the reference design.
///
/// Every colour in the app comes from here or from `ColorScheme`. Do not
/// hard-code hex values in widgets.
abstract final class GarraColors {
  // ── Surfaces ────────────────────────────────────────────────────────────
  static const Color bg = Color(0xFF05060F);
  static const Color bgElevated = Color(0xFF0B0C1A);
  static const Color panel = Color(0xFF12132A);
  static const Color panelBorder = Color(0xFF232447);
  static const Color panelBorderStrong = Color(0xFF34366A);

  // ── Brand ───────────────────────────────────────────────────────────────
  static const Color violet = Color(0xFF8B5CF6);
  static const Color violetLight = Color(0xFFA78BFA);
  static const Color violetDeep = Color(0xFF5B4CF5);
  static const Color cyan = Color(0xFF16D9FF);
  static const Color gold = Color(0xFFFFD400);
  static const Color magenta = Color(0xFFFF3FA4);
  static const Color rose = Color(0xFFFB7185);
  static const Color blue = Color(0xFF3B82F6);

  // ── Text ────────────────────────────────────────────────────────────────
  static const Color text = Color(0xFFFFFFFF);
  static const Color textMuted = Color(0xFF9CA3C0);
  static const Color textDim = Color(0xFF6B7194);

  // ── Semantic ────────────────────────────────────────────────────────────
  static const Color ok = Color(0xFF22C55E);
  static const Color info = Color(0xFF3B82F6);
  static const Color warn = Color(0xFFF59E0B);
  static const Color danger = Color(0xFFEF4444);
}

/// Corner radii used across cards, tiles and inputs.
abstract final class GarraRadius {
  static const double input = 14;
  static const double card = 16;
  static const double tile = 20;
  static const double pill = 999;
}

/// Spacing scale (dp).
abstract final class GarraSpace {
  static const double xs = 4;
  static const double sm = 8;
  static const double md = 12;
  static const double lg = 16;
  static const double xl = 20;
  static const double xxl = 28;
}

/// The colour "family" a feature tile belongs to. Each family renders a
/// tinted glass card with a matching neon icon and glow.
enum GarraAccent { violet, magenta, gold, cyan, indigo, rose }

extension GarraAccentColors on GarraAccent {
  Color get color => switch (this) {
    GarraAccent.violet => GarraColors.violet,
    GarraAccent.magenta => GarraColors.magenta,
    GarraAccent.gold => GarraColors.gold,
    GarraAccent.cyan => GarraColors.cyan,
    GarraAccent.indigo => GarraColors.violetDeep,
    GarraAccent.rose => GarraColors.rose,
  };

  /// Card fill: a deep tint of the accent over the panel colour.
  LinearGradient get tileGradient => LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [
      Color.alphaBlend(color.withValues(alpha: 0.22), GarraColors.panel),
      Color.alphaBlend(color.withValues(alpha: 0.08), GarraColors.bgElevated),
    ],
  );

  Color get tileBorder => color.withValues(alpha: 0.35);
}

/// Neon glow — used behind icons and the brand mark.
List<BoxShadow> garraGlow(
  Color color, {
  double blur = 18,
  double alpha = 0.55,
}) => [
  BoxShadow(
    color: color.withValues(alpha: alpha),
    blurRadius: blur,
    spreadRadius: 1,
  ),
];
