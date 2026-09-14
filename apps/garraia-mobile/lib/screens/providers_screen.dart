import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../l10n/l10n.dart';
import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/garra_page.dart';

part 'providers_screen.g.dart';

@riverpod
Future<List<ProviderInfo>> llmProviders(Ref ref) =>
    requireConnection(ref).providers();

/// LLM providers the runtime knows (`GET /api/providers`). Read-only in
/// v0.4.0: keys are entered on the runtime (CLI wizard / web console), which
/// keeps provider secrets off the phone's storage while the runtime is a
/// separate app (Termux UID ≠ app UID — strategy §23).
class ProvidersScreen extends ConsumerWidget {
  const ProvidersScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final value = ref.watch(llmProvidersProvider);
    final l10n = context.l10n;
    return GarraPage(
      title: l10n.providersTitle,
      body: AsyncBody<List<ProviderInfo>>(
        value: value,
        emptyText: l10n.providersEmpty,
        onRetry: () => ref.invalidate(llmProvidersProvider),
        builder: (list) => ListView(
          padding: const EdgeInsets.only(top: 4, bottom: 24),
          children: [
            for (final p in list) _ProviderRow(info: p),
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
              child: Text(
                l10n.providersApiKeysNote,
                style: garraText(
                  size: 12,
                  color: GarraColors.textMuted,
                  height: 1.4,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ProviderRow extends StatelessWidget {
  final ProviderInfo info;
  const _ProviderRow({required this.info});

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final local = const {
      'ollama',
      'llamacpp',
      'lmstudio',
      'vllm',
    }.contains(info.id);
    return GarraListTile(
      icon: local ? Icons.desktop_windows_rounded : Icons.cloud_outlined,
      iconColor: info.active ? GarraColors.cyan : GarraColors.textDim,
      title: info.displayName,
      subtitle: [
        info.model ??
            (info.models.isEmpty ? l10n.providersNoModel : info.models.first),
        info.active
            ? l10n.providersStatusActive
            : (info.needsApiKey
                  ? l10n.providersStatusNeedsApiKey
                  : l10n.providersStatusInactive),
      ].join(' · '),
      trailing: info.isDefault
          ? Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
              decoration: BoxDecoration(
                color: GarraColors.violet.withValues(alpha: 0.18),
                borderRadius: BorderRadius.circular(GarraRadius.pill),
                border: Border.all(
                  color: GarraColors.violetLight.withValues(alpha: 0.6),
                ),
              ),
              child: Text(
                l10n.providersDefaultBadge,
                style: garraText(
                  size: 10.5,
                  weight: FontWeight.w700,
                  color: GarraColors.violetLight,
                ),
              ),
            )
          : null,
    );
  }
}
