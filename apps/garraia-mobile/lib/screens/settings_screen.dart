import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../app_version.dart';
import '../l10n/l10n.dart';
import '../providers/auth_provider.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';
import '../services/api_service.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/garra_page.dart';

/// Settings — runtime card (always), account card + logout (cloud mode
/// only, plan 0029 / GAR-358), app version.
class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final config = ref.watch(runtimeConfigStateProvider).value;
    final health = ref.watch(runtimeHealthProvider);
    final isCloud = config?.mode == RuntimeMode.cloud;

    return GarraPage(
      title: context.l10n.settingsTitle,
      body: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          _RuntimeCard(config: config, healthValue: health),
          const SizedBox(height: 16),
          const _LanguageCard(),
          const SizedBox(height: 16),
          if (isCloud) const _AccountSection(),
          const SizedBox(height: 40),
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
}

class _RuntimeCard extends StatelessWidget {
  final RuntimeConfig? config;
  final AsyncValue healthValue;
  const _RuntimeCard({required this.config, required this.healthValue});

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final h = healthValue.value;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              l10n.settingsRuntimeTitle,
              style: garraText(size: 16, weight: FontWeight.w600),
            ),
            const SizedBox(height: 14),
            _InfoRow(
              label: l10n.settingsRuntimeModeLabel,
              value: config?.mode.title(l10n) ?? l10n.commonNotConfigured,
            ),
            const SizedBox(height: 10),
            _InfoRow(
              label: l10n.settingsRuntimeAddressLabel,
              value: config?.baseUrl ?? '—',
            ),
            const SizedBox(height: 10),
            _InfoRow(
              label: l10n.settingsRuntimeStatusLabel,
              value: healthValue.hasError
                  ? l10n.settingsRuntimeUnreachable
                  : h == null
                  ? l10n.settingsRuntimeChecking
                  : l10n.settingsRuntimeStatusVersion(h.status, h.version),
            ),
            if (h != null && h.provider != null) ...[
              const SizedBox(height: 10),
              _InfoRow(
                label: l10n.settingsRuntimeProviderLabel,
                value: h.model != null
                    ? l10n.settingsRuntimeProviderModel(h.provider, h.model)
                    : h.provider,
              ),
            ],
            const SizedBox(height: 14),
            OutlinedButton.icon(
              onPressed: () => context.push('/onboarding'),
              icon: const Icon(Icons.swap_horiz_rounded, size: 18),
              label: Text(l10n.settingsChangeRuntime),
            ),
          ],
        ),
      ),
    );
  }
}

/// Settings > Language (#1178): System default / English / Português (Brasil),
/// persisted by [AppLanguageState] and applied on the spot — the
/// `MaterialApp` watches the provider, so every screen rebuilds in the new
/// language without a restart.
class _LanguageCard extends ConsumerWidget {
  const _LanguageCard();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final current = ref.watch(appLanguageStateProvider);
    final labels = {
      AppLanguage.system: l10n.settingsLanguageSystem,
      AppLanguage.en: l10n.settingsLanguageEnglish,
      AppLanguage.ptBR: l10n.settingsLanguagePortuguese,
    };
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              l10n.settingsLanguageTitle,
              style: garraText(size: 16, weight: FontWeight.w600),
            ),
            const SizedBox(height: 4),
            Text(
              l10n.settingsLanguageHint,
              style: garraText(size: 12, color: GarraColors.textDim),
            ),
            const SizedBox(height: 8),
            RadioGroup<AppLanguage>(
              groupValue: current,
              onChanged: (value) {
                if (value != null) {
                  ref.read(appLanguageStateProvider.notifier).set(value);
                }
              },
              child: Column(
                children: [
                  for (final language in AppLanguage.values)
                    RadioListTile<AppLanguage>(
                      key: ValueKey('language-${language.storageValue}'),
                      value: language,
                      contentPadding: EdgeInsets.zero,
                      dense: true,
                      title: Text(
                        labels[language]!,
                        style: garraText(size: 14),
                      ),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _AccountSection extends ConsumerWidget {
  const _AccountSection();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final authState = ref.watch(authStateProvider);
    return authState.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (err, _) => _ErrorState(error: err.toString()),
      data: (me) {
        if (me == null) {
          // Rare race: AuthState resolved to null while on this screen.
          // The router redirect will kick us to /login on next frame;
          // show a graceful empty state until then.
          return Center(
            child: Text(context.l10n.settingsSessionExpiredRedirecting),
          );
        }
        return _AccountBody(me: me);
      },
    );
  }
}

class _AccountBody extends ConsumerWidget {
  final MeResult me;
  const _AccountBody({required this.me});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  l10n.settingsAccountTitle,
                  style: garraText(size: 16, weight: FontWeight.w600),
                ),
                const SizedBox(height: 14),
                _InfoRow(label: l10n.commonEmailLabel, value: me.email),
                const SizedBox(height: 10),
                _InfoRow(
                  label: l10n.settingsAccountIdLabel,
                  value: _shortId(me.userId),
                ),
                const SizedBox(height: 10),
                _InfoRow(
                  label: l10n.settingsAccountCreatedLabel,
                  value: _formatDate(me.createdAt),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 24),
        SizedBox(
          height: 52,
          child: FilledButton.tonalIcon(
            style: FilledButton.styleFrom(
              backgroundColor: garraColorScheme.errorContainer,
              foregroundColor: garraColorScheme.onErrorContainer,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(GarraRadius.input),
              ),
            ),
            icon: const Icon(Icons.logout_rounded),
            label: Text(
              l10n.settingsLogout,
              style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
            ),
            onPressed: () => _confirmLogout(context, ref),
          ),
        ),
      ],
    );
  }

  static String _shortId(String uuid) {
    // `8f2c7e1a-1234-4abc-9def-0123456789ab` → `8f2c7e1a…89ab`; inputs
    // shorter than 13 chars pass through untouched.
    if (uuid.length < 13) return uuid;
    return '${uuid.substring(0, 8)}…${uuid.substring(uuid.length - 4)}';
  }

  static String _formatDate(String iso) {
    final t = iso.indexOf('T');
    if (t == -1) return iso;
    return iso.substring(0, t);
  }

  Future<void> _confirmLogout(BuildContext context, WidgetRef ref) async {
    final l10n = context.l10n;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogCtx) => AlertDialog(
        title: Text(l10n.settingsLogoutConfirmTitle),
        content: Text(l10n.settingsLogoutConfirmBody),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogCtx).pop(false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton.tonal(
            onPressed: () => Navigator.of(dialogCtx).pop(true),
            child: Text(l10n.commonLogout),
          ),
        ],
      ),
    );
    if (confirmed != true) return;

    await ref.read(authStateProvider.notifier).logout();
    if (context.mounted) context.go('/login');
  }
}

class _InfoRow extends StatelessWidget {
  final String label;
  final String value;
  const _InfoRow({required this.label, required this.value});

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 80,
          child: Text(
            label,
            style: garraText(size: 12, color: GarraColors.textMuted),
          ),
        ),
        Expanded(child: Text(value, style: garraText(size: 14))),
      ],
    );
  }
}

class _ErrorState extends StatelessWidget {
  final String error;
  const _ErrorState({required this.error});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(
            Icons.error_outline_rounded,
            size: 48,
            color: GarraColors.danger,
          ),
          const SizedBox(height: 16),
          Text(
            context.l10n.settingsAccountLoadError,
            style: garraText(size: 16, weight: FontWeight.w600),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 8),
          Text(
            error,
            style: garraText(size: 12, color: GarraColors.textMuted),
            textAlign: TextAlign.center,
          ),
        ],
      ),
    );
  }
}
