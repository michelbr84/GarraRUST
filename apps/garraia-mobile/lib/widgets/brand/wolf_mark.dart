import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../theme/garra_tokens.dart';

/// The Garra Mobile brand mark — a geometric wolf head drawn as a neon
/// gradient stroke with a soft glow. Pure vector (`CustomPainter`), so it
/// scales to any density, has no binary asset in the repo, and doubles as
/// the source for the adaptive launcher icon.
///
/// The parrot in `assets/logo.png` remains the CLI / desktop / site brand.
class WolfMark extends StatelessWidget {
  final double size;

  /// Stroke colours, head-to-tail along the gradient.
  final List<Color> colors;

  /// Glow strength (0 disables the blur layer — cheaper for tiny sizes).
  final double glow;

  const WolfMark({
    super.key,
    this.size = 64,
    this.colors = const [
      GarraColors.violetLight,
      GarraColors.violet,
      GarraColors.magenta,
    ],
    this.glow = 1,
  });

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      child: CustomPaint(
        size: Size.square(size),
        painter: _WolfPainter(colors: colors, glow: glow),
      ),
    );
  }
}

class _WolfPainter extends CustomPainter {
  final List<Color> colors;
  final double glow;

  const _WolfPainter({required this.colors, required this.glow});

  /// Head outline in a 100×100 design box (left-facing profile).
  static Path _head(Size s) {
    final k = s.width / 100;
    Offset p(double x, double y) => Offset(x * k, y * k);

    final path = Path()
      ..moveTo(p(22, 14).dx, p(22, 14).dy) // left ear tip
      ..lineTo(p(38, 40).dx, p(38, 40).dy) // ear base (inner)
      ..lineTo(p(54, 34).dx, p(54, 34).dy) // brow
      ..lineTo(p(62, 10).dx, p(62, 10).dy) // right ear tip
      ..lineTo(p(78, 40).dx, p(78, 40).dy) // right ear base
      ..lineTo(p(88, 52).dx, p(88, 52).dy) // crown → cheek
      ..lineTo(p(76, 78).dx, p(76, 78).dy) // jaw
      ..lineTo(p(56, 92).dx, p(56, 92).dy) // chin
      ..lineTo(p(34, 84).dx, p(34, 84).dy) // muzzle underside
      ..lineTo(p(8, 70).dx, p(8, 70).dy) // snout tip
      ..lineTo(p(22, 58).dx, p(22, 58).dy) // snout top
      ..lineTo(p(26, 42).dx, p(26, 42).dy) // forehead
      ..close();
    return path;
  }

  /// Inner facets: eye, brow line, muzzle line — kept as open strokes.
  static Path _facets(Size s) {
    final k = s.width / 100;
    Offset p(double x, double y) => Offset(x * k, y * k);
    return Path()
      // brow ridge
      ..moveTo(p(40, 50).dx, p(40, 50).dy)
      ..lineTo(p(58, 46).dx, p(58, 46).dy)
      ..lineTo(p(70, 54).dx, p(70, 54).dy)
      // muzzle crease
      ..moveTo(p(24, 62).dx, p(24, 62).dy)
      ..lineTo(p(44, 70).dx, p(44, 70).dy)
      ..lineTo(p(60, 80).dx, p(60, 80).dy);
  }

  @override
  void paint(Canvas canvas, Size size) {
    final head = _head(size);
    final facets = _facets(size);
    final rect = Offset.zero & size;
    final shader = LinearGradient(
      begin: Alignment.topLeft,
      end: Alignment.bottomRight,
      colors: colors,
    ).createShader(rect);
    final stroke = math.max(1.6, size.width * 0.055);

    if (glow > 0) {
      final glowPaint = Paint()
        ..shader = shader
        ..style = PaintingStyle.stroke
        ..strokeWidth = stroke * 1.6
        ..strokeJoin = StrokeJoin.round
        ..maskFilter = MaskFilter.blur(
          BlurStyle.normal,
          size.width * 0.09 * glow,
        );
      canvas.drawPath(head, glowPaint);
    }

    // Translucent fill so the mark reads as a solid silhouette on dark bg.
    canvas.drawPath(
      head,
      Paint()
        ..shader = LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [
            colors.first.withValues(alpha: 0.28),
            colors.last.withValues(alpha: 0.10),
          ],
        ).createShader(rect)
        ..style = PaintingStyle.fill,
    );

    final outline = Paint()
      ..shader = shader
      ..style = PaintingStyle.stroke
      ..strokeWidth = stroke
      ..strokeJoin = StrokeJoin.round
      ..strokeCap = StrokeCap.round;
    canvas.drawPath(head, outline);
    canvas.drawPath(facets, outline..strokeWidth = stroke * 0.7);

    // Eye — a bright cyan point with its own glow.
    final k = size.width / 100;
    final eye = Offset(50 * k, 58 * k);
    canvas.drawCircle(
      eye,
      size.width * 0.05,
      Paint()
        ..color = GarraColors.cyan.withValues(alpha: 0.6)
        ..maskFilter = MaskFilter.blur(BlurStyle.normal, size.width * 0.05),
    );
    canvas.drawCircle(
      eye,
      size.width * 0.032,
      Paint()..color = GarraColors.cyan,
    );
  }

  @override
  bool shouldRepaint(_WolfPainter old) =>
      old.colors != colors || old.glow != glow;
}
