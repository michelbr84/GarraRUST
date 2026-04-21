# Plan 0027 — Mobile Alpha docs (OpenAPI v1 spec + QA checklist)

> **Docs-only slice** fechando 2 issues deferidas do EPIC GAR-331 (Mobile Alpha): **GAR-332** (OpenAPI contract formal) e **GAR-368** (manual QA checklist). Zero código, zero dependências, zero risco. Escopo explicitamente paralelo aos planos de código — nenhuma migração, nenhum schema.

**Linear issues:**
- [GAR-332](https://linear.app/chatgpt25/issue/GAR-332) — "Definir contrato API mobile v1 (OpenAPI/doc)" (Backlog, High, parent GAR-331).
- [GAR-368](https://linear.app/chatgpt25/issue/GAR-368) — "Checklist QA manual — login, chat, mascote, offline" (Backlog, Urgent, parent GAR-331).

**Status:** Draft v1 — 2026-04-21.

**Goal:** formalizar o contrato API mobile v1 em um OpenAPI 3.1 YAML versionado no repo + entregar uma checklist QA executável por humanos antes de cada build release. Ambas fecham gaps não-funcionais que os sub-issues de mobile Alpha deixaram abertos após o código já estar em produção.

## Scope

1. **GAR-332 — OpenAPI v1 mobile contract:** novo arquivo `docs/mobile-api-v1.yaml` em OpenAPI 3.1 cobrindo os 5 endpoints do gateway para o app:
   - `POST /auth/register` (201 / 400 / 409 / 500 / 503)
   - `POST /auth/login` (200 / 401 / 500 / 503)
   - `GET /me` (200 / 401 / 404 / 500 / 503)
   - `POST /chat` (200 / 400 / 401 / 500)
   - `GET /chat/history` (200 / 401 / 500)

   Schemas: `AuthRequest`, `AuthResponse`, `MeResponse`, `ChatRequest`, `ChatResponse`, `HistoryResponse`, `HistoryMessage`, `ErrorResponse`. `securitySchemes.bearerAuth` (JWT). Servers: `https://api.garraia.org` (prod) + `http://localhost:3888` (dev) + `http://10.0.2.2:3888` (Android emulator).

2. **GAR-368 — Manual QA checklist:** novo arquivo `docs/mobile-qa-checklist.md` com checklist executável pelo time QA antes de APK release / TestFlight cut. Áreas cobertas:
   - Auth (register, login, logout, token persistence)
   - Chat (send, receive, history hydration, empty state)
   - Mascot (idle, thinking, talking, happy transitions)
   - Offline (queue enqueue, SnackBar, flush on resume)
   - Error handling (invalid credentials, network down, server 500)
   - Performance (cold start, message send p95, scroll smoothness)
   - Security (token never in logs, HTTPS only in prod, session expiry)

## Non-scope

- Implementação de mudanças de API: este plano apenas documenta o estado existente. Qualquer alteração de request/response schema vai para um plano separado.
- Automação da QA checklist (Playwright, Maestro, Appium): fora de scope; a checklist é manual por design.
- Geração de código client a partir do OpenAPI: fora de scope.
- Swagger UI endpoint no gateway: `/docs` já existe para `/v1/*` workspace endpoints (plan 0015); adicionar uma segunda UI para `/auth/*` + `/chat/*` mobile vira follow-up se necessário.

## Tech stack

- OpenAPI 3.1 (YAML).
- Markdown puro.
- **Nenhuma dependência nova.**

## File structure

| File | Action | Responsibility |
|---|---|---|
| `docs/mobile-api-v1.yaml` | Create | OpenAPI 3.1 spec — 5 endpoints + 8 schemas |
| `docs/mobile-qa-checklist.md` | Create | Manual QA checklist — 7 seções, ~40 items |
| `plans/0027-mobile-alpha-docs.md` | Create | This plan file |
| `plans/README.md` | Modify | Index entry 0027 |

## Acceptance criteria

1. `docs/mobile-api-v1.yaml` valida como OpenAPI 3.1 (basicamente sintaxe YAML + chaves mínimas presentes — a validação formal via `openapi-cli lint` é follow-up).
2. Schemas do YAML batem 1:1 com `RegisterRequest` / `LoginRequest` / `AuthResponse` / `MeResponse` / `ChatRequest` / `ChatResponse` / `HistoryResponse` em `crates/garraia-gateway/src/mobile_auth.rs` e `mobile_chat.rs`.
3. Status codes documentados no YAML batem com os handlers reais (e.g. register retorna 201 Created, login retorna 200 OK, não trocados).
4. `docs/mobile-qa-checklist.md` tem pelo menos 30 items acionáveis em markdown checkbox (`- [ ]`).
5. Plans index atualizado.

## Rollback plan

Pura deleção dos 2 arquivos. Zero dependentes, zero breakage.

## Open questions

Nenhuma. Os endpoints existentes são a fonte de verdade — o YAML e a checklist apenas formalizam o que já está em produção.

## Review plan

Nenhum code review necessário (zero código). Doc-writer agent pode revisar o YAML + markdown por clareza/consistência. Security-auditor não é necessário (nenhum secret, nenhuma lógica).
