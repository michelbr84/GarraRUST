# Plan 0014 — GAR-391d: App-layer cross-group authz matrix via HTTP

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-391d](https://linear.app/chatgpt25/issue/GAR-391) — **fourth and final vertex of epic GAR-391**. Merging this plan closes the epic.

**Status:** ⏳ Draft — aprovado 2026-04-14 (Florida) pelo owner. Plano imutável após primeiro commit.

**Goal:** Materializar a matriz HTTP cross-group authz que valida sistematicamente o contrato de autorização dos 3 endpoints `/v1/*` tenant-scoped atualmente em `main` (`GET /v1/me`, `POST /v1/groups`, `GET /v1/groups/{id}`), fechando o epic GAR-391.

**Architecture:** Um único test binary `crates/garraia-gateway/tests/authz_http_matrix.rs` gated via `[[test]] required-features = ["test-helpers"]`. Um único `#[tokio::test]` que semeia 3 "actors" (alice, bob, eve), constrói um `Vec<MatrixCase>` estático com 15 casos, e itera executando `oneshot` em cada um contra a harness compartilhada. Segue o padrão de M3/M4 (bundled scenarios em uma única função) para evitar o sqlx runtime-teardown race documentado no commit `4f8be37`.

**Tech Stack:** Axum 0.8, `tower::ServiceExt::oneshot`, `serde_json`, `common::Harness` + `common::fixtures::*` (tudo já existente). **Zero código de produção novo.** Zero nova migration. Zero nova dependência.

**Numeração:** ocupa o slot **`0014`** que esteve reservado para GAR-391d desde o encerramento do plan 0013 (path C). Os plans 0015/0016 foram escritos antes de 0014 sair da reserva — visualmente fora de ordem mas fiel à intenção histórica.

**Out of scope (não entra nesta slice):**

- Variantes de role além de `owner` (`admin`, `member`, `guest`, `child`) — seed helper seria necessário mas owner já valida o eixo de membership vs non-membership, que é o core de cross-group authz
- Endpoints que não existem em `main` pós-M4 (chats, messages, memory, tasks, files, invites, members role change, PATCH /v1/groups) — só aparecem em slices futuras de Fase 3.4
- ADR 0005 amendment documentando fechamento de GAR-391 — owner deferiu para pós-merge
- Itens M5 deferidos de plan 0016 (admin_url accessor, `exec_with_tenant`, URL parsing, `test-support` rename, `/docs/{*path}` wildcard, get_group transactional wrap) — nenhum deles bloqueia esta matriz

**Rollback plan:** Totalmente aditivo. Todo commit é reversível via `git revert`. Não há:
- migração nova
- endpoint novo
- env var nova
- alteração em handlers de produção
- alteração no router principal
- alteração em `CLAUDE.md`, `.env.example`, `.gitignore`

Se a matriz precisar ser recolhida por qualquer razão, `git revert` do commit que cria `tests/authz_http_matrix.rs` + `git revert` do commit que adiciona `seed_user_without_group` restaura o estado anterior. `ROADMAP.md` não é tocado porque GAR-391d não é um endpoint novo — é validação de contratos existentes.

**Pré-condições já satisfeitas (verificadas empiricamente em `main` at `b72cc1b`):**

| Pré-requisito | Caminho | Status |
|---|---|---|
| Harness testcontainer pgvector + 3 typed pools | `crates/garraia-gateway/tests/common/mod.rs` | ✅ M2 |
| `seed_user_with_group(&h, email) -> (Uuid, Uuid, String)` | `tests/common/fixtures.rs` | ✅ M3 |
| `harness_get(path)` helper com `ConnectInfo` injection | `tests/common/mod.rs` | ✅ M2 |
| `test-helpers` feature + `build_router_for_test` | `garraia-gateway/Cargo.toml` + `server.rs` | ✅ M2 |
| `GET /v1/me` handler state-agnostic | `rest_v1/me.rs` | ✅ M3 |
| `POST /v1/groups` handler com `SET LOCAL` protocol | `rest_v1/groups.rs` | ✅ M4 |
| `GET /v1/groups/{id}` handler com header/path check | `rest_v1/groups.rs` | ✅ M4 |
| `RestError::BadRequest` + `NotFound` variants | `rest_v1/problem.rs` | ✅ M4 |
| `Principal` extractor com `X-Group-Id` optional | `garraia-auth/src/extractor.rs` | ✅ GAR-391c |
| `JwtIssuer::new_for_test` + `issue_access_for_test` | `garraia-auth/src/jwt.rs` | ✅ M2 |
| `SecurityAddon` bearer scheme no OpenAPI | `rest_v1/openapi.rs` | ✅ M3 |

**Pré-requisito FALTANDO (1 item menor, Task 1):** `seed_user_without_group(h, email) -> (Uuid, String)` helper para o "eve" actor que precisa estar autenticado mas não ser membro de nenhum grupo. ~10 linhas copiando o padrão existente sem os INSERTs de `groups` / `group_members`.

---

## File Structure

**Criar:**

- `crates/garraia-gateway/tests/authz_http_matrix.rs` — test binary com uma função `#[tokio::test]` e 15 matrix cases

**Modificar:**

- `crates/garraia-gateway/tests/common/fixtures.rs` — adicionar `seed_user_without_group`
- `crates/garraia-gateway/Cargo.toml` — adicionar `[[test]]` entry com `required-features = ["test-helpers"]`
- `plans/README.md` — bumpar linha do `0014` de `_(planejado)_` para `⏳ Em execução 2026-04-14` (Task 0, só updates a própria linha)

**NÃO tocar:**

- Qualquer arquivo sob `crates/garraia-gateway/src/` (zero código de produção)
- Qualquer migration
- `CLAUDE.md`, `ROADMAP.md` (GAR-391d não adiciona endpoints)
- `docs/adr/*` (ADR amendment deferido para pós-merge)
- Outros test binaries existentes

---

## M0 — Plan + helper foundation (Tasks 0–2)

### Task 0: Update `plans/README.md` — mark 0014 as "in execution"

**Files:**
- Modify: `plans/README.md`

**Regra:** só tocar na linha do `0014`. Linhas 0015/0016 intocadas.

- [ ] **Step 1: Substitute the 0014 line**

Localizar linha atual (~39):

```markdown
| 0014 | _(planejado)_ App-layer cross-group authz matrix via HTTP | [GAR-391d](https://linear.app/chatgpt25/issue/GAR-391) | ⏳ Deferido — aguarda endpoints REST `/v1/{chats,messages,memory,tasks,groups,me}` materializarem na Fase 3.4 |
```

Substituir por:

```markdown
| 0014 | [GAR-391d — App-layer cross-group authz matrix via HTTP](0014-gar-391d-app-layer-authz-matrix.md) | [GAR-391d](https://linear.app/chatgpt25/issue/GAR-391) | ⏳ Em execução 2026-04-14 — destravado após plan 0016 M4 entregar `POST /v1/groups` + `GET /v1/groups/{id}`. 15 cases em uma única `#[tokio::test]`. Fechamento do epic GAR-391. |
```

- [ ] **Step 2: Commit**

```bash
git add plans/0014-gar-391d-app-layer-authz-matrix.md plans/README.md
git commit -m "docs(plans): add plan 0014 (GAR-391d authz matrix via HTTP)"
```

---

### Task 1: Add `seed_user_without_group` helper

**Files:**
- Modify: `crates/garraia-gateway/tests/common/fixtures.rs`

**Reference:** o helper existente `seed_user_with_group` (M3) é o template. O novo helper faz exatamente a mesma coisa exceto pular os INSERTs de `groups` e `group_members`.

- [ ] **Step 1: Read the existing helper for pattern reference**

Run: `rg -n 'seed_user_with_group' crates/garraia-gateway/tests/common/fixtures.rs`
Expected: hit on `pub async fn seed_user_with_group` around line 30.

- [ ] **Step 2: Add the new helper at the end of `fixtures.rs`**

Insert AFTER the existing `seed_user_with_group` function (do not replace it):

```rust
/// Seed an authenticated user with NO group membership, then mint a
/// JWT for that user via `Harness::jwt::issue_access_for_test`.
///
/// Returns `(user_id, jwt_token)`.
///
/// Used by the GAR-391d authz matrix (plan 0014) to exercise the
/// "authenticated but not a member of any group" vector. Cross-group
/// authorization cannot be validated without this actor: the
/// `Principal` extractor's 403 path on `GET /v1/groups/{id}` only
/// fires when the caller has a valid JWT but no matching row in
/// `group_members` for the requested `X-Group-Id`.
///
/// Follows the same transactional pattern as `seed_user_with_group`
/// so the test suite does not exhaust Postgres connections under
/// parallel scenarios (lesson from plan 0016 M3-T3 pool exhaustion).
pub async fn seed_user_without_group(
    h: &Harness,
    email: &str,
) -> anyhow::Result<(Uuid, String)> {
    let user_id = Uuid::new_v4();

    let mut tx = h
        .admin_pool
        .begin()
        .await
        .context("seed_user_without_group: tx begin")?;

    sqlx::query(
        "INSERT INTO users (id, email, display_name, status) \
         VALUES ($1, $2, $3, 'active')",
    )
    .bind(user_id)
    .bind(email)
    .bind(format!("Test {}", email))
    .execute(&mut *tx)
    .await
    .context("seed_user_without_group: insert users")?;

    tx.commit()
        .await
        .context("seed_user_without_group: tx commit")?;

    let token = h.jwt.issue_access_for_test(user_id);

    Ok((user_id, token))
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p garraia-gateway --tests --features test-helpers`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/tests/common/fixtures.rs
git commit -m "test(gateway): seed_user_without_group fixture helper (plan 0014 T1)"
```

---

### Task 2: Declare `authz_http_matrix` test binary in `Cargo.toml`

**Files:**
- Modify: `crates/garraia-gateway/Cargo.toml`

- [ ] **Step 1: Add the `[[test]]` block**

Locate the existing `[[test]]` entries (they are at the bottom of the file, grouped with a comment like `# Plan 0016 M3-T3: authed /v1/me integration tests`). Insert AFTER the `rest_v1_groups` entry:

```toml
# Plan 0014 — GAR-391d app-layer cross-group authz matrix. 15 HTTP
# scenarios in one bundled `#[tokio::test]`. Same gating as the
# other integration binaries so running `cargo test -p garraia-gateway`
# without `--features test-helpers` yields a clean Cargo-level error
# rather than a confusing "unresolved import".
[[test]]
name = "authz_http_matrix"
path = "tests/authz_http_matrix.rs"
required-features = ["test-helpers"]
```

- [ ] **Step 2: Verify parse**

Run: `cargo check -p garraia-gateway --tests --features test-helpers`
Expected: Cargo does not yet find `tests/authz_http_matrix.rs` (file does not exist) and emits a clean compile-time error pointing to the missing file. **This is expected** — Task 3 creates the file.

Alternative pre-T3 smoke: `cargo metadata -p garraia-gateway --format-version 1 --no-deps | grep authz_http_matrix` — should show the target registered.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/Cargo.toml
git commit -m "test(gateway): declare authz_http_matrix test binary (plan 0014 T2)"
```

---

## M1 — Matrix implementation (Task 3)

### Task 3: Write the 15-case matrix in `authz_http_matrix.rs`

**Files:**
- Create: `crates/garraia-gateway/tests/authz_http_matrix.rs`

**Goal:** a single `#[tokio::test]` named `gar_391d_app_layer_authz_matrix` that:
1. Seeds the 3 actors (`alice`, `bob`, `eve`) once at the top
2. Builds a `Vec<MatrixCase>` of 15 cases
3. Iterates calling `oneshot` on each
4. Asserts on status + optional detail substring
5. Reports a single summary line at the end for operator visibility

**Case enumeration (canonical, matches plan body):**

| # | Endpoint | Actor | X-Group-Id | Body | Expected |
|---|---|---|---|---|---|
| 1 | `GET /v1/me` | alice | group_alice | — | 200 role="owner" |
| 2 | `GET /v1/me` | alice | group_bob | — | 403 (cross-group) |
| 3 | `GET /v1/me` | alice | (absent) | — | 200 no group_id no role |
| 4 | `GET /v1/me` | (none) | — | — | 401 |
| 5 | `POST /v1/groups` | eve | — | `{name:"eve's team", type:"team"}` | 201 |
| 6 | `POST /v1/groups` | (none) | — | `{name:"x", type:"team"}` | 401 |
| 7 | `POST /v1/groups` | alice | — | `{name:"   ", type:"team"}` | 400 (empty name) |
| 8 | `GET /v1/groups/{group_alice}` | alice | group_alice | — | 200 role="owner" |
| 9 | `GET /v1/groups/{group_bob}` | alice | group_bob | — | 403 (non-member) |
| 10 | `GET /v1/groups/{group_bob}` | alice | group_alice | — | 403 (header sends alice-member lookup first; Principal extractor sees alice-group_alice as valid, then handler rejects with 400 path/header mismatch — **but** case 9's pattern shows extractor runs lookup on header value, so alice→group_alice lookup OK, then handler compares `principal.group_id==Some(group_alice)` vs `path==group_bob`, returns 400 BadRequest "X-Group-Id header and path id must match") — **expected: 400** |
| 11 | `GET /v1/groups/{group_alice}` | eve | group_alice | — | 403 (non-member) |
| 12 | `GET /v1/groups/{group_alice}` | (none) | group_alice | — | 401 |
| 13 | `GET /v1/groups/{group_alice}` | alice | (absent) | — | 400 (header required) |
| 14 | `GET /v1/me` | (tampered bearer) | group_alice | — | 401 |
| 15 | `GET /v1/me` | (expired bearer) | group_alice | — | 401 |

> **Case 10 semantic note**: the `Principal` extractor runs the `group_members` lookup on whatever is in the `X-Group-Id` header. In case 10, header = `group_alice`, and alice IS a member of group_alice, so the extractor succeeds and populates `Principal { group_id: Some(group_alice), role: Some(Role::Owner), user_id: alice }`. THEN `get_group` handler compares `principal.group_id == path_id` → `Some(group_alice) == group_bob` → false → returns `RestError::BadRequest("X-Group-Id header and path id must match")` → **400**. This is the "true 400 mismatch" coverage that plan 0016 M4 review flagged as missing (code-reviewer M-2).

> **Cases 14 and 15**: tampered = a valid JWT string with one char flipped in the signature segment. Expired = a JWT with `exp` set to `now() - 60s`. We mint both via `JwtIssuer::new_for_test` utility functions — but **`new_for_test` does not currently expose an "issue expired" helper**. For case 15, emit the raw JWT with `iat/exp` manually: the `jsonwebtoken` crate is already a dep, so we construct `AccessClaims { sub, iat: past, exp: past, iss: "garraia-gateway" }` and encode with the harness JWT secret. If that becomes unwieldy, a simpler approach is to mint via the harness's `jwt` at runtime and then modify the last char of the signature segment (same trick as case 14). **Minimum slice: implement case 14 as "tampered" by mutating the last character of the signature, and implement case 15 via manual `AccessClaims` + `jsonwebtoken::encode` using the harness's JWT secret.**

---

- [ ] **Step 1: Read current `JwtIssuer` surface to understand what's exposed**

Run: `rg -n 'pub fn|#\[cfg' crates/garraia-auth/src/jwt.rs | head -30`

You need to know: does `JwtIssuer` expose the `jwt_secret` bytes or the `EncodingKey`? If yes, the test can sign an "expired" token directly. If not, case 15 must use the tamper-the-signature trick.

Likely finding: `JwtIssuer` keeps both `encoding_key` and `config` private. `config.jwt_secret` is `SecretString` exposed in the same crate but NOT outside. This means the test CANNOT mint a custom-claims JWT directly — it has to use `issue_access_for_test(user_id)` and post-process.

**Decision:** Implement BOTH case 14 and case 15 as "tamper the signature" variants. Case 14 mutates the signature segment of a valid token. Case 15 mutates the payload segment to bump `exp` into the past while keeping the (now-invalid) signature — which also yields 401 because signature validation fires before `exp` validation. The two cases are semantically distinct (signature vs claim tamper) even if both collapse to 401.

> Implementation detail: JWT structure is `header.payload.signature`, each base64url-encoded with no padding. To tamper the signature: split on `.`, mutate the last byte of the signature part, rejoin. To tamper the payload: decode the middle part as JSON, set `exp` to a past timestamp, re-encode, rejoin — the signature now does not match the modified payload and verification fails with `InvalidSignature`.

- [ ] **Step 2: Create `tests/authz_http_matrix.rs` with full matrix code**

```rust
//! GAR-391d app-layer cross-group authorization matrix (plan 0014).
//!
//! This is the fourth and final vertex of epic GAR-391. Validates the
//! HTTP-level authorization contract of the 3 tenant-scoped `/v1`
//! endpoints currently in main:
//!
//!   * `GET /v1/me`              — plan 0015 slice 1 + plan 0016 M3
//!   * `POST /v1/groups`         — plan 0016 M4
//!   * `GET /v1/groups/{id}`     — plan 0016 M4
//!
//! 15 scenarios, bundled into ONE `#[tokio::test]` to avoid the sqlx
//! runtime-teardown race documented in plan 0016 M3 fixup (commit
//! `4f8be37`). Every scenario runs against the shared `Harness` via
//! `tower::ServiceExt::oneshot`.
//!
//! ## Actors
//!
//! - `alice`: seeded user, owner of `group_alice`
//! - `bob`: seeded user, owner of `group_bob`
//! - `eve`: seeded user with no group membership
//!
//! ## How to read the matrix
//!
//! Each `MatrixCase` is a data record. The test loop walks the vec
//! calling `run_case` on each and collects failures, so a single
//! broken case does not mask the rest. The final assertion fails
//! with the first case name + human-readable delta so the operator
//! can grep `cargo test` output for the case number.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

use common::fixtures::{seed_user_with_group, seed_user_without_group};
use common::{harness_get, Harness};

// ─── Actor seeding ───────────────────────────────────────────

struct Actors {
    alice_id: Uuid,
    alice_group: Uuid,
    alice_token: String,
    bob_id: Uuid,
    bob_group: Uuid,
    bob_token: String,
    eve_id: Uuid,
    eve_token: String,
}

async fn seed_actors(h: &Harness) -> Actors {
    let (alice_id, alice_group, alice_token) =
        seed_user_with_group(h, "alice@gar-391d.test")
            .await
            .expect("seed alice");
    let (bob_id, bob_group, bob_token) =
        seed_user_with_group(h, "bob@gar-391d.test")
            .await
            .expect("seed bob");
    let (eve_id, eve_token) = seed_user_without_group(h, "eve@gar-391d.test")
        .await
        .expect("seed eve");
    Actors {
        alice_id,
        alice_group,
        alice_token,
        bob_id,
        bob_group,
        bob_token,
        eve_id,
        eve_token,
    }
}

// ─── JWT tampering helpers ───────────────────────────────────

/// Flip the last base64 character of the SIGNATURE segment of a
/// JWT. The signature no longer matches the header+payload so
/// verification fails with `InvalidSignature` -> extractor maps to
/// `401 Unauthenticated`.
fn tamper_signature(token: &str) -> String {
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT must have 3 segments");
    let sig = parts[2];
    // Flip the last character by swapping between 'A' and 'B'.
    let mut bytes = sig.as_bytes().to_vec();
    let last = *bytes.last().unwrap();
    let flipped = if last == b'A' { b'B' } else { b'A' };
    *bytes.last_mut().unwrap() = flipped;
    let tampered_sig = String::from_utf8(bytes).unwrap();
    format!("{}.{}.{}", parts[0], parts[1], tampered_sig)
}

/// Replace the PAYLOAD segment of a JWT with a new JSON that has
/// `exp` in the past. Since the header+signature were computed
/// over the original payload, signature verification fails first
/// and the extractor maps to `401 Unauthenticated` — same as
/// `tamper_signature` from the outside, but validates the
/// "semantic tamper" vector (not just random-byte mutation).
fn tamper_payload_expired(token: &str, user_id: Uuid) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let expired_payload = json!({
        "sub": user_id,
        "iat": 0_i64,
        "exp": 1_i64,
        "iss": "garraia-gateway",
    });
    let encoded = URL_SAFE_NO_PAD.encode(expired_payload.to_string().as_bytes());
    format!("{}.{}.{}", parts[0], encoded, parts[2])
}

// ─── Request builders ────────────────────────────────────────

fn req_get(
    path: &str,
    bearer: Option<&str>,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = harness_get(path);
    if let Some(token) = bearer {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
    }
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

fn req_post(
    path: &str,
    bearer: Option<&str>,
    body: Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(token) = bearer {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
    }
    req
}

// ─── Matrix case type ────────────────────────────────────────

struct MatrixCase {
    id: u8,
    name: &'static str,
    build: Box<dyn Fn(&Actors) -> Request<Body> + Send + Sync>,
    expected_status: StatusCode,
    /// Optional substring the response body's `detail` (or `role`
    /// or similar) MUST contain. Applied only when set.
    expected_body_contains: Option<&'static str>,
}

async fn run_case(h: &Harness, c: &MatrixCase, actors: &Actors) -> Result<(), String> {
    let req = (c.build)(actors);
    let resp = h
        .router
        .clone()
        .oneshot(req)
        .await
        .map_err(|e| format!("case #{} ({}) oneshot error: {e}", c.id, c.name))?;
    let status = resp.status();
    if status != c.expected_status {
        let body = resp.into_body().collect().await.map(|b| b.to_bytes());
        let body_str = body
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_else(|_| "<unreadable>".into());
        return Err(format!(
            "case #{} ({}): expected {}, got {}. body: {}",
            c.id, c.name, c.expected_status, status, body_str
        ));
    }
    if let Some(needle) = c.expected_body_contains {
        let bytes = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| format!("case #{} body collect: {e}", c.id))?
            .to_bytes();
        let body_str = String::from_utf8_lossy(&bytes).to_string();
        if !body_str.contains(needle) {
            return Err(format!(
                "case #{} ({}): body missing '{}'. body: {}",
                c.id, c.name, needle, body_str
            ));
        }
    }
    Ok(())
}

// ─── Matrix definition ───────────────────────────────────────

fn build_matrix() -> Vec<MatrixCase> {
    vec![
        // ── GET /v1/me (cases 1-4) + tamper variants (14-15) ──
        MatrixCase {
            id: 1,
            name: "GET /v1/me as alice with X-Group-Id=alice_group -> 200 owner",
            build: Box::new(|a| {
                req_get("/v1/me", Some(&a.alice_token), Some(&a.alice_group.to_string()))
            }),
            expected_status: StatusCode::OK,
            expected_body_contains: Some("\"role\":\"owner\""),
        },
        MatrixCase {
            id: 2,
            name: "GET /v1/me as alice with X-Group-Id=bob_group -> 403",
            build: Box::new(|a| {
                req_get("/v1/me", Some(&a.alice_token), Some(&a.bob_group.to_string()))
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 3,
            name: "GET /v1/me as alice without X-Group-Id -> 200 no group",
            build: Box::new(|a| req_get("/v1/me", Some(&a.alice_token), None)),
            expected_status: StatusCode::OK,
            expected_body_contains: Some("\"user_id\""),
        },
        MatrixCase {
            id: 4,
            name: "GET /v1/me without bearer -> 401",
            build: Box::new(|_a| req_get("/v1/me", None, None)),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        // ── POST /v1/groups (cases 5-7) ──
        MatrixCase {
            id: 5,
            name: "POST /v1/groups as eve (no group) -> 201",
            build: Box::new(|a| {
                req_post(
                    "/v1/groups",
                    Some(&a.eve_token),
                    json!({"name": "eve's team", "type": "team"}),
                )
            }),
            expected_status: StatusCode::CREATED,
            expected_body_contains: Some("\"name\":\"eve's team\""),
        },
        MatrixCase {
            id: 6,
            name: "POST /v1/groups without bearer -> 401",
            build: Box::new(|_a| {
                req_post(
                    "/v1/groups",
                    None,
                    json!({"name": "anon", "type": "team"}),
                )
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 7,
            name: "POST /v1/groups as alice with empty name -> 400",
            build: Box::new(|a| {
                req_post(
                    "/v1/groups",
                    Some(&a.alice_token),
                    json!({"name": "   ", "type": "team"}),
                )
            }),
            expected_status: StatusCode::BAD_REQUEST,
            expected_body_contains: Some("name"),
        },
        // ── GET /v1/groups/{id} (cases 8-13) ──
        MatrixCase {
            id: 8,
            name: "GET /v1/groups/{alice_group} as alice member -> 200 owner",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_get(
                    &path,
                    Some(&a.alice_token),
                    Some(&a.alice_group.to_string()),
                )
            }),
            expected_status: StatusCode::OK,
            expected_body_contains: Some("\"role\":\"owner\""),
        },
        MatrixCase {
            id: 9,
            name: "GET /v1/groups/{bob_group} as alice with X-Group-Id=bob_group -> 403",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.bob_group);
                req_get(&path, Some(&a.alice_token), Some(&a.bob_group.to_string()))
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 10,
            name: "GET /v1/groups/{bob_group} as alice with X-Group-Id=alice_group -> 400 (true mismatch path)",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.bob_group);
                req_get(
                    &path,
                    Some(&a.alice_token),
                    Some(&a.alice_group.to_string()),
                )
            }),
            expected_status: StatusCode::BAD_REQUEST,
            expected_body_contains: Some("match"),
        },
        MatrixCase {
            id: 11,
            name: "GET /v1/groups/{alice_group} as eve (non-member) -> 403",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_get(
                    &path,
                    Some(&a.eve_token),
                    Some(&a.alice_group.to_string()),
                )
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 12,
            name: "GET /v1/groups/{alice_group} without bearer -> 401",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_get(&path, None, Some(&a.alice_group.to_string()))
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 13,
            name: "GET /v1/groups/{alice_group} as alice without X-Group-Id header -> 400",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_get(&path, Some(&a.alice_token), None)
            }),
            expected_status: StatusCode::BAD_REQUEST,
            expected_body_contains: Some("X-Group-Id"),
        },
        // ── JWT tamper variants (cases 14-15) ──
        MatrixCase {
            id: 14,
            name: "GET /v1/me with tampered signature -> 401",
            build: Box::new(|a| {
                let tampered = tamper_signature(&a.alice_token);
                req_get("/v1/me", Some(&tampered), Some(&a.alice_group.to_string()))
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 15,
            name: "GET /v1/me with expired payload tamper -> 401",
            build: Box::new(|a| {
                let expired = tamper_payload_expired(&a.alice_token, a.alice_id);
                req_get("/v1/me", Some(&expired), Some(&a.alice_group.to_string()))
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
    ]
}

#[tokio::test]
async fn gar_391d_app_layer_authz_matrix() {
    let h = Harness::get().await;
    let actors = seed_actors(&h).await;

    // Bob's actor is seeded but only his group_id is read (by cases
    // 2 and 9). Touch the other fields to silence unused warnings
    // without `#[allow(dead_code)]` on the struct.
    let _ = (actors.bob_id, &actors.bob_token, actors.eve_id);

    let matrix = build_matrix();
    assert_eq!(
        matrix.len(),
        15,
        "GAR-391d matrix must have exactly 15 cases; got {}",
        matrix.len()
    );

    let mut failures: Vec<String> = Vec::new();
    for case in &matrix {
        if let Err(err) = run_case(&h, case, &actors).await {
            failures.push(err);
        }
    }

    assert!(
        failures.is_empty(),
        "GAR-391d authz matrix failed {} of {} cases:\n  - {}",
        failures.len(),
        matrix.len(),
        failures.join("\n  - ")
    );
}
```

> **Critical note on Send bound**: `MatrixCase::build` is a `Box<dyn Fn(&Actors) -> Request<Body> + Send + Sync>`. The closures that capture `&a.alice_group` via `move |a|` need `a` to outlive the matrix build, which it does because `build_matrix()` returns `Vec<MatrixCase>` whose closures capture nothing owned — they take `&Actors` by argument. **Do not** capture actors by move in the closures; take them by `&Actors` argument via the `|a|` parameter of each closure.

- [ ] **Step 3: Run and see it compile + exercise**

Run: `cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix -- --nocapture`
Expected: `running 1 test` followed by `test gar_391d_app_layer_authz_matrix ... ok` in ~15–25s. On first cold run Docker may take longer to pull the pgvector image (expected behavior from plan 0016 M2).

If the test fails, read the combined failure list — `run_case` returns structured error strings with case id + name + expected/got + body so you can pinpoint without rerunning individually.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/tests/authz_http_matrix.rs
git commit -m "test(gateway): GAR-391d app-layer authz matrix (15 cases, plan 0014 T3)"
```

---

## M2 — Validation + review + ship (Tasks 4–6)

### Task 4: Consolidation validation

- [ ] **Step 1: Workspace check**

Run: `cargo check --workspace`
Expected: `Finished` clean.

- [ ] **Step 2: Clippy under test-helpers**

Run: `cargo clippy -p garraia-gateway --lib --tests --features test-helpers`
Expected: zero errors, zero new warnings in `authz_http_matrix` / `fixtures`.

- [ ] **Step 3: Run every test binary**

```bash
cargo test -p garraia-auth --lib                                            # 41 passed
cargo test -p garraia-gateway --lib                                         # 167 passed
cargo test -p garraia-gateway --test rest_v1_me                             # 1 passed
cargo test -p garraia-gateway --features test-helpers --test harness_smoke  # 1 passed
cargo test -p garraia-gateway --features test-helpers --test rest_v1_me_authed  # 1 passed
cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups # 1 passed
cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix  # 1 passed NEW
cargo test -p garraia-gateway --test router_smoke_test                      # 3 passed
cargo test -p garraia-gateway --test projects_test                          # 4 passed
cargo test -p garraia-gateway --test skins_test                             # 3 passed
```

Expected: every line above ends in `test result: ok.`. No commit here — pure validation.

---

### Task 5: Review team dispatch

Dispatch `security-auditor` + `code-reviewer` **in parallel** (single message, two `Agent` tool calls). Prompts must include:

- Full commit list (`git log --oneline main..HEAD`)
- Pointer to `tests/authz_http_matrix.rs` and `tests/common/fixtures.rs`
- Pointer to plan 0014 for architectural intent
- Explicit focus: is the matrix exhaustive enough to justify **closing epic GAR-391**?

Apply any mandatory finding in a review-fix commit. Deferred items go to M5 or future slices.

---

### Task 6: Ship

- [ ] **Step 1: Push**

```bash
git push -u origin feat/0014-gar-391d-authz-matrix
```

- [ ] **Step 2: Open PR**

Title: `test(gateway): plan 0014 — GAR-391d app-layer authz matrix (fechamento GAR-391)`
Body: full scope + validations + review findings + mandatory fixes applied + known deferred + rollback.

- [ ] **Step 3: Auto Dream honest check**

Inspect `.garra-estado.md` and hook banner. Report honestly — if no new block appended, record `não disparado com evidência direta`.

- [ ] **Step 4: Final audit-compliant report**

Standard 6-section method audit.

---

## §8 Rollback plan

Every commit is independent and reversible via `git revert`. No migration, no prod code, no env var, no ROADMAP change (GAR-391d is not an endpoint). The branch touches only:

- `plans/README.md` (1 line)
- `plans/0014-gar-391d-app-layer-authz-matrix.md` (new file)
- `crates/garraia-gateway/tests/common/fixtures.rs` (+40 lines)
- `crates/garraia-gateway/Cargo.toml` (+6 lines)
- `crates/garraia-gateway/tests/authz_http_matrix.rs` (new file, ~400 lines)

To fully rollback: `git revert` each commit. The production `main` returns to `b72cc1b` state with zero residue.

## §12 Open questions (pre-start)

1. **Case 10 verb choice**: the plan asserts 400 for "X-Group-Id header and path id must match". Confirm via `rg -n 'header and path' crates/garraia-gateway/src/rest_v1/groups.rs` that this is the exact string emitted by the handler. If not, update the `expected_body_contains` substring in case 10.
2. **Case 3 body contains check**: assumes `MeResponse` serializes `user_id` as a JSON field. Confirm via `rg -n 'pub user_id' crates/garraia-gateway/src/rest_v1/me.rs`.
3. **`run_case` error-collecting vs fail-fast**: the test loop collects failures instead of panicking on the first one, so a single broken case does not mask the rest. Owner may prefer fail-fast for debuggability. Default is collect-all because the matrix is a systematic proof artifact.

These three are verification points at implementation time, not blockers.

---

## Self-review (executed 2026-04-14)

- **Spec coverage:** 15 cases listed in file structure match 15 cases in Task 3 match the final assertion `matrix.len() == 15`. The 4 questions from the analysis response (numeração, escopo, sequência, ADR amendment) are all addressed. ✅
- **Placeholder scan:** no TBDs. Every code step shows the code. Every command step shows the command. The Open Questions §12 are verification points, not placeholders. ✅
- **Type consistency:** `MatrixCase`, `Actors`, `build_matrix()`, `run_case()` are all defined once in Task 3. Fixtures helper signature matches: `seed_user_without_group(&h, &str) -> anyhow::Result<(Uuid, String)>` consistent with `seed_user_with_group` return shape minus the group_id. ✅
- **Ambiguidade:** case 10 has a detailed explanation of the semantic collapse (Principal extractor → handler). Tamper variants (14/15) have explicit implementation strategies. `Send` bound on `MatrixCase::build` noted with a critical callout. ✅

Plan ready for execution.
