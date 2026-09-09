import 'dart:convert';

import 'package:web_socket_channel/web_socket_channel.dart';

/// The slice of a WebSocket that streaming chat actually needs.
///
/// Narrow on purpose: a test can implement it with a `StreamController` and no
/// socket at all, which is the only way `flutter test` can exercise the turn
/// protocol — deltas, an unknown frame, a cancel — without a running gateway.
abstract interface class ChatSocket {
  /// Text frames from the server, in arrival order.
  Stream<String> get incoming;

  /// Completes when the connection is usable, throws when it never opened.
  Future<void> get ready;

  /// Sends one text frame.
  void send(String frame);

  Future<void> close();
}

/// Builds a socket for [uri]. Injected so tests can swap the transport.
typedef ChatSocketFactory = ChatSocket Function(Uri uri);

/// [ChatSocket] over a real `WebSocketChannel`.
class WebSocketChatSocket implements ChatSocket {
  final WebSocketChannel _channel;

  WebSocketChatSocket(this._channel);

  factory WebSocketChatSocket.connect(Uri uri) =>
      WebSocketChatSocket(WebSocketChannel.connect(uri));

  @override
  Stream<String> get incoming => _channel.stream.map((frame) {
    if (frame is String) return frame;
    if (frame is List<int>) return utf8.decode(frame, allowMalformed: true);
    return '';
  });

  @override
  Future<void> get ready => _channel.ready;

  @override
  void send(String frame) => _channel.sink.add(frame);

  @override
  Future<void> close() => _channel.sink.close();
}

/// Default factory: a real WebSocket.
ChatSocket connectWebSocket(Uri uri) => WebSocketChatSocket.connect(uri);

/// Turns an `http(s)://host/base` into the `ws(s)://host/base/ws` the gateway
/// serves, carrying the gateway key as `?token=` when there is one.
///
/// The key travels in the query because `WebSocketChannel.connect` cannot set
/// headers on the web target; `ws_handler` accepts either
/// (`crates/garraia-gateway/src/ws.rs`).
Uri chatSocketUri(String baseUrl, {String? apiKey}) {
  final base = Uri.parse(baseUrl);
  final scheme = base.scheme == 'https' ? 'wss' : 'ws';
  final basePath = base.path.endsWith('/')
      ? base.path.substring(0, base.path.length - 1)
      : base.path;
  final query = <String, String>{
    ...base.queryParameters,
    if (apiKey != null && apiKey.isNotEmpty) 'token': apiKey,
  };
  // A `null` here keeps the base query (which is empty whenever the map is),
  // where an empty map would append a bare `?`.
  return base.replace(
    scheme: scheme,
    path: '$basePath/ws',
    queryParameters: query.isEmpty ? null : query,
  );
}
