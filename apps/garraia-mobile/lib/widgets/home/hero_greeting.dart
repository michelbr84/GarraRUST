import 'package:flutter/material.dart';

import '../../theme/garra_theme.dart';
import '../../theme/garra_tokens.dart';
import '../brand/night_ridge.dart';

/// "Hello, <name> 👋 — Good to see you again." with the night-ridge art on
/// the right and the "Smaller device. Bigger possibilities." caption.
class HeroGreeting extends StatelessWidget {
  /// Display name collected in onboarding; empty renders a neutral greeting.
  final String name;

  const HeroGreeting({super.key, required this.name});

  @override
  Widget build(BuildContext context) {
    final greeting = name.trim().isEmpty
        ? 'Hello 👋'
        : 'Hello, ${name.trim()} 👋';
    // Min height keeps the art a proper card; IntrinsicHeight lets the row
    // grow with the text instead of overflowing when fonts are larger than
    // the design assumed (accessibility text scale, test fonts).
    return ConstrainedBox(
      constraints: const BoxConstraints(minHeight: 132),
      child: IntrinsicHeight(
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              flex: 12,
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    greeting,
                    style: garraText(
                      size: 24,
                      weight: FontWeight.w800,
                      height: 1.1,
                    ),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    'Good to see you again.\nYour AI assistant is ready.',
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                    style: garraText(
                      size: 13.5,
                      color: GarraColors.textMuted,
                      height: 1.35,
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              flex: 8,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(GarraRadius.card),
                child: Stack(
                  fit: StackFit.expand,
                  children: [
                    const NightRidge(seed: 11, moonSize: 0.14),
                    // Fade the art into the page background on the left edge.
                    DecoratedBox(
                      decoration: BoxDecoration(
                        gradient: LinearGradient(
                          colors: [
                            GarraColors.bg,
                            GarraColors.bg.withValues(alpha: 0),
                          ],
                          stops: const [0, 0.35],
                        ),
                      ),
                    ),
                    Positioned(
                      right: 8,
                      bottom: 8,
                      child: Text(
                        'Smaller device.\nBigger possibilities.',
                        textAlign: TextAlign.right,
                        style: garraText(
                          size: 11.5,
                          weight: FontWeight.w500,
                          height: 1.3,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
