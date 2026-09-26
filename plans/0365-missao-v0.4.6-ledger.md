# Plan 0365 — Missão v0.4.6: ledger único

> Prompt de 2026-09-26 (dono): escopo integral das 51 issues, #1462 opção 3, #1425 opção B,
> revalidação completa de #1390/#1429/#1418/#1461, ratchet por modularização, instaladores
> candidatos durante o trabalho, ZERO GATE só para publicação. DONE = v0.4.6 pública com
> assets/checksums verificados. Complementa o plan 0364 (pré-voo; checkpoints C1–C4).

## 0. Contadores (consultas reais)

| Quando (EDT) | `open_issues` | `open_PRs` | alertas | checks obrigatórios do SHA de `main` | regressões |
|---|---|---|---|---|---|
| 26/09 01:40 (início da missão) | 51 | 0 | 0 (#176 dismissed 00:35Z) | 6/6 verdes em `e1d62523` | 0 |
| 26/09 06:45 | 51 | 4 (#1484, #1486, #1487, #1488 trem) | 0 | `e9e97673`: 6/6 verdes na #1483 antes do merge; run de `main` em curso | 0 |
| 26/09 09:10 | 50 | 0 | 0 (code scanning 0, dependabot 0) | `87c66ec3`: Format verde, 5 obrigatórios em curso | 0 |

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
| #1385 | P0 | classify tools by capabilities instead of relying only on tool names | L1-b | entregue no nucleo (branch); restam: diagnostics mostrar classificacao (L2-reg) | `feat/1385-classes-de-capacidade` | modes/capacidades/teto tests | em curso |
| #1387 | P0 | runtime honesty — distinguish hidden, denied, unavailable and not  | L2-reg | honestidade do runtime (prompt + estados) | — | — | aberta |
| #1388 | P0 | WhatsApp Access Policy v2 with explicit restricted/open modes | L1-c | modelo/engine Access Policy v2 (restricted|open, users, default, groups) | — | — | aberta |
| #1390 | P0 | wildcard/open WhatsApp users must never receive Full access by def | L1-c | wildcard nunca full: engine + teste | — | — | aberta |
| #1391 | P0 | add per-identity effective permissions for WhatsApp-linked | L1-c | principal por identidade + teto de sessao | — | — | aberta |
| #1392 | P0 | write permission must be enforced through ToolGate capabilities, never a b | L1-b | `politica_do_nivel(nivel, write)` compila write em `filesystem.write`+`mcp.write` como teto; restam: audit log (L2-api), E2E nativo+MCP (L2-e2e), UI toggle (L2-web) | `feat/1385-classes-de-capacidade` | `niveis_chat_read_full_com_write_ligado_e_desligado` | em curso |
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
| #1462 | security,P1 | security: X-Session-Id forjado na rota compat OpenAI le resumo + ate 1 | L3 | X-Session-Id opcao 3 | `sec/1462-leitura-de-sessao-opcao-3` | `leitura_de_sessao_por_id_do_cliente.rs` 12/12 | **fechada** (PR #1484 via trem #1488, `87c66ec3`) |

Bugs/regressões descobertos na missão entram aqui como linhas novas (com issue própria quando
não houver uma que os cubra).

## 3. Diário

| Hora (EDT) | O quê | Evidência |
|---|---|---|
| 26/09 01:40 | Revalidação: 51/0/0, #176 dismissed, `e1d62523` 6/6 verdes; critérios das 51 lidos; código do portão lido | consultas `gh` nesta sessão |
| 26/09 01:55 | Achado: raiz do MCP `filesystem` = `<data_dir>/workspace` (pai das sessões) + leitura MCP no `search` (#1384) → leitura cruzada; vira L1-a | `bootstrap/execution.rs:108-131` |
| 26/09 02:05 | Issue **#1482** aberta para o achado (security, P0). RED: 10 testes de `mcp::confinamento` (todo!). GREEN: `confinar_argumentos` + `McpManager::set_jail_das_file_tools` + `McpTool::execute` confina antes de `call_tool_once` + boot entrega o jail; duas guardas de fonte; agents 756 (`--features mcp`), gateway bootstrap 65, clippy limpo | branch `sec/mcp-filesystem-confinado-ao-jail` |
| 26/09 02:15 | **PR #1483** aberta (Closes #1482). Prova ao vivo opt-in contra o `server-filesystem` real: sem jail serve a outra sessao; com jail recusa; 1/1 em 0,65 s | comentário na #1483 |
| 26/09 02:25 | Delegação em worktrees (concorrência 2): agente **instaladores/dogfood Linux** (pacote candidato + container limpo) e agente **#1462 opção 3** (leitura do cliente restrita às sessões alcançáveis; export do console por rota admin autenticada; mobile próprio) | Agent tool |
| 26/09 02:50 | L1-b em curso na branch `feat/1385-classes-de-capacidade` (empilhada na #1483): `capacidades.rs` (13 classes, tabela fechada das nativas, MCP por operacao/anotacoes `readOnlyHint`/`destructiveHint`), `Tool::capacidades()`, `ToolPolicy.{allowed,denied}_capabilities`+`no_tools`, `TetoDeCapacidades` composto no `ToolGate` (`permite_com_capacidades`, `explica_recusa`), `Nivel {chat,read,full}` + `politica_do_nivel(nivel, write)`, `ExecContext.teto`, runtime pergunta por nome+classe em todos os pontos (`portao_permite`), inventario com classes. RED 8 → GREEN 9 (incl. guarda dos call sites) | testes `modes::tests::{whitelist_por_classe…,niveis_chat_read_full…,teto_do_principal…}`, `runtime/tests/teto_por_capacidade.rs` |
| 26/09 03:52 | L1-b verificado: agents 768 (lib, +mcp) / 796 (todos os alvos), gateway 1650, cli 829, clippy limpo, fmt ok. ADR 0025 escrita e indexada; fragmento `changelog.d/added/1385-classes-de-capacidade.md` | `cargo test` nesta sessão |
| 26/09 06:2x | Sessão cortada pelo limite de uso (5 h) com os dois subagentes ainda rodando; retomada 06:20 EDT. Commit `0e885516` (L1-b) empurrado; PR #1485 aberta empilhada na #1483 | `git log` |
| 26/09 06:22 | **#1483 MERGED** (`e9e97673`; 6/6 obrigatórios verdes). O `--delete-branch` fez o GitHub **fechar** a #1485 (base apagada) — reaberta como **#1486** contra `main` (1 commit) | `gh pr view 1483/1485/1486` |
| 26/09 06:25 | Subagentes cortados pelo mesmo limite: **#1462 opção 3** já tinha aberto a **PR #1484** (`154012b9`, 6/6 verdes + macOS; worktree limpa); **dogfood Linux** tinha 2 commits empurrados em `feat/1426-dogfood-linux-limpo` (script + fix do zumbi) sem PR | notificações das tasks; `git -C .claude/worktrees/... status` |
| 26/09 06:30 | Revisão do integrador da #1484 (diff inteiro: `state.rs`, `api.rs`, `ws.rs`, `admin/*`, trechos do `webchat.html`): pronto — regra única em `sessao_da_api`, recusa antes de hidratar, rota admin em `auth_routes` + `ManageSessions` + audit, `/ws` resume fechado, mobile pelo `sub`. Comentário postado na PR | `gh pr review 1484 --comment` |
| 26/09 06:35 | Dogfood: run final `2026-09-26-0347` (candidato `846e96c1`, `.deb` local via nfpm pinado, ubuntu:24.04 limpo) **13/13 PASSOU** em 370 s, resposta real do `qwen3.5:0.8b`. RED do zumbi: run `0331` (`.deb` v0.4.5, PID 1 = `sleep`): "Process 127 survived SIGTERM and SIGKILL". Prova do fix pelo integrador **sem `--init`** com o `.deb` do 0347: daemon PID 64 vira `Z`, `garraia stop` → "GarraIA stopped." em 0 s. Unitário `zumbi_nao_conta_como_processo_rodando` re-executado: 1 passed. **PR #1487** aberta | `dogfood/linux/2026-09-26-0347/{resumo.txt,repro-zumbi-sem-init/}` na worktree do agente |
| 26/09 06:40 | **Trem `train/v046-c` → PR #1488** (`--no-ff`: #1484 + #1486 + #1487; base `e9e97673`; sem conflito; `cargo check` da união exit 0). Auto-merge desligado nas três | `gh pr view 1488` |
| 26/09 07:00 | **L1-c em curso** na worktree `GarraRUST-l1c`, branch `feat/1388-access-policy-v2` (empilhada no trem): `bootstrap/whatsapp_linked/politica.rs` (`PoliticaDeAcesso::da_secao` fail-closed, `Principal {Dono,Usuario,Pareado,Desconhecido,Grupo,Bloqueado,Estranho}`, `principal_do_turno`, `teto_do_principal`), `LinkedSettings.access` + `entrada_de`/`e_dono`/`responde_em_grupo`, `PortaoDoCanal` com `bloqueados`/`aberto`/`pareado()`, `admissao_vigente` relê a política inteira, `turno` põe `exec.teto` e loga `principal`/`alcance`. Decisão de compat: `allow` legado e grupo sem política ficam **sem teto** (o piso de modo decide, como sempre); nível/`write` só onde declarado; pareado por código = `read`. RED 16/17 → GREEN 114/114 no módulo | `tests/politica.rs` (17 testes) |
| 26/09 09:05 | PC reiniciou sozinho (~07:10–09:04 EDT); worktrees, commit do ledger e o L1-c não commitado sobreviveram. **Trem #1488 MERGED** (`87c66ec3`) com 12/12 checks verdes → #1484, #1486, #1487 `MERGED`; #1462 fechada pelo `Closes` | `gh pr view`, `git status` |
| 26/09 09:20 | L1-c: aviso único por mudança da política normalizada (`avisar_politica_normalizada`, boot + turno), docs `docs/whatsapp.md` §"Politica de acesso por principal", ADR 0025 §4 emendada (legado sem teto; grupo declarado honrado; pareado `read`), fragmento `changelog.d/added/1388-access-policy-v2.md` | worktree `GarraRUST-l1c` |
