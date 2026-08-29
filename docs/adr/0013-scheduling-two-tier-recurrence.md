# 0013 — Recorrência de agendamento em duas camadas

## Status

✅ accepted — 2026-08-29

## Context and Problem Statement

O GarraIA só sabia agendar tarefas **one-shot**: a tool `schedule_heartbeat`
gravava uma linha em `scheduled_tasks` (SQLite) e um worker de 60s a
executava uma vez. Não havia crate de cron no `Cargo.lock`, nenhum parser de
expressão, e `tasks.recurrence_rrule` (Postgres, migration 006) existia
apenas como coluna com CHECK de charset e o comentário "full parsing and
expansion is app-layer responsibility (future recurrence engine)".

Isso deixava a lacuna funcional mais visível de um assistente sempre-online:
"todo dia às 8h me manda o resumo" era impossível. Pior, a documentação
afirmava em dois lugares que cron já existia (`docs/src/README.md`,
`docs/index.md`), e o CHANGELOG alegava um "retry with backoff" que o código
não tinha (`fail_task` era terminal).

Existem **dois** contextos de agendamento no produto, com donos e requisitos
diferentes: o assistente pessoal (SQLite local, sessão de canal, sem
Postgres) e o workspace multi-tenant (Postgres + RLS, tarefas de grupo com
`due_at`). A decisão é como atender aos dois.

## Decision Drivers

- **Local-first**: a recorrência pessoal não pode exigir Postgres — a maior
  parte das instalações roda só com SQLite.
- **Multi-tenant correto**: no workspace, qualquer varredura cruza tenants e
  precisa respeitar FORCE RLS e funcionar com múltiplas réplicas.
- **Semântica de tempo humana**: "8h" significa 8h no fuso do operador;
  ignorar DST erra por uma hora duas vezes por ano.
- **Downtime é normal**: um gateway desligado por dias não pode despejar
  todas as ocorrências perdidas ao voltar.
- Reuso do que já existe (worker de 60s, entrega por canal, padrão de
  advisory lock do `uploads_worker`).

## Considered Options

1. **Uma única camada em Postgres** para tudo, migrando o scheduler pessoal.
2. **Uma única camada em SQLite**, com o workspace lendo de lá.
3. **Duas camadas independentes** com política de tempo compartilhada.

## Decision Outcome

Escolhida a **opção 3**: duas implementações, uma política.

- **Pessoal (SQLite)** — colunas aditivas em `scheduled_tasks`
  (`cron_expr`, `timezone`, `last_run_at`, `run_count`, `max_runs`,
  `attempts`); expressão cron via `croner`; tool `schedule_recurring`
  exposta ao agente. Reusa o worker e a entrega por canal existentes.
- **Workspace (Postgres)** — `garraia-workspace::recurrence` expande RFC 5545
  (`rrule`) e o `tasks_recurrence_worker` materializa a próxima ocorrência de
  tarefas recorrentes concluídas, sob advisory lock, lendo via função
  SECURITY DEFINER (migration 033) e **escrevendo** com
  `app.current_group_id` do próprio grupo para que o WITH CHECK do RLS
  continue valendo.

Política comum às duas camadas, deliberadamente idêntica:

- **Timezone IANA obrigatório na semântica**, default `America/New_York`
  (mesma convenção de datas narrativas do projeto). O horário local é
  preservado através da virada de DST.
- **Catch-up "executa uma vez"**: após downtime a tarefa roda uma única vez e
  retoma a cadência, em vez de replayar cada ocorrência perdida.
- **Regra exaurida (`UNTIL`/`COUNT`/`max_runs`) encerra a série** em vez de
  virar erro recorrente.
- **Falha não mata a série**: retry com backoff quadrático; esgotado o
  orçamento, a *ocorrência* é pulada e a recorrência sobrevive.

Rejeitadas: (1) forçaria Postgres numa instalação pessoal, contrariando o
local-first; (2) faria o multi-tenant depender de um banco por processo, sem
RLS nem isolamento entre réplicas.

## Consequences

**Boas.** Recorrência funciona sem Postgres; o workspace ganha o motor que a
migration 006 prometeu há tempos; a política de tempo é única e testável em
funções puras (14 unit tests entre as duas camadas, incluindo bordas de DST).

**Ruins.** Duas implementações de recorrência coexistem — `croner` (cron
5 campos) no pessoal e `rrule` (RFC 5545) no workspace. É intencional (os
formatos servem públicos diferentes: cron para quem escreve no chat, RRULE
para interop de calendário), mas exige manter as duas políticas em sincronia;
qualquer mudança de semântica precisa ser aplicada nos dois lados.

**Limites conhecidos.** A granularidade do scheduler pessoal continua sendo o
tick de 60s. O worker do workspace materializa a próxima ocorrência apenas
quando a tarefa é **concluída**; lembretes por `due_at` sem conclusão são um
slice futuro. A migration 033 e o worker não foram verificados contra
Postgres real no ambiente de desenvolvimento em que foram escritos (sem
Docker) — a lógica de expansão é coberta por testes puros.

## Links

- plan `plans/0355-recurrence-and-mcp-lifecycle.md`
- migration `crates/garraia-workspace/migrations/033_task_recurrence_engine.sql`
- ADR 0003 (Postgres para o Group Workspace) — contexto do multi-tenant
- ROADMAP §Tier-1 Tasks — item de recorrência que este ADR fecha
