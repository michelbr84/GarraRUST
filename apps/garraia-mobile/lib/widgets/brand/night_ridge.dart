import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../theme/garra_tokens.dart';

/// A night mountain ridge with a moon and a sparse star field — the
/// background art of the hero block and of the footer banner. Drawn with a
/// `CustomPainter` so it needs no image asset and scales with the layout.
///
/// [seed] makes the star field deterministic (stable across rebuilds and
/// golden tests).
class NightRidge extends StatelessWidget {
  final bool showMoon;
  final int seed;
  final double moonSize;

  const NightRidge({
    super.key,
    this.showMoon = true,
    this.seed = 7,
    this.moonSize = 0.16,
  });

  @override
  Widget build(BuildContext context) {
    return RepaintBoundary(
      child: CustomPaint(
        painter: _RidgePainter(
          showMoon: showMoon,
          seed: seed,
          moonSize: moonSize,
        ),
        child: const SizedBox.expand(),
      ),
    );
  }
}

class _RidgePainter extends CustomPainter {
  final bool showMoon;
  final int seed;
  final double moonSize;

  const _RidgePainter({
    required this.showMoon,
    required this.seed,
    required this.moonSize,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;

    // Sky: deep indigo → near black.
    canvas.drawRect(
      rect,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Color(0xFF1B1747), Color(0xFF0B0C1F), GarraColors.bg],
          stops: [0, 0.6, 1],
        ).createShader(rect),
    );

    // Stars.
    final rnd = math.Random(seed);
    final star = Paint()..color = Colors.white;
    for (var i = 0; i < 28; i++) {
      final dx = rnd.nextDouble() * size.width;
      final dy = rnd.nextDouble() * size.height * 0.55;
      final r = 0.4 + rnd.nextDouble() * 0.9;
      star.color = Colors.white.withValues(
        alpha: 0.35 + rnd.nextDouble() * 0.5,
      );
      canvas.drawCircle(Offset(dx, dy), r, star);
    }

    // Moon with halo.
    if (showMoon) {
      final c = Offset(size.width * 0.68, size.height * 0.30);
      final r = size.shortestSide * moonSize;
      canvas.drawCircle(
        c,
        r * 1.9,
        Paint()
          ..color = const Color(0xFFCBD5F5).withValues(alpha: 0.18)
          ..maskFilter = MaskFilter.blur(BlurStyle.normal, r * 0.9),
      );
      canvas.drawCircle(
        c,
        r,
        Paint()
          ..shader = const RadialGradient(
            center: Alignment(-0.3, -0.3),
            colors: [Colors.white, Color(0xFFD7DCF0), Color(0xFFAAB2D6)],
          ).createShader(Rect.fromCircle(center: c, radius: r)),
      );
    }

    // Three ridge layers, back to front.
    _ridge(
      canvas,
      size,
      base: 0.58,
      amp: 0.16,
      color: const Color(0xFF2B2A5C),
      phase: 0.0,
      seedOffset: 1,
    );
    _ridge(
      canvas,
      size,
      base: 0.70,
      amp: 0.14,
      color: const Color(0xFF1A1A3F),
      phase: 1.7,
      seedOffset: 2,
    );
    _ridge(
      canvas,
      size,
      base: 0.82,
      amp: 0.10,
      color: const Color(0xFF0E0F26),
      phase: 3.1,
      seedOffset: 3,
    );
  }

  void _ridge(
    Canvas canvas,
    Size size, {
    required double base,
    required double amp,
    required Color color,
    required double phase,
    required int seedOffset,
  }) {
    final rnd = math.Random(seed + seedOffset);
    final path = Path()..moveTo(0, size.height);
    const steps = 14;
    for (var i = 0; i <= steps; i++) {
      final t = i / steps;
      final x = t * size.width;
      // Two sines plus jitter give a jagged, mountain-like profile.
      final y =
          size.height *
          (base -
              amp *
                  (0.55 * math.sin(t * math.pi * 2.2 + phase) +
                          0.45 * math.sin(t * math.pi * 5.1 + phase * 0.7))
                      .abs() -
              amp * 0.25 * rnd.nextDouble());
      path.lineTo(x, y);
    }
    path
      ..lineTo(size.width, size.height)
      ..close();

    canvas.drawPath(
      path,
      Paint()
        ..shader = LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [
            color,
            Color.alphaBlend(GarraColors.bg.withValues(alpha: 0.6), color),
          ],
        ).createShader(Offset.zero & size),
    );
  }

  @override
  bool shouldRepaint(_RidgePainter old) =>
      old.showMoon != showMoon || old.seed != seed || old.moonSize != moonSize;
}
