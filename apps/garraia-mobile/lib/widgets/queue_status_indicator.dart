import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../services/offline_queue.dart';

/// Shows a small banner when there are pending messages in the offline queue.
class QueueStatusIndicator extends ConsumerWidget {
  const QueueStatusIndicator({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(queueStatusProvider);
    final cs = Theme.of(context).colorScheme;

    // Hide when there are no pending messages and we are online
    if (status.pendingCount == 0 && status.isOnline) {
      return const SizedBox.shrink();
    }

    final Color bgColor;
    final String text;
    final IconData icon;

    if (!status.isOnline) {
      bgColor = cs.error;
      icon = Icons.cloud_off_rounded;
      text = status.pendingCount > 0
          ? 'Sem conexao - ${status.pendingCount} mensagem(ns) pendente(s)'
          : 'Sem conexao';
    } else if (status.isSyncing) {
      bgColor = cs.tertiary;
      icon = Icons.sync_rounded;
      text = 'Sincronizando ${status.pendingCount} mensagem(ns)...';
    } else {
      bgColor = cs.secondary;
      icon = Icons.schedule_rounded;
      text = '${status.pendingCount} mensagem(ns) pendente(s)';
    }

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      color: bgColor,
      child: Row(
        children: [
          Icon(icon, size: 16, color: cs.onError),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              text,
              style: TextStyle(color: cs.onError, fontSize: 12),
            ),
          ),
          if (status.isSyncing)
            SizedBox(
              width: 14,
              height: 14,
              child: CircularProgressIndicator(
                strokeWidth: 1.5,
                color: cs.onError,
              ),
            ),
        ],
      ),
    );
  }
}
