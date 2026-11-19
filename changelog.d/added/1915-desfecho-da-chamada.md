- **Cada chamada de ferramenta termina com um desfecho classificado, e nao com um
  `success: bool` (#1915).** O evento `ToolFinished` do runtime ganhou o campo
  `outcome` (`DesfechoDaChamada`): timeout, recusa de politica, ferramenta
  indisponivel, recusa do breaker, bloqueio do detector de loop, orcamento
  esgotado, pedido de confirmacao humana, erro e sucesso eram todos o mesmo
  `false`, e quem lia o evento nao tinha como saber o que houve. Os codigos sao
  os mesmos do registro local de confiabilidade (`success`, `timeout`,
  `denied_by_policy`, …), com dois novos para as recusas que sao do runtime e nao
  da ferramenta: `loop_blocked` e `budget_exhausted`. O frame `tool_finished` do
  `/ws` e o resumo de tool do host MCP levam o codigo num campo aditivo ao lado
  do `success`, que continua igual. O `success` do evento passa a ser derivado do
  desfecho na origem, para que os dois nao possam divergir.
