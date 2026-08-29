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

**Nota sobre `working_dir`**: `projects_handler::create_session_with_project`
aceita um `working_dir` e foi endurecido junto, mas **não está roteado** —
`POST /api/sessions` vai para `api::create_session`, cujo `CreateSessionRequest`
só tem `agent_id`, então o campo é descartado pelo serde. Não era superfície
viva; o guard existe para o dia em que a rota mudar, e
`post_sessions_does_not_expose_working_dir_today` fixa esse estado.

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
