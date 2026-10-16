- **Site de documentacao Docusaurus em `website/`, publicado no GitHub Pages (#1639).**
  O conteudo nao e escrito la: vem do markdown que ja existe no repositorio
  (`README`, `docs/**`, `docs/adr/**`, `wiki/**`, `CHANGELOG`, `ROADMAP`,
  `CONTRIBUTING`, ...) por um script de sync em Node puro — fonte unica de verdade,
  nada reescrito a mao. Portugues (Brasil) como idioma padrao e ingles parcial
  (Introducao, Instalacao, Releases e as paginas What's New), com o restante caindo
  para o portugues. Busca offline (`@easyops-cn/docusaurus-search-local`), tema
  claro/escuro e o workflow `docs-site.yml`, que compila cada PR e publica no Pages
  a cada push na `main`. Publicado em https://michelbr84.github.io/GarraRUST/
  (`url`/`baseUrl` configuraveis por env). Nao altera o `ci.yml` nem o site
  `garraia.org`.
