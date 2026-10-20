#!/usr/bin/env python3
"""Testes para check_deny_exceptions_expiry.py.

Escrito em `unittest.TestCase` (stdlib), como `test_ledger_anchors.py`, para
rodar com `python3 scripts/security/tests/test_check_deny_exceptions_expiry.py`
em qualquer runner, sem `pip install` — a superficie de supply-chain do
workflow fica em zero.

Cobre os tres casos exigidos pela autorizacao da excecao temporaria do
`deny.toml` (#1662 / #1667):

* excecao **valida** (dentro do prazo) — nao pode falhar;
* excecao **vencida** — precisa falhar, uma vez por advisory;
* excecao **sem prazo** — precisa falhar, porque nesse caso a excecao vira
  permanente sem ninguem perceber.

Mais: entrada em forma de string (a excecao antiga de `unmaintained`) nao e
verificada pelo script, e o caso de borda do dia do prazo vale como valido.
"""

import datetime as dt
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from check_deny_exceptions_expiry import PADRAO_PRAZO, verificar  # noqa: E402


def toml_com(entradas: str) -> str:
    """Monta um deny.toml minimo com as entradas informadas."""
    return f'[advisories]\nignore = [\n{entradas}\n]\n'


def entrada(id_: str, reason: str) -> str:
    return f'  {{ id = "{id_}", reason = "{reason}" }},'


class ExcecaoValidaTest(unittest.TestCase):
    def test_dentro_do_prazo_nao_falha(self):
        conteudo = toml_com(entrada("RUSTSEC-2026-0370", "unmaintained. EXPIRA 2026-11-18."))
        self.assertEqual(verificar(conteudo, dt.date(2026, 10, 19)), [])

    def test_dia_do_prazo_ainda_vale(self):
        # Borda: o prazo e ate aquele dia.
        conteudo = toml_com(entrada("RUSTSEC-2026-0370", "unmaintained. EXPIRA 2026-11-18."))
        self.assertEqual(verificar(conteudo, dt.date(2026, 11, 18)), [])


class ExcecaoVencidaTest(unittest.TestCase):
    def test_depois_do_prazo_falha(self):
        conteudo = toml_com(entrada("RUSTSEC-2026-0370", "unmaintained. EXPIRA 2026-11-18."))
        problemas = verificar(conteudo, dt.date(2026, 11, 19))
        self.assertEqual(len(problemas), 1)
        self.assertIn("RUSTSEC-2026-0370", problemas[0])
        self.assertIn("VENCIDA", problemas[0])

    def test_uma_linha_por_advisory_vencido(self):
        # As tres entradas do deny.toml real: todas vencem no mesmo dia.
        entradas = "\n".join(
            entrada(i, f"unmaintained. EXPIRA 2026-11-18.")
            for i in ("RUSTSEC-2026-0370", "RUSTSEC-2026-0371", "RUSTSEC-2026-0372")
        )
        problemas = verificar(toml_com(entradas), dt.date(2026, 11, 19))
        self.assertEqual(len(problemas), 3)
        for id_ in ("RUSTSEC-2026-0370", "RUSTSEC-2026-0371", "RUSTSEC-2026-0372"):
            self.assertTrue(any(id_ in p for p in problemas), id_)


class ExcecaoSemPrazoTest(unittest.TestCase):
    def test_sem_expira_falha(self):
        conteudo = toml_com(entrada("RUSTSEC-2026-0370", "unmaintained, sem prazo registrado."))
        problemas = verificar(conteudo, dt.date(2026, 10, 19))
        self.assertEqual(len(problemas), 1)
        self.assertIn("sem prazo", problemas[0])

    def test_data_invalida_falha(self):
        conteudo = toml_com(entrada("RUSTSEC-2026-0370", "EXPIRA 2026-13-45."))
        problemas = verificar(conteudo, dt.date(2026, 10, 19))
        self.assertEqual(len(problemas), 1)
        self.assertIn("invalida", problemas[0])


class EntradaComoStringTest(unittest.TestCase):
    def test_string_nao_e_verificada(self):
        # Excecoes antigas de `unmaintained` sao strings sem prazo: fora do
        # escopo do verificador (nao viram permanentes por acidente novo).
        conteudo = '[advisories]\nignore = [\n  "RUSTSEC-2024-0411",\n]\n'
        self.assertEqual(verificar(conteudo, dt.date(2026, 10, 19)), [])


class PadraoTest(unittest.TestCase):
    def test_padrao_captura_a_data(self):
        m = PADRAO_PRAZO.search("algo. EXPIRA 2026-11-18. mais texto")
        self.assertIsNotNone(m)
        self.assertEqual(m.group(1), "2026-11-18")

    def test_padrao_ignora_texto_sem_data(self):
        self.assertIsNone(PADRAO_PRAZO.search("sem data aqui"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
