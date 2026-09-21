- **Loop detector: o erro de tool em loop traz o diagnostico do input repetido (#1295, item 2 de 3).**
  Em vez do seco "tool loop detected: <tool>", o erro agora leva o nome da
  ferramenta, a contagem da janela (3 chamadas identicas em sequencia) e o
  input repetido — e quem le (o humano no log, no ledger de runs ou no
  cartao da CLI) tem como corrigir. O input sai pela mesma allow-list do
  `summarize_tool_input` (#937): o campo que interessa da ferramenta
  (`command`, `path`, `url`...), redigido e truncado; ferramenta sem caso
  proprio mostra so os nomes das chaves e o tamanho, nunca valor — o erro e
  persistido e exibido, e um `{"password": ...}` de tool nova nao tem
  formato que o redactor reconheca. A mensagem nasce num unico lugar
  (`ExecutionBudget::mensagem_de_loop`) e passa pelo despacho unico do
  #1311; o gatilho (3 assinaturas identicas) nao mudou.
