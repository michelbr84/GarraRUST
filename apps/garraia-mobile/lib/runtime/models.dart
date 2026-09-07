// Data models for the Garra Local Protocol (the gateway's `/api/*` surface)
// as consumed by the mobile app. Parsing is tolerant on purpose: the
// gateway is a moving target across versions, and a missing optional field
// must degrade to an empty value, never crash the UI.

/// `GET /api/health` — `crates/garraia-gateway/src/health.rs::HealthResponse`.
class GarraHealth {
  final String status; // healthy | degraded | unhealthy
  final String version;
  final String gatewayUrl;
  final int uptimeSecs;
  final int activeSessions;
  final String? provider;
  final String? model;
  final List<String> channels;
  final List<String> warnings;

  const GarraHealth({
    required this.status,
    required this.version,
    required this.gatewayUrl,
    required this.uptimeSecs,
    required this.activeSessions,
    this.provider,
    this.model,
    this.channels = const [],
    this.warnings = const [],
  });

  bool get healthy => status == 'healthy';
  bool get degraded => status == 'degraded';

  factory GarraHealth.fromJson(Map<String, dynamic> json) => GarraHealth(
    status: json['status'] as String? ?? 'unknown',
    version: json['version'] as String? ?? '?',
    gatewayUrl: json['gateway_url'] as String? ?? '',
    uptimeSecs: (json['uptime_secs'] as num?)?.toInt() ?? 0,
    activeSessions: (json['active_sessions'] as num?)?.toInt() ?? 0,
    provider: json['provider'] as String?,
    model: json['model'] as String?,
    channels: _strings(json['channels']),
    warnings: _strings(json['warnings']),
  );
}

/// `GET /api/capabilities` — `capabilities.rs::CapabilitiesResponse`.
///
/// [features] is the negotiation surface: a tile whose feature is absent
/// renders as unavailable (see `FeatureTile`).
class GarraCapabilities {
  final Set<String> features;
  final List<String> providers;
  final List<String> models;
  final List<String> channels;
  final List<String> commands;
  final String version;

  const GarraCapabilities({
    required this.features,
    this.providers = const [],
    this.models = const [],
    this.channels = const [],
    this.commands = const [],
    this.version = '?',
  });

  static const empty = GarraCapabilities(features: {});

  bool has(String feature) => features.contains(feature);

  factory GarraCapabilities.fromJson(Map<String, dynamic> json) =>
      GarraCapabilities(
        features: _strings(json['features']).toSet(),
        providers: _strings(json['providers']),
        models: _strings(json['models']),
        channels: _strings(json['channels']),
        commands: _strings(json['commands']),
        version: json['version'] as String? ?? '?',
      );
}

/// Feature names the gateway advertises. Kept as constants so the UI and the
/// Rust side (`capabilities_handler`) drift visibly rather than silently.
abstract final class GarraFeature {
  static const chat = 'chat';
  static const websocket = 'websocket';
  static const memory = 'memory';
  static const learningSkills = 'learning-skills';
  static const projects = 'projects';
  static const modes = 'modes';
  static const mcp = 'mcp';
  static const stt = 'stt';
  static const tts = 'tts';
  static const automations = 'automations';
}

/// `GET /api/sessions` — `api.rs::SessionInfo`.
class GarraSession {
  final String sessionId;
  final String? channelId;
  final bool connected;
  final int historyLength;

  const GarraSession({
    required this.sessionId,
    this.channelId,
    this.connected = false,
    this.historyLength = 0,
  });

  factory GarraSession.fromJson(Map<String, dynamic> json) => GarraSession(
    sessionId: json['session_id'] as String? ?? '',
    channelId: json['channel_id'] as String?,
    connected: json['connected'] as bool? ?? false,
    historyLength: (json['history_length'] as num?)?.toInt() ?? 0,
  );
}

/// One chat turn. `GET /api/sessions/{id}/history` yields `{role, content}`
/// only; the cloud `/chat/history` adds `timestamp`.
class ChatMessage {
  final String role; // "user" | "assistant"
  final String content;
  final String timestamp;

  const ChatMessage({
    required this.role,
    required this.content,
    this.timestamp = '',
  });

  factory ChatMessage.fromJson(Map<String, dynamic> json) => ChatMessage(
    role: json['role'] as String? ?? 'assistant',
    content: json['content'] as String? ?? '',
    timestamp: json['timestamp'] as String? ?? '',
  );
}

/// `GET /api/memory/recent|search` → `memories[]` (`garraia-db::MemoryEntry`).
class MemoryEntry {
  final String id;
  final String role;
  final String content;
  final String createdAt;
  final String? sessionId;

  const MemoryEntry({
    required this.id,
    required this.role,
    required this.content,
    required this.createdAt,
    this.sessionId,
  });

  factory MemoryEntry.fromJson(Map<String, dynamic> json) => MemoryEntry(
    id: json['id']?.toString() ?? '',
    role: json['role']?.toString().toLowerCase() ?? '',
    content: json['content'] as String? ?? '',
    createdAt: json['created_at']?.toString() ?? '',
    sessionId: json['session_id'] as String?,
  );
}

/// `GET /api/learning/skills` — `learning_handler.rs::SkillSummary`.
class SkillSummary {
  final String name;
  final String version;
  final String scope;
  final double score;
  final bool locked;
  final bool deprecated;
  final int failCount;
  final String source;

  const SkillSummary({
    required this.name,
    this.version = '',
    this.scope = '',
    this.score = 0,
    this.locked = false,
    this.deprecated = false,
    this.failCount = 0,
    this.source = '',
  });

  factory SkillSummary.fromJson(Map<String, dynamic> json) => SkillSummary(
    name: json['name'] as String? ?? '',
    version: json['version']?.toString() ?? '',
    scope: json['scope']?.toString() ?? '',
    score: (json['score'] as num?)?.toDouble() ?? 0,
    locked: json['locked'] as bool? ?? false,
    deprecated: json['deprecated'] as bool? ?? false,
    failCount: (json['fail_count'] as num?)?.toInt() ?? 0,
    source: json['source']?.toString() ?? '',
  );
}

/// `GET /api/providers` entry (`router.rs::list_providers`).
class ProviderInfo {
  final String id;
  final String displayName;
  final bool active;
  final bool isDefault;
  final bool needsApiKey;
  final String? model;
  final List<String> models;

  const ProviderInfo({
    required this.id,
    required this.displayName,
    this.active = false,
    this.isDefault = false,
    this.needsApiKey = false,
    this.model,
    this.models = const [],
  });

  factory ProviderInfo.fromJson(Map<String, dynamic> json) => ProviderInfo(
    id: json['id'] as String? ?? '',
    displayName: json['display_name'] as String? ?? json['id'] as String? ?? '',
    active: json['active'] as bool? ?? false,
    isDefault: json['is_default'] as bool? ?? false,
    needsApiKey: json['needs_api_key'] as bool? ?? false,
    model: json['model'] as String?,
    models: _strings(json['models']),
  );
}

/// `GET /api/projects` — `projects_handler.rs::Project`.
class ProjectInfo {
  final String id;
  final String name;
  final String path;
  final String? description;
  final String updatedAt;

  const ProjectInfo({
    required this.id,
    required this.name,
    required this.path,
    this.description,
    this.updatedAt = '',
  });

  factory ProjectInfo.fromJson(Map<String, dynamic> json) => ProjectInfo(
    id: json['id']?.toString() ?? '',
    name: json['name'] as String? ?? '',
    path: json['path'] as String? ?? '',
    description: json['description'] as String?,
    updatedAt: json['updated_at']?.toString() ?? '',
  );
}

/// `GET /api/modes` — `api.rs::ModeInfo` (custom modes included).
class ModeInfo {
  final String id;
  final String name;
  final String description;

  const ModeInfo({required this.id, required this.name, this.description = ''});

  factory ModeInfo.fromJson(Map<String, dynamic> json) => ModeInfo(
    id: json['id'] as String? ?? json['name'] as String? ?? '',
    name: json['name'] as String? ?? '',
    description: json['description'] as String? ?? '',
  );
}

/// `GET /api/mcp` entry.
class McpServerInfo {
  final String name;
  final int tools;
  final bool connected;

  const McpServerInfo({
    required this.name,
    this.tools = 0,
    this.connected = false,
  });

  factory McpServerInfo.fromJson(Map<String, dynamic> json) => McpServerInfo(
    name: json['name'] as String? ?? '',
    tools: (json['tools'] as num?)?.toInt() ?? 0,
    connected: json['connected'] as bool? ?? false,
  );
}

// ── helpers ─────────────────────────────────────────────────────────────────

List<String> _strings(Object? v) {
  if (v is List) return v.map((e) => e.toString()).toList();
  return const [];
}

/// Unwraps `{ "<key>": [...] }` or a bare list into a list of maps. Handlers
/// are inconsistent about the wrapper name, so several keys are tried.
List<Map<String, dynamic>> unwrapList(Object? body, List<String> keys) {
  Object? list = body;
  if (body is Map) {
    for (final k in keys) {
      if (body[k] is List) {
        list = body[k];
        break;
      }
    }
  }
  if (list is! List) return const [];
  return list
      .whereType<Map>()
      .map((m) => Map<String, dynamic>.from(m))
      .toList();
}

/// Like [unwrapList] but for lists of scalars (or of `{name: …}` objects).
List<String> unwrapNames(Object? body, List<String> keys) {
  Object? list = body;
  if (body is Map) {
    for (final k in keys) {
      if (body[k] is List) {
        list = body[k];
        break;
      }
    }
  }
  if (list is! List) return const [];
  return list
      .map(
        (e) =>
            e is Map ? (e['name'] ?? e['id'] ?? '').toString() : e.toString(),
      )
      .where((s) => s.isNotEmpty)
      .toList();
}
