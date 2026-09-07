import 'package:flutter/material.dart';

import '../../theme/garra_theme.dart';
import '../../theme/garra_tokens.dart';
import '../brand/wolf_mark.dart';

/// Brand row at the top of the home screen: wolf mark, "Garra Mobile",
/// "Local-first AI Assistant", the "Private • Powerful • Yours" pill and the
/// tagline quote.
class HomeHeader extends StatelessWidget {
  const HomeHeader({super.key});

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const WolfMark(size: 52),
        const SizedBox(width: 12),
        Expanded(
          flex: 6,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(height: 6),
              Text.rich(
                maxLines: 2,
                TextSpan(
                  style: garraText(
                    size: 22,
                    weight: FontWeight.w800,
                    height: 1.1,
                  ),
                  children: [
                    const TextSpan(text: 'Garra '),
                    TextSpan(
                      text: 'Mobile',
                      style: garraText(
                        size: 22,
                        weight: FontWeight.w800,
                        color: GarraColors.violetLight,
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 3),
              Text(
                'Local-first AI Assistant',
                style: garraText(size: 12.5, color: GarraColors.textMuted),
              ),
            ],
          ),
        ),
        const SizedBox(width: 8),
        Flexible(
          flex: 4,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              // Scales down instead of overflowing on narrow screens / large
              // system fonts.
              const FittedBox(fit: BoxFit.scaleDown, child: _ValuesPill()),
              const SizedBox(height: 8),
              Text(
                '“AI that works for you.\nOn your terms.”',
                textAlign: TextAlign.right,
                maxLines: 3,
                overflow: TextOverflow.ellipsis,
                style: garraText(
                  size: 11,
                  color: GarraColors.textMuted,
                  height: 1.3,
                ).copyWith(fontStyle: FontStyle.italic),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _ValuesPill extends StatelessWidget {
  const _ValuesPill();

  @override
  Widget build(BuildContext context) {
    final dot = garraText(size: 10.5, color: GarraColors.textMuted);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
      decoration: BoxDecoration(
        color: GarraColors.panel.withValues(alpha: 0.7),
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: GarraColors.panelBorderStrong),
      ),
      child: Text.rich(
        maxLines: 1,
        TextSpan(
          style: garraText(size: 10.5, weight: FontWeight.w600),
          children: [
            TextSpan(
              text: 'Private',
              style: garraText(
                size: 10.5,
                weight: FontWeight.w600,
                color: GarraColors.cyan,
              ),
            ),
            TextSpan(text: '  •  ', style: dot),
            TextSpan(
              text: 'Powerful',
              style: garraText(
                size: 10.5,
                weight: FontWeight.w600,
                color: GarraColors.violetLight,
              ),
            ),
            TextSpan(text: '  •  ', style: dot),
            TextSpan(
              text: 'Yours',
              style: garraText(
                size: 10.5,
                weight: FontWeight.w600,
                color: GarraColors.cyan,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
