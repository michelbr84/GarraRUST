#!/usr/bin/env python3
"""Tests para check-ledger-anchors.py — a ancoragem por conteudo.

Escrito em `unittest` (stdlib) e nao no estilo de funcoes soltas de
`test_rekey_ledger.py`, de proposito: aquele arquivo precisa do pytest para
rodar e por isso nunca foi executado por nenhum job do ci.yml
(`grep -rn pytest .github/` nao retorna nada). Com `unittest.TestCase` o
arquivo roda com `python3 scripts/security/tests/test_ledger_anchors.py` em
qualquer runner, sem `pip install`, o que mantem a superficie de
supply-chain do workflow em zero — a mesma razao pela qual o
security-gate-bola.yml usa `rm -rf` inline em vez de action externa.

Cobre os casos historicos que motivaram o script — o alerta #113, que passou
tres meses descrevendo o statement errado, e o PR #882, que deslocou as cinco
ancoras de skins_handler.rs em ate 18 linhas — e os quatro criterios de aceite
da #1263, que inverteu a ancora: o `sink_snippet` identifica o statement e a
linha e **derivada** dele.

`AceiteTest` e a classe que guarda esses quatro, um teste por criterio, cada um
escrito para MORRER se o comportamento correspondente sair do checker:

1. linhas acrescentadas ACIMA do sink mantem o gate verde (era o que falhava);
2. statement ALTERADO deixa o gate vermelho pedindo reauditoria;
3. statement REMOVIDO deixa o gate vermelho;
4. `sink_snippet` com 2+ ocorrencias e erro de configuracao, nao silencio.
"""

from __future__ import annotations

import contextlib
import datetime as dt
import importlib.util
import io
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


class LedgerCase(unittest.TestCase):
    """Base: monta um ledger sintetico completo em tmp e roda o checker."""

    def setUp(self) -> None:
        self._tmp = __import__("tempfile").TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    # -- helpers ----------------------------------------------------------

    def build(
        self, entries, *, source: str, md_rows=None, extra_sources=None
    ) -> tuple[Path, Path]:
        """Escreve src/handler.rs, o ledger .json e o ledger .md."""
        src = self.root / "src"
        src.mkdir(exist_ok=True)
        (src / "handler.rs").write_text(source, encoding="utf-8")

        for rel, text in (extra_sources or {}).items():
            target = self.root / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")

        ledger = self.root / "ledger.json"
        ledger.write_text(
            json.dumps(
                {
                    "schema_version": "1.2.0",
                    # Longe no futuro de proposito: estes casos exercitam a
                    # deteccao de drift das ancoras, e um prazo vencido faria
                    # todos falharem por um motivo que nao e o que testam.
                    # A janela de re-auditoria tem sua propria classe.
                    "audit_expiration": "2099-01-01",
                    "entries": entries,
                },
                indent=2,
            ),
            encoding="utf-8",
        )

        md = self.root / "ledger.md"
        if md_rows is None:
            md_rows = [
                _md_row(e["alert_number"], e["rule_id"], e["path"], e.get("line", 0))
                for e in entries
            ]
        md.write_text(MD_HEADER + "".join(md_rows), encoding="utf-8")
        return ledger, md

    def run_check(self, ledger: Path, md: Path, *extra_argv: str) -> int:
        """Roda o checker e devolve o exit code, venha ele de return ou raise."""
        code, _, _ = self.run_check_capturing(ledger, md, *extra_argv)
        return code

    def run_check_capturing(
        self, ledger: Path, md: Path, *extra_argv: str
    ) -> tuple[int, str, str]:
        """Como `run_check`, mas devolve tambem stdout e stderr."""
        import sys

        argv = sys.argv
        sys.argv = [
            "check-ledger-anchors.py",
            "--ledger", str(ledger),
            "--md", str(md),
            "--root", str(self.root),
            *extra_argv,
        ]
        out, err = io.StringIO(), io.StringIO()
        try:
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                try:
                    code = MOD.main()
                except SystemExit as exc:
                    code = int(exc.code)
        finally:
            sys.argv = argv
        return code, out.getvalue(), err.getvalue()

    @staticmethod
    def entry(number=162, line=3, snippet="if !file_path.is_file() {", **extra):
        entry = {
            "alert_number": number,
            "rule_id": "rust/path-injection",
            "path": "src/handler.rs",
            "line": line,
            "sink_snippet": snippet,
            "disposition": "dismissed-false-positive",
            "dismissed_reason": "false_positive",
        }
        entry.update(extra)
        return entry

    @staticmethod
    def ledger_line(ledger: Path, number: int) -> int | None:
        entries = json.loads(ledger.read_text(encoding="utf-8"))["entries"]
        for entry in entries:
            if entry["alert_number"] == number:
                return entry.get("line")
        raise AssertionError(f"entrada #{number} nao esta no ledger")

    SOURCE = (
        "fn get(name: &str) {\n"
        "    let file_path = dir.join(name);\n"
        "    if !file_path.is_file() {\n"
        "        return not_found();\n"
        "    }\n"
        "}\n"
    )

    # Duas ocorrencias identicas do mesmo statement, em funcoes diferentes —
    # a forma real do ledger deste repo (skins_handler.rs, skills_handler.rs,
    # whatsapp/api.rs: 13 das 30 entradas no dia da #1263).
    AMBIGUOUS_SOURCE = (
        "fn get(name: &str) {\n"
        "    let file_path = dir.join(name);\n"
        "    if !file_path.is_file() {\n"
        "        return not_found();\n"
        "    }\n"
        "}\n"
        "\n"
        "pub async fn delete(name: &str) {\n"
        "    let file_path = dir.join(name);\n"
        "    if !file_path.is_file() {\n"
        "        return not_found();\n"
        "    }\n"
        "}\n"
    )


class AceiteTest(LedgerCase):
    """Os quatro criterios de aceite da #1263, um teste cada.

    Cada um morre se o comportamento correspondente sair do
    `check-ledger-anchors.py` — as mutacoes estao documentadas no PR.
    """

    def test_aceite_1_linhas_acima_do_sink_mantem_o_gate_verde(self):
        """ACEITE 1 — o caso que motivou a issue.

        18 linhas entram acima do sink (a forma exata do PR #882) e o
        statement suprimido nao e tocado. Antes da #1263 isso era exit 6; a
        entrada #113 andou duas vezes assim no mesmo PR (#1252). Agora a linha
        e derivada do conteudo e o ledger e reescrito.

        MORRE SE: a busca por conteudo voltar a ser leitura posicional
        (`candidates = [ledger_line] if lines[ledger_line-1] == snippet`).
        """
        source = "// helper novo\n" * 18 + self.SOURCE
        ledger, md = self.build([self.entry(line=3)], source=source)

        code, out, _ = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_OK)
        # A ancora esta 18 linhas abaixo, e o ledger aprendeu isso sozinho.
        self.assertEqual(self.ledger_line(ledger, 162), 21)
        self.assertIn("linha derivada 21 (antes 3)", out)
        self.assertIn("src/handler.rs:21", md.read_text(encoding="utf-8"))

    def test_aceite_2_statement_alterado_pede_reauditoria(self):
        """ACEITE 2 — mexer no statement suprimido fica vermelho.

        MORRE SE: o ramo `if not candidates` parar de dizer REAUDITORIA.
        """
        source = self.SOURCE.replace(
            "if !file_path.is_file() {", "if !file_path.is_file() && allow_any {"
        )
        ledger, md = self.build([self.entry()], source=source)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_DRIFT)
        self.assertIn("REAUDITORIA", err)
        self.assertIn("sink_snippet nao aparece mais no arquivo", err)

    def test_aceite_3_statement_removido_fica_vermelho(self):
        """ACEITE 3 — apagar o statement suprimido fica vermelho.

        MORRE SE: o ramo `if not candidates` deixar de registrar drift.
        """
        source = self.SOURCE.replace("    if !file_path.is_file() {\n", "")
        ledger, md = self.build([self.entry()], source=source)

        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_aceite_4_snippet_ambiguo_e_erro_de_configuracao(self):
        """ACEITE 4 — 2+ ocorrencias nao podem virar escolha silenciosa.

        MORRE SE: a ambiguidade sem `disambiguator` voltar a resolver para
        `candidates[0]` em vez de acusar.
        """
        ledger, md = self.build([self.entry()], source=self.AMBIGUOUS_SOURCE)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_PRECONDITION)
        self.assertIn("ambiguo", err)
        self.assertIn("disambiguator", err)
        # Nao adivinha: as duas ocorrencias aparecem na mensagem.
        self.assertIn("3, 10", err)


class LedgerAnchorsTest(LedgerCase):
    """Ancoragem, desambiguacao e o espelho no .md."""

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

    def test_entry_without_line_gets_one(self):
        """`line` e derivado: entrada sem ele e valida, e o checker o escreve."""
        entry = self.entry()
        del entry["line"]
        rows = [_md_row(162, "rust/path-injection", "src/handler.rs", 3)]
        ledger, md = self.build([entry], source=self.SOURCE, md_rows=rows)

        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)
        self.assertEqual(self.ledger_line(ledger, 162), 3)

    def test_stale_line_is_repaired_instead_of_failing(self):
        """Era `test_anchor_pointing_at_a_different_statement_is_drift`.

        O ledger aponta :4, o snippet esta em :3. Com a ancora no conteudo isso
        e posicao velha, nao drift — e o conserto e mecanico.
        """
        rows = [_md_row(162, "rust/path-injection", "src/handler.rs", 4)]
        ledger, md = self.build(
            [self.entry(line=4)], source=self.SOURCE, md_rows=rows
        )

        code, out, _ = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_OK)
        self.assertEqual(self.ledger_line(ledger, 162), 3)
        self.assertIn("linha derivada 3 (antes 4)", out)

    def test_line_out_of_range_is_repaired_by_derivation(self):
        """Era precondicao (exit 5) quando a linha mandava; hoje e so ruido."""
        ledger, md = self.build([self.entry(line=9999)], source=self.SOURCE)

        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)
        self.assertEqual(self.ledger_line(ledger, 162), 3)

    def test_no_rewrite_reports_without_touching_the_ledger(self):
        """`--no-rewrite`: para arvore read-only, e para o proprio teste do repo."""
        source = "// linha nova\n" + self.SOURCE
        ledger, md = self.build([self.entry(line=3)], source=source)
        before_json = ledger.read_text(encoding="utf-8")
        before_md = md.read_text(encoding="utf-8")

        code, out, _ = self.run_check_capturing(ledger, md, "--no-rewrite")

        self.assertEqual(code, MOD.EXIT_OK)
        self.assertIn("linha derivada 4 (antes 3)", out)
        self.assertIn("nada foi escrito", out)
        self.assertEqual(ledger.read_text(encoding="utf-8"), before_json)
        self.assertEqual(md.read_text(encoding="utf-8"), before_md)

    # -- desambiguacao -----------------------------------------------------

    def test_disambiguator_picks_the_occurrence_inside_the_named_function(self):
        entry = self.entry(
            line=10, disambiguator={"kind": "function", "value": "delete"}
        )
        rows = [_md_row(162, "rust/path-injection", "src/handler.rs", 10)]
        ledger, md = self.build(
            [entry], source=self.AMBIGUOUS_SOURCE, md_rows=rows
        )

        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)
        self.assertEqual(self.ledger_line(ledger, 162), 10)

    def test_disambiguator_survives_lines_added_above(self):
        """A ancora auxiliar tambem e conteudo — ela nao pode drifar por posicao."""
        source = "// cabecalho novo\n" * 7 + self.AMBIGUOUS_SOURCE
        entry = self.entry(
            line=10, disambiguator={"kind": "function", "value": "delete"}
        )
        rows = [_md_row(162, "rust/path-injection", "src/handler.rs", 10)]
        ledger, md = self.build([entry], source=source, md_rows=rows)

        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)
        self.assertEqual(self.ledger_line(ledger, 162), 17)

    def test_disambiguator_matching_nothing_is_drift(self):
        """A fn que continha o sink nao contem mais: reauditoria, nao chute."""
        entry = self.entry(
            line=10, disambiguator={"kind": "function", "value": "purge"}
        )
        ledger, md = self.build([entry], source=self.AMBIGUOUS_SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_disambiguator_still_ambiguous_is_config_error(self):
        """Duas ocorrencias na MESMA fn: a ancora auxiliar nao resolve nada."""
        source = (
            "fn get(name: &str) {\n"
            "    if !file_path.is_file() {\n"
            "        return not_found();\n"
            "    }\n"
            "    if !file_path.is_file() {\n"
            "        return not_found();\n"
            "    }\n"
            "}\n"
        )
        entry = self.entry(
            line=2, disambiguator={"kind": "function", "value": "get"}
        )
        ledger, md = self.build([entry], source=source)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_PRECONDITION)
        self.assertIn("mais especifica", err)

    def test_unknown_disambiguator_kind_is_config_error(self):
        entry = self.entry(
            line=10, disambiguator={"kind": "occurrence", "value": "2"}
        )
        ledger, md = self.build([entry], source=self.AMBIGUOUS_SOURCE)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_PRECONDITION)
        self.assertIn("desconhecido", err)

    def test_stale_disambiguator_on_a_unique_sink_is_only_a_warning(self):
        """Ocorrencia unica: `fn` renomeada nao derruba o gate, mas avisa."""
        entry = self.entry(disambiguator={"kind": "function", "value": "fetch"})
        ledger, md = self.build([entry], source=self.SOURCE)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_OK)
        self.assertIn("aviso:", err)
        self.assertIn("ancora auxiliar", err)

    # -- citacoes dentro da justificativa ---------------------------------

    def test_citation_in_range_passes(self):
        entry = self.entry(
            justification="o guard esta em src/handler.rs:2, antes do sink."
        )
        ledger, md = self.build([entry], source=self.SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)

    def test_citation_out_of_range_is_drift(self):
        """O drift silencioso do #113: a justificativa citava :630, era :824."""
        entry = self.entry(
            justification="o guard esta em src/handler.rs:630, antes do sink."
        )
        ledger, md = self.build([entry], source=self.SOURCE)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_DRIFT)
        self.assertIn("fora de faixa", err)

    def test_citation_to_a_missing_repo_file_is_drift(self):
        entry = self.entry(
            justification="ver crates/foo/src/sumiu.rs:12 para o guard."
        )
        ledger, md = self.build([entry], source=self.SOURCE)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_DRIFT)
        self.assertIn("nao resolve", err)

    def test_citation_resolved_against_the_entry_directory_passes(self):
        entry = self.entry(justification="`validate` vive em guard.rs:2.")
        ledger, md = self.build(
            [entry],
            source=self.SOURCE,
            extra_sources={"src/guard.rs": "fn validate() {\n    true\n}\n"},
        )
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)

    def test_unresolvable_bare_filename_is_only_a_warning(self):
        """Prosa nao e caminho: nome solto que nao resolve avisa, nao derruba."""
        entry = self.entry(justification="comparar com o antigo legado.rs:99.")
        ledger, md = self.build([entry], source=self.SOURCE)

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_OK)
        self.assertIn("aviso:", err)

    def test_pr_and_squash_references_are_not_citations(self):
        """`#1252` e `613510d` nao sao `arquivo:linha` — nada a validar."""
        entry = self.entry(
            justification=(
                "GAR-490 PR A (PR #111, squash 613510d): regressao em "
                "tests/skills_test.rs::get_skill_rejects_path_traversal."
            )
        )
        ledger, md = self.build([entry], source=self.SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)

    # -- drift (exit 6) ----------------------------------------------------

    def test_md_pointing_to_another_file_is_drift(self):
        entries = [self.entry()]
        rows = [_md_row(162, "rust/path-injection", "src/outro.rs", 3)]
        ledger, md = self.build(entries, source=self.SOURCE, md_rows=rows)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_md_line_out_of_sync_is_rewritten_not_drift(self):
        """Era exit 6. A coluna `File:line` do .md e posicao — logo, derivada.

        Se ela continuasse sendo drift, o .md seria o novo tripwire posicional
        e a #1263 nao teria resolvido nada.
        """
        entries = [self.entry()]
        rows = [_md_row(162, "rust/path-injection", "src/handler.rs", 99)]
        ledger, md = self.build(entries, source=self.SOURCE, md_rows=rows)

        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_OK)
        self.assertIn("src/handler.rs:3", md.read_text(encoding="utf-8"))

    def test_md_rule_id_out_of_sync_is_drift(self):
        entries = [self.entry()]
        rows = [_md_row(162, "rust/cleartext-logging", "src/handler.rs", 3)]
        ledger, md = self.build(entries, source=self.SOURCE, md_rows=rows)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    def test_entry_absent_from_md_is_drift(self):
        ledger, md = self.build([self.entry()], source=self.SOURCE, md_rows=[])
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_DRIFT)

    # -- precondicao (exit 5) ---------------------------------------------

    def test_missing_sink_snippet_is_precondition_failure(self):
        entry = self.entry()
        del entry["sink_snippet"]
        ledger, md = self.build([entry], source=self.SOURCE)
        self.assertEqual(self.run_check(ledger, md), MOD.EXIT_PRECONDITION)

    def test_empty_sink_snippet_is_precondition_failure(self):
        ledger, md = self.build([self.entry(snippet="   ")], source=self.SOURCE)
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
        ledger.write_text(json.dumps({"schema_version": "1.2.0"}), encoding="utf-8")
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

    def test_every_ambiguous_entry_is_reported_in_one_run(self):
        """13 das 30 entradas precisaram de `disambiguator` na migracao.

        Abortar na primeira custaria 13 rodadas de CI para descobrir as 13 —
        e a fila deste repo esta em ~110 jobs por push.
        """
        entries = [self.entry(number=161), self.entry(number=162)]
        rows = [
            _md_row(161, "rust/path-injection", "src/handler.rs", 3),
            _md_row(162, "rust/path-injection", "src/handler.rs", 10),
        ]
        ledger, md = self.build(
            entries, source=self.AMBIGUOUS_SOURCE, md_rows=rows
        )

        code, _, err = self.run_check_capturing(ledger, md)

        self.assertEqual(code, MOD.EXIT_PRECONDITION)
        self.assertIn("#161", err)
        self.assertIn("#162", err)
        self.assertIn("2 entrada(s)", err)


class AnchorPrimitivesTest(unittest.TestCase):
    """As primitivas da ancoragem por conteudo, sem I/O."""

    SOURCE = [
        "pub(crate) async fn handler(name: &str) {",
        "    let p = dir.join(name);",
        "    if !p.is_file() {",
        "        return;",
        "    }",
        "}",
        "",
        "fn helper() {",
        "    if !p.is_file() {",
        "    }",
        "}",
    ]

    def test_find_snippet_lines_returns_every_occurrence(self):
        self.assertEqual(
            MOD.find_snippet_lines(self.SOURCE, "if !p.is_file() {"), [3, 9]
        )

    def test_find_snippet_lines_ignores_indentation(self):
        self.assertEqual(
            MOD.find_snippet_lines(["        let x = 1;"], "let x = 1;"), [1]
        )

    def test_find_snippet_lines_is_empty_when_the_statement_changed(self):
        self.assertEqual(MOD.find_snippet_lines(self.SOURCE, "if !p.is_dir() {"), [])

    def test_enclosing_function_walks_up(self):
        self.assertEqual(MOD.enclosing_function(self.SOURCE, 3), "handler")
        self.assertEqual(MOD.enclosing_function(self.SOURCE, 9), "helper")

    def test_enclosing_function_is_none_at_the_top_of_the_file(self):
        self.assertIsNone(MOD.enclosing_function(["// comentario"], 1))


class ExpirationTest(unittest.TestCase):
    """A janela de re-auditoria de 90 dias (§3 regra 4).

    Ate 2026-08-30 o campo `audit_expiration` nao era lido por nada — nem
    script, nem workflow, nem teste. Ficou vencido 29 dias sem que nenhum
    build reclamasse. Estes testes existem para que isso nao se repita.
    """

    def test_a_future_date_passes(self):
        ok, msg = MOD.check_expiration("2026-12-01", dt.date(2026, 8, 30))
        self.assertTrue(ok)
        self.assertIn("2026-12-01", msg)

    def test_today_is_still_inside_the_window(self):
        # A data e o ultimo dia valido, nao o primeiro dia vencido.
        ok, _ = MOD.check_expiration("2026-08-30", dt.date(2026, 8, 30))
        self.assertTrue(ok)

    def test_a_past_date_fails_and_says_how_overdue(self):
        ok, msg = MOD.check_expiration("2026-08-01", dt.date(2026, 8, 30))
        self.assertFalse(ok)
        self.assertIn("29 dia", msg)

    def test_a_missing_field_fails(self):
        # Nao e "sem data, sem problema": um ledger sem prazo e um ledger sem
        # re-auditoria, que e justamente o que a regra impede.
        for missing in (None, ""):
            with self.subTest(missing=missing):
                ok, _ = MOD.check_expiration(missing, dt.date(2026, 8, 30))
                self.assertFalse(ok)

    def test_a_malformed_date_fails_instead_of_crashing(self):
        ok, msg = MOD.check_expiration("01/08/2026", dt.date(2026, 8, 30))
        self.assertFalse(ok)
        self.assertIn("ISO", msg)


class RealLedgerTest(unittest.TestCase):
    """O ledger versionado do repo tem que passar — a guarda nasce verde.

    Roda com `--no-rewrite`: um teste nao mexe em arquivo versionado. Se a
    posicao derivada divergir do commitado, o teste segue verde e a saida diz
    qual entrada andou — quem roda o checker a mao (ou o CI) reescreve.
    """

    def test_repo_ledger_is_consistent(self):
        import sys

        argv = sys.argv
        sys.argv = ["check-ledger-anchors.py", "--root", str(REPO_ROOT), "--no-rewrite"]
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
