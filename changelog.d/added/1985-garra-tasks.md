- **`garra tasks`: listar e reautorizar tarefas agendadas (#1985).** `tasks
  list` mostra as tarefas em aberto e se cada uma tem autoridade gravada;
  `tasks authorize <id>` mostra o texto que o agente vai receber e, com
  `--yes`, da a tarefa acesso de operador nas proximas execucoes; `tasks
  revoke <id>` desfaz. Le e grava o `sessions.db` direto, como o `garra runs`.
  E a resposta da tarefa agendada passa a comecar por um aviso quando um teto
  barra o que ela pediu, ou quando ela roda com acesso reduzido. O aviso fica
  no historico da sessao e no ledger de runs, e chega ao canal pela entrega
  da #2002.
