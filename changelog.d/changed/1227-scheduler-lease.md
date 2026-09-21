- **Scheduler reivindica tarefas sob lease; re-poll apos queda e explicito e
  logado (#1227 slice 2).** O `run_scheduler` lia as tarefas vencidas com um
  `SELECT` puro e so mudava o status ao terminar — uma queda no meio do turno
  deixava a linha `pending`, e o tick seguinte (60 s) reexecutava a mesma
  tarefa EM SILENCIO, com mensagem de sistema duplicada no canal. Agora o
  tick passa por `claim_due_tasks`: numa transacao `BEGIN IMMEDIATE`, cada
  tarefa vencida vira `status = 'running'` com `lease_until = now + 600 s` e
  `leased_by = <uuid do processo>`, e so as linhas que ESTE chamador de fato
  virou (`UPDATE ... WHERE status = 'pending'`, contando `changes()`) sao
  devolvidas — dois schedulers sobre o mesmo arquivo nunca levam a mesma
  tarefa. `recover_expired_leases` devolve a `pending` toda tarefa `running`
  cuja lease passou (ou que ficou `running` sem lease) e a DEVOLVE ao
  chamador; `log_recovered_leases` grava `warn!` com id, `attempts` e
  `execute_at` — nunca `payload` (PII) — e roda na subida do gateway, ao lado
  de `log_interrupted_runs`, e no inicio de cada tick. Os terminais
  (`complete_task`, `fail_task`, `retry_or_fail_task`, `complete_recurring_run`)
  limpam a lease; o retry e o pulo de ocorrencia recorrente tambem devolvem o
  status a `pending`, o que antes nao precisavam fazer. Migration forward-only
  no `run_migrations` do `SessionStore`: colunas `lease_until TEXT` e
  `leased_by TEXT` em `scheduled_tasks`, com `ALTER TABLE` guardado por
  `pragma_table_info` (falha real sobe como erro em vez de virar coluna
  ausente), mais indice parcial `idx_tasks_lease`. `poll_due_tasks` continua
  existindo como leitura pura, documentado como tal. Teste de varredura fixa
  que `run_scheduler` usa `claim_due_tasks` e nao `poll_due_tasks`, e que a
  recuperacao roda nos dois lugares.
