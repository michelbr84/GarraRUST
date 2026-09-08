import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/copy_to_clipboard.dart';
import '../widgets/garra_page.dart';

part 'memory_screen.g.dart';

@riverpod
Future<List<MemoryEntry>> recentMemory(Ref ref) =>
    requireConnection(ref).recentMemory(limit: 50);

@riverpod
Future<List<MemoryEntry>> memorySearch(Ref ref, String query) =>
    query.trim().isEmpty
    ? Future.value(const [])
    : requireConnection(ref).searchMemory(query.trim());

/// What Garra remembers — `GET /api/memory/recent` and `/api/memory/search`.
class MemoryScreen extends ConsumerStatefulWidget {
  const MemoryScreen({super.key});

  @override
  ConsumerState<MemoryScreen> createState() => _MemoryScreenState();
}

class _MemoryScreenState extends ConsumerState<MemoryScreen> {
  final _query = TextEditingController();
  String _submitted = '';

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final searching = _submitted.isNotEmpty;
    final value = searching
        ? ref.watch(memorySearchProvider(_submitted))
        : ref.watch(recentMemoryProvider);

    return GarraPage(
      title: 'Memory',
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 4, 16, 8),
            child: TextField(
              controller: _query,
              textInputAction: TextInputAction.search,
              decoration: InputDecoration(
                hintText: 'Search memories',
                prefixIcon: const Icon(Icons.search_rounded),
                suffixIcon: searching
                    ? IconButton(
                        icon: const Icon(Icons.close_rounded),
                        onPressed: () {
                          _query.clear();
                          setState(() => _submitted = '');
                        },
                      )
                    : null,
              ),
              onSubmitted: (v) => setState(() => _submitted = v.trim()),
            ),
          ),
          Expanded(
            child: AsyncBody<List<MemoryEntry>>(
              value: value,
              emptyText: searching
                  ? 'No memory matches "$_submitted".'
                  : 'Garra has not stored any memory yet. Chat a little and come back.',
              onRetry: () => ref.invalidate(
                searching
                    ? memorySearchProvider(_submitted)
                    : recentMemoryProvider,
              ),
              builder: (items) => ListView.builder(
                padding: const EdgeInsets.only(bottom: 24),
                itemCount: items.length,
                itemBuilder: (_, i) => _MemoryRow(entry: items[i]),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _MemoryRow extends StatelessWidget {
  final MemoryEntry entry;
  const _MemoryRow({required this.entry});

  @override
  Widget build(BuildContext context) {
    final isUser = entry.role == 'user';
    return GarraListTile(
      icon: isUser ? Icons.person_outline_rounded : Icons.psychology_outlined,
      iconColor: isUser ? GarraColors.cyan : GarraColors.magenta,
      title: entry.content,
      subtitle: [
        if (entry.role.isNotEmpty) entry.role,
        if (entry.createdAt.isNotEmpty) entry.createdAt.split('T').first,
      ].join(' · '),
      trailing: const Icon(
        Icons.chevron_right_rounded,
        color: GarraColors.textDim,
      ),
      // The row truncates; the sheet shows the whole memory, copies and
      // deletes it.
      onTap: () => showModalBottomSheet<void>(
        context: context,
        showDragHandle: true,
        isScrollControlled: true,
        builder: (_) => MemorySheet(entry: entry),
      ),
    );
  }
}

/// Full text of one memory, selectable, with copy and delete.
///
/// Delete asks first (it is irreversible and, on the runtime, ignores the
/// pin), then calls `DELETE /api/memory/{id}` and refreshes both lists. A
/// `404` means it was already gone: still a success for the user.
class MemorySheet extends ConsumerWidget {
  final MemoryEntry entry;
  const MemorySheet({super.key, required this.entry});

  Future<void> _delete(BuildContext context, WidgetRef ref) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Apagar esta memoria?'),
        content: Text(
          entry.pinned
              ? 'Ela esta fixada, mas apagar nao pergunta duas vezes: some do runtime e nao da para desfazer.'
              : 'Some do runtime e nao da para desfazer.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: const Text('Cancelar'),
          ),
          FilledButton(
            key: const ValueKey('confirm-delete-memory'),
            onPressed: () => Navigator.of(ctx).pop(true),
            child: const Text('Apagar'),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    final conn = ref.read(garraConnectionProvider);
    if (conn == null) return;
    try {
      final existed = await conn.deleteMemory(entry.id);
      ref.invalidate(recentMemoryProvider);
      ref.invalidate(memorySearchProvider);
      if (!context.mounted) return;
      ScaffoldMessenger.maybeOf(context)?.showSnackBar(
        SnackBar(
          content: Text(existed ? 'Memoria apagada' : 'Ja nao existia'),
          duration: const Duration(seconds: 1),
        ),
      );
      Navigator.of(context).maybePop();
    } catch (e) {
      if (!context.mounted) return;
      ScaffoldMessenger.maybeOf(
        context,
      )?.showSnackBar(SnackBar(content: Text('Nao deu para apagar: $e')));
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final meta = [
      if (entry.role.isNotEmpty) entry.role,
      if (entry.createdAt.isNotEmpty) entry.createdAt.replaceFirst('T', ' '),
    ].join(' · ');
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxHeight: MediaQuery.sizeOf(context).height * 0.75,
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (meta.isNotEmpty)
                Text(
                  meta,
                  style: garraText(size: 12.5, color: GarraColors.textMuted),
                ),
              const SizedBox(height: 10),
              Flexible(
                child: SingleChildScrollView(
                  child: SelectableText(
                    entry.content,
                    style: garraText(size: 14.5, height: 1.45),
                  ),
                ),
              ),
              const SizedBox(height: 14),
              Row(
                children: [
                  Expanded(
                    child: FilledButton.icon(
                      key: const ValueKey('copy-memory'),
                      onPressed: () => copyToClipboard(
                        context,
                        entry.content,
                        toast: 'Memoria copiada',
                      ),
                      icon: const Icon(Icons.copy_rounded, size: 18),
                      label: const Text('Copiar'),
                    ),
                  ),
                  const SizedBox(width: 10),
                  OutlinedButton.icon(
                    key: const ValueKey('delete-memory'),
                    onPressed: () => _delete(context, ref),
                    icon: const Icon(Icons.delete_outline_rounded, size: 18),
                    label: const Text('Apagar'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Small helper reused by other screens for section headers.
class SectionHeader extends StatelessWidget {
  final String text;
  const SectionHeader(this.text, {super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 6),
      child: Text(
        text.toUpperCase(),
        style: garraText(
          size: 11,
          weight: FontWeight.w700,
          letterSpacing: 1.2,
          color: GarraColors.textMuted,
        ),
      ),
    );
  }
}
