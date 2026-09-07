import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
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
