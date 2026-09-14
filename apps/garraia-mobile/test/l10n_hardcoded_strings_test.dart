// #1178 — the "lint de strings externalizadas" the issue asks for.
//
// Scans every Dart file under lib/ (except the l10n machinery itself) for
// string literals that look like user-facing copy and bypass `context.l10n`.
// The gate is inverted on purpose: a literal is suspicious by default and is
// only waved through when the line is a known non-UI sink (logging, keys,
// routes, exceptions, storage, regexes, SQL...) or the literal is plainly an
// identifier. Anything that survives must go through the ARB files — or be
// justified inline with a trailing `// l10n-ignore` comment.
//
// The scanner is a pure function over lines so the fixtures below pin the
// shapes it must catch (the ones this PR itself had to migrate) and the
// shapes it must ignore.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Lines whose literals never reach the screen.
final _nonUiSink = RegExp(
  r'^\s*(import|export|part)\b'
  r'|\b(debugPrint|print|log|logger\.\w+|assert|throw|rethrow)\b'
  r'|\b\w*(Exception|Error)\s*\('
  r'|\b(RegExp|Uri\.parse|Uri\(|ValueKey|Key|GoRoute|DateFormat|Duration)\s*\('
  r'|\b(path|name|route|scheme|host|fontFamily|package|channelId|url|baseUrl|method|endpoint|key|id|type|mode|role|status|semanticsIdentifier)\s*:'
  r'|\b(contains|startsWith|endsWith|split|replaceAll|indexOf|compareTo|padLeft|padRight)\s*\('
  r'|\b(rawQuery|rawInsert|rawUpdate|rawDelete|execute|query|insert|update|delete)\s*\('
  r'|\b(where|orderBy|groupBy|having|columns|whereArgs)\s*:'
  r'|\b(SELECT|CREATE TABLE|INSERT INTO|DELETE FROM|ORDER BY)\b'
  r'|\b(getString|setString|getBool|setBool|getInt|setInt|remove|containsKey|read|write|delete)\s*\('
  r'|\bBearer\b|\bapplication/json\b|\bheaders\b'
  r'|//\s*l10n-ignore',
);

/// A literal that carries a word of 3+ letters (accents included).
final _literal = RegExp(
  r'''(?<![\w$])(['"])((?:(?!\1)[^\\\n]|\\.)*?[A-Za-zÀ-ÿ]{3,}(?:(?!\1)[^\\\n]|\\.)*?)\1''',
);

/// Two words separated by a space, letters on both sides — copy, not a key.
final _multiWord = RegExp(r'[A-Za-zÀ-ÿ]\s+[A-Za-zÀ-ÿ]');

/// Accented letters only appear in copy.
final _accented = RegExp(r'[À-ÿ]');

/// A widget or parameter that renders text: with one of these on the line,
/// even a single Capitalized word (`Text('Retry')`) is copy.
final _uiMarker = RegExp(
  r'\b(Text|SelectableText|TextSpan|SnackBar|Tooltip|AlertDialog|ListTile|Tab|SectionHeader|EmptyState|ErrorState|UnavailableFeature|ChatFailed|ChatTurnFailed|GarraPage)\s*\('
  r'|\b(hintText|labelText|helperText|errorText|tooltip|semanticsLabel|semanticLabel|title|subtitle|label|content|message|text|emptyText|toast|why|feature|reason|counterText|prefixText|suffixText)\s*:',
);

/// Identifier-ish literal: one token of word/path characters, no spaces.
final _identifier = RegExp(r'^[\w\-./:$@{}%+=?&~]+$');

/// One Capitalized word, optionally punctuated (`Retry`, `Sessões`, `Voltar`).
final _capitalizedWord = RegExp(r'^[A-ZÀ-Ý][a-zà-ÿ]{2,}[!?.…]*$');

/// Literals that are the same in every language.
const _allowlist = <String>{
  'Garra',
  'Garra Mobile',
  'GarraIA',
  'Garra Cloud',
  'Termux',
  'Inter',
  'JetBrainsMono',
  'Roboto',
  'OK',
};

bool _isInterpolationOnly(String s) =>
    RegExp(r'^\s*(\$\{?[\w.]+\}?\s*)+$').hasMatch(s);

/// Offenders as `path:line: literal` for the given files. Pure, so it can
/// be exercised with fixtures.
List<String> scanForHardcodedCopy(Map<String, List<String>> files) {
  final offenders = <String>[];
  final paths = files.keys.toList()..sort();
  for (final path in paths) {
    final lines = files[path]!;
    for (var i = 0; i < lines.length; i++) {
      final line = lines[i];
      final trimmed = line.trimLeft();
      if (trimmed.startsWith('//') || trimmed.startsWith('///')) continue;
      if (_nonUiSink.hasMatch(line)) continue;
      final hasMarker = _uiMarker.hasMatch(line);
      for (final m in _literal.allMatches(line)) {
        final text = m.group(2)!;
        if (_allowlist.contains(text)) continue;
        if (_isInterpolationOnly(text)) continue;
        // Copy is: two words, or an accented word, or — under a UI marker —
        // a single Capitalized word (`Text('Retry')`).
        final looksLikeCopy =
            _multiWord.hasMatch(text) ||
            _accented.hasMatch(text) ||
            (hasMarker && _capitalizedWord.hasMatch(text));
        if (!looksLikeCopy) continue;
        if (!hasMarker && _identifier.hasMatch(text)) continue;
        offenders.add('$path:${i + 1}: $text');
      }
    }
  }
  return offenders;
}

void main() {
  group('scanner fixtures', () {
    List<String> scan(String line) => scanForHardcodedCopy({
      'lib/x.dart': [line],
    });

    test('catches every literal shape this PR migrated', () {
      for (final line in [
        "Text('Retry')",
        "const Text('No sessions on the runtime. Start a chat.')",
        "emptyText: 'No projects yet',",
        "text: 'Meu QR Code',",
        "const SectionHeader('Sessions'),",
        "toast: 'Log copiado',",
        "feature: 'Automations',",
        "why: 'The connected Garra runtime does not expose a scheduling API yet.',",
        "RuntimeMode.local => 'On this phone',",
        "return 'E-mail ou senha incorretos.';",
        "setState(() => _error = 'Permissão de microfone necessária');",
        "yield const ChatFailed('a conexao caiu antes da resposta');",
        "hintText: 'Type a message...',",
        "tooltip: 'Copiar mensagem',",
        "title: const Text('Sair da conta?'),",
        "TextSpan(text: 'Bom te ver de novo.'),",
        "'Não configurado'",
      ]) {
        expect(scan(line), isNotEmpty, reason: 'missed: $line');
      }
    });

    test('ignores non-UI literals', () {
      for (final line in [
        "import 'package:flutter/material.dart';",
        "debugPrint('SyncService: failed to parse message: \$e');",
        "throw ArgumentError('sessionId is required for gateway chat');",
        "key: const ValueKey('copy-message'),",
        "if (msg.contains('invalid credentials')) return l10n.loginErrorInvalidCredentials;",
        "await db.rawQuery('SELECT id FROM queue WHERE sent = 0');",
        "final prefs = await SharedPreferences.getInstance(); prefs.getString('app_language');",
        "GoRoute(path: '/settings', builder: (_, __) => const SettingsScreen()),",
        "Text(l10n.settingsTitle)",
        "Text('Garra Mobile')",
        "fontFamily: 'JetBrainsMono',",
        "headers: {'Authorization': 'Bearer \$token'},",
        "final label = 'v\${h.version}';",
        "options.baseUrl = 'http://127.0.0.1:3888';",
        "Text('Custom copy here') // l10n-ignore: brand slogan",
      ]) {
        expect(scan(line), isEmpty, reason: 'false positive: $line');
      }
    });
  });

  test('user-facing strings go through context.l10n (#1178)', () {
    final lib = Directory('lib');
    expect(lib.existsSync(), isTrue, reason: 'run from apps/garraia-mobile');

    final files = <String, List<String>>{};
    for (final f in lib.listSync(recursive: true).whereType<File>()) {
      if (!f.path.endsWith('.dart') || f.path.endsWith('.g.dart')) continue;
      if (f.path.startsWith('lib/l10n/')) continue;
      files[f.path] = f.readAsLinesSync();
    }

    final offenders = scanForHardcodedCopy(files);
    expect(
      offenders,
      isEmpty,
      reason:
          'Hard-coded user-facing strings found — move them to lib/l10n/app_en.arb + app_pt.arb and read them via context.l10n (or justify with a trailing `// l10n-ignore: <why>`):\n${offenders.join('\n')}',
    );
  });
}
