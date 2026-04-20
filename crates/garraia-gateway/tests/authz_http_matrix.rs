//! GAR-391d app-layer cross-group authorization matrix (plan 0014).
//!
//! This is the fourth and final vertex of epic GAR-391. Validates
//! the HTTP-level authorization contract of the 3 tenant-scoped
//! `/v1` endpoints currently in `main`:
//!
//!   * `GET /v1/me`              — plan 0015 slice 1 + plan 0016 M3
//!   * `POST /v1/groups`         — plan 0016 M4
//!   * `GET /v1/groups/{id}`     — plan 0016 M4
//!
//! 23 scenarios, bundled into ONE `#[tokio::test]` to avoid the
//! sqlx runtime-teardown race documented in plan 0016 M3 fixup
//! (commit `4f8be37`). Every scenario runs against the shared
//! `Harness` via `tower::ServiceExt::oneshot`.
//!
//! ## Actors
//!
//! - `alice`: seeded user, owner of `group_alice`
//! - `bob`:   seeded user, owner of `group_bob`
//! - `eve`:   seeded user with no group membership
//!
//! ## How to read the matrix
//!
//! Each `MatrixCase` is a data record. The test loop walks the
//! vec calling `run_case` on each and collects failures, so a
//! single broken case does not mask the rest. The final
//! assertion fails with the list of all failing cases so the
//! operator can grep `cargo test` output for the case id.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::fixtures::{seed_user_with_group, seed_user_without_group};
use common::{Harness, harness_get};

// ─── Actor seeding ───────────────────────────────────────────

#[allow(dead_code)] // Some fields are used only by a subset of cases.
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
    let (alice_id, alice_group, alice_token) = seed_user_with_group(h, "alice@gar-391d.test")
        .await
        .expect("seed alice");
    let (bob_id, bob_group, bob_token) = seed_user_with_group(h, "bob@gar-391d.test")
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

/// Tamper the signature segment of a JWT so it no longer matches
/// the HMAC of `header.payload`. `JwtIssuer::verify_access` fails
/// with `InvalidSignature` and the `Principal` extractor maps that
/// to `401 Unauthenticated`.
///
/// Implementation: XOR the middle byte of the signature segment
/// with `0x01`. This is a deterministic mutation that always
/// produces a different byte regardless of the original value
/// (no special-case branches), and flipping a bit in the middle of
/// the signature guarantees the HMAC comparison fails. The
/// alternative (flip last char between 'A' and 'B') was rejected in
/// review because even though the code path was actually always a
/// mutation, the semantic was ambiguous and fragile to refactor.
fn tamper_signature(token: &str) -> String {
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT must have 3 segments");
    let sig = parts[2];
    let mut bytes = sig.as_bytes().to_vec();
    assert!(
        bytes.len() >= 2,
        "JWT signature segment must be at least 2 bytes"
    );
    let mid = bytes.len() / 2;
    // XOR with 0x01 on an ASCII base64 alphabet char.  The result
    // may land outside the base64 alphabet (ex: 'A' ^ 0x01 = '@'),
    // which makes the decode step reject the segment — that is
    // equally a signature failure from `jsonwebtoken`'s perspective
    // and still maps to `AuthError::JwtIssue` -> 401.
    bytes[mid] ^= 0x01;
    let tampered_sig = String::from_utf8_lossy(&bytes).into_owned();
    format!("{}.{}.{}", parts[0], parts[1], tampered_sig)
}

/// Replace the PAYLOAD segment of a JWT with a new JSON that has
/// `exp` in the past.
///
/// The token fails with `401 Unauthenticated` regardless of whether
/// `jsonwebtoken` rejects it for `InvalidSignature` (HMAC computed
/// over the new payload does not match the original signature) or
/// for `ExpiredSignature` (the `exp=1` claim is in the past). Both
/// paths collapse into `AuthError::JwtIssue` in the extractor. The
/// assertion is on `StatusCode::UNAUTHORIZED` so the test is not
/// coupled to which failure mode `jsonwebtoken` reports internally.
/// This avoids depending on implementation-detail verification
/// order of the crate.
fn tamper_payload_expired(token: &str, user_id: Uuid) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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

fn req_get(path: &str, bearer: Option<&str>, x_group_id: Option<&str>) -> Request<Body> {
    // `harness_get` already injects `ConnectInfo<SocketAddr>`
    // into the request extensions so the `tower_governor`
    // `PeerIpKeyExtractor` resolves. See plan 0016 M2.
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

fn req_post(path: &str, bearer: Option<&str>, body: Value) -> Request<Body> {
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

fn req_patch(
    path: &str,
    bearer: Option<&str>,
    x_group_id: Option<&str>,
    body: Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method(Method::PATCH)
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
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

fn req_post_grouped(
    path: &str,
    bearer: Option<&str>,
    x_group_id: Option<&str>,
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
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

// ─── Matrix case type ────────────────────────────────────────

/// Type alias for the request-builder closure on each `MatrixCase`.
/// Factored out to keep `MatrixCase::build` below `clippy::type_complexity`.
type RequestBuilder = Box<dyn Fn(&Actors) -> Request<Body> + Send + Sync>;

struct MatrixCase {
    id: u8,
    name: &'static str,
    build: RequestBuilder,
    expected_status: StatusCode,
    /// Optional substring that the response body MUST contain.
    /// Applied only when `Some`. Matched against the raw UTF-8
    /// body bytes (so both JSON keys and values are matchable).
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
    let bytes = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("case #{} body collect: {e}", c.id))?
        .to_bytes();
    let body_str = String::from_utf8_lossy(&bytes).to_string();

    if status != c.expected_status {
        return Err(format!(
            "case #{} ({}): expected {}, got {}. body: {}",
            c.id, c.name, c.expected_status, status, body_str
        ));
    }
    if let Some(needle) = c.expected_body_contains
        && !body_str.contains(needle)
    {
        return Err(format!(
            "case #{} ({}): body missing '{}'. body: {}",
            c.id, c.name, needle, body_str
        ));
    }
    Ok(())
}

// ─── Matrix definition ───────────────────────────────────────

fn build_matrix() -> Vec<MatrixCase> {
    vec![
        // ── GET /v1/me (cases 1–4) ───────────────────────────
        MatrixCase {
            id: 1,
            name: "GET /v1/me as alice with X-Group-Id=alice_group -> 200 owner",
            build: Box::new(|a| {
                req_get(
                    "/v1/me",
                    Some(&a.alice_token),
                    Some(&a.alice_group.to_string()),
                )
            }),
            expected_status: StatusCode::OK,
            expected_body_contains: Some("\"role\":\"owner\""),
        },
        MatrixCase {
            id: 2,
            name: "GET /v1/me as alice with X-Group-Id=bob_group -> 403",
            build: Box::new(|a| {
                req_get(
                    "/v1/me",
                    Some(&a.alice_token),
                    Some(&a.bob_group.to_string()),
                )
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
        // ── POST /v1/groups (cases 5–7) ──────────────────────
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
                req_post("/v1/groups", None, json!({"name": "anon", "type": "team"}))
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
            // Discriminative substring: matches the exact validation
            // message in `rest_v1::groups::create_group`. Loose
            // substrings like `"name"` were rejected in review
            // (security F-1) because the word "name" could appear in
            // other 400 bodies and mask a regression.
            expected_body_contains: Some("group name must not be empty"),
        },
        // ── GET /v1/groups/{id} (cases 8–13) ─────────────────
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
                req_get(&path, Some(&a.eve_token), Some(&a.alice_group.to_string()))
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
        // ── JWT tamper variants (cases 14–15) ────────────────
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
        // ── PATCH /v1/groups/{id} (cases 16–19, plan 0017 Task 6) ──
        MatrixCase {
            id: 16,
            name: "PATCH /v1/groups/{alice_group} as alice (owner) -> 200",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_patch(
                    &path,
                    Some(&a.alice_token),
                    Some(&a.alice_group.to_string()),
                    json!({"name": "alice-renamed-by-matrix"}),
                )
            }),
            expected_status: StatusCode::OK,
            expected_body_contains: Some("alice-renamed-by-matrix"),
        },
        MatrixCase {
            id: 17,
            name: "PATCH /v1/groups/{alice_group} as bob (non-member) -> 403",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_patch(
                    &path,
                    Some(&a.bob_token),
                    Some(&a.alice_group.to_string()),
                    json!({"name": "bob-hack"}),
                )
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 18,
            name: "PATCH /v1/groups/{alice_group} as eve (no group) -> 403",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_patch(
                    &path,
                    Some(&a.eve_token),
                    Some(&a.alice_group.to_string()),
                    json!({"name": "eve-hack"}),
                )
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 19,
            name: "PATCH /v1/groups/{alice_group} without bearer -> 401",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}", a.alice_group);
                req_patch(
                    &path,
                    None,
                    Some(&a.alice_group.to_string()),
                    json!({"name": "anon-hack"}),
                )
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        // ── POST /v1/groups/{id}/invites (plan 0018, cases 20-23) ──
        MatrixCase {
            id: 20,
            name: "POST invite as alice(owner) -> 201",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}/invites", a.alice_group);
                req_post_grouped(
                    &path,
                    Some(&a.alice_token),
                    Some(&a.alice_group.to_string()),
                    json!({"email": "matrix-20@0018.test", "role": "member"}),
                )
            }),
            expected_status: StatusCode::CREATED,
            expected_body_contains: Some("matrix-20@0018.test"),
        },
        MatrixCase {
            id: 21,
            name: "POST invite as alice(owner) different email -> 201",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}/invites", a.alice_group);
                req_post_grouped(
                    &path,
                    Some(&a.alice_token),
                    Some(&a.alice_group.to_string()),
                    json!({"email": "matrix-21@0018.test", "role": "guest"}),
                )
            }),
            expected_status: StatusCode::CREATED,
            expected_body_contains: Some("matrix-21@0018.test"),
        },
        MatrixCase {
            id: 22,
            name: "POST invite as bob(non-member of alice group) -> 403",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}/invites", a.alice_group);
                req_post_grouped(
                    &path,
                    Some(&a.bob_token),
                    Some(&a.alice_group.to_string()),
                    json!({"email": "matrix-22@0018.test", "role": "member"}),
                )
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 23,
            name: "POST invite as eve(no group) -> 403",
            build: Box::new(|a| {
                let path = format!("/v1/groups/{}/invites", a.alice_group);
                req_post_grouped(
                    &path,
                    Some(&a.eve_token),
                    Some(&a.alice_group.to_string()),
                    json!({"email": "matrix-23@0018.test", "role": "member"}),
                )
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        // ── POST /v1/invites/{token}/accept (plan 0019, cases 24-26) ──
        //
        // The matrix does not seed a real invite token, so these
        // cases exercise only the authz/error paths:
        //   24: valid JWT (alice) + bogus token -> 404 (handler runs,
        //       no hash match in pending set)
        //   25: no JWT -> 401 (Principal extractor short-circuits)
        //   26: valid JWT (eve, not a member of anything) + bogus
        //       token -> 404 (accept is not gated by membership)
        //
        // Happy-path and 409/410 coverage live in the dedicated
        // rest_v1_invites.rs integration test (plan 0019 T4).
        MatrixCase {
            id: 24,
            name: "POST accept invite as alice + bogus token -> 404",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::POST)
                    .uri("/v1/invites/bogus-token-24/accept")
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&format!("Bearer {}", a.alice_token)).unwrap(),
                );
                req
            }),
            expected_status: StatusCode::NOT_FOUND,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 25,
            name: "POST accept invite no bearer -> 401",
            build: Box::new(|_a| {
                let mut req = Request::builder()
                    .method(Method::POST)
                    .uri("/v1/invites/bogus-token-25/accept")
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 26,
            name: "POST accept invite as eve(no group) + bogus token -> 404",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::POST)
                    .uri("/v1/invites/bogus-token-26/accept")
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&format!("Bearer {}", a.eve_token)).unwrap(),
                );
                req
            }),
            expected_status: StatusCode::NOT_FOUND,
            expected_body_contains: None,
        },
        // ── POST /v1/groups/{id}/members/{user_id}/setRole (plan 0020, cases 27-30) ──
        //
        // Path-level authz for the new setRole endpoint. Happy-path and
        // 400/409 coverage live in rest_v1_groups.rs (M1-M10). These
        // cases exercise ONLY:
        //   27: owner + non-member target -> 404 (handler SELECT empty)
        //   28: non-member caller -> 403 (Principal extractor)
        //   29: no-group caller -> 403 (Principal extractor)
        //   30: no bearer -> 401 (Principal short-circuit)
        MatrixCase {
            id: 27,
            name: "POST setRole as alice(owner) on bob(non-member of alice_group) -> 404",
            build: Box::new(|a| {
                let path = format!(
                    "/v1/groups/{}/members/{}/setRole",
                    a.alice_group, a.bob_id
                );
                req_post_grouped(
                    &path,
                    Some(&a.alice_token),
                    Some(&a.alice_group.to_string()),
                    json!({"role": "admin"}),
                )
            }),
            expected_status: StatusCode::NOT_FOUND,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 28,
            name: "POST setRole as bob(non-member of alice_group) on alice -> 403",
            build: Box::new(|a| {
                let path = format!(
                    "/v1/groups/{}/members/{}/setRole",
                    a.alice_group, a.alice_id
                );
                req_post_grouped(
                    &path,
                    Some(&a.bob_token),
                    Some(&a.alice_group.to_string()),
                    json!({"role": "admin"}),
                )
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 29,
            name: "POST setRole as eve(no group) on alice -> 403",
            build: Box::new(|a| {
                let path = format!(
                    "/v1/groups/{}/members/{}/setRole",
                    a.alice_group, a.alice_id
                );
                req_post_grouped(
                    &path,
                    Some(&a.eve_token),
                    Some(&a.alice_group.to_string()),
                    json!({"role": "admin"}),
                )
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 30,
            name: "POST setRole no bearer -> 401",
            build: Box::new(|a| {
                let path = format!(
                    "/v1/groups/{}/members/{}/setRole",
                    a.alice_group, a.alice_id
                );
                req_post_grouped(
                    &path,
                    None,
                    Some(&a.alice_group.to_string()),
                    json!({"role": "admin"}),
                )
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        // ── DELETE /v1/groups/{id}/members/{user_id} (plan 0020, cases 31-34) ──
        MatrixCase {
            id: 31,
            name: "DELETE member as alice(owner) on bob(non-member of alice_group) -> 404",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/v1/groups/{}/members/{}",
                        a.alice_group, a.bob_id
                    ))
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&format!("Bearer {}", a.alice_token)).unwrap(),
                );
                req.headers_mut().insert(
                    HeaderName::from_static("x-group-id"),
                    HeaderValue::from_str(&a.alice_group.to_string()).unwrap(),
                );
                req
            }),
            expected_status: StatusCode::NOT_FOUND,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 32,
            name: "DELETE member as bob(non-member of alice_group) on alice -> 403",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/v1/groups/{}/members/{}",
                        a.alice_group, a.alice_id
                    ))
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&format!("Bearer {}", a.bob_token)).unwrap(),
                );
                req.headers_mut().insert(
                    HeaderName::from_static("x-group-id"),
                    HeaderValue::from_str(&a.alice_group.to_string()).unwrap(),
                );
                req
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 33,
            name: "DELETE member as eve(no group) on alice -> 403",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/v1/groups/{}/members/{}",
                        a.alice_group, a.alice_id
                    ))
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&format!("Bearer {}", a.eve_token)).unwrap(),
                );
                req.headers_mut().insert(
                    HeaderName::from_static("x-group-id"),
                    HeaderValue::from_str(&a.alice_group.to_string()).unwrap(),
                );
                req
            }),
            expected_status: StatusCode::FORBIDDEN,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 34,
            name: "DELETE member no bearer -> 401",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/v1/groups/{}/members/{}",
                        a.alice_group, a.alice_id
                    ))
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("x-group-id"),
                    HeaderValue::from_str(&a.alice_group.to_string()).unwrap(),
                );
                req
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

    let matrix = build_matrix();
    assert_eq!(
        matrix.len(),
        34,
        "GAR-391d + plans 0017/0018/0019/0020 matrix must have exactly 34 cases; got {}",
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
