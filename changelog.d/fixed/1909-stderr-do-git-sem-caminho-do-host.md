- **Recusa do git em `git_diff`/`code_review` deixa de entregar o caminho do
  host no turno restrito (#1909).** O git nomeia o diretorio por extenso nas
  proprias recusas (`cannot use bare repository '/abs'`, `dubious ownership in
  repository at '/abs'` e a sugestao `safe.directory /abs`), e esse stderr saia
  no corpo da resposta, logo abaixo da linha `Repositorio:` que ja retinha o
  caminho. Num turno restrito cada caminho absoluto do stderr agora sai
  relativo ao `working_dir` da sessao quando cai dentro dele (`.`,
  `.git/config`) e vira o marcador `<outside the session's directory>` quando
  nao, pelo mesmo `pode_revelar_caminho_do_host`; a causa da recusa continua dita e o turno
  aberto do operador local ve o texto do git inteiro, como antes. So o stderr
  e tocado: o stdout (o diff em si) e conteudo do repositorio e segue igual.
  Limite conhecido, herdado do portao: "turno restrito" e a forma da politica
  de tools, nao a identidade do remetente.
