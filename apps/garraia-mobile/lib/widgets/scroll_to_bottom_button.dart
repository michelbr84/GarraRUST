import 'package:flutter/material.dart';

/// Floating button that appears when the user scrolls up in the chat,
/// allowing them to quickly jump to the latest message.
class ScrollToBottomButton extends StatelessWidget {
  final VoidCallback onPressed;
  final int unreadCount;

  const ScrollToBottomButton({
    super.key,
    required this.onPressed,
    this.unreadCount = 0,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    return Positioned(
      bottom: 8,
      right: 16,
      child: Material(
        color: cs.surfaceContainerHighest,
        elevation: 4,
        shape: const CircleBorder(),
        child: InkWell(
          onTap: onPressed,
          customBorder: const CircleBorder(),
          child: SizedBox(
            width: 40,
            height: 40,
            child: Stack(
              alignment: Alignment.center,
              children: [
                Icon(
                  Icons.keyboard_arrow_down_rounded,
                  color: cs.onSurface,
                  size: 24,
                ),
                if (unreadCount > 0)
                  Positioned(
                    top: 2,
                    right: 2,
                    child: Container(
                      padding: const EdgeInsets.all(4),
                      decoration: BoxDecoration(
                        color: cs.primary,
                        shape: BoxShape.circle,
                      ),
                      child: Text(
                        unreadCount > 9 ? '9+' : '$unreadCount',
                        style: TextStyle(
                          color: cs.onPrimary,
                          fontSize: 9,
                          fontWeight: FontWeight.bold,
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
