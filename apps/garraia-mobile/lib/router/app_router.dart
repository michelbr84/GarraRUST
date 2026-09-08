import 'package:flutter/widgets.dart';
import 'package:go_router/go_router.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../providers/auth_provider.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';
import '../screens/activity_screen.dart';
import '../screens/agents_screen.dart';
import '../screens/automations_screen.dart';
import '../screens/chat_screen.dart';
import '../screens/files_screen.dart';
import '../screens/home_screen.dart';
import '../screens/login_screen.dart';
import '../screens/memory_screen.dart';
import '../screens/notifications_screen.dart';
import '../screens/onboarding_screen.dart';
import '../screens/pair_screen.dart';
import '../screens/profile_screen.dart';
import '../screens/providers_screen.dart';
import '../screens/register_screen.dart';
import '../screens/settings_screen.dart';
import '../screens/skills_screen.dart';
import '../screens/splash_screen.dart';

part 'app_router.g.dart';

/// Route gate (v0.4.0): "is a runtime configured?" replaced "is there a
/// JWT?". Login only exists in cloud mode; local and LAN runtimes go straight
/// to the home screen.
@Riverpod(keepAlive: true)
GoRouter appRouter(Ref ref) {
  final router = GoRouter(
    initialLocation: '/splash',
    redirect: (context, state) {
      final configState = ref.read(runtimeConfigStateProvider);
      final location = state.matchedLocation;

      // Still reading preferences: stay on the splash.
      if (configState.isLoading) return null;

      final config = configState.value;
      final onOnboarding = location == '/onboarding';
      if (config == null) return onOnboarding ? null : '/onboarding';

      final onAuth = location == '/login' || location == '/register';
      if (config.mode == RuntimeMode.cloud) {
        final auth = ref.read(authStateProvider);
        if (auth.isLoading) return null;
        final authenticated = auth.value != null;
        if (!authenticated && !onAuth && !onOnboarding) return '/login';
        if (authenticated && onAuth) return '/home';
      } else if (onAuth) {
        return '/home';
      }

      if (location == '/splash') return '/home';
      return null;
    },
    routes: [
      GoRoute(path: '/splash', builder: (_, __) => const SplashScreen()),
      GoRoute(
        path: '/onboarding',
        builder: (_, __) => const OnboardingScreen(),
      ),
      GoRoute(path: '/login', builder: (_, __) => const LoginScreen()),
      GoRoute(path: '/register', builder: (_, __) => const RegisterScreen()),

      // Tabs
      GoRoute(path: '/home', builder: (_, __) => const HomeScreen()),
      GoRoute(path: '/activity', builder: (_, __) => const ActivityScreen()),
      GoRoute(
        path: '/notifications',
        builder: (_, __) => const NotificationsScreen(),
      ),
      GoRoute(path: '/profile', builder: (_, __) => const ProfileScreen()),

      // Feature tiles
      GoRoute(
        path: '/chat',
        // `?draft=` pre-fills the input (Skills → tap a command).
        builder: (_, state) =>
            ChatScreen(initialDraft: state.uri.queryParameters['draft']),
      ),
      GoRoute(
        // Deep link: garraia://chat/:sessionId
        path: '/chat/:sessionId',
        builder: (_, state) {
          final sessionId = state.pathParameters['sessionId'] ?? '';
          return ChatScreen(key: ValueKey(sessionId), sessionId: sessionId);
        },
      ),
      GoRoute(
        // Deep link: garraia://session/:id (alias for chat)
        path: '/session/:id',
        redirect: (_, state) => '/chat/${state.pathParameters['id'] ?? ''}',
      ),
      GoRoute(path: '/memory', builder: (_, __) => const MemoryScreen()),
      GoRoute(path: '/skills', builder: (_, __) => const SkillsScreen()),
      GoRoute(path: '/files', builder: (_, __) => const FilesScreen()),
      GoRoute(path: '/agents', builder: (_, __) => const AgentsScreen()),
      GoRoute(
        path: '/automations',
        builder: (_, __) => const AutomationsScreen(),
      ),

      // Quick actions
      GoRoute(path: '/pair', builder: (_, __) => const PairScreen()),
      GoRoute(path: '/providers', builder: (_, __) => const ProvidersScreen()),
      GoRoute(path: '/settings', builder: (_, __) => const SettingsScreen()),
    ],
  );

  // Re-evaluate the redirect whenever the runtime or the auth state changes.
  ref.listen(runtimeConfigStateProvider, (_, __) => router.refresh());
  ref.listen(authStateProvider, (_, __) => router.refresh());
  return router;
}
