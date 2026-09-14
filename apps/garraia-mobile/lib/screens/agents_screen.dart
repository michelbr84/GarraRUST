import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../l10n/l10n.dart';
import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_tokens.dart';
import '../widgets/garra_page.dart';
import 'memory_screen.dart' show SectionHeader;

part 'agents_screen.g.dart';

@riverpod
Future<List<ModeInfo>> agentModes(Ref ref) => requireConnection(ref).modes();

@riverpod
Future<List<McpServerInfo>> mcpServers(Ref ref) =>
    requireConnection(ref).mcpServers();

/// Create and manage agents — agent modes (`/api/modes`, built-in + custom)
/// and the MCP servers the runtime has wired (`/api/mcp`).
class AgentsScreen extends ConsumerWidget {
  const AgentsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final modes = ref.watch(agentModesProvider);
    final mcp = ref.watch(mcpServersProvider);
    final l10n = context.l10n;

    return GarraPage(
      title: l10n.agentsTitle,
      body: ListView(
        padding: const EdgeInsets.only(bottom: 24),
        children: [
          SectionHeader(l10n.agentsSectionModes),
          modes.when(
            loading: () => const Padding(
              padding: EdgeInsets.all(24),
              child: Center(child: CircularProgressIndicator()),
            ),
            error: (e, _) => ErrorState(
              message: e.toString(),
              onRetry: () => ref.invalidate(agentModesProvider),
            ),
            data: (list) => list.isEmpty
                ? EmptyState(
                    icon: Icons.groups_outlined,
                    text: l10n.agentsNoModes,
                  )
                : Column(
                    children: [
                      for (final m in list)
                        GarraListTile(
                          icon: Icons.smart_toy_outlined,
                          iconColor: GarraColors.violetLight,
                          title: m.name,
                          subtitle: m.description.isEmpty
                              ? m.id
                              : m.description,
                        ),
                    ],
                  ),
          ),
          SectionHeader(l10n.agentsSectionMcpServers),
          mcp.when(
            loading: () => const SizedBox.shrink(),
            error: (e, _) => ErrorState(
              message: e.toString(),
              onRetry: () => ref.invalidate(mcpServersProvider),
            ),
            data: (list) => list.isEmpty
                ? EmptyState(
                    icon: Icons.extension_outlined,
                    text: l10n.agentsNoMcpServers,
                  )
                : Column(
                    children: [
                      for (final s in list)
                        GarraListTile(
                          icon: Icons.extension_outlined,
                          iconColor: s.connected
                              ? GarraColors.ok
                              : GarraColors.textDim,
                          title: s.name,
                          subtitle: [
                            l10n.agentsToolCount(s.tools),
                            s.connected
                                ? l10n.agentsMcpConnected
                                : l10n.agentsMcpDisconnected,
                          ].join(' · '),
                        ),
                    ],
                  ),
          ),
        ],
      ),
    );
  }
}
