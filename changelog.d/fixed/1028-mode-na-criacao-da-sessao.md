- **`POST /api/sessions` aplica o `mode` pedido em vez de engoli-lo (#1028).** O
  corpo aceitava qualquer campo e descartava o que nao conhecia: `{"mode":
  "search"}` devolvia 201 e a sessao nascia sem politica nenhuma — a escrita de
  arquivo que o modo devia bloquear passava. Agora `mode` e validado pela mesma
  funcao do `POST /api/mode/select` (nativos e customizados), gravado como modo
  escolhido antes da resposta e ecoado nela; nome desconhecido e 400 sem criar
  sessao, e sem `session_store` e 503 em vez de fingir que aplicou. Quem nao
  manda `mode` nao ve diferenca.
