import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart';
import 'package:intl/intl.dart';
import 'package:markdown/markdown.dart' as md;

import '../runtime/models.dart';
import 'brand/wolf_mark.dart';
import 'copy_to_clipboard.dart';

/// Modern chat bubble with markdown rendering for AI messages,
/// timestamps, and avatar for assistant messages.
///
/// Copying: every bubble carries a small copy button next to its timestamp
/// (`copy-message`), both texts are selectable for partial copies, and a
/// fenced code block gets its own button (`copy-code`) that copies just the
/// code. Field report from v0.4.0: long-press selection existed on the
/// assistant side only and nothing hinted at it; the user's own messages
/// could not be copied at all.
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
        mainAxisAlignment: _isUser
            ? MainAxisAlignment.end
            : MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          if (!_isUser) ...[
            const SizedBox(
              width: 32,
              height: 32,
              child: WolfMark(size: 32, glow: 0.4),
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
                      horizontal: 14,
                      vertical: 10,
                    ),
                    decoration: BoxDecoration(
                      color: _isUser ? cs.primary : cs.surfaceContainerHighest,
                      borderRadius: BorderRadius.only(
                        topLeft: const Radius.circular(16),
                        topRight: const Radius.circular(16),
                        bottomLeft: Radius.circular(_isUser ? 16 : 4),
                        bottomRight: Radius.circular(_isUser ? 4 : 16),
                      ),
                    ),
                    child: _isUser
                        ? SelectableText(
                            message.content,
                            style: TextStyle(color: cs.onPrimary, fontSize: 15),
                          )
                        : _MarkdownBody(
                            content: message.content,
                            textColor: cs.onSurface,
                          ),
                  ),
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 4),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          _formattedTime,
                          style: TextStyle(
                            color: cs.onSurface.withValues(alpha: 0.35),
                            fontSize: 10,
                          ),
                        ),
                        CopyIconButton(
                          key: const ValueKey('copy-message'),
                          text: message.content,
                          tooltip: 'Copiar mensagem',
                          toast: 'Mensagem copiada',
                          color: cs.onSurface.withValues(alpha: 0.45),
                        ),
                      ],
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
      builders: {'code': _CodeBlockBuilder(textColor: textColor)},
      styleSheet: MarkdownStyleSheet(
        p: TextStyle(color: textColor, fontSize: 15, height: 1.4),
        code: TextStyle(
          color: textColor,
          backgroundColor: Theme.of(
            context,
          ).colorScheme.surface.withValues(alpha: 0.3),
          fontSize: 13,
          fontFamily: 'JetBrainsMono',
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
          color: textColor,
          fontSize: 20,
          fontWeight: FontWeight.bold,
        ),
        h2: TextStyle(
          color: textColor,
          fontSize: 18,
          fontWeight: FontWeight.bold,
        ),
        h3: TextStyle(
          color: textColor,
          fontSize: 16,
          fontWeight: FontWeight.w600,
        ),
        listBullet: TextStyle(color: textColor, fontSize: 15),
        strong: TextStyle(color: textColor, fontWeight: FontWeight.bold),
        em: TextStyle(color: textColor, fontStyle: FontStyle.italic),
        a: TextStyle(
          color: Theme.of(context).colorScheme.primary,
          decoration: TextDecoration.underline,
        ),
      ),
    );
  }
}

/// Fenced code blocks with a copy button.
///
/// Registered on the `code` tag: inline code (no newline) returns `null` and
/// keeps the default rendering; a block replaces the default scrollable text
/// with the same text plus a button that copies **only the code**, which is
/// what someone on a phone wants from a snippet the model produced. The
/// `pre` frame (`codeblockDecoration`) still wraps it.
class _CodeBlockBuilder extends MarkdownElementBuilder {
  final Color textColor;

  _CodeBlockBuilder({required this.textColor});

  @override
  Widget? visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    final raw = element.textContent;
    if (!raw.contains('\n')) return null;
    final code = raw.trimRight();
    return Stack(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 12, 40, 12),
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: SelectableText(
              code,
              style: TextStyle(
                color: textColor,
                fontFamily: 'JetBrainsMono',
                fontSize: 13,
                height: 1.45,
              ),
            ),
          ),
        ),
        Positioned(
          top: 2,
          right: 2,
          child: CopyIconButton(
            key: const ValueKey('copy-code'),
            text: code,
            tooltip: 'Copiar codigo',
            toast: 'Codigo copiado',
            color: textColor.withValues(alpha: 0.6),
            size: 16,
          ),
        ),
      ],
    );
  }
}
