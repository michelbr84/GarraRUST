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
//! 15 scenarios, bundled into ONE `#[tokio::test]` to avoid the
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
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

use common::fixtures::{seed_user_with_group, seed_user_without_group};
use common::{harness_get, Harness};

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
/// JWT. The signature no longer matches the `header.payload` HMAC
/// so `JwtIssuer::verify_access` fails with `InvalidSignature`
/// and the `Principal` extractor maps that to `401 Unauthenticated`.
fn tamper_signature(token: &str) -> String {
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT must have 3 segments");
    let sig = parts[2];
    let mut bytes = sig.as_bytes().to_vec();
    let last = *bytes.last().unwrap();
    // 'A' (base64 value 0) and 'B' (base64 value 1) are both valid
    // base64 alphabet chars. Flipping between them always produces
    // a valid but different base64 decoding, so the tamper is
    // guaranteed to fail HMAC verification. Team-coordinator gate
    // question 3 validated this approach.
    let flipped = if last == b'A' { b'B' } else { b'A' };
    *bytes.last_mut().unwrap() = flipped;
    let tampered_sig = String::from_utf8(bytes).unwrap();
    format!("{}.{}.{}", parts[0], parts[1], tampered_sig)
}

/// Replace the PAYLOAD segment of a JWT with a new JSON that has
/// `exp` in the past. The original signature was computed over
/// the original payload, so `JwtIssuer::verify_access` computes
/// HMAC over `header.new_payload` and finds a mismatch — failing
/// with `InvalidSignature` BEFORE `exp` is parsed. Both errors
/// map to `401 Unauthenticated` in the extractor, so the observed
/// status is the same; the distinction matters for documenting
/// the semantic vector, not for the assertion.
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
    if let Some(needle) = c.expected_body_contains {
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
                req_get(
                    &path,
                    Some(&a.alice_token),
                    Some(&a.bob_group.to_string()),
                )
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
        // ── JWT tamper variants (cases 14–15) ────────────────
        MatrixCase {
            id: 14,
            name: "GET /v1/me with tampered signature -> 401",
            build: Box::new(|a| {
                let tampered = tamper_signature(&a.alice_token);
                req_get(
                    "/v1/me",
                    Some(&tampered),
                    Some(&a.alice_group.to_string()),
                )
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 15,
            name: "GET /v1/me with expired payload tamper -> 401",
            build: Box::new(|a| {
                let expired = tamper_payload_expired(&a.alice_token, a.alice_id);
                req_get(
                    "/v1/me",
                    Some(&expired),
                    Some(&a.alice_group.to_string()),
                )
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
