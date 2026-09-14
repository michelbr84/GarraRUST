import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../l10n/l10n.dart';

/// Where the Garra runtime that this app talks to lives.
///
/// The UI never branches on this beyond labels and onboarding copy — every
/// data call goes through `GarraConnection`, which is the whole point of the
/// abstraction (strategy §15–16, ADR 0016 v1).
enum RuntimeMode {
  /// `garra` running on this phone (Termux today, embedded Rust later).
  local,

  /// A Garra gateway on another machine (PC / home server / LAN).
  remote,

  /// The hosted Garra Cloud API (JWT login).
  cloud,
}

extension RuntimeModeLabels on RuntimeMode {
  /// Display name in the app's language (#1178). Takes the strings rather
  /// than a `BuildContext` so providers and tests can label a mode too:
  /// `mode.title(context.l10n)` in widgets, `lookupAppLocalizations` elsewhere.
  String title(AppLocalizations l10n) => switch (this) {
    RuntimeMode.local => l10n.runtimeModeLocalTitle,
    RuntimeMode.remote => l10n.runtimeModeRemoteTitle,
    RuntimeMode.cloud => l10n.runtimeModeCloudTitle,
  };

  /// One-line explanation shown under [title] in onboarding.
  String description(AppLocalizations l10n) => switch (this) {
    RuntimeMode.local => l10n.runtimeModeLocalDescription,
    RuntimeMode.remote => l10n.runtimeModeRemoteDescription,
    RuntimeMode.cloud => l10n.runtimeModeCloudDescription,
  };

  /// Base URL suggested when the user has not typed one.
  String get defaultBaseUrl => switch (this) {
    RuntimeMode.local => kDefaultLocalBaseUrl,
    RuntimeMode.remote => 'http://192.168.1.10:3888',
    RuntimeMode.cloud => kCloudBaseUrl,
  };

  String get storageKey => name;

  static RuntimeMode? parse(String? v) {
    if (v == null) return null;
    for (final m in RuntimeMode.values) {
      if (m.name == v) return m;
    }
    return null;
  }
}

/// `garra start` binds 3888 by default (`SETUP.md`, `ci.yml`). Inside Termux
/// the gateway and the app share the loopback interface.
const String kDefaultLocalBaseUrl = 'http://127.0.0.1:3888';

/// Hosted API. Overridable at build time for staging:
/// `--dart-define=API_BASE_URL=https://staging.example`.
const String kCloudBaseUrl = String.fromEnvironment(
  'API_BASE_URL',
  defaultValue: 'https://api.garraia.org',
);

/// Persisted runtime selection. Secrets (gateway API key) are NOT part of
/// this value object — they live in the secure store, see [RuntimeStore].
class RuntimeConfig {
  final RuntimeMode mode;
  final String baseUrl;

  /// Display name shown in the home greeting; empty = neutral greeting.
  final String ownerName;

  const RuntimeConfig({
    required this.mode,
    required this.baseUrl,
    this.ownerName = '',
  });

  RuntimeConfig copyWith({
    RuntimeMode? mode,
    String? baseUrl,
    String? ownerName,
  }) => RuntimeConfig(
    mode: mode ?? this.mode,
    baseUrl: baseUrl ?? this.baseUrl,
    ownerName: ownerName ?? this.ownerName,
  );

  /// Normalises what people type: trims, adds `http://` when the scheme is
  /// missing, drops a trailing slash.
  static String normalizeBaseUrl(String raw) {
    var s = raw.trim();
    if (s.isEmpty) return s;
    if (!s.contains('://')) s = 'http://$s';
    while (s.endsWith('/')) {
      s = s.substring(0, s.length - 1);
    }
    return s;
  }

  static bool isValidBaseUrl(String raw) {
    final s = normalizeBaseUrl(raw);
    final uri = Uri.tryParse(s);
    return uri != null &&
        (uri.scheme == 'http' || uri.scheme == 'https') &&
        uri.host.isNotEmpty;
  }

  @override
  bool operator ==(Object other) =>
      other is RuntimeConfig &&
      other.mode == mode &&
      other.baseUrl == baseUrl &&
      other.ownerName == ownerName;

  @override
  int get hashCode => Object.hash(mode, baseUrl, ownerName);
}

/// Persistence for [RuntimeConfig] (SharedPreferences) and the optional
/// gateway API key (`flutter_secure_storage`, never preferences).
class RuntimeStore {
  static const _kMode = 'garraia_runtime_mode';
  static const _kBaseUrl = 'garraia_runtime_base_url';
  static const _kOwnerName = 'garraia_owner_name';
  static const _kSessionId = 'garraia_session_id';
  static const _kApiKey = 'garraia_gateway_api_key';

  final FlutterSecureStorage _secure;

  RuntimeStore({FlutterSecureStorage? secure})
    : _secure = secure ?? const FlutterSecureStorage();

  Future<RuntimeConfig?> load() async {
    final prefs = await SharedPreferences.getInstance();
    final mode = RuntimeModeLabels.parse(prefs.getString(_kMode));
    if (mode == null) return null;
    return RuntimeConfig(
      mode: mode,
      baseUrl: prefs.getString(_kBaseUrl) ?? mode.defaultBaseUrl,
      ownerName: prefs.getString(_kOwnerName) ?? '',
    );
  }

  Future<void> save(RuntimeConfig config) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_kMode, config.mode.storageKey);
    await prefs.setString(_kBaseUrl, config.baseUrl);
    await prefs.setString(_kOwnerName, config.ownerName);
  }

  Future<void> clear() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_kMode);
    await prefs.remove(_kBaseUrl);
    await prefs.remove(_kSessionId);
    await _secure.delete(key: _kApiKey);
  }

  Future<String?> readApiKey() => _secure.read(key: _kApiKey);

  Future<void> saveApiKey(String? key) async {
    if (key == null || key.trim().isEmpty) {
      await _secure.delete(key: _kApiKey);
    } else {
      await _secure.write(key: _kApiKey, value: key.trim());
    }
  }

  /// The gateway session the chat screen resumes on relaunch.
  Future<String?> readSessionId() async =>
      (await SharedPreferences.getInstance()).getString(_kSessionId);

  Future<void> saveSessionId(String? id) async {
    final prefs = await SharedPreferences.getInstance();
    if (id == null) {
      await prefs.remove(_kSessionId);
    } else {
      await prefs.setString(_kSessionId, id);
    }
  }
}
