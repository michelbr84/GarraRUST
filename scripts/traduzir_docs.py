"""
GarraIA - Script de traducao da documentacao para Portugues (pt-BR).

Traduz automaticamente os arquivos de navegacao do mdBook
para manter o projeto 100% em portugues.
"""

import sys
import io
import os
import re
import shutil
from datetime import datetime

# Corrige encoding para suportar unicode no Windows
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

# === CONSTANTES ===

CAMINHO_DOCS = os.path.join(os.path.dirname(__file__), "..", "docs")
CAMINHO_SUMMARY = os.path.join(CAMINHO_DOCS, "src", "SUMMARY.md")
CAMINHO_BACKUP = os.path.join(CAMINHO_DOCS, "src", "SUMMARY.md.bak")

MAPA_TRADUCAO = {
    "Summary": "Sumario",
    "Introduction": "Introducao",
    "Getting Started": "Primeiros Passos",
    "Architecture": "Arquitetura",
    "Decision Records": "Registros de Decisao",
    "Channels": "Canais",
    "iMessage Setup": "Configuracao do iMessage",
    "Providers": "Provedores",
    "Tools": "Ferramentas",
    "MCP": "MCP",
    "Security": "Seguranca",
    "Attack Surfaces": "Superficies de Ataque",
    "Checklist": "Lista de Verificacao",
    "Plugins": "Plugins",
}

LARGURA_SEPARADOR = 55


# === FUNCOES DE TRADUCAO ===

def criar_backup(caminho_origem, caminho_backup):
    """Cria uma copia de seguranca do arquivo original."""
    if not os.path.exists(caminho_origem):
        raise FileNotFoundError(f"Arquivo nao encontrado: {caminho_origem}")
    shutil.copy2(caminho_origem, caminho_backup)
    return True


def carregar_arquivo(caminho):
    """Carrega o conteudo de um arquivo texto."""
    with open(caminho, "r", encoding="utf-8") as arquivo:
        return arquivo.read()


def salvar_arquivo(caminho, conteudo):
    """Salva conteudo em um arquivo texto."""
    with open(caminho, "w", encoding="utf-8") as arquivo:
        arquivo.write(conteudo)
    return True


def traduzir_linha(linha, mapa):
    """
    Traduz os textos de exibicao de uma linha do SUMMARY.md.
    Mantem a estrutura Markdown intacta (links, indentacao).
    
    Exemplo:
      '- [Getting Started](./getting_started.md)'
      vira
      '- [Primeiros Passos](./getting_started.md)'
    """
    for original, traduzido in mapa.items():
        # Busca o texto dentro de colchetes: [Texto](link)
        padrao = re.compile(r'\[' + re.escape(original) + r'\]')
        if padrao.search(linha):
            linha = padrao.sub(f'[{traduzido}]', linha)
    
    # Traduz titulo de primeiro nivel (# Summary -> # Sumario)
    if linha.startswith("# "):
        for original, traduzido in mapa.items():
            if original in linha:
                linha = linha.replace(original, traduzido)
    
    return linha


def traduzir_conteudo(conteudo, mapa):
    """Traduz todas as linhas do conteudo usando o mapa de traducao."""
    linhas = conteudo.split("\n")
    linhas_traduzidas = [traduzir_linha(linha, mapa) for linha in linhas]
    return "\n".join(linhas_traduzidas)


def obter_estatisticas(conteudo_original, conteudo_traduzido):
    """Calcula estatisticas da traducao realizada."""
    linhas_originais = conteudo_original.split("\n")
    linhas_traduzidas = conteudo_traduzido.split("\n")
    
    total_linhas = len(linhas_originais)
    linhas_alteradas = sum(
        1 for orig, trad in zip(linhas_originais, linhas_traduzidas)
        if orig != trad
    )
    
    return {
        "total_linhas": total_linhas,
        "linhas_alteradas": linhas_alteradas,
        "linhas_mantidas": total_linhas - linhas_alteradas,
        "termos_no_mapa": len(MAPA_TRADUCAO),
        "data_hora": datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
    }


# === FUNCOES DE EXIBICAO ===

def exibir_cabecalho(titulo):
    """Exibe um cabecalho formatado."""
    print("=" * LARGURA_SEPARADOR)
    print(f"  {titulo}")
    print("=" * LARGURA_SEPARADOR)
    print()


def exibir_estatisticas(estatisticas):
    """Exibe as estatisticas da traducao."""
    print("  Estatisticas da traducao:")
    print(f"    Total de linhas:      {estatisticas['total_linhas']}")
    print(f"    Linhas traduzidas:    {estatisticas['linhas_alteradas']}")
    print(f"    Linhas mantidas:      {estatisticas['linhas_mantidas']}")
    print(f"    Termos no dicionario: {estatisticas['termos_no_mapa']}")
    print(f"    Data/hora:            {estatisticas['data_hora']}")
    print()


def exibir_preview(conteudo):
    """Exibe uma previa do conteudo traduzido."""
    print("  Preview do SUMMARY.md traduzido:")
    print("  " + "-" * (LARGURA_SEPARADOR - 2))
    for linha in conteudo.split("\n"):
        print(f"  {linha}")
    print("  " + "-" * (LARGURA_SEPARADOR - 2))
    print()


# === FUNCAO PRINCIPAL ===

def executar_traducao(modo_seco=False):
    """
    Executa o fluxo completo de traducao.
    
    Args:
        modo_seco: Se True, apenas exibe o resultado sem salvar.
    """
    exibir_cabecalho("GarraIA - Traducao da Documentacao")

    # Carregar original
    conteudo_original = carregar_arquivo(CAMINHO_SUMMARY)
    
    # Traduzir
    conteudo_traduzido = traduzir_conteudo(conteudo_original, MAPA_TRADUCAO)
    
    # Estatisticas
    estatisticas = obter_estatisticas(conteudo_original, conteudo_traduzido)
    exibir_estatisticas(estatisticas)
    
    # Preview
    exibir_preview(conteudo_traduzido)
    
    if modo_seco:
        print("  [MODO SECO] Nenhum arquivo foi alterado.")
        print()
        return conteudo_traduzido
    
    # Backup e salvar
    criar_backup(CAMINHO_SUMMARY, CAMINHO_BACKUP)
    print(f"  Backup criado: SUMMARY.md.bak")
    
    salvar_arquivo(CAMINHO_SUMMARY, conteudo_traduzido)
    print(f"  Arquivo salvo: SUMMARY.md")
    print()
    
    exibir_cabecalho("Traducao concluida com sucesso!")
    
    return conteudo_traduzido


if __name__ == "__main__":
    modo_seco = "--seco" in sys.argv
    executar_traducao(modo_seco=modo_seco)
