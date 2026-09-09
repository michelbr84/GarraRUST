/// Single source of truth for the version string shown in the UI and sent to
/// the gateway (`register_device.app_version`).
///
/// Must match `version:` in `pubspec.yaml` — `test/app_version_test.dart`
/// parses the pubspec and fails the build when the two drift. Before this
/// file existed the app carried three different versions at once
/// (`0.2.1+2` in pubspec, `v0.1.0 (Alpha)` in Settings, `0.1.0` in sync).
const String kAppVersion = '0.4.1';

/// Android `versionCode` — the `+N` suffix of the pubspec version.
const int kAppBuildNumber = 5;

/// Human-facing label, e.g. `Garra Mobile v0.4.1`.
const String kAppVersionLabel = 'Garra Mobile v$kAppVersion';
