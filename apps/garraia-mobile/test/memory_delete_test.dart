import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/garra_connection.dart';
import 'package:garraia_mobile/runtime/models.dart';
import 'package:garraia_mobile/runtime/runtime_providers.dart';
import 'package:garraia_mobile/screens/memory_screen.dart';
import 'package:garraia_mobile/theme/garra_theme.dart';

import 'support/l10n_test_support.dart';

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

// The sheet's copy was written in pt-BR, so the tests pin that locale and
// read the expected strings from the ARB (#1178) instead of repeating them.
Widget _app(GarraConnection conn, {MemoryEntry entry = _entry}) =>
    ProviderScope(
      overrides: [garraConnectionProvider.overrideWithValue(conn)],
      child: MaterialApp(
        theme: garraTheme(),
        locale: testLocalePt,
        supportedLocales: supportedLocales,
        localizationsDelegates: localizationsDelegates,
        home: Scaffold(body: MemorySheet(entry: entry)),
      ),
    );

void main() {
  testWidgets('delete asks first; cancel keeps the memory', (tester) async {
    final l10n = await loadL10n();
    final conn = _FakeConnection();
    await tester.pumpWidget(_app(conn));
    await tester.tap(find.byKey(const ValueKey('delete-memory')));
    await tester.pumpAndSettle();
    expect(find.text(l10n.memoryDeleteTitle), findsOneWidget);
    await tester.tap(find.text(l10n.commonCancel));
    await tester.pumpAndSettle();
    expect(conn.deleted, isEmpty);
  });

  testWidgets('confirm calls DELETE with the id and reports it', (
    tester,
  ) async {
    final l10n = await loadL10n();
    final conn = _FakeConnection();
    await tester.pumpWidget(_app(conn));
    await tester.tap(find.byKey(const ValueKey('delete-memory')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('confirm-delete-memory')));
    await tester.pumpAndSettle();
    expect(conn.deleted, ['m1']);
    expect(find.text(l10n.memoryDeleted), findsOneWidget);
  });

  testWidgets('a pinned memory warns; a 404 is "already gone"', (tester) async {
    final l10n = await loadL10n();
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
    expect(find.text(l10n.memoryDeletePinnedBody), findsOneWidget);
    expect(find.text(l10n.memoryDeleteBody), findsNothing);
    await tester.tap(find.byKey(const ValueKey('confirm-delete-memory')));
    await tester.pumpAndSettle();
    expect(conn.deleted, ['m2']);
    expect(find.text(l10n.memoryAlreadyGone), findsOneWidget);
  });

  test('MemoryEntry.pinned reads pinned_at', () {
    expect(
      MemoryEntry.fromJson({'id': 'a', 'pinned_at': '2026'}).pinned,
      isTrue,
    );
    expect(MemoryEntry.fromJson({'id': 'a'}).pinned, isFalse);
  });
}
