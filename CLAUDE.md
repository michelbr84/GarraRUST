# GarraIA — Gateway de IA Multi-Canal

> Rust-based AI gateway: multi-channel, multi-provider LLM orchestration with mobile client.

## Identidade do Projeto

- **Nome:** GarraIA (GarraRUST)
- **Stack:** Rust (Axum 0.8) + Flutter + Tauri v2
- **Repo:** michelbr84/GarraRUST
- **Equipe Linear:** GAR

## Protocolo de início de sessão

1. Leia `.garra-estado.md` para contexto da sessão anterior
2. Verifique `git status` e `git log --oneline -5`
3. Consulte a memória em `.claude/` se o contexto for relevante

## Estrutura de crates

Atualizado após GAR-407 (2026-04-13). **18 crates ativos** no workspace + 1 PoC efêmero em `benches/`.

```text
crates/
  garraia-cli/        — binário "garraia" (clap), wizard, chat interativo, migrate
  garraia-gateway/    — servidor HTTP/WS (Axum 0.8), admin API, MCP registry, router
  garraia-agents/     — LLM providers (OpenAI/OpenRouter/Anthropic/Ollama), AgentRuntime, tools
  garraia-channels/   — Telegram, Discord, Slack, WhatsApp, iMessage
  garraia-db/         — SQLite (rusqlite), SessionStore, CRUD (dev/CLI single-user)
  garraia-security/   — CredentialVault (AES-256-GCM), PBKDF2, RedactingWriter
  garraia-config/     — schema unificado de config (serde + validator + notify)
  garraia-telemetry/  — ✅ OpenTelemetry + Prometheus baseline (GAR-384) — feature-gated
  garraia-workspace/  — ✅ Postgres 16 + pgvector multi-tenant — Fase 3 schema COMPLETO
                        (GAR-407 + GAR-386 + GAR-388 + GAR-389 + GAR-408 + GAR-390).
                        25 tabelas em 6 migrations, 18 sob FORCE RLS, 7 tenant-root
                        sob app-layer:
                        • 001 users/groups/identities/sessions/api_keys/invites (tenant roots)
                        • 002 RBAC roles/permissions/63 role_permissions + audit_events + single-owner idx
                        • 004 chats/chat_members/messages (FTS) /message_threads com compound FK
                        • 005 memory_items/memory_embeddings (pgvector HNSW cosine)
                        • 006 tasks Tier 1 Notion-like (8 tabelas com RLS embedded + subtasks)
                        • 007 RLS FORCE wrap-up em 10 tabelas com NULLIF fail-closed
                        Slot 003 reservado para GAR-387 (files, bloqueado por ADR 0004).
                        Handle PII-safe via skip(config) + custom Debug redaction.
                        Decisão: docs/adr/0003-database-for-workspace.md.
  garraia-plugins/    — sandbox WASM inicial (wasmtime) — features adicionais na Fase 2.2
  garraia-voice/      — STT (Whisper) + TTS (Chatterbox/ElevenLabs/Kokoro)
  garraia-media/      — processamento de PDF, imagens, mídia
  garraia-skills/     — registry de skills para o agente
  garraia-tools/      — tools compartilhadas (file ops, search, web)
  garraia-runtime/    — runtime helpers
  garraia-common/     — tipos + erros compartilhados
  garraia-glob/       — glob matching utilitário
  garraia-desktop/    — Tauri v2 app (Windows MSI, overlay)
apps/
  garraia-mobile/     — Flutter Android client (Riverpod, go_router, Dio)
```

### Crates planejados (ROADMAP AAA Fases 2-3)

```text
garraia-embeddings/  — Fase 2.1 (GAR-372) — embeddings locais mxbai + vector store lancedb
garraia-auth/        — Fase 3.3 (GAR-391) — Scope/Principal/RBAC central separado de -security
garraia-storage/     — Fase 3.5 (GAR-394) — trait ObjectStore (LocalFs/S3/MinIO) + presigned + tus
```

### PoCs efêmeros

```text
benches/
  database-poc/    — GAR-373 bench harness (Postgres vs SQLite). Crate ISOLADO, NÃO é
                     workspace member. Deletar depois que garraia-workspace (GAR-407)
                     estiver estabilizado. Tem [workspace] próprio no Cargo.toml.
```

## Convenções de código

### Rust

- `AppState` é `Arc<AppState>` — import via `crate::state::AppState`
- DB via `SessionStore` (rusqlite, sync, `tokio::sync::Mutex`)
- Axum 0.8: `FromRequestParts` usa AFIT nativo — **sem** `#[async_trait]`
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

### Commits

- Formato: Conventional Commits (`feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`)
- Imperativo: "adiciona feature" (não "adicionada feature")
- Limite 72 chars no assunto

## Regras absolutas

1. **NUNCA** commitar `.env`, credenciais ou tokens
2. **NUNCA** `rm -rf /`, `rm -rf ~` ou fork bombs
3. **NUNCA** force push para `main`
4. **NUNCA** usar `unwrap()` em código de produção (apenas em testes)
5. **NUNCA** concatenar strings em SQL queries — `params!` (rusqlite) ou `sqlx::query!` (Postgres)
6. **NUNCA** expor secrets/PII em logs (`GARRAIA_JWT_SECRET`, `ANTHROPIC_API_KEY`, etc.)
7. **NUNCA** ignorar erros de compilação do `cargo check`
8. **SEMPRE** escrever ADR em `docs/adr/NNNN-*.md` antes de decisão arquitetural irreversível (Postgres vs SQLite, vector store, storage backend, etc.) — ver `ROADMAP.md` §3.1
9. **SEMPRE** migrations Postgres forward-only (colunas novas → backfill → NOT NULL depois)
10. **SEMPRE** testes de autorização cross-group antes de merge em qualquer rota nova de `garraia-workspace`/`garraia-auth`

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
| `/assemble-team` | Monta equipe de agentes coordenados |
| `/generate-docs` | Gera documentação automática |
| `/code-review` | Revisão de código inline |
| `/git-assist` | Ajuda com git workflow |

## Agents disponíveis

| Agent | Papel |
| ------- | ------- |
| `code-reviewer` | Revisor sênior Rust/Flutter |
| `security-auditor` | Auditor OWASP, JWT, crypto |
| `doc-writer` | Escritor técnico PT-BR/EN |
| `team-coordinator` | Orquestrador de equipes de agentes |

## Ferramentas preferenciais

- Buscar arquivos: `Glob` (não `find`)
- Buscar conteúdo: `Grep` (não `grep`)
- Ler arquivos: `Read` (não `cat`)
- Editar arquivos: `Edit` (não `sed`)
- Testar Rust: `cargo test -p <crate>`
- Testar Flutter: `flutter test`
- Lint Rust: `cargo clippy --workspace`

## Referências

- @imports `.claude/agents/` para agentes especializados
- @imports `skills/` para workflows reutilizáveis
- @imports `.garra-estado.md` para estado da sessão anterior
- @imports `ROADMAP.md` — plano AAA em 7 fases, fonte de verdade do planejamento
- @imports `deep-research-report.md` — base arquitetural da Fase 3 (Group Workspace multi-tenant)
- @imports `docs/adr/` — decisões arquiteturais. **Accepted:** 0003 (Postgres para Group Workspace). **Proposed/blocked:** 0001, 0002, 0004-0008. Ver `docs/adr/README.md` para o índice.
- Linear: [time GarraIA-RUST (GAR)](https://linear.app/chatgpt25/team/GAR/projects) — execução semana a semana
