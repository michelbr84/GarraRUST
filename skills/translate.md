---
name: translate
description: Traduz textos entre idiomas. Detecta automaticamente o idioma de origem.
triggers:
  # Oficiais (PT)
  - traduzir
  - tradução
  - traduzir para
  - em espanhol
  - em árabe
  - em francês
  - para inglês
  - para português
  - para pt-br

  # Aliases opcionais (EN) — compatibilidade (remova se quiser 100% PT-only)
  - translate
  - translation
  - to english
  - in spanish
  - in arabic
  - in french
dependencies: []
---

# Traduzir

Traduza textos entre idiomas quando o usuário solicitar.

---

## Como traduzir

1. **Detecte o idioma de origem** a partir do texto (ou o usuário pode especificá-lo).
2. **Determine o idioma de destino** com base no pedido do usuário ("traduzir para espanhol", "em árabe", etc.).
3. **Traduza** preservando significado, tom e contexto.

---

## Formato de saída

> **[Idioma de origem] → [Idioma de destino]**
>
> [Texto traduzido]

Se o texto for longo, preserve a estrutura dos parágrafos.  
Se contiver termos técnicos, mantenha-os e adicione uma observação se houver ambiguidade relevante.

---

## Regras

- Preserve o significado original. Não parafraseie desnecessariamente.
- Mantenha a formatação (listas, títulos, blocos de código) intacta.
- Para palavras ambíguas, escolha a tradução mais adequada ao contexto e mencione alternativas se isso fizer diferença.
- Se o usuário fornecer um caminho de arquivo, use `file_read` para obter o conteúdo, traduza e apresente o resultado.
- Se o usuário fornecer uma URL, use `web_fetch` para obter o conteúdo antes de traduzir.
- Para comentários em código, traduza apenas os comentários e mantenha o código intacto.
- Se você não tiver confiança na tradução (idioma raro ou jargão muito específico), informe isso.

---

## Solicitações comuns

- "Traduza isso para [idioma]" — tradução direta.
- "O que isso diz?" — detectar idioma e **traduzir para português** (padrão do GarraIA).
- "Como se diz [frase] em [idioma]?" — fornecer a tradução e, se útil, orientação de pronúncia.
- "Traduza este arquivo" — ler o arquivo, traduzir o conteúdo e apresentar o resultado.