import 'dart:async';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter/foundation.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'package:sqflite/sqflite.dart';

import '../providers/chat_provider.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';

part 'offline_queue.g.dart';

/// Offline message queue with SQLite persistence and auto-retry.
@Riverpod(keepAlive: true)
OfflineQueue offlineQueue(Ref ref) => OfflineQueue(ref);

/// Provides the current queue status for UI display.
@riverpod
class QueueStatus extends _$QueueStatus {
  @override
  OfflineQueueState build() => const OfflineQueueState();

  void update(OfflineQueueState newState) => state = newState;
}

class OfflineQueueState {
  final int pendingCount;
  final bool isSyncing;
  final bool isOnline;

  const OfflineQueueState({
    this.pendingCount = 0,
    this.isSyncing = false,
    this.isOnline = true,
  });

  OfflineQueueState copyWith({
    int? pendingCount,
    bool? isSyncing,
    bool? isOnline,
  }) => OfflineQueueState(
    pendingCount: pendingCount ?? this.pendingCount,
    isSyncing: isSyncing ?? this.isSyncing,
    isOnline: isOnline ?? this.isOnline,
  );
}

class OfflineQueue {
  final Ref _ref;
  Database? _db;
  StreamSubscription<List<ConnectivityResult>>? _connectivitySub;
  Timer? _retryTimer;
  bool _syncing = false;

  OfflineQueue(this._ref);

  /// Initialize the queue database and start monitoring connectivity.
  Future<void> initialize() async {
    _db = await openDatabase(
      'garraia_offline_queue.db',
      version: 1,
      onCreate: (db, version) async {
        await db.execute('''
          CREATE TABLE pending_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message TEXT NOT NULL,
            created_at TEXT NOT NULL,
            retry_count INTEGER DEFAULT 0,
            status TEXT DEFAULT 'pending'
          )
        ''');
      },
    );

    // Monitor connectivity changes
    _connectivitySub = Connectivity().onConnectivityChanged.listen(
      _onConnectivityChanged,
    );

    // Check initial connectivity
    final result = await Connectivity().checkConnectivity();
    _onConnectivityChanged(result);

    // Update pending count
    await _updateQueueStatus();
  }

  void _onConnectivityChanged(List<ConnectivityResult> results) {
    final isOnline = results.any((r) => r != ConnectivityResult.none);

    _ref
        .read(queueStatusProvider.notifier)
        .update(_ref.read(queueStatusProvider).copyWith(isOnline: isOnline));

    if (isOnline) {
      // Connection restored — try to flush the queue
      _scheduleRetry();
    } else {
      _retryTimer?.cancel();
    }
  }

  /// Enqueue a message for sending. Returns immediately.
  Future<int> enqueue(String message) async {
    final db = _db;
    if (db == null) return -1;

    final id = await db.insert('pending_messages', {
      'message': message,
      'created_at': DateTime.now().toIso8601String(),
      'status': 'pending',
    });

    await _updateQueueStatus();
    _scheduleRetry();
    return id;
  }

  /// Try to send all pending messages in order.
  ///
  /// Goes to the runtime directly instead of through `chatMessagesProvider`:
  /// that notifier is autoDispose and only alive while a chat screen watches
  /// it, whereas this queue is keepAlive and fires from connectivity
  /// callbacks with no screen open. The session comes from
  /// `currentSessionProvider` (keepAlive, persisted), so the flush lands in
  /// the conversation the user opens next; an open chat is told to reload.
  Future<void> flushQueue() async {
    if (_syncing) return;
    _syncing = true;

    _ref
        .read(queueStatusProvider.notifier)
        .update(_ref.read(queueStatusProvider).copyWith(isSyncing: true));

    final db = _db;
    if (db == null) {
      _syncing = false;
      return;
    }

    var sentAny = false;
    try {
      // Before onboarding there is nothing to send to; messages stay queued.
      final conn = _ref.read(garraConnectionProvider);
      if (conn == null) return;

      final pending = await db.query(
        'pending_messages',
        where: 'status = ?',
        whereArgs: ['pending'],
        orderBy: 'id ASC',
      );

      for (final row in pending) {
        final id = row['id'] as int;
        final message = row['message'] as String;
        final retryCount = row['retry_count'] as int? ?? 0;

        try {
          final sessionId = conn.mode == RuntimeMode.cloud
              ? null
              : await _ref.read(currentSessionProvider.notifier).ensure();
          await conn.sendMessage(message, sessionId: sessionId);
          await db.delete('pending_messages', where: 'id = ?', whereArgs: [id]);
          sentAny = true;
        } catch (e) {
          debugPrint('OfflineQueue: failed to send message $id: $e');
          if (retryCount >= 5) {
            // Mark as failed after 5 retries
            await db.update(
              'pending_messages',
              {'status': 'failed', 'retry_count': retryCount + 1},
              where: 'id = ?',
              whereArgs: [id],
            );
          } else {
            await db.update(
              'pending_messages',
              {'retry_count': retryCount + 1},
              where: 'id = ?',
              whereArgs: [id],
            );
          }
          // Stop processing on first failure (preserve order)
          break;
        }
      }
    } finally {
      _syncing = false;
      await _updateQueueStatus();
      // The replies live in the runtime's history now; a chat screen that is
      // open rebuilds from it (no-op when nobody is watching).
      if (sentAny) _ref.invalidate(chatMessagesProvider);
    }
  }

  /// Get all pending messages (for UI display).
  Future<List<PendingMessage>> getPendingMessages() async {
    final db = _db;
    if (db == null) return [];

    final rows = await db.query(
      'pending_messages',
      where: 'status = ?',
      whereArgs: ['pending'],
      orderBy: 'id ASC',
    );

    return rows.map(PendingMessage.fromRow).toList();
  }

  /// Clear failed messages from the queue.
  Future<void> clearFailed() async {
    final db = _db;
    if (db == null) return;
    await db.delete(
      'pending_messages',
      where: 'status = ?',
      whereArgs: ['failed'],
    );
    await _updateQueueStatus();
  }

  void _scheduleRetry() {
    _retryTimer?.cancel();
    _retryTimer = Timer(const Duration(seconds: 3), () {
      flushQueue();
    });
  }

  Future<void> _updateQueueStatus() async {
    final db = _db;
    if (db == null) return;

    final count = Sqflite.firstIntValue(
      await db.rawQuery(
        "SELECT COUNT(*) FROM pending_messages WHERE status = 'pending'",
      ),
    );

    final current = _ref.read(queueStatusProvider);
    _ref
        .read(queueStatusProvider.notifier)
        .update(
          current.copyWith(pendingCount: count ?? 0, isSyncing: _syncing),
        );
  }

  /// Call on app resume to flush any pending messages.
  Future<void> onAppResume() async {
    final results = await Connectivity().checkConnectivity();
    final isOnline = results.any((r) => r != ConnectivityResult.none);
    if (isOnline) {
      await flushQueue();
    }
  }

  Future<void> dispose() async {
    _retryTimer?.cancel();
    await _connectivitySub?.cancel();
    await _db?.close();
  }
}

class PendingMessage {
  final int id;
  final String message;
  final String createdAt;
  final int retryCount;
  final String status;

  PendingMessage({
    required this.id,
    required this.message,
    required this.createdAt,
    required this.retryCount,
    required this.status,
  });

  factory PendingMessage.fromRow(Map<String, dynamic> row) => PendingMessage(
    id: row['id'] as int,
    message: row['message'] as String,
    createdAt: row['created_at'] as String,
    retryCount: row['retry_count'] as int? ?? 0,
    status: row['status'] as String? ?? 'pending',
  );
}
