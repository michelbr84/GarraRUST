import 'package:flutter/material.dart';

import '../../theme/garra_theme.dart';
import '../../theme/garra_tokens.dart';

/// One of the six feature cards on the home grid (Chat, Memory, Skills,
/// Files, Agents, Automations).
///
/// [available] comes from `GET /api/capabilities`: a feature the connected
/// runtime does not expose renders dimmed with an "Unavailable" pill instead
/// of pretending to work. The tile still navigates, so the destination
/// screen can explain *why* (capability negotiation, ADR 0016).
class FeatureTile extends StatelessWidget {
  final String title;
  final String subtitle;
  final IconData icon;
  final GarraAccent accent;
  final VoidCallback onTap;
  final bool available;

  const FeatureTile({
    super.key,
    required this.title,
    required this.subtitle,
    required this.icon,
    required this.accent,
    required this.onTap,
    this.available = true,
  });

  @override
  Widget build(BuildContext context) {
    final color = accent.color;
    return Semantics(
      button: true,
      label:
          '$title. $subtitle${available ? '' : '. Unavailable on this runtime'}',
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(GarraRadius.tile),
          child: Ink(
            decoration: BoxDecoration(
              gradient: accent.tileGradient,
              borderRadius: BorderRadius.circular(GarraRadius.tile),
              border: Border.all(color: accent.tileBorder),
            ),
            child: Opacity(
              opacity: available ? 1 : 0.55,
              child: Padding(
                padding: const EdgeInsets.fromLTRB(11, 14, 6, 14),
                child: Row(
                  children: [
                    _NeonIcon(icon: icon, color: color),
                    const SizedBox(width: 9),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          FittedBox(
                            fit: BoxFit.scaleDown,
                            alignment: Alignment.centerLeft,
                            child: Text(
                              title,
                              maxLines: 1,
                              style: garraText(
                                size: 16,
                                weight: FontWeight.w700,
                              ),
                            ),
                          ),
                          const SizedBox(height: 4),
                          Flexible(
                            child: Text(
                              subtitle,
                              style: garraText(
                                size: 11.5,
                                color: GarraColors.textMuted,
                                height: 1.3,
                              ),
                              maxLines: 2,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                          if (!available) ...[
                            const SizedBox(height: 6),
                            const _UnavailablePill(),
                          ],
                        ],
                      ),
                    ),
                    const Icon(
                      Icons.chevron_right_rounded,
                      color: GarraColors.textMuted,
                      size: 20,
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _NeonIcon extends StatelessWidget {
  final IconData icon;
  final Color color;

  const _NeonIcon({required this.icon, required this.color});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 40,
      height: 40,
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: color.withValues(alpha: 0.45)),
        boxShadow: garraGlow(color, blur: 16, alpha: 0.45),
      ),
      child: Icon(icon, color: color, size: 24),
    );
  }
}

class _UnavailablePill extends StatelessWidget {
  const _UnavailablePill();

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        color: Colors.white.withValues(alpha: 0.06),
        borderRadius: BorderRadius.circular(GarraRadius.pill),
        border: Border.all(color: GarraColors.panelBorderStrong),
      ),
      child: Text(
        'Unavailable',
        style: garraText(
          size: 10,
          weight: FontWeight.w600,
          color: GarraColors.textMuted,
        ),
      ),
    );
  }
}
