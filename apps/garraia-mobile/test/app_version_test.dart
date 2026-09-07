import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/app_version.dart';

/// `lib/app_version.dart` must mirror `pubspec.yaml`'s `version:` line. This
/// is the guard against the three-way version drift the app carried before
/// v0.4.0 (pubspec 0.2.1+2, Settings "v0.1.0 (Alpha)", sync "0.1.0").
void main() {
  test('kAppVersion / kAppBuildNumber match pubspec.yaml', () {
    final pubspec = File('pubspec.yaml').readAsStringSync();
    final match = RegExp(
      r'^version:\s*(\d+\.\d+\.\d+)\+(\d+)\s*$',
      multiLine: true,
    ).firstMatch(pubspec);
    expect(
      match,
      isNotNull,
      reason: 'pubspec.yaml has no `version: X.Y.Z+N` line',
    );

    expect(kAppVersion, match!.group(1));
    expect(kAppBuildNumber, int.parse(match.group(2)!));
    expect(kAppVersionLabel, 'Garra Mobile v$kAppVersion');
  });
}
