import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/garra_connection.dart';
import 'package:garraia_mobile/runtime/models.dart';
import 'package:garraia_mobile/runtime/runtime_providers.dart';
import 'package:garraia_mobile/screens/memory_screen.dart';
import 'package:garraia_mobile/theme/garra_theme.dart';

/// Only `deleteMemory` matters here; everything else is unreachable from
/// the sheet, so `noSuchMethod` keeps the fake honest and short.
class _FakeConnection implements GarraConnection {
  final deleted = <String>[];
  final bool existed;
  _FakeConnection({this.existed = true});

  @override
  Future<bool> deleteMemory(String id) async {
    deleted.add(id);
    return existed;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

const _entry = MemoryEntry(
  id: 'm1',
  role: 'assistant',
  content: 'O usuario prefere respostas curtas.',
  createdAt: '2026-09-07T20:00:00Z',
);

Widget _app(GarraConnection conn, {MemoryEntry entry = _entry}) =>
    ProviderScope(
      overrides: [garraConnectionProvider.overrideWithValue(conn)],
      child: MaterialApp(
        theme: garraTheme(),
        home: Scaffold(body: MemorySheet(entry: entry)),
      ),
    );

void main() {
  testWidgets('delete asks first; cancel keeps the memory', (tester) async {
    final conn = _FakeConnection();
    await tester.pumpWidget(_app(conn));
    await tester.tap(find.byKey(const ValueKey('delete-memory')));
    await tester.pumpAndSettle();
    expect(find.text('Apagar esta memoria?'), findsOneWidget);
    await tester.tap(find.text('Cancelar'));
    await tester.pumpAndSettle();
    expect(conn.deleted, isEmpty);
  });

  testWidgets('confirm calls DELETE with the id and reports it', (
    tester,
  ) async {
    final conn = _FakeConnection();
    await tester.pumpWidget(_app(conn));
    await tester.tap(find.byKey(const ValueKey('delete-memory')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('confirm-delete-memory')));
    await tester.pumpAndSettle();
    expect(conn.deleted, ['m1']);
    expect(find.text('Memoria apagada'), findsOneWidget);
  });

  testWidgets('a pinned memory warns; a 404 is "already gone"', (tester) async {
    final conn = _FakeConnection(existed: false);
    await tester.pumpWidget(
      _app(
        conn,
        entry: const MemoryEntry(
          id: 'm2',
          role: 'user',
          content: 'fixada',
          createdAt: '',
          pinned: true,
        ),
      ),
    );
    await tester.tap(find.byKey(const ValueKey('delete-memory')));
    await tester.pumpAndSettle();
    expect(find.textContaining('fixada'), findsWidgets);
    await tester.tap(find.byKey(const ValueKey('confirm-delete-memory')));
    await tester.pumpAndSettle();
    expect(conn.deleted, ['m2']);
    expect(find.text('Ja nao existia'), findsOneWidget);
  });

  test('MemoryEntry.pinned reads pinned_at', () {
    expect(
      MemoryEntry.fromJson({'id': 'a', 'pinned_at': '2026'}).pinned,
      isTrue,
    );
    expect(MemoryEntry.fromJson({'id': 'a'}).pinned, isFalse);
  });
}
