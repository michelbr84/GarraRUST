import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/models.dart';
import 'package:garraia_mobile/screens/memory_screen.dart';
import 'package:garraia_mobile/theme/garra_theme.dart';
import 'package:garraia_mobile/widgets/chat_bubble.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final copied = <String>[];

  setUp(() {
    copied.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
          if (call.method == 'Clipboard.setData') {
            copied.add((call.arguments as Map)['text'] as String);
          }
          return null;
        });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, null);
  });

  Widget wrap(Widget child) => MaterialApp(
    theme: garraTheme(),
    home: Scaffold(body: ListView(children: [child])),
  );

  testWidgets('user bubble is selectable and copies whole message', (
    tester,
  ) async {
    await tester.pumpWidget(
      wrap(
        const ChatBubble(
          message: ChatMessage(role: 'user', content: 'oi garra'),
        ),
      ),
    );
    expect(find.byType(SelectableText), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('copy-message')));
    await tester.pump();
    expect(copied, ['oi garra']);
    expect(find.text('Mensagem copiada'), findsOneWidget);
  });

  testWidgets('assistant code block gets its own copy button', (tester) async {
    const content = 'Veja:\n\n```dart\nvoid main() {}\nprint(1);\n```\n';
    await tester.pumpWidget(
      wrap(
        const ChatBubble(
          message: ChatMessage(role: 'assistant', content: content),
        ),
      ),
    );
    expect(find.byKey(const ValueKey('copy-message')), findsOneWidget);
    expect(find.byKey(const ValueKey('copy-code')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('copy-code')));
    await tester.pump();
    expect(copied, ['void main() {}\nprint(1);']);

    await tester.tap(find.byKey(const ValueKey('copy-message')));
    await tester.pump();
    expect(copied.last, content);
  });

  testWidgets('inline code keeps the default rendering (no code button)', (
    tester,
  ) async {
    await tester.pumpWidget(
      wrap(
        const ChatBubble(
          message: ChatMessage(role: 'assistant', content: 'use `garra start`'),
        ),
      ),
    );
    expect(find.byKey(const ValueKey('copy-code')), findsNothing);
  });

  testWidgets('memory sheet shows the full text and copies it', (tester) async {
    const entry = MemoryEntry(
      id: 'm1',
      role: 'assistant',
      content: 'O usuario prefere respostas curtas.',
      createdAt: '2026-09-07T20:00:00Z',
    );
    await tester.pumpWidget(wrap(const MemorySheet(entry: entry)));
    expect(find.text(entry.content), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('copy-memory')));
    await tester.pump();
    expect(copied, [entry.content]);
  });
}
