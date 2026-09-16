- **O contexto de projeto do `garra chat` para de gastar prompt com `target/` e
  passa a dizer que projeto e em que ramo (#1219).** O `scan_directory_context`
  listava qualquer entrada do topo do diretorio, entao `target/`,
  `node_modules/` e `dist/` entravam no system prompt de todo turno — tokens
  pagos para informar ao modelo que o projeto tem um diretorio de build. Pior
  que o desperdicio era o que faltava: o resumo nao dizia **qual** projeto era
  (so marcadores genericos tipo "Rust project") nem **em que ramo** o agente
  estava, embora o painel `/contexto` ja mostrasse o ramo na tela ao lado.
  Agora doze diretorios de build e dependencia sao filtrados da listagem, o
  nome do projeto sai do primeiro heading do `README.md` e o ramo vem do mesmo
  `ui::git_branch` que o painel `/contexto` usa — leitura direta de `HEAD`, sem
  spawnar `git` e sem varrer a arvore, entao worktree e submodulo continuam
  resolvendo certo. O filtro so descarta o que e **diretorio**: um arquivo
  chamado `build` ou `coverage` segue aparecendo, que e o caso em que o nome
  carrega informacao de verdade.
  A composicao do resumo passou a ser por partes juntadas com `|`, porque com
  quatro campos opcionais a concatenacao antiga deixava separador orfao toda
  vez que um deles vinha vazio.
