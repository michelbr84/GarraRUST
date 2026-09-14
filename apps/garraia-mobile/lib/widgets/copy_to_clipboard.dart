import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../l10n/l10n.dart';

/// Copies [text] and confirms with a short SnackBar.
///
/// One place for the gesture so every "copy" in the app — a chat bubble, a
/// code block, a memory, the runtime log — feels the same and is tested the
/// same way (the widget tests mock `Clipboard.setData`).
///
/// [toast] defaults to the localized "Copied" (a default value must be const,
/// so the lookup happens in the body, not in the signature).
Future<void> copyToClipboard(
  BuildContext context,
  String text, {
  String? toast,
}) async {
  await Clipboard.setData(ClipboardData(text: text));
  if (!context.mounted) return;
  final message = toast ?? context.l10n.commonCopied;
  ScaffoldMessenger.maybeOf(context)
    ?..hideCurrentSnackBar()
    ..showSnackBar(
      SnackBar(content: Text(message), duration: const Duration(seconds: 1)),
    );
}

/// The small copy affordance used next to timestamps and on code blocks.
class CopyIconButton extends StatelessWidget {
  final String text;
  final String tooltip;
  final String toast;
  final Color? color;
  final double size;

  const CopyIconButton({
    super.key,
    required this.text,
    required this.tooltip,
    required this.toast,
    this.color,
    this.size = 14,
  });

  @override
  Widget build(BuildContext context) {
    return IconButton(
      padding: EdgeInsets.zero,
      constraints: BoxConstraints(minWidth: size + 12, minHeight: size + 8),
      iconSize: size,
      visualDensity: VisualDensity.compact,
      tooltip: tooltip,
      icon: Icon(Icons.copy_rounded, color: color),
      onPressed: () => copyToClipboard(context, text, toast: toast),
    );
  }
}
