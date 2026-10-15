# Segurança e Operação

## Reportar vulnerabilidade

**Não abra issue pública.** Reporte de forma privada via [GitHub Security Advisories](https://github.com/michelbr84/GarraRUST/security/advisories/new) ou **security@garraia.org**. Política completa e o que incluir no reporte: [SECURITY.md](https://github.com/michelbr84/GarraRUST/blob/main/SECURITY.md).

## Modelo de segurança

- [Visão geral](https://github.com/michelbr84/GarraRUST/blob/main/docs/security.md) — cofre AES-256-GCM, allowlists por canal, pareamento, bind em localhost por padrão
- [Hardening do gateway + ferramentas de execução](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardening-gateway.md) — perfis loopback/exposto, confirmação de comandos (safety_gate), modos/ToolPolicy, MCP com política ([receitas](https://github.com/michelbr84/GarraRUST/blob/main/docs/integrations/mcp-capacidades.md)) e [config de referência](https://github.com/michelbr84/GarraRUST/blob/main/config.hardened.example.yml)
- [Arquitetura security-first](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/security/architecture.md) · [Superfícies de ataque de agentes de IA](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/security/attack-surfaces.md) · [Checklist prático](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/security/checklist.md)
- [Threat model STRIDE](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md) · [Decisões de hardening](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/hardening-decisions.md)

## Perfis de execução (v0.4.4, ADR 0024)

- **`standard`** (default, seção ausente): a postura descrita acima — `ToolGate` fail-closed por modo, jail das file tools (`agent.file_roots`), piso `search` no WhatsApp pessoal, MCP `filesystem` autoprovisionado no workspace (`<data_dir>/workspace`, nunca `$HOME`).
- **`bash` sem humano no laço** (v0.4.5, #1272): em `standard`, o `garraia mcp-server` e o gateway só registram `bash` (e o `run_tests` do gateway) dentro de um sandbox `docker`/`podman` válido; sem ele a tool não existe, o boot avisa e o `/api/diagnostics` mostra `tools.bash`. `garraia chat`, com humano no terminal, não muda. Desde a v0.4.8 (#1272), um caminho alternativo em `standard`: declarar padrões em `agent.bash_allowlist` coloca o `bash` em modo allowlist-only no host (sem sandbox) — a lista vira fronteira e todo comando fora dela é negado antes do tier arriscado, com a denylist do `safety_gate` sempre na frente.
- **`isolated-pod`** (`execution.profile: isolated-pod` ou `GARRAIA_EXECUTION_PROFILE=isolated-pod`, que vence o arquivo): declaração do operador de que o processo roda num pod **descartável**. Regra: **poder total dentro do pod isolado; nenhum acesso implícito fora do pod.** O dono do WhatsApp (`channels.whatsapp_linked.owners`, conversa 1:1) recebe o piso `code` — `bash`, `file_write`, subagentes e toda tool MCP; o `filesystem` MCP nasce em `execution.pod_root`. Grupo e contato só pareado **nunca** herdam. O gate de comando arriscado do `bash`, o jail das file tools e `agent.sandbox` continuam ligados: o perfil libera ferramentas, não desliga proteções.
- **O que o perfil NÃO isola** (é declaração, não mecanismo): volume do host montado · socket do Docker/Podman · `--privileged` · `--pid=host` / `--network=host` · mounts não declarados · segredos do host no ambiente. Se algum vale para o seu container, ele não é um pod isolado — fique em `standard`.
- **Fail-closed em toda dúvida**: nunca inferido de `/.dockerenv`, cgroups ou env de runtime (testes varrem o fonte); valor inválido recusa o boot e é `Error` no `config check`; a env nunca é promovida ao arquivo por um `save`; identidade desconhecida, grupo ou lock envenenado caem no piso `standard`.
- **Onde aparece**: `WARN` único no boot; `Warning` permanente em `GET /api/diagnostics` (`execution.profile`, com origem, piso do dono, contagem de donos, raiz do MCP e `next_step`); check `mcp.filesystem_root` (avisa em `standard` quando um `mcp.json` anterior à v0.4.4 ainda aponta para `$HOME`); linha read-only `security.execution_profile` em `/api/settings/effective`; `garra whatsapp status`; sumário do `garra config check`.
- Guia: [`docs/execution-profiles.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/execution-profiles.md) · [threat model §5.15](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md) · [ADR 0024](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0024-perfis-de-execucao-isolated-pod.md).

## Política de acesso e capacidades (v0.4.6, ADR 0025)

- **Access Policy v2** (`channels.whatsapp_linked.access`, #1388): admissão explícita (`restricted` \| `open`), default declarado para o desconhecido, piso e teto por principal (`chat` \| `read` \| `full`, com escrita de arquivo à parte) e política independente por grupo. O nível é um **teto que compõe com o modo da sessão: só tira, nunca põe** — política e modo se cruzam no ponto mais restritivo. CLI (`garraia whatsapp access|level|write|block|unblock|users|remove|owner|unowner`), API admin e a página *WhatsApp Access* do console usam o **mesmo motor**, não três cópias da regra. Bloqueio vence `open`, `allow` e pareamento. `access.groups.enabled` declarado vence o `reply_in_groups` legado (#1501), e grupos valem na mensagem seguinte, sem restart (#1412). Todo comando de permissão deixa rastro no audit (#1414); identidade só pelos quatro últimos dígitos em toda tela e todo `--json`.
- **Classes de capacidade e teto por principal** (#1385, #1392): toda ferramenta declara o que faz em classes fechadas (`filesystem.read`, `filesystem.write`, `process.execute`, `network.read`, `message.send`, `device.*`, `memory.*`, `runtime.inspect`, `scheduling`, `mcp.read`, `mcp.write`). Nativas por tabela fechada; MCP pela operação conhecida ou pelas anotações do servidor. **Sem classe declarada, modo restrito falha fechado.**
- **Um jail por sessão governa file tools nativas E MCP `filesystem`** (#1482, #1383). Antes eram dois portões com regras diferentes no mesmo turno. Sessão sem projeto tem raiz em `<data_dir>/workspace/<sessão>` (nome = SHA-256 do `session_id`), criada `0700` (#1378, #1449, #1463) — nunca `/`, `$HOME` ou `pod_root`.
- **Ferramenta MCP não vira slash command** (#1386): o registro automático expunha toda tool MCP como `/comando`, **fora do `ToolGate`** dos modos.
- **Sessão por id separada em cliente e operador** (#1462). Ids de canal são adivinháveis por construção. `GET /api/sessions`, `GET /api/sessions/{id}/history`, `DELETE /api/sessions/{id}` e o `resume` sem token do `/ws` só alcançam as superfícies locais do operador (`api`, `vscode`, `web`, `parrot`); sessão de canal ou do mobile responde o mesmo `404 session not found` de id inexistente, byte a byte, e não é hidratada, desconectada nem listada. A leitura do operador é `GET /admin/api/sessions/{id}/history` (cookie do `/admin` + `manage_sessions`; `viewer` recebe 403).
- **Registro de capacidades como fonte única** (#1381, #1387, #1416): `garra_status`, `/api/diagnostics` (`tools.capabilities`), `GET /admin/api/capabilities` e a nota do prompt leem o mesmo registro, com estado (`visible` · `denied` · `unavailable` · `unhealthy` · `not_configured`), motivo e remediação. O modelo é proibido de inferir ausência a partir de invisibilidade: `denied` é "existe e não está liberada", nunca "não existe". Ferramenta registrada mas não operacional sai da lista chamável (#1425).
- **Redação**: `/api/diagnostics` não publica caminho absoluto do host (#1465), e o mascaramento de telefone cobre separadores comuns (#1514).
- Referências: [ADR 0025](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0025-classes-de-capacidade-e-politica-de-acesso-v2.md) · [threat model](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md) · [`docs/whatsapp.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md).

## Runbooks

| Situação | Runbook |
|---|---|
| Segredo vazou num commit | [secret-scanning-runbook.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/secret-scanning-runbook.md) |
| Incidente com dados pessoais (ANPD/GDPR, 72h) | [compliance/incident-response.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/compliance/incident-response.md) |
| Operação em produção | [production-runbook.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/production-runbook.md) |
| CodeQL (setup + supressões) | [codeql-setup.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/codeql-setup.md) · [codeql-suppressions.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/codeql-suppressions.md) |
| Proteção da branch main | [protect-main-ruleset.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/protect-main-ruleset.md) |

## Compliance e observabilidade

- [DPIA — LGPD/GDPR](https://github.com/michelbr84/GarraRUST/blob/main/docs/compliance/dpia.md)
- [Telemetria (OpenTelemetry + Prometheus)](https://github.com/michelbr84/GarraRUST/blob/main/docs/telemetry.md)
- [Deploy: Docker](https://github.com/michelbr84/GarraRUST/blob/main/docs/deployment.md) · [Runpod serverless](https://github.com/michelbr84/GarraRUST/blob/main/docs/deployment-runpod.md)
