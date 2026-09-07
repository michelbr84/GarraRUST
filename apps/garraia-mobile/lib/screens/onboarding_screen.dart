import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../runtime/gateway_connection.dart';
import '../runtime/models.dart';
import '../runtime/runtime_config.dart';
import '../runtime/runtime_providers.dart';
import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/brand/wolf_mark.dart';

/// First-run flow (also reachable later as "Pair PC" / "Change runtime"):
///   1. your name (greeting only, stays on the device)
///   2. where Garra runs — this phone (Termux), another Garra on the LAN, or
///      Garra Cloud — with a live connection test against `/api/health`
///   3. done
///
/// No Kotlin bridge yet (ADR 0016 v1 is Flutter-first): for "On this phone"
/// the screen explains the one-line Termux install and checks that the
/// gateway answers on loopback.
class OnboardingScreen extends ConsumerStatefulWidget {
  const OnboardingScreen({super.key});

  @override
  ConsumerState<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends ConsumerState<OnboardingScreen> {
  final _page = PageController();
  final _name = TextEditingController();
  final _url = TextEditingController();
  final _apiKey = TextEditingController();
  RuntimeMode _mode = RuntimeMode.local;
  int _step = 0;
  bool _testing = false;
  bool _saving = false;
  GarraHealth? _probe;
  String? _probeError;

  @override
  void initState() {
    super.initState();
    final existing = ref.read(runtimeConfigStateProvider).value;
    if (existing != null) {
      _mode = existing.mode;
      _name.text = existing.ownerName;
      _url.text = existing.baseUrl;
    } else {
      _url.text = _mode.defaultBaseUrl;
    }
  }

  @override
  void dispose() {
    _page.dispose();
    _name.dispose();
    _url.dispose();
    _apiKey.dispose();
    super.dispose();
  }

  void _go(int step) {
    setState(() => _step = step);
    _page.animateToPage(
      step,
      duration: const Duration(milliseconds: 260),
      curve: Curves.easeOutCubic,
    );
  }

  void _pickMode(RuntimeMode m) {
    setState(() {
      _mode = m;
      _url.text = m.defaultBaseUrl;
      _probe = null;
      _probeError = null;
    });
  }

  Future<void> _test() async {
    if (!RuntimeConfig.isValidBaseUrl(_url.text)) {
      setState(() => _probeError = 'Enter a valid http(s) address');
      return;
    }
    setState(() {
      _testing = true;
      _probe = null;
      _probeError = null;
    });
    try {
      final conn = GatewayConnection(
        mode: _mode,
        baseUrl: RuntimeConfig.normalizeBaseUrl(_url.text),
        apiKey: _apiKey.text.trim().isEmpty ? null : _apiKey.text.trim(),
      );
      final h = await conn.health();
      if (mounted) setState(() => _probe = h);
    } on DioException catch (e) {
      // The probe outlives the screen if the user backs out mid-request.
      if (mounted) setState(() => _probeError = _explain(e));
    } catch (e) {
      if (mounted) setState(() => _probeError = e.toString());
    } finally {
      if (mounted) setState(() => _testing = false);
    }
  }

  String _explain(DioException e) {
    final code = e.response?.statusCode;
    if (code == 401 || code == 403) return 'The gateway asked for an API key';
    if (code != null) return 'HTTP $code from the gateway';
    return switch (_mode) {
      RuntimeMode.local =>
        'Nothing answered on this phone. Is `garra start` running in Termux?',
      RuntimeMode.remote =>
        'No answer. Same Wi-Fi? Is the gateway bound to 0.0.0.0?',
      RuntimeMode.cloud => 'Could not reach Garra Cloud',
    };
  }

  Future<void> _finish() async {
    if (!RuntimeConfig.isValidBaseUrl(_url.text)) {
      setState(() => _probeError = 'Enter a valid http(s) address');
      return;
    }
    setState(() => _saving = true);
    final config = RuntimeConfig(
      mode: _mode,
      baseUrl: RuntimeConfig.normalizeBaseUrl(_url.text),
      ownerName: _name.text.trim(),
    );
    await ref
        .read(runtimeConfigStateProvider.notifier)
        .save(config, apiKey: _mode == RuntimeMode.remote ? _apiKey.text : '');
    if (!mounted) return;
    setState(() => _saving = false);
    context.go(_mode == RuntimeMode.cloud ? '/login' : '/home');
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
              child: Row(
                children: [
                  const WolfMark(size: 40),
                  const SizedBox(width: 10),
                  Text(
                    'Garra Mobile',
                    style: garraText(size: 18, weight: FontWeight.w800),
                  ),
                  const Spacer(),
                  _StepDots(step: _step),
                ],
              ),
            ),
            Expanded(
              child: PageView(
                controller: _page,
                physics: const NeverScrollableScrollPhysics(),
                children: [
                  _NameStep(controller: _name, onNext: () => _go(1)),
                  _RuntimeStep(
                    mode: _mode,
                    url: _url,
                    apiKey: _apiKey,
                    testing: _testing,
                    probe: _probe,
                    error: _probeError,
                    onPick: _pickMode,
                    onTest: _test,
                    onBack: () => _go(0),
                    onNext: () => _go(2),
                  ),
                  _DoneStep(
                    mode: _mode,
                    name: _name.text,
                    saving: _saving,
                    onBack: () => _go(1),
                    onFinish: _finish,
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _StepDots extends StatelessWidget {
  final int step;
  const _StepDots({required this.step});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        for (var i = 0; i < 3; i++)
          Container(
            width: i == step ? 18 : 6,
            height: 6,
            margin: const EdgeInsets.only(left: 4),
            decoration: BoxDecoration(
              color: i == step
                  ? GarraColors.violetLight
                  : GarraColors.panelBorderStrong,
              borderRadius: BorderRadius.circular(GarraRadius.pill),
            ),
          ),
      ],
    );
  }
}

class _NameStep extends StatelessWidget {
  final TextEditingController controller;
  final VoidCallback onNext;
  const _NameStep({required this.controller, required this.onNext});

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        const SizedBox(height: 24),
        Text(
          'What should Garra\ncall you?',
          style: garraText(size: 30, weight: FontWeight.w800, height: 1.1),
        ),
        const SizedBox(height: 10),
        Text(
          'Only used for the greeting. It never leaves this device.',
          style: garraText(size: 14, color: GarraColors.textMuted),
        ),
        const SizedBox(height: 28),
        TextField(
          controller: controller,
          autofocus: true,
          textCapitalization: TextCapitalization.words,
          decoration: const InputDecoration(hintText: 'Your name'),
          onSubmitted: (_) => onNext(),
        ),
        const SizedBox(height: 28),
        ElevatedButton(onPressed: onNext, child: const Text('Continue')),
      ],
    );
  }
}

class _RuntimeStep extends StatelessWidget {
  final RuntimeMode mode;
  final TextEditingController url;
  final TextEditingController apiKey;
  final bool testing;
  final GarraHealth? probe;
  final String? error;
  final void Function(RuntimeMode) onPick;
  final VoidCallback onTest;
  final VoidCallback onBack;
  final VoidCallback onNext;

  const _RuntimeStep({
    required this.mode,
    required this.url,
    required this.apiKey,
    required this.testing,
    required this.probe,
    required this.error,
    required this.onPick,
    required this.onTest,
    required this.onBack,
    required this.onNext,
  });

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text(
          'Where does\nGarra run?',
          style: garraText(size: 30, weight: FontWeight.w800, height: 1.1),
        ),
        const SizedBox(height: 10),
        Text(
          'Your memory, skills and files stay on the runtime you pick. The LLM can be anywhere.',
          style: garraText(size: 14, color: GarraColors.textMuted),
        ),
        const SizedBox(height: 20),
        for (final m in RuntimeMode.values) ...[
          _ModeCard(mode: m, selected: m == mode, onTap: () => onPick(m)),
          const SizedBox(height: 10),
        ],
        const SizedBox(height: 8),
        if (mode == RuntimeMode.local) const _TermuxHint(),
        if (mode != RuntimeMode.cloud) ...[
          Text(
            'Gateway address',
            style: garraText(size: 13, weight: FontWeight.w600),
          ),
          const SizedBox(height: 6),
          TextField(
            controller: url,
            keyboardType: TextInputType.url,
            autocorrect: false,
            decoration: InputDecoration(hintText: mode.defaultBaseUrl),
          ),
        ],
        if (mode == RuntimeMode.remote) _PlainHttpHint(url: url),
        if (mode == RuntimeMode.remote) ...[
          const SizedBox(height: 12),
          Text(
            'Gateway API key (optional)',
            style: garraText(size: 13, weight: FontWeight.w600),
          ),
          const SizedBox(height: 6),
          TextField(
            controller: apiKey,
            obscureText: true,
            autocorrect: false,
            decoration: const InputDecoration(
              hintText: 'Only if gateway.api_key is set',
            ),
          ),
        ],
        if (mode != RuntimeMode.cloud) ...[
          const SizedBox(height: 14),
          OutlinedButton.icon(
            onPressed: testing ? null : onTest,
            icon: testing
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.wifi_tethering_rounded, size: 18),
            label: Text(testing ? 'Testing…' : 'Test connection'),
          ),
          const SizedBox(height: 10),
          if (probe != null) _ProbeResult(health: probe!),
          if (error != null)
            Text(error!, style: garraText(size: 13, color: GarraColors.warn)),
        ],
        const SizedBox(height: 24),
        Row(
          children: [
            TextButton(onPressed: onBack, child: const Text('Back')),
            const Spacer(),
            SizedBox(
              width: 160,
              child: ElevatedButton(
                onPressed: onNext,
                child: const Text('Continue'),
              ),
            ),
          ],
        ),
      ],
    );
  }
}

class _ModeCard extends StatelessWidget {
  final RuntimeMode mode;
  final bool selected;
  final VoidCallback onTap;
  const _ModeCard({
    required this.mode,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final icon = switch (mode) {
      RuntimeMode.local => Icons.smartphone_rounded,
      RuntimeMode.remote => Icons.desktop_windows_rounded,
      RuntimeMode.cloud => Icons.cloud_rounded,
    };
    final accent = selected ? GarraColors.violetLight : GarraColors.textMuted;
    return Semantics(
      button: true,
      selected: selected,
      label: mode.title,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(GarraRadius.card),
        child: Ink(
          padding: const EdgeInsets.all(14),
          decoration: BoxDecoration(
            color: selected
                ? GarraColors.violet.withValues(alpha: 0.12)
                : GarraColors.panel,
            borderRadius: BorderRadius.circular(GarraRadius.card),
            border: Border.all(
              color: selected
                  ? GarraColors.violetLight
                  : GarraColors.panelBorder,
              width: selected ? 1.4 : 1,
            ),
          ),
          child: Row(
            children: [
              Icon(icon, color: accent, size: 26),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      mode.title,
                      style: garraText(size: 15, weight: FontWeight.w700),
                    ),
                    const SizedBox(height: 3),
                    Text(
                      mode.description,
                      style: garraText(
                        size: 12.5,
                        color: GarraColors.textMuted,
                        height: 1.3,
                      ),
                    ),
                  ],
                ),
              ),
              if (selected)
                const Icon(
                  Icons.check_circle_rounded,
                  color: GarraColors.violetLight,
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TermuxHint extends StatelessWidget {
  const _TermuxHint();

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(bottom: 14),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: GarraColors.bgElevated,
        borderRadius: BorderRadius.circular(GarraRadius.card),
        border: Border.all(color: GarraColors.panelBorder),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Install Garra in Termux (once):',
            style: garraText(size: 12.5, weight: FontWeight.w600),
          ),
          const SizedBox(height: 6),
          SelectableText(
            'curl -fsSL https://garraia.org/install.sh | bash\ngarra doctor\ngarra start',
            style: garraText(size: 12, mono: true, color: GarraColors.cyan),
          ),
          const SizedBox(height: 6),
          Text(
            'Then come back and tap "Test connection". The app talks to it on 127.0.0.1.',
            style: garraText(size: 12, color: GarraColors.textMuted),
          ),
        ],
      ),
    );
  }
}

/// Shown under the address field while a LAN address is plain `http://`.
/// Loopback (On this phone) never leaves the device, and Cloud is pinned to
/// HTTPS by the network security config, so only the LAN mode needs it.
class _PlainHttpHint extends StatelessWidget {
  final TextEditingController url;
  const _PlainHttpHint({required this.url});

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<TextEditingValue>(
      valueListenable: url,
      builder: (context, value, _) {
        final text = value.text.trim().toLowerCase();
        if (text.isEmpty || text.startsWith('https://')) {
          return const SizedBox.shrink();
        }
        return Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Icon(
                Icons.lock_open_rounded,
                size: 14,
                color: GarraColors.textMuted,
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  'Plain HTTP: your messages and the API key are readable on '
                  'this network. Fine on your home Wi-Fi, not on a public one.',
                  style: garraText(
                    size: 12,
                    color: GarraColors.textMuted,
                    height: 1.3,
                  ),
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

class _ProbeResult extends StatelessWidget {
  final GarraHealth health;
  const _ProbeResult({required this.health});

  @override
  Widget build(BuildContext context) {
    final ok = health.healthy;
    return Row(
      children: [
        Icon(
          ok ? Icons.check_circle_rounded : Icons.warning_amber_rounded,
          color: ok ? GarraColors.ok : GarraColors.warn,
          size: 18,
        ),
        const SizedBox(width: 8),
        Expanded(
          child: Text(
            'Garra ${health.version} · ${health.status}'
            '${health.provider != null ? ' · ${health.provider}' : ''}',
            style: garraText(size: 13),
          ),
        ),
      ],
    );
  }
}

class _DoneStep extends StatelessWidget {
  final RuntimeMode mode;
  final String name;
  final bool saving;
  final VoidCallback onBack;
  final VoidCallback onFinish;

  const _DoneStep({
    required this.mode,
    required this.name,
    required this.saving,
    required this.onBack,
    required this.onFinish,
  });

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        const SizedBox(height: 12),
        const Center(child: WolfMark(size: 120)),
        const SizedBox(height: 24),
        Text(
          name.trim().isEmpty ? 'All set.' : 'All set, ${name.trim()}.',
          textAlign: TextAlign.center,
          style: garraText(size: 28, weight: FontWeight.w800),
        ),
        const SizedBox(height: 10),
        Text(
          'Runtime: ${mode.title}.\n'
          '${mode == RuntimeMode.cloud ? 'Next: sign in to your account.' : 'Your AI lives on your device. The model doesn\'t have to.'}',
          textAlign: TextAlign.center,
          style: garraText(size: 14, color: GarraColors.textMuted, height: 1.4),
        ),
        const SizedBox(height: 32),
        ElevatedButton(
          onPressed: saving ? null : onFinish,
          child: Text(
            saving
                ? 'Saving…'
                : (mode == RuntimeMode.cloud ? 'Sign in' : 'Open Garra'),
          ),
        ),
        const SizedBox(height: 8),
        TextButton(onPressed: onBack, child: const Text('Back')),
      ],
    );
  }
}
