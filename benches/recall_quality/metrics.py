#!/usr/bin/env python3
"""Metricas de qualidade de recall a partir da saida crua do `run.sh` (#958).

Determinístico e sem chamada de IA: mesma entrada, mesma saida. A separacao
entre *coletar* (o `run.sh`, que fala com o binario) e *pontuar* (este arquivo,
que so le JSON) e o que permite repontuar uma coleta antiga com uma metrica
nova, sem rodar os modelos de novo.

Uso:
    python3 metrics.py results/<data>-<host>/raw.json
"""

import json
import sys
from collections import defaultdict

# Os cortes reportados. `k=1` diz "acertou de primeira", que e o que o usuario
# sente; `k=5` diz se a resposta estava ao alcance do modelo mas mal ordenada.
CORTES = (1, 3, 5, 10)


def recall_at_k(esperados, obtidos, k):
    """Fracao dos documentos esperados que aparece nos `k` primeiros.

    Consulta sem esperados (o grupo `ruido-puro`) nao tem recall definido —
    dividir por zero seria inventar um numero. Ela e pontuada em separado,
    pela `taxa_de_ruido`.
    """
    if not esperados:
        return None
    topo = set(obtidos[:k])
    return len(topo & set(esperados)) / len(esperados)


def precision_at_k(esperados, obtidos, k):
    """Fracao dos `k` primeiros que era esperada.

    O denominador e `min(k, len(obtidos))` e nao `k`: quando a memoria tem
    menos de `k` entradas, dividir por `k` puniria o sistema por uma escassez
    que nao e erro dele.
    """
    if not esperados:
        return None
    topo = obtidos[:k]
    if not topo:
        return 0.0
    return len(set(topo) & set(esperados)) / len(topo)


def reciprocal_rank(esperados, obtidos):
    """1/posicao do primeiro acerto, ou 0 se nenhum apareceu."""
    if not esperados:
        return None
    for i, doc in enumerate(obtidos, start=1):
        if doc in esperados:
            return 1.0 / i
    return 0.0


def taxa_de_ruido(esperados, obtidos, k):
    """Para consulta que **nao deve** casar com nada: quanto veio mesmo assim.

    A issue nomeia este caso: "quem e Michel" devolveu "oi" entre os top-K. Um
    benchmark que so mede acerto daria nota cheia a um sistema que devolve tudo
    para tudo.
    """
    if esperados:
        return None
    return len(obtidos[:k]) / k if k else 0.0


def media(valores):
    limpos = [v for v in valores if v is not None]
    return sum(limpos) / len(limpos) if limpos else None


def fmt(v):
    return "  n/a" if v is None else f"{v:5.3f}"


def main(caminho):
    with open(caminho, encoding="utf-8") as f:
        bruto = json.load(f)

    consultas = bruto["consultas"]
    por_grupo = defaultdict(list)
    globais = defaultdict(list)

    for c in consultas:
        esperados = c["esperados"]
        obtidos = c["obtidos"]
        linha = {"rr": reciprocal_rank(esperados, obtidos)}
        for k in CORTES:
            linha[f"r@{k}"] = recall_at_k(esperados, obtidos, k)
            linha[f"p@{k}"] = precision_at_k(esperados, obtidos, k)
            linha[f"ruido@{k}"] = taxa_de_ruido(esperados, obtidos, k)
        por_grupo[c["grupo"]].append(linha)
        globais["todas"].append(linha)

    print(f"# Recall quality — {bruto.get('modelo', 'modelo desconhecido')}")
    print(f"# provider={bruto.get('provider')}  busca={bruto.get('modo_de_busca')}")
    print(f"# documentos={bruto.get('documentos')}  consultas={len(consultas)}")
    print()

    cabecalho = ["grupo", "n", "MRR"]
    cabecalho += [f"r@{k}" for k in CORTES]
    cabecalho += [f"p@{k}" for k in CORTES]
    print(f"{cabecalho[0]:<20} {cabecalho[1]:>3} " + " ".join(f"{h:>5}" for h in cabecalho[2:]))
    print("-" * (20 + 4 + 6 * (len(cabecalho) - 2)))

    def imprime(nome, linhas):
        vals = [fmt(media([l["rr"] for l in linhas]))]
        vals += [fmt(media([l[f"r@{k}"] for l in linhas])) for k in CORTES]
        vals += [fmt(media([l[f"p@{k}"] for l in linhas])) for k in CORTES]
        print(f"{nome:<20} {len(linhas):>3} " + " ".join(vals))

    for nome in sorted(por_grupo):
        imprime(nome, por_grupo[nome])
    print("-" * (20 + 4 + 6 * (len(cabecalho) - 2)))
    imprime("TODAS", globais["todas"])

    # O ruido sai separado porque ele nao e "acerto": e o oposto, e misturar as
    # duas escalas na mesma tabela faria um numero alto parecer bom.
    ruidosas = [l for g in por_grupo.values() for l in g if l["ruido@5"] is not None]
    if ruidosas:
        print()
        print("Consultas que NAO deviam casar com nada (menor e melhor):")
        for k in CORTES:
            print(f"  ruido@{k}: {fmt(media([l[f'ruido@{k}'] for l in ruidosas]))}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__)
        raise SystemExit(2)
    main(sys.argv[1])
