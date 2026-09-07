// Home screen (v0.4.0) — the reference design rendered from real providers.
// Health and capabilities are stubbed; the assertions cover what the screen
// promises: greeting from onboarding, the two status cards, the six tiles,
// and capability negotiation (a missing feature renders "Unavailable").

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/models.dart';
import 'package:garraia_mobile/runtime/runtime_config.dart';
import 'package:garraia_mobile/runtime/runtime_providers.dart';
import 'package:garraia_mobile/screens/home_screen.dart';
import 'package:garraia_mobile/widgets/home/feature_tile.dart';
import 'package:go_router/go_router.dart';

class _LocalConfig extends RuntimeConfigState {
  @override
  Future<RuntimeConfig?> build() async => const RuntimeConfig(
    mode: RuntimeMode.local,
    baseUrl: kDefaultLocalBaseUrl,
    ownerName: 'Michel',
  );
}

const _health = GarraHealth(
  status: 'healthy',
  version: '0.4.0',
  gatewayUrl: 'http://127.0.0.1:3888',
  uptimeSecs: 42,
  activeSessions: 1,
  provider: 'ollama',
  model: 'qwen2.5:7b',
);

const _caps = GarraCapabilities(
  features: {
    GarraFeature.chat,
    GarraFeature.memory,
    GarraFeature.learningSkills,
    GarraFeature.projects,
    GarraFeature.modes,
    // no `automations` — the gateway does not expose scheduling yet
  },
);

Widget _app(ProviderContainer container) {
  final router = GoRouter(
    initialLocation: '/home',
    routes: [
      GoRoute(path: '/home', builder: (_, __) => const HomeScreen()),
      for (final r in [
        '/chat',
        '/memory',
        '/skills',
        '/files',
        '/agents',
        '/automations',
        '/providers',
        '/settings',
        '/onboarding',
      ])
        GoRoute(
          path: r,
          builder: (_, __) => Scaffold(body: Text('ROUTE $r')),
        ),
    ],
  );
  return UncontrolledProviderScope(
    container: container,
    child: MaterialApp.router(routerConfig: router),
  );
}

ProviderContainer _container({
  GarraCapabilities caps = _caps,
  Object? healthError,
}) {
  return ProviderContainer(
    // Riverpod 3 retries throwing providers with a backoff Timer; widget
    // tests must end with no pending timers.
    retry: (_, __) => null,
    overrides: [
      runtimeConfigStateProvider.overrideWith(_LocalConfig.new),
      runtimeHealthProvider.overrideWith((ref) async {
        if (healthError != null) throw healthError;
        return _health;
      }),
      runtimeCapabilitiesProvider.overrideWith((ref) async => caps),
    ],
  );
}

void main() {
  setUp(() {
    // The reference layout is a phone; widget tests default to 800×600.
    TestWidgetsFlutterBinding.ensureInitialized();
  });

  Future<void> pumpPhone(WidgetTester tester, Widget app) async {
    tester.view.physicalSize = const Size(390 * 3, 844 * 3);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(app);
    await tester.pumpAndSettle();
  }

  testWidgets('renders header, greeting, status cards and the six tiles', (
    tester,
  ) async {
    final container = _container();
    addTearDown(container.dispose);
    await pumpPhone(tester, _app(container));

    expect(find.text('Local-first AI Assistant'), findsOneWidget);
    expect(find.text('Hello, Michel 👋'), findsOneWidget);

    // Status cards come from /api/health.
    expect(find.textContaining('Local on this phone'), findsOneWidget);
    expect(find.textContaining('Connected to PC'), findsOneWidget);
    expect(find.text('qwen2.5:7b'), findsOneWidget);

    for (final t in [
      'Chat',
      'Memory',
      'Skills',
      'Files',
      'Agents',
      'Automations',
    ]) {
      expect(
        find.widgetWithText(FeatureTile, t),
        findsOneWidget,
        reason: 'tile $t',
      );
    }
    await tester.scrollUntilVisible(
      find.text('Quick Actions'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Quick Actions'), findsOneWidget);
    await tester.scrollUntilVisible(
      find.text('LOCAL AI. A BRIGHTER YOU.'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('LOCAL AI. A BRIGHTER YOU.'), findsOneWidget);
  });

  testWidgets(
    'a feature the runtime does not advertise renders as Unavailable',
    (tester) async {
      final container = _container();
      addTearDown(container.dispose);
      await pumpPhone(tester, _app(container));

      // Exactly one tile is unavailable: Automations.
      expect(find.text('Unavailable'), findsOneWidget);
      final automations = tester.widget<FeatureTile>(
        find.widgetWithText(FeatureTile, 'Automations'),
      );
      expect(automations.available, isFalse);
      final chat = tester.widget<FeatureTile>(
        find.widgetWithText(FeatureTile, 'Chat'),
      );
      expect(chat.available, isTrue);
    },
  );

  testWidgets('unreachable runtime is said plainly on the status card', (
    tester,
  ) async {
    final container = _container(healthError: Exception('connection refused'));
    addTearDown(container.dispose);
    await pumpPhone(tester, _app(container));

    expect(find.textContaining('Not running'), findsOneWidget);
    expect(find.textContaining('Not connected'), findsOneWidget);
  });

  testWidgets('tiles navigate to their routes', (tester) async {
    final container = _container();
    addTearDown(container.dispose);
    await pumpPhone(tester, _app(container));

    await tester.tap(find.widgetWithText(FeatureTile, 'Memory'));
    await tester.pumpAndSettle();
    expect(find.text('ROUTE /memory'), findsOneWidget);
  });
}
