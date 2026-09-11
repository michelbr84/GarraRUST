# GarraIA — Gateway de IA Multi-Canal

> Rust-based AI gateway: multi-channel, multi-provider LLM orchestration with mobile client.
> **Harness:** ClaudeMaxPower (branded localmente como "GarraIA SuperPowers") + official Superpowers plugin — hooks, agent teams, quality gates e subagents são parte do workflow obrigatório (ver `skills/`, `.claude/agents/`, `.claude/hooks/`).

## Identidade do Projeto

- **Nome:** GarraIA (GarraRUST)
- **Stack:** Rust (Axum 0.8) + Flutter + Tauri v2
- **Repo:** michelbr84/GarraRUST
- **Tracking:** tracker interno desde 2026-08-18 (Linear descontinuado; IDs `GAR-xxx` neste doc são registro histórico)

## Protocolo de início de sessão

1. Leia `TODO.md` (backlog operacional) e, se existir, `.garra-estado.md` (handoff local gerado por `garra max-power`; gitignored, ausente em clone novo)
2. Verifique `git status` e `git log --oneline -5`
3. Consulte a memória em `.claude/` se o contexto for relevante

## Estrutura de crates

**22 crates ativos** no workspace (contagem ao vivo: `grep -c '^    "crates/' Cargo.toml`),
mais o harness `benches/agent-framework-comparison/` (fora do workspace, não é crate).
O histórico de entrega (plans, PRs, datas, IDs `GAR-xxx`) vive em `plans/`, `docs/adr/`
e `CHANGELOG.md` — aqui fica só o estado atual e os invariantes que um agente precisa
respeitar.

```text
crates/
  garraia-cli/        — binário "garraia" (clap): wizard, chat interativo, `config check`
                        (plan 0035; exit codes sysexits 0/2/65), `migrate workspace
                        --from-sqlite --to-postgres` (plans 0039/0040/0045: users +
                        identities + groups + chats, audit atômico in-tx; stages 6+
                        pendentes). `spinner.rs`: estado puro sem relógio, renderizado
                        como braço do `tokio::select!` em `stream_turn` — nunca task
                        própria; `detect()` devolve `None` fora de TTY / `NO_COLOR` /
                        `TERM=dumb` / `GARRAIA_NO_SPINNER`; nunca esconde o cursor;
                        fallback ASCII; proibido em `ask.rs` e `mcp_server.rs` (teste
                        varre o fonte). `Ctrl+C` cancela o turno, não o processo.
  garraia-gateway/    — servidor HTTP/WS (Axum 0.8), admin API, MCP registry, router.
                        `webchat.html` (`GET /`) segue o design system Garra Glass
                        (ADR 0009): tokens `--garra-*`, gold `#ffd400` para CTAs, cyan
                        `#16d9ff` para info/foco, Inter + JetBrains Mono. **Nunca**
                        importar Bootstrap/AdminLTE/Animate.css de CDN — ports inline.
                        Web Console em `/api/*` (auth-free, secret-free): `/api/health`,
                        `/api/capabilities`, `/api/channels`, `POST /api/providers/test`,
                        `PATCH /api/providers/default`, `/api/settings/{schema,effective}`
                        (secrets como `configured: bool`, nunca valor), `PATCH /api/settings`
                        (validate + audit + dry-run), `/api/diagnostics` (checks com
                        `next_step`; `voice.tts`/`voice.stt` sondam servidores locais
                        com modo voz ligado).
                        Auth: sem fallback de JWT secret hardcoded — `AppState::
                        jwt_signing_secret() -> Result<SecretString, AuthConfigMissing>`
                        e handlers respondem **503 fail-closed** sem secret (plan 0046).
                        `std::env::var("GARRAIA_JWT_SECRET")` / `("GarraIA_VAULT_PASSPHRASE")`
                        só em `crates/garraia-config/src/auth.rs`; matriz de precedência
                        em `docs/auth-config.md`. Passphrase do cofre: as duas grafias
                        são aceitas via `garraia_security::vault_passphrase_from_env()`
                        (all-caps canônica > mixed-case deprecated com warning); em
                        `AuthConfig::from_env` a ordem é `GARRAIA_JWT_SECRET` >
                        `GarraIA_VAULT_PASSPHRASE` > `GARRAIA_VAULT_PASSPHRASE` (#824).
                        Uploads tus 1.0 em `rest_v1::uploads` (`POST/HEAD/PATCH/DELETE
                        /v1/uploads[/{id}]` + `OPTIONS`, precondition `Tus-Resumable:
                        1.0.0`), ledger `tus_uploads` (migration 014, FORCE RLS),
                        `ObjectStore` em `AppState` via `StorageConfig`, commit two-phase
                        blob-first (plan 0044 §5.3.1), cap `storage.max_patch_bytes`
                        default 100 MiB, worker de expiração em `uploads_worker.rs`.
                        CI (`.github/workflows/ci.yml`): o binário é `garraia`
                        (`cargo build --bin garraia --release`; `garraia-gateway` é lib);
                        zero `continue-on-error` ativo (higiene #1094). Specs Playwright
                        do admin DEVEM usar `data-testid` estáveis (contrato de teste),
                        não `placeholder*=` nem `getByRole(button,{name})` (plan 0052).
  garraia-agents/     — LLM providers (OpenAI/OpenRouter/Anthropic/Ollama), AgentRuntime, tools
  garraia-auth/       — IdentityProvider trait + InternalProvider; `LoginPool`/`SignupPool`
                        newtypes (inner PgPool privado, validado via SELECT current_user,
                        !Clone via static_assertions); Role/Action enums + `can()` central
                        (teste table-driven 5×22); `Principal` extractor (FromRequestParts)
                        + `RequirePermission` como método de struct (limitação
                        const-generic do Axum). Crypto: Argon2id (RFC 9106 m=64MiB,t=3,
                        p=4) + PBKDF2 dual-verify + lazy upgrade transacional sob
                        `FOR NO KEY UPDATE OF ui` + anti-enumeração constant-time via
                        `DUMMY_HASH` (build.rs). JWT HS256 (15 min) com guards de
                        algorithm-confusion + refresh opaco HMAC-SHA256. PII:
                        `Credential.password` em `SecretString`. Endpoints default-on:
                        `POST /v1/auth/{login,refresh,logout,signup}` — 401 byte-identical
                        em toda falha, 409 em signup duplicado, audit em todos os
                        terminais. Matriz RLS (81 cenários, pgvector/pg16 real) e matriz
                        HTTP cross-group (`tests/authz_http_matrix.rs`, 50 cenários) são
                        o gate. Decisão: `docs/adr/0005-identity-provider.md`.
  garraia-channels/   — Telegram, Discord, Slack, WhatsApp, iMessage
  garraia-db/         — SQLite (rusqlite), SessionStore, CRUD (dev/CLI single-user).
                        `update_mobile_user_hash` faz lazy upgrade PBKDF2 → Argon2id
                        sem mexer no schema.
  garraia-security/   — CredentialVault (AES-256-GCM), PBKDF2, RedactingWriter
  garraia-config/     — schema unificado de config (serde + validator + notify). Módulo
                        `check` (`run_check` + `ConfigCheck`/`Finding`/`Severity`/
                        `SourceReport`) alimenta `garraia config check [--json] [--strict]`.
                        `StorageConfig` + `StorageBackend` (`local` | `s3`) + `LocalFsConfig`
                        + `S3Config` com validações (staging_dir gravável, faixa de
                        `max_patch_bytes`, endpoint S3, MIME allow-list override via
                        `allow_unsafe_mime_in_local_fs`). `AuthSection` em `AppConfig` só
                        com knobs não-secret (algoritmo, TTLs, hint de métricas) —
                        secrets seguem env-only via `AuthConfig::from_env`. Invariante
                        de redaction: `config check` (humano + JSON) só reporta presença
                        (`api_key_set: true`), nunca valores.
  garraia-telemetry/  — OpenTelemetry + Prometheus baseline — feature-gated
  garraia-workspace/  — Postgres 16 + pgvector multi-tenant (Fase 3 schema completo).
                        37 tabelas em 33 migrations; 32 sob FORCE RLS e 5 fora (users,
                        roles, permissions, role_permissions, group_invites). Marcos:
                        001 tenant roots (users/groups/identities/sessions/api_keys/
                        invites) · 002 RBAC + audit_events · 003 folders/files/
                        file_versions (compound FK, object_key UNIQUE, HMAC integrity)
                        · 004 chats/chat_members/messages (FTS)/threads · 005 memory +
                        pgvector HNSW cosine · 006 tasks Notion-like · 007 FORCE RLS
                        wrap-up com NULLIF fail-closed · 008 role `garraia_login`
                        NOLOGIN BYPASSRLS · 010 role `garraia_signup` · 013 audit WITH
                        CHECK explícito · 014 `tus_uploads` (CHECK ≤ 5 GiB, índice
                        parcial `expires_in_progress_idx`). Handle PII-safe via
                        skip(config) + Debug redigido. ADRs 0003 e 0004.
  garraia-plugins/    — sandbox WASM inicial (wasmtime) — features adicionais na Fase 2.2
  garraia-voice/      — STT (Whisper) + TTS (Chatterbox/ElevenLabs/Kokoro)
  garraia-media/      — processamento de PDF, imagens, mídia
  garraia-skills/     — registry de skills para o agente
  garraia-learning/   — Garra Learning Agent / Self-Improving Operations Manual (ADR
                        0010). 10 módulos: miner, generator, registry, retriever (stub
                        até garraia-embeddings real), evaluator, updater, safety (gate
                        hard-wall), versioning (git-backed), skill_override (CLI stub),
                        lib. Frontmatter `LearningSkillFrontmatter` com `score`/`locked`/
                        `critical_paths_touched`/`fail_count`. Separação rígida: memória
                        (`workspace.memory_items`) ≠ skill (`learning.skills`) ≠ log
                        (`telemetry.traces`) ≠ manual distribuível (crate `garraia-skills`).
                        Nunca copiar código do Hermes Agent — referência só conceitual.
  garraia-tools/      — tools compartilhadas (file ops, search, web)
  garraia-runtime/    — runtime helpers
  garraia-common/     — tipos + erros compartilhados
  garraia-glob/       — glob matching utilitário
  garraia-desktop/    — Tauri v2 app: bandeja + overlay do papagaio + Chat Bar
                        (Ctrl+Space); MSI/NSIS no Windows e .deb/AppImage no Linux,
                        CLI como sidecar `binaries/garraia`
  garraia-embeddings/ — Fase 2.1 (ADR 0002; ADR 0018 Proposed). Só superfície pública:
                        traits `EmbeddingProvider` + `VectorStore` (scoped por `Scope` +
                        `Option<Uuid> group_id`), tipos `Scope`/`EmbeddingVector(768)`/
                        `Document`/`Chunk`/`SearchHit`, `HybridQuery` builder (rejeita
                        cross-tenant em build-time), `DeterministicProvider` (feature
                        `testing-provider`). `PgVectorStore` real e `MxbaiProvider`
                        ainda não existem; sem wiring em learning/agents.
  garraia-storage/    — trait `ObjectStore` (`#[async_trait]`, usado como `dyn`) +
                        `LocalFs` + `path_sanitize`; `S3Compatible` (aws-sdk-s3) atrás
                        da feature `storage-s3` com SSE-S3 obrigatório, MIME allow-list
                        compartilhada, HMAC-SHA256 sobre `{key}:{version_id}:{sha256_hex}`
                        via `PutOptions::hmac_secret`, presigned URLs com TTL [30s, 900s].
                        MinIO via endpoint override; testcontainer gated pela feature.
apps/
  garraia-mobile/     — Garra Mobile (Flutter, Riverpod 3, go_router, Dio). v0.4.0
                        (ADR 0016): home "Garra Neon" + `lib/runtime/` (`GarraConnection`:
                        Termux local em 127.0.0.1:3888 / outro Garra na LAN / Garra Cloud)
                        + negociação de capabilities via `/api/capabilities`. Codegen:
                        `dart run build_runner build` (`*.g.dart` gitignored). APK sai do
                        CI (`mobile.yml` em PR, `build-android-apk` na release) — este
                        container não alcança o Android SDK.
benches/
  agent-framework-comparison/ — harness shell (`run.sh` + `README.md` + `results/`),
                        NÃO é crate nem workspace member. Mede binário, pico de RSS e
                        cold start contra CLIs concorrentes fixados por ref via env var;
                        resultados versionados em `results/<data>-<host>/`.
```

> Sem crates planejados no momento. `benches/database-poc/` foi removido em 2026-08-16;
> seus números seguem citados em ADR 0003 e nas migrations 005/007.

## Convenções de código

### Rust

- `AppState` é `Arc<AppState>` — import via `crate::state::AppState`
- DB via `SessionStore` (rusqlite, sync, `tokio::sync::Mutex`)
- Axum 0.8: `FromRequestParts` usa AFIT nativo — **sem** `#[async_trait]`.
  Exceção documentada: traits que são usados como `dyn Trait` (ex.:
  `garraia_storage::ObjectStore`) usam `#[async_trait]` por causa de
  limitação de AFIT + `dyn` em Rust stable. Ver plan 0037 §5.1.
- Usar `?` operator para tratamento de erros (não `unwrap()` em produção)
- SQL queries via `params!` macro (nunca concatenar strings)
- `cargo check -p <crate>` antes de qualquer commit
- `cargo clippy --workspace` para linting

### Flutter

- State management: Riverpod + code generation
- Navigation: go_router com auth redirect
- HTTP: Dio com `_AuthInterceptor` (JWT bearer)
- Nunca usar `withOpacity()` — usar `withValues(alpha:)`

### Shell / Scripts

- `set -euo pipefail` em todos os scripts
- Usar `#!/usr/bin/env bash` (não `/bin/bash`)
- Paths devem funcionar cross-platform (usar `which` ou env vars)

### Convenção de datas

- **Project narrative dates** (ROADMAP, plans, ADRs, READMEs, commit prose, doc paragraphs como "entregue em YYYY-MM-DD") usam **America/New_York (Florida)** local time. Nunca usar UTC para data narrativa do projeto sem dizer explicitamente.
- **API timestamps, audit_events, log timestamps, JWT `iat`/`exp`, `expires_at` em response bodies** são sempre **UTC ISO 8601 com sufixo `Z`** — declaração explícita de UTC.
- Quando estiver em dúvida em prosa de doc/plan/commit, use o local time da Flórida. Se a referência for tecnicamente UTC (ex.: timestamp de log capturado), anote `(UTC)` inline.

### Commits

- Formato: Conventional Commits (`feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`)
- Imperativo: "adiciona feature" (não "adicionada feature")
- Limite 72 chars no assunto

### Changelog

- **NUNCA** editar o `CHANGELOG.md` direto num PR de feature/fix. Cada PR
  deixa um fragmento em `changelog.d/<seção>/<numero>-<slug>.md` — arquivos
  diferentes nunca conflitam, e era a colisão na seção `[Unreleased]` que
  fazia todo par de PRs paralelos precisar de um merge de resolução.
- Seções válidas: `added`, `changed`, `deprecated`, `removed`, `fixed`,
  `security` (Keep a Changelog 1.1.0). Texto sem acento, como o resto do
  `CHANGELOG.md`.
- `python3 scripts/changelog/assemble.py --check` valida os fragmentos;
  `--write` é **passo de release** (`docs/releasing.md` §1.3), não de PR.
- Ver `changelog.d/README.md`.

## Regras absolutas

1. **NUNCA** commitar `.env`, credenciais ou tokens
2. **NUNCA** `rm -rf /`, `rm -rf ~` ou fork bombs
3. **NUNCA** force push para `main`
4. **NUNCA** usar `unwrap()` em código de produção (apenas em testes)
5. **NUNCA** concatenar strings em SQL queries — `params!` (rusqlite) ou `sqlx::query!` (Postgres).
   Desde o sqlx 0.9 o compilador reforça isso: `sqlx::query()` / `query_as()` / `query_scalar()` /
   `raw_sql()` só aceitam `&'static str` (trait `SqlSafeStr`), então qualquer `format!()` vira erro
   `E0277`. **Exceção única e delimitada:** identificador SQL que o Postgres não aceita como bind
   parameter (nome de tabela/coluna, `ORDER BY` dinâmico, DDL). Nesses casos usar
   `sqlx::AssertSqlSafe` (note: `sqlx::AssertSqlSafe`, **não** `sqlx::sql_str::AssertSqlSafe`, que
   não é público) **com comentário de auditoria nomeando a origem fechada do valor**. Não é
   permitido em `crates/*/src/` — só em alvos de teste; hoje há exatamente 3 usos, todos em
   `tests/`. Valor de dado continua **sempre** por `.bind()`. Para GUC de RLS, use
   `SELECT set_config('app.current_group_id', $1, true)` com bind — nunca `SET LOCAL` interpolado.
6. **NUNCA** expor secrets/PII em logs (`GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_METRICS_TOKEN`, `ANTHROPIC_API_KEY`, etc.)
7. **NUNCA** ignorar erros de compilação do `cargo check`
8. **SEMPRE** escrever ADR em `docs/adr/NNNN-*.md` antes de decisão arquitetural irreversível (Postgres vs SQLite, vector store, storage backend, etc.) — ver `ROADMAP.md` §3.1
9. **SEMPRE** migrations Postgres forward-only (colunas novas → backfill → NOT NULL depois)
10. **SEMPRE** testes de autorização cross-group antes de merge em qualquer rota nova de `garraia-workspace`/`garraia-auth`
11. **SEMPRE** usar a `garraia_login` BYPASSRLS dedicated role exclusivamente em paths de credential verification (login + lazy upgrade PBKDF2→Argon2id + extractor membership lookup + refresh token verify/revoke). Acesso ao role só via `garraia-auth::LoginPool` newtype — nunca raw `PgPool`. Documentado em `docs/adr/0005-identity-provider.md` (com Amendment 2026-04-13 cobrindo Gaps A/C).
12. **NUNCA** ler `user_identities.password_hash` no app pool role (`garraia_app`) — RLS filtra para 0 rows. Tratar 0 rows como "user not found" é anti-pattern (significa "RLS bloqueou"). Sempre usar `garraia_login` via login endpoint. Ver ADR 0005 §"Anti-patterns".
13. **SEMPRE** usar a `garraia_signup` BYPASSRLS dedicated role exclusivamente para o signup flow (`POST /v1/auth/signup`). Acesso só via `garraia-auth::SignupPool` newtype — nunca raw `PgPool`, nunca substituível pelo `LoginPool`. O role tem `INSERT` em `users`/`user_identities` mas NENHUM acesso a `sessions`, `messages`, `chats`, `memory_*`, `tasks*`, `groups`, `group_members` ou qualquer dado de tenant. Migration 010, ADR 0005 §"Amendment 2026-04-13" Gap B.
14. **SEMPRE** passar por `garraia_common::ssrf` (`vet_url` + `pinned_client`) qualquer requisição HTTP de saída cuja URL venha de um request, de config editável por request, ou de uma tool call de LLM. **NUNCA** `reqwest::get(url)` cru nesses caminhos. O guard faz allowlist de esquema, resolve o host uma vez e bloqueia faixas internas, pina os IPs vetados (`resolve_to_addrs`, anti-DNS-rebinding), desliga redirects e limita o corpo. Auth não substitui o guard: `plugins_handler` exige `Permission::ManagePlugins` **e** valida a URL. Onde o alvo legítimo é local (Ollama, MCP self-hosted), usar `IpScope::AllowPrivate` — que ainda bloqueia link-local (`169.254.169.254`), CGNAT, multicast e unspecified. Ver `docs/security/threat-model.md` §5.6.
15. **NUNCA** renomear ou remover os assets de release "crus" (`garraia-<os>-<arch>[.exe]`)
    nem seus `<asset>.sha256` irmaos. `crates/garraia-cli/src/update.rs:42-48` resolve o
    asset por nome exato e `:127` exige o `.sha256` irmao, entao qualquer renomeacao quebra
    o `garra update` de toda instalacao ja existente no momento em que ela pular para essa
    versao. Formatos novos (`.tar.gz`, `.zip`, `.msi`, `.deb`, `.rpm`, `.AppImage`) entram
    **aditivamente**, ao lado.
    O `select_checksum_line` do `install.sh` (e o `Select-ChecksumLine` do `install.ps1`)
    ancoram o nome em fim de linha justamente para que `garraia-linux-x86_64` nunca case
    com `garraia-linux-x86_64.tar.gz`.
    **O corpo da release vem do CHANGELOG, nao da lista de commits.** O
    `release.yml` roda `scripts/release/notes.py "$VERSION" CHANGELOG.md` e passa o
    resultado como `body_path`; o script extrai a *prosa de abertura* da secao da
    versao (o que vem antes da primeira subsecao `###`) e o softprops anexa a lista
    gerada embaixo. A secao inteira nao serve: a da v0.3.9 tem 1298 linhas e 90 KB,
    perto do teto de 125 KB do GitHub. Apagar o script quebra a release inteira — ele
    roda **antes** do `Create Release`, e o job usa `set -euo pipefail`.
    **E cuidado com `.gitignore` sem ancora:** `release/` sem a barra inicial casa em
    QUALQUER profundidade e ja engoliu `scripts/release/` uma vez, com o `git add -A`
    pulando o arquivo em silencio e a release referenciando um script inexistente. Os
    padroes de artefato de build sao `/release/` e `/dist/`, ancorados na raiz.
16. **SEMPRE** manter `install.sh` e `install.ps1` em paridade de comportamento. Sao o mesmo
    contrato em dois sistemas operacionais (flags, env vars, precedencia env-vence-flag,
    verificacao SHA-256, encadeamento `init`/`start`); mudou um, muda o outro, e as suites
    em `tests/install_sh/` e `tests/install_ps1/` espelham-se uma a outra. A paridade
    vale também para o *serving*: cada instalador tem cópia estática + entrada no
    workflow de sync do repo do site (regra 17) e sondas espelhadas no
    `install-endpoints.yml`.
17. **O site `garraia.org` NÃO está neste repositório.** Ele vive em
    `michelbr84/garraia-74c335d5` (Vite + React + shadcn, publicado pelo Lovable).
    Editar `install.sh`/`install.ps1` aqui **não** muda o que `garraia.org` serve.
    O hosting da Lovable **IGNORA** `public/_redirects`: `/install.sh` e
    `/install.ps1` só funcionam porque existem **cópias estáticas em `public/`**
    daquele repo (o Vite copia `public/*` para a raiz do build), mantidas em
    sincronia com o raw do `main` daqui pelo workflow de sync do site
    (`sync-install-sh.yml`, cron diário). Publicar em produção é um passo
    **manual** do Lovable (Publish no editor, ou `deploy_project` via MCP) —
    merge no repo do site não muda o que garraia.org serve até alguém publicar.
    Essa separação já causou dois apagões do
    `irm https://garraia.org/install.ps1 | iex`: em 2026-08, HTML da home com
    HTTP 200 (o fallback SPA engolia a rota, na época em que se acreditava que
    `_redirects` valia); em 2026-08-31, 404 puro (faltava `public/install.ps1`).
    Ao adicionar um instalador novo, adicione **junto** o arquivo estático + a
    entrada no sync do repo do site, e republique. O `_redirects` e o
    `infra/nginx.conf` de lá são future-proofing para self-hosting, não o
    mecanismo vivo. O workflow `install-endpoints.yml` (agendado, não é gate de
    PR) sonda todas as URLs documentadas, imprime o código HTTP de cada sonda,
    roda todas antes de falhar e verifica o contrato real do Windows com
    `Invoke-RestMethod`. O Worker em `deploy/installer-worker/` está
    descontinuado e nunca serviu o `garraia.org`.
18. **NUNCA** cherry-pickar ou misturar arquivos de migration (`crates/garraia-workspace/migrations/`) entre repositórios diferentes sem verificar a numeração estrita e linear. Os esquemas e numerações de migrations são independentes e forward-only.

## Framework de Desenvolvimento: Superpowers

O projeto utiliza [Superpowers](https://github.com/obra/superpowers) como framework primário de workflow de desenvolvimento.

- **Config:** `.claude/superpowers-config.md` — contexto do projeto para o Superpowers
- **Bridge:** `skills/superpowers-bridge.md` — mapeamento entre skills locais e Superpowers
- **Regra:** Para features novas, bugs complexos e refactoring → usar workflow Superpowers (brainstorming → spec → plan → TDD → review → merge)
- **Skills locais** são usadas para operações específicas: pre-commit, generate-docs, translate, shell-explain

## Skills disponíveis

| Skill | Uso |
| ------- | ----- |
| `/superpowers-bridge` | Mapeamento skills locais ↔ Superpowers |
| `/review-pr` | Revisa PR com code-reviewer + security-auditor |
| `/tdd-loop` | Red-Green-Refactor automático |
| `/fix-issue` | Corrige issue GitHub via TDD |
| `/pre-commit` | Validação pré-commit (segredos, debug, lint) |
| `/refactor-module` | Refactoring seguro com testes |
| `/assemble-team` | Monta equipe de agentes coordenados, selecionada por risco R0-R5 |
| `/repo-autopilot` | Varredura autonoma: triagem de issues/PRs, correcoes e relatorio de saude |
| `/generate-docs` | Gera documentação automática |
| `/code-review` | Revisão de código inline |
| `/git-assist` | Ajuda com git workflow |
| `steward` | Guia para dirigir PR até o verde — quais falhas de CI são ambientais e quais são suas. Não é slash command: é lida automaticamente por um agente ao reagir a evento de CI/review num PR. |

## Agents disponíveis

| Agent | Modelo | Papel |
| ------- | ------ | ------- |
| `team-coordinator` | tencent/hy4-preview | Orquestração, delegação e decisão de merge. Nao implementa |
| `repo-analyst` | deepseek/deepseek-v4-flash-0731 | Diagnostico de issues/PRs, causa raiz, duplicadas. Nao escreve codigo |
| `implementer` | z-ai/glm-5.3-flash | Implementacao Rust/Flutter em worktree isolada |
| `test-engineer` | deepseek/deepseek-v4-flash-0731 | fmt/check/clippy/test e teste de regressao |
| `code-reviewer` | openai/gpt-5.6-luna | Revisao independente e gate MERGE_READY |
| `security-auditor` | openai/gpt-5.6-luna | auth, JWT, crypto, RLS, SSRF, secrets. Convocado em R4 |
| `doc-writer` | deepseek/deepseek-v4-flash-0731 | README/SETUP/CHANGELOG, docstrings e higiene do repo |

Modelos diferentes de proposito para Implementer e Reviewer: quem escreve nao julga.
Selecao por risco (R0-R5) em `skills/assemble-team.md`; varredura autonoma em
`skills/repo-autopilot.md`. R5 (release, secrets, destrutivo) sempre escala ao humano.

## Ferramentas preferenciais

- Buscar arquivos: `Glob` (não `find`)
- Buscar conteúdo: `Grep` (não `grep`)
- Ler arquivos: `Read` (não `cat`)
- Editar arquivos: `Edit` (não `sed`)
- Testar Rust: `cargo test -p <crate>`
- Testar Flutter: `flutter test`
- Lint Rust: `cargo clippy --workspace`

## AI Quality Ratchet (`.quality/` + `scripts/quality/`)

Sistema de Quality Gates inspirado no padrão Catraca: métricas só sobem ou ficam, **nunca regridem**. Vide `plans/0064-quality-ratchet-pr1.md` para o scaffold inicial e `.quality/README.md` para a filosofia completa.

**Status atual: PR-1 — report-only.** Nenhum PR é bloqueado pelo ratchet ainda. Workflow `.github/workflows/quality-ratchet.yml` posta `quality-report.md` como comentário no PR e segue. Promoção a bloqueante (`compare.py --mode enforce`) entra em PR-4 com aprovação explícita.

### Comandos rápidos

```bash
# Coleta rápida (default — sob 10s):
bash scripts/quality/collect-metrics.sh > current-metrics.json

# Comparar contra baseline (report-only — sempre exit 0):
python3 scripts/quality/compare.py --mode report-only \
    .quality/baseline.json current-metrics.json

# Propor novo baseline (gera .proposed.json — NÃO commita):
python3 scripts/quality/freeze-baseline.py current-metrics.json

# Rodar testes dos parsers:
python3 -m pytest scripts/quality/tests/
```

### Regras absolutas (ratchet)

- **NUNCA** editar `.quality/baseline.json` manualmente para "passar" o ratchet — é fraude. Use `freeze-baseline.py` que gera `.quality/baseline.proposed.json` para review humano.
- **NUNCA** adicionar `continue-on-error: true` em workflows. Modo report-only é controlado pela flag `compare.py --mode report-only`.
- **NUNCA** desativar gates pré-existentes do `ci.yml` (fmt/clippy/test/audit/deny/etc.).
- Se o `/quality-babysit` propuser correção que toca segurança, auth, storage, RLS, secrets ou CI crítico → chamar `security-auditor` + `code-reviewer` agents antes de continuar (ver `.claude/commands/quality-babysit.md` §Guardrails).

## Referências

- @imports `.claude/agents/` para agentes especializados
- @imports `skills/` para workflows reutilizáveis
- @imports `TODO.md` (backlog operacional) e `.garra-estado.md` (handoff local, gitignored) para estado da sessão anterior
- @imports `ROADMAP.md` — plano AAA em 7 fases, fonte de verdade do planejamento
- @imports `deep-research-report.md` — base arquitetural da Fase 3 (Group Workspace multi-tenant)
- @imports `docs/adr/` — decisões arquiteturais: 19 ADRs (0001-0019). As 0001-0017 e a **0019** (confinamento das tools, #1084) estão **Accepted**; a **0018** (crate `garraia-embeddings`, #949) está **Proposed** — a decisão é do dono, e aceitá-la é o gatilho da remoção. Ver `docs/adr/README.md` para o índice.
- Tracking: tracker interno (o Linear foi descontinuado em 2026-08-18 — não criar/consultar issues lá; IDs `GAR-xxx` permanecem como registro histórico de entregas)
