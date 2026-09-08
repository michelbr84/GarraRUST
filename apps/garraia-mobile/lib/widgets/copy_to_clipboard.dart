import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// Copies [text] and confirms with a short SnackBar.
///
/// One place for the gesture so every "copy" in the app — a chat bubble, a
/// code block, a memory, the runtime log — feels the same and is tested the
/// same way (the widget tests mock `Clipboard.setData`).
Future<void> copyToClipboard(
  BuildContext context,
  String text, {
  String toast = 'Copiado',
}) async {
  await Clipboard.setData(ClipboardData(text: text));
  if (!context.mounted) return;
  ScaffoldMessenger.maybeOf(context)
    ?..hideCurrentSnackBar()
    ..showSnackBar(
      SnackBar(content: Text(toast), duration: const Duration(seconds: 1)),
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
