import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../l10n/l10n.dart';
import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/garra_bottom_nav.dart';
import '../widgets/copy_to_clipboard.dart';
import '../widgets/garra_page.dart';
import 'memory_screen.dart' show SectionHeader;

part 'activity_screen.g.dart';

@riverpod
Future<List<GarraSession>> gatewaySessions(Ref ref) =>
    requireConnection(ref).listSessions();

@riverpod
Future<List<String>> gatewayLogs(Ref ref) => requireConnection(ref).logs();

/// Activity tab — live sessions on the runtime and the tail of its log.
class ActivityScreen extends ConsumerWidget {
  const ActivityScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final sessions = ref.watch(gatewaySessionsProvider);
    final logs = ref.watch(gatewayLogsProvider);
    final current = ref.watch(currentSessionProvider).value;
    final l10n = context.l10n;

    return GarraPage(
      title: l10n.activityTitle,
      actions: [
        IconButton(
          icon: const Icon(Icons.refresh_rounded),
          tooltip: l10n.commonRefresh,
          onPressed: () {
            ref.invalidate(gatewaySessionsProvider);
            ref.invalidate(gatewayLogsProvider);
          },
        ),
      ],
      bottomNavigationBar: const GarraBottomNav(current: GarraTab.activity),
      body: ListView(
        padding: const EdgeInsets.only(bottom: 24),
        children: [
          SectionHeader(l10n.activitySectionSessions),
          sessions.when(
            loading: () => const Padding(
              padding: EdgeInsets.all(24),
              child: Center(child: CircularProgressIndicator()),
            ),
            error: (e, _) => ErrorState(
              message: e.toString(),
              onRetry: () => ref.invalidate(gatewaySessionsProvider),
            ),
            data: (list) => list.isEmpty
                ? EmptyState(
                    icon: Icons.forum_outlined,
                    text: l10n.activityNoSessions,
                  )
                : Column(
                    children: [
                      for (final s in list)
                        GarraListTile(
                          icon: Icons.forum_outlined,
                          iconColor: s.sessionId == current
                              ? GarraColors.violetLight
                              : GarraColors.textMuted,
                          title: s.sessionId == current
                              ? l10n.activityCurrentSession
                              : _short(s.sessionId),
                          subtitle:
                              l10n.activityMessageCount(s.historyLength) +
                              (s.channelId != null ? ' · ${s.channelId}' : ''),
                          trailing: const Icon(
                            Icons.chevron_right_rounded,
                            color: GarraColors.textMuted,
                          ),
                          onTap: () async {
                            await ref
                                .read(currentSessionProvider.notifier)
                                .select(s.sessionId);
                            if (context.mounted) context.push('/chat');
                          },
                        ),
                    ],
                  ),
          ),
          SectionHeader(l10n.activitySectionRuntimeLog),
          logs.when(
            loading: () => const SizedBox.shrink(),
            error: (e, _) => Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20),
              child: Text(
                e.toString(),
                style: garraText(size: 12, color: GarraColors.warn),
              ),
            ),
            data: (lines) {
              final text = lines.isEmpty
                  ? l10n.activityLogEmpty
                  : lines.reversed.take(80).toList().reversed.join('\n');
              return Container(
                margin: const EdgeInsets.symmetric(horizontal: 16),
                padding: const EdgeInsets.fromLTRB(12, 12, 12, 12),
                decoration: BoxDecoration(
                  color: GarraColors.bgElevated,
                  borderRadius: BorderRadius.circular(GarraRadius.card),
                  border: Border.all(color: GarraColors.panelBorder),
                ),
                child: Stack(
                  children: [
                    Padding(
                      padding: const EdgeInsets.only(right: 28),
                      child: SelectableText(
                        text,
                        style: garraText(
                          size: 11,
                          mono: true,
                          color: GarraColors.textMuted,
                          height: 1.4,
                        ),
                      ),
                    ),
                    Positioned(
                      top: -6,
                      right: -6,
                      child: CopyIconButton(
                        key: const ValueKey('copy-log'),
                        text: text,
                        tooltip: l10n.activityCopyLogTooltip,
                        toast: l10n.activityCopyLogToast,
                        color: GarraColors.textMuted,
                        size: 16,
                      ),
                    ),
                  ],
                ),
              );
            },
          ),
        ],
      ),
    );
  }

  static String _short(String id) => id.length > 12
      ? '${id.substring(0, 8)}…${id.substring(id.length - 4)}'
      : id;
}
