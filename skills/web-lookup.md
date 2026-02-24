---
name: web-lookup
description: Busca e sintetiza informações a partir de URLs. Resume páginas, extrai fatos principais e responde perguntas com base em conteúdo da web.
triggers:
  # Oficiais (PT)
  - pesquisar
  - buscar
  - procurar
  - verificar link
  - consultar
  - o que é
  - veja este link
  - acessar link

  # Aliases opcionais (EN) — compatibilidade
  - look up
  - search for
  - what is
  - check this link
  - fetch
dependencies: []
---

# Consulta Web

Quando o usuário pedir para pesquisar algo ou fornecer um link, busque e sintetize a informação.

---

## Como realizar a busca

1. **URL fornecida**: Use `web_fetch` para obter o conteúdo da página.
2. **Tópico ou pergunta fornecida**:
   - Se o usuário disser "o que é X" ou "pesquise Y", explique que você pode buscar uma URL específica caso ele forneça.
   - Caso contrário, responda com base no seu conhecimento interno (se aplicável).

---

## Processamento do conteúdo

Após buscar uma URL:

1. **Extraia apenas as informações relevantes** — não copie a página inteira.
2. **Estruture a resposta**:
   - Principais fatos ou descobertas (bullet points)
   - Citações ou dados relevantes
   - Atribuição da fonte (URL)
3. **Responda diretamente à pergunta do usuário**, se houver uma pergunta específica.

---

## Formato de saída

### Para buscas gerais:

> **[Título da página ou tópico]** — [URL da fonte]
>
> - Principais descobertas (3–5 bullet points)

---

### Para perguntas específicas:

> [Resposta direta à pergunta]
>
> Fonte: [URL]

---

## Lidando com múltiplas URLs

Se o usuário fornecer múltiplos links ou pedir comparação entre fontes:

1. Busque cada URL separadamente.
2. Apresente os achados de cada fonte.
3. Destaque concordâncias e contradições entre elas.

---

## Regras

- Sempre cite a URL da fonte.
- Se `web_fetch` falhar (timeout, 404, paywall), informe o usuário e sugira alternativas.
- Nunca invente informações. Se a página não contiver o que o usuário procura, diga claramente.
- Para páginas muito longas, foque nas seções mais relevantes em vez de resumir tudo.
- Se o conteúdo estiver protegido por login ou paywall, informe que não é possível acessar.