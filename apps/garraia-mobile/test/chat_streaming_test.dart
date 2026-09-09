import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/chat_event.dart';
import 'package:garraia_mobile/runtime/chat_socket.dart';
import 'package:garraia_mobile/runtime/gateway_connection.dart';
import 'package:garraia_mobile/runtime/runtime_config.dart';

/// A socket with no socket in it: frames go in and out of a controller, so the
/// turn protocol can be exercised without a gateway.
class _FakeSocket implements ChatSocket {
  final _incoming = StreamController<String>();

  /// Every frame the client sent, in order.
  final sent = <String>[];

  final bool failToOpen;
  bool closed = false;

  _FakeSocket({this.failToOpen = false});

  @override
  Stream<String> get incoming => _incoming.stream;

  @override
  Future<void> get ready =>
      failToOpen ? Future.error(StateError('upgrade recusado')) : Future.value();

  @override
  void send(String frame) => sent.add(frame);

  @override
  Future<void> close() async {
    closed = true;
    if (!_incoming.isClosed) await _incoming.close();
  }

  void emit(String frame) => _incoming.add(frame);

  /// The server hanging up without finishing the turn.
  Future<void> hangUp() => _incoming.close();

  Map<String, dynamic> sentFrame(int index) =>
      jsonDecode(sent[index]) as Map<String, dynamic>;
}

/// A gateway whose POST path is a counter, so the fallback is observable
/// without an HTTP server.
class _TestGateway extends GatewayConnection {
  int posts = 0;
  String? lastPostSession;

  _TestGateway(ChatSocketFactory factory)
    : super(
        mode: RuntimeMode.local,
        baseUrl: 'http://127.0.0.1:3888',
        socketFactory: factory,
      );

  @override
  Future<String> sendMessage(String text, {String? sessionId}) async {
    posts++;
    lastPostSession = sessionId;
    return 'resposta via POST';
  }
}

/// Lets the turn generator advance: it awaits between frames, so a single
/// microtask hop is not enough. Local on purpose — one less framework API to
/// be wrong about.
Future<void> settle() async {
  for (var i = 0; i < 10; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

void main() {
  group('ChatEvent.tryParse', () {
    test('reads every frame the gateway documents', () {
      expect(
        (ChatEvent.tryParse('{"type":"delta","content":"oi"}') as ChatDelta)
            .content,
        'oi',
      );
      expect(
        (ChatEvent.tryParse('{"type":"tool_started","name":"bash"}')
                as ChatToolStarted)
            .name,
        'bash',
      );
      expect(
        ChatEvent.tryParse(
          '{"type":"tool_finished","name":"bash","success":true}',
        ),
        isA<ChatToolFinished>(),
      );
      expect(
        (ChatEvent.tryParse('{"type":"message","content":"pronto"}')
                as ChatCompleted)
            .content,
        'pronto',
      );
      expect(ChatEvent.tryParse('{"type":"stopped"}'), isA<ChatStopped>());
      expect(
        ChatEvent.tryParse('{"type":"error","message":"deu ruim"}'),
        isA<ChatFailed>(),
      );
    });

    test('an unknown frame is null, not an exception', () {
      // The gateway may grow a frame type before the app learns it.
      expect(ChatEvent.tryParse('{"type":"telemetry","v":1}'), isNull);
      expect(ChatEvent.tryParse('{"type":"connected","session_id":"s1"}'),
          isNull);
      expect(ChatEvent.tryParse('nao e json'), isNull);
      expect(ChatEvent.tryParse('[1,2,3]'), isNull);
      expect(ChatEvent.tryParse('{"sem":"tipo"}'), isNull);
      // A number where a string belongs is malformed, not a cast to force.
      expect(ChatEvent.tryParse('{"type":"delta","content":7}'), isNull);
    });
  });

  group('chatSocketUri', () {
    test('http becomes ws and https becomes wss', () {
      expect(
        chatSocketUri('http://127.0.0.1:3888').toString(),
        'ws://127.0.0.1:3888/ws',
      );
      expect(
        chatSocketUri('https://garra.example.com').toString(),
        'wss://garra.example.com/ws',
      );
    });

    test('carries the gateway key, which cannot go in a header on web', () {
      expect(
        chatSocketUri('http://127.0.0.1:3888', apiKey: 'k1').toString(),
        'ws://127.0.0.1:3888/ws?token=k1',
      );
    });

    test('a trailing slash does not produce //ws', () {
      expect(
        chatSocketUri('http://127.0.0.1:3888/').toString(),
        'ws://127.0.0.1:3888/ws',
      );
    });
  });

  group('sendMessageStreaming', () {
    test('resumes the session, submits, and streams the turn', () async {
      final socket = _FakeSocket();
      final conn = _TestGateway((_) => socket);

      final events = <ChatEvent>[];
      final done = conn
          .sendMessageStreaming('oi', sessionId: 's1')
          .listen(events.add)
          .asFuture<void>();

      await settle();
      // First frame is the resume for the session the app already has, so the
      // turn lands in the same history the HTTP path reads.
      expect(socket.sentFrame(0)['type'], 'resume');
      expect(socket.sentFrame(0)['session_id'], 's1');
      expect(socket.sent.length, 1, reason: 'nothing submitted before the ack');

      socket.emit('{"type":"resumed","session_id":"s1"}');
      await settle();
      expect(socket.sentFrame(1)['content'], 'oi');

      socket.emit('{"type":"delta","content":"oi "}');
      socket.emit('{"type":"telemetry","ignore":true}');
      socket.emit('{"type":"delta","content":"mundo"}');
      socket.emit('{"type":"message","content":"oi mundo"}');
      await done;

      expect(events.whereType<ChatDelta>().map((e) => e.content).toList(), [
        'oi ',
        'mundo',
      ]);
      expect((events.last as ChatCompleted).content, 'oi mundo');
      expect(conn.posts, 0, reason: 'the socket carried it, not POST');
      expect(socket.closed, isTrue);
    });

    test('an unknown frame does not take the stream down', () async {
      final socket = _FakeSocket();
      final conn = _TestGateway((_) => socket);
      final events = <ChatEvent>[];
      final done = conn
          .sendMessageStreaming('oi', sessionId: 's1')
          .listen(events.add)
          .asFuture<void>();

      await settle();
      socket.emit('{"type":"resumed","session_id":"s1"}');
      await settle();

      socket.emit('{"type":"quem_sabe_o_que","x":1}');
      socket.emit('{"type":"delta","content":"segue"}');
      socket.emit('{"type":"message","content":"segue"}');
      await done;

      expect(events.length, 2, reason: 'the stranger was skipped: $events');
      expect(events.first, isA<ChatDelta>());
    });

    test('a cancelled turn ends with stopped, keeping what arrived', () async {
      final socket = _FakeSocket();
      final conn = _TestGateway((_) => socket);
      final events = <ChatEvent>[];
      final done = conn
          .sendMessageStreaming('oi', sessionId: 's1')
          .listen(events.add)
          .asFuture<void>();

      await settle();
      socket.emit('{"type":"resumed","session_id":"s1"}');
      await settle();
      socket.emit('{"type":"delta","content":"metade"}');
      await settle();

      await conn.stopStreaming();
      // Always addressed: an unaddressed stop would cancel whatever turn the
      // socket happens to be running.
      final stop = socket.sentFrame(2);
      expect(stop['type'], 'stop');
      expect(stop['session_id'], 's1');

      socket.emit('{"type":"stopped","session_id":"s1"}');
      await done;

      expect(events.first, isA<ChatDelta>());
      expect(events.last, isA<ChatStopped>());
    });

    test('a rotated session is reported so the app can follow', () async {
      // A resume the gateway cannot honour is not refused: it answers
      // `connected` with a different session.
      final socket = _FakeSocket();
      final conn = _TestGateway((_) => socket);
      final events = <ChatEvent>[];
      final done = conn
          .sendMessageStreaming('oi', sessionId: 'expirada')
          .listen(events.add)
          .asFuture<void>();

      await settle();
      socket.emit(
        '{"type":"connected","session_id":"nova","note":"previous session expired"}',
      );
      await settle();
      socket.emit('{"type":"message","content":"pronto"}');
      await done;

      expect((events.first as ChatSessionChanged).sessionId, 'nova');
      expect(socket.sentFrame(1)['content'], 'oi');
    });

    test('falls back to POST when the socket never opens', () async {
      // An older gateway with no /ws, a proxy refusing the upgrade, the LAN
      // dropping: the user gets a reply, not an error.
      final socket = _FakeSocket(failToOpen: true);
      final conn = _TestGateway((_) => socket);

      final events = await conn
          .sendMessageStreaming('oi', sessionId: 's1')
          .toList();

      expect(conn.posts, 1);
      expect(conn.lastPostSession, 's1');
      expect((events.single as ChatCompleted).content, 'resposta via POST');
    });

    test('falls back when the socket dies before the turn is submitted',
        () async {
      final socket = _FakeSocket();
      final conn = _TestGateway((_) => socket);

      final pending = conn.sendMessageStreaming('oi', sessionId: 's1').toList();
      await settle();
      await socket.hangUp(); // hung up during the handshake

      final events = await pending;
      expect(conn.posts, 1, reason: 'nothing was submitted, so POST is safe');
      expect((events.single as ChatCompleted).content, 'resposta via POST');
    });

    test('does not resend over POST once the turn was submitted', () async {
      // The message is already with the runtime; retrying would send it twice.
      final socket = _FakeSocket();
      final conn = _TestGateway((_) => socket);
      final events = <ChatEvent>[];
      final done = conn
          .sendMessageStreaming('oi', sessionId: 's1')
          .listen(events.add)
          .asFuture<void>();

      await settle();
      socket.emit('{"type":"resumed","session_id":"s1"}');
      await settle();
      socket.emit('{"type":"delta","content":"metade"}');
      await settle();
      await socket.hangUp();
      await done;

      expect(conn.posts, 0);
      expect(events.last, isA<ChatFailed>());
    });

    test('stopStreaming is a no-op with no turn in flight', () async {
      final socket = _FakeSocket();
      final conn = _TestGateway((_) => socket);
      await conn.stopStreaming();
      expect(socket.sent, isEmpty);
    });

    test('a session id is required in gateway mode', () async {
      final conn = _TestGateway((_) => _FakeSocket());
      // `async*` reports it when the stream is listened to, not at the call.
      await expectLater(
        conn.sendMessageStreaming('oi').toList(),
        throwsArgumentError,
      );
    });
  });
}
