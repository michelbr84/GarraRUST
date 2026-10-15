# GarraIA — site de documentação (Docusaurus)

Site de documentação do GarraIA, construído com [Docusaurus](https://docusaurus.io/)
e publicado no GitHub Pages em **https://michelbr84.github.io/GarraRUST/**.

## Fonte única de verdade

O conteúdo **não é escrito aqui**. A fonte de verdade é o markdown que já existe
no repositório (`README.md`, `docs/**`, `docs/adr/**`, `wiki/**`, `CHANGELOG.md`,
`ROADMAP.md`, `CONTRIBUTING.md`, ...). O script `scripts/sync-docs.mjs` (Node
puro, sem dependências) copia um conjunto **explícito** desses arquivos para
`docs/` (gerado, gitignored), injetando front matter e reescrevendo links.

As únicas páginas escritas à mão pelo site ficam em `content/` (versionadas):
a Introdução, a página de Troubleshooting (que só agrega links para as seções
reais) e a página de Releases.

## Como rodar

```bash
npm ci
npm run sync     # gera docs/ e i18n/en/.../current a partir do repositório
npm start        # dev server (roda o sync antes); http://localhost:3000
```

Atalho: `npm start` e `npm run build` já rodam `npm run sync` automaticamente.

## Build e preview de produção

```bash
npm run build    # gera build/ para os dois locales (pt-BR e en)
npm run serve    # serve o build/ localmente
```

`url`/`baseUrl` são configuráveis por ambiente (`DOCS_SITE_URL`, `DOCS_BASE_URL`),
com os defaults do GitHub Pages deste repositório — para um futuro
`garraia.org/docs/` ou `docs.garraia.org` sem mudar código.

## Idiomas

`pt-BR` é o locale padrão. `en` tem overrides para Introdução, Instalação e
Releases (mais as páginas "What's New"); o resto cai para o conteúdo em
português. Para regenerar as strings de UI em inglês:

```bash
npm run write-translations -- --locale en
```

## Publicação

O workflow `.github/workflows/docs-site.yml` roda `npm ci && npm run sync &&
npm run build` em cada PR (como artifact) e publica no GitHub Pages a cada push
na `main`. O `garraia.org` é outro repositório (Lovable) e pode, no futuro,
apontar `/docs/` para este site.
