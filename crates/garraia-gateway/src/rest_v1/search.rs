//! `GET /v1/search` — unified full-text search across messages and memory_items
//! (plan 0084, GAR-549, epic GAR-WS-SEARCH / Fase 3.4).
//!
//! ## Scope (slice 1)
//!
//! `GET /v1/search?q=...&scope_type=group&scope_id=<uuid>&types=messages,memory
//!               &limit=<1-50>&offset=<n>`
//!
//! Searches two FORCE-RLS tables within a single transaction:
//!
//! - `messages.body_tsv` (GIN-indexed, 'portuguese' tokenizer, migration 004)
//! - `memory_items.content` (runtime `to_tsvector`, no persistent index in slice 1)
//!
//! Results are merged in Rust, sorted by `(ts_rank DESC, created_at DESC, id DESC)`,
//! then offset-sliced. Offset pagination is standard for FTS; cursor pagination across
//! heterogeneous ranked results is deferred.
//!
//! ## Tenant-context protocol (plan 0056)
//!
//! Both `app.current_user_id` and `app.current_group_id` are SET LOCAL via
//! parameterized `set_config` before any SELECT. `true` = transaction-local.
//!
//! ## Security filters on memory_items
//!
//! `deleted_at IS NULL` · `sensitivity <> 'secret'` ·
//! `(ttl_expires_at IS NULL OR ttl_expires_at > now())` — mirrors memory.rs.
//!
//! ## FTS safety
//!
//! User-supplied `q` is always passed to `websearch_to_tsquery('portuguese', $1)`.
//! Never use raw `to_tsquery` for user input (operator injection — see migration 004
//! comment on `body_tsv`).

use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum query string length (chars). Prevents absurdly large tsquery parsing.
const MAX_QUERY_CHARS: usize = 256;

/// Default page size.
const DEFAULT_LIMIT: u32 = 20;

/// Maximum page size.
const MAX_LIMIT: u32 = 50;

/// Maximum offset allowed. Prevents full-table scans as a DoS mitigation.
const MAX_OFFSET: u32 = 10_000;

// ─── DTOs ─────────────────────────────────────────────────────────────────────

/// Type discriminant for a search result item.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchResultType {
    Message,
    Memory,
}

/// A single item in a search result list.
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResult {
    /// Result type discriminant.
    #[serde(rename = "type")]
    pub result_type: SearchResultType,
    /// Row UUID.
    pub id: Uuid,
    /// FTS relevance rank (`ts_rank`). Higher = more relevant.
    pub score: f32,
    /// The full content / body of the matched item. Not truncated in slice 1;
    /// excerpt highlighting is a future enhancement.
    pub excerpt: String,
    /// The group this item belongs to.
    pub group_id: Uuid,
    /// For `message` results: the chat the message was sent in.
    pub chat_id: Option<Uuid>,
    /// For `message` results: the sender's user_id.
    pub sender_user_id: Option<Uuid>,
    /// For `memory` results: the scope type (`user`, `group`, `chat`).
    pub scope_type: Option<String>,
    /// For `memory` results: the scope id.
    pub scope_id: Option<Uuid>,
    /// For `memory` results: the kind (fact, preference, note, …).
    pub kind: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Response body for `GET /v1/search`.
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    pub items: Vec<SearchResult>,
    /// `true` when more results exist beyond the current `offset + limit` window.
    pub has_more: bool,
}

/// Query parameters for `GET /v1/search`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct SearchQuery {
    /// Full-text search query. Passed to `websearch_to_tsquery`. Required.
    pub q: String,
    /// Scope type. Only `group` is supported in slice 1.
    pub scope_type: String,
    /// The group UUID to search within. Must equal the caller's active group.
    pub scope_id: Uuid,
    /// Comma-separated list of resource types to search.
    /// Supported: `messages`, `memory`. Default: `messages,memory`.
    pub types: Option<String>,
    /// Page size. Default 20, max 50.
    pub limit: Option<u32>,
    /// Offset for pagination. Default 0, max 10 000.
    pub offset: Option<u32>,
}

// ─── Validation ───────────────────────────────────────────────────────────────

/// Parsed, validated search parameters.
struct ValidatedSearch {
    q: String,
    scope_id: Uuid,
    include_messages: bool,
    include_memory: bool,
    limit: u32,
    offset: u32,
}

fn parse_and_validate(params: &SearchQuery) -> Result<ValidatedSearch, RestError> {
    // Validate q.
    let q = params.q.trim().to_owned();
    if q.is_empty() {
        return Err(RestError::BadRequest("q must not be empty".into()));
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(RestError::BadRequest(format!(
            "q must be at most {MAX_QUERY_CHARS} characters"
        )));
    }

    // scope_type: only "group" supported in slice 1.
    if params.scope_type.as_str() != "group" {
        return Err(RestError::BadRequest(
            "scope_type must be 'group' (user/chat scope deferred)".into(),
        ));
    }

    // Parse types list.
    let types_str = params.types.as_deref().unwrap_or("messages,memory");
    let mut include_messages = false;
    let mut include_memory = false;
    for t in types_str.split(',') {
        match t.trim() {
            "messages" => include_messages = true,
            "memory" => include_memory = true,
            other => {
                return Err(RestError::BadRequest(format!(
                    "unknown type '{other}'; supported: messages, memory"
                )));
            }
        }
    }
    if !include_messages && !include_memory {
        return Err(RestError::BadRequest(
            "types must include at least one of: messages, memory".into(),
        ));
    }

    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = params.offset.unwrap_or(0);
    if offset > MAX_OFFSET {
        return Err(RestError::BadRequest(format!(
            "offset must be at most {MAX_OFFSET}"
        )));
    }

    Ok(ValidatedSearch {
        q,
        scope_id: params.scope_id,
        include_messages,
        include_memory,
        limit,
        offset,
    })
}

// ─── Internal row types ───────────────────────────────────────────────────────

/// Row returned by the messages FTS query.
#[derive(sqlx::FromRow)]
struct MessageSearchRow {
    id: Uuid,
    score: f32,
    body: String,
    group_id: Uuid,
    chat_id: Uuid,
    sender_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Row returned by the memory_items FTS query.
#[derive(sqlx::FromRow)]
struct MemorySearchRow {
    id: Uuid,
    score: f32,
    content: String,
    group_id: Uuid,
    scope_type: String,
    scope_id: Option<Uuid>,
    kind: String,
    created_at: DateTime<Utc>,
}

// ─── RLS context helper ───────────────────────────────────────────────────────

async fn set_rls_context(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    group_id: Uuid,
) -> Result<(), RestError> {
    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(group_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(())
}

// ─── FTS queries ─────────────────────────────────────────────────────────────

/// Fetch message results from messages.body_tsv (GIN indexed).
async fn fetch_messages(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    fetch_up_to: i64,
) -> Result<Vec<MessageSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, MessageSearchRow>(
        "SELECT m.id,
                ts_rank(m.body_tsv, websearch_to_tsquery('portuguese', $1))::real AS score,
                m.body,
                m.group_id,
                m.chat_id,
                m.sender_user_id,
                m.created_at
         FROM   messages m
         WHERE  m.body_tsv @@ websearch_to_tsquery('portuguese', $1)
           AND  m.group_id = $2
           AND  m.deleted_at IS NULL
         ORDER BY score DESC, m.created_at DESC, m.id DESC
         LIMIT $3",
    )
    .bind(q)
    .bind(group_id)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    Ok(rows)
}

/// Fetch memory results using runtime `to_tsvector` on content.
async fn fetch_memory(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    fetch_up_to: i64,
) -> Result<Vec<MemorySearchRow>, RestError> {
    let rows = sqlx::query_as::<_, MemorySearchRow>(
        "SELECT mi.id,
                ts_rank(
                    to_tsvector('portuguese', mi.content),
                    websearch_to_tsquery('portuguese', $1)
                )::real AS score,
                mi.content,
                mi.group_id,
                mi.scope_type,
                mi.scope_id,
                mi.kind,
                mi.created_at
         FROM   memory_items mi
         WHERE  to_tsvector('portuguese', mi.content) @@ websearch_to_tsquery('portuguese', $1)
           AND  mi.group_id = $2
           AND  mi.deleted_at IS NULL
           AND  mi.sensitivity <> 'secret'
           AND  (mi.ttl_expires_at IS NULL OR mi.ttl_expires_at > now())
         ORDER BY score DESC, mi.created_at DESC, mi.id DESC
         LIMIT $3",
    )
    .bind(q)
    .bind(group_id)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    Ok(rows)
}

// ─── Handler ──────────────────────────────────────────────────────────────────

/// `GET /v1/search` — unified full-text search across messages and memory.
///
/// Requires the caller to be a group member. `scope_id` must equal the caller's
/// active `group_id`; mismatches return 404 (cross-group isolation).
///
/// Results are ranked by `ts_rank` (descending), then `created_at` (descending),
/// then `id` (descending) for stable ordering. Offset-based pagination.
///
/// ## Error matrix
///
/// | Condition                           | Status |
/// |-------------------------------------|--------|
/// | Missing/invalid JWT                 | 401    |
/// | Caller has no group membership      | 404    |
/// | `scope_id` ≠ `principal.group_id`   | 404    |
/// | `scope_type` ≠ `group`              | 400    |
/// | Empty `q` or `q` > 256 chars        | 400    |
/// | Unknown type in `types`             | 400    |
/// | `offset` > 10 000                   | 400    |
/// | Happy path                          | 200    |
#[utoipa::path(
    get,
    path = "/v1/search",
    params(SearchQuery),
    responses(
        (status = 200, description = "Search results.", body = SearchResponse),
        (status = 400, description = "Invalid query parameters.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 404, description = "Group not found or cross-group attempt.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn search(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, RestError> {
    // Caller must be in a group.
    let caller_group_id = match principal.group_id {
        Some(g) => g,
        None => return Err(RestError::NotFound),
    };

    // Validate params.
    let validated = parse_and_validate(&params)?;

    // Cross-group isolation: scope_id must equal the caller's group.
    if validated.scope_id != caller_group_id {
        return Err(RestError::NotFound);
    }

    // Fetch more than needed to detect has_more.
    let fetch_up_to = (validated.offset + validated.limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, caller_group_id).await?;

    // Collect all candidate results.
    let mut all: Vec<SearchResult> = Vec::new();

    if validated.include_messages {
        let rows = fetch_messages(&mut tx, &validated.q, caller_group_id, fetch_up_to).await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::Message,
                id: r.id,
                score: r.score,
                excerpt: r.body,
                group_id: r.group_id,
                chat_id: Some(r.chat_id),
                sender_user_id: r.sender_user_id,
                scope_type: None,
                scope_id: None,
                kind: None,
                created_at: r.created_at,
            });
        }
    }

    if validated.include_memory {
        let rows = fetch_memory(&mut tx, &validated.q, caller_group_id, fetch_up_to).await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::Memory,
                id: r.id,
                score: r.score,
                excerpt: r.content,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: None,
                scope_type: Some(r.scope_type),
                scope_id: r.scope_id,
                kind: Some(r.kind),
                created_at: r.created_at,
            });
        }
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Sort merged results: score DESC, created_at DESC, id DESC.
    all.sort_unstable_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| b.id.cmp(&a.id))
    });

    // Offset slice.
    let offset = validated.offset as usize;
    let limit = validated.limit as usize;
    let slice_end = (offset + limit).min(all.len());
    let has_more = all.len() > offset + limit;
    let items: Vec<SearchResult> = all.into_iter().skip(offset).take(limit).collect();

    // If offset >= slice_end, skip was past all results — return empty.
    let _ = slice_end; // used implicitly via skip+take
    Ok(Json(SearchResponse { items, has_more }))
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params(q: &str, scope_type: &str, types: Option<&str>) -> SearchQuery {
        SearchQuery {
            q: q.to_owned(),
            scope_type: scope_type.to_owned(),
            scope_id: Uuid::new_v4(),
            types: types.map(|s| s.to_owned()),
            limit: None,
            offset: None,
        }
    }

    #[test]
    fn empty_q_rejected() {
        let params = make_params("", "group", None);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn whitespace_only_q_rejected() {
        let params = make_params("   ", "group", None);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn q_too_long_rejected() {
        let long_q: String = "a".repeat(257);
        let params = make_params(&long_q, "group", None);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn q_exactly_max_len_accepted() {
        let max_q: String = "a".repeat(256);
        let params = make_params(&max_q, "group", None);
        assert!(parse_and_validate(&params).is_ok());
    }

    #[test]
    fn unsupported_scope_type_rejected() {
        let params = make_params("hello", "user", None);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn unknown_type_rejected() {
        let params = make_params("hello", "group", Some("messages,files"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_messages_only_accepted() {
        let params = make_params("hello", "group", Some("messages"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_memory_only_accepted() {
        let params = make_params("hello", "group", Some("memory"));
        let v = parse_and_validate(&params).unwrap();
        assert!(!v.include_messages);
        assert!(v.include_memory);
    }

    #[test]
    fn default_types_is_both() {
        let params = make_params("hello", "group", None);
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_memory);
    }

    #[test]
    fn offset_too_large_rejected() {
        let params = SearchQuery {
            q: "hello".to_owned(),
            scope_type: "group".to_owned(),
            scope_id: Uuid::new_v4(),
            types: None,
            limit: None,
            offset: Some(10_001),
        };
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn limit_clamped_to_max() {
        let params = SearchQuery {
            q: "hello".to_owned(),
            scope_type: "group".to_owned(),
            scope_id: Uuid::new_v4(),
            types: None,
            limit: Some(999),
            offset: None,
        };
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.limit, MAX_LIMIT);
    }

    #[test]
    fn valid_params_accepted() {
        let params = make_params("hello world", "group", Some("messages,memory"));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.q, "hello world");
        assert!(v.include_messages);
        assert!(v.include_memory);
        assert_eq!(v.limit, DEFAULT_LIMIT);
        assert_eq!(v.offset, 0);
    }
}
