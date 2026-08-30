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

    mapping, unmatched, ambiguous = mod.plan_rekey(entries, alerts)

    assert mapping == {67: 149}
    assert unmatched == []
    assert ambiguous == []


def test_entry_already_pointing_at_open_alert_is_left_alone():
    mod = _load_module()
    entries = [_entry(149, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 84)]

    mapping, unmatched, ambiguous = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert unmatched == []
    assert ambiguous == []


def test_dismissed_entry_without_open_duplicate_is_reported_not_rewritten():
    """Entrada quieta e o estado saudavel — nao e erro, mas e reportada."""
    mod = _load_module()
    entries = [_entry(43, "rust/hard-coded-cryptographic-value", "x.rs", 49)]

    mapping, unmatched, ambiguous = mod.plan_rekey(entries, [])

    assert mapping == {}
    assert [e["alert_number"] for e in unmatched] == [43]
    assert ambiguous == []


def test_two_open_alerts_on_same_key_are_ambiguous_not_guessed():
    """Fail-closed: o script nunca escolhe por conta propria."""
    mod = _load_module()
    entries = [_entry(67, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 84), _alert(201, RULE, PATH, 84)]

    mapping, unmatched, ambiguous = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert len(ambiguous) == 1
    assert ambiguous[0][1] == [149, 201]


def test_different_line_is_not_matched():
    """A chave inclui a linha: um alerta noutra linha e outro achado."""
    mod = _load_module()
    entries = [_entry(67, RULE, PATH, 84)]
    alerts = [_alert(149, RULE, PATH, 999)]

    mapping, unmatched, _ = mod.plan_rekey(entries, alerts)

    assert mapping == {}
    assert [e["alert_number"] for e in unmatched] == [67]


def test_different_rule_on_same_line_is_not_matched():
    mod = _load_module()
    entries = [_entry(67, RULE, PATH, 84)]
    alerts = [_alert(149, "rust/cleartext-logging", PATH, 84)]

    mapping, unmatched, _ = mod.plan_rekey(entries, alerts)

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
