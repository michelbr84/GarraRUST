import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../providers/streaming_reply_provider.dart';
import 'brand/wolf_mark.dart';
import 'typing_indicator.dart';

/// The assistant bubble while the reply is still arriving.
///
/// It watches [streamingReplyStateProvider] itself instead of taking the text
/// as a parameter. That is the point: the chat screen only watches whether a
/// turn is active, so a delta rebuilds this widget and nothing else — the
/// message list above stays put, token after token.
///
/// The text renders as plain text, not markdown, on purpose. Half-written
/// markdown reflows on every token (an unclosed fence turns the rest of the
/// reply into a code block until it closes), and parsing per token is work
/// thrown away. The finished message becomes a real ChatBubble with markdown
/// once the turn ends.
class StreamingBubble extends ConsumerWidget {
  const StreamingBubble({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final reply = ref.watch(streamingReplyStateProvider);
    final cs = Theme.of(context).colorScheme;
    final maxWidth = MediaQuery.of(context).size.width * 0.78;

    // Nothing has arrived yet: the runtime is thinking, or a tool is running.
    if (reply.text.isEmpty && reply.tool == null) {
      return const TypingIndicator();
    }

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          const SizedBox(
            width: 32,
            height: 32,
            child: WolfMark(size: 32, glow: 0.4),
          ),
          const SizedBox(width: 8),
          Flexible(
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: maxWidth),
              child: Container(
                key: const ValueKey('streaming-bubble'),
                padding: const EdgeInsets.symmetric(
                  horizontal: 14,
                  vertical: 10,
                ),
                decoration: BoxDecoration(
                  color: cs.surfaceContainerHighest,
                  borderRadius: BorderRadius.circular(16),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (reply.tool != null)
                      Padding(
                        padding: const EdgeInsets.only(bottom: 6),
                        child: _ToolChip(name: reply.tool!),
                      ),
                    if (reply.text.isNotEmpty)
                      Text(reply.text, style: TextStyle(color: cs.onSurface)),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// "rodando <tool>" while a tool call is open.
class _ToolChip extends StatelessWidget {
  final String name;

  const _ToolChip({required this.name});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        SizedBox(
          width: 12,
          height: 12,
          child: CircularProgressIndicator(strokeWidth: 2, color: cs.primary),
        ),
        const SizedBox(width: 8),
        Text(
          'rodando $name',
          key: const ValueKey('streaming-tool'),
          style: TextStyle(
            fontSize: 12,
            color: cs.onSurfaceVariant,
            fontStyle: FontStyle.italic,
          ),
        ),
      ],
    );
  }
}
