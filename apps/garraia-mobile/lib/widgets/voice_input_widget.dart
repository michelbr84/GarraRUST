import 'dart:async';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:record/record.dart';

/// Hold-to-record voice input widget with waveform visualization.
///
/// The widget sends recorded audio to the backend /voice endpoint via
/// the provided [onAudioRecorded] callback, which should return the
/// transcription text.
class VoiceInputWidget extends StatefulWidget {
  /// Called when audio recording is complete.
  /// Should send to backend and return the transcription text.
  final Future<String> Function(String audioPath) onAudioRecorded;

  /// Called with the transcription result.
  final void Function(String transcription) onTranscription;

  const VoiceInputWidget({
    super.key,
    required this.onAudioRecorded,
    required this.onTranscription,
  });

  @override
  State<VoiceInputWidget> createState() => _VoiceInputWidgetState();
}

class _VoiceInputWidgetState extends State<VoiceInputWidget>
    with SingleTickerProviderStateMixin {
  final AudioRecorder _recorder = AudioRecorder();
  bool _isRecording = false;
  bool _isProcessing = false;
  String? _error;
  Duration _recordDuration = Duration.zero;
  Timer? _durationTimer;
  Timer? _amplitudeTimer;
  List<double> _amplitudes = [];

  late AnimationController _pulseController;
  late Animation<double> _pulseAnimation;

  @override
  void initState() {
    super.initState();
    _pulseController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 1000),
    );
    _pulseAnimation = Tween<double>(begin: 1.0, end: 1.3).animate(
      CurvedAnimation(parent: _pulseController, curve: Curves.easeInOut),
    );
  }

  @override
  void dispose() {
    _pulseController.dispose();
    _durationTimer?.cancel();
    _amplitudeTimer?.cancel();
    _recorder.dispose();
    super.dispose();
  }

  Future<void> _startRecording() async {
    try {
      if (!await _recorder.hasPermission()) {
        setState(() => _error = 'Permissao de microfone necessaria');
        return;
      }

      await _recorder.start(
        const RecordConfig(
          encoder: AudioEncoder.aacLc,
          bitRate: 128000,
          sampleRate: 44100,
        ),
        path: '', // Use default temp path
      );

      setState(() {
        _isRecording = true;
        _error = null;
        _recordDuration = Duration.zero;
        _amplitudes = [];
      });

      _pulseController.repeat(reverse: true);

      // Track duration
      _durationTimer = Timer.periodic(const Duration(seconds: 1), (_) {
        if (mounted) {
          setState(() {
            _recordDuration += const Duration(seconds: 1);
          });
        }
      });

      // Track amplitude for waveform
      _amplitudeTimer =
          Timer.periodic(const Duration(milliseconds: 100), (_) async {
        final amplitude = await _recorder.getAmplitude();
        if (mounted && _isRecording) {
          setState(() {
            // Normalize amplitude from dB (-160 to 0) to 0.0-1.0
            final normalized =
                ((amplitude.current + 60) / 60).clamp(0.0, 1.0);
            _amplitudes.add(normalized);
            // Keep only last 50 samples for display
            if (_amplitudes.length > 50) {
              _amplitudes = _amplitudes.sublist(_amplitudes.length - 50);
            }
          });
        }
      });
    } catch (e) {
      setState(() => _error = 'Erro ao iniciar gravacao: $e');
    }
  }

  Future<void> _stopRecording() async {
    _durationTimer?.cancel();
    _amplitudeTimer?.cancel();
    _pulseController.stop();
    _pulseController.reset();

    if (!_isRecording) return;

    try {
      final path = await _recorder.stop();
      setState(() {
        _isRecording = false;
        _isProcessing = true;
      });

      if (path != null) {
        final transcription = await widget.onAudioRecorded(path);
        widget.onTranscription(transcription);
      }
    } catch (e) {
      setState(() => _error = 'Erro ao processar audio: $e');
    } finally {
      if (mounted) {
        setState(() => _isProcessing = false);
      }
    }
  }

  String _formatDuration(Duration d) {
    final minutes = d.inMinutes.remainder(60).toString().padLeft(2, '0');
    final seconds = d.inSeconds.remainder(60).toString().padLeft(2, '0');
    return '$minutes:$seconds';
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    if (_isProcessing) {
      return Container(
        padding: const EdgeInsets.all(12),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(
              width: 20,
              height: 20,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                color: cs.primary,
              ),
            ),
            const SizedBox(width: 8),
            Text(
              'Transcrevendo...',
              style: TextStyle(
                color: cs.onSurface.withValues(alpha: 0.7),
                fontSize: 13,
              ),
            ),
          ],
        ),
      );
    }

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(bottom: 4),
            child: Text(
              _error!,
              style: TextStyle(color: cs.error, fontSize: 12),
            ),
          ),
        if (_isRecording) ...[
          // Waveform visualization
          Container(
            height: 40,
            padding: const EdgeInsets.symmetric(horizontal: 8),
            child: CustomPaint(
              size: const Size(double.infinity, 40),
              painter: _WaveformPainter(
                amplitudes: _amplitudes,
                color: cs.primary,
              ),
            ),
          ),
          const SizedBox(height: 4),
          Text(
            _formatDuration(_recordDuration),
            style: TextStyle(
              color: cs.error,
              fontSize: 12,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 4),
        ],
        // Record button
        GestureDetector(
          onLongPressStart: (_) => _startRecording(),
          onLongPressEnd: (_) => _stopRecording(),
          child: _VoiceAnimatedBuilder(
            animation: _pulseAnimation,
            builder: (context, child) {
              return Transform.scale(
                scale: _isRecording ? _pulseAnimation.value : 1.0,
                child: Container(
                  width: 44,
                  height: 44,
                  decoration: BoxDecoration(
                    color: _isRecording
                        ? cs.error
                        : cs.surfaceContainerHighest,
                    shape: BoxShape.circle,
                  ),
                  child: Icon(
                    _isRecording ? Icons.stop_rounded : Icons.mic_rounded,
                    color: _isRecording ? cs.onError : cs.onSurface,
                    size: 22,
                  ),
                ),
              );
            },
          ),
        ),
        if (!_isRecording)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Text(
              'Segure para gravar',
              style: TextStyle(
                color: cs.onSurface.withValues(alpha: 0.4),
                fontSize: 10,
              ),
            ),
          ),
      ],
    );
  }
}

/// Custom painter for waveform visualization during recording.
class _WaveformPainter extends CustomPainter {
  final List<double> amplitudes;
  final Color color;

  _WaveformPainter({required this.amplitudes, required this.color});

  @override
  void paint(Canvas canvas, Size size) {
    if (amplitudes.isEmpty) return;

    final paint = Paint()
      ..color = color
      ..strokeWidth = 2.5
      ..strokeCap = StrokeCap.round;

    final barWidth = size.width / max(amplitudes.length, 1);
    final centerY = size.height / 2;

    for (int i = 0; i < amplitudes.length; i++) {
      final x = i * barWidth + barWidth / 2;
      final barHeight = max(amplitudes[i] * size.height * 0.8, 2.0);
      canvas.drawLine(
        Offset(x, centerY - barHeight / 2),
        Offset(x, centerY + barHeight / 2),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _WaveformPainter oldDelegate) =>
      oldDelegate.amplitudes.length != amplitudes.length ||
      (amplitudes.isNotEmpty &&
          oldDelegate.amplitudes.isNotEmpty &&
          amplitudes.last != oldDelegate.amplitudes.last);
}

/// Helper to use AnimatedWidget with builder pattern for voice input.
class _VoiceAnimatedBuilder extends AnimatedWidget {
  final Widget Function(BuildContext context, Widget? child) builder;

  const _VoiceAnimatedBuilder({
    required Animation<double> animation,
    required this.builder,
  }) : super(listenable: animation);

  @override
  Widget build(BuildContext context) => builder(context, null);
}
