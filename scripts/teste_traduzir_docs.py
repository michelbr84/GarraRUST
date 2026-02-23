"""
Modulo de testes para o script traduzir_docs.py
GarraIA - 100% em portugues!
"""

import unittest
import sys
import os
import io
import tempfile
import shutil

# Adiciona o diretorio scripts ao path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from traduzir_docs import (
    MAPA_TRADUCAO,
    LARGURA_SEPARADOR,
    traduzir_linha,
    traduzir_conteudo,
    obter_estatisticas,
    criar_backup,
    carregar_arquivo,
    salvar_arquivo,
    exibir_cabecalho,
    exibir_estatisticas,
    exibir_preview,
)


# === CONTEUDO DE EXEMPLO PARA TESTES ===

SUMMARY_INGLES = """# Summary

- [Introduction](./README.md)
- [Getting Started](./getting_started.md)
- [Architecture](./architecture.md)
  - [Decision Records](./adr/README.md)
- [Channels](./channels.md)
  - [iMessage Setup](./channels/imessage.md)
- [Providers](./providers.md)
- [Tools](./tools.md)
- [MCP](./mcp.md)
- [Security](./security.md)
  - [Architecture](./security/architecture.md)
  - [Attack Surfaces](./security/attack-surfaces.md)
  - [Checklist](./security/checklist.md)
- [Plugins](./plugins.md)"""

SUMMARY_PORTUGUES_ESPERADO = """# Sumario

- [Introducao](./README.md)
- [Primeiros Passos](./getting_started.md)
- [Arquitetura](./architecture.md)
  - [Registros de Decisao](./adr/README.md)
- [Canais](./channels.md)
  - [Configuracao do iMessage](./channels/imessage.md)
- [Provedores](./providers.md)
- [Ferramentas](./tools.md)
- [MCP](./mcp.md)
- [Seguranca](./security.md)
  - [Arquitetura](./security/architecture.md)
  - [Superficies de Ataque](./security/attack-surfaces.md)
  - [Lista de Verificacao](./security/checklist.md)
- [Plugins](./plugins.md)"""


class TestMapaTraducao(unittest.TestCase):
    """Testes para o dicionario de traducao."""

    def test_mapa_nao_esta_vazio(self):
        """Verifica que o mapa de traducao tem entradas."""
        self.assertGreater(len(MAPA_TRADUCAO), 0)

    def test_todos_valores_sao_strings_nao_vazias(self):
        """Verifica que todos os valores do mapa sao strings preenchidas."""
        for chave, valor in MAPA_TRADUCAO.items():
            self.assertIsInstance(chave, str)
            self.assertIsInstance(valor, str)
            self.assertGreater(len(chave), 0, f"Chave vazia encontrada")
            self.assertGreater(len(valor), 0, f"Valor vazio para chave '{chave}'")

    def test_mapa_contem_termos_essenciais(self):
        """Verifica se os termos mais importantes estao no mapa."""
        termos_essenciais = ["Summary", "Introduction", "Getting Started", "Security"]
        for termo in termos_essenciais:
            self.assertIn(termo, MAPA_TRADUCAO, f"Termo '{termo}' ausente no mapa")

    def test_nenhuma_traducao_igual_ao_original(self):
        """Verifica que nenhuma traducao e identica ao original (exceto siglas)."""
        siglas_permitidas = {"MCP", "Plugins"}
        for original, traduzido in MAPA_TRADUCAO.items():
            if original not in siglas_permitidas:
                self.assertNotEqual(
                    original, traduzido,
                    f"'{original}' nao foi traduzido",
                )


class TestTraduzirLinha(unittest.TestCase):
    """Testes para a funcao traduzir_linha."""

    def test_traduz_link_simples(self):
        """Verifica traducao de uma linha com link Markdown."""
        linha = "- [Getting Started](./getting_started.md)"
        resultado = traduzir_linha(linha, MAPA_TRADUCAO)
        self.assertEqual(resultado, "- [Primeiros Passos](./getting_started.md)")

    def test_traduz_link_indentado(self):
        """Verifica traducao de uma linha indentada."""
        linha = "  - [Decision Records](./adr/README.md)"
        resultado = traduzir_linha(linha, MAPA_TRADUCAO)
        self.assertEqual(resultado, "  - [Registros de Decisao](./adr/README.md)")

    def test_traduz_titulo(self):
        """Verifica traducao do titulo de primeiro nivel."""
        linha = "# Summary"
        resultado = traduzir_linha(linha, MAPA_TRADUCAO)
        self.assertEqual(resultado, "# Sumario")

    def test_mantem_link_intacto(self):
        """Verifica que os links (URLs) nao sao alterados."""
        linha = "- [Getting Started](./getting_started.md)"
        resultado = traduzir_linha(linha, MAPA_TRADUCAO)
        self.assertIn("./getting_started.md", resultado)

    def test_linha_sem_traducao_fica_igual(self):
        """Verifica que linhas sem correspondencia permanecem intactas."""
        linha = "- [Algo Desconhecido](./desconhecido.md)"
        resultado = traduzir_linha(linha, MAPA_TRADUCAO)
        self.assertEqual(resultado, linha)

    def test_linha_vazia_fica_vazia(self):
        """Verifica que linhas vazias permanecem vazias."""
        resultado = traduzir_linha("", MAPA_TRADUCAO)
        self.assertEqual(resultado, "")

    def test_preserva_indentacao(self):
        """Verifica que a indentacao original e preservada."""
        linha = "    - [Checklist](./security/checklist.md)"
        resultado = traduzir_linha(linha, MAPA_TRADUCAO)
        self.assertTrue(resultado.startswith("    "))

    def test_nao_traduz_dentro_de_links(self):
        """Verifica que URLs dentro de parenteses nao sao traduzidas."""
        linha = "- [Security](./security.md)"
        resultado = traduzir_linha(linha, MAPA_TRADUCAO)
        self.assertIn("(./security.md)", resultado)
        self.assertIn("[Seguranca]", resultado)


class TestTraduzirConteudo(unittest.TestCase):
    """Testes para a funcao traduzir_conteudo."""

    def test_traduz_conteudo_completo(self):
        """Verifica traducao completa do SUMMARY.md."""
        resultado = traduzir_conteudo(SUMMARY_INGLES, MAPA_TRADUCAO)
        self.assertEqual(resultado, SUMMARY_PORTUGUES_ESPERADO)

    def test_conteudo_vazio(self):
        """Verifica comportamento com conteudo vazio."""
        resultado = traduzir_conteudo("", MAPA_TRADUCAO)
        self.assertEqual(resultado, "")

    def test_conteudo_sem_termos_traduziveis(self):
        """Verifica que conteudo sem termos do mapa fica inalterado."""
        conteudo = "Linha qualquer\nOutra linha"
        resultado = traduzir_conteudo(conteudo, MAPA_TRADUCAO)
        self.assertEqual(resultado, conteudo)

    def test_mapa_vazio_nao_altera_nada(self):
        """Verifica que mapa vazio nao altera o conteudo."""
        resultado = traduzir_conteudo(SUMMARY_INGLES, {})
        self.assertEqual(resultado, SUMMARY_INGLES)

    def test_resultado_nao_contem_termos_em_ingles(self):
        """Verifica que termos traduzidos nao aparecem em ingles."""
        resultado = traduzir_conteudo(SUMMARY_INGLES, MAPA_TRADUCAO)
        termos_que_devem_sumir = ["Getting Started", "Decision Records", "Attack Surfaces"]
        for termo in termos_que_devem_sumir:
            self.assertNotIn(termo, resultado)


class TestObterEstatisticas(unittest.TestCase):
    """Testes para a funcao obter_estatisticas."""

    def test_estrutura_das_estatisticas(self):
        """Verifica se as estatisticas tem todas as chaves esperadas."""
        traduzido = traduzir_conteudo(SUMMARY_INGLES, MAPA_TRADUCAO)
        stats = obter_estatisticas(SUMMARY_INGLES, traduzido)
        chaves_esperadas = {
            "total_linhas", "linhas_alteradas", "linhas_mantidas",
            "termos_no_mapa", "data_hora",
        }
        self.assertEqual(set(stats.keys()), chaves_esperadas)

    def test_soma_alteradas_mais_mantidas_igual_total(self):
        """Verifica que alteradas + mantidas = total."""
        traduzido = traduzir_conteudo(SUMMARY_INGLES, MAPA_TRADUCAO)
        stats = obter_estatisticas(SUMMARY_INGLES, traduzido)
        self.assertEqual(
            stats["linhas_alteradas"] + stats["linhas_mantidas"],
            stats["total_linhas"],
        )

    def test_conteudos_iguais_zero_alteracoes(self):
        """Verifica que conteudos identicos resultam em zero alteracoes."""
        stats = obter_estatisticas(SUMMARY_INGLES, SUMMARY_INGLES)
        self.assertEqual(stats["linhas_alteradas"], 0)

    def test_data_hora_formatada(self):
        """Verifica que a data/hora esta no formato esperado."""
        traduzido = traduzir_conteudo(SUMMARY_INGLES, MAPA_TRADUCAO)
        stats = obter_estatisticas(SUMMARY_INGLES, traduzido)
        # Formato: YYYY-MM-DD HH:MM:SS
        self.assertRegex(stats["data_hora"], r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}")


class TestOperacoesArquivo(unittest.TestCase):
    """Testes para funcoes de manipulacao de arquivo."""

    def setUp(self):
        """Cria diretorio temporario para testes."""
        self.dir_temp = tempfile.mkdtemp()
        self.arquivo_teste = os.path.join(self.dir_temp, "teste.md")
        with open(self.arquivo_teste, "w", encoding="utf-8") as f:
            f.write("conteudo de teste")

    def tearDown(self):
        """Remove diretorio temporario."""
        shutil.rmtree(self.dir_temp)

    def test_carregar_arquivo_existente(self):
        """Verifica leitura de arquivo existente."""
        conteudo = carregar_arquivo(self.arquivo_teste)
        self.assertEqual(conteudo, "conteudo de teste")

    def test_carregar_arquivo_inexistente_lanca_erro(self):
        """Verifica que arquivo inexistente gera excecao."""
        with self.assertRaises(FileNotFoundError):
            carregar_arquivo(os.path.join(self.dir_temp, "nao_existe.md"))

    def test_salvar_arquivo(self):
        """Verifica escrita de arquivo."""
        caminho = os.path.join(self.dir_temp, "novo.md")
        salvar_arquivo(caminho, "novo conteudo")
        conteudo = carregar_arquivo(caminho)
        self.assertEqual(conteudo, "novo conteudo")

    def test_criar_backup(self):
        """Verifica criacao de backup."""
        caminho_backup = os.path.join(self.dir_temp, "teste.md.bak")
        criar_backup(self.arquivo_teste, caminho_backup)
        self.assertTrue(os.path.exists(caminho_backup))
        conteudo_backup = carregar_arquivo(caminho_backup)
        self.assertEqual(conteudo_backup, "conteudo de teste")

    def test_criar_backup_arquivo_inexistente_lanca_erro(self):
        """Verifica que backup de arquivo inexistente gera excecao."""
        with self.assertRaises(FileNotFoundError):
            criar_backup(
                os.path.join(self.dir_temp, "nao_existe.md"),
                os.path.join(self.dir_temp, "backup.md"),
            )


class TestExibicao(unittest.TestCase):
    """Testes para funcoes de exibicao no terminal."""

    def _capturar_saida(self, funcao, *args):
        """Helper para capturar saida do print."""
        saida = io.StringIO()
        stdout_original = sys.stdout
        sys.stdout = saida
        funcao(*args)
        sys.stdout = stdout_original
        return saida.getvalue()

    def test_cabecalho_contem_titulo(self):
        """Verifica se o cabecalho exibe o titulo."""
        resultado = self._capturar_saida(exibir_cabecalho, "Meu Titulo")
        self.assertIn("Meu Titulo", resultado)

    def test_cabecalho_contem_separadores(self):
        """Verifica se o cabecalho tem separadores."""
        resultado = self._capturar_saida(exibir_cabecalho, "Teste")
        separador = "=" * LARGURA_SEPARADOR
        self.assertEqual(resultado.count(separador), 2)

    def test_estatisticas_exibe_campos(self):
        """Verifica se as estatisticas exibem todos os campos."""
        stats = {
            "total_linhas": 10,
            "linhas_alteradas": 5,
            "linhas_mantidas": 5,
            "termos_no_mapa": 14,
            "data_hora": "2025-01-01 12:00:00",
        }
        resultado = self._capturar_saida(exibir_estatisticas, stats)
        self.assertIn("10", resultado)
        self.assertIn("5", resultado)
        self.assertIn("14", resultado)

    def test_preview_exibe_conteudo(self):
        """Verifica se o preview exibe o conteudo."""
        resultado = self._capturar_saida(exibir_preview, "Linha de teste")
        self.assertIn("Linha de teste", resultado)


if __name__ == "__main__":
    sys.stdout = sys.__stdout__
    unittest.main(verbosity=2)
