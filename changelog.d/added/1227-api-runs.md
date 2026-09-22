- **`GET /api/runs` le o ledger de runs pelo gateway (#1227).** Somente
  leitura, com `status` filtrado no SQL (valor desconhecido e `400` de corpo
  constante) e `limit` preso em `[1, 200]`. Os instantes saem em UTC ISO 8601
  com `Z` e o conteudo sai so como previa de ate 120 caracteres, com segredo
  de formato conhecido redigido e caractere de controle trocado; o trecho
  completo do ledger nao sai por HTTP. O acesso e mais estrito que o resto de
  `/api/*`: com `gateway.api_key` exige o bearer; sem chave, so responde a
  peer loopback com `Host` de loopback (LAN recebe `503`, DNS rebinding
  recebe `403`, mesmo sem `Origin`). Os leitores de instante e de previa
  passaram para o `garraia-db`, compartilhados com `garraia runs list`, que
  agora explica que run `interrupted` de tarefa agendada e reexecutado
  sozinho pelo scheduler — por isso nao ha `runs resume`.
