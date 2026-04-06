import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import 'api_service.dart';

part 'sync_service.g.dart';

/// Real-time sync service using WebSocket for cross-platform session sharing.
@Riverpod(keepAlive: true)
SyncService syncService(Ref ref) => SyncService(ref);

/// Tracks sync connection state for UI.
@riverpod
class SyncConnectionState extends _$SyncConnectionState {
  @override
  SyncStatus build() => SyncStatus.disconnected;

  void update(SyncStatus status) => state = status;
}

enum SyncStatus { disconnected, connecting, connected, error }

/// Holds the list of paired devices.
@riverpod
class PairedDevices extends _$PairedDevices {
  @override
  List<PairedDevice> build() => [];

  void setDevices(List<PairedDevice> devices) => state = devices;

  void addDevice(PairedDevice device) => state = [...state, device];

  void removeDevice(String deviceId) =>
      state = state.where((d) => d.deviceId != deviceId).toList();
}

class SyncService {
  final Ref _ref;
  WebSocketChannel? _channel;
  StreamSubscription<dynamic>? _subscription;
  Timer? _reconnectTimer;
  Timer? _heartbeatTimer;
  bool _disposed = false;

  /// Stream controller for incoming sync events.
  final StreamController<SyncEvent> _eventController =
      StreamController<SyncEvent>.broadcast();

  Stream<SyncEvent> get events => _eventController.stream;

  SyncService(this._ref);

  /// Connect to the sync WebSocket endpoint.
  Future<void> connect() async {
    if (_disposed) return;

    _ref
        .read(syncConnectionStateProvider.notifier)
        .update(SyncStatus.connecting);

    try {
      final api = _ref.read(apiServiceProvider);
      final token = await api.getSavedToken();
      if (token == null) {
        _ref
            .read(syncConnectionStateProvider.notifier)
            .update(SyncStatus.disconnected);
        return;
      }

      // Build WebSocket URL from API base URL
      final wsUrl = kApiBaseUrl
          .replaceFirst('https://', 'wss://')
          .replaceFirst('http://', 'ws://');

      _channel = WebSocketChannel.connect(
        Uri.parse('$wsUrl/ws/sync?token=$token'),
      );

      await _channel!.ready;

      _ref
          .read(syncConnectionStateProvider.notifier)
          .update(SyncStatus.connected);

      _subscription = _channel!.stream.listen(
        _onMessage,
        onError: _onError,
        onDone: _onDone,
      );

      // Start heartbeat
      _heartbeatTimer?.cancel();
      _heartbeatTimer = Timer.periodic(
        const Duration(seconds: 30),
        (_) => _sendPing(),
      );

      // Register this device
      _send(SyncCommand.registerDevice());
    } catch (e) {
      debugPrint('SyncService: connection failed: $e');
      _ref
          .read(syncConnectionStateProvider.notifier)
          .update(SyncStatus.error);
      _scheduleReconnect();
    }
  }

  void _onMessage(dynamic data) {
    try {
      final json = jsonDecode(data as String) as Map<String, dynamic>;
      final event = SyncEvent.fromJson(json);

      switch (event.type) {
        case 'message_sync':
          _eventController.add(event);
          break;
        case 'read_status':
          _eventController.add(event);
          break;
        case 'device_list':
          final devices = (event.data['devices'] as List<dynamic>?)
                  ?.map((d) =>
                      PairedDevice.fromJson(d as Map<String, dynamic>))
                  .toList() ??
              [];
          _ref.read(pairedDevicesProvider.notifier).setDevices(devices);
          break;
        case 'device_paired':
          final device =
              PairedDevice.fromJson(event.data['device'] as Map<String, dynamic>);
          _ref.read(pairedDevicesProvider.notifier).addDevice(device);
          break;
        case 'pong':
          // Heartbeat response — connection is alive
          break;
        default:
          debugPrint('SyncService: unknown event type: ${event.type}');
      }
    } catch (e) {
      debugPrint('SyncService: failed to parse message: $e');
    }
  }

  void _onError(Object error) {
    debugPrint('SyncService: WebSocket error: $error');
    _ref
        .read(syncConnectionStateProvider.notifier)
        .update(SyncStatus.error);
    _scheduleReconnect();
  }

  void _onDone() {
    debugPrint('SyncService: WebSocket closed');
    _ref
        .read(syncConnectionStateProvider.notifier)
        .update(SyncStatus.disconnected);
    _scheduleReconnect();
  }

  void _scheduleReconnect() {
    if (_disposed) return;
    _reconnectTimer?.cancel();
    _reconnectTimer = Timer(const Duration(seconds: 5), () {
      connect();
    });
  }

  void _sendPing() {
    _send(SyncCommand.ping());
  }

  void _send(SyncCommand command) {
    try {
      _channel?.sink.add(jsonEncode(command.toJson()));
    } catch (e) {
      debugPrint('SyncService: failed to send: $e');
    }
  }

  /// Send a message sync event to other devices.
  void syncMessage({
    required String sessionId,
    required String role,
    required String content,
    required String timestamp,
  }) {
    _send(SyncCommand(
      type: 'message_sync',
      data: {
        'session_id': sessionId,
        'role': role,
        'content': content,
        'timestamp': timestamp,
      },
    ));
  }

  /// Mark messages as read on a session (syncs to other devices).
  void markRead(String sessionId) {
    _send(SyncCommand(
      type: 'read_status',
      data: {'session_id': sessionId, 'read': true},
    ));
  }

  /// Request the list of paired devices.
  void requestDeviceList() {
    _send(SyncCommand(type: 'get_devices', data: {}));
  }

  /// Send a pairing token to link another device.
  void pairDevice(String pairingToken) {
    _send(SyncCommand(
      type: 'pair_device',
      data: {'token': pairingToken},
    ));
  }

  /// Disconnect and clean up.
  Future<void> disconnect() async {
    _heartbeatTimer?.cancel();
    _reconnectTimer?.cancel();
    await _subscription?.cancel();
    await _channel?.sink.close();
    _channel = null;
    _ref
        .read(syncConnectionStateProvider.notifier)
        .update(SyncStatus.disconnected);
  }

  void dispose() {
    _disposed = true;
    _heartbeatTimer?.cancel();
    _reconnectTimer?.cancel();
    _subscription?.cancel();
    _channel?.sink.close();
    _eventController.close();
  }
}

/// Incoming sync event from the WebSocket.
class SyncEvent {
  final String type;
  final Map<String, dynamic> data;

  SyncEvent({required this.type, required this.data});

  factory SyncEvent.fromJson(Map<String, dynamic> json) => SyncEvent(
        type: json['type'] as String? ?? '',
        data: json['data'] as Map<String, dynamic>? ?? {},
      );
}

/// Outgoing sync command to the WebSocket.
class SyncCommand {
  final String type;
  final Map<String, dynamic> data;

  SyncCommand({required this.type, required this.data});

  factory SyncCommand.registerDevice() => SyncCommand(
        type: 'register_device',
        data: {
          'platform': defaultTargetPlatform.name,
          'app_version': '0.1.0',
        },
      );

  factory SyncCommand.ping() => SyncCommand(type: 'ping', data: {});

  Map<String, dynamic> toJson() => {'type': type, 'data': data};
}

/// Represents a paired device.
class PairedDevice {
  final String deviceId;
  final String platform;
  final String lastSeen;
  final bool isOnline;

  PairedDevice({
    required this.deviceId,
    required this.platform,
    required this.lastSeen,
    this.isOnline = false,
  });

  factory PairedDevice.fromJson(Map<String, dynamic> json) => PairedDevice(
        deviceId: json['device_id'] as String? ?? '',
        platform: json['platform'] as String? ?? 'unknown',
        lastSeen: json['last_seen'] as String? ?? '',
        isOnline: json['is_online'] as bool? ?? false,
      );
}
