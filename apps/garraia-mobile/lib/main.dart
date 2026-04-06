import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'router/app_router.dart';
import 'services/biometric_service.dart';
import 'services/notification_service.dart';
import 'services/offline_queue.dart';
import 'services/sync_service.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Initialize services before app starts
  final notificationService = NotificationService();
  await notificationService.initialize();

  runApp(ProviderScope(
    overrides: [
      notificationServiceProvider.overrideWithValue(notificationService),
    ],
    child: const GarraApp(),
  ));
}

class GarraApp extends ConsumerStatefulWidget {
  const GarraApp({super.key});

  @override
  ConsumerState<GarraApp> createState() => _GarraAppState();
}

class _GarraAppState extends ConsumerState<GarraApp>
    with WidgetsBindingObserver {
  bool _biometricChecked = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _initializeServices();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  Future<void> _initializeServices() async {
    // Initialize offline queue
    await ref.read(offlineQueueProvider).initialize();

    // Connect sync service
    ref.read(syncServiceProvider).connect();

    // Check biometric auth requirement
    await _checkBiometric();
  }

  Future<void> _checkBiometric() async {
    if (_biometricChecked) return;

    final biometric = ref.read(biometricServiceProvider);
    final enabled = await biometric.isEnabled();

    if (enabled) {
      final authenticated = await biometric.authenticate();
      if (authenticated) {
        ref.read(biometricAuthStateProvider.notifier).setAuthenticated();
      }
      // If not authenticated, the app will still load but
      // screens can check biometricAuthStateProvider
    }
    _biometricChecked = true;
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      // Re-check biometric on resume (if enabled)
      _biometricChecked = false;
      _checkBiometric();
      // Flush offline queue
      ref.read(offlineQueueProvider).onAppResume();
    }
  }

  @override
  Widget build(BuildContext context) {
    final router = ref.watch(appRouterProvider);

    // Listen for notification taps and navigate accordingly
    ref.listen(notificationServiceProvider, (_, service) {
      service.onNotificationTap.listen((payload) {
        if (payload.type == 'chat') {
          router.go('/chat/${payload.id}');
        } else if (payload.type == 'session') {
          router.go('/session/${payload.id}');
        }
      });
    });

    return MaterialApp.router(
      title: 'Garra',
      debugShowCheckedModeBanner: false,
      theme: _buildTheme(),
      routerConfig: router,
    );
  }

  ThemeData _buildTheme() {
    return ThemeData(
      useMaterial3: true,
      colorScheme: ColorScheme.fromSeed(
        seedColor: const Color(0xFF5B4CF5), // Garra purple
        brightness: Brightness.dark,
      ),
      fontFamily: 'Roboto',
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: Colors.white.withValues(alpha: 0.08),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(14),
          borderSide: BorderSide.none,
        ),
        contentPadding:
            const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      ),
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          minimumSize: const Size.fromHeight(50),
          shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(14)),
          textStyle:
              const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
        ),
      ),
    );
  }
}
