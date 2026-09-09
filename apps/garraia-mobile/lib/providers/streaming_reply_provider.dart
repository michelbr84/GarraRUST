import 'package:riverpod_annotation/riverpod_annotation.dart';

part 'streaming_reply_provider.g.dart';

/// The reply currently being streamed, and the tool it is waiting on.
///
/// It lives in its own provider for one reason: a delta arrives per token, and
/// if the growing text sat in `ChatMessages` every token would rebuild the
/// whole message list. Here only the widget watching this provider rebuilds,
/// and the list above it stays untouched until the turn ends.
class StreamingReply {
  /// Whether a turn is in flight. The chat screen watches this alone, so it
  /// rebuilds twice per turn rather than once per token.
  final bool active;

  /// Text accumulated from `delta` frames so far.
  final String text;

  /// Tool running right now, `null` between calls.
  final String? tool;

  const StreamingReply({this.active = false, this.text = '', this.tool});

  static const idle = StreamingReply();

  StreamingReply copyWith({bool? active, String? text, String? tool}) =>
      StreamingReply(
        active: active ?? this.active,
        text: text ?? this.text,
        // `copyWith(tool: null)` has to be able to clear it, which the usual
        // `??` idiom cannot express — so clearing goes through [clearTool].
        tool: tool ?? this.tool,
      );

  StreamingReply clearTool() =>
      StreamingReply(active: active, text: text, tool: null);
}

/// Generated provider name: `streamingReplyStateProvider`.
@riverpod
class StreamingReplyState extends _$StreamingReplyState {
  @override
  StreamingReply build() => StreamingReply.idle;

  /// Marks a turn as started, dropping anything left from the previous one.
  void start() => state = const StreamingReply(active: true);

  void appendDelta(String chunk) =>
      state = state.copyWith(active: true, text: state.text + chunk);

  void toolStarted(String name) => state = state.copyWith(tool: name);

  void toolFinished() => state = state.clearTool();

  /// Ends the turn. The text is dropped because by now the caller has either
  /// committed it as a real message or decided not to.
  void finish() => state = StreamingReply.idle;
}
