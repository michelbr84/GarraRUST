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

```
crates/
  garraia-gateway/    — servidor HTTP/WS (Axum 0.8), admin API, MCP registry
  garraia-agents/     — LLM providers (OpenAI/Anthropic/Ollama), AgentRuntime, tools
  garraia-db/         — SQLite (rusqlite), SessionStore, CRUD
  garraia-security/   — CredentialVault (AES-256-GCM), PBKDF2
  garraia-channels/   — Telegram, Discord, Slack, WhatsApp, iMessage
  garraia-desktop/    — Tauri v2 app (Windows MSI)
apps/
  garraia-mobile/     — Flutter Android client (Riverpod, go_router, Dio)
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
5. **NUNCA** concatenar strings em SQL queries (usar `params!`)
6. **NUNCA** expor secrets em logs (`GARRAIA_JWT_SECRET`, `ANTHROPIC_API_KEY`, etc.)
7. **NUNCA** ignorar erros de compilação do `cargo check`

## Skills disponíveis

| Skill | Uso |
|-------|-----|
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
|-------|-------|
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
