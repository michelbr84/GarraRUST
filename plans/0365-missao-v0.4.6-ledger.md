# Plan 0365 — Missão v0.4.6: ledger único

> Prompt de 2026-09-26 (dono): escopo integral das 51 issues, #1462 opção 3, #1425 opção B,
> revalidação completa de #1390/#1429/#1418/#1461, ratchet por modularização, instaladores
> candidatos durante o trabalho, ZERO GATE só para publicação. DONE = v0.4.6 pública com
> assets/checksums verificados. Complementa o plan 0364 (pré-voo; checkpoints C1–C4).

## 0. Contadores (consultas reais)

| Quando (EDT) | `open_issues` | `open_PRs` | alertas | checks obrigatórios do SHA de `main` | regressões |
|---|---|---|---|---|---|
| 26/09 01:40 (início da missão) | 51 | 0 | 0 (#176 dismissed 00:35Z) | 6/6 verdes em `e1d62523` | 0 |

## 1. Lotes e dependências

- **L1 (núcleo, integrador):** a) confinamento do MCP `filesystem` ao jail da sessão — **achado de segurança** desta missão: em `standard` a raiz do MCP é `<data_dir>/workspace`, pai de todo diretório de sessão, e a #1384 liberou a leitura MCP no `search` → leitura cruzada entre sessões; b) classes de capacidade (`Tool::capacidades`, `ToolPolicy` por classe, teto por principal composto no `ToolGate`); c) Access Policy v2 (config `channels.whatsapp_linked.access`, engine puro, aplicação por turno com reload atômico). ADR 0025.
- **L2 (paralelo, worktrees, um integrador):** cli · api+audit · reg (registry/honestidade/breaker/opção B) · web · sess · e2e. Cada um só depende de L1.
- **L3:** #1462 (opção 3), #1461 (revalidação), instaladores candidatos (workflow `desktop.yml`/`release.yml` por dispatch), dogfood, release.

## 2. Ledger

| Item | Rótulos | Título | Lote | Critério pendente / nota | Branch | Teste / commit | Resolução |
|---|---|---|---|---|---|---|---|
| #1379 | P0 | persist project/workspace selection per remote session | L2-sess | projeto ativo por sessao remota (/project) | — | — | aberta |
| #1381 | P0 | add a Runtime Capability Registry as the single source of truth | L2-reg | Capability Registry | — | — | aberta |
| #1383 | P0 | unify native file-tool and filesystem-MCP workspace policy | L1-a | nucleo entregue pelo confinamento (#1482); restam: override explicito visivel em diagnostics, migracao, testes de paridade cruzada | `sec/mcp-filesystem-confinado-ao-jail` | `mcp::confinamento::tests` (11) | em curso |
| #1385 | P0 | classify tools by capabilities instead of relying only on tool nam | L1-b | classes de capacidade no Tool + ToolPolicy | — | — | aberta |
| #1387 | P0 | runtime honesty — distinguish hidden, denied, unavailable and not  | L2-reg | honestidade do runtime (prompt + estados) | — | — | aberta |
| #1388 | P0 | WhatsApp Access Policy v2 with explicit restricted/open modes | L1-c | modelo/engine Access Policy v2 (restricted|open, users, default, groups) | — | — | aberta |
| #1390 | P0 | wildcard/open WhatsApp users must never receive Full access by def | L1-c | wildcard nunca full: engine + teste | — | — | aberta |
| #1391 | P0 | add per-identity effective permissions for WhatsApp-linked | L1-c | principal por identidade + teto de sessao | — | — | aberta |
| #1392 | P0 | write permission must be enforced through ToolGate capabilities, n | L1-b | write compilado em capacidades; nunca booleano | — | — | aberta |
| #1396 | P1 | CLI — add `garraia whatsapp access open|restricted` | L2-cli | access open|restricted | — | — | aberta |
| #1397 | P1 | CLI — add per-phone `whatsapp write on|off` | L2-cli | write on|off | — | — | aberta |
| #1398 | P1 | CLI — add WhatsApp access levels `chat|read|full` | L2-cli | niveis chat|read|full | — | — | aberta |
| #1399 | P1 | CLI — configure default access for unknown WhatsApp users | L1-c/L2-cli | default de desconhecidos (engine) + comando | — | — | aberta |
| #1400 | P1 | CLI — `garraia whatsapp access` should print the complete effectiv | L2-cli | access imprime politica efetiva + json | — | — | aberta |
| #1401 | P1 | CLI — add `garraia whatsapp access reset` safe defaults | L2-cli | access reset | — | — | aberta |
| #1402 | P1 | Web Console — WhatsApp Access & Permissions page | L2-web | pagina Access & Permissions | — | — | aberta |
| #1403 | P1 | Web Console — Add phone workflow for WhatsApp | L2-web | add phone | — | — | aberta |
| #1404 | P1 | Web Console — Remove, Block and Owner controls for WhatsApp users | L2-web | remove/block/owner | — | — | aberta |
| #1405 | P1 | Web Console — `Anyone (*)` toggle for WhatsApp open access | L2-web | toggle Anyone | — | — | aberta |
| #1406 | P1 | Web Console — per-phone Write ON/OFF control | L2-web | write on/off | — | — | aberta |
| #1407 | P1 | Web Console — Chat / Read / Full access selector | L2-web | seletor chat/read/full | — | — | aberta |
| #1408 | P1 | Web Console — configure wildcard/default WhatsApp permissions | L2-web | default curinga | — | — | aberta |
| #1409 | P1 | Web Console — show the effective mode for each WhatsApp session | L2-web | modo efetivo por sessao | — | — | aberta |
| #1411 | P1 | Web Console — capability matrix per WhatsApp principal | L2-web | matriz de capacidades | — | — | aberta |
| #1412 | P1 | WhatsApp policy changes must hot-reload reliably across CLI/API/We | L1-c | hot reload atomico por turno | — | — | aberta |
| #1413 | P1 | add dry-run and impact preview for WhatsApp permission changes | L2-cli/L2-api | dry-run + preview | — | — | aberta |
| #1414 | P1 | local audit log for channel permission and identity changes | L2-cli/L2-api | audit log local | — | — | aberta |
| #1415 | P1 | add a per-conversation Capability Panel in the Web Console | L2-web | painel por conversa | — | — | aberta |
| #1416 | P1 | garra_status must report operational capability health, not only r | L2-reg | garra_status saude operacional | — | — | aberta |
| #1417 | P1 | circuit breaker for repeatedly failing or timing-out tools | L2-reg | circuit breaker | — | — | aberta |
| #1418 | P1 | make NoRoots/file-tool errors actionable | L2-reg | NoRoots acionavel (revalidar 6/6) | — | — | aberta |
| #1420 | P1 | Web Console — add a `Test WhatsApp` diagnostics workflow | L2-web | Test WhatsApp (doctor) | — | — | aberta |
| #1421 | P1 | resolve WhatsApp phone/JID/LID identities transparently | L1-c | identidades phone/JID/LID | — | — | aberta |
| #1422 | P1 | Web Console — show rejected WhatsApp messages safely | L2-web | recusas | — | — | aberta |
| #1423 | P1 | independent WhatsApp group access and capability policies | L1-c | grupos como fronteira propria | — | — | aberta |
| #1424 | P1 | persist WhatsApp mode, project and access context safely per sessi | L2-sess | persistencia segura de modo/projeto/acesso | — | — | aberta |
| #1425 | P1 | hide or mark channel tools unavailable when their integration is n | L2-reg | tools de canal so operacionais (opcao B) | — | — | aberta |
| #1426 | P1 | E2E dogfood — clean install to WhatsApp read/write workflow | L2-e2e | E2E instalacao limpa -> WhatsApp (fixture) | — | — | aberta |
| #1427 | P1 | E2E multi-user WhatsApp permissions — owner Full, user Read, wildc | L2-e2e | E2E multi-usuario | — | — | aberta |
| #1428 | P1 | behavioral tests for truthful agent capability answers | L2-reg | testes comportamentais | — | — | aberta |
| #1429 | P2 | WhatsApp wizard should offer access management immediately after Q | L2-cli | wizard oferece gestao de acesso (revalidar 6/6) | — | — | aberta |
| #1431 | P2 | improve vault onboarding for WhatsApp linked-device session secret | L2-cli | vault onboarding | — | — | aberta |
| #1432 | P2 | Settings Registry should render first-class phone/identity control | L2-web | settings registry phone controls | — | — | aberta |
| #1433 | P2 | Web Console — global Agents & Permissions administration page | L2-web | Agents & Permissions global | — | — | aberta |
| #1434 | P2 | reusable permission presets — Chat Only, Read, Developer, Full Pod | L2-cli | presets | — | — | aberta |
| #1435 | P2 | import/export channel permission policies without secrets | L2-cli | import/export | — | — | aberta |
| #1436 | P2 | Web Console — memory and run-ledger retention administration | L2-web | retencao memoria/runs | — | — | aberta |
| #1438 | P2 | local-only reliability observability for tools and channels | L2-reg | observabilidade local | — | — | aberta |
| #1439 | P2 | mandatory AAA dogfood gate before release tags | L2-e2e | gate de dogfood automatizado + manual | — | — | aberta |
| #1461 | security,P1 | security: iMessage usa group_name cru (renomeavel por qualquer partici | L3 | iMessage: revalidar e fechar com prova | — | — | aberta |
| #1462 | security,P1 | security: X-Session-Id forjado na rota compat OpenAI le resumo + ate 1 | L3 | X-Session-Id opcao 3 | — | — | aberta |

Bugs/regressões descobertos na missão entram aqui como linhas novas (com issue própria quando
não houver uma que os cubra).

## 3. Diário

| Hora (EDT) | O quê | Evidência |
|---|---|---|
| 26/09 01:40 | Revalidação: 51/0/0, #176 dismissed, `e1d62523` 6/6 verdes; critérios das 51 lidos; código do portão lido | consultas `gh` nesta sessão |
| 26/09 01:55 | Achado: raiz do MCP `filesystem` = `<data_dir>/workspace` (pai das sessões) + leitura MCP no `search` (#1384) → leitura cruzada; vira L1-a | `bootstrap/execution.rs:108-131` |
| 26/09 02:05 | Issue **#1482** aberta para o achado (security, P0). RED: 10 testes de `mcp::confinamento` (todo!). GREEN: `confinar_argumentos` + `McpManager::set_jail_das_file_tools` + `McpTool::execute` confina antes de `call_tool_once` + boot entrega o jail; duas guardas de fonte; agents 756 (`--features mcp`), gateway bootstrap 65, clippy limpo | branch `sec/mcp-filesystem-confinado-ao-jail` |
