- **Retencao de memoria e ledger: fatia de dados (#1436).** `garraia-config::retention`
  passa a ser o dono das faixas de `memory.retention` e `runs.retention_days`: o
  `config check` delega para ele e o futuro `PATCH /admin/api/retention` vai
  recusar o que esses mesmos achados chamam de Error, com patch campo a campo
  (`deny_unknown_fields`) e a janela da limpeza manual. `garraia-db::retention`
  conta o que uma limpeza apagaria sem apagar (mesma clausula do `DELETE`,
  cobrada por teste), mede o tamanho dos dois bancos e grava a ultima limpeza no
  proprio banco: `compact` e `prune_agent_runs` anotam a execucao, entao worker,
  CLI e console passam a ver o mesmo lugar. A pagina e o endpoint do console
  vêm em fatia propria.
