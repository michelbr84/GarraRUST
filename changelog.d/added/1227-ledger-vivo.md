- O ledger de runs tem o primeiro escritor de producao (#1227, slice 1). A
  subida do gateway e do CLI converte runs `running` deixados por uma queda
  em `interrupted`, com ids no log (nunca o `goal`, que e PII), e cada
  execucao de tarefa agendada do scheduler deixa uma linha em `agent_runs`
  com `session_id` preenchido e desfecho terminal (`done`/`error`) — uma
  queda no meio da execucao aparece como `interrupted` no restart seguinte,
  em vez de sumir. Falha do ledger e fail-soft: nunca impede a tarefa nem
  muda o fluxo de retry do scheduler.
