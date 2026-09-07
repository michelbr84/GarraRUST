import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../runtime/models.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/garra_page.dart';

part 'files_screen.g.dart';

@riverpod
Future<List<ProjectInfo>> projects(Ref ref) =>
    requireConnection(ref).projects();

@riverpod
Future<List<String>> projectFiles(Ref ref, String projectId) =>
    requireConnection(ref).projectFiles(projectId);

/// Access and manage files — the runtime's projects (`/api/projects`) and the
/// files each one tracks (`/api/projects/{id}/files`).
class FilesScreen extends ConsumerWidget {
  const FilesScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final value = ref.watch(projectsProvider);
    return GarraPage(
      title: 'Files',
      body: AsyncBody<List<ProjectInfo>>(
        value: value,
        emptyText:
            'No projects on this runtime yet. Create one from the Garra console or CLI.',
        onRetry: () => ref.invalidate(projectsProvider),
        builder: (list) => ListView.builder(
          padding: const EdgeInsets.only(bottom: 24, top: 4),
          itemCount: list.length,
          itemBuilder: (_, i) {
            final p = list[i];
            return GarraListTile(
              icon: Icons.folder_outlined,
              iconColor: GarraColors.cyan,
              title: p.name.isEmpty ? p.path : p.name,
              subtitle: p.description ?? p.path,
              trailing: const Icon(
                Icons.chevron_right_rounded,
                color: GarraColors.textMuted,
              ),
              onTap: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => _ProjectFilesScreen(project: p),
                ),
              ),
            );
          },
        ),
      ),
    );
  }
}

class _ProjectFilesScreen extends ConsumerWidget {
  final ProjectInfo project;
  const _ProjectFilesScreen({required this.project});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final value = ref.watch(projectFilesProvider(project.id));
    return GarraPage(
      title: project.name.isEmpty ? 'Files' : project.name,
      body: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 4, 20, 8),
            child: Align(
              alignment: Alignment.centerLeft,
              child: Text(
                project.path,
                style: garraText(
                  size: 12,
                  mono: true,
                  color: GarraColors.textMuted,
                ),
              ),
            ),
          ),
          Expanded(
            child: AsyncBody<List<String>>(
              value: value,
              emptyText: 'This project has no tracked files.',
              onRetry: () => ref.invalidate(projectFilesProvider(project.id)),
              builder: (files) => ListView.builder(
                padding: const EdgeInsets.only(bottom: 24),
                itemCount: files.length,
                itemBuilder: (_, i) => GarraListTile(
                  icon: Icons.insert_drive_file_outlined,
                  iconColor: GarraColors.textMuted,
                  title: files[i],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
