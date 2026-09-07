import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../app_version.dart';
import '../providers/auth_provider.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/brand/wolf_mark.dart';
import '../widgets/garra_bottom_nav.dart';
import '../widgets/garra_page.dart';

/// Profile tab — who you are to Garra, where it runs, and the exits.
class ProfileScreen extends ConsumerWidget {
  const ProfileScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final config = ref.watch(runtimeConfigStateProvider).value;
    final health = ref.watch(runtimeHealthProvider).value;
    final name = config?.ownerName ?? '';

    return GarraPage(
      title: 'Profile',
      bottomNavigationBar: const GarraBottomNav(current: GarraTab.profile),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
        children: [
          Center(
            child: Column(
              children: [
                const WolfMark(size: 88),
                const SizedBox(height: 10),
                Text(
                  name.isEmpty ? 'Garra user' : name,
                  style: garraText(size: 22, weight: FontWeight.w800),
                ),
                const SizedBox(height: 4),
                Text(
                  config?.mode.title ?? 'No runtime',
                  style: garraText(size: 13, color: GarraColors.textMuted),
                ),
              ],
            ),
          ),
          const SizedBox(height: 20),
          _Row(
            icon: Icons.badge_outlined,
            title: 'Display name',
            subtitle: name.isEmpty ? 'Not set' : name,
            onTap: () => _editName(context, ref, name),
          ),
          _Row(
            icon: Icons.memory_rounded,
            title: 'Runtime',
            subtitle: config == null
                ? 'Not configured'
                : '${config.mode.title} · ${config.baseUrl}${health != null ? ' · v${health.version}' : ''}',
            onTap: () => context.push('/onboarding'),
          ),
          _Row(
            icon: Icons.devices_rounded,
            title: 'Paired devices',
            subtitle: 'Sync sessions across your devices',
            onTap: () => context.push('/pair'),
          ),
          _Row(
            icon: Icons.settings_outlined,
            title: 'Settings',
            subtitle: 'Runtime, account and app details',
            onTap: () => context.push('/settings'),
          ),
          if (config?.mode == RuntimeMode.cloud)
            _Row(
              icon: Icons.logout_rounded,
              title: 'Sign out',
              subtitle: 'Clear the cloud session on this device',
              color: GarraColors.danger,
              onTap: () async {
                await ref.read(authStateProvider.notifier).logout();
                if (context.mounted) context.go('/login');
              },
            ),
          const SizedBox(height: 24),
          Center(
            child: Text(
              kAppVersionLabel,
              style: garraText(size: 12, color: GarraColors.textDim),
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _editName(
    BuildContext context,
    WidgetRef ref,
    String current,
  ) async {
    final ctrl = TextEditingController(text: current);
    final value = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Display name'),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          textCapitalization: TextCapitalization.words,
          decoration: const InputDecoration(hintText: 'Your name'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(ctx).pop(ctrl.text),
            child: const Text('Save'),
          ),
        ],
      ),
    );
    if (value == null) return;
    await ref.read(runtimeConfigStateProvider.notifier).updateOwnerName(value);
  }
}

class _Row extends StatelessWidget {
  final IconData icon;
  final String title;
  final String subtitle;
  final Color color;
  final VoidCallback onTap;

  const _Row({
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.onTap,
    this.color = GarraColors.violetLight,
  });

  @override
  Widget build(BuildContext context) {
    return GarraListTile(
      icon: icon,
      iconColor: color,
      title: title,
      subtitle: subtitle,
      trailing: const Icon(
        Icons.chevron_right_rounded,
        color: GarraColors.textMuted,
      ),
      onTap: onTap,
    );
  }
}
