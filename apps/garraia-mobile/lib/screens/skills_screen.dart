import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../l10n/l10n.dart';
import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/garra_page.dart';
import 'memory_screen.dart' show SectionHeader;

part 'skills_screen.g.dart';

@riverpod
Future<List<SkillSummary>> learningSkills(Ref ref) =>
    requireConnection(ref).skills();

/// Extend what Garra can do — learned skills (`/api/learning/skills`) and
/// the slash-command registry (`/api/slash-commands`).
class SkillsScreen extends ConsumerWidget {
  const SkillsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final skills = ref.watch(learningSkillsProvider);
    final commands = ref.watch(slashCommandsProvider);
    final l10n = context.l10n;

    return GarraPage(
      title: l10n.skillsTitle,
      body: ListView(
        padding: const EdgeInsets.only(bottom: 24),
        children: [
          SectionHeader(l10n.skillsSectionLearned),
          skills.when(
            loading: () => const Padding(
              padding: EdgeInsets.all(24),
              child: Center(child: CircularProgressIndicator()),
            ),
            error: (e, _) => ErrorState(
              message: e.toString(),
              onRetry: () => ref.invalidate(learningSkillsProvider),
            ),
            data: (list) => list.isEmpty
                ? EmptyState(
                    icon: Icons.bolt_outlined,
                    text: l10n.skillsNoLearnedSkills,
                  )
                : Column(children: [for (final s in list) _SkillRow(skill: s)]),
          ),
          SectionHeader(l10n.skillsSectionSlashCommands),
          commands.when(
            loading: () => const SizedBox.shrink(),
            error: (e, _) => Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20),
              child: Text(
                e.toString(),
                style: garraText(size: 12, color: GarraColors.warn),
              ),
            ),
            data: (list) => Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  // Tapping a command opens the chat with it typed in,
                  // caret after a trailing space, ready for arguments.
                  for (final c in list)
                    Tooltip(
                      message: c.description,
                      child: ActionChip(
                        label: Text(
                          '/${c.name}',
                          style: garraText(size: 12.5, mono: true),
                        ),
                        onPressed: () => context.go(
                          '/chat?draft=${Uri.encodeComponent('/${c.name} ')}',
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _SkillRow extends StatelessWidget {
  final SkillSummary skill;
  const _SkillRow({required this.skill});

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return GarraListTile(
      icon: skill.locked ? Icons.lock_outline_rounded : Icons.bolt_outlined,
      iconColor: skill.deprecated ? GarraColors.textDim : GarraColors.gold,
      title: skill.name,
      subtitle: [
        if (skill.version.isNotEmpty) l10n.skillsVersion(skill.version),
        if (skill.scope.isNotEmpty) skill.scope,
        l10n.skillsScore(skill.score.toStringAsFixed(2)),
        if (skill.failCount > 0) l10n.skillsFailCount(skill.failCount),
        if (skill.deprecated) l10n.skillsDeprecated,
      ].join(' · '),
    );
  }
}
