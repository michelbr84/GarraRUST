import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../providers/auth_provider.dart';
import '../screens/chat_screen.dart';
import '../screens/login_screen.dart';
import '../screens/pair_screen.dart';
import '../screens/register_screen.dart';
import '../screens/splash_screen.dart';

part 'app_router.g.dart';

@Riverpod(keepAlive: true)
GoRouter appRouter(Ref ref) {
  final router = GoRouter(
    initialLocation: '/splash',
    redirect: (context, state) {
      final authState = ref.read(authStateProvider);
      final isLoading = authState.isLoading;
      final isAuthenticated = authState.valueOrNull != null;
      final location = state.matchedLocation;
      final onAuth = location == '/login' || location == '/register';

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

      // Deep link: garraia://chat/:sessionId
      GoRoute(
        path: '/chat/:sessionId',
        builder: (_, state) {
          final sessionId = state.pathParameters['sessionId'] ?? '';
          return ChatScreen(key: ValueKey(sessionId));
        },
      ),

      // Deep link: garraia://session/:id (alias for chat)
      GoRoute(
        path: '/session/:id',
        redirect: (_, state) {
          final id = state.pathParameters['id'] ?? '';
          return '/chat/$id';
        },
      ),

      // Device pairing screen
      GoRoute(path: '/pair', builder: (_, __) => const PairScreen()),
    ],
  );

  // Trigger redirect re-evaluation whenever auth state changes.
  ref.listen(authStateProvider, (_, __) => router.refresh());
  return router;
}

// ── Deep Link Configuration Notes ────────────────────────────────────────────
//
// Android: Add to AndroidManifest.xml inside <activity>:
//   <intent-filter>
//     <action android:name="android.intent.action.VIEW"/>
//     <category android:name="android.intent.category.DEFAULT"/>
//     <category android:name="android.intent.category.BROWSABLE"/>
//     <data android:scheme="garraia" android:host="chat"/>
//     <data android:scheme="garraia" android:host="session"/>
//   </intent-filter>
//
// iOS: Add to Info.plist:
//   <key>CFBundleURLTypes</key>
//   <array>
//     <dict>
//       <key>CFBundleURLSchemes</key>
//       <array>
//         <string>garraia</string>
//       </array>
//     </dict>
//   </array>
