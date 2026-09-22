- **O Swagger UI do gateway entra pela feature `vendored`, sem download no
  build (#1228).** O build.rs do `utoipa-swagger-ui` baixava o zip do Swagger UI
  do GitHub a cada build limpo, o que quebrava build offline e punha um download
  sem pino de conteudo na cadeia de build (o CI contornava com um cache e
  `SWAGGER_UI_DOWNLOAD_URL=file://`). Agora o zip vem da crate
  `utoipa-swagger-ui-vendored` (mesma versao 5.17.14, MIT OR Apache-2.0), e a
  build-dependency `reqwest` que so servia para baixar sai. A rota `/docs`
  continua igual. Um teste prende a feature no manifesto e confere que a pagina
  embutida e servida. A action `swagger-ui-cache` do CI fica uma release como
  vestigial: o download dela virou best-effort (avisa em vez de abortar o job),
  entao uma queda do GitHub nao derruba mais uma release por um zip que nada le.
