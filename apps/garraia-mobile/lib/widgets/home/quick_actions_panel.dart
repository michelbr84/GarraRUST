import 'package:flutter/material.dart';

import '../../theme/garra_theme.dart';
import '../../theme/garra_tokens.dart';

/// A quick-action chip: outlined, icon + label.
class QuickAction {
  final String label;
  final IconData icon;
  final VoidCallback onTap;

  const QuickAction({
    required this.label,
    required this.icon,
    required this.onTap,
  });
}

/// "Quick Actions — Common tasks, one tap away." panel with three chips.
class QuickActionsPanel extends StatelessWidget {
  final List<QuickAction> actions;

  const QuickActionsPanel({super.key, required this.actions});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.fromLTRB(16, 14, 16, 16),
      decoration: BoxDecoration(
        color: GarraColors.panel.withValues(alpha: 0.6),
        borderRadius: BorderRadius.circular(GarraRadius.tile),
        border: Border.all(color: GarraColors.panelBorder),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.bolt_rounded, color: GarraColors.text, size: 20),
              const SizedBox(width: 8),
              Flexible(
                child: Text(
                  'Quick Actions',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: garraText(size: 16, weight: FontWeight.w700),
                ),
              ),
              const Spacer(),
              Container(
                width: 36,
                height: 2,
                decoration: const BoxDecoration(
                  gradient: LinearGradient(
                    colors: [Colors.transparent, GarraColors.violet],
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Flexible(
                child: Text(
                  'Get more done, locally.',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.right,
                  style: garraText(size: 11, color: GarraColors.textMuted),
                ),
              ),
            ],
          ),
          const SizedBox(height: 2),
          Padding(
            padding: const EdgeInsets.only(left: 28),
            child: Text(
              'Common tasks, one tap away.',
              style: garraText(size: 12, color: GarraColors.textMuted),
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              for (var i = 0; i < actions.length; i++) ...[
                if (i > 0) const SizedBox(width: 10),
                Expanded(child: _ActionChip(action: actions[i])),
              ],
            ],
          ),
        ],
      ),
    );
  }
}

class _ActionChip extends StatelessWidget {
  final QuickAction action;
  const _ActionChip({required this.action});

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: action.label,
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: action.onTap,
          borderRadius: BorderRadius.circular(GarraRadius.input),
          child: Ink(
            height: 48,
            decoration: BoxDecoration(
              color: GarraColors.bgElevated.withValues(alpha: 0.7),
              borderRadius: BorderRadius.circular(GarraRadius.input),
              border: Border.all(color: GarraColors.panelBorderStrong),
            ),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Icon(action.icon, size: 18, color: GarraColors.violetLight),
                const SizedBox(width: 8),
                Flexible(
                  child: Text(
                    action.label,
                    style: garraText(size: 13, weight: FontWeight.w600),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
