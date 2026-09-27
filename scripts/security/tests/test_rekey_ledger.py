"""Tests para codeql-rekey-ledger.py — a logica pura de remapeamento.

Cobre o cenario real de 2026-08-29, em que 16 entradas `rust/path-injection`
seguiam `dismissed` nos numeros antigos (#67-#82) enquanto 16 alertas NOVOS
(#147-#162) apontavam para os mesmos `(rule_id, path, line)` e estavam abertos.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "codeql-rekey-ledger.py"


def _load_module():
    spec = importlib.util.spec_from_file_location("codeql_rekey_ledger", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def _entry(number: int, rule: str, path: str, line: int) -> dict:
    return {
        "alert_number": number,
        "rule_id": rule,
        "path": path,
        "line": line,
        "ledger_md_anchor": f"alert-{number}",
    }


def _alert(number: int, rule: str, path: str, line: int) -> dict:
    return {
        "number": number,
        "rule": {"id": rule},
        "most_recent_instance": {"location": {"path": path, "start_line": line}},
    }


RULE = "rust/path-injection"
PATH = "crates/garraia-gateway/src/skins_handler.rs"


def test_duplicate_open_alert_is_repointed():
    """O caso real: entrada dispensada em #67, duplicata aberta em #149."""
    mod = _load_module()
    entries = [_entry(67, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 84)]

    mapping, unmatched, ambiguous, _drift = mod.plan_rekey(entries, alerts)

    assert mapping == {67: 149}
    assert unmatched == []
    assert ambiguous == []


def test_entry_already_pointing_at_open_alert_is_left_alone():
    mod = _load_module()
    entries = [_entry(149, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 84)]

    mapping, unmatched, ambiguous, drifted = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert unmatched == []
    assert ambiguous == []
    # Numero aberto E chave batendo: nada a fazer, e nada de drift.
    assert drifted == []


def test_open_number_with_moved_key_is_reported_as_drift():
    """Numero aberto nao basta — a chave tem que bater.

    Ate 2026-08-30 `plan_rekey` so olhava o numero, entao este caso era pulado
    em silencio: nem `mapping`, nem `unmatched`, nem `ambiguous`. A entrada
    apodrecia sem aparecer em relatorio nenhum.
    """
    mod = _load_module()
    entries = [_entry(149, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 117)]  # mesmo numero, linha outra

    mapping, unmatched, ambiguous, drifted = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert unmatched == []
    assert ambiguous == []
    assert len(drifted) == 1
    entry, live = drifted[0]
    assert entry["alert_number"] == 149
    assert live == (RULE, PATH, 117)


def test_the_113_case_drifted_to_a_different_statement():
    """O caso real do #113, encontrado no re-audit de 2026-08-30.

    O ledger apontava `wizard/mod.rs:640` e descrevia um `eprintln!`; o alerta
    #113 seguia aberto, mas em `:673`, outro statement. Como o numero nunca
    deixou de existir, o relatorio nao acusava nada.
    """
    mod = _load_module()
    rule, path = "rust/cleartext-logging", "crates/garraia-cli/src/wizard/mod.rs"
    entries = [_entry(113, rule, path, 640)]
    alerts = [_alert(113, rule, path, 673)]

    mapping, unmatched, ambiguous, drifted = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert unmatched == []
    assert ambiguous == []
    assert drifted == [(entries[0], (rule, path, 673))]


def test_dismissed_entry_without_open_duplicate_is_reported_not_rewritten():
    """Entrada quieta e o estado saudavel — nao e erro, mas e reportada."""
    mod = _load_module()
    entries = [_entry(43, "rust/hard-coded-cryptographic-value", "x.rs", 49)]

    mapping, unmatched, ambiguous, _drift = mod.plan_rekey(entries, [])

    assert mapping == {}
    assert [e["alert_number"] for e in unmatched] == [43]
    assert ambiguous == []


def test_two_open_alerts_on_same_key_are_ambiguous_not_guessed():
    """Fail-closed: o script nunca escolhe por conta propria."""
    mod = _load_module()
    entries = [_entry(67, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 84), _alert(201, RULE, PATH, 84)]

    mapping, unmatched, ambiguous, _drift = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert len(ambiguous) == 1
    assert ambiguous[0][1] == [149, 201]


def test_different_line_is_not_matched():
    """A chave inclui a linha: um alerta noutra linha e outro achado."""
    mod = _load_module()
    entries = [_entry(67, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 999)]

    mapping, unmatched, _, _drift = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert [e["alert_number"] for e in unmatched] == [67]


def test_different_rule_on_same_line_is_not_matched():
    mod = _load_module()
    entries = [_entry(67, RULE, PATH, 84)]
    alerts = [_alert(149, "rust/cleartext-logging", PATH, 84)]

    mapping, unmatched, _, _drift = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert [e["alert_number"] for e in unmatched] == [67]


def test_md_rewrite_touches_only_ledger_patterns():
    mod = _load_module()
    md = (
        '| <a id="alert-67"></a>[#67](https://github.com/michelbr84/GarraRUST'
        "/security/code-scanning/67) | `rust/path-injection` | linha 67 do arquivo |\n"
        "Ver tambem [#67](#alert-67).\n"
    )
    out = mod.rewrite_md(md, {67: 149})

    assert '<a id="alert-149"></a>' in out
    assert "/security/code-scanning/149)" in out
    assert "[#149]" in out
    assert "(#alert-149)" in out
    # Prosa que apenas contem o numero nao pode ser tocada.
    assert "linha 67 do arquivo" in out
    assert "alert-67" not in out


def test_md_rewrite_handles_swap_without_collision():
    """67 -> 149 e 149 -> 200 na mesma passada nao podem se atropelar."""
    mod = _load_module()
    md = '<a id="alert-67"></a> <a id="alert-149"></a>'
    out = mod.rewrite_md(md, {67: 149, 149: 200})

    assert '<a id="alert-149"></a>' in out
    assert '<a id="alert-200"></a>' in out
    assert "alert-67" not in out


# ── Statement multi-linha: o alerta cobre um span, o ledger ancora o sink ──


def _alert_span(number: int, rule: str, path: str, start: int, end: int) -> dict:
    return {
        "number": number,
        "rule": {"id": rule},
        "most_recent_instance": {
            "location": {"path": path, "start_line": start, "end_line": end}
        },
    }


LOG_RULE = "rust/cleartext-logging"
LOG_PATH = "crates/garraia-cli/src/whatsapp.rs"


def test_multiline_statement_alert_matches_sink_line_inside_its_span():
    """O caso real do #176 (2026-09-25).

    O `println!` de `garra whatsapp link` virou multi-linha com o rustfmt: o
    CodeQL reporta o span do statement (1356-1358) e o ledger ancora a linha
    do SINK, derivada do `sink_snippet` pelo `check-ledger-anchors.py` (1358).
    O `codeql-reapply-dismissals.sh` ja casa por span; o rekey comparava
    `start_line` exato e deixava a duplicata aberta como "sem entrada".
    """
    mod = _load_module()
    entries = [_entry(173, LOG_RULE, LOG_PATH, 1358)]
    alerts = [_alert_span(176, LOG_RULE, LOG_PATH, 1356, 1358)]

    mapping, unmatched, ambiguous, drifted = mod.plan_rekey(entries, alerts)

    assert mapping == {173: 176}
    assert unmatched == []
    assert ambiguous == []
    assert drifted == []


def test_sink_line_outside_the_span_is_not_matched():
    """Span nao e tolerancia: uma linha fora dele e outro statement."""
    mod = _load_module()
    entries = [_entry(173, LOG_RULE, LOG_PATH, 1360)]
    alerts = [_alert_span(176, LOG_RULE, LOG_PATH, 1356, 1358)]

    mapping, unmatched, _ambiguous, _drift = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert [e["alert_number"] for e in unmatched] == [173]


def test_entry_already_pointing_at_open_span_alert_is_not_drift():
    """Depois do rekey, a entrada aponta para #176 e o alerta vivo e o mesmo
    span: nada a fazer, e nao e drift."""
    mod = _load_module()
    entries = [_entry(176, LOG_RULE, LOG_PATH, 1358)]
    alerts = [_alert_span(176, LOG_RULE, LOG_PATH, 1356, 1358)]

    mapping, unmatched, ambiguous, drifted = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert unmatched == []
    assert ambiguous == []
    assert drifted == []


def test_orphan_report_uses_the_span_start_line():
    """`alert_key` continua expondo a linha inicial para o relatorio de orfaos."""
    mod = _load_module()
    key = mod.alert_key(_alert_span(176, LOG_RULE, LOG_PATH, 1356, 1358))
    assert key[0] == LOG_RULE and key[1] == LOG_PATH and key[2] == 1356
