import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';

/// The four top-level destinations of Garra Mobile.
enum GarraTab { home, activity, notifications, profile }

extension GarraTabRoute on GarraTab {
  String get route => switch (this) {
    GarraTab.home => '/home',
    GarraTab.activity => '/activity',
    GarraTab.notifications => '/notifications',
    GarraTab.profile => '/profile',
  };

  String get label => switch (this) {
    GarraTab.home => 'Home',
    GarraTab.activity => 'Activity',
    GarraTab.notifications => 'Notifications',
    GarraTab.profile => 'Profile',
  };

  IconData get icon => switch (this) {
    GarraTab.home => Icons.home_rounded,
    GarraTab.activity => Icons.show_chart_rounded,
    GarraTab.notifications => Icons.notifications_none_rounded,
    GarraTab.profile => Icons.person_outline_rounded,
  };

  IconData get activeIcon => switch (this) {
    GarraTab.home => Icons.home_rounded,
    GarraTab.activity => Icons.show_chart_rounded,
    GarraTab.notifications => Icons.notifications_rounded,
    GarraTab.profile => Icons.person_rounded,
  };
}

/// Bottom navigation bar of the reference design: hairline top border,
/// active item in violet with a soft glow, inactive items muted.
class GarraBottomNav extends StatelessWidget {
  final GarraTab current;

  const GarraBottomNav({super.key, required this.current});

  @override
  Widget build(BuildContext context) {
    return Container(
      decoration: const BoxDecoration(
        color: GarraColors.bgElevated,
        border: Border(top: BorderSide(color: GarraColors.panelBorder)),
      ),
      child: SafeArea(
        top: false,
        child: SizedBox(
          height: 64,
          child: Row(
            children: [
              for (final tab in GarraTab.values)
                Expanded(
                  child: _NavItem(
                    tab: tab,
                    active: tab == current,
                    onTap: () {
                      if (tab != current) context.go(tab.route);
                    },
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _NavItem extends StatelessWidget {
  final GarraTab tab;
  final bool active;
  final VoidCallback onTap;

  const _NavItem({
    required this.tab,
    required this.active,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final color = active ? GarraColors.violetLight : GarraColors.textMuted;
    return Semantics(
      button: true,
      selected: active,
      label: tab.label,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(GarraRadius.card),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Container(
              decoration: active
                  ? BoxDecoration(
                      shape: BoxShape.circle,
                      boxShadow: garraGlow(
                        GarraColors.violet,
                        blur: 14,
                        alpha: 0.5,
                      ),
                    )
                  : null,
              child: Icon(
                active ? tab.activeIcon : tab.icon,
                color: color,
                size: 24,
              ),
            ),
            const SizedBox(height: 4),
            Text(
              tab.label,
              style: garraText(
                size: 11,
                weight: active ? FontWeight.w600 : FontWeight.w500,
                color: color,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
