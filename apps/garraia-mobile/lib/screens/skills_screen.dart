import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

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

@riverpod
Future<List<String>> slashCommands(Ref ref) =>
    requireConnection(ref).slashCommands();

/// Extend what Garra can do — learned skills (`/api/learning/skills`) and
/// the slash-command registry (`/api/slash-commands`).
class SkillsScreen extends ConsumerWidget {
  const SkillsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final skills = ref.watch(learningSkillsProvider);
    final commands = ref.watch(slashCommandsProvider);

    return GarraPage(
      title: 'Skills',
      body: ListView(
        padding: const EdgeInsets.only(bottom: 24),
        children: [
          const SectionHeader('Learned skills'),
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
                ? const EmptyState(
                    icon: Icons.bolt_outlined,
                    text:
                        'No learned skills yet. Garra mines them from your sessions (ADR 0010).',
                  )
                : Column(children: [for (final s in list) _SkillRow(skill: s)]),
          ),
          const SectionHeader('Slash commands'),
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
                  for (final c in list)
                    Chip(
                      label: Text(
                        '/${c.replaceFirst('/', '')}',
                        style: garraText(size: 12.5, mono: true),
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
    return GarraListTile(
      icon: skill.locked ? Icons.lock_outline_rounded : Icons.bolt_outlined,
      iconColor: skill.deprecated ? GarraColors.textDim : GarraColors.gold,
      title: skill.name,
      subtitle: [
        if (skill.version.isNotEmpty) 'v${skill.version}',
        if (skill.scope.isNotEmpty) skill.scope,
        'score ${skill.score.toStringAsFixed(2)}',
        if (skill.failCount > 0)
          '${skill.failCount} fail${skill.failCount == 1 ? '' : 's'}',
        if (skill.deprecated) 'deprecated',
      ].join(' · '),
    );
  }
}
