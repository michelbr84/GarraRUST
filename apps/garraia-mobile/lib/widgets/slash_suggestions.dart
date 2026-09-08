import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';

/// Commands matching what the user is typing.
///
/// `text` must start with `/` and contain no whitespace yet: once the name is
/// complete and an argument begins, suggestions get out of the way. Prefix
/// match, case-insensitive, registry order preserved. Pure, so it is unit
/// tested without a widget tree.
List<SlashCommandInfo> matchSlashCommands(
  String text,
  List<SlashCommandInfo> all,
) {
  final t = text.trimLeft();
  if (!t.startsWith('/') || t.contains(RegExp(r'\s'))) return const [];
  final prefix = t.substring(1).toLowerCase();
  return [
    for (final c in all)
      if (c.name.toLowerCase().startsWith(prefix)) c,
  ];
}

/// Strip above the chat input: one chip per registry command matching the
/// current `/prefix`. Tapping completes the name and leaves the caret after
/// a trailing space, ready for arguments. Renders nothing while the list is
/// loading, failed, or the input is not a `/prefix`.
class SlashSuggestions extends ConsumerWidget {
  final TextEditingController controller;

  const SlashSuggestions({super.key, required this.controller});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final commands = ref.watch(slashCommandsProvider).value ?? const [];
    if (commands.isEmpty) return const SizedBox.shrink();
    return ValueListenableBuilder<TextEditingValue>(
      valueListenable: controller,
      builder: (context, value, _) {
        final matches = matchSlashCommands(value.text, commands);
        if (matches.isEmpty) return const SizedBox.shrink();
        return SizedBox(
          height: 46,
          child: ListView.separated(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
            itemCount: matches.length,
            separatorBuilder: (_, __) => const SizedBox(width: 6),
            itemBuilder: (_, i) {
              final c = matches[i];
              return Tooltip(
                message: c.description,
                child: ActionChip(
                  visualDensity: VisualDensity.compact,
                  label: Text(
                    '/${c.name}',
                    style: garraText(size: 12.5, mono: true),
                  ),
                  onPressed: () {
                    final text = '/${c.name} ';
                    controller.value = TextEditingValue(
                      text: text,
                      selection: TextSelection.collapsed(offset: text.length),
                    );
                  },
                ),
              );
            },
          ),
        );
      },
    );
  }
}
