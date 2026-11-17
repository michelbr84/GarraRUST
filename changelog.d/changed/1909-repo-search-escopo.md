- **`repo_search` diz onde olhou quando nao acha nada (#1909).** O resultado
  vazio era so "No matches found.", sem separar "nao existe" de "olhei em
  outro lugar" — no dogfood do Telegram a busca rodou no workspace da sessao
  enquanto o assunto era outro worktree. Agora a resposta nomeia o escopo
  (o diretorio de trabalho da sessao, ou o do processo quando a sessao nao
  tem um) e avisa que so ele foi olhado; o caminho da sessao nao entra na
  mensagem. E um `working_dir` que nao existe ou nao e diretorio vira erro
  proprio, em vez de "Search failed (rg: No such file or directory, ...)". O mesmo
  vale para o fallback de `grep`: uma busca que sai com erro e sem nenhuma
  linha deixa de ser lida como resultado vazio.
