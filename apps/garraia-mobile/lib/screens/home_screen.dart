import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../l10n/l10n.dart';
import '../runtime/models.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_tokens.dart';
import '../widgets/garra_bottom_nav.dart';
import '../widgets/home/brand_banner.dart';
import '../widgets/home/feature_tile.dart';
import '../widgets/home/hero_greeting.dart';
import '../widgets/home/home_header.dart';
import '../widgets/home/quick_actions_panel.dart';
import '../widgets/home/runtime_status_card.dart';

/// Garra Mobile home — the screen of the reference design.
///
/// Everything dynamic on it is real: the two status cards read
/// `GET /api/health` (polled), the tiles read `GET /api/capabilities`, the
/// greeting reads the name from onboarding. Nothing is a mock-up.
class HomeScreen extends ConsumerWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final config = ref.watch(runtimeConfigStateProvider).value;
    final health = ref.watch(runtimeHealthProvider);
    final caps =
        ref.watch(runtimeCapabilitiesProvider).value ?? GarraCapabilities.empty;
    final capsLoaded = ref.watch(runtimeCapabilitiesProvider).hasValue;
    final l10n = context.l10n;

    bool available(String feature) => !capsLoaded || caps.has(feature);

    return Scaffold(
      backgroundColor: GarraColors.bg,
      body: Container(
        decoration: const BoxDecoration(
          gradient: RadialGradient(
            center: Alignment(-0.6, -1.1),
            radius: 1.2,
            colors: [Color(0xFF231B4F), GarraColors.bg],
            stops: [0, 0.65],
          ),
        ),
        child: SafeArea(
          bottom: false,
          child: RefreshIndicator(
            color: GarraColors.cyan,
            backgroundColor: GarraColors.panel,
            onRefresh: () async {
              ref.invalidate(runtimeHealthProvider);
              ref.invalidate(runtimeCapabilitiesProvider);
              try {
                await ref.read(runtimeHealthProvider.future);
              } catch (_) {
                // The status card already shows the failure; nothing to add.
              }
            },
            child: ListView(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 24),
              children: [
                const HomeHeader(),
                const SizedBox(height: 18),
                HeroGreeting(name: config?.ownerName ?? ''),
                const SizedBox(height: 14),
                _StatusRow(config: config, health: health),
                const SizedBox(height: 14),
                _TileGrid(available: available),
                const SizedBox(height: 14),
                QuickActionsPanel(
                  actions: [
                    QuickAction(
                      label: l10n.homeQuickPairPc,
                      icon: Icons.link_rounded,
                      onTap: () => context.push('/onboarding'),
                    ),
                    QuickAction(
                      label: l10n.providersTitle,
                      icon: Icons.cloud_outlined,
                      onTap: () => context.push('/providers'),
                    ),
                    QuickAction(
                      label: l10n.settingsTitle,
                      icon: Icons.settings_outlined,
                      onTap: () => context.push('/settings'),
                    ),
                  ],
                ),
                const SizedBox(height: 14),
                const BrandBanner(),
              ],
            ),
          ),
        ),
      ),
      bottomNavigationBar: const GarraBottomNav(current: GarraTab.home),
    );
  }
}

class _StatusRow extends StatelessWidget {
  final RuntimeConfig? config;
  final AsyncValue<GarraHealth> health;

  const _StatusRow({required this.config, required this.health});

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final mode = config?.mode ?? RuntimeMode.local;
    final h = health.value;
    final reachable = h != null && !health.hasError;
    final dot = health.hasError
        ? GarraColors.danger
        : h == null
        ? GarraColors.warn
        : h.healthy
        ? GarraColors.ok
        : GarraColors.warn;

    final runtimeValue = switch (mode) {
      RuntimeMode.local => l10n.homeRuntimeLocal,
      RuntimeMode.remote => l10n.homeRuntimeRemote,
      RuntimeMode.cloud => l10n.runtimeModeCloudTitle,
    };
    final runtimeSubtitle = health.hasError
        ? (mode == RuntimeMode.local
              ? l10n.homeRuntimeNotRunning
              : l10n.homeRuntimeUnreachable)
        : h == null
        ? l10n.commonChecking
        : mode == RuntimeMode.local
        ? l10n.homeRuntimeLocalTagline
        : l10n.homeRuntimeVersionSessions(h.version, h.activeSessions);

    final llmValue = !reachable
        ? l10n.homeLlmNotConnected
        : h.provider == null
        ? l10n.homeLlmNoProvider
        : _llmLabel(l10n, h.provider!, mode);
    final llmSubtitle = !reachable
        ? l10n.homeLlmWaitingRuntime
        : h.model ?? h.provider ?? l10n.homeLlmPickProvider;

    return IntrinsicHeight(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Expanded(
            child: RuntimeStatusCard(
              icon: mode == RuntimeMode.cloud
                  ? Icons.cloud_rounded
                  : Icons.smartphone_rounded,
              iconColor: GarraColors.cyan,
              dotColor: dot,
              label: l10n.homeRuntimeLabel,
              value: runtimeValue,
              subtitle: runtimeSubtitle,
              onTap: () => context.push('/settings'),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: RuntimeStatusCard(
              icon: Icons.desktop_windows_rounded,
              iconColor: GarraColors.blue,
              dotColor: reachable ? GarraColors.info : GarraColors.textDim,
              label: l10n.homeLlmLabel,
              value: llmValue,
              valueColor: reachable ? GarraColors.cyan : GarraColors.textMuted,
              subtitle: llmSubtitle,
              showChevron: true,
              onTap: () => context.push('/providers'),
            ),
          ),
        ],
      ),
    );
  }

  static String _llmLabel(
    AppLocalizations l10n,
    String provider,
    RuntimeMode mode,
  ) {
    final p = provider.toLowerCase();
    if (p == 'ollama' || p == 'llamacpp' || p == 'lmstudio' || p == 'vllm') {
      return mode == RuntimeMode.local
          ? l10n.homeLlmConnectedToPc
          : l10n.homeLlmLocalServer;
    }
    return provider;
  }
}

class _TileGrid extends StatelessWidget {
  final bool Function(String feature) available;

  const _TileGrid({required this.available});

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tiles = <Widget>[
      FeatureTile(
        title: l10n.homeTileChat,
        subtitle: l10n.homeTileChatSubtitle,
        icon: Icons.chat_bubble_outline_rounded,
        accent: GarraAccent.violet,
        available: available(GarraFeature.chat),
        onTap: () => context.push('/chat'),
      ),
      FeatureTile(
        title: l10n.memoryTitle,
        subtitle: l10n.homeTileMemorySubtitle,
        icon: Icons.psychology_outlined,
        accent: GarraAccent.magenta,
        available: available(GarraFeature.memory),
        onTap: () => context.push('/memory'),
      ),
      FeatureTile(
        title: l10n.skillsTitle,
        subtitle: l10n.homeTileSkillsSubtitle,
        icon: Icons.bolt_outlined,
        accent: GarraAccent.gold,
        available: available(GarraFeature.learningSkills),
        onTap: () => context.push('/skills'),
      ),
      FeatureTile(
        title: l10n.filesTitle,
        subtitle: l10n.homeTileFilesSubtitle,
        icon: Icons.folder_outlined,
        accent: GarraAccent.cyan,
        available: available(GarraFeature.projects),
        onTap: () => context.push('/files'),
      ),
      FeatureTile(
        title: l10n.agentsTitle,
        subtitle: l10n.homeTileAgentsSubtitle,
        icon: Icons.groups_outlined,
        accent: GarraAccent.indigo,
        available: available(GarraFeature.modes),
        onTap: () => context.push('/agents'),
      ),
      FeatureTile(
        title: l10n.automationsTitle,
        subtitle: l10n.homeTileAutomationsSubtitle,
        icon: Icons.settings_suggest_outlined,
        accent: GarraAccent.rose,
        available: available(GarraFeature.automations),
        onTap: () => context.push('/automations'),
      ),
    ];

    return GridView.builder(
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
        crossAxisCount: 2,
        mainAxisSpacing: 12,
        crossAxisSpacing: 12,
        mainAxisExtent: 132,
      ),
      itemCount: tiles.length,
      itemBuilder: (_, i) => tiles[i],
    );
  }
}
