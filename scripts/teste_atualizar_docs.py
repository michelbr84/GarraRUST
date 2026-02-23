"""
Modulo de testes para o script atualizar_docs.py
GarraIA - 100% em portugues!
"""

import unittest
import sys
import os
import io

# Adiciona o diretorio scripts ao path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from atualizar_docs import (
    CONFIGURACAO_ATUAL,
    CONFIGURACAO_NOVA,
    LARGURA_SEPARADOR,
    obter_diferencas,
    exibir_cabecalho,
    exibir_diferencas,
    exibir_comparacao,
)


class TestConfiguracoes(unittest.TestCase):
    """Testes para as constantes de configuracao."""

    def test_configuracao_atual_tem_campos_obrigatorios(self):
        """Verifica se a configuracao atual possui todos os campos esperados."""
        campos_esperados = {"title", "language", "authors"}
        self.assertEqual(set(CONFIGURACAO_ATUAL.keys()), campos_esperados)

    def test_configuracao_nova_tem_campos_obrigatorios(self):
        """Verifica se a configuracao nova possui todos os campos esperados."""
        campos_esperados = {"title", "language", "authors"}
        self.assertEqual(set(CONFIGURACAO_NOVA.keys()), campos_esperados)

    def test_configuracao_atual_esta_em_ingles(self):
        """Verifica se a configuracao atual esta em ingles."""
        self.assertEqual(CONFIGURACAO_ATUAL["language"], "en")

    def test_configuracao_nova_esta_em_portugues(self):
        """Verifica se a configuracao nova esta em portugues brasileiro."""
        self.assertEqual(CONFIGURACAO_NOVA["language"], "pt-BR")

    def test_configuracoes_tem_mesmas_chaves(self):
        """Verifica se ambas as configuracoes possuem as mesmas chaves."""
        self.assertEqual(
            set(CONFIGURACAO_ATUAL.keys()),
            set(CONFIGURACAO_NOVA.keys()),
        )

    def test_nenhum_valor_vazio(self):
        """Verifica se nenhum valor das configuracoes esta vazio."""
        for chave, valor in CONFIGURACAO_ATUAL.items():
            self.assertTrue(len(valor) > 0, f"Campo '{chave}' vazio na config atual")
        for chave, valor in CONFIGURACAO_NOVA.items():
            self.assertTrue(len(valor) > 0, f"Campo '{chave}' vazio na config nova")


class TestObterDiferencas(unittest.TestCase):
    """Testes para a funcao obter_diferencas."""

    def test_todos_campos_diferentes(self):
        """Verifica que todos os campos foram alterados entre atual e nova."""
        diferencas = obter_diferencas(CONFIGURACAO_ATUAL, CONFIGURACAO_NOVA)
        for diff in diferencas:
            self.assertTrue(
                diff["alterado"],
                f"Campo '{diff['campo']}' deveria estar marcado como alterado",
            )

    def test_nenhum_campo_diferente(self):
        """Verifica comportamento quando os dicionarios sao iguais."""
        diferencas = obter_diferencas(CONFIGURACAO_ATUAL, CONFIGURACAO_ATUAL)
        for diff in diferencas:
            self.assertFalse(
                diff["alterado"],
                f"Campo '{diff['campo']}' nao deveria estar alterado",
            )

    def test_quantidade_de_diferencas(self):
        """Verifica se retorna a quantidade correta de campos."""
        diferencas = obter_diferencas(CONFIGURACAO_ATUAL, CONFIGURACAO_NOVA)
        self.assertEqual(len(diferencas), 3)

    def test_estrutura_da_diferenca(self):
        """Verifica se cada diferenca possui as chaves esperadas."""
        diferencas = obter_diferencas(CONFIGURACAO_ATUAL, CONFIGURACAO_NOVA)
        chaves_esperadas = {"campo", "antes", "depois", "alterado"}
        for diff in diferencas:
            self.assertEqual(set(diff.keys()), chaves_esperadas)

    def test_dicionarios_vazios(self):
        """Verifica comportamento com dicionarios vazios."""
        diferencas = obter_diferencas({}, {})
        self.assertEqual(len(diferencas), 0)

    def test_chaves_extras_no_antes(self):
        """Verifica tratamento de chaves que existem apenas no 'antes'."""
        antes = {"extra": "valor_extra"}
        depois = {}
        diferencas = obter_diferencas(antes, depois)
        self.assertEqual(len(diferencas), 1)
        self.assertEqual(diferencas[0]["antes"], "valor_extra")
        self.assertEqual(diferencas[0]["depois"], "(nao definido)")
        self.assertTrue(diferencas[0]["alterado"])

    def test_chaves_extras_no_depois(self):
        """Verifica tratamento de chaves que existem apenas no 'depois'."""
        antes = {}
        depois = {"nova_chave": "novo_valor"}
        diferencas = obter_diferencas(antes, depois)
        self.assertEqual(len(diferencas), 1)
        self.assertEqual(diferencas[0]["antes"], "(nao definido)")
        self.assertEqual(diferencas[0]["depois"], "novo_valor")
        self.assertTrue(diferencas[0]["alterado"])

    def test_resultado_ordenado_por_campo(self):
        """Verifica se os resultados vem ordenados alfabeticamente."""
        diferencas = obter_diferencas(CONFIGURACAO_ATUAL, CONFIGURACAO_NOVA)
        campos = [d["campo"] for d in diferencas]
        self.assertEqual(campos, sorted(campos))


class TestExibirCabecalho(unittest.TestCase):
    """Testes para a funcao exibir_cabecalho."""

    def test_cabecalho_contem_titulo(self):
        """Verifica se o cabecalho exibe o titulo fornecido."""
        saida = io.StringIO()
        sys.stdout = saida
        exibir_cabecalho("Titulo de Teste")
        sys.stdout = sys.__stdout__
        resultado = saida.getvalue()
        self.assertIn("Titulo de Teste", resultado)

    def test_cabecalho_contem_separadores(self):
        """Verifica se o cabecalho contem linhas separadoras."""
        saida = io.StringIO()
        sys.stdout = saida
        exibir_cabecalho("Teste")
        sys.stdout = sys.__stdout__
        resultado = saida.getvalue()
        separador = "=" * LARGURA_SEPARADOR
        self.assertEqual(resultado.count(separador), 2)


class TestExibirDiferencas(unittest.TestCase):
    """Testes para a funcao exibir_diferencas."""

    def test_exibe_campo_alterado_com_asterisco(self):
        """Verifica se campos alterados sao marcados com asterisco."""
        diferencas = [{"campo": "teste", "antes": "a", "depois": "b", "alterado": True}]
        saida = io.StringIO()
        sys.stdout = saida
        exibir_diferencas(diferencas)
        sys.stdout = sys.__stdout__
        resultado = saida.getvalue()
        self.assertIn("*", resultado)

    def test_exibe_campo_inalterado_sem_asterisco(self):
        """Verifica se campos inalterados nao tem asterisco."""
        diferencas = [{"campo": "teste", "antes": "a", "depois": "a", "alterado": False}]
        saida = io.StringIO()
        sys.stdout = saida
        exibir_diferencas(diferencas)
        sys.stdout = sys.__stdout__
        resultado = saida.getvalue()
        self.assertNotIn("*", resultado)

    def test_exibe_valores_antes_e_depois(self):
        """Verifica se os valores antes e depois sao exibidos."""
        diferencas = [{"campo": "idioma", "antes": "en", "depois": "pt-BR", "alterado": True}]
        saida = io.StringIO()
        sys.stdout = saida
        exibir_diferencas(diferencas)
        sys.stdout = sys.__stdout__
        resultado = saida.getvalue()
        self.assertIn("en", resultado)
        self.assertIn("pt-BR", resultado)


class TestExibirComparacao(unittest.TestCase):
    """Testes para a funcao orquestradora exibir_comparacao."""

    def test_executa_sem_erros(self):
        """Verifica se a funcao principal executa sem lancar excecoes."""
        saida = io.StringIO()
        sys.stdout = saida
        try:
            exibir_comparacao()
            executou = True
        except Exception as e:
            executou = False
        finally:
            sys.stdout = sys.__stdout__
        self.assertTrue(executou)

    def test_saida_contem_total_alterados(self):
        """Verifica se a saida mostra o total de campos alterados."""
        saida = io.StringIO()
        sys.stdout = saida
        exibir_comparacao()
        sys.stdout = sys.__stdout__
        resultado = saida.getvalue()
        self.assertIn("Total de campos alterados", resultado)
        self.assertIn("3/3", resultado)


if __name__ == "__main__":
    # Restaura stdout para garantir compatibilidade com o runner
    sys.stdout = sys.__stdout__
    unittest.main(verbosity=2)
