# TODO

Status operacional do backlog do GarraIA/GarraRUST. Este arquivo complementa
`ROADMAP.md`: o roadmap guarda a direção de produto; este TODO registra o que
foi concluído, o que ficou parcial ou adiado, decisões tomadas e próximos passos
curtos para a próxima sessão autônoma.

**Atualizado:** 2026-09-02 (America/New_York)

> O Linear foi descontinuado em 2026-08-18; o planejamento vive no tracker
> interno. Menções a "Done in Linear", "In Review" ou "issues Linear" nas seções
> históricas abaixo são registro da época, não estado atual. IDs `GAR-xxx`
> permanecem como identificadores históricos.

## Concluído em 2026-09-02 (Fase 0 Garra Mobile + fix Dependabot Updates)

- **Fix dos runs falhando do Dependabot Updates (#281-283)**: entradas
  `docker`/`docker-compose` apontando para `/deploy/docker` (que nunca
  existiu no repo público — layout do privado) removidas do
  `.github/dependabot.yml`; bumps `futures 0.3.34` + `toml 1.1.5`
  pré-aplicados à mão para contornar o bug upstream dependabot-core
  [#12487](https://github.com/dependabot/dependabot-core/pull/12487)
  (VersionResolver vs LockfileUpdater em workspaces).
- **Fase 0 Garra Mobile Local ([ADR 0016](docs/adr/0016-mobile-termux-local-first.md))**
  na v0.3.6: job `build-android-arm64` no `release.yml` (bionic
  `aarch64-linux-android` via cargo-ndk; openssl vendido; asset
  `garraia-android-aarch64` aditivo, regra 15); branch Termux no
  `install.sh` + suíte `tests/install_sh/detect_platform.sh`; braço
  `("android", "aarch64")` no `garra update`; `garra doctor` novo
  subcomando (sysexits 0/2/65, `--json`/`--strict`, probe SSRF-vetado);
  `llamacpp` keyless wired no CLI (chat/ask/set-model) e no bootstrap do
  gateway (lockstep `provider_key_env`).
- **Fix shellcheck herdado de #904**: a prosa "# shellcheck list of the
  installer job" em `tests/install_sh/detect_platform.sh` era parseada
  como diretiva (SC1073/SC1072) e derrubava o job Install.sh shellcheck
  no `main` desde 2026-09-02T17:01Z.

### Próximos passos (Fase 0 → v1)

1. **Release v0.3.6** — validar o job `build-android-arm64` via
   `workflow_dispatch` antes da tag; tag + GH release com o asset android +
   `.sha256`; **publish manual do site na Lovable é passo do dono** (regra
   17 — o merge não muda o que garraia.org serve).
2. **Aparelho real** (dono): Termux → `curl install.sh | bash` →
   `garraia doctor` → `garraia chat` na cloud/LAN; depois da tag,
   `garraia update` dentro do Termux.
3. **v1 — companion Kotlin/Compose** (ADR 0016): RUN_COMMAND +
   `allow-external-apps`, cap 100KB no PENDING_INTENT, detecção do fork
   Play, foreground service.

## Concluído em 2026-09-02 (auditoria docs ↔ código)

- **Reconciliação de `README.md`, `ROADMAP.md` e `TODO.md` com `main@v0.3.5`**:
  662 afirmações verificadas contra código, CI e API do GitHub; 300 corrigidas.
  Destaques: MSRV 1.94 → 1.95 no badge/Quick Start; `--features tls` não existe
  no binário (documentado `cargo build -p garraia --features garraia-gateway/tls`);
  bloco `embeddings:`/`memory.auto_extract` reescrito na forma real do
  `AppConfig`; transports MCP aceitos no `config.yml` (`stdio`/`http`);
  matriz de assets ganha os bundles do Garra Desktop; `POST /v1/messages` +
  `garra agents setup` (plan 0360 / ADR 0014) documentados pela primeira vez;
  contagens unificadas (22 crates, 33 migrations, 37 tabelas, 32 sob FORCE RLS,
  6 binários CLI, wasmtime 48). ROADMAP: ~50 checkboxes `[ ]` já entregues
  marcados com evidência; contradições GAR-410 / GAR-603 / GAR-641 (9/10)
  resolvidas; §7 e cabeçalho atualizados; nova §4.4 (canais sem wiring).
- **Docs irmãos sincronizados** no mesmo PR: `README.pt-BR.md`, `CLAUDE.md`,
  `CHANGELOG.md` ([0.3.4] e [0.3.5] completados), `docs/installation.md`
  (MSRV + receita Docker que fazia `ENTRYPOINT ["garraia"]` de um binário
  `garra`), `docs/configuration.md`/`docs/memory.md`/`docs/hardening-gateway.md`
  (chaves inventadas `auto_extract`/`extraction_interval`/`max_facts`,
  `--features tls`), `CONTRIBUTING.md` (MSRV, links `linear.app`),
  `plans/README.md` (linha 0362; 0358 mergeado; 0022-0024 reapontados),
  `docs/adr/README.md` (data do 0009), `.github/dependabot.yml` (comentário
  MSRV).

## Concluído em 2026-09-01 (release v0.3.5 — Garra Desktop Linux + Chat Bar)

- **Release v0.3.5 publicada** (plan 0362, Amendment 2026-09-01 do ADR 0015):
  Release run #13 via `workflow_dispatch` com os 10 jobs verdes (incl. os
  best-effort `package-linux`, `build-linux-desktop`, `build-windows-aarch64`,
  `build-windows-installer`), Deploy #12 manual e `install-endpoints.yml` #7
  verde no mesmo dia. 47 assets, incluindo
  `garraia-desktop-linux-x86_64.{deb,AppImage}`.
- **Garra Desktop para Linux** (`.deb` + AppImage x86_64 via bundler Tauri):
  job best-effort `build-linux-desktop` no `release.yml` + gate
  `build-linux-bundles` no `desktop.yml`, `scripts/build-desktop-linux.sh`
  (irmão do `build-installer.ps1`). O `.deb` declara
  `Provides/Conflicts/Replaces: garraia` porque instala o sidecar em
  `/usr/bin/garraia`. Docs em `docs/installation.md` §"Garra Desktop on Linux".
  Antes disso, quem baixava o AppImage esperando o papagaio recebia a CLI.
- **Garra Chat Bar**: barra flutuante (`chat_bar.rs`, `ui/chat-bar.html`)
  substitui o quick-chat (`quick_chat.rs`/`quick-chat.html` removidos);
  `Ctrl+Space`, item da bandeja, posição/visibilidade persistidas em
  `chat-bar.json`; cliente WebSocket compartilhado em `ui/ws.js`. Uma única
  sessão `parrot-desktop` com o papagaio.
- **Streaming no `/ws/parrot`**: frames `{"type":"chunk"}` por delta + o
  `{"type":"response"}` final autoritativo (`parrot_ws.rs`).
- **Papagaio visível de novo**: `ui/assets/parrot-sprite.png` estava no
  `.gitignore` (todo MSI da v0.3.4 embarcava um overlay transparente que ainda
  comia cliques); commitado + job `assert-ui-assets` no CI.
- **Deps de segurança**: argon2 0.6 + pbkdf2 0.13 + password-hash 0.6.1 em
  `garraia-auth` (GAR-669 slice 3, fecha o Dependabot #430; registrado em
  `docs/security/dependabot-status.md`).
- **Fixes menores**: `.deb` da CLI volta a carregar LICENSE/README (#897);
  `install-endpoints.yml` sobrevive a errexit e imprime o código HTTP (#892);
  `tests/install_ps1/platform.ps1` em ASCII puro para o PSScriptAnalyzer;
  `set -euo pipefail` no `Get version` do `package-linux`; bumps do dependabot
  (#894/#895).

## Concluído em 2026-08-31 (release v0.3.4 + pacotes Linux + Windows ARM64)

- **Release v0.3.4 publicada** (plan 0361, ADR 0015): `[Unreleased]` fundido na
  seção `[0.3.4]` do CHANGELOG (data 2026-08-31). Primeira release a exercitar
  a matriz do plan 0359 (archives, MSI/NSIS, `install.ps1`); 43 assets.
  Publicação: Release #12 via `workflow_dispatch` (`version=v0.3.4`) + Deploy
  #11 manual (tag via `GITHUB_TOKEN` não dispara workflows de tag-push —
  documentado no runbook §2/§4); `install-endpoints.yml` #6 verde em seguida.
- **Pacotes Linux `.deb`/`.rpm`/AppImage** no novo job best-effort
  `package-linux` do `release.yml`: nfpm 2.47.0 + appimagetool 1.9.1, ambos
  pinados por versão + SHA-256; empacotam os binários já buildados
  (`/usr/bin/garraia`; `.deb` exige `libc6 >= 2.35`); nomes de asset sem
  versão. Config em `packaging/`. ADR 0015 registra a decisão de toolchain
  (nfpm vs cargo-deb/fpm; appimagetool 1.9.1 porque o AppImageKit 13 foi
  marcado obsoleto pelo upstream).
- **Windows ARM64 nativo best-effort**: job `build-windows-aarch64`
  (`aarch64-pc-windows-msvc`; wasmtime é Tier 3 nesse alvo — falha não bloqueia
  a release), `garra update` com o arm `(windows, aarch64)` + primeiro módulo
  de testes do `update.rs`, `install.ps1` instalando o nativo (pinar
  `GARRAIA_VERSION < v0.3.4` em ARM64 dá 404 — caveat documentado).
- **Paridade de testes dos instaladores** (regra 16): nova suíte
  `tests/install_ps1/platform.ps1` (registrada no `installer-powershell` do
  ci.yml) e caso mixed de `SHA256SUMS` com `.deb`/`.rpm`/`.AppImage` nos
  `checksum_format` **dos dois lados** (o sh só cobria o irmão `.sha256`).
- **Docs sincronizadas**: READMEs (incl. a linha estale do pt-BR que dizia
  "MSI/APK até a v0.2.1"), `docs/installation.md` (tabela + seção Linux
  packages + caveat `garra update` sob pacote root-owned), `docs/releasing.md`
  (matriz de assets + verificação + Deploy manual), wiki, ROADMAP §4.1
  (restantes: só DMG notarizado + AppImage aarch64), CLAUDE.md regra 15
  (lista aditiva ganha `.deb`/`.rpm`/`.AppImage`); typo do ADR 0014 (plan
  0360, não 0361) corrigido.

## Concluído em 2026-08-27..30 (v0.3.3 + plans 0355-0360)

- **Release v0.3.3** (2026-08-27): primeira com `install.sh` como asset de
  release (13 assets: 5 binários crus + `install.sh` + `SHA256SUMS`).
- **wasmtime 47 → 48 + MSRV 1.94 → 1.95** (PR #865, 2026-08-28): migra a API
  WASI; `ci.yml` ganha o job `MSRV check (1.95)`. *(Badges e docs seguiram em
  1.94 até 2026-09-02.)*
- **sqlx 0.9** (PR #868): `SqlSafeStr` enforçado pelo compilador;
  `AssertSqlSafe` só em alvos de teste (regra 5 do CLAUDE.md).
- **Guard SSRF centralizado** em `garraia_common::ssrf` (`vet_url` +
  `pinned_client`; PRs #869/#883/#889; regra 14 do CLAUDE.md).
- **`garra chat`**: spinner de atividade + `Ctrl+C` cancela o turno em vez de
  matar o processo (PR #884); default Ollama `qwen3.8:latest` (plan 0357).
- **ADR 0013** (plan 0355): motor de recorrência RFC 5545 em
  `garraia-workspace::recurrence` + `tasks_recurrence_worker` (migration 033)
  e lifecycle dos servidores MCP.
- **Interop Hermes ↔ Garra via MCP** (PR #859); workflow
  `codeql-apply-dismissals` (PR #871); `wiki/` sincronizado com o GitHub Wiki
  (`wiki-sync.yml`); lopdf 0.44 (plan 0356) e rmcp 1.7 → 2.2 (plan 0358, PR
  #876) destravados em 2026-08-29.
- **`garra agents setup|status|link|rollback|web`** (plan 0360, front-end do
  AgentDeck para provisionar GarraIA/OpenClaw/Hermes/Claude Code com um mesmo
  provedor+modelo) + **shim Anthropic-compatible `POST /v1/messages`**
  (ADR 0014, 2026-08-30) para o Claude Code apontar para o gateway. *(Ficou fora
  do README/ROADMAP/CHANGELOG até 2026-09-02.)*
- **`install.ps1`** (`irm | iex`) + archives `.tar.gz`/`.zip` + MSI/NSIS via
  `build-installer.ps1` (plan 0359, PR #878, 2026-08-30).

## Concluído em 2026-08-18 (cleanup total + v0.3.1/v0.3.2)

- **Repo zerado e verificado**: 0 issues abertas, 0 PRs abertos, só a branch
  `main` (ainda verdadeiro em 2026-09-02). Trigger `garra-routine-trigger.yml`
  **desativado** no GitHub (`gh workflow disable`, reversível; o arquivo ainda
  tem o `schedule` — o estado vive só no GitHub) — era ele que mantinha 1 issue
  de tracking rolante.
- **Linear descontinuado**: planejamento interno migrou para o tracker interno.
  README/ROADMAP/CLAUDE.md atualizados (links mortos removidos dos docs de topo;
  `plans/README.md` e `docs/adr/README.md` mantêm as URLs antigas como índice
  histórico; IDs `GAR-xxx` preservados).
- **Fix crítico LGPD** (plan 0354, PR #843): `POST /v1/me/anonymize` retornava
  **500 em toda chamada** desde junho (coluna fantasma `user_identities.login`
  portada do repo interno sem o schema — regra 14). Agora anonimiza
  `provider_sub` + `users.email` + `group_invites.invited_email` com token
  determinístico de UUID completo. Revisado por security-auditor +
  code-reviewer. Era a causa do cargo-mutants vermelho desde 2026-04-28.
- **Gap estrutural de CI fechado** (PR #843): job `auth-integration` roda os 15
  binários de integração do garraia-auth (matriz RLS 81 cenários) em todo PR —
  antes o `cargo test --workspace` os pulava silenciosamente
  (`required-features`). Promover a required check: tracker interno #164.
- **Deps consolidadas** (PR #844): jsonwebtoken 11, validator 0.21, base64
  0.23, serial_test 4, itertools 0.15, uuid/tauri, e **wasmtime 47 em par**
  (grupo dependabot novo evita o par quebrado; 47 → 48 + MSRV 1.95 vieram no
  PR #865 em 2026-08-28). lopdf destravado em 2026-08-29 (0.42→0.44 com a
  feature `time` off, plan 0356) e **rmcp destravado no mesmo dia** (1.7→2.2,
  `ContentBlock`, plan 0358) — do #163 resta só o salto 3.x. h2 0.4.16
  (RUSTSEC-2026-0258).
- **CI higiene** (PR #842): `timeout-minutes` em e2e/playwright (2 runs de 6h
  presos no download do Chromium em 2026-08-17), `Cross.toml` para o build
  ARM64.
- **Releases**: **v0.3.1** (fix LGPD + deps, 4 binários) e **v0.3.2** (PR #846:
  cross da git + **sqlx native-tls → rustls** `tls-rustls-ring-native-roots`) —
  **primeira release com `garraia-linux-aarch64`** (5 binários).
- Issue #827 (domínio `get.garraia.cloud`) fechada *not planned* — 429 mitigado
  por release asset + jsDelivr (#826); RUSTSEC do `lru` migrado para o tracker
  interno #162 (re-triage ≤ 2026-11-14).

## Histórico (maio–junho 2026, sessões `garra-routine`)

Registro das entregas das sessões autônomas anteriores. Todos os PRs abaixo
foram mergeados (ver `plans/README.md` para hash e data de cada um).

### 2026-06-10

- GAR-835 / plan 0297 — Docs Tier 2 scaffold: migration 026 + POST/GET /v1/groups/{group_id}/doc-pages:
  - Migration `026_doc_pages.sql`: `doc_pages` table with FORCE RLS, NULLIF fail-closed group
    isolation policy, `GRANT SELECT/INSERT/UPDATE` to `garraia_app`, keyset + parent indexes.
  - `WorkspaceAuditAction::DocPageCreated` → `"doc_page.created"` added to `garraia-auth`.
  - `docs.rs`: `CreateDocPageRequest`, `DocPageResponse`, `DocPageSummary`, `ListDocPagesResponse`,
    `ListDocPagesQuery`; `create_doc_page` (POST 201, authz DocsWrite) and `list_doc_pages`
    (GET, cursor-keyset, optional `parent_page_id` filter, authz DocsRead); 6 unit tests pass.
  - Routes wired in all 3 `mod.rs` branches (full / auth-stub / no-auth stub).
  - `openapi.rs`: paths + schemas registered.
  - ROADMAP §3.8 Tier 2: `doc_pages` schema + 2 API endpoints marked ✅.
  - PR #706 squash-merged 2026-06-10 (`54f88bc`) — 20/20 CI green.

### 2026-06-05..06

- GAR-806 / plan 0269 — GET /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}:
  - `get_task_comment` handler in `comments.rs`: validates group_id, TasksRead check,
    SET LOCAL both RLS configs, single query (`task_comments WHERE id=$1 AND task_id=$2 AND deleted_at IS NULL`),
    returns `CommentResponse`; 404 for deleted/cross-group/unknown (no existence leak).
  - Route wired as `get(tasks::get_task_comment)` alongside delete+patch in all 3 `mod.rs` branches.
  - `super::tasks::comments::get_task_comment` added to `openapi.rs` paths list.
  - ROADMAP §3.8 and §3.4 updated with GET single task-label (GAR-802) and GET single comment (GAR-806).
  - 6 unit tests: serializes all fields, nil author_user_id → null, nil edited_at → null,
    edited_at UTC ISO-8601 Z, nil UUID round-trip, task_id preserved.
  - `cargo clippy --workspace` clean (0 warnings). 12 total comments tests pass.
  - Branch `routine/202506061830-get-task-comment`; squash-merged PR #655 (`760dfd8`) 2026-06-06.

- GAR-800 / plan 0266 — PATCH /v1/groups/{group_id}/task-labels/{label_id}:
  - `PatchTaskLabelRequest { name, color }` + `patch_task_label` handler in `labels.rs`.
  - COALESCE UPDATE (at least one field required → 400, 404 on 0 rows, 409 on duplicate name).
  - `WorkspaceAuditAction::TaskLabelEdited` added to `audit_workspace.rs` (PII-safe).
  - Routes wired in all 3 `mod.rs` branches (full / auth-only stub / no-auth stub).
  - OpenAPI: `super::tasks::labels::patch_task_label` path + `PatchTaskLabelRequest` schema.
  - 6 unit tests: name-only / color-only / both-absent roundtrip, hex valid/invalid, response nil-UUID.
  - `cargo clippy --workspace` clean; 634 unit tests pass. Branch: `routine/202506060020-task-label-patch`.
  - Merged PR #649 (`28d052a`).

- GAR-798 / plan 0265 — GET /v1/threads/{thread_id}:
  - `get_thread` handler in `chats.rs` (before `patch_thread`): validates group_id, ChatsRead check,
    SET LOCAL both RLS configs, single JOIN query (`message_threads JOIN chats WHERE group_id = $2`),
    returns `ThreadDetailResponse`; 404 for cross-group or unknown threads (no existence leak).
  - Route wired as `get(chats::get_thread).patch(chats::patch_thread)` in all 3 `mod.rs` branches.
  - `super::chats::get_thread` added to `openapi.rs` paths list.
  - Removed now-unused standalone `patch` import from `mod.rs`.
  - 6 unit tests: serializes all fields, nil title → null, nil created_by → null,
    unresolved → null resolved_at, resolved → UTC ISO-8601 Z timestamp, nil UUID round-trip.
  - `cargo clippy --workspace` clean (0 warnings). Branch: `routine/202506051820-get-thread`.
  - Merged PR #646 (`7913904`).

- PR #643 (docs/mark-plan-0263-merged) — merged (20/20 CI green); GAR-794 fechado.

- GAR-795 / plan 0264 — PATCH /v1/groups/{group_id}/tasks/{task_id}/comments/{comment_id}:
  - `TaskCommentEdited` variant added to `WorkspaceAuditAction` in `garraia-auth`.
  - `EditCommentRequest` + `EditedCommentResponse` types in `comments.rs`.
  - `patch_task_comment` handler: sender-only (404 for other authors), body_md 1-50k validated,
    `edited_at = now()` in same UPDATE, audit `body_len` only (no PII).
  - Route wired in all 3 `mod.rs` branches; OpenAPI path + components registered.
  - 6 unit tests pass. `cargo clippy --workspace` green (622+6 tests, 0 warnings).
  - Closes CRUD gap: POST/GET/DELETE were GAR-520; PATCH was missing.
  - Squash-merged PR #644 (`6974812`) 2026-06-05 — 20/20 CI green.

- GAR-794 / plan 0263 — POST /v1/me/invites/{invite_id}/accept:
  - `accept_my_invite` handler in `me.rs`: UUID-based authenticated accept.
  - Atomic tx: UPDATE group_invites (with all terminal guards in WHERE) + INSERT group_members + audit InviteAccepted.
  - 410 (expired) distinguished from 404 via follow-up SELECT when UPDATE returns None.
  - 409 (already member) via SQLSTATE 23505 on group_members INSERT.
  - Route registered in all 3 mod.rs branches; OpenAPI path + AcceptMyInviteResponse schema registered.
  - 6 unit tests covering: serialization, no-PII fields, role variants, nil UUID round-trip, PendingInviteSummary excludes accepted_at, exactly-3-fields shape.
  - Completes the invite lifecycle: list (GAR-777) → accept (GAR-794) / decline (GAR-783); token-based accept (plan 0019) unchanged.

### 2026-06-02..03

- GAR-777 / plan 0255 — GET /v1/me/invites (caller-scoped pending group invites inbox):
  - Merged PR #621 (`762d63c`) after CI went 20/20 green; bookkeeping PR #624 (docs/mark-plan-0255-merged) merged.

- GAR-780 / plan 0257 — GET + DELETE /v1/groups/{id}/invites/{invite_id} (invite revocation):
  - Migration 024: `revoked_at` + `revoked_by` columns on `group_invites`; recreated partial unique index to exclude revoked rows (enables re-invite after revocation).
  - `WorkspaceAuditAction::InviteRevoked` variant + `"invite.revoked"` string + test assertion.
  - `list_invites` WHERE updated: `AND revoked_at IS NULL`.
  - `get_invite` handler: returns `InviteSummary` (404 if not found/accepted/revoked).
  - `revoke_invite` handler: `UPDATE SET revoked_at = now()`, emits `InviteRevoked` audit event, 204 No Content (404 if already accepted/revoked).
  - Routes in all 3 `mod.rs` branches. OpenAPI paths + schemas (`InviteSummary`, `ListInvitesResponse`).
  - 5 unit tests (serialization, cursor, role round-trip, no `revoked_at` in response).
  - Squash-merged PR #625 (`46a8658`) 2026-06-03 — 20/20 CI green; bookkeeping PR (docs/mark-plan-0257-merged) merged.

### 2026-05-31..06-01

- GAR-767 / plan 0246 — GET /v1/me/files (caller-scoped uploaded-files inbox):
  - `ListMyFilesQuery` struct with `group_id` (required), `after`, `limit`, `folder_id` (optional).
  - `MyFileSummary` fields: `id`, `group_id`, `name`, `mime_type`, `size_bytes`, `folder_id` (skip_if_none), `created_at`, `updated_at` (skip_if_none).
  - `MyFilesResponse` with `items` + `next_cursor` (skip_serializing_if None).
  - 4-branch query (cursor × folder_id filter), keyset on `(files.created_at DESC, files.id DESC)`.
  - Route `.route("/v1/me/files", get(me::list_my_files))` registered in all 3 `mod.rs` branches.
  - OpenAPI annotation + component registration in `openapi.rs`.
  - 8 new unit tests (serialization, limit clamp, folder filter, cursor, large size).
  - Branch `routine/202506010015-me-files-inbox`; merged 2026-06-01 via PR #603.

- GAR-765 / plan 0245 — GET /v1/me/chats (caller-scoped chat membership inbox):
  - `ListMyChatsQuery` struct with `group_id` (required), `after`, `limit`, `type` (optional).
  - `ChatMembershipSummary` fields: `chat_id`, `group_id`, `name`, `type`, `role`, `joined_at`, `muted`, `last_read_at`.
  - `MyChatsMembershipResponse` with `items` + `next_cursor` (skip_serializing_if None).
  - 4-branch query (cursor × type filter), keyset on `(cm.joined_at DESC, cm.chat_id DESC)`.
  - Route `.route("/v1/me/chats", get(me::list_my_chats))` registered in `mod.rs`.
  - OpenAPI annotation + component registration in `openapi.rs`.
  - 8 new unit tests (serialization, type filter validation, cursor, muted/last_read_at).
  - Branch `routine/202605311818-me-chats-inbox`; merged 2026-05-31 via PR #601 (`2bf1f5b`).

### 2026-05-25..29

- GAR-733 / plan 0215 — Search slice 14 (`types=groups` group name FTS):
  - `SearchResultType::Group` variant; `include_groups: bool` in `ValidatedSearch`.
  - `parse_and_validate`: recognizes `"groups"`, rejects non-user scope with 400.
  - `GroupSearchRow` struct + `fetch_groups()` async (runtime `to_tsvector('simple', g.name)`).
  - Handler block: `if validated.include_groups { ... }` mapping to `SearchResult`.
  - 6 unit tests (scope guards + multi-type combos). No migration needed — FORCE RLS migration 018.
  - PR #561 squash-merged 2026-05-29 (`1bb2f10`).

- GAR-705 / plan 0187 — Health run 30: all surfaces clean, priority (i). PR #508 squash-merged (`ef040ad`).

- GAR-467 / plan 0188 — Q6.5 Mutation Testing — audit_event observability coverage:
  - Added `count_audit_action(...) == 1` assertion to all 7 terminals of `verify_credential_with_ctx`.
  - Added `row.ip.is_some()` assertion to all non-argon2id terminals (T3–8).
  - New test `null_stored_hash_emits_unknown_hash_audit`: seeds user with NULL password_hash,
    asserts `Err(UnknownHashFormat)` + 1 audit row committed + ip populated.
  - Total: 11 integration tests (was 10). Tests-only PR, no production code changes.
  - PR #509 squash-merged (`a1b0fdd`).

- GAR-702 / plan 0184 — Health run 28: all surfaces clean, priority (i). PR #504 squash-merged.

- GAR-703 / plan 0185 — Search slice 5 (`types=files` file name FTS):
  - `SearchResultType::File` variant added.
  - `include_files: bool` in `ValidatedSearch`.
  - `parse_and_validate`: recognizes `"files"`, rejects non-group scope.
  - `FileSearchRow` struct + `fetch_files()` async function (runtime tsvector 'simple').
  - Handler: `if validated.include_files { ... }` block mapping to `SearchResult`.
  - 6 new unit tests; `unknown_type_rejected` updated to use `"tasks"` (not `"files"`).
  - ROADMAP.md + plans/README.md + TODO.md updated.
  - Branch: `routine/202605251215-search-slice5-files`, PR #505, merged `bb8c040`.

- GAR-697 / plan 0179 — Search slice 4 (`has_attachment` filter):
  - Migration 020 (`message_attachments` M:N join table, FORCE RLS via JOIN
    through messages, índice `message_attachments_message_idx` para o EXISTS
    subquery path).
  - `search.rs`: `SearchQuery.has_attachment: Option<bool>`, validação (rejeita
    quando `types` não inclui `messages`), predicado SQL EXISTS-equality trick.
  - Tests: 5 unit tests novos (slice 4 block), S18/S19/S20 integration scenarios.
  - ROADMAP.md + plans/README.md + TODO.md atualizados.
  - Branch: `routine/202605250015-search-has-attachment`; merged 2026-05-25 via PR #498 (`be8c880`).

## Parcialmente concluído

- GAR-603 Runpod Load Balancer Serverless:
  - Concluído por evidência estática/docs: `garra start` em modo HTTP,
    container bindando `0.0.0.0`, rotas `/ping` e `/health`, `PORT`/`HOST`,
    Dockerfile sem REPL, receita local Docker, settings Runpod e distinção
    queue-based vs Load Balancer.
  - Pendente: smoke Docker local e smoke público
    `https://<ENDPOINT_ID>.api.runpod.ai/ping`.
  - Pendente técnico: suporte a `PORT_HEALTH` separado quando a health port
    precisar diferir de `PORT`; hoje a documentação exige `PORT_HEALTH=PORT`.
- GAR-641 Garra Learning Agent (9/10): tudo entregue exceto GAR-646 Skill
  Retriever — `retriever.rs` é stub que retorna erro até a Fase 2.1; o CLI
  de override (`garra skills approve|lock|rollback|...`) e o módulo
  `skill_override` também são stubs (só a Web UI faz isso hoje).
- Desktop auto-updater: `tauri-plugin-updater` ligado (`check_for_updates`/
  `install_update` na bandeja) mas inerte — sem chave de assinatura e nenhum
  workflow publica `latest.json` (`docs/releasing.md` §Débito conhecido).

## Adiado com justificativa

- GAR-372 / Fase 2.1 RAG embeddings: scaffold do crate `garraia-embeddings`
  entregue (PR #396, 2026-05-18 — traits + `DeterministicProvider`); faltam o
  `PgVectorStore` real (sqlx/pgvector sobre `memory_embeddings`), o
  `MxbaiProvider` (candle, ADR 0001/0002) e o wiring em learning/agents.
  Adiado por ser o maior slice aberto da Fase 2 e pré-requisito do GAR-646.
- GAR-374 / Object storage S3-compatible validation: adiado por depender de
  MinIO/S3/R2/GCS ou CI com serviço externo configurado (o `S3Compatible`
  existe atrás da feature `storage-s3`; nenhum job de CI exercita MinIO).
- GAR-410 / CredentialVault final: adiado por ser item crítico de segurança e
  amplo. O que já existe: leitura de secrets centralizada em
  `garraia-config::auth` (plan 0046) e refactor do `admin/secrets.rs` (plan
  0133). O que falta: gateway consumindo o cofre como fonte única, rotação de
  chaves, master key via argon2id. (ROADMAP §1.1 dizia "mergeado em
  2026-05-17" — corrigido para "parcial" em 2026-09-02.)
- GAR-504 / benchmark evidence run: o run em VM x86_64 está versionado em
  `benches/agent-framework-comparison/results/2026-08-28-vm/` e alimenta a
  tabela do README; só o run de referência no droplet 1 vCPU / 1 GB segue
  adiado por depender de infra externa.
- Execução async/provider-backed das native skills GarraMaxPower: adiada para
  slice próprio após decidir o fechamento do épico GAR-492.

## Pendências abertas

- `ROADMAP.md` continha dezenas de itens `[ ]` já entregues — reconciliado em
  2026-09-02 apenas onde havia evidência clara (path:line). Itens sem evidência
  ficaram `[ ]`.
- Débitos de código encontrados na auditoria de 2026-09-02 (viram itens, não
  patches na PR de docs):
  - `sessions.db` cai em `~/.garraia/data/` (`crates/garraia-gateway/src/server.rs:232-241`)
    enquanto `memory.db` e `memoria/fatos.json` usam o config dir XDG —
    unificar via `ConfigLoader::default_config_dir()`.
  - Feature `tls` sem passthrough no binário `garraia`
    (`crates/garraia-cli/Cargo.toml`) — adicionar `tls = ["garraia-gateway/tls"]`
    como já existe para `mcp-http`; até lá o README documenta
    `--features garraia-gateway/tls`.
  - `docker-compose.turboquant.yml` monta `docs/deployment/config.turboquant.yml`,
    que não existe no repo.
  - `benches/agent-framework-comparison/results/2026-08-28-vm/README.md:12`
    cita checkout `ea06286` enquanto `environment.txt:44` registra `f34cbfa`.
  - Comentário "16 binários" em `ci.yml:437` (são 15 arquivos em
    `crates/garraia-auth/tests/`).
  - `MemoryConfig` não tem `auto_extract`/`extraction_interval`/`max_facts`
    (a extração roda em todo turno, sem knob) — as docs que prometiam essas
    chaves foram corrigidas; decidir se viram configuração real.
  - `.claude/commands/garra-routine.md` (linhas 2/16/44/79) ainda instrui
    consultar o Linear.
- `install-endpoints.yml:133-150`: remover o bloco de tolerância
  `KNOWN_PRE_PS1_TAG="v0.3.3"` e o argumento extra do probe
  `release-cdn/install.ps1` — o próprio workflow pede isso após a primeira
  release ≥ v0.3.4 (v0.3.4 e v0.3.5 já publicam `install.ps1`).
- `GAR-492`: decidir no tracker interno se o épico fecha como MVP completo ou
  se abre follow-ups (execução provider-backed das native skills, dogfood em
  bug real com relatório de review).

## Decisões tomadas

- 2026-09-02: reconciliar docs com código **só** com evidência apontável;
  manter as seções históricas de maio–junho como registro datado (com a nota
  sobre o Linear no topo) em vez de apagá-las; bugs de código achados durante
  a auditoria viram pendências aqui, não patches na PR de docs.
- 2026-05-24: marcar GAR-603 como parcialmente concluído, não totalmente
  fechado — a implementação/documentação está presente, mas falta prova
  operacional recente em Docker e Runpod público.
- 2026-05-24: criar `TODO.md` como backlog operacional, evitando sobrecarregar
  `ROADMAP.md` com detalhes de sessão.

## Próximos passos recomendados

1. Promover `Auth Integration (test-support)` a required check do ruleset
   (tracker interno #164): sem flakes conhecidos e o `cargo-mutants` semanal
   está verde desde 2026-08-24 (runs #21/#22) — manter o monitoramento.
2. Retomar o salto **rmcp 2.2→3.x** quando houver janela (tracker interno
   #163). `Cargo.lock` está em 2.2.0; o 3.x mantém
   `ContentBlock`/`Role`/`PromptMessage` e as features que usamos, mas adiciona
   módulos novos (`mrtr`, `request_state`, `service/client/`, `mcp_headers`)
   ainda não auditados. O `@dependabot ignore` de rmcp foi aplicado por
   comentário em PR, não pelo `.github/dependabot.yml`. lopdf fica em 0.44 com a
   feature `time` desligada até um release > 0.44.0 trazer o fix do upstream
   #518.
3. Re-triage do RUSTSEC-2026-0253 (`lru` via aws-sdk-s3) até 2026-11-14 —
   o ignore em `deny.toml`/`.cargo/audit.toml` expira em 2026-11-15 e ainda
   cita o owner antigo (`#812 / GAR-896`; hoje tracker interno #162).
4. Limpar o bloco `KNOWN_PRE_PS1_TAG` do `install-endpoints.yml` (ver
   Pendências) e ajustar `.claude/commands/garra-routine.md` para o tracker
   interno.
5. Desktop (ROADMAP §4.1): chave de assinatura + `latest.json` para o
   `tauri-plugin-updater`; DMG notarizado; AppImage aarch64
   (`--runtime-file` + segundo pin de runtime).
6. Fase 2.1 (GAR-372 → GAR-646): `PgVectorStore` + `MxbaiProvider` + wiring do
   Skill Retriever — único item aberto do épico GAR-641.
7. Débitos de código da auditoria (ver Pendências): começar por
   `sessions.db` no config dir e pelo passthrough `tls` no binário — ambos
   pequenos e com docs já apontando para o comportamento esperado.
8. Rodar smoke Docker GAR-603:
   `docker build -t garraia:local .`,
   `docker run --rm -p 3888:3888 garraia:local`,
   `curl -fsS http://localhost:3888/ping`,
   `curl -fsS http://localhost:3888/health`;
   depois o smoke público Runpod `curl -fsS https://<ENDPOINT_ID>.api.runpod.ai/ping`
   quando houver endpoint. Abrir follow-up para `PORT_HEALTH` separado só se o
   Runpod exigir listener distinto de `PORT`.
9. Decidir destino de GAR-492 (ver Pendências).
10. A cada release, repetir o checklist de sincronia dos docs: versão, badge
    MSRV, contagens (crates / migrations / tabelas / FORCE RLS / binários /
    wasmtime), matriz de assets do `release.yml`, bloco novo aqui e no
    cabeçalho do ROADMAP, `[Unreleased]` do CHANGELOG. Candidato a virar seção
    do `docs/releasing.md`.
