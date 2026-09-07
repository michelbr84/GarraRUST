import 'package:flutter/material.dart';

import '../../theme/garra_theme.dart';
import '../../theme/garra_tokens.dart';

/// One of the two status cards under the hero ("Runtime: …" / "LLM: …").
///
/// The dot colour is the health signal: green when the runtime answered
/// `/api/health` as healthy, blue for informational (remote LLM connected),
/// amber for degraded and red when unreachable.
///
/// Label and value sit on separate lines: two of these share a 360–412 dp
/// screen, and "Runtime: Local on this phone" does not fit beside an icon on
/// one line without truncating.
class RuntimeStatusCard extends StatelessWidget {
  final IconData icon;
  final Color iconColor;
  final Color dotColor;
  final String label;
  final String value;
  final Color valueColor;
  final String subtitle;
  final VoidCallback? onTap;
  final bool showChevron;

  const RuntimeStatusCard({
    super.key,
    required this.icon,
    required this.iconColor,
    required this.dotColor,
    required this.label,
    required this.value,
    required this.subtitle,
    this.valueColor = GarraColors.text,
    this.onTap,
    this.showChevron = false,
  });

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: onTap != null,
      label: '$label $value. $subtitle',
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(GarraRadius.card),
          child: Ink(
            decoration: BoxDecoration(
              color: GarraColors.panel.withValues(alpha: 0.8),
              borderRadius: BorderRadius.circular(GarraRadius.card),
              border: Border.all(color: GarraColors.panelBorder),
            ),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(10, 12, 8, 12),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.center,
                children: [
                  Icon(icon, color: iconColor, size: 22),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Row(
                          children: [
                            _Dot(color: dotColor),
                            const SizedBox(width: 6),
                            Flexible(
                              child: Text(
                                label,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: garraText(
                                  size: 11,
                                  weight: FontWeight.w600,
                                  color: GarraColors.textMuted,
                                ),
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 2),
                        Text(
                          value,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: garraText(
                            size: 13,
                            weight: FontWeight.w700,
                            color: valueColor,
                            height: 1.2,
                          ),
                        ),
                        const SizedBox(height: 3),
                        Text(
                          subtitle,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: garraText(
                            size: 10.5,
                            color: GarraColors.textMuted,
                            height: 1.25,
                          ),
                        ),
                      ],
                    ),
                  ),
                  if (showChevron && onTap != null)
                    const Icon(
                      Icons.chevron_right_rounded,
                      color: GarraColors.textMuted,
                      size: 18,
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _Dot extends StatelessWidget {
  final Color color;
  const _Dot({required this.color});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 8,
      height: 8,
      decoration: BoxDecoration(
        color: color,
        shape: BoxShape.circle,
        boxShadow: garraGlow(color, blur: 8, alpha: 0.7),
      ),
    );
  }
}
