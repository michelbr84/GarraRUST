import 'package:flutter/material.dart';

import '../../theme/garra_theme.dart';
import '../../theme/garra_tokens.dart';
import '../brand/night_ridge.dart';

/// Footer banner: "LOCAL AI. A BRIGHTER YOU." over a mountain silhouette
/// with a violet glow line at the bottom.
class BrandBanner extends StatelessWidget {
  const BrandBanner({super.key});

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(GarraRadius.tile),
      child: SizedBox(
        height: 118,
        child: Stack(
          fit: StackFit.expand,
          children: [
            const NightRidge(showMoon: false, seed: 23),
            DecoratedBox(
              decoration: BoxDecoration(
                border: Border.all(color: GarraColors.panelBorder),
                borderRadius: BorderRadius.circular(GarraRadius.tile),
              ),
            ),
            Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Text(
                  'LOCAL AI. A BRIGHTER YOU.',
                  textAlign: TextAlign.center,
                  style: garraText(
                    size: 13,
                    weight: FontWeight.w700,
                    letterSpacing: 3.2,
                  ),
                ),
                const SizedBox(height: 8),
                Text(
                  'More control. A more capable you.',
                  textAlign: TextAlign.center,
                  style: garraText(size: 12.5, color: GarraColors.textMuted),
                ),
              ],
            ),
            Positioned(
              left: 0,
              right: 0,
              bottom: 10,
              child: Center(
                child: Container(
                  width: 120,
                  height: 3,
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(GarraRadius.pill),
                    gradient: const LinearGradient(
                      colors: [
                        Colors.transparent,
                        GarraColors.violetLight,
                        Colors.transparent,
                      ],
                    ),
                    boxShadow: garraGlow(
                      GarraColors.violet,
                      blur: 12,
                      alpha: 0.8,
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
