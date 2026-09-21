- **`tool_program` fecha os achados da revisao pos-merge (#1226).** O
  envelope continua contando no orcamento, mas saiu da janela de deteccao de
  loop: repetir o mesmo passo em programas de um passo so, volta apos volta,
  alternava a janela entre `tool_program` e o passo e o corte de 3 chamadas
  identicas nunca vinha (sobrava so o teto da tarefa, ~25 repeticoes). Um
  `"$nome"` sem valor agora falha o passo antes do despacho, com
  `parou_no_passo` e o nome da variavel, em vez de chegar a ferramenta como o
  texto `"$nome"` (que o `bash` expandiria como variavel de ambiente); e um
  passo com `as` cuja saida nao e inteiro falha ali, em vez de sair
  `"ok": true` sem criar a variavel. Na pausa por confirmacao humana, o
  `ToolResult` do modelo passa a levar o relatorio parcial (passos ja
  executados, `parou_no_passo`, `vars`) depois do pedido, e o texto do humano
  continua so com o pedido do passo; o primeiro marcador do conteudo e sempre
  o do pedido. Um passo negado pelo gate fecha o proprio `tool_started` com um
  `tool_finished` (antes ficava aberto dentro do par do programa). Testes
  novos cobrem o modo `ask` nativo com a lista `denied`, quais perfis nativos
  expoem `tool_program`, a retomada apos aprovacao e o equilibrio dos eventos
  no streaming; `docs/src/modes.md` e o threat model documentam a superficie.
  Na revisao de seguranca do proprio conserto, dois ajustes: um programa
  que para antes de despachar qualquer passo (mal formado, mais de 16
  passos, `$var` indefinida no passo 0) volta a cair no detector de loop na
  terceira repeticao, e o relatorio parcial de uma pausa neutraliza qualquer
  `[CONFIRM_REQUIRED:` vindo de saida de passo, entao a aprovacao nao
  depende mais de o marcador verdadeiro vir primeiro.
