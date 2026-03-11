import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:garraia_mobile/main.dart';

void main() {
  testWidgets('GarraApp smoke test — renders without crashing',
      (WidgetTester tester) async {
    await tester.pumpWidget(const ProviderScope(child: GarraApp()));
    // The app initialises with a splash screen while auth is loading.
    expect(find.byType(MaterialApp), findsOneWidget);
  });
}
