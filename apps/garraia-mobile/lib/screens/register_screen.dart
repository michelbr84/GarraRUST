import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../l10n/l10n.dart';
import '../providers/auth_provider.dart';
import '../widgets/mascot_widget.dart';

class RegisterScreen extends ConsumerStatefulWidget {
  const RegisterScreen({super.key});

  @override
  ConsumerState<RegisterScreen> createState() => _RegisterScreenState();
}

class _RegisterScreenState extends ConsumerState<RegisterScreen> {
  final _formKey = GlobalKey<FormState>();
  final _emailCtrl = TextEditingController();
  final _passCtrl = TextEditingController();
  final _confirmCtrl = TextEditingController();
  bool _obscure = true;
  String? _error;

  @override
  void dispose() {
    _emailCtrl.dispose();
    _passCtrl.dispose();
    _confirmCtrl.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (!_formKey.currentState!.validate()) return;
    setState(() => _error = null);
    await ref
        .read(authStateProvider.notifier)
        .register(_emailCtrl.text.trim(), _passCtrl.text);
    if (!mounted) return;
    final s = ref.read(authStateProvider);
    if (s is AsyncError) {
      final l10n = context.l10n;
      setState(() => _error = _friendlyError(l10n, s.error ?? 'unknown error'));
    }
  }

  @override
  Widget build(BuildContext context) {
    final loading = ref.watch(authStateProvider).isLoading;
    final cs = Theme.of(context).colorScheme;
    final l10n = context.l10n;

    return Scaffold(
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(horizontal: 28, vertical: 40),
          child: Form(
            key: _formKey,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const MascotWidget(size: 100),
                const SizedBox(height: 12),
                Text(
                  l10n.registerCreateAccount,
                  textAlign: TextAlign.center,
                  style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                    fontWeight: FontWeight.bold,
                  ),
                ),
                const SizedBox(height: 32),
                TextFormField(
                  controller: _emailCtrl,
                  keyboardType: TextInputType.emailAddress,
                  decoration: InputDecoration(
                    labelText: l10n.commonEmailLabel,
                    prefixIcon: const Icon(Icons.email_outlined),
                  ),
                  validator: (v) => v != null && v.contains('@')
                      ? null
                      : l10n.commonEmailInvalid,
                ),
                const SizedBox(height: 14),
                TextFormField(
                  controller: _passCtrl,
                  obscureText: _obscure,
                  decoration: InputDecoration(
                    labelText: l10n.commonPasswordLabel,
                    prefixIcon: const Icon(Icons.lock_outline),
                    suffixIcon: IconButton(
                      icon: Icon(
                        _obscure ? Icons.visibility_off : Icons.visibility,
                      ),
                      onPressed: () => setState(() => _obscure = !_obscure),
                    ),
                  ),
                  validator: (v) => v != null && v.length >= 8
                      ? null
                      : l10n.commonPasswordMinLength,
                ),
                const SizedBox(height: 14),
                TextFormField(
                  controller: _confirmCtrl,
                  obscureText: _obscure,
                  decoration: InputDecoration(
                    labelText: l10n.registerConfirmPasswordLabel,
                    prefixIcon: const Icon(Icons.lock_outline),
                  ),
                  validator: (v) => v == _passCtrl.text
                      ? null
                      : l10n.registerPasswordsMismatch,
                ),
                const SizedBox(height: 8),
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Text(
                      _error!,
                      style: TextStyle(color: cs.error, fontSize: 13),
                    ),
                  ),
                const SizedBox(height: 24),
                ElevatedButton(
                  onPressed: loading ? null : _submit,
                  child: loading
                      ? const SizedBox(
                          height: 20,
                          width: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : Text(l10n.registerCreateAccount),
                ),
                const SizedBox(height: 16),
                TextButton(
                  onPressed: () => context.go('/login'),
                  child: Text(l10n.registerHaveAccountCta),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  String _friendlyError(AppLocalizations l10n, Object e) {
    final s = e.toString().toLowerCase();
    if (s.contains('409') || s.contains('already registered')) {
      return l10n.registerErrorEmailTaken;
    }
    if (s.contains('network') || s.contains('connection')) {
      return l10n.commonErrorNoConnection;
    }
    return l10n.registerErrorGeneric;
  }
}
