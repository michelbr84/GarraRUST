import 'package:flutter/material.dart';

import '../providers/chat_provider.dart';

/// Animated mascot widget with smooth state transitions.
///
/// States:
///   - idle: gentle breathing (scale pulse)
///   - thinking: spinning rotation
///   - talking: vertical bounce
///   - happy: jumping with overshoot
///
/// When the Rive file (`assets/garra_mascot.riv`) is ready, swap the body
/// to use RiveAnimation.asset() with the state machine trigger mapped to
/// [MascotState] values.
class MascotWidget extends StatefulWidget {
  final double size;
  final MascotState state;

  const MascotWidget({
    super.key,
    this.size = 96,
    this.state = MascotState.idle,
  });

  @override
  State<MascotWidget> createState() => _MascotWidgetState();
}

class _MascotWidgetState extends State<MascotWidget>
    with TickerProviderStateMixin {
  late AnimationController _breathController;
  late AnimationController _spinController;
  late AnimationController _bounceController;
  late AnimationController _jumpController;

  late Animation<double> _breathAnimation;
  late Animation<double> _spinAnimation;
  late Animation<double> _bounceAnimation;
  late Animation<double> _jumpAnimation;

  @override
  void initState() {
    super.initState();
    _initAnimations();
    _startAnimationForState(widget.state);
  }

  void _initAnimations() {
    // Idle: gentle breathing (scale 0.95 - 1.05)
    _breathController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 2000),
    );
    _breathAnimation = Tween<double>(begin: 0.95, end: 1.05).animate(
      CurvedAnimation(parent: _breathController, curve: Curves.easeInOut),
    );

    // Thinking: continuous spin
    _spinController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 1500),
    );
    _spinAnimation = Tween<double>(begin: 0, end: 1).animate(
      CurvedAnimation(parent: _spinController, curve: Curves.linear),
    );

    // Talking: vertical bounce
    _bounceController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 500),
    );
    _bounceAnimation = Tween<double>(begin: 0, end: -12).animate(
      CurvedAnimation(parent: _bounceController, curve: Curves.easeInOut),
    );

    // Happy: jump with overshoot
    _jumpController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 800),
    );
    _jumpAnimation = Tween<double>(begin: 0, end: -20).animate(
      CurvedAnimation(
        parent: _jumpController,
        curve: Curves.elasticOut,
      ),
    );
  }

  @override
  void didUpdateWidget(MascotWidget oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.state != widget.state) {
      _stopAllAnimations();
      _startAnimationForState(widget.state);
    }
  }

  void _stopAllAnimations() {
    _breathController.stop();
    _spinController.stop();
    _bounceController.stop();
    _jumpController.stop();
  }

  void _startAnimationForState(MascotState state) {
    switch (state) {
      case MascotState.idle:
        _breathController.repeat(reverse: true);
      case MascotState.thinking:
        _spinController.repeat();
      case MascotState.talking:
        _bounceController.repeat(reverse: true);
      case MascotState.happy:
        _jumpController.repeat(reverse: true);
    }
  }

  @override
  void dispose() {
    _breathController.dispose();
    _spinController.dispose();
    _bounceController.dispose();
    _jumpController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    return _MascotAnimatedBuilder(
      animation: Listenable.merge([
        _breathController,
        _spinController,
        _bounceController,
        _jumpController,
      ]),
      builder: (context) {
        Widget child = AnimatedContainer(
          duration: const Duration(milliseconds: 400),
          curve: Curves.easeOut,
          width: widget.size,
          height: widget.size,
          decoration: BoxDecoration(
            color: _bgColor(cs),
            shape: BoxShape.circle,
            boxShadow: [
              BoxShadow(
                color: cs.primary.withValues(alpha: _shadowAlpha),
                blurRadius: _shadowBlur,
                spreadRadius: _shadowSpread,
              ),
            ],
          ),
          child: Center(
            child: AnimatedSwitcher(
              duration: const Duration(milliseconds: 300),
              child: Icon(
                _icon,
                key: ValueKey(_icon),
                size: widget.size * 0.55,
                color: cs.onPrimaryContainer,
              ),
            ),
          ),
        );

        // Apply state-specific transform
        switch (widget.state) {
          case MascotState.idle:
            child = Transform.scale(
              scale: _breathAnimation.value,
              child: child,
            );
          case MascotState.thinking:
            child = Transform.rotate(
              angle: _spinAnimation.value * 2 * 3.14159,
              child: child,
            );
          case MascotState.talking:
            child = Transform.translate(
              offset: Offset(0, _bounceAnimation.value),
              child: child,
            );
          case MascotState.happy:
            child = Transform.translate(
              offset: Offset(0, _jumpAnimation.value),
              child: child,
            );
        }

        return child;
      },
    );
  }

  double get _shadowAlpha => switch (widget.state) {
        MascotState.idle => 0.3,
        MascotState.thinking => 0.4,
        MascotState.talking => 0.5,
        MascotState.happy => 0.6,
      };

  double get _shadowBlur => switch (widget.state) {
        MascotState.idle => 8,
        MascotState.thinking => 16,
        MascotState.talking => 20,
        MascotState.happy => 24,
      };

  double get _shadowSpread => switch (widget.state) {
        MascotState.idle => 0,
        MascotState.thinking => 2,
        MascotState.talking => 4,
        MascotState.happy => 6,
      };

  Color _bgColor(ColorScheme cs) => switch (widget.state) {
        MascotState.idle => cs.primaryContainer,
        MascotState.thinking => cs.secondaryContainer,
        MascotState.talking => cs.tertiaryContainer,
        MascotState.happy => cs.primaryContainer,
      };

  IconData get _icon => switch (widget.state) {
        MascotState.idle => Icons.smart_toy_rounded,
        MascotState.thinking => Icons.psychology_rounded,
        MascotState.talking => Icons.record_voice_over_rounded,
        MascotState.happy => Icons.sentiment_very_satisfied_rounded,
      };
}

/// Core AnimatedWidget subclass for the mascot animations.
class _MascotAnimatedBuilder extends AnimatedWidget {
  final Widget Function(BuildContext context) builder;

  const _MascotAnimatedBuilder({
    required Listenable animation,
    required this.builder,
  }) : super(listenable: animation);

  @override
  Widget build(BuildContext context) => builder(context);
}
