import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:local_auth/local_auth.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';
import 'package:shared_preferences/shared_preferences.dart';

part 'biometric_service.g.dart';

const _kBiometricEnabledKey = 'garraia_biometric_enabled';
const _kPinKey = 'garraia_pin';

@Riverpod(keepAlive: true)
BiometricService biometricService(Ref ref) => BiometricService();

/// Tracks whether biometric auth has been completed this session.
@riverpod
class BiometricAuthState extends _$BiometricAuthState {
  @override
  bool build() => false;

  void setAuthenticated() => state = true;
  void reset() => state = false;
}

class BiometricService {
  final LocalAuthentication _auth = LocalAuthentication();

  /// Check if the device supports biometric authentication.
  Future<bool> isAvailable() async {
    try {
      final canCheck = await _auth.canCheckBiometrics;
      final isSupported = await _auth.isDeviceSupported();
      return canCheck || isSupported;
    } on PlatformException {
      return false;
    }
  }

  /// Get available biometric types (fingerprint, face, etc.).
  Future<List<BiometricType>> getAvailableBiometrics() async {
    try {
      return await _auth.getAvailableBiometrics();
    } on PlatformException {
      return [];
    }
  }

  /// Attempt biometric authentication.
  /// Returns true if authenticated, false if cancelled/failed.
  Future<bool> authenticate({
    String reason = 'Autentique-se para acessar o Garra',
  }) async {
    try {
      return await _auth.authenticate(
        localizedReason: reason,
        options: const AuthenticationOptions(
          stickyAuth: true,
          biometricOnly: false, // Allow PIN/pattern fallback
        ),
      );
    } on PlatformException {
      return false;
    }
  }

  /// Check if biometric lock is enabled by the user.
  Future<bool> isEnabled() async {
    final prefs = await SharedPreferences.getInstance();
    return prefs.getBool(_kBiometricEnabledKey) ?? false;
  }

  /// Enable or disable biometric lock.
  Future<void> setEnabled(bool enabled) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool(_kBiometricEnabledKey, enabled);
  }

  /// Set a PIN fallback code.
  Future<void> setPin(String pin) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_kPinKey, pin);
  }

  /// Verify a PIN code.
  Future<bool> verifyPin(String pin) async {
    final prefs = await SharedPreferences.getInstance();
    final stored = prefs.getString(_kPinKey);
    return stored != null && stored == pin;
  }

  /// Check if a PIN has been set.
  Future<bool> hasPin() async {
    final prefs = await SharedPreferences.getInstance();
    return prefs.containsKey(_kPinKey);
  }

  /// Remove the PIN.
  Future<void> removePin() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_kPinKey);
  }
}
