import 'package:go_router/go_router.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../providers/auth_provider.dart';
import '../screens/chat_screen.dart';
import '../screens/login_screen.dart';
import '../screens/register_screen.dart';
import '../screens/splash_screen.dart';

part 'app_router.g.dart';

@Riverpod(keepAlive: true)
GoRouter appRouter(AppRouterRef ref) {
  final router = GoRouter(
    initialLocation: '/splash',
    redirect: (context, state) {
      final authState = ref.read(authStateProvider);
      final isLoading = authState.isLoading;
      final isAuthenticated = authState.valueOrNull != null;
      final onAuth = state.matchedLocation == '/login' ||
          state.matchedLocation == '/register';

      // While loading: stay put (splash on first load, login/register during auth ops).
      if (isLoading) return null;
      if (!isAuthenticated && !onAuth) return '/login';
      if (isAuthenticated && onAuth) return '/chat';
      return null;
    },
    routes: [
      GoRoute(path: '/splash', builder: (_, __) => const SplashScreen()),
      GoRoute(path: '/login', builder: (_, __) => const LoginScreen()),
      GoRoute(path: '/register', builder: (_, __) => const RegisterScreen()),
      GoRoute(path: '/chat', builder: (_, __) => const ChatScreen()),
    ],
  );

  // Trigger redirect re-evaluation whenever auth state changes.
  ref.listen(authStateProvider, (_, __) => router.refresh());
  return router;
}
