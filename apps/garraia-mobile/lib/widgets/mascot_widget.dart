import 'package:flutter/material.dart';

import '../providers/chat_provider.dart';

/// Placeholder mascot widget.
///
/// When the Rive file (`assets/garra_mascot.riv`) is ready, swap the body
/// to use RiveAnimation.asset() with the state machine trigger mapped to
/// [MascotState] values.
class MascotWidget extends StatelessWidget {
  final double size;
  final MascotState state;

  const MascotWidget({
    super.key,
    this.size = 96,
    this.state = MascotState.idle,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return AnimatedContainer(
      duration: const Duration(milliseconds: 300),
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: _bgColor(cs),
        shape: BoxShape.circle,
        boxShadow: [
          BoxShadow(
            color: cs.primary.withValues(alpha: 0.3),
            blurRadius: state == MascotState.idle ? 8 : 20,
            spreadRadius: state == MascotState.talking ? 4 : 0,
          ),
        ],
      ),
      child: Center(
        child: Icon(_icon, size: size * 0.55, color: cs.onPrimaryContainer),
      ),
    );
  }

  Color _bgColor(ColorScheme cs) => switch (state) {
        MascotState.idle => cs.primaryContainer,
        MascotState.thinking => cs.secondaryContainer,
        MascotState.talking => cs.tertiaryContainer,
        MascotState.happy => cs.primaryContainer,
      };

  IconData get _icon => switch (state) {
        MascotState.idle => Icons.smart_toy_rounded,
        MascotState.thinking => Icons.psychology_rounded,
        MascotState.talking => Icons.record_voice_over_rounded,
        MascotState.happy => Icons.sentiment_very_satisfied_rounded,
      };
}
