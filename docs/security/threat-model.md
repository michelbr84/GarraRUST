# GarraIA — STRIDE Threat Model

- **Status:** Draft v1 (2026-04-21)
- **Owner:** @michelbr84 + `@security-auditor`
- **Issue:** [GAR-398](https://linear.app/chatgpt25/issue/GAR-398)
- **Plan:** [`plans/0031-compliance-docs-batch.md`](../../plans/0031-compliance-docs-batch.md)
- **Scope:** Gateway HTTP/WS, `garraia-auth`, `garraia-storage` (planned), `garraia-plugins` (WASM), `garraia-channels` (webhooks), mobile apps.
- **Supersedes:** none
- **Review cadence:** trimestral + após qualquer ADR novo que altere surface de segurança.

---

## Sobre STRIDE

STRIDE é um framework de modelagem de ameaças proposto por Loren Kohnfelder e Praerit Garg na Microsoft (2002). Classifica ameaças em 6 categorias:

- **S — Spoofing** — impersonation de identidade (user ou serviço).
- **T — Tampering** — modificação não-autorizada de dados (em trânsito ou em repouso).
- **R — Repudiation** — usuário nega ação sem evidência contrária (gap de audit).
- **I — Information disclosure** — vazamento de dados (PII, secrets, content).
- **D — Denial of service** — degradação/queda de disponibilidade.
- **E — Elevation of privilege** — ator ganha permissões além do escopo.

Cada componente abaixo tem uma matriz com: ameaça → cenário concreto → mitigação atual (shipped) → mitigação planejada (roadmap issue).

---

## Trust boundaries

```
[Internet] ─┬─► [Reverse proxy / CDN (optional)]
            │
            └─► [garraia-gateway] (Axum HTTP+WS)
                   │
                   ├─► [garraia-auth]    (LoginPool + SignupPool BYPASSRLS)
                   ├─► [AppPool]         (garraia_app RLS role)
                   ├─► [garraia-db]      (legacy SQLite; CLI/dev)
                   ├─► [garraia-workspace] (Postgres 16 + pgvector)
                   ├─► [garraia-storage]   (ADR 0004 — LocalFs/S3/MinIO)
                   ├─► [garraia-plugins]   (WASM sandbox — Fase 2.2)
                   ├─► [garraia-channels]  (webhooks Telegram/Discord/Slack/WhatsApp/iMessage)
                   └─► [garraia-agents]    (LLM providers: OpenAI/OpenRouter/Anthropic/Ollama)
```

Boundaries críticos:
- **Internet ↔ Gateway**: TLS 1.3 (ADR futuro, GAR-383), rate limit (plan 0022), XFF fail-closed (plan 0022/0023).
- **Gateway ↔ Postgres**: 3 roles separados (LoginPool BYPASSRLS, SignupPool BYPASSRLS, AppPool RLS-enforced) — ADR 0005.
- **Gateway ↔ external providers** (OpenAI etc.): outbound TLS, secrets via CredentialVault (GAR-410 track).
- **Gateway ↔ channels webhook**: inbound HMAC signature verification (Telegram/Discord/Slack — exists; WhatsApp pending audit).

---

## 1. Gateway HTTP/WS (`garraia-gateway`)

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | Atacante apresenta JWT forjado; atacante imita XFF header; api_key comprometida. | JWT HS256 com `GARRAIA_JWT_SECRET` ≥32B validado via `jsonwebtoken`, algorithm-confusion guards (ADR 0005). `real_client_ip` + `GARRAIA_TRUSTED_PROXIES` fail-closed (plan 0022/0023). `api_keys.key_hash` Argon2id + `scopes` JSON restritivo. | Migrar `api.rs` + `admin/middleware.rs` remanescentes para `real_client_ip` (plan 0024+ follow-up). Rotação programática de api_keys + audit `api_key.revoked`. |
| **T** Tampering | Request body modificado em trânsito; DB row alterada por bypass RLS. | TLS 1.3 target (GAR-383 pending). WITH CHECK policies em RLS (migration 007/013). Audit events em mutations de workspace (plan 0021). | GAR-383 (TLS hardening) + integrity HMAC em storage (ADR 0004 §Security policy 4 — planned). |
| **R** Repudiation | Usuário nega ter aceito invite / promovido membro; nega ter enviado mensagem. | `audit_events` em `invite.accepted`, `member.role_changed`, `member.removed` (plan 0021). `sessions.created_at` + `sessions.last_seen_at`. | Audit `message.sent` + `file.presign_get_issued` (ADR 0004). |
| **I** Information disclosure | Error message vaza stack trace; JWT em log; cross-tenant via 403 vs 404. | RFC 9457 Problem Details redige conteúdo interno; REDACT_HEADERS cobre bearer/auth/cookie + IAP variants (plan 0025/0026); 404 em cross-tenant (plan 0016 + ADR 0004). | Cardinality guard em `/metrics` (plan 0025 M1). |
| **D** Denial of service | Slowloris; connection flood; body bomb; rate-limit bypass via múltiplas conexões. | Rate limiter com per-user JWT sub key (plan 0022 F-03); per-route limits (accept/setRole/DELETE tuned, plan 0021); Axum body limit default. | Body size limit explícito por rota; timeout agressivo em `timeouts` config (plan 0024+). GAR-402 fuzzing de parsers. |
| **E** Elevation of privilege | Promote-to-owner via API; último owner deleta a si mesmo; RLS bypass via `app.current_user_id` injeção. | Capability gate + hierarchy gate (plan 0020). Last-owner invariant (COUNT FILTER pós-UPDATE + SELECT FOR UPDATE). `SET LOCAL` só aceita UUID validado. | Continuar cobertura de authz matrix cross-group (plan 0014 + futuros endpoints). |

---

## 2. `garraia-auth` (Identity Provider)

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | Login com password roubado; session hijack via access token; **token de invite interceptado em transit** (email/link sharing). | Argon2id m=64MiB/t=3/p=4 (ADR 0005); PBKDF2 legacy com dual-verify + lazy upgrade transacional; access token 15 min + refresh opaco HMAC-SHA256 separado. `group_invites.token_hash` Argon2id + expiry ≤ 7 dias + race-safe UPDATE guard (plan 0019). | Hardware-based MFA (plan futuro, out of scope Alpha). TLS enforcement para transport (GAR-383). |
| **T** Tampering | Alterar `role='member'` → `role='owner'` direto no DB. | RLS FORCE em `group_members`; partial UNIQUE index `WHERE role='owner' AND status='active'` (migration 012); WITH CHECK policies. | — |
| **R** Repudiation | Usuário nega tentativa de login após credential stuffing. | Audit events em `auth.login.*` + `auth.refresh.*` + `auth.logout.*` terminals (plan 0011/0012); `ip` + `user_agent` registrados. | Retain audit > 90 dias (RoPA — DPIA); export para SIEM externo. |
| **I** Information disclosure | Anti-enumeration em email inexistente (timing attack); password hash em logs. | `DUMMY_HASH` constant-time em `build.rs` + `subtle::ConstantTimeEq`; `SecretString` em `Credential.password`; `RedactedStorageError` wrapper; 401 byte-identical em todos os modos de falha. | — |
| **D** Denial of service | Brute force em login; DoS via signup enxurrada. | Rate limit em `/auth/*` (20/min per-IP + per-user fallback, plan 0022); login pool `max_connections` controla scan. | Migrar `/auth/*` do deprecated middleware para novo (plan 0022 follow-up plano 0026+). CAPTCHA em signup (out of scope Alpha). |
| **E** Elevation of privilege | Usuário obtém access token de outro usuário; JWT sub swap; session fixation. | `SET LOCAL app.current_user_id = $1` verificado transacionalmente; `FromRequestParts` para `Principal` extractor não-clonável; `SignupPool` newtype ≠ `LoginPool` newtype (CLAUDE.md regra 11/13). | — |

---

## 3. `garraia-storage` (ADR 0004 — planned)

Componente ainda não implementado (crate `garraia-storage` é tracked em GAR-394). Matriz abaixo é **pre-implementation threat model** para informar o design.

| STRIDE | Cenário concreto | Mitigação prevista (ADR 0004) | Gap |
|---|---|---|---|
| **S** Spoofing | Presigned URL reutilizada por terceiro; operador substitui creds S3. | Presigned URL escopado a `{group_id}/{key}` + HMAC sobre path; TTL 30s–900s; Content-Disposition attachment. | — |
| **T** Tampering | Operador substitui blob no bucket; bit-rot em LocalFs. | HMAC-SHA256 `{key}:{version}:{sha256}` em `file_versions.integrity_hmac` verificado em `get`. | — |
| **R** Repudiation | Usuário nega upload de arquivo sensível. | Audit `file.uploaded`, `file.presign_get_issued`, `file.deleted` + `file.access_denied` para tentativas cross-tenant. | — |
| **I** Information disclosure | Presigned URL em log/Referer; enumeração de file_id; diretório cross-tenant. | `Referrer-Policy: no-referrer` header; S3 access log filter documentado; 404 em cross-tenant; path sanitization em `ObjectKey::new` (anti-traversal). | Testar `Referrer-Policy` contra proxies em prod. |
| **D** Denial of service | Upload bomba (1TB); abuse de presigned URL para egress. | tus expira uploads incompletos 24h; presigned URL TTL 15 min cap; rate limit por grupo. | Quota de storage por grupo (fase futura). |
| **E** Elevation of privilege | Escape de `{group_id}` prefix via path traversal; right-to-erasure contornado por v1-immune. | Charset restrito `[a-zA-Z0-9_\-./]` + rejeita `.`/`..`. Right-to-erasure tem flag `--include-origin` (ADR 0004 §Versionamento). | — |

---

## 4. `garraia-plugins` (WASM sandbox — Fase 2.2)

Hoje plugin system é scaffold wasmtime sem runtime efetivo. Matriz reflete design pretendido.

| STRIDE | Cenário concreto | Mitigação prevista | Gap |
|---|---|---|---|
| **S** Spoofing | Plugin malicioso apresenta identidade de plugin confiável. | Plugin registry com hash (sha256) + signed manifest. | Registry ainda não existe. |
| **T** Tampering | Plugin escreve em FS fora de sandbox; modifica state global. | `wasmtime` sandbox (`WasiCtxBuilder` sem filesystem, sem network default); `engine.allocation_strategy(OnDemand)`. | Policy grants explícitos ainda por definir. |
| **R** Repudiation | Plugin executa ação destrutiva sem audit. | Cada chamada de host function emite `audit_events` (`plugin.call.*`). | Design pendente. |
| **I** Information disclosure | Plugin lê env vars do processo; extrai model outputs. | `WasiCtxBuilder::envs([])`; plugin só recebe inputs explícitos. | — |
| **D** Denial of service | Plugin CPU-bound infinito; plugin OOM. | `fuel`-based metering em `wasmtime`; `InstanceLimits` memory cap. | — |
| **E** Elevation of privilege | Plugin chama função de host não-exportada; escape de sandbox. | Export apenas de host functions documentadas; audit cobre cada chamada. | Ship matrix de host functions quando runtime materializar (GAR plugin epic). |

---

## 5. `garraia-channels` (webhooks)

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | Atacante forja webhook do Telegram para injetar comandos. | Secret token em URL query param (Telegram); signature verify Discord/Slack. | Auditar WhatsApp + iMessage adapters (plan futuro). |
| **T** Tampering | Replay de webhook antigo; modificar payload. | Timestamp tolerance + HMAC (Slack). | Replay protection via nonce em Telegram (currently none — documented gap). |
| **R** Repudiation | Ação via channel sem audit (ex.: user bloqueia bot, nega ter). | `sessions` registra `telegram-{chat_id}`. | Explicit audit `channel.*` events. |
| **I** Information disclosure | Bot leak de system prompt via `/debug` injection. | Tools whitelist + system prompt template não-user-influenced. | Validate all channel inputs as untrusted (fuzzing GAR-402). |
| **D** Denial of service | Flood de messages via bot. | Rate limit per `session_id` (inherited from gateway). | Per-channel dedicated limits. |
| **E** Elevation of privilege | Admin command via DM sem role check. | Admin allowlist em `channels.*.admin_users` config. | Alinhar com RBAC central (post GAR-410). |

---

## 5.5. `garraia-agents` (LLM providers outbound)

Componente: `crates/garraia-agents/src/providers/*.rs` (OpenAI, OpenRouter, Anthropic, Ollama, mistral.rs planned per ADR 0001). Trust boundary **saindo** do gateway para terceiros.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | Provider endpoint DNS hijack; atacante impersona OpenAI API. | HTTPS hardcoded + SNI validation (padrão reqwest/tls). | Certificate pinning para providers críticos (plano futuro). |
| **T** Tampering | MITM altera resposta do provider injetando tool call malicioso. | HTTPS end-to-end. | Validation estruturada de response schema (JSON schema enforcement além de serde). |
| **R** Repudiation | Provider deleta logs após incidente de breach upstream. | Local audit de chamadas outbound (request_id) armazenado em `audit_events` quando shipped. | Estender audit `agent.request.sent` + `agent.response.received` com redacted content hash. |
| **I** Information disclosure | **Conteúdo de message enviado ao provider terceiro** — provider pode reter conforme política interna (OpenAI retém 30 dias por default; Anthropic zero-retention opt-in). **Risco primário do componente.** | TOS informa usuário; `do-not-retain` headers quando provider suporta (ex.: `OpenAI-Beta: no_retention`); local-first (Ollama / mistral.rs ADR 0001) elimina esse caminho. | PII scrubbing opcional via `presidio`/regex antes de enviar (plano futuro Fase 5, `agent.sensitive_data_filter` config). |
| **D** Denial of service | Provider rate-limit retorna 429 em cascade; provider down. | `AgentRuntime` retry com backoff; provider fallback (Ollama quando OpenAI down). | Circuit breaker per provider; budget de tokens/minuto por grupo. |
| **E** Elevation of privilege | Tool call response cria comando privileged em host; prompt injection via user input faz agent ignorar system prompt. | Tool whitelist + input sanitization em tool arguments; system prompt não-user-influenced. | Structured output enforcement (JSON schema) + adversarial prompt testing (plan futuro). |

**Guard de injeção indireta — mapa de cobertura (#1213, #1243)**: o conteúdo
de terceiro que entra no contexto do modelo passa por
`garraia_security::sanitize_indirect` (remove caracteres invisíveis e devolve
o relatório; suspeito chega precedido do `warning_banner` de dado
não-confiável) nestas superfícies:

| Superfície | Coberto desde | Por quê |
|---|---|---|
| `web_fetch` | #1213 | corpo HTTP é conteúdo de terceiro |
| resultado de tool MCP (`McpTool::execute`) | #1243 (fatia 1) | o servidor MCP é tipicamente um `npx` de terceiro; payload hostil entrava cru, e sem teto de tamanho era vetor de exaustão de contexto (cap 256 KiB com marca visível, alinhado ao `MAX_CONNECTOR_FRAME_BYTES`) |
| `file_read` | **pendência** — #1243 fatia 2 | conteúdo de arquivo lido a pedido do modelo vira instrução |
| `device_read`/`device_list` | **pendência** — #1243 fatia 3 | quem está no barramento escreve o nome/estado do dispositivo |

---

## 5.6. SSRF — toda requisição outbound com URL de origem remota

Adicionado em 2026-08-29, depois de a onda de alertas Critical do CodeQL
(`rust/request-forgery`, security-severity 9.1) mostrar que o padrão implementado
em `plugins_handler.rs` (GAR-460/461) existia em quatro outros lugares, em três
crates diferentes, cada um com uma defesa diferente ou nenhuma.

**Regra:** todo fetch outbound cuja URL venha de um request HTTP, de config
editável por request, ou de uma tool call de LLM, passa por
`garraia_common::ssrf` antes de abrir conexão. Nunca `reqwest::get(url)` cru.

O guard cobre cinco coisas: allowlist de esquema; resolução **única** do host com
bloqueio de faixas internas; DNS pinning via `resolve_to_addrs` (fecha o TOCTOU
de rebinding); `redirect::Policy::none()` (um host permitido não redireciona para
um bloqueado depois da checagem); e leitura de corpo com cap de bytes.

Auth e SSRF são independentes: `plugins_handler` exige
`Permission::ManagePlugins` **e** valida a URL, porque um operador legítimo
também não deve conseguir fazer o gateway varrer a rede interna.

| Call site | Política | Por quê |
|---|---|---|
| `plugins_handler::download_and_validate_manifest` | https + allowlist de host (vazia = desligado) + `PublicOnly` | manifest de plugin é código de terceiro |
| `garraia_skills::SkillInstaller::install_from_url` | https + `PublicOnly` | skill é conteúdo executável-adjacente; guard fica no crate para cobrir também o CLI |
| `tools::web_fetch_tool` | http+https + `PublicOnly` | URL vem de tool call do LLM — dirigível por prompt injection |
| `router::add_provider` (`base_url`) | http+https + `AllowPrivate` | Ollama local e LM Studio na LAN são o caso de uso; metadata continua bloqueada |
| `mcp::manager::connect_http` | http+https + `AllowPrivate` | MCP self-hosted em loopback/LAN é o caso ordinário; validado no sink terminal para cobrir create, restart, boot e reconnect |

`IpScope::AllowPrivate` é um afrouxamento deliberado e limitado: libera loopback
e RFC 1918/ULA, mas **continua** bloqueando link-local (`169.254.169.254`, o
metadata de instância em nuvem — o alvo de maior valor), CGNAT, multicast e o
endereço unspecified. Nenhum deles é endpoint legítimo.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **I** Information disclosure | `POST /api/skills/import` ou `POST /api/providers` apontado para `169.254.169.254` extrai credenciais de instância em nuvem. | `garraia_common::ssrf` recusa antes de conectar, em todos os call sites da tabela acima. | Estender aos itens ainda não cobertos: `health.rs::check_http`, `admin/providers.rs`, `admin/mcp_templates.rs`, `channels/{teams,matrix,google_chat}`, `voice/tts/chatterbox_client`, `agents/a2a/client`. |
| **E** Elevation of privilege | Prompt injection em conteúdo ingerido faz `web_fetch` varrer a rede interna e devolver o resultado ao modelo. | Bloqueio de IP + sem redirects + cap de corpo. | Registrar a deny-list de domínios (hoje `WebFetchTool::new(None)` = vazia) a partir do `AppConfig`. |
| **S** Spoofing | DNS rebinding troca o IP entre a checagem e o connect. | `resolve_to_addrs` pina os endereços já validados; `.send()` não re-resolve. | — |

## 5.7. Caminhos de filesystem vindos do request (`/api/projects`)

Fechado em 2026-08-29. `POST /api/projects` aceitava `path` como `String` crua
do corpo JSON e guardava sem validar; `GET /api/projects/{id}/files` percorria
esse diretório **recursivamente**. Todo o `/api/*` é auth-free por decisão de
design, e o gateway pode escutar em `0.0.0.0`, então a sequência

```text
POST /api/projects {"name":"x","path":"/etc"}
GET  /api/projects/{id}/files
```

enumerava `/etc` (ou `/root`, `/proc`, `/`) para qualquer um que alcançasse a
porta. Não era alerta do CodeQL — a regra `rust/path-injection` não modela este
caminho.

**Mitigação**: `garraia_gateway::project_root::confine`. O caminho é resolvido
com `std::fs::canonicalize` — que segue symlinks e achata `..` — e só então
comparado contra as raízes permitidas com `Path::starts_with`, que compara
componente a componente. Resolver **antes** de comparar é o ponto: a string
crua deixaria passar tanto `~/projs/../../etc` quanto um symlink
`~/projs/fuga → /etc`. Comparar por prefixo de string aceitaria
`/home/user-evil` como estando sob `/home/user`.

Raiz padrão: o home do usuário, e só ele. Override pelo operador via
`GARRAIA_PROJECT_ROOTS` (formato de `PATH`), que **substitui** o padrão. Sem
nenhuma raiz que resolva, tudo é recusado — fail-closed.

Aplicado nos três pontos que tocam o disco, não só na escrita: `create_project`,
`update_project` (senão o `PUT` reabriria o que o `POST` fechou) e
`list_project_files` — este último **re-confina na leitura**, porque é a linha
que de fato percorre o disco e o caminho pode ter virado symlink entre o POST e
o GET.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **I** Information disclosure | `POST /api/projects {"path":"/etc"}` + `GET .../files` enumera o filesystem sem autenticação. | `project_root::confine` em create/update/list; guards de regressão em `tests/projects_test.rs` verificados contra o código vulnerável. | Raiz configurável hoje só por env var; mover para `AppConfig` quando `projects_handler` passar a receber `State`. |
| **I** Information disclosure | Resposta distingue "não existe" de "fora das raízes", virando oráculo de existência de diretório. | 400 com corpo idêntico para todas as variantes de erro, como o 401 byte-idêntico de `/v1/auth/login`. | — |
| **T** Tampering | Symlink dentro da raiz apontando para fora, criado entre o registro e a leitura (TOCTOU). | `canonicalize` resolve o symlink, e a re-validação em `list_project_files` roda no momento da leitura. | Janela residual entre o `canonicalize` e o `read_dir` é inerente ao filesystem; reduzida, não eliminada. |

**Nota sobre o pressuposto "auth-free por desenho" (#1182)**: todo o `/api/*`
ser auth-free se apoia em "quem alcança a porta é o dono". O pressuposto tem um
buraco: **o navegador do dono alcança a porta rodando código de terceiros**.
Basta o dono visitar uma página qualquer para ela disparar
`POST /api/projects {"path":"…"}` contra `127.0.0.1:3888` de dentro do
navegador dele — com cookies, com a rede local, com tudo. Fechado para a
superfície HTTP mutante e para os handshakes de `/ws` e `/ws/parrot` pela
guarda da §5.10.

**Nota sobre `working_dir`**: desde o #1028, `POST /api/sessions`
(`api::create_session`) aceita `working_dir` e o passa por
`project_root::confine` **antes** de criar a sessão — fora das raízes é 400 com
um corpo só para todas as variantes (não existe, fora da raiz, symlink para
fora), como no `POST /api/projects`, sem sessão órfã, e a resposta ecoa só o
caminho canonicalizado. Antes o campo era descartado pelo serde (a rota viva só
conhecia `agent_id`); o `projects_handler::create_session_with_project`, que já
o confinava, segue **não roteado**. Guards:
`post_sessions_confines_working_dir` e
`post_sessions_accepts_working_dir_inside_the_allowed_root` em
`tests/projects_test.rs`.

## 5.72. Caminhos de filesystem vindos da tool call do LLM (#1244)

Fechado em 2026-09-16. A §5.7 fechou o caminho que vem do **request HTTP**. O
que ficou aberto foi o irmão dele: o caminho que vem da **tool call do modelo**.

`file_read`, `file_write` e `list_dir` recebiam o argumento `path` cru,
expandiam `~` e aceitavam caminho absoluto sem confinamento. O parâmetro
`allowed_directories` existia no construtor, tinha teste próprio, e os dois
pontos de registro em produção (`bootstrap/mod.rs` do gateway e `chat.rs` da
CLI) passavam `None`. Um prompt chegando por Telegram, Discord ou WhatsApp
mandava o modelo ler `~/.ssh/id_rsa`, `/etc/shadow` ou o próprio `config.yml`
do gateway — que carrega chave de LLM em claro quando o operador não usa o
cofre. `list_dir` era o reconhecimento: com ela o modelo achava o alvo antes de
pedir a leitura.

Combina mal com três coisas que existem: o guard de injeção indireta não cobre
`file_read` (o conteúdo lido vira instrução), o `sandbox` por tool tem default
`Off`, e a §5.9 já descreve a rota de chat como identidade não verificada.

**Mitigação**: `garraia_agents::FileJail`, o mesmo "resolve, depois confina" da
§5.7, com a diferença que a escrita exige. As raízes efetivas de uma chamada
são a união de `agent.file_roots` (config, vazia por padrão, mais a env
`GARRAIA_FILE_ROOTS`) com o `working_dir` da sessão. **Conjunto vazio nega
tudo** — sem raiz conhecida não há como afirmar que um caminho é seguro. No
gateway isso faz a raiz padrão ser o diretório da sessão e nada mais, e esse
`working_dir` já passou por `project_root::confine` (§5.7) antes de ser
gravado. Na CLI o CWD do processo entra como raiz, porque quem roda
`garra chat` é o dono da máquina no diretório que escolheu.

O alvo de uma escrita normalmente não existe, então `canonicalize` falharia: o
jail sobe até o **ancestral existente mais próximo**, canonicaliza esse e
recola a cauda. É o que barra `raiz/link-para-fora/novo.txt` — que uma checagem
só do `parent` textual deixaria passar, e que é o vetor de escrita equivalente
ao symlink de leitura.

**E aqui está a parte contraintuitiva, que a primeira versão desta mitigação
errou e a auditoria R4 pegou: `canonicalize` falhar não quer dizer "não
existe", quer dizer "não resolve".** Um symlink *pendurado* — cujo alvo não
existe — falha no `canonicalize` e existe para o `lstat`; e o `open(O_CREAT)`
de uma escrita **segue** esse link e cria o arquivo no alvo. Com a cauda
recolada dentro da raiz, o `starts_with` aprovava e o byte caía fora. O vetor
plausível não passa pelo `bash`: um repositório clonado traz
`raiz/evil -> ../../../home/u/.ssh/authorized_keys` versionado no git, o CWD é
raiz na CLI, e uma injeção indireta no README manda escrever em `evil` — que é
exatamente a tese da #1244. Por isso **todo componente que não canonicaliza
ainda passa por `symlink_metadata`**: existir para o `lstat` sem resolver é
recusa, não "cauda inexistente". O caminho é normalizado por `components()`
antes desse `lstat`, porque com barra final (`raiz/evil/`) o `lstat` segue o
link por POSIX e o pendurado voltaria a parecer inexistente.

Custo aceito: um pendurado apontando para **dentro** da raiz também é recusado.
Distinguir exigiria reimplementar resolução de symlink à mão — alvo relativo,
ciclo, teto de profundidade — e fail-closed sai mais barato que uma segunda
resolução caseira. O furo também tinha reaberto o oráculo de existência da
terceira linha da tabela abaixo: link vivo devolvia a frase de recusa, link
pendurado devolvia `Ok` — e escrevia.

O construtor das três tools passou a **exigir** o jail: `FileReadTool::new(None)`
não compila mais. Era o ponto exato da falha — um jail opcional é um jail
esquecido.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **I** Information disclosure | Prompt de canal faz o modelo chamar `file_read {"path": "~/.ssh/id_rsa"}` ou o `config.yml` do gateway. | `FileJail::confine` nas três tools, obrigatório no construtor; testes que pedem a tool ao runtime de `build_agent_runtime`, não ao construtor. | — |
| **I** Information disclosure | Symlink dentro da raiz apontando para fora (`raiz/atalho → /etc`). | `canonicalize` resolve o link **antes** da comparação, que é por componente (`Path::starts_with`). | — |
| **I** Information disclosure | Recusa distingue "não existe" de "existe mas está fora", virando oráculo. | Uma única frase para as três recusas, sem caminho e sem raiz. A mensagem útil da #923 fica só para arquivo ausente **dentro** da raiz. | — |
| **T** Tampering | `file_write` cria arquivo fora da raiz através de um diretório-symlink. | Subida até o ancestral existente + canonicalização dele. | — |
| **T** Tampering | `file_write` cria arquivo fora da raiz através de um symlink **pendurado** (`raiz/evil → /fora/inexistente`), versionado num repositório clonado. `canonicalize` falha por não resolver — não por não existir — e o `open(O_CREAT)` segue o link. | Cada componente que não canonicaliza passa por `symlink_metadata`: existe para o `lstat` e não resolve ⇒ recusa. Caminho normalizado por `components()` antes do `lstat`, senão a barra final (`raiz/evil/`) faz o `lstat` seguir o link. Três testes: função pura (folha e pai) e `FileWriteTool` ponta a ponta. | Pendurado para **dentro** da raiz também é recusado — fail-closed assumido. |
| **T** Tampering | Troca de symlink entre o `canonicalize` e o `open` (TOCTOU). | Reduzida: a tool abre o caminho **resolvido**, não o original. | **Residual conhecido, não fechado.** Fechar exige abrir por descritor (`openat2` + `RESOLVE_BENEATH` no Linux), sem equivalente portátil nos três sistemas operacionais. Exige quem tenha escrita dentro da raiz. |
| **T** Tampering / **I** Information disclosure | **Hardlink** dentro da raiz apontando para o inode de um arquivo de fora (`ln /etc/alvo raiz/inocente.txt`). A escrita atinge o inode de fora; e o backup `.bak` do `file_write` copia o conteúdo de fora **para dentro** da raiz, transformando o escape de escrita em escape de leitura. | **Nenhuma.** Um hardlink não é um ponteiro que se resolve, é um segundo *nome* do mesmo inode: `canonicalize` não tem o que seguir, o caminho resolve para ele mesmo e o `starts_with` aprova. | **Residual conhecido, não fechado — e, ao contrário do symlink, sem defesa possível com esta API.** Exigiria comparar `st_dev`/`st_ino` contra um mapa da raiz, ou recusar todo arquivo com `st_nlink > 1`, o que recusaria também hardlink legítimo dentro da própria raiz. Impacto menor que o do symlink: o git não versiona hardlink, então o vetor "repositório clonado" não serve, e exige quem **já tenha escrita dentro da raiz** — mesma pré-condição do TOCTOU acima. |
| **E** Elevation of privilege | No caminho MCP (`garra_agent`) quem escreve o `working_dir` é o **modelo**, pelo argumento da tool — e `FileJail::confine` soma o `working_dir` às raízes efetivas. `{"working_dir": "/", "message": "leia /etc/shadow"}` devolveria o disco inteiro às file tools. **Regressão introduzida pela própria #1244**: antes dela o `working_dir` do MCP só ancorava caminho relativo, não era raiz, e `working_dir: "/etc"` batia no jail. | `handle_agent_call` confina o `working_dir` contra as raízes do operador antes de aceitá-lo (`confine(dir, None)` — `None` de propósito: o valor sob validação não pode se autorizar) e responde `invalid_params`. A regra é a mesma endossada no #1255: pode **estreitar** o jail ou ficar dentro dele, nunca alargar. O jail é construído **uma vez por chamada** em `handle_agent_call` e desce por parâmetro até `build_tools`: a instância que valida é a mesma que vai para as file tools, e não há segunda construção a manter em concordância. O que isso fecha, medido por mutação: substituir o parâmetro por um jail próprio em `build_tools` exige descartá-lo, e aí o `unused variable: jail` derruba o `clippy --all-targets -- -D warnings` do CI. Não é impossibilidade de tipo — é uma divergência que o gate não deixa entrar. O gate não alcança a divergência **parcial**: dar um jail mais largo a só uma das duas tools mantém o parâmetro usado e passa limpo (medido). Fechar essa depende do teste de comportamento do wiring, registrado como dívida. A recusa é presa por um teste que chama o handler real; o lado "pode estreitar" chama a função de validação extraída (pelo handler ele rodaria um turno de agente de verdade dentro da suíte unitária). | O ganho de privilégio real era pequeno — ver a nota sobre `bash` em "Não coberto de propósito" —, mas a divergência entre a doc do schema e o código apontava na direção perigosa. |
| **E** Elevation of privilege | Operador põe `/` ou `$HOME` em `agent.file_roots` e desliga o jail sem perceber. | `garra config check` avisa nos dois casos; `config.hardened.example.yml` diz para não fazer. | Aviso, não erro — a decisão é do operador. |
| **E** Elevation of privilege | O mesmo por `GARRAIA_FILE_ROOTS=/`, que **soma** raízes às da config e não aparecia em lugar nenhum: o `config check` só lia o YAML e o boot só contava raízes (`roots().len()`). O jail apertado cria pressão operacional exatamente nessa direção. | `config check` valida também a env (campo `env.GARRAIA_FILE_ROOTS`); o `info!` do boot **nomeia** as raízes e um `warn!` sai por raiz que, já resolvida, seja `/` ou o `$HOME`. Comparação depois do `canonicalize`, senão `$HOME/../$USER` passa. | Continua aviso, não erro. |

**Não coberto de propósito** (cada um com o porquê):

- `repo_search` não recebe **caminho** do modelo: ele roda `rg`/`grep` com
  `current_dir` no `working_dir` da sessão e alvo fixo `.`, e o `file_pattern`
  vai por `--glob`, que não escapa da raiz da busca. Sem `working_dir` ele cai
  no CWD do processo — mesma superfície de antes.
  **Correção de um parágrafo errado desta mesma seção:** a versão anterior
  concluía daí que `repo_search` era "nem melhor nem pior", e esse raciocínio
  olhou só o `file_pattern`. O `query` também vai como argumento — literalmente
  `cmd.arg(query).arg(".")`, **sem nenhum `--` separando opção de operando** —
  e a auditoria R4 achou ali injeção de flag: um `query` começando com `-` é
  lido pelo `rg` como opção. É defeito próprio, aberto como **#1266** (P0) e
  **não** corrigido aqui: misturá-lo ao jail de caminho tornaria as duas
  correções mais difíceis de revisar.
- `git_diff` e `code_review` passam `file_path` como pathspec para o `git`, que
  só enxerga o repositório. Vale registrar um defeito vizinho encontrado aqui e
  **não corrigido** nesta mudança: `GitDiffTool::run_git_command` não seta
  `current_dir`, então ignora o `working_dir` da sessão e roda no CWD do
  processo do gateway. É bug de correção, não de confinamento.
- `bash` e `run_tests` são a fronteira da #1225 (sandbox por tool) e da §6, não
  desta. Um `bash` irrestrito lê qualquer arquivo — mas o ponto da #1244 é
  justamente que o modelo não precisava do `bash`.
  **Medido, não presumido** (auditoria R4 da #1244, dimensionamento do
  `working_dir`): com o `BashTool::new(None)` que o `build_tools` do MCP
  registra e `agent.bash_allowlist` vazia (o padrão), `cat /etc/shadow`,
  `head -c 32 /etc/passwd`, `ls /etc`, `echo pwned > /tmp/x` e
  `tee /tmp/y < /etc/hostname` **executam com `requires_confirmation=false` e
  `is_error=false`** — leitura *e* escrita fora de qualquer raiz, sem
  confirmação. Nenhum desses programas está na `DENY_LIST`, na `CONFIRM_LIST`
  nem em `SENSITIVE_PROGRAMS`, e a `bash_allowlist` do operador é uma lista
  *positiva* (dispensa confirmação, não restringe), então configurá-la não
  aperta nada. Consequência para quem for dimensionar um achado do jail no
  caminho MCP: enquanto o mesmo servidor entregar esse `bash`, o ganho de
  privilégio de furar o jail das file tools é ~nulo em capacidade. O jail
  continua valendo como defesa em profundidade, pelo dia em que o `bash`
  apertar — e porque no **gateway** (canal de chat, identidade não verificada
  da §5.9) é ele que segura, não o `bash`.
- `garraia-tools` tem uma segunda implementação de `RepoSearchTool`/`ListDirTool`
  com `root_path`, consumida só por `garraia-runtime::executor`, que o gateway
  não usa para tools (só `RuntimeSettings`). Fora do alcance do agente hoje;
  se entrar, entra com jail.

**Dívida registrada, não corrigida aqui** (auditoria R4 da #1244):

- Só o wiring do **gateway** tem teste de comportamento do jail.
  `chat.rs::register_cli_tools` não tem nenhum, e em `mcp_agent` o que os testes
  do `working_dir` cobrem é `file_jail()` — que o jail montado chegue às **duas**
  file tools de `build_tools` (`ListDirTool` é pulada de propósito no caminho
  MCP; "três" é a contagem do gateway) continua sem teste de comportamento.
  O que **está** fechado, e por construção e não por disciplina, é a divergência
  entre a régua que valida e a régua que executa: `build_tools` não constrói
  jail nenhum, recebe por parâmetro o mesmo que `handle_agent_call` usou para
  confinar o `working_dir`. Enquanto eram duas chamadas a `file_jail()`, trocar
  a de `build_tools` por `FileJail::from_roots(["/"])` entregava o disco inteiro
  às file tools com a suíte inteira verde (medido na revisão da rodada 4: 505 +
  11 + 1 testes passando, incluindo os três do `working_dir`). Depois do
  refactor, substituir o parâmetro inteiro por um jail próprio exige descartá-lo,
  e aí não compila sob o `-D warnings` do CI (`unused variable: jail`) — gate,
  não sistema de tipos. E o gate **para aí**: medido na passada curta de
  segurança, trocar o jail de **uma só** das duas tools (`FileReadTool` com
  `from_roots(["/"])`, `FileWriteTool` com o parâmetro) mantém o parâmetro
  usado, não gera aviso nenhum, e passa com clippy limpo e 506 testes verdes,
  com o `file_read` enxergando o disco inteiro. Ou seja: o CI pega a
  substituição total, não a divergência parcial tool a tool. O que fecharia
  essa é o teste de comportamento do wiring, que segue como dívida no item
  acima — não o `#[deny(unused)]` (pega a mesma forma que o CI já pega) nem um
  newtype `JailValidado`, que só provaria alguma coisa se `FileReadTool::new` e
  `FileWriteTool::new` passassem a exigi-lo, mudando a API de todos os pontos
  de registro.
  Vale registrar o que a mitigação anterior **não** cobria, porque o texto antigo
  deste bullet já convenceu um auditor do contrário: o construtor **exigir** o
  `FileJail` (`Default` = zero raízes = nega tudo) transforma "esqueci de passar"
  de fail-open em fail-closed, e só isso — não dizia nada sobre **passar um jail
  diferente e mais largo**, que era o modo de falha real aqui.
  Mas o defeito original da #1244 foi exatamente "ponto de chamada em produção
  que nenhum teste exercitava", e ele ainda vale para o `chat.rs`.
- Um symlink **quebrado apontando para dentro da raiz** recebe a mensagem de
  "fora das raízes". É seguro e está declarado em teste (fail-closed assumido,
  ver o Gap da linha do pendurado), mas confunde o usuário legítimo: um
  `node_modules` clonado pela metade produz uma recusa de segurança onde o
  problema é um link quebrado.
- As duas varreduras de fonte do boot (que provam que o wiring de produção
  passa o jail) afirmam só que *a linha existe em algum lugar do arquivo*:
  mover o laço para uma função privada que ninguém chama as mantém verdes. A
  alternativa é `build_agent_runtime` expor a contagem de raízes perigosas e o
  teste asserir o valor.

## 5.75. Saída de ferramenta escrita no terminal (#995)

O `garra chat` imprime, a cada chamada de ferramenta, uma linha com o que ela
fez e uma com o que devolveu (#937). Esse texto **não é confiável**: é o que o
agente leu de um arquivo, baixou de uma página, ou o que um comando escreveu.
Um `README` de repositório clonado é conteúdo de terceiro chegando ao terminal
do usuário.

Até o #995 ele chegava com os caracteres de controle intactos, o que dava a
quem controlasse a saída: limpar a tela (`\x1b[2J`), esconder o cursor
(`\x1b[?25l`, violando invariante explícita do projeto), trocar o título da
janela (OSC), e reposicionar o cursor para sobrescrever linhas já impressas —
**forjando texto que parece ter vindo do próprio Garra**, que é a consequência
que importa: o usuário decide o que autorizar lendo essa tela.

A sanitização mora no `garraia-agents`, junto da redação de segredo, mantendo
a fronteira do ADR 0017 — o renderer recebe texto pronto e só decide como
desenhar. Ordem obrigatória: **redige, saneia, trunca**; truncar antes deixaria
meia sequência (ou meio segredo) passar.

A garantia vem da regra **por caractere**, não do reconhecimento de sequência:
todo controle C0, C1 e DEL sai, e sem `ESC` o `[2J` restante é texto inerte.
O parser de CSI/OSC que existe é para legibilidade — saída colorida é comum e
legítima —, e se ele errar numa sequência exótica a falha é cosmética, não de
segurança.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | Conteúdo de terceiro reposiciona o cursor e sobrescreve linhas, forjando saída que parece do Garra — o usuário autoriza uma ação com base em tela falsificada. | Todo controle C0/C1/DEL removido antes de chegar ao renderer (#995). | — |
| **T** Tampering | `\x1b[2J` limpa a tela; OSC troca o título da janela; `\x1b[?25l` deixa o terminal sem cursor após a sessão. | Idem. | — |

**A superfície irmã foi fechada em seguida (#996):** o texto do **modelo**
(`write_delta`) também ia cru ao terminal, e o mesmo vale para aviso, erro e
dica, que carregam corpo de erro de provedor. A correção é separada porque
precisa de **estado**: o texto é streaming, e uma sequência pode chegar partida
entre dois deltas, cada metade inofensiva isolada — um filtro sem estado
deixaria as duas passarem, e o terminal vê a concatenação. O `AnsiFilter`
(`crates/garraia-cli/src/ui/ansi_filter.rs`) guarda a posição dentro da
sequência entre chamadas, e há teste varrendo **todos** os cortes possíveis de
uma carga hostil.

Cor do modelo fica bloqueada de propósito. A alternativa — permitir SGR e
bloquear o resto — passaria por cima do `NO_COLOR`: cor vinda do modelo não
sabe se o usuário a desligou, se a saída vai para um pipe, ou se o terminal é
legado. Quem decide cor é o `TerminalRenderer`, olhando `Capabilities`.

---

## 5.8. `/metrics` — o que a observabilidade conta sobre a instalação (#957)

O endpoint Prometheus é autenticado (`metrics_auth.rs`: loopback por padrão,
token ou allowlist para expor remotamente, fail-closed no boot). O que segue
não é uma falha desse controle — é o que um leitor **legítimo** do `/metrics`
passa a saber, e que não estava escrito em lugar nenhum.

As métricas de memória do #957 carregam a label `provider` com o id do
provedor de embeddings em uso (`ollama`, `openai`, `cohere`), e o desfecho
`no_provider` denuncia a **ausência** de configuração. Isso é topologia: quem
lê o `/metrics` sabe qual provedor atacar, e sabe se a memória semântica está
sequer ligada. Somando com `garraia_memory_ingested_total`, também se infere
volume de uso.

**Decisão: a label fica.** Mascarar o provider (como o `config check` faz com
secrets, reportando `configured: true` em vez do valor) destruiria a utilidade
da métrica — "qual provedor está falhando" é exatamente a pergunta que ela
existe para responder, e com um provedor só ela viraria uma constante. O
controle correto para topologia é o mesmo de sempre: **não exponha o
`/metrics` para quem não deveria ver a topologia.** Por isso o endpoint é
loopback por padrão e o listener dedicado se recusa a subir sem token ou
allowlist.

O que **não** está lá, e é o que importa: nenhum id de sessão, id de usuário,
conteúdo de memória ou nome de modelo. O nome do modelo ficou de fora
deliberadamente — vem da config do usuário, então além de cardinalidade
ilimitada seria mais um detalhe de infraestrutura publicado. As labels são
enums Rust que viram `&'static str`, o que faz disso uma garantia de
compilação e não de convenção; há teste afirmando que todo valor de `provider`
vem de conjunto fechado.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **I** Information disclosure | Leitor do `/metrics` (scraper legítimo, ou credencial de scraper vazada) descobre qual provedor de embeddings está configurado, se há algum, e o volume de turnos ingeridos. | Endpoint loopback por padrão; listener remoto exige token ou allowlist, fail-closed no boot (plan 0024). Nenhuma label carrega PII, conteúdo ou nome de modelo. | Aceito: mascarar `provider` inutilizaria a métrica. Revisar se o `/metrics` algum dia for exposto a um scraper multi-tenant. |

---

## 5.9. Identidade em `/v1/chat/completions` — auth-free nao e identidade livre (#1012)

Fechado em 2026-09-07. `POST /v1/chat/completions` e a rota de compatibilidade
OpenAI: e o que um plugin de editor, um `curl`, ou qualquer cliente que fale o
protocolo da OpenAI aponta para o gateway local. Ela **nao tem camada de auth**
(`router.rs:204`, sem `layer`; so `governor_layer` e CORS) — e isso e desenho,
nao esquecimento: e a mesma postura de todo o `/api/*` descrita na §5.7.

O defeito nao era a ausencia de auth. Era `resolve_user_id` derivar a
identidade **gravada** das duas coisas que o proprio chamador escreve:

1. `Authorization: Bearer <token>` — para qualquer token que nao fosse
   `garra-local`, **o proprio token virava o `user_id`**, com o comentario no
   fonte *"this allows custom API keys to identify users"*. Nunca identificou
   ninguem: a rota nao verifica o token contra nada, entao equivalia a deixar
   o chamador escolher o proprio nome.
2. Sem `Authorization`, o header `X-User-Id` era usado **cru**.

**Severidade: latente, nao ativa.** Rastreado ate o fim, o `user_id` nao abria
leitura de dado alheio: o unico consumidor e `hydrate_session_history`, que so
grava `session.user_id` e a coluna `user` do upsert; a carga de historico e
chaveada por `session_id`. O impacto era **atribuicao falsa** — a sessao e o
registro no banco ficavam sob uma identidade que ninguem provou.

Vale fechar porque e a mesma forma do buraco que a #1010 fechou de proposito
*antes* de ligar execucao nele. A arma engatilha sozinha no dia em que
qualquer leitura passar a filtrar por `user_id` em vez de `session_id`, ou em
que a identidade real do `garraia-auth` chegar a este caminho legado.

**Mitigacao**: a identidade e a do dono da instalacao local (allowlist), ou
`None`. O `X-User-Id` deixou de ser lido; um bearer que nao seja `garra-local`
e ignorado (com fingerprint no log, nunca o token — invariante herdado de
2026-08-29), e o **valor do dono tambem nao sai no log**: a primeira versao
desta correcao o logava em toda requisicao, o que era pior que o codigo
vulneravel, que so o logava quando um `garra-local` era apresentado. `None` e a resposta honesta para instalacao sem dono: preenche-la
com o header seria inventar um dono. Guards em
`openai_api.rs`: `x_user_id_forjado_nao_vira_identidade`,
`x_user_id_forjado_nao_preenche_instalacao_sem_dono`,
`bearer_arbitrario_nao_vira_identidade`, e
`garra_local_continua_resolvendo_o_dono` para a nao-regressao.

**Tambem**: `AppState::continuity_key` recebia um `_user_id` que **nunca
usava**. Dos quatorze chamadores, **sete** passavam um id por pessoa literal
(Telegram x2, Slack, WhatsApp, Discord, iMessage, e a `task.user_id` do A2A em
`server.rs:1173`), tres repassavam o parametro recebido e quatro ja passavam
`None` — e os quatorze recebiam a mesma `bus:shared-global` de volta. O
parametro foi removido em vez de passar a ser honrado: honra-lo mudaria o
significado de `memory.shared_continuity` para quem ja o ligou, e "shared" e o
que a opcao promete. O barramento e global **por desenho**, e agora a
assinatura diz isso.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | `curl -H 'X-User-Id: vitima' localhost:3000/v1/chat/completions` grava a sessão sob o nome da vítima. | Header não é lido; identidade vem da allowlist local. Guard de regressão contra o código vulnerável. | — |
| **S** Spoofing | Bearer arbitrário vira `user_id` ("custom API keys identify users"). | Bearer não-`garra-local` é ignorado; só o fingerprint vai ao log. | Se a rota algum dia precisar de auth real, verificar contra `garraia-auth` e devolver 401 — decisão de produto, quebra todo cliente local existente. |
| **I** Information disclosure | Bearer de terceiro escrito no log de toda requisição. | `token_fingerprint` (6 bytes de SHA-256); guard `api_token_never_reaches_the_log`. | — |
| **I** Information disclosure | O **dono** escrito no log de toda requisição. No WhatsApp o dono é o próprio número de telefone (`bootstrap/whatsapp.rs:88`, `claim_owner(&from_number)`); no iMessage, número ou Apple ID. | O valor nunca sai: o log diz só se houve dono ou não. Guard `o_valor_do_dono_nao_vai_para_o_log`. | — |
| **E** Elevation of privilege | Barramento de memória "por usuário" que na verdade é global, ligado por quem leu a assinatura. | Parâmetro removido: a assinatura não sugere mais escopo por pessoa. | Barramento por pessoa, se desejado, exige função nova e decisão explícita. |

**Emenda #1182**: o mesmo pressuposto vale aqui e tinha o mesmo buraco. A rota
não ter camada de auth não a torna alcançável só pelo dono: o navegador do dono
alcança a porta rodando código de terceiros, e `POST /v1/chat/completions` gasta
a chave de LLM **dele**. A identidade gravada deixou de ser escolhida pelo
chamador no #1012; quem podia *disparar* a chamada só passou a ser restrito na
§5.10.

**O que ficou fora, de propósito**: promover `Security Gate (BOLA & Tenant
Isolation)` a required check da `main` (hoje os obrigatórios são quatro —
`docs/security/protect-main-ruleset.md`). É mudança de branch protection, que
é do dono.

---

## 5.10. CSRF de navegador contra o gateway local (#1182)

Fechado em 2026-09-13. As §5.7 e §5.9 registram a postura "`/api/*` é auth-free
por desenho: quem alcança a porta é o dono". O #1093 já tinha mostrado o buraco
do pressuposto em `/api/learning/*`, e fechado **só ali**. O #1182 é o mesmo
buraco no resto da superfície.

**O vetor**: o dono visita uma página qualquer. Ela roda
`fetch("http://127.0.0.1:3888/api/settings", {method:"PATCH", body:…})` — e o
navegador do dono, que está no loopback, entrega. Nenhum token é necessário
porque a instalação default não tem `gateway.api_key`. O que dava para fazer:

| Rota | Efeito |
|---|---|
| `PATCH /api/settings` | reescrever a config do gateway |
| `POST /api/mode/select` / `POST /api/modes/custom` | trocar o modo do agente (e com ele o `ToolGate`) |
| `POST /api/mcp/marketplace/install` | instalar servidor MCP |
| `POST /api/skills` / `PUT /api/skills/{n}` | escrever skill que o agente executa |
| `POST /v1/chat/completions`, `POST /v1/messages`, `POST /chat` | gastar a chave de LLM do dono |
| `DELETE /api/memory`, `DELETE /api/sessions/{id}` | destruir dados |
| `POST /api/projects` | (§5.7) registrar raiz de projeto |

E a **resposta voltava legível**: o CORS default era `allow_origin(Any)` +
`allow_methods(Any)` + `allow_headers(Any)`, então a página do atacante não só
disparava a escrita como lia o retorno — o `GET /api/settings/effective` inteiro,
por exemplo. Em `/ws` era pior de outro jeito: WebSocket não passa por CORS
nenhum, então `new WebSocket("ws://127.0.0.1:3888/ws")` de qualquer página subia
uma sessão de chat completa.

**Mitigação**, em três peças:

1. `garraia_gateway::origin_guard::cross_origin_guard` — middleware sobre
   `POST`/`PUT`/`PATCH`/`DELETE` de toda a superfície (e sobre todo handshake
   de WebSocket, ver 3), montado **por dentro** do gate de `gateway.api_key`
   (o 401 do gate vem primeiro). Recusa com `403` de corpo constante quando o
   `Origin` não é o do próprio gateway (mesmo esquema, mesma authority do
   `Host`, gramática RFC 6454 estrita, `Origin: null` e `Sec-Fetch-Site:
   cross-site` inclusos) ou quando, havendo `Origin`, o `Host` é um nome DNS
   que não é `localhost`/`*.localhost` nem está em `gateway.allowed_origins`
   — a âncora anti-DNS-rebinding. O `Host` vem do header ou, em HTTP/2, da
   `:authority` (senão o console servido com TLS nativo, onde o navegador
   negocia h2 e não manda `Host`, seria recusado). O esquema do transporte é
   o que o `server.rs` de fato serve (`esquema_efetivo`: feature `tls` **e**
   cert **e** chave) — `tls_cert_path` preenchido num binário sem a feature
   cai para `http` nos dois lugares. Skip-list só para quem tem guarda
   própria mais estrita (`/api/learning/`, `/api/plugins/`) ou é
   server-to-server assinado (`/webhooks/`); `/admin/` **não** está nela,
   porque `POST /admin/api/setup` (cria o primeiro admin), `/admin/api/login`
   e `/admin/api/recovery/*` são montados fora do `require_csrf` do
   sub-router admin. Nada do pedido é ecoado no corpo nem no log.
2. **CORS default deixou de ser allow-all.** Sem `gateway.allowed_origins`,
   nenhuma origem cross-origin é anunciada. O Web Console é servido pelo
   próprio gateway e é same-origin — não usa CORS; cliente não-navegador
   (app mobile, `curl`, Claude Code) ignora CORS. Uma entrada `*` é ignorada
   com aviso (o `AllowOrigin::list` do tower-http entraria em pânico no boot).
3. **`/ws` e `/ws/parrot` checam o `Origin` do handshake**
   (`ws_upgrade_permitido`) — no middleware, para qualquer rota com
   `Upgrade: websocket`, e de novo em cada handler como defesa em
   profundidade. Sem `Origin` (app, CLI) o handshake segue como antes. Uma
   página web só consegue apresentar `Origin` `http`/`https` (ou `null`),
   então origem de esquema de app (`tauri://localhost`, extensão de
   navegador) passa; e a webview do Garra Desktop passa pela sua origem exata
   (`origin_guard::ORIGENS_TAURI`: `tauri://localhost` em Linux/macOS;
   `ORIGENS_TAURI_WEBVIEW2`: `http://tauri.localhost`/`https://tauri.localhost`,
   aceitas **só quando o gateway roda em Windows** — o `ws.js` conecta em
   `localhost`, então webview e gateway estão no mesmo SO, e num gateway
   Linux/macOS um `Origin` `http://tauri.localhost` só pode ser navegador:
   o Safari entrega `*.localhost` ao resolvedor do sistema, que num Wi-Fi
   hostil é do atacante). Pelo mesmo motivo a âncora anti-rebinding aceita
   `localhost` **exato**, nunca `*.localhost`. Essa lista foi derivada do
   fonte do Tauri 2.11 (`tauri_protocol_url` e o parse do header `Origin` no
   protocolo de IPC, cujos testes usam `tauri://localhost`), **não medida em
   runtime nesta entrega** — o ambiente não tem GTK/webkit nem `DISPLAY`. Se
   o pássaro parar de conectar após o upgrade, a guarda do router responde
   antes do handler: o log diz `gateway: cross-origin request refused` com
   `path=/ws/parrot method=GET`, e a lista é o lugar a olhar.

**Recorte deliberado**: o guarda genérico **não** herdou o
`503 auth not configured` do `learning_mutations_guard` para peer não-loopback
sem credencial. Herdá-lo mataria o app mobile na LAN contra um Garra sem
`api_key`, que é cenário suportado. Contra quem já executa código na máquina, o
gate de verdade continua sendo `gateway.api_key` — este módulo fecha o que o
**navegador** pode ser forçado a fazer.

**Quebra conhecida**: alcançar o console por **nome DNS** sem o nome em
`allowed_origins` — reverse proxy com domínio próprio, mas também mDNS
(`nas.local`), Tailscale MagicDNS, nome de serviço Docker/Compose e ingress. A
âncora só conhece IP literal, `localhost` e nomes declarados; no reverse proxy
há ainda o esquema (`Origin: https://…` contra um gateway que fala `http`).
Efeito: 403 em `POST`/`PATCH`/`DELETE` e no handshake do chat (`/ws`).
Mitigação: listar a origem em `gateway.allowed_origins`, que é aceita como
declarada pelo dono (escotilha `origem_declarada`, a mesma confiança que a
lista já carregava para o CORS). Documentado em `docs/hardening-gateway.md`,
com a linha adicionada às receitas de proxy do `production-runbook.md`.

### Residual: leitura `GET` sob DNS rebinding

Depois de um rebinding bem-sucedido a página do atacante é **same-origin** com
o gateway, e o navegador **não manda `Origin` em `GET` same-origin** — então
nenhuma regra baseada em `Origin` distingue `GET /api/sessions`,
`/api/memory/recent` ou `/api/logs` vindos dessa página de um `GET` do console
legítimo. Fechar isso exigiria recusar todo `Host` que é nome DNS não
declarado, **inclusive sem `Origin`**, o que mataria `curl http://nas.local:3888/health`,
o app mobile por hostname e os healthchecks do Compose. Fica registrado como
residual: escrita e WebSocket estão fechados; leitura sob rebinding depende de
`gateway.api_key` (o gate de verdade, que o rebinding não tem como fornecer) ou
de servir o console só por IP/`localhost`.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **T** Tampering | Página visitada pelo dono dispara `PATCH /api/settings` contra `127.0.0.1:3888` e reescreve a config. | `origin_guard::cross_origin_guard` em toda a superfície mutante fora da skip-list; tabela de rotas reais em `origin_guard.rs` e no `build_router` de verdade em `tests/origin_guard_layering.rs`. | — |
| **E** Elevation of privilege | Página visitada pelo dono dispara `POST /admin/api/setup` numa instalação nova e cria o primeiro admin com credenciais do atacante (rota pública, sem `require_csrf`). | `/admin/` fora da skip-list: a guarda cobre o bootstrap do admin. | — |
| **I** Information disclosure | CORS `allow_origin(Any)` deixava a página do atacante **ler** a resposta (`/api/settings/effective`, `/api/sessions`). | Sem `allowed_origins`, nenhuma origem cross-origin é anunciada. | — |
| **E** Elevation of privilege | DNS rebinding: domínio do atacante re-resolvido para `127.0.0.1` faz `Origin` e `Host` casarem entre si. | Âncora `ancora_ok`: só IP literal, `localhost`/`*.localhost` ou nome declarado em `allowed_origins`. | — |
| **I** Information disclosure | DNS rebinding + `GET` same-origin (sem `Origin`) lê `/api/sessions`, `/api/logs`. | **Residual** — ver acima. | `gateway.api_key`; console só por IP/`localhost`. |
| **T** Tampering | `new WebSocket("ws://127.0.0.1:3888/ws")` de qualquer página abre sessão de chat (WebSocket não passa por CORS). | `ws_upgrade_permitido` no middleware e no `ws_handler`. | — |
| **T** Tampering | O mesmo contra `/ws/parrot` (overlay do desktop): turno completo do agente, com tools, escrevendo na sessão persistente `parrot-desktop`, resposta legível — e sem gate de `api_key`, que só cobre `/api/*`. | `ws_upgrade_permitido` no middleware e no `parrot_ws_handler`, com `ORIGENS_TAURI` para a webview. | Medir o `Origin` real da webview por plataforma (Linux WebKitGTK, Windows WebView2) na próxima release do desktop e confirmar a lista. |
| **D** Denial of service | Console alcançado por nome DNS não declarado deixa de aceitar mutação e chat de navegador. | Quebra conhecida e deliberada; escotilha por `gateway.allowed_origins`. | — |
| **D** Denial of service | `allowed_origins: ["*"]` (o reflexo de quem quer o allow-all de volta) derrubaria o gateway no boot. | Entrada ignorada com aviso (`origens_validas`). | Report em `garraia config check` (opt-in — nada no boot chama o `run_check`, issue #1247). |

---

## 5.11. Pareamento de canais — `/pair` e `PairingManager` (#1189, #1191)

O `/pair` (`Role::Owner`) gera um código de 6 dígitos (~20 bits) que vale
5 minutos; um usuário não autorizado que o envie ao bot entra na allowlist
**em disco** (`Allowlist::add`). Até o #1190 cada canal tinha um
`PairingManager` próprio e o `claim()` nunca casava com o código do `/pair`
(que mora em `state.pairing`) — o pareamento não funcionava, e por isso o
caminho de `claim()` nunca tinha sido exercido em produção. O #1190 uniu as
instâncias; o #1191 endureceu o `claim()` que passou a ser alcançável.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | Conta não autorizada chuta códigos de 6 dígitos em mensagens comuns dentro da janela de 5 min; a única barreira era o rate limit do transporte (Telegram/Discord), que não é nosso. | `ClaimLimits`: 5 erros do mesmo `user_id` → 15 min de `LockedOut` (sem comparar, e sem contar no global); 20 erros comparados de qualquer usuário desde o último `/pair` → todo código pendente é queimado (`Burned`). 20 palpites num espaço de 10^6 = 2e-5 por ciclo de `/pair`. O usuário vê o mesmo "unauthorized" nos três casos — a resposta não revela se o código existia. Um `/pair` em outro canal **não** zera o contador enquanto um código pendente sobrevive. | **DoS do pareamento**: 4 identidades × 5 erros queimam o código. Onde identidade custa (Telegram, WhatsApp, Signal) o `/pair` seguinte é seguro enquanto os ofensores estão em `lockout`; onde é grátis (IRC sem NickServ, alts de Discord) um atacante persistente queima cada código novo. O dono **fica sabendo**: o `/pair` seguinte diz que o anterior foi queimado e com quantos erros (`GenerateStatus::previous_burned`). **Decidido (dono, 2026-09-14):** o código fica em 6 dígitos — com 20 palpites comparados por ciclo a chance é 2e-5, e o código é ditado por voz/telefone; os limites (5 por usuário / 15 min / 20 globais) ficam fixos, sem knob de config, até um deployment real pedir. |
| **I** Information disclosure | Comparação `==` sai no primeiro byte diferente (side channel de tempo). | `subtle::ConstantTimeEq`, percorrendo todos os códigos pendentes sem sair no primeiro que casa. Sinal fraco pela rede + transporte de chat, mas consistente com `garraia-auth`. | — |
| **E** Elevation of privilege | `claim()` ignora o `channel_id`: código gerado num canal é resgatável em qualquer canal habilitado. | Coerente com o desenho atual — a allowlist é global por instalação e o `/pair` gera com a chave literal `"telegram"` em todo canal. | **Decidido (dono, 2026-09-14): pareamento global por instalação.** Um dono, uma allowlist, um código válido em qualquer canal habilitado — o `channel_id` do `PairingManager` é informativo, não um escopo. Escopar por canal só voltaria à mesa com allowlist por canal, que é outro produto. |
| **R** Repudiation / UX | Um segundo `/pair` sobrescrevia em silêncio o código pendente que o dono acabou de mandar para alguém; uma queima era invisível para dono e convidado (o canal descarta em silêncio). | `generate_with_status` devolve `replaced_pending` e `previous_burned`; o `/pair` avisa os dois casos, no `commands.rs` e no `/pair` local do Discord. | — |

O log de `Burned` leva só contagens — nunca `user_id` nem código. Esta seção
descreve o mundo **depois do #1190** (um `PairingManager` por processo); sem
ele, cada canal tem o seu e o `claim()` do Telegram nunca casa com o `/pair`.

---

## 5.12. Ambiente dos filhos MCP (#1075 — continuação)

Um servidor MCP de transporte stdio é um processo filho do gateway, quase
sempre um pacote de terceiro resolvido na hora (`npx -y algum-server`).
Até esta correção o `McpManager::connect` montava o `Command` sem
`env_clear()`: o filho herdava o ambiente **inteiro** do gateway —
`GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, `ANTHROPIC_API_KEY` /
`OPENROUTER_API_KEY`, `GarraIA_VAULT_PASSPHRASE`, `DATABASE_URL` e tudo que
o `dotenvy` tivesse carregado do `.env`. O mapa `env` da config era aplicado
**por cima** dessa herança, não no lugar dela. O #1075 já tinha fechado esse
mesmo buraco para as tools de shell (`bash_tool`, `run_tests`, `git_diff`,
`code_review`, `repo_search`) via `R3_ENV_ALLOWLIST`; o caminho MCP ficou
de fora.

O ambiente agora é construído do zero, nesta ordem: allowlist
(`garraia_common::safety_gate::is_mcp_child_env_allowed` — só caminhos,
locale, temp e identidade) e, por cima, o mapa `env` daquele servidor. A
allowlist MCP é um superset da R3 porque um servidor MCP é um programa real
(`npx`/`uvx`/`python`) e não um comando de shell efêmero; as duas listas
ficam separadas para que afrouxar uma não afrouxe a outra.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **I** Information disclosure | Um servidor MCP de terceiro (`npx`) lê `std::env` no `main()` e exfiltra o JWT secret e as chaves de provider do dono na primeira execução. Não precisa de tool call, prompt injection nem rede do agente — basta ser spawnado. | `cmd.env_clear()` + allowlist + mapa `env` explícito do servidor. Nenhum nome com `key`/`token`/`secret`/`password`/`passphrase` pode entrar na allowlist (teste de unidade é o gate). | Residual: o filho roda com o **mesmo UID** do gateway e pode ler `/proc/<pid>/environ` do pai. Sandbox real do processo MCP segue no #1225; os limites atuais (`setrlimit`, PDEATHSIG) são limites de recurso, não confinamento. |
| **E** Elevation of privilege | Servidor MCP usa uma credencial do gateway (ex.: `DATABASE_URL` do Postgres de workspace) para agir fora do escopo que o operador lhe deu. | A credencial não chega mais ao filho por herança. O que ele recebe é o que o operador declarou em `env` — auditável por servidor. Guardar esse valor no cofre (`vault:mcp.<server>.<KEY>`) só funciona pelo caminho `mcp.json` + admin API; no `config.yml` o valor é literal. | **#1237**: resolver `vault:` também no boot do `config.yml` — hoje a referência chega ao filho como string. |
| **D** Denial of service | Um servidor legado que dependia de variável herdada (`HTTP_PROXY`/`HTTPS_PROXY` corporativo, `NODE_OPTIONS`, `npm_config_*`) para de subir após a atualização. Bundles de CA (`SSL_CERT_FILE`, `SSL_CERT_DIR`, `NODE_EXTRA_CA_CERTS`) estão na allowlist porque são caminhos; as variáveis de proxy não, porque a URL pode embutir credencial. | Válvula de escape por servidor `inherit_env: true`, que restaura a herança completa e emite `warn!` nomeando o servidor a cada conexão. Padrão `false`. | O caminho certo é migrar a variável para o mapa `env` do servidor; `inherit_env` é destravamento temporário, não configuração de regime. |

O `warn!` de `inherit_env` leva apenas o **nome** do servidor — nunca nome
nem valor de variável — e é emitido no primeiro connect daquele servidor; os
reconnects automáticos registram em `debug!`, para que um servidor em loop de
restart não afogue o log justamente quando ele está sendo lido.

A válvula existe só no `config.yml`/`mcp.json`. Servidores criados **ou
reiniciados** pela admin API conectam sempre com o ambiente isolado, mesmo
quando o arquivo declara `inherit_env: true` para aquele nome. Desde #1273
o `McpServerConfig` do registro do gateway carrega o campo (para round-trip
do arquivo), mas o handler de restart mantém o `false` explícito: a
divergência deixou de ser falta de informação e é política do reconnect
pela admin API — o boot honra a declaração, o restart isola mais (nunca
menos).
## 5.13. Sandbox por tool (`agent.sandbox`) — #1222, #1225

O `BashTool` pode envolver o comando num backend em vez de executá-lo direto
no host. A política mora em `garraia_agents::sandbox::SandboxPolicy`, a
configuração do operador é a seção `agent.sandbox` (#1225) e a tradução entre
as duas é `garraia_gateway::bootstrap::sandbox_policy_from` — a mesma função
nos três pontos de produção (gateway, `garra chat`, `garra mcp-agent`).

Até a #1225 a seção não existia: os três construtores fixavam
`SandboxPolicy::default()` (= `off`) e `set_sandbox_policy` só era chamado
pelos próprios testes. A contenção estava escrita, testada e **inalcançável**
— que é o motivo de esta seção existir antes da matriz.

### O que cada backend garante

| Backend | Rede | Sistema de arquivos | Privilégios | Onde o comando roda | O que **não** cobre |
|---|---|---|---|---|---|
| `docker` | `--network none` quando `network_disabled` (default `true`) | Só o `cwd` montado rw quando `mount_workdir` (default `true`) e o diretório existe; o resto é a imagem | `--security-opt no-new-privileges`; **sem** `--user`, `--read-only`, `--cap-drop`, limite de pids/memória | Container efêmero (`--rm`) no host local | Não é hardening completo do container (flags acima ficam para um slice próprio); o daemon do Docker é root, então escape do container é escape para root; o `cwd` montado é rw e é código do projeto |
| `podman` | igual ao `docker` | igual ao `docker` | igual ao `docker`, mais o rootless do próprio podman quando instalado assim | Container efêmero no host local | Idem, menos a parte do daemon root quando rootless |
| `ssh` | **nenhuma** — `network_disabled` é **ignorado** | **nenhuma** — `mount_workdir` e `image` são **ignorados** | os do usuário SSH no host remoto | Máquina remota, shell do usuário SSH | **Não é sandbox.** É execução remota: isola o host *local* e nada mais. O comando roda com tudo que aquele usuário pode fazer, inclusive rede |

Três limites valem para os três backends:

- **Só a tool `bash` é envolvida hoje.** `run_tests`, `git_diff`, `code_review`
  e `repo_search` continuam nascendo no host mesmo com `mode = all` — a
  policy é consultada dentro do `BashTool` e em nenhum outro lugar.
  Acompanhamento na #1225 (slices S2/S3) — a issue segue aberta. Quem liga `mode = all` esperando "nada roda no
  host" está enganado sobre quatro tools.
- **Unix, e agora dito em voz alta.** No Windows o `BashTool` escolhe
  `powershell -Command` e receberia uma linha com quoting POSIX
  (`docker run ... sh -lc '…'`), que o PowerShell não reparseia da mesma
  forma — o quoting de aspa simples lá é `''`, não `'\''`. Desde a #1225
  isso não é mais só documentação: `wrap_command` **recusa fail-closed**
  fora de unix e o `config check` reporta Error, em vez de deixar a
  contenção parecer ligada.
- **`elevated` roda no host.** É o escape hatch: a tool listada pula o
  backend mesmo em `mode = all`. Ele é duplamente gated só quando
  `agent.tool_confirmation_enabled = true`; sem isso resta apenas a denylist
  do `safety_gate`, e o `garra config check` avisa.

### Matriz

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **T** Tampering | Tool call do LLM (influenciável por injeção indireta de prompt, #1213) escreve fora do projeto. | Denylist + tier arriscado do `safety_gate` rodam **antes** do sandbox; com `docker`/`podman` o comando só enxerga o `cwd` montado. | `--read-only` no rootfs e mount do `cwd` em `ro` quando a tool for de leitura: slice próprio da #1225. |
| **I** Information disclosure | Comando lê `~/.ssh`, `.env` do host, ou exfiltra por rede. | `--network none` por default; `#1075 R3` já limpa o env do filho para uma allowlist; fora do mount o container não vê o host. | Com `backend = ssh` **nada disso vale** — a seção acima diz por quê. |
| **E** Elevation of privilege | Escape do container; `sudo` dentro do comando. | `--security-opt no-new-privileges`. | Sem `--user` o processo é root **dentro** do container, e o daemon do Docker é root **fora**; podman rootless é a recomendação enquanto o hardening não chega. |
| **E** Elevation of privilege | Operador liga `mode = all` e acredita que o agente perdeu o host. | Quatro tools seguem no host (acima); `config check` e esta seção dizem quais. | Estender a policy às demais tools — tracking na #1225 (slices S2/S3). |
| **D** Denial of service | Comando consome CPU/memória da máquina inteira dentro do container. | Timeout do próprio `BashTool` + orçamento de tool calls. | Sem `--memory`/`--pids-limit`; mesmo slice de hardening — tracking na #1225 (slices S2/S3). |
| **R** Repudiation | Não se sabe depois se um comando rodou contido ou no host. | `tracing::info!` "comando executado dentro do sandbox" no caminho envolvido e `tracing::error!` no fail-closed. | Evento de audit dedicado (`agent.tool.sandboxed`) quando o audit de tools existir. |
| **S** Spoofing | Backend ausente no host faz o comando cair no host em silêncio. | **Fail-closed**: `wrap_command` devolve erro e o `BashTool` recusa o comando; `backend = ssh` sem `ssh_host` também não constrói backend nenhum. | — |
| **E** Elevation of privilege | **Injeção de opção** por `ssh_host` / `image`: `sh_quote` garante um token, não um *operando*. O host fica antes do `--` em `ssh {host} -- sh -lc …`, então `ssh_host: "-oProxyCommand=…"` é lido como flag e executa no host **local**, já depois do `safety_gate`; `image: "-…"` desloca o posicional do `docker run`. | Valor começando com `-` é recusado em **três** camadas. Duas rodam sempre e são as que garantem a propriedade: `sandbox_policy_from` no boot (backend não é construído / imagem cai no default, com `warn!` que nunca loga o valor) e o próprio `wrap_command` (Err fail-closed, antes do `is_available()`). A terceira é o `garra config check`, que **reporta** Error — comando opt-in, **não** gate de boot: nada no boot do gateway invoca o `run_check`. Nenhum host e nenhuma imagem reais começam com `-`. | Conserto estrutural: montar **argv** em vez de uma linha de shell, eliminando a classe inteira — tracking na #1225 (slices S2/S3), como já recomendado na #1231. |
| **T** Tampering | Sandbox ligado numa plataforma onde o wrap não tem significado. | `wrap_command` devolve `Err` fail-closed fora de unix, e o `config check` reporta Error em `cfg!(windows)` — em vez de entregar uma linha POSIX ao `powershell -Command`. | — |

### Config mínima

```yaml
agent:
  tool_confirmation_enabled: true   # `elevated` sem isto é single-gated
  sandbox:
    mode: all                       # off (default) | all | allowlist
    backend: podman                 # docker | podman | ssh
    image: debian:bookworm-slim
    network_disabled: true
    mount_workdir: true
    elevated: []                    # tools que rodam NO HOST
```

O `garra config check` é um relatório que o operador roda (`config_cmd.rs`) ou
que o `garra doctor` invoca — **não** é um gate de boot, e um gateway com a
seção inválida sobe. O que ele faz é dar nome ao problema antes de alguém
esbarrar nele em produção; quem impede o comando de rodar são as camadas 2 e 3
descritas acima.

Ele reporta Error para `mode != off` sem `backend`, `backend: ssh` sem
`ssh_host`, `ssh_host` ou `image` começando com `-`, e para a seção ligada fora
de unix. Avisa (Warning) que `ssh` é execução remota — **sempre**, mesmo com a
seção coerente —, que `ssh` ignora `network_disabled`/`mount_workdir`, que
`elevated` sem confirmação humana é escape hatch desacompanhado, que
`mode: all` com `bash` em `elevated` deixa a seção inerte, que `allowlist` com
lista vazia sandboxa nada, e nomeia cada entrada de
`sandboxed_tools`/`elevated` que não é uma tool que o sandbox saiba envolver.
Nenhum finding ecoa o `ssh_host`; nomes de tool são ecoados de propósito — é o
ponto do finding.

---

## 6. Mobile apps (`apps/garraia-mobile`)

**Divergência JWT TTL (conhecida)**: o path mobile legacy (`crates/garraia-gateway/src/mobile_auth.rs`, wired via GAR-335) emite JWT com TTL de **30 dias** (`JWT_EXPIRY_SECS = 30 * 24 * 3600`), distinto do access token de 15 min do `garraia-auth` workspace (plans 0011/0012). Coexistência é temporária — consolidação depende de GAR-413 (migrate workspace) + migração dos clientes mobile para `/v1/auth/*`. Enquanto coexistem, a janela de hijack de session mobile é 48× maior que a do fluxo workspace. Risco documentado, mitigação parcial via `flutter_secure_storage` (Keystore/Keychain) + refresh token rotation planejada.

| STRIDE | Cenário concreto | Mitigação atual | Gap / Planejada |
|---|---|---|---|
| **S** Spoofing | Device compromised extrai JWT. | `flutter_secure_storage` usa Android Keystore / iOS Keychain. | Certificate pinning pending (plano futuro). Consolidar mobile para `garraia-auth` 15-min TTL quando GAR-413 destravar. |
| **T** Tampering | MITM altera responses em Wi-Fi público. | HTTPS only (plan 0027 base URL). | GAR-383 TLS 1.3 cap alinhamento quando backend estiver cloud. |
| **R** Repudiation | Usuário nega mensagem enviada do device. | Backend audit suficiente para chat. | Client-side audit export (out of scope). |
| **I** Information disclosure | App log contém PII; screenshot de conversa em task switcher. | `debugPrint` off em release; Android `FLAG_SECURE` pending. | Adicionar `FLAG_SECURE` via `flutter_windowmanager` (small plan futuro). |
| **D** Denial of service | Backend timeout deixa UI travada. | Dio default timeout 30s + retry configurável (plan 0028 ajuste `rest_v1_me` 80×250ms). | User-visible error states com retry manual. |
| **E** Elevation of privilege | Refresh token extraído de device → acesso sem senha. | Secure storage + rotation em refresh (plan 0012). | Device binding + biometric gate em refresh (plano futuro). |

---

## Threat inventory (prioritized)

Agregado das matrizes. Prioridade = (likelihood × impact) dado o estado atual do repo.

| # | Ameaça | Componente | Prioridade | Tracking |
|---|---|---|---|---|
| 1 | TLS 1.3 ainda não enforced em produção | Gateway | **Alta** | GAR-383 |
| 2 | CredentialVault não é single source de secrets | Gateway + Auth | **Alta** | GAR-410 |
| 3 | `api.rs` + `admin/middleware.rs` remanescentes sem XFF fail-closed consolidation | Gateway | Média | plan 0024+ follow-ups |
| 4 | WhatsApp/iMessage webhook signature verification sem auditoria | Channels | Média | plan futuro |
| 5 | Mobile sem certificate pinning | Mobile | Média | plan futuro |
| 6 | Plugin WASM runtime ainda scaffold | Plugins | Baixa (não shipped) | Fase 2.2 |
| 7 | Storage HMAC integrity + allow-list MIME pendente impl | Storage (future) | Baixa (ADR apenas) | GAR-394 |
| 8 | Mobile Android `FLAG_SECURE` ausente | Mobile | Baixa | plan futuro |
| 9 | Sandbox por tool cobre so `bash`; sem hardening de container (`--user`, `--read-only`, `--cap-drop`, limites) | Agents | Média | #1225 (slices S2/S3) |

---

## Controles compensatórios já shipped

- **RLS FORCE** em 10 tabelas críticas (migration 007).
- **3 Postgres roles segregados** (`garraia_login`, `garraia_signup`, `garraia_app`) com newtype `LoginPool`/`SignupPool`/`AppPool` — compile-time non-interchangeable.
- **Argon2id + lazy upgrade PBKDF2**.
- **Refresh token opaco** com HMAC separado.
- **Audit events** em invite accept + member setRole + member remove.
- **Rate limit per-user** (plan 0022 F-03).
- **Metrics endpoint auth** (Bearer + IP ACL + startup fail-closed, plan 0024).
- **Telemetry hardening**: REDACT_HEADERS + idempotent init + cardinality guard debug assert (plan 0025/0026).

---

## Revisão e próxima iteração

**v1.1 (próxima revisão):** após GAR-383 (TLS) e GAR-410 (CredentialVault) fecharem, regenerar matrizes #1 e #2. Disparar quando qualquer um dos dois merge landar.

**v2 (revisão majorada):** após `garraia-storage` shippar (GAR-394) — reescrever §3 Storage com mitigação efetiva (não prevista).

**Cadence baseline:** trimestral + após cada ADR que mude surface (0001 quando mistral.rs wirar, 0007 Desktop frontend, 0008 Doc collab).

## Links e referências

- OWASP Application Security Verification Standard 5.0 (ASVS): <https://owasp.org/www-project-application-security-verification-standard/>
- NIST SP 800-53 Rev 5: <https://csrc.nist.gov/publications/detail/sp/800-53/rev-5/final>
- STRIDE original: Kohnfelder & Garg (2002), <https://learn.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats>
- ADR 0003 Postgres multi-tenant: [`../adr/0003-database-for-workspace.md`](../adr/0003-database-for-workspace.md)
- ADR 0005 Identity Provider: [`../adr/0005-identity-provider.md`](../adr/0005-identity-provider.md)
- ADR 0004 Object storage (planned): [`../adr/0004-object-storage.md`](../adr/0004-object-storage.md)
- Plan 0021 Workspace security hardening: [`../../plans/0021-gar-425-workspace-security-hardening.md`](../../plans/0021-gar-425-workspace-security-hardening.md)
- Plan 0022 Workspace security part 2: [`../../plans/0022-gar-426-workspace-security-part-2.md`](../../plans/0022-gar-426-workspace-security-part-2.md)
- Plan 0024 /metrics endpoint auth: [`../../plans/0024-gar-412-metrics-endpoint-auth.md`](../../plans/0024-gar-412-metrics-endpoint-auth.md)
