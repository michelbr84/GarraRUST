import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'notification_service.g.dart';

/// Firebase messaging setup is a stub — actual Firebase config requires
/// manual setup of google-services.json / GoogleService-Info.plist.
///
/// This service handles local notifications and provides hooks for
/// Firebase Cloud Messaging when configured.

@Riverpod(keepAlive: true)
NotificationService notificationService(Ref ref) => NotificationService();

class NotificationService {
  final FlutterLocalNotificationsPlugin _localNotifications =
      FlutterLocalNotificationsPlugin();

  final StreamController<NotificationPayload> _onTapController =
      StreamController<NotificationPayload>.broadcast();

  /// Stream of notification taps — listen to navigate to specific chats.
  Stream<NotificationPayload> get onNotificationTap => _onTapController.stream;

  bool _initialized = false;

  /// Initialize local notification channels and request permissions.
  Future<void> initialize() async {
    if (_initialized) return;
    _initialized = true;

    const androidSettings =
        AndroidInitializationSettings('@mipmap/ic_launcher');
    const iosSettings = DarwinInitializationSettings(
      requestAlertPermission: true,
      requestBadgePermission: true,
      requestSoundPermission: true,
    );

    await _localNotifications.initialize(
      const InitializationSettings(
        android: androidSettings,
        iOS: iosSettings,
      ),
      onDidReceiveNotificationResponse: _onNotificationResponse,
    );

    // Create notification channels for Android
    await _createChannels();
  }

  Future<void> _createChannels() async {
    final android = _localNotifications.resolvePlatformSpecificImplementation<
        AndroidFlutterLocalNotificationsPlugin>();

    if (android != null) {
      await android.createNotificationChannel(const AndroidNotificationChannel(
        'chat_messages',
        'Mensagens do Chat',
        description: 'Notificacoes de novas mensagens do chat',
        importance: Importance.high,
      ));

      await android.createNotificationChannel(const AndroidNotificationChannel(
        'sync_status',
        'Status de Sincronizacao',
        description: 'Notificacoes de sincronizacao entre dispositivos',
        importance: Importance.low,
      ));

      await android.createNotificationChannel(const AndroidNotificationChannel(
        'system',
        'Sistema',
        description: 'Notificacoes do sistema',
        importance: Importance.defaultImportance,
      ));
    }
  }

  void _onNotificationResponse(NotificationResponse response) {
    final payload = response.payload;
    if (payload != null && payload.isNotEmpty) {
      _onTapController.add(NotificationPayload.parse(payload));
    }
  }

  /// Show a local notification for a new chat message.
  Future<void> showChatNotification({
    required String sessionId,
    required String senderName,
    required String message,
    int notificationId = 0,
  }) async {
    await _localNotifications.show(
      notificationId,
      senderName,
      message,
      const NotificationDetails(
        android: AndroidNotificationDetails(
          'chat_messages',
          'Mensagens do Chat',
          importance: Importance.high,
          priority: Priority.high,
          showWhen: true,
        ),
        iOS: DarwinNotificationDetails(
          presentAlert: true,
          presentBadge: true,
          presentSound: true,
        ),
      ),
      payload: 'chat:$sessionId',
    );
  }

  /// Show a sync status notification.
  Future<void> showSyncNotification({
    required String title,
    required String body,
  }) async {
    await _localNotifications.show(
      9999, // Fixed ID for sync notifications (replaces previous)
      title,
      body,
      const NotificationDetails(
        android: AndroidNotificationDetails(
          'sync_status',
          'Status de Sincronizacao',
          importance: Importance.low,
          priority: Priority.low,
          ongoing: false,
        ),
      ),
    );
  }

  /// Cancel all notifications.
  Future<void> cancelAll() async {
    await _localNotifications.cancelAll();
  }

  void dispose() {
    _onTapController.close();
  }

  // ── Firebase stub ──────────────────────────────────────────────────────────
  // Uncomment and configure when Firebase is set up:
  //
  // Future<void> initializeFirebase() async {
  //   await Firebase.initializeApp();
  //   final messaging = FirebaseMessaging.instance;
  //
  //   // Request permission
  //   await messaging.requestPermission(
  //     alert: true, badge: true, sound: true,
  //   );
  //
  //   // Get FCM token for device registration
  //   final token = await messaging.getToken();
  //   debugPrint('FCM Token: $token');
  //
  //   // Handle foreground messages
  //   FirebaseMessaging.onMessage.listen((message) {
  //     showChatNotification(
  //       sessionId: message.data['session_id'] ?? '',
  //       senderName: message.notification?.title ?? 'Garra',
  //       message: message.notification?.body ?? '',
  //     );
  //   });
  //
  //   // Handle background/terminated tap
  //   FirebaseMessaging.onMessageOpenedApp.listen((message) {
  //     _onTapController.add(
  //       NotificationPayload.parse('chat:${message.data['session_id']}'),
  //     );
  //   });
  // }
}

/// Parsed notification payload for routing.
class NotificationPayload {
  final String type; // "chat" | "session"
  final String id;

  NotificationPayload({required this.type, required this.id});

  /// Parse a payload string like "chat:abc123" or "session:xyz".
  factory NotificationPayload.parse(String raw) {
    final parts = raw.split(':');
    return NotificationPayload(
      type: parts.isNotEmpty ? parts[0] : 'chat',
      id: parts.length > 1 ? parts.sublist(1).join(':') : '',
    );
  }
}
