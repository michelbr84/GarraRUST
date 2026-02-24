---
name: summarize
description: Resume textos, URLs ou arquivos. Produz resumos concisos com tamanho ajustável.
triggers:
  - resumir
  - tldr
  - recap
  - resumo
dependencies: []
---

# Summarizar

Quando o usuário pedir para resumir um conteúdo, siga este processo:

---

## Determinar a fonte

- **URL**: Use a ferramenta `web_fetch` para obter o conteúdo da página.
- **Caminho de arquivo**: Use a ferramenta `file_read` para ler o conteúdo do arquivo.
- **Texto inline**: Trabalhe diretamente com o texto fornecido na mensagem.

---

## Produzir o resumo

1. Leia o conteúdo completo antes de resumir. Nunca resuma incrementalmente.
2. Identifique os pontos-chave, argumentos ou conclusões.
3. Preserve a estrutura original (se houver seções, reflita isso no resumo).
4. Por padrão, gere um resumo curto (3–5 bullet points). Se o usuário pedir mais detalhes, expanda.

---

## Formato de saída

- **Curto (padrão)**: 3–5 bullet points com os principais pontos.
- **Médio**: 1–2 parágrafos com detalhes de apoio.
- **Longo**: Quebra seção por seção, preservando a estrutura do documento.

Se o usuário disser “tldr” ou “recap”, use o formato curto.  
Se disser “resuma em detalhes” ou equivalente, use o formato longo.

---

## Regras

- Nunca invente informações que não estejam na fonte.
- Se uma URL falhar ao carregar, informe o usuário e sugira que ele cole o conteúdo diretamente.
- Para conteúdos muito longos (>10.000 palavras), resuma por seções em vez de um único bloco.
- Sempre mencione a fonte no topo do resumo (URL, nome do arquivo ou "a partir da sua mensagem").