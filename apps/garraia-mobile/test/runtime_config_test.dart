import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:garraia_mobile/runtime/runtime_config.dart';
import 'package:garraia_mobile/services/saved_localizations.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'support/l10n_test_support.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('RuntimeModeLabels', () {
    test('title and description follow the language (#1178)', () async {
      final pt = await loadL10n(testLocalePt);
      final en = await loadL10n(testLocaleEn);

      expect(RuntimeMode.local.title(pt), pt.runtimeModeLocalTitle);
      expect(RuntimeMode.remote.title(pt), pt.runtimeModeRemoteTitle);
      expect(RuntimeMode.cloud.title(pt), pt.runtimeModeCloudTitle);
      expect(RuntimeMode.local.description(en), en.runtimeModeLocalDescription);
      expect(
        RuntimeMode.remote.description(en),
        en.runtimeModeRemoteDescription,
      );
      expect(RuntimeMode.cloud.description(en), en.runtimeModeCloudDescription);

      // The two languages really differ — "Garra Cloud" is a product name and
      // reads the same in both, so it is left out here.
      expect(RuntimeMode.local.title(pt), isNot(RuntimeMode.local.title(en)));
      expect(
        RuntimeMode.remote.description(pt),
        isNot(RuntimeMode.remote.description(en)),
      );
    });
  });

  group('savedLocalizations', () {
    test('reads the language Settings persisted', () async {
      SharedPreferences.setMockInitialValues({
        AppLanguageState.prefsKey: AppLanguage.ptBR.storageValue,
      });
      final pt = await loadL10n(testLocalePt);
      expect(
        (await savedLocalizations()).runtimeModeLocalTitle,
        pt.runtimeModeLocalTitle,
      );

      SharedPreferences.setMockInitialValues({
        AppLanguageState.prefsKey: AppLanguage.en.storageValue,
      });
      final en = await loadL10n(testLocaleEn);
      expect(
        (await savedLocalizations()).runtimeModeLocalTitle,
        en.runtimeModeLocalTitle,
      );
    });

    test('nothing persisted follows the device like the app does', () async {
      SharedPreferences.setMockInitialValues({});
      final expected = lookupAppLocalizations(
        effectiveLocale(AppLanguage.system),
      );
      expect(
        (await savedLocalizations()).runtimeModeLocalTitle,
        expected.runtimeModeLocalTitle,
      );
    });
  });

  group('RuntimeConfig.normalizeBaseUrl', () {
    test('adds http:// and strips trailing slashes', () {
      expect(
        RuntimeConfig.normalizeBaseUrl('192.168.1.24:3888/'),
        'http://192.168.1.24:3888',
      );
      expect(
        RuntimeConfig.normalizeBaseUrl('  https://garra.local//  '),
        'https://garra.local',
      );
      expect(RuntimeConfig.normalizeBaseUrl(''), '');
    });

    test('validates scheme and host', () {
      expect(RuntimeConfig.isValidBaseUrl('127.0.0.1:3888'), isTrue);
      expect(RuntimeConfig.isValidBaseUrl('https://api.garraia.org'), isTrue);
      expect(RuntimeConfig.isValidBaseUrl('ftp://nope'), isFalse);
      expect(RuntimeConfig.isValidBaseUrl(''), isFalse);
      expect(RuntimeConfig.isValidBaseUrl('http://'), isFalse);
    });
  });

  group('RuntimeStore', () {
    setUp(() {
      SharedPreferences.setMockInitialValues({});
      FlutterSecureStorage.setMockInitialValues({});
    });

    test('load returns null before onboarding', () async {
      expect(await RuntimeStore().load(), isNull);
    });

    test(
      'save / load round-trips mode, url and name; api key stays in secure storage',
      () async {
        final store = RuntimeStore();
        const cfg = RuntimeConfig(
          mode: RuntimeMode.remote,
          baseUrl: 'http://10.0.0.5:3888',
          ownerName: 'Ana',
        );
        await store.save(cfg);
        await store.saveApiKey('secret-key');
        await store.saveSessionId('sess-1');

        expect(await store.load(), cfg);
        expect(await store.readApiKey(), 'secret-key');
        expect(await store.readSessionId(), 'sess-1');

        // The key must not leak into preferences.
        final prefs = await SharedPreferences.getInstance();
        for (final k in prefs.getKeys()) {
          expect(
            prefs.get(k).toString(),
            isNot(contains('secret-key')),
            reason: k,
          );
        }
      },
    );

    test('clear forgets runtime, session and key', () async {
      final store = RuntimeStore();
      await store.save(
        const RuntimeConfig(
          mode: RuntimeMode.local,
          baseUrl: kDefaultLocalBaseUrl,
        ),
      );
      await store.saveApiKey('k');
      await store.saveSessionId('s');
      await store.clear();
      expect(await store.load(), isNull);
      expect(await store.readApiKey(), isNull);
      expect(await store.readSessionId(), isNull);
    });

    test('empty api key deletes the stored one', () async {
      final store = RuntimeStore();
      await store.saveApiKey('k');
      await store.saveApiKey('   ');
      expect(await store.readApiKey(), isNull);
    });
  });

  test('RuntimeMode defaults are the documented ports', () {
    expect(RuntimeMode.local.defaultBaseUrl, 'http://127.0.0.1:3888');
    expect(RuntimeMode.cloud.defaultBaseUrl, kCloudBaseUrl);
    expect(RuntimeModeLabels.parse('remote'), RuntimeMode.remote);
    expect(RuntimeModeLabels.parse('bogus'), isNull);
  });
}
