// #1178 — the "lint de strings externalizadas" the issue asks for.
//
// Scans every Dart file under lib/ (except the l10n machinery itself) for
// user-facing string literals that bypass `context.l10n`: a quoted literal
// with real words sitting on the same line as a widget property that renders
// text (`Text(`, `hintText:`, `tooltip:`, `SnackBar`, ...). Anything new must
// go through the ARB files; the few legitimate literals (brand names, glyphs)
// live in [_allowlist].
//
// Heuristic on purpose: it catches the way strings are written in this
// codebase today, and a false positive costs one allowlist line, while a
// hard-coded string costs a user their language.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Lines are flagged only when one of these markers is present, so log
/// messages, storage keys and route paths are not reported.
final _uiMarkers = RegExp(
  r'\b(Text|SelectableText|SnackBar|Tooltip|AlertDialog|ListTile)\s*\('
  r'|\b(hintText|labelText|helperText|errorText|tooltip|semanticsLabel|semanticLabel|title|subtitle|label|content|message|counterText|prefixText|suffixText)\s*:',
);

/// Logging calls: their literals never reach the screen.
final _logCall = RegExp(r'\b(debugPrint|print|log|logger\.\w+)\s*\(');

/// A literal with at least one word of 3+ letters (accents included).
final _literal = RegExp(
  r'''(?<![\w$])(['"])((?:(?!\1)[^\\\n]|\\.)*?[A-Za-zÀ-ÿ]{3,}(?:(?!\1)[^\\\n]|\\.)*?)\1''',
);

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

/// Files that are allowed to keep literal copy (none today; keep the list so
/// a justified exception is one line, with a comment, instead of a lint
/// weakening).
const _fileAllowlist = <String>{};

bool _isInterpolationOnly(String s) =>
    RegExp(r'^\s*(\$\{?[\w.]+\}?\s*)+$').hasMatch(s);

void main() {
  test('user-facing strings go through context.l10n (#1178)', () {
    final lib = Directory('lib');
    expect(lib.existsSync(), isTrue, reason: 'run from apps/garraia-mobile');

    final offenders = <String>[];
    final files =
        lib
            .listSync(recursive: true)
            .whereType<File>()
            .where((f) => f.path.endsWith('.dart'))
            .where((f) => !f.path.endsWith('.g.dart'))
            .where((f) => !f.path.startsWith('lib/l10n/'))
            .where((f) => !_fileAllowlist.contains(f.path))
            .toList()
          ..sort((a, b) => a.path.compareTo(b.path));

    for (final file in files) {
      final lines = file.readAsLinesSync();
      for (var i = 0; i < lines.length; i++) {
        final line = lines[i];
        final trimmed = line.trimLeft();
        if (trimmed.startsWith('//') || trimmed.startsWith('///')) continue;
        // Log lines are not UI: `debugPrint('...: message: $e')` would
        // otherwise trip the `message:` marker inside the literal.
        if (_logCall.hasMatch(line)) continue;
        if (!_uiMarkers.hasMatch(line)) continue;
        for (final m in _literal.allMatches(line)) {
          final text = m.group(2)!;
          if (_allowlist.contains(text)) continue;
          if (_isInterpolationOnly(text)) continue;
          // Keys and identifiers, not copy: no spaces and snake/camel/kebab.
          if (RegExp(r'^[\w\-./:]+$').hasMatch(text) && !text.contains(' ')) {
            continue;
          }
          offenders.add('${file.path}:${i + 1}: $text');
        }
      }
    }

    expect(
      offenders,
      isEmpty,
      reason:
          'Hard-coded user-facing strings found — move them to lib/l10n/app_en.arb + app_pt.arb and read them via context.l10n:\n${offenders.join('\n')}',
    );
  });
}
