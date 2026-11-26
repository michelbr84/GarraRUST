- **As docs de instalacao no Windows ensinam o raw do `main`, nao o `garraia.org` (#2016).**
  `README.pt-BR.md`, `docs/index.md`, `website/content/en/instalacao.md`, a
  pagina de instalacao da wiki e a mensagem do launcher do Ollama
  (`contrib/ollama-launch`) ainda mandavam rodar
  `irm https://garraia.org/install.ps1 | iex`, que morre no desafio que a borda
  do site faz ao cliente HTTP do PowerShell. As notas do `README.md` e do
  `docs/installation.md` deixam de dizer que `application/octet-stream` quebra
  o `irm`: o `Invoke-RestMethod` decodifica qualquer resposta como texto.
