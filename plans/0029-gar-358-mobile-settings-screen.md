# Plan 0029 — GAR-358: Mobile Settings screen (account info + logout)

> **Narrow Flutter slice** fechando a última issue funcional pendente do EPIC GAR-331 (Mobile Alpha) que requer código. Zero Rust, zero backend, zero schema. Escopo cirúrgico.

**Linear issue:** [GAR-358](https://linear.app/chatgpt25/issue/GAR-358) — "GAR-MOB-037: Tela de Settings — logout e info da conta" (Backlog, Medium, parent GAR-331).

**Status:** Draft v1 — 2026-04-21.

## Goal

Adicionar uma tela dedicada `SettingsScreen` em `apps/garraia-mobile/` que cumpre o que GAR-358 pede: logout + info da conta. Mantém o atalho existente de logout no popup menu da AppBar do chat (quick-access) e adiciona um segundo entry point via `Icons.settings_rounded` que leva para a tela dedicada com confirmação modal + dados do usuário via `/me`.

## Scope

1. **Novo arquivo `apps/garraia-mobile/lib/screens/settings_screen.dart`** (~220 LOC):
   - `SettingsScreen` `ConsumerWidget` lê `authStateProvider`.
   - Account card com 3 linhas: `Email` (value literal), `ID` (UUID encurtado via `_shortId` — `xxxxxxxx…yyyy`), `Cadastro` (ISO-8601 trimmed ao dia via `_formatDate`).
   - Botão logout `FilledButton.tonalIcon` estilizado com `errorContainer` color scheme.
   - Confirmação modal `AlertDialog` — cancel/confirm — antes de chamar `AuthState.logout()`.
   - Version footer (`v0.1.0 (Alpha)`).
   - `_ErrorState` widget para falha no `/me` (raro mas possível).
   - `_InfoRow` helper compartilhado.

2. **Route em `lib/router/app_router.dart`:** novo `GoRoute(path: '/settings', builder: ...)` ao lado do `/pair` existente.

3. **Entry point em `lib/screens/chat_screen.dart`:** novo `IconButton(Icons.settings_rounded)` na AppBar `actions[]` entre o botão de pareamento e o popup menu de logout existente. O popup de logout NÃO é removido — é preservado como atalho rápido.

4. **Widget test em `apps/garraia-mobile/test/settings_screen_test.dart`:** stub `_StubApiService implements ApiService` com canned `MeResult`, 2 cenários — (a) render de account info; (b) logout button → dialog → confirm calls `api.logout()`; cancel não chama. Não roda em CI hoje (sem Flutter CI job) mas documenta contrato + serve para execução manual local via `flutter test`.

## Non-scope

- Theme preferences (dark/light toggle): fora de scope — o app já é dark-only.
- Notification preferences: fora de scope; não há backend para armazenar preferences.
- Biometric toggle: já existe via `biometric_service.dart`; não é surfaced nesta tela no MVP.
- Privacy policy / Terms of service links: fora de scope; documentos legais ainda não existem.
- Language switcher: app é PT-BR only no Alpha.
- Delete account: fora de scope — depende de GAR-400 (LGPD endpoints de export/delete).
- Flutter CI job: fora de scope; o widget test vive no repo mas não roda em GitHub Actions (nenhum workflow atual executa `flutter test`).

## Tech stack

- `flutter_riverpod` `ConsumerWidget` + `ref.watch(authStateProvider)`.
- `go_router` `context.push('/settings')` + existing redirect logic para logout.
- `flutter/material.dart` Material 3 — reusa o `ColorScheme` dark do `main.dart`.
- Nenhuma nova dependência em `pubspec.yaml`.

## File structure

| File | Action | Responsibility |
|---|---|---|
| `apps/garraia-mobile/lib/screens/settings_screen.dart` | Create | Settings screen widget — ~220 LOC |
| `apps/garraia-mobile/lib/router/app_router.dart` | Modify | Import + new `/settings` route |
| `apps/garraia-mobile/lib/screens/chat_screen.dart` | Modify | AppBar entry point (IconButton.settings_rounded) |
| `apps/garraia-mobile/test/settings_screen_test.dart` | Create | Widget tests (stub ApiService, 2 scenarios) |
| `plans/0029-gar-358-mobile-settings-screen.md` | Create | This plan file |
| `plans/README.md` | Modify | Index entry 0029 |

## Design invariants

1. **Zero Rust change** — mobile slice, zero blast radius no gateway.
2. **Quick-access logout preserved** — popup menu do AppBar de chat continua funcional. Settings screen é entrada *adicional*, não substituição.
3. **Confirmation required** — novo logout tem modal de confirmação (dialog). Popup menu continua imediato (trade-off: popup = quick, dialog = dedicated screen).
4. **No network beyond existing** — todas as chamadas de backend (`/me`, logout local) já existem e são consumidas por `AuthState`.
5. **Route redirect works** — após `logout()` o `authStateProvider` resolve para `null`; o `appRouter` redirect logic já manda para `/login`. Chamada explícita `context.go('/login')` é belt-and-suspenders.

## Acceptance criteria

1. `SettingsScreen` renderiza email, user_id encurtado, e data de cadastro.
2. Botão "Sair da conta" abre dialog de confirmação. Cancel → sem logout. Confirm → `AuthState.logout()` called, router redireciona para `/login`.
3. AppBar do chat tem novo `Icons.settings_rounded` que navega para `/settings`.
4. Route `/settings` registrada em `app_router.dart` e coberta pelo redirect de auth (só acessível logado).
5. Widget test roda localmente com `flutter test` (documentação + execução manual).
6. Existing `flutter test` smoke (`widget_test.dart`) continua passing.

## Rollback plan

Reversível. Pura deleção de 1 arquivo novo (`settings_screen.dart`) + 1 arquivo de teste + reversão de 3 edits (1 import + 1 route + 1 IconButton).

## Open questions

Nenhuma. Infraestrutura toda existente (`AuthState.logout()`, `apiServiceProvider`, `appRouterProvider` redirect).

## Review plan

Dado que é 100% Flutter UI sem mudança de contrato/API, `code-reviewer` agent é suficiente. Sem security-auditor review (nenhum secret, nenhuma crypto, nenhum endpoint novo).
