"""Testes do upsert de comentario de PR (#1228, slice E)."""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "upsert_pr_comment.py"
_spec = importlib.util.spec_from_file_location("upsert_pr_comment", MODULE_PATH)
assert _spec and _spec.loader
up = importlib.util.module_from_spec(_spec)
sys.modules["upsert_pr_comment"] = up
_spec.loader.exec_module(up)

COV = "<!-- coverage-comment -->"
RATCHET = "<!-- quality-ratchet-comment -->"
BOT = "github-actions[bot]"


def c(id_, body, login=BOT):
    return {"id": id_, "body": body, "user": {"login": login}}


def test_escolhe_o_do_bot_com_o_marcador():
    comentarios = [c(1, "oi"), c(2, f"{COV}\nvelho"), c(3, "review")]
    assert up.escolher(comentarios, COV) == 2


def test_ignora_marcador_colado_por_outra_pessoa():
    comentarios = [c(5, f"{COV} sequestro", login="atacante")]
    assert up.escolher(comentarios, COV) is None


def test_bot_sem_marcador_nao_e_alvo():
    assert up.escolher([c(1, "outro relatorio do bot")], COV) is None


def test_varios_do_bot_edita_o_mais_recente():
    comentarios = [c(10, f"{COV} a"), c(30, f"{COV} c"), c(20, f"{COV} b")]
    assert up.escolher(comentarios, COV) == 30


def test_marcadores_de_cobertura_e_ratchet_nao_colidem():
    comentarios = [c(1, f"{COV}\ncobertura"), c(2, f"{RATCHET}\nratchet")]
    assert up.escolher(comentarios, COV) == 1
    assert up.escolher(comentarios, RATCHET) == 2
    assert COV not in RATCHET and RATCHET not in COV


def test_parse_paginas_concatenadas():
    texto = json.dumps([c(1, "a")]) + "\n" + json.dumps([c(2, "b"), c(3, "c")]) + "\n"
    assert [x["id"] for x in up.parse_paginas(texto)] == [1, 2, 3]
    assert up.parse_paginas("") == []
    assert up.parse_paginas("[]") == []


def test_garantir_marcador():
    assert up.garantir_marcador("corpo", COV) == f"{COV}\ncorpo"
    assert up.garantir_marcador(f"x {COV} y", COV) == f"x {COV} y"


class FakeGh:
    def __init__(self, existentes):
        self.existentes = existentes
        self.chamadas = []

    def __call__(self, args, stdin):
        self.chamadas.append((args, stdin))
        if "--paginate" in args:
            meio = len(self.existentes) // 2
            return json.dumps(self.existentes[:meio]) + json.dumps(self.existentes[meio:])
        return "{}"


def test_upsert_cria_quando_nao_ha():
    gh = FakeGh([c(1, "conversa", login="humano")])
    assert up.upsert("o/r", 7, COV, "corpo", gh) == "created"
    args, stdin = gh.chamadas[-1]
    assert args[:4] == ["api", "-X", "POST", "repos/o/r/issues/7/comments"]
    assert json.loads(stdin)["body"].startswith(COV)


def test_upsert_edita_no_lugar():
    gh = FakeGh([c(1, "x", login="humano"), c(9, f"{COV}\nvelho"), c(4, f"{RATCHET} r")])
    assert up.upsert("o/r", 7, COV, f"{COV}\nnovo", gh) == "updated:9"
    args, stdin = gh.chamadas[-1]
    assert args[:4] == ["api", "-X", "PATCH", "repos/o/r/issues/comments/9"]
    assert json.loads(stdin)["body"] == f"{COV}\nnovo"
    # Uma listagem e uma escrita: nada de POST extra.
    assert len(gh.chamadas) == 2


def test_dois_pushes_deixam_um_comentario():
    """O ciclo que o slice E promete: o 1o push cria, o 2o edita o mesmo."""
    estado = []

    def gh(args, stdin):
        if "--paginate" in args:
            return json.dumps(estado)
        corpo = json.loads(stdin)["body"]
        if args[2] == "POST":
            estado.append(c(len(estado) + 100, corpo))
        else:
            alvo = int(args[3].rsplit("/", 1)[1])
            for item in estado:
                if item["id"] == alvo:
                    item["body"] = corpo
        return "{}"

    assert up.upsert("o/r", 1, COV, "push 1", gh) == "created"
    assert up.upsert("o/r", 1, COV, "push 2", gh).startswith("updated:")
    assert len(estado) == 1
    assert estado[0]["body"].endswith("push 2")


def test_main_valida_argumentos(tmp_path):
    corpo = tmp_path / "c.md"
    corpo.write_text("x", encoding="utf-8")
    base = ["--repo", "o/r", "--pr", "1", "--body-file", str(corpo)]
    assert up.main(base + ["--marker", "sem-comentario-html"], runner=FakeGh([])) == 2
    assert up.main(["--repo", "semdono", "--pr", "1", "--marker", COV, "--body-file", str(corpo)], runner=FakeGh([])) == 2
    assert up.main(base + ["--marker", COV], runner=FakeGh([])) == 0

    def quebrado(args, stdin):
        raise RuntimeError("gh api falhou (1): 403")

    assert up.main(base + ["--marker", COV], runner=quebrado) == 1
