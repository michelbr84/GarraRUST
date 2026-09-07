import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'router/app_router.dart';
import 'runtime/runtime_config.dart';
import 'runtime/runtime_providers.dart';
import 'services/biometric_service.dart';
import 'services/notification_service.dart';
import 'services/offline_queue.dart';
import 'services/sync_service.dart';
import 'theme/garra_theme.dart';
import 'theme/garra_tokens.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // The app is always dark: match the system bars from the first frame.
  SystemChrome.setSystemUIOverlayStyle(
    const SystemUiOverlayStyle(
      statusBarColor: Colors.transparent,
      statusBarIconBrightness: Brightness.light,
      systemNavigationBarColor: GarraColors.bgElevated,
      systemNavigationBarIconBrightness: Brightness.light,
    ),
  );

  // Initialize services before app starts
  final notificationService = NotificationService();
  await notificationService.initialize();

  runApp(
    ProviderScope(
      overrides: [
        notificationServiceProvider.overrideWithValue(notificationService),
      ],
      child: const GarraApp(),
    ),
  );
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
    // Offline queue (SQLite). A missing/locked database must not take the
    // whole app down — chat still works, messages just are not queued.
    try {
      await ref.read(offlineQueueProvider).initialize();
    } catch (e) {
      debugPrint('offline queue unavailable: $e');
    }

    // The cross-device sync socket is a Cloud Alpha feature (`/ws/sync` +
    // JWT). Local and LAN runtimes have nothing to connect to.
    try {
      final config = await ref.read(runtimeConfigStateProvider.future);
      if (config?.mode == RuntimeMode.cloud) {
        ref.read(syncServiceProvider).connect();
      }
    } catch (e) {
      debugPrint('runtime config unavailable: $e');
    }

    // Check biometric auth requirement
    try {
      await _checkBiometric();
    } catch (e) {
      debugPrint('biometric check skipped: $e');
    }
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
      title: 'Garra Mobile',
      debugShowCheckedModeBanner: false,
      theme: garraTheme(),
      darkTheme: garraTheme(),
      themeMode: ThemeMode.dark,
      routerConfig: router,
    );
  }
}
