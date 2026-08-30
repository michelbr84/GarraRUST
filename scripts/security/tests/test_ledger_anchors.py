#!/usr/bin/env python3
"""Tests para check-ledger-anchors.py — a deteccao de drift das ancoras.

Escrito em `unittest` (stdlib) e nao no estilo de funcoes soltas de
`test_rekey_ledger.py`, de proposito: aquele arquivo precisa do pytest para
rodar e por isso nunca foi executado por nenhum job do ci.yml
(`grep -rn pytest .github/` nao retorna nada). Com `unittest.TestCase` o
arquivo roda com `python3 scripts/security/tests/test_ledger_anchors.py` em
qualquer runner, sem `pip install`, o que mantem a superficie de
supply-chain do workflow em zero — a mesma razao pela qual o
security-gate-bola.yml usa `rm -rf` inline em vez de action externa.

Cobre os dois casos historicos que motivaram o script: o alerta #113, que
passou tres meses descrevendo o statement errado, e o PR #882, que deslocou
as cinco ancoras de skins_handler.rs em ate 18 linhas.
"""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = SCRIPTS_DIR.parents[1]
SCRIPT = SCRIPTS_DIR / "check-ledger-anchors.py"


def _load_module():
    spec = importlib.util.spec_from_file_location("check_ledger_anchors", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


MOD = _load_module()

MD_HEADER = "| # | Rule | File:line | Disposition | Reason | Justificativa | GAR |\n"


def _md_row(number: int, rule: str, path: str, line: int) -> str:
    return (
        f'| <a id="alert-{number}"></a>'
        f"[#{number}](https://github.com/o/r/security/code-scanning/{number}) "
        f"| `{rule}` | `{path}:{line}` | dismissed-false-positive "
        f"| `false_positive` | justificativa revisada. | GAR-490 |\n"
    )


class LedgerAnchorsTest(unittest.TestCase):
    """Cada caso monta um ledger sintetico completo em tmp e roda o checker."""

    def setUp(self) -> None:
        self._tmp = __import__("tempfile").TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    # -- helpers ----------------------------------------------------------

    def build(self, entries, *, source: str, md_rows=None) -> tuple[Path, Path]:
        """Escreve src/handler.rs, o ledger .json e o ledger .md."""
        src = self.root / "src"
        src.mkdir(exist_ok=True)
        (src / "handler.rs").write_text(source, encoding="utf-8")

        ledger = self.root / "ledger.json"
        ledger.write_text(
            json.dumps({"schema_version": "1.1.0", "entries": entries}, indent=2),
            encoding="utf-8",
        )

        md = self.root / "ledger.md"
        if md_rows is None:
            md_rows = [
                _md_row(e["alert_number"], e["rule_id"], e["path"], e["line"])
                for e in entries
            ]
        md.write_text(MD_HEADER + "".join(md_rows), encoding="utf-8")
        return ledger, md

    def run_check(self, ledger: Path, md: Path) -> int:
        """Roda o checker e devolve o exit code, venha ele de return ou raise."""
        import sys

        argv = sys.argv
        sys.argv = [
            "check-ledger-anchors.py",
            "--ledger", str(ledger),
            "--md", str(md),
            "--root", str(self.root),
        ]
        try:
            return MOD.main()
        except SystemExit as exc:
            return int(exc.code)
        finally:
            sys.argv = argv

    @staticmethod
    def entry(number=162, line=3, snippet="if !file_path.is_file() {"):
        return {
            "alert_number": number,
            "rule_id": "rust/path-injection",
            "path": "src/handler.rs",
            "line": line,
            "sink_snippet": snippet,
            "disposition": "dismissed-false-positive",
            "dismissed_reason": "false_positive",
        }

    SOURCE = (
        "fn get(name: &str) {\n"
        "    let file_path = dir.join(name);\n"
        "    if !file_path.is_file() {\n"
        "        return not_found();\n"
        "    }\n"
        "}\n"
    )

    # -- casos que devem passar -------------------------------------------

    def test_matching_anchor_passes(self):
        ledger, md = self.build([self.entry()], source=self.SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)

    def test_indentation_is_ignored(self):
        """O snippet e comparado com .strip(), entao reindentar nao e drift."""
        source = self.SOURCE.replace(
            "    if !file_path.is_file() {", "            if !file_path.is_file() {"
        )
        ledger, md = self.build([self.entry()], source=source)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)

    # -- drift (exit 6) ----------------------------------------------------

    def test_shifted_anchor_is_drift(self):
        """O caso do PR #882: linhas inseridas acima deslocam o sink."""
        source = "// helper novo\n" * 18 + self.SOURCE
        ledger, md = self.build([self.entry()], source=source)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_anchor_pointing_at_a_different_statement_is_drift(self):
        """O caso do alerta #113: numero certo, statement errado."""
        ledger, md = self.build(
            [self.entry(line=4, snippet="if !file_path.is_file() {")],
            source=self.SOURCE,
        )
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_md_fileline_out_of_sync_is_drift(self):
        entries = [self.entry()]
        rows = [_md_row(162, "rust/path-injection", "src/handler.rs", 99)]
        ledger, md = self.build(entries, source=self.SOURCE, md_rows=rows)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_md_rule_id_out_of_sync_is_drift(self):
        entries = [self.entry()]
        rows = [_md_row(162, "rust/cleartext-logging", "src/handler.rs", 3)]
        ledger, md = self.build(entries, source=self.SOURCE, md_rows=rows)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_entry_absent_from_md_is_drift(self):
        ledger, md = self.build([self.entry()], source=self.SOURCE, md_rows=[])
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    # -- precondicao (exit 5) ---------------------------------------------

    def test_line_out_of_range_is_precondition_failure(self):
        ledger, md = self.build([self.entry(line=9999)], source=self.SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)

    def test_missing_sink_snippet_is_precondition_failure(self):
        entry = self.entry()
        del entry["sink_snippet"]
        ledger, md = self.build([entry], source=self.SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)

    def test_missing_source_file_is_precondition_failure(self):
        entry = self.entry()
        entry["path"] = "src/nao_existe.rs"
        ledger, md = self.build([entry], source=self.SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)

    def test_malformed_ledger_is_precondition_failure(self):
        ledger, md = self.build([self.entry()], source=self.SOURCE)
        ledger.write_text("{ nao e json", encoding="utf-8")
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)

    def test_ledger_without_entries_list_is_precondition_failure(self):
        ledger, md = self.build([self.entry()], source=self.SOURCE)
        ledger.write_text(json.dumps({"schema_version": "1.1.0"}), encoding="utf-8")
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)

    def test_missing_ledger_file_is_precondition_failure(self):
        ledger, md = self.build([self.entry()], source=self.SOURCE)
        ledger.unlink()
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)

    def test_duplicate_md_row_is_precondition_failure(self):
        entries = [self.entry()]
        row = _md_row(162, "rust/path-injection", "src/handler.rs", 3)
        ledger, md = self.build(entries, source=self.SOURCE, md_rows=[row, row])
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)


class RealLedgerTest(unittest.TestCase):
    """O ledger versionado do repo tem que passar — a guarda nasce verde."""

    def test_repo_ledger_is_consistent(self):
        import sys

        argv = sys.argv
        sys.argv = ["check-ledger-anchors.py", "--root", str(REPO_ROOT)]
        cwd = Path.cwd()
        import os

        os.chdir(REPO_ROOT)
        try:
            self.assertEqual(MOD.main(), MOD.EXIT_OK)
        except SystemExit as exc:  # pragma: no cover — so falha se algo quebrar
            self.fail(f"ledger do repo falhou a precondicao: exit {exc.code}")
        finally:
            os.chdir(cwd)
            sys.argv = argv


if __name__ == "__main__":
    unittest.main(verbosity=2)
