import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

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
    return GarraPage(
      title: 'Providers',
      body: AsyncBody<List<ProviderInfo>>(
        value: value,
        emptyText:
            'The runtime reported no providers. Run `garra init` to configure one.',
        onRetry: () => ref.invalidate(llmProvidersProvider),
        builder: (list) => ListView(
          padding: const EdgeInsets.only(top: 4, bottom: 24),
          children: [
            for (final p in list) _ProviderRow(info: p),
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
              child: Text(
                'API keys are configured on the runtime (garra init / web console), never stored on this phone.',
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
        info.model ?? (info.models.isEmpty ? 'no model' : info.models.first),
        info.active
            ? 'active'
            : (info.needsApiKey ? 'needs API key' : 'inactive'),
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
                'default',
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
