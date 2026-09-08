- **`POST /api/sessions` aplica `mode` e `working_dir` em vez de engoli-los (#1028).**
  O corpo aceitava qualquer campo e descartava o que nao conhecia: `{"mode":
  "search"}` devolvia 201 e a sessao nascia sem politica nenhuma — a escrita de
  arquivo que o modo devia bloquear passava — e `working_dir` nunca chegava as
  ferramentas de arquivo (o handler que o aceitava nunca foi roteado). Agora
  `mode` e validado pela mesma funcao do `POST /api/mode/select` (nativos e
  customizados), gravado como modo escolhido antes da resposta e ecoado nela;
  `working_dir` passa por `project_root::confine` como o `path` de projeto e a
  resposta ecoa o canonicalizado. Nome de modo desconhecido ou diretorio fora
  das raizes e 400 sem criar sessao; `mode` sem `session_store` e 503 em vez de
  fingir que aplicou. Quem nao manda nenhum dos dois nao ve diferenca.
