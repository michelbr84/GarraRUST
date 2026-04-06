import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart';
import 'package:intl/intl.dart';

import '../services/api_service.dart';

/// Modern chat bubble with markdown rendering for AI messages,
/// timestamps, and avatar for assistant messages.
class ChatBubble extends StatelessWidget {
  final ChatMessage message;

  const ChatBubble({super.key, required this.message});

  bool get _isUser => message.role == 'user';

  String get _formattedTime {
    try {
      final dt = DateTime.parse(message.timestamp);
      return DateFormat('HH:mm').format(dt);
    } catch (_) {
      return '';
    }
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final maxWidth = MediaQuery.of(context).size.width * 0.78;

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        mainAxisAlignment:
            _isUser ? MainAxisAlignment.end : MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          if (!_isUser) ...[
            CircleAvatar(
              radius: 16,
              backgroundColor: cs.primaryContainer,
              child: const Icon(Icons.smart_toy_rounded, size: 18),
            ),
            const SizedBox(width: 8),
          ],
          Flexible(
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: maxWidth),
              child: Column(
                crossAxisAlignment: _isUser
                    ? CrossAxisAlignment.end
                    : CrossAxisAlignment.start,
                children: [
                  Container(
                    padding: const EdgeInsets.symmetric(
                        horizontal: 14, vertical: 10),
                    decoration: BoxDecoration(
                      color:
                          _isUser ? cs.primary : cs.surfaceContainerHighest,
                      borderRadius: BorderRadius.only(
                        topLeft: const Radius.circular(16),
                        topRight: const Radius.circular(16),
                        bottomLeft: Radius.circular(_isUser ? 16 : 4),
                        bottomRight: Radius.circular(_isUser ? 4 : 16),
                      ),
                    ),
                    child: _isUser
                        ? Text(
                            message.content,
                            style: TextStyle(
                              color: cs.onPrimary,
                              fontSize: 15,
                            ),
                          )
                        : _MarkdownBody(
                            content: message.content,
                            textColor: cs.onSurface,
                          ),
                  ),
                  const SizedBox(height: 2),
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 4),
                    child: Text(
                      _formattedTime,
                      style: TextStyle(
                        color: cs.onSurface.withValues(alpha: 0.35),
                        fontSize: 10,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (_isUser) const SizedBox(width: 8),
        ],
      ),
    );
  }
}

/// Renders markdown content for AI assistant messages.
class _MarkdownBody extends StatelessWidget {
  final String content;
  final Color textColor;

  const _MarkdownBody({required this.content, required this.textColor});

  @override
  Widget build(BuildContext context) {
    return MarkdownBody(
      data: content,
      selectable: true,
      styleSheet: MarkdownStyleSheet(
        p: TextStyle(color: textColor, fontSize: 15, height: 1.4),
        code: TextStyle(
          color: textColor,
          backgroundColor:
              Theme.of(context).colorScheme.surface.withValues(alpha: 0.3),
          fontSize: 13,
          fontFamily: 'monospace',
        ),
        codeblockDecoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surface.withValues(alpha: 0.3),
          borderRadius: BorderRadius.circular(8),
        ),
        codeblockPadding: const EdgeInsets.all(12),
        blockquoteDecoration: BoxDecoration(
          border: Border(
            left: BorderSide(
              color: Theme.of(context).colorScheme.primary,
              width: 3,
            ),
          ),
        ),
        blockquotePadding: const EdgeInsets.only(left: 12),
        h1: TextStyle(
            color: textColor, fontSize: 20, fontWeight: FontWeight.bold),
        h2: TextStyle(
            color: textColor, fontSize: 18, fontWeight: FontWeight.bold),
        h3: TextStyle(
            color: textColor, fontSize: 16, fontWeight: FontWeight.w600),
        listBullet: TextStyle(color: textColor, fontSize: 15),
        strong: TextStyle(
            color: textColor, fontWeight: FontWeight.bold),
        em: TextStyle(color: textColor, fontStyle: FontStyle.italic),
        a: TextStyle(
          color: Theme.of(context).colorScheme.primary,
          decoration: TextDecoration.underline,
        ),
      ),
    );
  }
}
