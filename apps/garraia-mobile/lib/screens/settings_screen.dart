import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../app_version.dart';
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
      title: 'Configurações',
      body: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          _RuntimeCard(config: config, healthValue: health),
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
    final h = healthValue.value;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Runtime',
              style: garraText(size: 16, weight: FontWeight.w600),
            ),
            const SizedBox(height: 14),
            _InfoRow(
              label: 'Mode',
              value: config?.mode.title ?? 'Not configured',
            ),
            const SizedBox(height: 10),
            _InfoRow(label: 'Address', value: config?.baseUrl ?? '—'),
            const SizedBox(height: 10),
            _InfoRow(
              label: 'Status',
              value: healthValue.hasError
                  ? 'unreachable'
                  : h == null
                  ? 'checking…'
                  : '${h.status} · v${h.version}',
            ),
            if (h != null && h.provider != null) ...[
              const SizedBox(height: 10),
              _InfoRow(
                label: 'Provider',
                value: '${h.provider}${h.model != null ? ' / ${h.model}' : ''}',
              ),
            ],
            const SizedBox(height: 14),
            OutlinedButton.icon(
              onPressed: () => context.push('/onboarding'),
              icon: const Icon(Icons.swap_horiz_rounded, size: 18),
              label: const Text('Change runtime'),
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
          return const Center(child: Text('Sessão expirada. Redirecionando…'));
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
                  'Conta',
                  style: garraText(size: 16, weight: FontWeight.w600),
                ),
                const SizedBox(height: 14),
                _InfoRow(label: 'Email', value: me.email),
                const SizedBox(height: 10),
                _InfoRow(label: 'ID', value: _shortId(me.userId)),
                const SizedBox(height: 10),
                _InfoRow(label: 'Cadastro', value: _formatDate(me.createdAt)),
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
            label: const Text(
              'Sair da conta',
              style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
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
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogCtx) => AlertDialog(
        title: const Text('Sair da conta?'),
        content: const Text(
          'Você será desconectado e precisará entrar novamente na próxima vez que abrir o app.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogCtx).pop(false),
            child: const Text('Cancelar'),
          ),
          FilledButton.tonal(
            onPressed: () => Navigator.of(dialogCtx).pop(true),
            child: const Text('Sair'),
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
            'Erro ao carregar informações',
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
