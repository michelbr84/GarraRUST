import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/models.dart';
import 'package:garraia_mobile/runtime/runtime_providers.dart';
import 'package:garraia_mobile/widgets/slash_suggestions.dart';

const _all = [
  SlashCommandInfo(name: 'help', description: 'Show available commands'),
  SlashCommandInfo(name: 'health', description: 'Gateway health'),
  SlashCommandInfo(name: 'mode', description: 'Get or set the agent mode'),
];

void main() {
  group('matchSlashCommands', () {
    test('prefix match, case-insensitive, registry order', () {
      expect(matchSlashCommands('/he', _all).map((c) => c.name), [
        'help',
        'health',
      ]);
      expect(matchSlashCommands('/HE', _all).map((c) => c.name), [
        'help',
        'health',
      ]);
      expect(matchSlashCommands('/', _all).length, 3);
    });

    test('goes away once an argument starts or without a slash', () {
      expect(matchSlashCommands('/mode ', _all), isEmpty);
      expect(matchSlashCommands('/mode debug', _all), isEmpty);
      expect(matchSlashCommands('hello', _all), isEmpty);
      expect(matchSlashCommands('', _all), isEmpty);
      expect(matchSlashCommands('/zzz', _all), isEmpty);
    });
  });

  group('SlashCommandInfo.fromJson', () {
    test('object and bare-string shapes, leading slash dropped', () {
      final a = SlashCommandInfo.fromJson({'name': 'help', 'description': 'x'});
      expect(a.name, 'help');
      expect(a.description, 'x');
      expect(SlashCommandInfo.fromJson('/mode').name, 'mode');
      expect(SlashCommandInfo.fromJson({'name': '/clear'}).name, 'clear');
    });
  });

  testWidgets('typing /he shows chips; tapping completes the command', (
    tester,
  ) async {
    final ctrl = TextEditingController();
    addTearDown(ctrl.dispose);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [slashCommandsProvider.overrideWith((ref) async => _all)],
        child: MaterialApp(
          home: Scaffold(body: SlashSuggestions(controller: ctrl)),
        ),
      ),
    );
    await tester.pump();
    expect(find.byType(ActionChip), findsNothing);

    ctrl.text = '/he';
    await tester.pump();
    expect(find.text('/help'), findsOneWidget);
    expect(find.text('/health'), findsOneWidget);
    expect(find.text('/mode'), findsNothing);

    await tester.tap(find.text('/help'));
    await tester.pump();
    expect(ctrl.text, '/help ');
    expect(ctrl.selection.baseOffset, '/help '.length);
    // Argument position: suggestions step aside.
    expect(find.byType(ActionChip), findsNothing);
  });
}
