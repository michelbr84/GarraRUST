# 0355 — Robustez sempre-online: ciclo de vida MCP + agendamento recorrente

**Status:** ✅ entregue 2026-08-29
**Branch:** `claude/garraia-chat-mcp-tools-70mu4a`
**ADR:** [0013](../docs/adr/0013-scheduling-two-tier-recurrence.md)

## Motivação

Um comparativo externo GarraIA × Hermes elegeu como prioridades (1) cron
recorrente e (2) robustez do ciclo de vida MCP. A auditoria do código
confirmou o alvo e encontrou defeitos piores do que os relatados. O sintoma
citado no comparativo (`RuntimeWarning: coroutine never awaited`) é do Python
do Hermes, não nosso — mas o nosso lado tinha dois defeitos críticos que se
mascaravam mutuamente.

## Parte 1 — Ciclo de vida MCP

| # | Defeito | Correção |
|---|---------|----------|
| §0 | `RunningService::is_closed()` nunca vira `true` quando o filho morre sozinho (rmcp só o alterna em `close()`/`cancel()`/`waiting()`), então `check_and_reconnect` era **no-op** e o registry reportava `Running` para sempre | `McpConnection::is_alive()` usando `peer().is_transport_closed()` |
| §3 | `McpTool` guardava `Arc<Peer>` capturado no boot; o `AgentRuntime` é imutável depois do `Arc`, então após qualquer reconexão as tools do LLM falavam com um transporte morto **permanentemente** | `McpTool` guarda `(Arc<McpManager>, nome_servidor)` e resolve o peer atual **por chamada** |
| — | 4 RPCs de resources/prompts aguardavam **sem timeout** segurando `connections.read()`; RwLock write-fair ⇒ um servidor travado enguiçava o subsistema | `peer_and_timeout()` solta o guard antes do await e aplica o timeout do servidor |
| — | `RestartState::reset()` no handshake ⇒ crash-loop infinito (nunca alcançava `max_restarts`) | reset só após `STABILITY_WINDOW` (60s) de conexão viva |
| — | Falha de conexão no boot nunca era retentada (o servidor não entrava em `connections`) | fila `pending` varrida pelo health monitor |
| — | `disconnect_all` sequencial e sem teto; cleanup depois de um `?` ⇒ órfãos quando o listener falhava | `join_all` sob timeout de 10s; serve result propagado **após** a limpeza |
| — | `kill -9` no gateway deixava todos os filhos MCP vivos | `PR_SET_PDEATHSIG` nos filhos + `stop_daemon` sinalizando o process group com escalada a SIGKILL |

§0 e §3 foram num commit só de propósito: corrigir §0 isolado transformaria
um no-op silencioso em quebra permanente silenciosa.

**Testes (não existia nenhum):** fixture `fake_mcp_server.py` (stdio
JSON-RPC, stdlib apenas, com `--crash-after-calls`/`--ignore-eof`/`--hang`) +
`tests/mcp_lifecycle.rs`. Verificado que os testes pinam defeitos reais:
restaurando `is_closed()` dois falham; restaurando o peer congelado,
`tool_survives_reconnect` falha sozinho.

## Parte 2 — Recorrência pessoal (SQLite)

`garraia-db::recurrence` (croner + chrono-tz), colunas aditivas em
`scheduled_tasks`, tool `schedule_recurring`, reagendamento no worker
existente e retry com backoff quadrático (`fail_task` era terminal — o
CHANGELOG alegava retry que não existia). Detalhes de política no ADR 0013.

## Parte 3 — Recorrência do workspace (Postgres)

`garraia-workspace::recurrence` (RFC 5545 via `rrule`), migration 033
(`recurrence_tz`, `recurrence_spawned_at`, índice parcial, função SECURITY
DEFINER `due_task_recurrences`) e `tasks_recurrence_worker` com advisory
lock. Fecha o item de recorrência do ROADMAP §Tier-1 Tasks.

## Honestidade de documentação

Corrigidas alegações falsas encontradas na auditoria: `docs/src/README.md` e
`docs/index.md` diziam que cron já existia; `CHANGELOG.md` alegava teto de
24h (o código diz 30 dias) e retry com backoff (inexistente até esta
entrega); READMEs diziam "cron no roadmap" enquanto o ROADMAP não mencionava
cron em lugar nenhum.

## Verificação

`cargo check`/`clippy`/`test` limpos em `garraia-agents`, `garraia-db`,
`garraia-gateway`, `garraia-workspace` e `garraia`.

**Não verificado:** migration 033 e o worker do workspace não rodaram contra
Postgres real — o ambiente de desenvolvimento não tinha Docker, então
`migration_smoke` e os testes com testcontainers pulam/falham por falta de
socket. A lógica de expansão RRULE é coberta por 8 testes puros.
