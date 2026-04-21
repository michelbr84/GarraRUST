import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../providers/auth_provider.dart';
import '../services/api_service.dart';

/// Plan 0029 (GAR-358): dedicated Settings screen.
///
/// Shows the authenticated user's account info (email, user_id, created_at)
/// via the `/me` endpoint that already powers `AuthState`, and exposes a
/// prominent Logout button that clears the JWT from secure storage and
/// routes back to `/login` via the existing app router redirect logic.
///
/// The quick-access logout popup in `chat_screen.dart` AppBar stays — this
/// is an additional entry point, not a replacement. `app_router.dart`
/// exposes this screen at `/settings`.
class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final authState = ref.watch(authStateProvider);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Configurações'),
        centerTitle: true,
      ),
      body: authState.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (err, _) => _ErrorState(error: err.toString()),
        data: (me) {
          if (me == null) {
            // Rare race: AuthState resolved to null while on this screen.
            // The router redirect will kick us to /login on next frame;
            // show a graceful empty state until then.
            return const Center(
              child: Text('Sessão expirada. Redirecionando…'),
            );
          }
          return _SettingsBody(me: me);
        },
      ),
    );
  }
}

class _SettingsBody extends ConsumerWidget {
  final MeResult me;
  const _SettingsBody({required this.me});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = Theme.of(context);

    return SingleChildScrollView(
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // ── Account card ─────────────────────────────────────────────
          Card(
            elevation: 0,
            color: theme.colorScheme.surfaceContainerHigh,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(14),
            ),
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Conta',
                    style: theme.textTheme.titleMedium?.copyWith(
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 14),
                  _InfoRow(label: 'Email', value: me.email),
                  const SizedBox(height: 10),
                  _InfoRow(
                    label: 'ID',
                    // Show only the first segment of the UUID for readability;
                    // full value is copyable via long-press on row below.
                    value: _shortId(me.userId),
                  ),
                  const SizedBox(height: 10),
                  _InfoRow(label: 'Cadastro', value: _formatDate(me.createdAt)),
                ],
              ),
            ),
          ),

          const SizedBox(height: 24),

          // ── Logout button ────────────────────────────────────────────
          SizedBox(
            height: 52,
            child: FilledButton.tonalIcon(
              style: FilledButton.styleFrom(
                backgroundColor: theme.colorScheme.errorContainer,
                foregroundColor: theme.colorScheme.onErrorContainer,
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(14),
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

          const SizedBox(height: 40),

          // ── Version footer ───────────────────────────────────────────
          Center(
            child: Text(
              'Garra • v0.1.0 (Alpha)',
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurface.withValues(alpha: 0.6),
              ),
            ),
          ),
        ],
      ),
    );
  }

  static String _shortId(String uuid) {
    // `8f2c7e1a-1234-4abc-9def-0123456789ab` → `8f2c7e1a…89ab`
    if (uuid.length < 13) return uuid;
    return '${uuid.substring(0, 8)}…${uuid.substring(uuid.length - 4)}';
  }

  static String _formatDate(String iso) {
    // API returns ISO-8601 UTC (e.g. `2026-03-10T12:34:56Z`). Show just
    // the date segment to avoid timezone confusion on the client.
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
    // Router redirect (`redirect` in `app_router.dart`) will bounce us to
    // `/login` because authStateProvider now resolves to null. `context.go`
    // is still safe to call as a belt-and-suspenders fallback — it just
    // ends up in the same spot the redirect would have taken us.
    if (context.mounted) context.go('/login');
  }
}

class _InfoRow extends StatelessWidget {
  final String label;
  final String value;
  const _InfoRow({required this.label, required this.value});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 80,
          child: Text(
            label,
            style: theme.textTheme.bodySmall?.copyWith(
              color: theme.colorScheme.onSurface.withValues(alpha: 0.6),
            ),
          ),
        ),
        Expanded(
          child: Text(
            value,
            style: theme.textTheme.bodyMedium,
          ),
        ),
      ],
    );
  }
}

class _ErrorState extends StatelessWidget {
  final String error;
  const _ErrorState({required this.error});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.error_outline_rounded,
              size: 48,
              color: theme.colorScheme.error,
            ),
            const SizedBox(height: 16),
            Text(
              'Erro ao carregar informações',
              style: theme.textTheme.titleMedium,
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 8),
            Text(
              error,
              style: theme.textTheme.bodySmall?.copyWith(
                color: theme.colorScheme.onSurface.withValues(alpha: 0.6),
              ),
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}
