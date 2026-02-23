"""
Script para futura atualização da documentação do GarraIA para Português.
O GarraIA é 100% em português!
"""

import sys
import io

# Corrige encoding para suportar emojis/unicode no Windows
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

CONFIGURACAO_ATUAL = {
    "title": "GarraIA Documentation",
    "language": "en",
    "authors": "GarraIA Contributors",
}

CONFIGURACAO_NOVA = {
    "title": "Documentação do GarraIA",
    "language": "pt-BR",
    "authors": "Colaboradores do GarraIA",
}

LARGURA_SEPARADOR = 50


def exibir_cabecalho(titulo):
    """Exibe um cabeçalho formatado com separadores."""
    print("=" * LARGURA_SEPARADOR)
    print(titulo)
    print("=" * LARGURA_SEPARADOR)
    print()


def obter_diferencas(antes, depois):
    """Compara dois dicionários e retorna uma lista de diferenças."""
    todas_chaves = sorted(set(antes.keys()) | set(depois.keys()))
    diferencas = []

    for chave in todas_chaves:
        valor_antigo = antes.get(chave, "(nao definido)")
        valor_novo = depois.get(chave, "(nao definido)")
        diferencas.append({
            "campo": chave,
            "antes": valor_antigo,
            "depois": valor_novo,
            "alterado": valor_antigo != valor_novo,
        })

    return diferencas


def exibir_diferencas(diferencas):
    """Exibe a lista de diferenças formatada no terminal."""
    for diff in diferencas:
        status = " *" if diff["alterado"] else ""
        print(f"  {diff['campo']}:{status}")
        print(f"    Antes:  {diff['antes']}")
        print(f"    Depois: {diff['depois']}")
        print()


def exibir_comparacao():
    """Orquestra a exibição completa da comparação."""
    exibir_cabecalho("GarraIA - Atualizacao para Portugues")

    diferencas = obter_diferencas(CONFIGURACAO_ATUAL, CONFIGURACAO_NOVA)
    exibir_diferencas(diferencas)

    total_alterados = sum(1 for d in diferencas if d["alterado"])
    print(f"  Total de campos alterados: {total_alterados}/{len(diferencas)}")
    print()

    exibir_cabecalho("Atualizacao pendente para uma versao futura!")


if __name__ == "__main__":
    exibir_comparacao()
