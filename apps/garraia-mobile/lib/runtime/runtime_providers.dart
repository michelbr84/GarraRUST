import 'dart:async';

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../services/api_service.dart';
import 'cloud_connection.dart';
import 'garra_connection.dart';
import 'gateway_connection.dart';
import 'models.dart';
import 'runtime_config.dart';

part 'runtime_providers.g.dart';

@Riverpod(keepAlive: true)
RuntimeStore runtimeStore(Ref ref) => RuntimeStore();

/// The persisted runtime selection. `null` means onboarding has not run.
@Riverpod(keepAlive: true)
class RuntimeConfigState extends _$RuntimeConfigState {
  @override
  Future<RuntimeConfig?> build() => ref.read(runtimeStoreProvider).load();

  /// Persists [config] (and the gateway API key when given) and rebuilds
  /// every consumer — the router redirect, the connection, the home cards.
  Future<void> save(RuntimeConfig config, {String? apiKey}) async {
    final store = ref.read(runtimeStoreProvider);
    await store.save(config);
    if (apiKey != null) await store.saveApiKey(apiKey);
    // A new runtime means the old gateway session id is meaningless.
    await store.saveSessionId(null);
    ref.invalidate(gatewayApiKeyProvider);
    state = AsyncData(config);
  }

  Future<void> updateOwnerName(String name) async {
    final current = state.value;
    if (current == null) return;
    await save(current.copyWith(ownerName: name));
  }

  Future<void> clear() async {
    await ref.read(runtimeStoreProvider).clear();
    ref.invalidate(gatewayApiKeyProvider);
    state = const AsyncData(null);
  }
}

/// Gateway API key from the secure store (LAN mode), if any.
@Riverpod(keepAlive: true)
Future<String?> gatewayApiKey(Ref ref) =>
    ref.read(runtimeStoreProvider).readApiKey();

/// The live connection for the current runtime, or `null` before onboarding.
@Riverpod(keepAlive: true)
GarraConnection? garraConnection(Ref ref) {
  final config = ref.watch(runtimeConfigStateProvider).value;
  if (config == null) return null;
  final apiKey = ref.watch(gatewayApiKeyProvider).value;
  return switch (config.mode) {
    RuntimeMode.cloud => CloudConnection(
      api: ref.watch(apiServiceProvider),
      baseUrl: config.baseUrl,
    ),
    RuntimeMode.local || RuntimeMode.remote => GatewayConnection(
      mode: config.mode,
      baseUrl: config.baseUrl,
      apiKey: apiKey,
    ),
  };
}

/// Convenience: the connection or a typed error for screens that need one.
GarraConnection requireConnection(Ref ref) {
  final c = ref.watch(garraConnectionProvider);
  if (c == null) throw const NoRuntimeConfigured();
  return c;
}

/// Poll interval for the home status cards. Long enough not to hurt battery,
/// short enough that starting `garra` in Termux flips the card within a
/// screen glance.
const Duration kHealthPollInterval = Duration(seconds: 20);

/// `GET /api/health`, refreshed while someone is watching.
@riverpod
Future<GarraHealth> runtimeHealth(Ref ref) async {
  final conn = requireConnection(ref);
  final timer = Timer(kHealthPollInterval, ref.invalidateSelf);
  ref.onDispose(timer.cancel);
  return conn.health();
}

/// `GET /api/capabilities` — drives which tiles are available.
@riverpod
Future<GarraCapabilities> runtimeCapabilities(Ref ref) async {
  final conn = requireConnection(ref);
  return conn.capabilities();
}

/// Gateway session the chat screen is bound to (persisted across launches).
@Riverpod(keepAlive: true)
class CurrentSession extends _$CurrentSession {
  @override
  Future<String?> build() => ref.read(runtimeStoreProvider).readSessionId();

  /// Returns the current session id, creating one on the runtime if needed.
  Future<String> ensure() async {
    final existing = state.value;
    if (existing != null && existing.isNotEmpty) return existing;
    final conn = requireConnection(ref);
    final id = await conn.createSession();
    await ref.read(runtimeStoreProvider).saveSessionId(id);
    state = AsyncData(id);
    return id;
  }

  Future<void> reset() async {
    await ref.read(runtimeStoreProvider).saveSessionId(null);
    state = const AsyncData(null);
  }

  Future<void> select(String id) async {
    await ref.read(runtimeStoreProvider).saveSessionId(id);
    state = AsyncData(id);
  }
}
