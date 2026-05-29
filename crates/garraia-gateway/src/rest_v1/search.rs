//! `GET /v1/search` — unified full-text search across messages and memory_items
//! (plans 0084–0086, 0179, 0185, 0192, 0193, 0195, 0199, 0200, 0208, 0213, 0214, 0215;
//!  GAR-549, GAR-551, GAR-552, GAR-697, GAR-703, GAR-707, GAR-710, GAR-713, GAR-716, GAR-718,
//!  GAR-721, GAR-726, GAR-730, GAR-733;
//!  epic GAR-WS-SEARCH / Fase 3.4).
//!
//! ## Scope (slices 1–14)
//!
//! ```text
//! GET /v1/search?q=<q>&scope_type=group&scope_id=<group_uuid>&types=messages,memory
//! GET /v1/search?q=<q>&scope_type=chat &scope_id=<chat_uuid> &types=messages,memory
//! GET /v1/search?q=<q>&scope_type=user &scope_id=<user_uuid> &types=memory
//! ```
//!
//! Slice 4 adds `has_attachment=true|false` filter on message results:
//! rows with (or without) ≥1 entry in `message_attachments` (migration 020).
//!
//! Slice 5 (plan 0185 / GAR-703) adds `types=files` (group scope only):
//! searches `files.name` via runtime `to_tsvector('simple', name)`.
//!
//! Slice 6 (plan 0192 / GAR-707) adds `types=tasks` (group scope only):
//! searches `tasks.title || ' ' || coalesce(tasks.description_md, '')` via
//! runtime `to_tsvector('simple', ...)`. Deleted tasks excluded.
//!
//! Slice 7 (plan 0193 / GAR-710) adds `types=task_comments` (group scope only):
//! searches `task_comments.body_md` via runtime `to_tsvector('simple', body_md)`.
//! JOIN through `tasks` for group_id (RLS `task_comments_through_tasks` policy).
//! Deleted comments excluded.
//!
//! Slice 8 (plan 0195 / GAR-713) adds optional `sort_by` parameter:
//! `relevance` (default, `score DESC, created_at DESC, id DESC`),
//! `created_at_desc` (`created_at DESC, score DESC, id DESC`),
//! `created_at_asc` (`created_at ASC, score DESC, id DESC`).
//! Applied on the Rust side after per-type fetches merge into a single Vec.
//!
//! Slice 9 (plan 0199 / GAR-716) adds `types=folders` (group scope only):
//! searches `folders.name` via `to_tsvector('simple', name)`.
//! Deleted folders excluded. `sender_user_id` = `created_by`. `kind` = null.
//!
//! Slice 10 (plan 0200 / GAR-718) adds `types=chats` (group scope only):
//! searches `chats.name || ' ' || coalesce(chats.topic, '')` via
//! runtime `to_tsvector('simple', ...)`. Archived chats excluded.
//! `excerpt` = `name`. `sender_user_id` = `created_by`. `kind` = `type` ('channel','dm','thread').
//!
//! Slice 11 (plan 0208 / GAR-721) adds `types=task_lists` (group scope only):
//! searches `task_lists.name || ' ' || coalesce(task_lists.description, '')` via
//! runtime `to_tsvector('simple', ...)`. Archived lists excluded.
//! `excerpt` = name; `kind` = type ('list', 'board', 'calendar');
//! `sender_user_id` = `created_by`.
//!
//! Slice 12 (plan 0211 / GAR-726) adds `types=threads` (group scope only):
//! searches `message_threads.title` via runtime `to_tsvector('simple', title)` with
//! `title IS NOT NULL` guard. JOIN through `chats` for `group_id` (RLS policy
//! `message_threads_through_chats`, migration 007). `excerpt` = `title`; `kind` = null;
//! `sender_user_id` = `created_by`; `chat_id` = `chat_id`.
//!
//! Slice 13 (plan 0214 / GAR-730) adds `types=users` (group scope only):
//! searches `users.display_name` via runtime `to_tsvector('simple', display_name)`.
//! JOIN through `group_members` for group isolation; `status = 'active'` guard.
//! `excerpt` = `display_name`; `kind` = null; `sender_user_id` = `users.id` (the
//! matched user's own id); `chat_id` = null. Email excluded from FTS (PII safety).
//!
//! Slice 14 (plan 0215 / GAR-733) adds `types=groups` (user scope only):
//! searches `groups.name` via runtime `to_tsvector('simple', name)`.
//! Cross-tenant isolation via FORCE RLS (`groups_member_access`, migration 018) —
//! no explicit membership SQL predicate needed. `excerpt` = `name`; `kind` = group
//! `type` column ('family' | 'team' | 'personal'); `sender_user_id` = `created_by`;
//! `chat_id` = null. `group_id` = the matching group's own `id`.
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
//! Memory RLS (`memory_items_group_or_self`, migration 007:133) is dual-branch
//! and covers all three scopes — the user branch fires when `group_id IS NULL
//! AND created_by = app.current_user_id`.
//!
//! ## Cross-tenant isolation
//!
//! - `scope_type=group` + `scope_id ≠ principal.group_id` → 404
//! - `scope_type=chat`  + chat not in caller's group        → 404
//! - `scope_type=user`  + `scope_id ≠ principal.user_id`    → 404
//!
//! 404 (not 403) avoids leaking the existence of resources in other tenants.
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

/// Sort order for merged search results (slice 8 / GAR-713).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortBy {
    /// FTS rank DESC, created_at DESC, id DESC (default).
    Relevance,
    /// created_at DESC, score DESC, id DESC.
    CreatedAtDesc,
    /// created_at ASC, score DESC, id DESC.
    CreatedAtAsc,
}

/// Type discriminant for a search result item.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchResultType {
    Message,
    Memory,
    /// File name match (slice 5 / GAR-703).
    File,
    /// Task title/description match (slice 6 / GAR-707).
    Task,
    /// Task comment body match (slice 7 / GAR-710).
    TaskComment,
    /// Folder name match (slice 9 / GAR-716).
    Folder,
    /// Chat name/topic match (slice 10 / GAR-718).
    Chat,
    /// Task list name/description match (slice 11 / GAR-721).
    TaskList,
    /// Message thread title match (slice 12 / GAR-726).
    Thread,
    /// Group member display_name match (slice 13 / GAR-730).
    User,
    /// Group name match (slice 14 / GAR-733).
    Group,
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
    /// For `file` results: the uploader's user_id (`files.created_by`).
    pub sender_user_id: Option<Uuid>,
    /// For `memory` results: the scope type (`user`, `group`, `chat`).
    pub scope_type: Option<String>,
    /// For `memory` results: the scope id.
    pub scope_id: Option<Uuid>,
    /// For `memory` results: the kind (fact, preference, note, …).
    /// For `file` results: the MIME type (`files.mime_type`).
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
    /// Supported: `messages`, `memory`, `files`, `tasks`, `task_comments`, `folders`, `chats`,
    /// `task_lists`, `threads`, `users`, `groups`. Default: `messages,memory`.
    /// `files`, `tasks`, `task_comments`, `folders`, `chats`, `task_lists`, `threads`, and
    /// `users` are only valid for `scope_type=group`.
    /// `groups` is only valid for `scope_type=user`.
    pub types: Option<String>,
    /// Filter: only results created at or after this timestamp (ISO 8601 UTC). Optional.
    pub from_date: Option<DateTime<Utc>>,
    /// Filter: only results created at or before this timestamp (ISO 8601 UTC). Optional.
    pub to_date: Option<DateTime<Utc>>,
    /// Filter: for message results only, restrict to this sender UUID. Rejected for `scope_type=user`.
    pub author_id: Option<Uuid>,
    /// Filter: for message results only, restrict to messages that have (or lack) ≥1 file
    /// attachment in `message_attachments`. `true` = with attachment; `false` = without.
    /// Rejected when `types` does not include `messages`. Optional; absent means no filter.
    pub has_attachment: Option<bool>,
    /// Sort order for merged results (slice 8 / GAR-713). Accepted values:
    /// `relevance` (default), `created_at_desc`, `created_at_asc`.
    pub sort_by: Option<String>,
    /// Page size. Default 20, max 50.
    pub limit: Option<u32>,
    /// Offset for pagination. Default 0, max 10 000.
    pub offset: Option<u32>,
}

// ─── Validation ───────────────────────────────────────────────────────────────

/// Discriminator for the validated scope. Maps directly to the three accepted
/// `scope_type` values. Resolved app-layer cross-tenant checks (404) and SQL
/// filter selection are driven by this enum in the handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidatedScopeType {
    Group,
    Chat,
    User,
}

/// Parsed, validated search parameters.
#[derive(Debug)]
struct ValidatedSearch {
    q: String,
    scope_type: ValidatedScopeType,
    scope_id: Uuid,
    include_messages: bool,
    include_memory: bool,
    include_files: bool,
    /// Slice 6 / GAR-707: search task titles + descriptions.
    include_tasks: bool,
    /// Slice 7 / GAR-710: search task comment bodies.
    include_task_comments: bool,
    /// Slice 9 / GAR-716: search folder names.
    include_folders: bool,
    /// Slice 10 / GAR-718: search chat names and topics.
    include_chats: bool,
    /// Slice 11 / GAR-721: search task list names + descriptions.
    include_task_lists: bool,
    /// Slice 12 / GAR-726: search message thread titles.
    include_threads: bool,
    /// Slice 13 / GAR-730: search group member display names.
    include_users: bool,
    /// Slice 14 / GAR-733: search group names.
    include_groups: bool,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
    author_id: Option<Uuid>,
    has_attachment: Option<bool>,
    /// Slice 8 / GAR-713: result sort order.
    sort_by: SortBy,
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

    // scope_type ∈ {group, chat, user}.
    let scope_type = match params.scope_type.as_str() {
        "group" => ValidatedScopeType::Group,
        "chat" => ValidatedScopeType::Chat,
        "user" => ValidatedScopeType::User,
        other => {
            return Err(RestError::BadRequest(format!(
                "scope_type must be one of: group, chat, user (got '{other}')"
            )));
        }
    };

    // Parse types list.
    let types_str = params.types.as_deref().unwrap_or("messages,memory");
    let mut include_messages = false;
    let mut include_memory = false;
    let mut include_files = false;
    let mut include_tasks = false;
    let mut include_task_comments = false;
    let mut include_folders = false;
    let mut include_chats = false;
    let mut include_task_lists = false;
    let mut include_threads = false;
    let mut include_users = false;
    let mut include_groups = false;
    for t in types_str.split(',') {
        match t.trim() {
            "messages" => include_messages = true,
            "memory" => include_memory = true,
            "files" => include_files = true,
            "tasks" => include_tasks = true,
            "task_comments" => include_task_comments = true,
            "folders" => include_folders = true,
            "chats" => include_chats = true,
            "task_lists" => include_task_lists = true,
            "threads" => include_threads = true,
            "users" => include_users = true,
            "groups" => include_groups = true,
            other => {
                return Err(RestError::BadRequest(format!(
                    "unknown type '{other}'; supported: messages, memory, files, tasks, task_comments, folders, chats, task_lists, threads, users, groups"
                )));
            }
        }
    }
    if !include_messages
        && !include_memory
        && !include_files
        && !include_tasks
        && !include_task_comments
        && !include_folders
        && !include_chats
        && !include_task_lists
        && !include_threads
        && !include_users
        && !include_groups
    {
        return Err(RestError::BadRequest(
            "types must include at least one of: messages, memory, files, tasks, task_comments, folders, chats, task_lists, threads, users, groups"
                .into(),
        ));
    }

    // Messages have no user scope — they always belong to a chat in a group.
    // Reject the combination explicitly instead of silently filtering.
    if scope_type == ValidatedScopeType::User && include_messages {
        return Err(RestError::BadRequest(
            "scope_type=user does not support types=messages; use types=memory".into(),
        ));
    }

    // Files are always group-scoped — they cannot be retrieved via chat or user scope.
    if include_files && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=files is only supported for scope_type=group".into(),
        ));
    }

    // Tasks are always group-scoped — they cannot be retrieved via chat or user scope.
    if include_tasks && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=tasks is only supported for scope_type=group".into(),
        ));
    }

    // Task comments are always group-scoped (via JOIN through tasks).
    if include_task_comments && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=task_comments is only supported for scope_type=group".into(),
        ));
    }

    // Folders are always group-scoped — they cannot be retrieved via chat or user scope.
    if include_folders && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=folders is only supported for scope_type=group".into(),
        ));
    }

    // Chats are always group-scoped — they cannot be retrieved via chat or user scope.
    if include_chats && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=chats is only supported for scope_type=group".into(),
        ));
    }

    // Task lists are always group-scoped — they cannot be retrieved via chat or user scope.
    if include_task_lists && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=task_lists is only supported for scope_type=group".into(),
        ));
    }

    // Threads are scoped via chats → groups — they cannot be retrieved via chat or user scope.
    if include_threads && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=threads is only supported for scope_type=group".into(),
        ));
    }

    // Users are group-scoped (group members); cannot be retrieved via chat or user scope.
    if include_users && scope_type != ValidatedScopeType::Group {
        return Err(RestError::BadRequest(
            "types=users is only supported for scope_type=group".into(),
        ));
    }

    // Groups are user-scoped (cross-group membership search); cannot be retrieved via
    // group or chat scope — FORCE RLS on groups uses app.current_user_id membership
    // subquery, not a specific group_id filter.
    if include_groups && scope_type != ValidatedScopeType::User {
        return Err(RestError::BadRequest(
            "types=groups is only supported for scope_type=user".into(),
        ));
    }

    // author_id is only meaningful for message results; user scope never has messages.
    if params.author_id.is_some() && scope_type == ValidatedScopeType::User {
        return Err(RestError::BadRequest(
            "author_id is not supported for scope_type=user (user scope only searches memory)"
                .into(),
        ));
    }

    // When both dates are provided, from_date must not exceed to_date.
    if let (Some(from), Some(to)) = (params.from_date, params.to_date)
        && from > to
    {
        return Err(RestError::BadRequest(
            "from_date must not be later than to_date".into(),
        ));
    }

    // has_attachment only applies to message results.
    if params.has_attachment.is_some() && !include_messages {
        return Err(RestError::BadRequest(
            "has_attachment requires types to include 'messages'".into(),
        ));
    }

    // sort_by — slice 8 / GAR-713.
    let sort_by = match params.sort_by.as_deref().unwrap_or("relevance") {
        "relevance" => SortBy::Relevance,
        "created_at_desc" => SortBy::CreatedAtDesc,
        "created_at_asc" => SortBy::CreatedAtAsc,
        other => {
            return Err(RestError::BadRequest(format!(
                "invalid sort_by '{other}'; accepted: relevance, created_at_desc, created_at_asc"
            )));
        }
    };

    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = params.offset.unwrap_or(0);
    if offset > MAX_OFFSET {
        return Err(RestError::BadRequest(format!(
            "offset must be at most {MAX_OFFSET}"
        )));
    }

    Ok(ValidatedSearch {
        q,
        scope_type,
        scope_id: params.scope_id,
        include_messages,
        include_memory,
        include_files,
        include_tasks,
        include_task_comments,
        include_folders,
        include_chats,
        include_task_lists,
        include_threads,
        include_users,
        include_groups,
        from_date: params.from_date,
        to_date: params.to_date,
        author_id: params.author_id,
        has_attachment: params.has_attachment,
        sort_by,
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

/// Row returned by the files FTS query (slice 5 / GAR-703).
#[derive(sqlx::FromRow)]
struct FileSearchRow {
    id: Uuid,
    score: f32,
    name: String,
    group_id: Uuid,
    mime_type: String,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Row returned by the tasks FTS query (slice 6 / GAR-707).
#[derive(sqlx::FromRow)]
struct TaskSearchRow {
    id: Uuid,
    score: f32,
    title: String,
    group_id: Uuid,
    status: String,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Row returned by the task comments FTS query (slice 7 / GAR-710).
#[derive(sqlx::FromRow)]
struct TaskCommentSearchRow {
    id: Uuid,
    score: f32,
    body_md: String,
    group_id: Uuid,
    author_user_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Row returned by the folders FTS query (slice 9 / GAR-716).
#[derive(sqlx::FromRow)]
struct FolderSearchRow {
    id: Uuid,
    score: f32,
    name: String,
    group_id: Uuid,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Row returned by the chats FTS query (slice 10 / GAR-718).
#[derive(sqlx::FromRow)]
struct ChatSearchRow {
    id: Uuid,
    score: f32,
    name: String,
    group_id: Uuid,
    kind: String,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Row returned by the task_lists FTS query (slice 11 / GAR-721).
#[derive(sqlx::FromRow)]
struct TaskListSearchRow {
    id: Uuid,
    score: f32,
    name: String,
    group_id: Uuid,
    list_type: String,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Row returned by the message_threads FTS query (slice 12 / GAR-726).
#[derive(sqlx::FromRow)]
struct ThreadSearchRow {
    id: Uuid,
    score: f32,
    title: String,
    group_id: Uuid,
    chat_id: Uuid,
    created_by: Uuid,
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
///
/// `chat_filter`:
/// - `None` → group-wide search (slice 1: `scope_type=group`)
/// - `Some(chat_id)` → chat-scoped search (slice 2: `scope_type=chat`)
///
/// `from_date` / `to_date` / `author_id` / `has_attachment` are all optional;
/// `NULL` binds skip the predicate.
///
/// `has_attachment`:
/// - `None` → no attachment filter
/// - `Some(true)` → only messages with ≥1 row in `message_attachments`
/// - `Some(false)` → only messages with 0 rows in `message_attachments`
///
/// The EXISTS-equality trick `EXISTS(...) = $7` compares the boolean EXISTS result
/// directly with the parameter: when $7 IS NULL the outer IS-NULL guard short-circuits
/// to TRUE (no filter); otherwise `EXISTS(...) = true` or `EXISTS(...) = false` select
/// exactly the messages that match.
async fn fetch_messages(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    chat_filter: Option<Uuid>,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
    author_id: Option<Uuid>,
    has_attachment: Option<bool>,
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
           AND  ($3::uuid IS NULL OR m.chat_id = $3)
           AND  m.deleted_at IS NULL
           AND  ($4::timestamptz IS NULL OR m.created_at >= $4)
           AND  ($5::timestamptz IS NULL OR m.created_at <= $5)
           AND  ($6::uuid IS NULL OR m.sender_user_id = $6)
           AND  ($7::boolean IS NULL
                 OR EXISTS (SELECT 1 FROM message_attachments ma
                            WHERE ma.message_id = m.id) = $7)
         ORDER BY score DESC, m.created_at DESC, m.id DESC
         LIMIT $8",
    )
    .bind(q)
    .bind(group_id)
    .bind(chat_filter)
    .bind(from_date)
    .bind(to_date)
    .bind(author_id)
    .bind(has_attachment)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    Ok(rows)
}

/// Fetch memory results using runtime `to_tsvector` on content.
///
/// Three scope variants share a single SQL statement with NULL-able predicates:
///
/// - **Group**: `group_filter=Some(g)`, `scope_type_filter=None`, `scope_id_filter=None`.
///   Matches all memory rows visible at the group level (group-scope rows AND
///   chat-scope rows whose `group_id = g`). Slice 1 behavior — preserved.
/// - **Chat**:  `group_filter=Some(g)`, `scope_type_filter=Some("chat")`,
///   `scope_id_filter=Some(chat_id)`. Restricts to the specific chat.
/// - **User**:  `group_filter=None`,    `scope_type_filter=Some("user")`,
///   `scope_id_filter=Some(user_id)`. RLS branch 2
///   (`group_id IS NULL AND created_by = current_user_id`) handles the rest.
///
/// `from_date` / `to_date` are optional date filters (slice 3).
async fn fetch_memory(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_filter: Option<Uuid>,
    scope_type_filter: Option<&'static str>,
    scope_id_filter: Option<Uuid>,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
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
           AND  ($2::uuid IS NULL OR mi.group_id = $2)
           AND  ($3::text IS NULL OR mi.scope_type = $3)
           AND  ($4::uuid IS NULL OR mi.scope_id = $4)
           AND  mi.deleted_at IS NULL
           AND  mi.sensitivity <> 'secret'
           AND  (mi.ttl_expires_at IS NULL OR mi.ttl_expires_at > now())
           AND  ($5::timestamptz IS NULL OR mi.created_at >= $5)
           AND  ($6::timestamptz IS NULL OR mi.created_at <= $6)
         ORDER BY score DESC, mi.created_at DESC, mi.id DESC
         LIMIT $7",
    )
    .bind(q)
    .bind(group_filter)
    .bind(scope_type_filter)
    .bind(scope_id_filter)
    .bind(from_date)
    .bind(to_date)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    Ok(rows)
}

/// Fetch file results by searching `files.name` using runtime `to_tsvector('simple', name)`.
///
/// Only `scope_type=group` is supported; `scope_type=chat` and `scope_type=user` are
/// rejected at `parse_and_validate` before this function is ever called.
///
/// Uses the `'simple'` tokenizer (no stemming) — file names are identifiers, not prose.
/// RLS (`files_group_isolation` FORCE policy, migration 007) transparently filters to
/// `app.current_group_id`; the explicit `group_id = $2` is defense-in-depth.
async fn fetch_files(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    fetch_up_to: i64,
) -> Result<Vec<FileSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, FileSearchRow>(
        "SELECT f.id,
                ts_rank(
                    to_tsvector('simple', f.name),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                f.name,
                f.group_id,
                f.mime_type,
                f.created_by,
                f.created_at
         FROM   files f
         WHERE  to_tsvector('simple', f.name) @@ websearch_to_tsquery('simple', $1)
           AND  f.group_id = $2
           AND  f.deleted_at IS NULL
         ORDER BY score DESC, f.created_at DESC, f.id DESC
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

/// Fetch task results by searching `tasks.title || ' ' || coalesce(tasks.description_md, '')`
/// using runtime `to_tsvector('simple', ...)`.
///
/// Only `scope_type=group` is supported; `scope_type=chat` and `scope_type=user` are
/// rejected at `parse_and_validate` before this function is ever called.
///
/// Uses the `'simple'` tokenizer (no stemming) — task titles are short identifiers,
/// not prose. RLS (`tasks_group_rls_policy`, migration 006) transparently filters to
/// `app.current_group_id`; the explicit `group_id = $2` is defense-in-depth.
///
/// `from_date` / `to_date` filter on `tasks.created_at`.
/// `author_id` filters `tasks.created_by` (NULL-safe: `$5::uuid IS NULL OR t.created_by = $5`).
/// Deleted tasks (`deleted_at IS NOT NULL`) are always excluded.
async fn fetch_tasks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
    author_id: Option<Uuid>,
    fetch_up_to: i64,
) -> Result<Vec<TaskSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, TaskSearchRow>(
        "SELECT t.id,
                ts_rank(
                    to_tsvector('simple', t.title || ' ' || coalesce(t.description_md, '')),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                t.title,
                t.group_id,
                t.status,
                t.created_by,
                t.created_at
         FROM   tasks t
         WHERE  to_tsvector('simple', t.title || ' ' || coalesce(t.description_md, ''))
                    @@ websearch_to_tsquery('simple', $1)
           AND  t.group_id = $2
           AND  t.deleted_at IS NULL
           AND  ($3::timestamptz IS NULL OR t.created_at >= $3)
           AND  ($4::timestamptz IS NULL OR t.created_at <= $4)
           AND  ($5::uuid IS NULL OR t.created_by = $5)
         ORDER BY score DESC, t.created_at DESC, t.id DESC
         LIMIT $6",
    )
    .bind(q)
    .bind(group_id)
    .bind(from_date)
    .bind(to_date)
    .bind(author_id)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    Ok(rows)
}

/// Fetch task comment results by searching `task_comments.body_md` via runtime
/// `to_tsvector('simple', body_md)`.
///
/// Only `scope_type=group` is supported; rejected at `parse_and_validate`.
///
/// JOINs `task_comments tc → tasks t` to get `t.group_id` for the explicit
/// group-isolation filter (defense-in-depth; RLS `task_comments_through_tasks`
/// also filters via this JOIN path).
///
/// `from_date` / `to_date` filter on `tc.created_at`.
/// `author_id` filters `tc.author_user_id` (NULL-safe).
/// `tc.deleted_at IS NULL` always enforced.
/// `excerpt` is the full `body_md`; callers may truncate for display.
async fn fetch_task_comments(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
    author_id: Option<Uuid>,
    fetch_up_to: i64,
) -> Result<Vec<TaskCommentSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, TaskCommentSearchRow>(
        "SELECT tc.id,
                ts_rank(
                    to_tsvector('simple', tc.body_md),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                tc.body_md,
                t.group_id,
                tc.author_user_id,
                tc.created_at
         FROM   task_comments tc
         JOIN   tasks t ON t.id = tc.task_id
         WHERE  to_tsvector('simple', tc.body_md) @@ websearch_to_tsquery('simple', $1)
           AND  t.group_id = $2
           AND  tc.deleted_at IS NULL
           AND  ($3::timestamptz IS NULL OR tc.created_at >= $3)
           AND  ($4::timestamptz IS NULL OR tc.created_at <= $4)
           AND  ($5::uuid IS NULL OR tc.author_user_id = $5)
         ORDER BY score DESC, tc.created_at DESC, tc.id DESC
         LIMIT $6",
    )
    .bind(q)
    .bind(group_id)
    .bind(from_date)
    .bind(to_date)
    .bind(author_id)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    Ok(rows)
}

/// Fetch folder results by searching `folders.name` using runtime
/// `to_tsvector('simple', name)`.
///
/// Only `scope_type=group` is supported; `scope_type=chat` and `scope_type=user` are
/// rejected at `parse_and_validate` before this function is ever called.
///
/// Uses the `'simple'` tokenizer (no stemming) — folder names are identifiers, not prose.
/// RLS (`folders_group_isolation` FORCE policy, migration 003) transparently filters to
/// `app.current_group_id`; the explicit `group_id = $2` is defense-in-depth.
///
/// Soft-deleted folders (`deleted_at IS NOT NULL`) are always excluded.
async fn fetch_folders(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    fetch_up_to: i64,
) -> Result<Vec<FolderSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, FolderSearchRow>(
        "SELECT f.id,
                ts_rank(
                    to_tsvector('simple', f.name),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                f.name,
                f.group_id,
                f.created_by,
                f.created_at
         FROM   folders f
         WHERE  to_tsvector('simple', f.name) @@ websearch_to_tsquery('simple', $1)
           AND  f.group_id = $2
           AND  f.deleted_at IS NULL
         ORDER BY score DESC, f.created_at DESC, f.id DESC
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

/// Fetch chat results by searching `chats.name || ' ' || coalesce(chats.topic, '')`
/// using runtime `to_tsvector('simple', ...)`.
///
/// Only `scope_type=group` is supported; `scope_type=chat` and `scope_type=user` are
/// rejected at `parse_and_validate` before this function is ever called.
///
/// Uses the `'simple'` tokenizer — chat names are short identifiers, not prose.
/// RLS (`chats_group_isolation` FORCE policy, migration 007) transparently filters to
/// `app.current_group_id`; the explicit `group_id = $2` is defense-in-depth.
///
/// `topic` is used only for FTS boosting; `excerpt` is the display `name`.
/// Archived chats (`archived_at IS NOT NULL`) are always excluded.
async fn fetch_chats(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    fetch_up_to: i64,
) -> Result<Vec<ChatSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, ChatSearchRow>(
        "SELECT c.id,
                ts_rank(
                    to_tsvector('simple', c.name || ' ' || coalesce(c.topic, '')),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                c.name,
                c.group_id,
                c.type AS kind,
                c.created_by,
                c.created_at
         FROM   chats c
         WHERE  to_tsvector('simple', c.name || ' ' || coalesce(c.topic, ''))
                    @@ websearch_to_tsquery('simple', $1)
           AND  c.group_id = $2
           AND  c.archived_at IS NULL
         ORDER BY score DESC, c.created_at DESC, c.id DESC
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

/// Fetch task list results by searching
/// `task_lists.name || ' ' || coalesce(task_lists.description, '')` using
/// runtime `to_tsvector('simple', ...)`.
///
/// Only `scope_type=group` is supported; rejected at `parse_and_validate`.
///
/// RLS (`task_lists_group_isolation` FORCE policy, migration 006) transparently
/// filters to `app.current_group_id`; the explicit `group_id = $2` is
/// defense-in-depth. Archived lists excluded.
async fn fetch_task_lists(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    fetch_up_to: i64,
) -> Result<Vec<TaskListSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, TaskListSearchRow>(
        "SELECT tl.id,
                ts_rank(
                    to_tsvector('simple', tl.name || ' ' || coalesce(tl.description, '')),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                tl.name,
                tl.group_id,
                tl.type AS list_type,
                tl.created_by,
                tl.created_at
         FROM   task_lists tl
         WHERE  to_tsvector('simple', tl.name || ' ' || coalesce(tl.description, ''))
                    @@ websearch_to_tsquery('simple', $1)
           AND  tl.group_id = $2
           AND  tl.archived_at IS NULL
         ORDER BY score DESC, tl.created_at DESC, tl.id DESC
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

/// Fetch message thread results by searching `message_threads.title` using
/// runtime `to_tsvector('simple', title)`.
///
/// Only `scope_type=group` is supported; rejected at `parse_and_validate`.
///
/// RLS (`message_threads_through_chats` FORCE policy, migration 007) scopes via
/// JOIN to `chats`; the explicit `c.group_id = $2` is defense-in-depth.
/// `title IS NOT NULL` guard: threads with no title have no searchable content.
async fn fetch_threads(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    fetch_up_to: i64,
) -> Result<Vec<ThreadSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, ThreadSearchRow>(
        "SELECT mt.id,
                ts_rank(
                    to_tsvector('simple', mt.title),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                mt.title,
                c.group_id,
                mt.chat_id,
                mt.created_by,
                mt.created_at
         FROM   message_threads mt
         JOIN   chats c ON c.id = mt.chat_id
         WHERE  to_tsvector('simple', mt.title) @@ websearch_to_tsquery('simple', $1)
           AND  c.group_id = $2
           AND  mt.title IS NOT NULL
         ORDER BY score DESC, mt.created_at DESC, mt.id DESC
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

/// Row returned by the users FTS query (slice 13 / GAR-730).
#[derive(sqlx::FromRow)]
struct UserSearchRow {
    id: Uuid,
    score: f32,
    display_name: String,
    group_id: Uuid,
    joined_at: DateTime<Utc>,
}

/// Fetch group member results by searching `users.display_name` using runtime
/// `to_tsvector('simple', display_name)`.
///
/// Only `scope_type=group` is supported; rejected at `parse_and_validate`.
///
/// Cross-tenant isolation: `gm.group_id = $2` is the SQL constraint plus
/// defense-in-depth on top of RLS context set by `set_rls_context`. Only
/// active members (`gm.status = 'active'`) are returned. Email is excluded
/// from FTS for PII safety.
async fn fetch_users(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    group_id: Uuid,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
    fetch_up_to: i64,
) -> Result<Vec<UserSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, UserSearchRow>(
        "SELECT u.id,
                ts_rank(
                    to_tsvector('simple', u.display_name),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                u.display_name,
                gm.group_id,
                gm.joined_at
         FROM   group_members gm
         JOIN   users u ON u.id = gm.user_id
         WHERE  to_tsvector('simple', u.display_name) @@ websearch_to_tsquery('simple', $1)
           AND  gm.group_id = $2
           AND  gm.status = 'active'
           AND  ($3::timestamptz IS NULL OR gm.joined_at >= $3)
           AND  ($4::timestamptz IS NULL OR gm.joined_at <= $4)
         ORDER BY score DESC, gm.joined_at DESC, u.id DESC
         LIMIT $5",
    )
    .bind(q)
    .bind(group_id)
    .bind(from_date)
    .bind(to_date)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;
    Ok(rows)
}

/// Row returned by the groups FTS query (slice 14 / GAR-733).
#[derive(sqlx::FromRow)]
struct GroupSearchRow {
    id: Uuid,
    score: f32,
    name: String,
    kind: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

/// Fetch group results by searching `groups.name` using runtime
/// `to_tsvector('simple', name)`.
///
/// Only `scope_type=user` is supported; rejected at `parse_and_validate`.
///
/// Cross-tenant isolation: FORCE RLS on `groups` (migration 018) via
/// `groups_member_access` policy — only groups where `app.current_user_id`
/// has an active `group_members` row are visible. No explicit membership
/// SQL predicate needed; FORCE RLS handles isolation.
async fn fetch_groups(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    q: &str,
    from_date: Option<DateTime<Utc>>,
    to_date: Option<DateTime<Utc>>,
    fetch_up_to: i64,
) -> Result<Vec<GroupSearchRow>, RestError> {
    let rows = sqlx::query_as::<_, GroupSearchRow>(
        "SELECT g.id,
                ts_rank(
                    to_tsvector('simple', g.name),
                    websearch_to_tsquery('simple', $1)
                )::real AS score,
                g.name,
                g.type AS kind,
                g.created_by,
                g.created_at
         FROM   groups g
         WHERE  to_tsvector('simple', g.name) @@ websearch_to_tsquery('simple', $1)
           AND  ($2::timestamptz IS NULL OR g.created_at >= $2)
           AND  ($3::timestamptz IS NULL OR g.created_at <= $3)
         ORDER BY score DESC, g.created_at DESC, g.id DESC
         LIMIT $4",
    )
    .bind(q)
    .bind(from_date)
    .bind(to_date)
    .bind(fetch_up_to)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;
    Ok(rows)
}

// ─── Handler ──────────────────────────────────────────────────────────────────

/// `GET /v1/search` — unified full-text search across messages and memory.
///
/// Requires the caller to be a group member. Cross-tenant attempts return 404
/// in every variant (avoids leaking the existence of resources in other tenants).
///
/// Results are ranked by `ts_rank` (descending), then `created_at` (descending),
/// then `id` (descending) for stable ordering. Offset-based pagination.
///
/// ## Error matrix
///
/// | Condition                                                     | Status |
/// |---------------------------------------------------------------|--------|
/// | Missing/invalid JWT                                           | 401    |
/// | Caller has no group membership                                | 404    |
/// | `scope_type=group` and `scope_id ≠ principal.group_id`        | 404    |
/// | `scope_type=chat`  and chat not in caller's group / archived  | 404    |
/// | `scope_type=user`  and `scope_id ≠ principal.user_id`         | 404    |
/// | `scope_type` not in {group, chat, user}                       | 400    |
/// | `scope_type=user` + `types=messages`                          | 400    |
/// | `types=files` + `scope_type` ≠ `group`                       | 400    |
/// | `types=tasks` + `scope_type` ≠ `group`                       | 400    |
/// | `types=folders` + `scope_type` ≠ `group`                     | 400    |
/// | `types=chats` + `scope_type` ≠ `group`                       | 400    |
/// | `types=task_lists` + `scope_type` ≠ `group`                  | 400    |
/// | `types=threads` + `scope_type` ≠ `group`                     | 400    |
/// | Empty `q` or `q` > 256 chars                                  | 400    |
/// | Unknown type in `types`                                       | 400    |
/// | `has_attachment` set + `types` excludes `messages`            | 400    |
/// | `offset` > 10 000                                             | 400    |
/// | Happy path                                                    | 200    |
#[utoipa::path(
    get,
    path = "/v1/search",
    params(SearchQuery),
    responses(
        (status = 200, description = "Search results.", body = SearchResponse),
        (status = 400, description = "Invalid query parameters.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 404, description = "Cross-tenant scope or chat not in caller's group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn search(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, RestError> {
    // Caller must be in a group (every scope_type still requires a tenant context
    // for the RLS dual-branch policy on memory_items to fire correctly).
    let caller_group_id = match principal.group_id {
        Some(g) => g,
        None => return Err(RestError::NotFound),
    };

    // Validate params.
    let validated = parse_and_validate(&params)?;

    // App-layer cross-tenant checks (SQL-independent).
    match validated.scope_type {
        ValidatedScopeType::Group => {
            if validated.scope_id != caller_group_id {
                return Err(RestError::NotFound);
            }
        }
        ValidatedScopeType::User => {
            if validated.scope_id != principal.user_id {
                return Err(RestError::NotFound);
            }
        }
        ValidatedScopeType::Chat => {
            // In-tx check below — we need RLS context first.
        }
    }

    // Fetch more than needed to detect has_more.
    let fetch_up_to = (validated.offset + validated.limit + 1) as i64;

    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    set_rls_context(&mut tx, principal.user_id, caller_group_id).await?;

    // For chat scope, verify the chat belongs to the caller's group and is not
    // archived. Mirrors `memory.rs` and `messages.rs` patterns. RLS on `chats`
    // (migration 007:90-94) already filters by `group_id = current_group_id`,
    // so the explicit `group_id = $2` is defense-in-depth.
    if validated.scope_type == ValidatedScopeType::Chat {
        let chat_row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL",
        )
        .bind(validated.scope_id)
        .bind(caller_group_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

        if chat_row.is_none() {
            return Err(RestError::NotFound);
        }
    }

    // Collect all candidate results.
    let mut all: Vec<SearchResult> = Vec::new();

    if validated.include_messages {
        // User scope was rejected at parse_and_validate; messages here means
        // either Group (chat_filter=None) or Chat (chat_filter=Some(chat_id)).
        let chat_filter = match validated.scope_type {
            ValidatedScopeType::Chat => Some(validated.scope_id),
            ValidatedScopeType::Group => None,
            ValidatedScopeType::User => unreachable!(
                "include_messages with scope_type=user is rejected at parse_and_validate"
            ),
        };
        let rows = fetch_messages(
            &mut tx,
            &validated.q,
            caller_group_id,
            chat_filter,
            validated.from_date,
            validated.to_date,
            validated.author_id,
            validated.has_attachment,
            fetch_up_to,
        )
        .await?;
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
        // Memory query has three filter shapes — see fetch_memory rustdoc.
        let (group_filter, scope_type_filter, scope_id_filter): (
            Option<Uuid>,
            Option<&'static str>,
            Option<Uuid>,
        ) = match validated.scope_type {
            ValidatedScopeType::Group => (Some(caller_group_id), None, None),
            ValidatedScopeType::Chat => (
                Some(caller_group_id),
                Some("chat"),
                Some(validated.scope_id),
            ),
            ValidatedScopeType::User => (None, Some("user"), Some(principal.user_id)),
        };
        let rows = fetch_memory(
            &mut tx,
            &validated.q,
            group_filter,
            scope_type_filter,
            scope_id_filter,
            validated.from_date,
            validated.to_date,
            fetch_up_to,
        )
        .await?;
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

    if validated.include_files {
        // files are always group-scoped; scope_type != Group is rejected at
        // parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_files(&mut tx, &validated.q, caller_group_id, fetch_up_to).await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::File,
                id: r.id,
                score: r.score,
                excerpt: r.name,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: r.created_by,
                scope_type: None,
                scope_id: None,
                kind: Some(r.mime_type),
                created_at: r.created_at,
            });
        }
    }

    if validated.include_tasks {
        // tasks are always group-scoped; scope_type != Group is rejected at
        // parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_tasks(
            &mut tx,
            &validated.q,
            caller_group_id,
            validated.from_date,
            validated.to_date,
            validated.author_id,
            fetch_up_to,
        )
        .await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::Task,
                id: r.id,
                score: r.score,
                excerpt: r.title,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: r.created_by,
                scope_type: None,
                scope_id: None,
                kind: Some(r.status),
                created_at: r.created_at,
            });
        }
    }

    if validated.include_task_comments {
        // task_comments are always group-scoped (via tasks); scope_type != Group
        // is rejected at parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_task_comments(
            &mut tx,
            &validated.q,
            caller_group_id,
            validated.from_date,
            validated.to_date,
            validated.author_id,
            fetch_up_to,
        )
        .await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::TaskComment,
                id: r.id,
                score: r.score,
                excerpt: r.body_md,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: r.author_user_id,
                scope_type: None,
                scope_id: None,
                kind: None,
                created_at: r.created_at,
            });
        }
    }

    if validated.include_folders {
        // folders are always group-scoped; scope_type != Group is rejected at
        // parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_folders(&mut tx, &validated.q, caller_group_id, fetch_up_to).await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::Folder,
                id: r.id,
                score: r.score,
                excerpt: r.name,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: r.created_by,
                scope_type: None,
                scope_id: None,
                kind: None,
                created_at: r.created_at,
            });
        }
    }

    if validated.include_chats {
        // chats are always group-scoped; scope_type != Group is rejected at
        // parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_chats(&mut tx, &validated.q, caller_group_id, fetch_up_to).await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::Chat,
                id: r.id,
                score: r.score,
                excerpt: r.name,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: r.created_by,
                scope_type: None,
                scope_id: None,
                kind: Some(r.kind),
                created_at: r.created_at,
            });
        }
    }

    if validated.include_task_lists {
        // task_lists are always group-scoped; scope_type != Group is rejected at
        // parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_task_lists(&mut tx, &validated.q, caller_group_id, fetch_up_to).await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::TaskList,
                id: r.id,
                score: r.score,
                excerpt: r.name,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: r.created_by,
                scope_type: None,
                scope_id: None,
                kind: Some(r.list_type),
                created_at: r.created_at,
            });
        }
    }

    if validated.include_threads {
        // threads are scoped via chats → groups; scope_type != Group is rejected at
        // parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_threads(&mut tx, &validated.q, caller_group_id, fetch_up_to).await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::Thread,
                id: r.id,
                score: r.score,
                excerpt: r.title,
                group_id: r.group_id,
                chat_id: Some(r.chat_id),
                sender_user_id: Some(r.created_by),
                scope_type: None,
                scope_id: None,
                kind: None,
                created_at: r.created_at,
            });
        }
    }

    if validated.include_users {
        // users are group-scoped (group members); scope_type != Group is rejected at
        // parse_and_validate, so this branch only fires for Group scope.
        let rows = fetch_users(
            &mut tx,
            &validated.q,
            caller_group_id,
            validated.from_date,
            validated.to_date,
            fetch_up_to,
        )
        .await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::User,
                id: r.id,
                score: r.score,
                excerpt: r.display_name,
                group_id: r.group_id,
                chat_id: None,
                sender_user_id: Some(r.id),
                scope_type: None,
                scope_id: None,
                kind: None,
                created_at: r.joined_at,
            });
        }
    }

    if validated.include_groups {
        // groups are user-scoped (cross-group search); scope_type != User is rejected at
        // parse_and_validate, so this branch only fires for User scope.
        // FORCE RLS (groups_member_access, migration 018) filters to groups where
        // app.current_user_id has an active group_members row.
        let rows = fetch_groups(
            &mut tx,
            &validated.q,
            validated.from_date,
            validated.to_date,
            fetch_up_to,
        )
        .await?;
        for r in rows {
            all.push(SearchResult {
                result_type: SearchResultType::Group,
                id: r.id,
                score: r.score,
                excerpt: r.name,
                group_id: r.id,
                chat_id: None,
                sender_user_id: Some(r.created_by),
                scope_type: None,
                scope_id: None,
                kind: Some(r.kind),
                created_at: r.created_at,
            });
        }
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // Sort merged results per sort_by (slice 8 / GAR-713).
    all.sort_unstable_by(|a, b| match validated.sort_by {
        SortBy::Relevance => b
            .score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| b.id.cmp(&a.id)),
        SortBy::CreatedAtDesc => b
            .created_at
            .cmp(&a.created_at)
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| b.id.cmp(&a.id)),
        SortBy::CreatedAtAsc => a
            .created_at
            .cmp(&b.created_at)
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| b.id.cmp(&a.id)),
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
            from_date: None,
            to_date: None,
            author_id: None,
            has_attachment: None,
            sort_by: None,
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
        let params = make_params("hello", "everyone", None);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn scope_type_chat_accepted() {
        let params = make_params("hello", "chat", None);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.scope_type, ValidatedScopeType::Chat);
        assert!(v.include_messages);
        assert!(v.include_memory);
    }

    #[test]
    fn scope_type_user_with_memory_accepted() {
        let params = make_params("hello", "user", Some("memory"));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.scope_type, ValidatedScopeType::User);
        assert!(!v.include_messages);
        assert!(v.include_memory);
    }

    #[test]
    fn scope_type_user_with_default_types_rejected() {
        // default types = "messages,memory" — messages not allowed for user scope.
        let params = make_params("hello", "user", None);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn scope_type_user_with_messages_rejected() {
        let params = make_params("hello", "user", Some("messages"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn scope_type_user_with_messages_and_memory_rejected() {
        let params = make_params("hello", "user", Some("messages,memory"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn scope_type_chat_messages_only_accepted() {
        let params = make_params("hello", "chat", Some("messages"));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.scope_type, ValidatedScopeType::Chat);
        assert!(v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn scope_type_chat_memory_only_accepted() {
        let params = make_params("hello", "chat", Some("memory"));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.scope_type, ValidatedScopeType::Chat);
        assert!(!v.include_messages);
        assert!(v.include_memory);
    }

    #[test]
    fn scope_type_group_default_preserved_slice1() {
        let params = make_params("hello", "group", None);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.scope_type, ValidatedScopeType::Group);
        assert!(v.include_messages);
        assert!(v.include_memory);
    }

    #[test]
    fn unknown_type_rejected() {
        let params = make_params("hello", "group", Some("messages,docs"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_tasks_group_scope_accepted() {
        let params = make_params("hello", "group", Some("tasks"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_tasks);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
    }

    #[test]
    fn types_tasks_chat_scope_rejected() {
        let params = make_params("hello", "chat", Some("tasks"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_tasks_user_scope_rejected() {
        let params = make_params("hello", "user", Some("tasks"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_tasks_and_messages_group_scope_accepted() {
        let params = make_params("hello", "group", Some("messages,tasks"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_tasks);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_tasks_and_files_group_scope_accepted() {
        let params = make_params("hello", "group", Some("files,tasks"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_files);
        assert!(v.include_tasks);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_all_four_group_scope_accepted() {
        let params = make_params("hello", "group", Some("messages,memory,files,tasks"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_memory);
        assert!(v.include_files);
        assert!(v.include_tasks);
        assert!(!v.include_task_comments);
    }

    // ── Slice 7: types=task_comments ─────────────────────────────────────────

    #[test]
    fn types_task_comments_group_scope_accepted() {
        let params = make_params("hello", "group", Some("task_comments"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_task_comments);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
        assert!(!v.include_tasks);
    }

    #[test]
    fn types_task_comments_chat_scope_rejected() {
        let params = make_params("hello", "chat", Some("task_comments"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_task_comments_user_scope_rejected() {
        let params = make_params("hello", "user", Some("task_comments"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_task_comments_and_messages_group_scope_accepted() {
        let params = make_params("hello", "group", Some("messages,task_comments"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_task_comments);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_task_comments_and_tasks_group_scope_accepted() {
        let params = make_params("hello", "group", Some("tasks,task_comments"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_tasks);
        assert!(v.include_task_comments);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_all_five_group_scope_accepted() {
        let params = make_params(
            "hello",
            "group",
            Some("messages,memory,files,tasks,task_comments"),
        );
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_memory);
        assert!(v.include_files);
        assert!(v.include_tasks);
        assert!(v.include_task_comments);
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
            from_date: None,
            to_date: None,
            author_id: None,
            has_attachment: None,
            sort_by: None,
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
            from_date: None,
            to_date: None,
            author_id: None,
            has_attachment: None,
            sort_by: None,
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

    // ─── Slice 3: date-range + author_id filter tests ─────────────────────────

    fn make_params_full(
        q: &str,
        scope_type: &str,
        from_date: Option<DateTime<Utc>>,
        to_date: Option<DateTime<Utc>>,
        author_id: Option<Uuid>,
    ) -> SearchQuery {
        SearchQuery {
            q: q.to_owned(),
            scope_type: scope_type.to_owned(),
            scope_id: Uuid::new_v4(),
            types: None,
            from_date,
            to_date,
            author_id,
            has_attachment: None,
            sort_by: None,
            limit: None,
            offset: None,
        }
    }

    #[test]
    fn from_date_only_accepted() {
        let from = "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let params = make_params_full("hello", "group", Some(from), None, None);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.from_date, Some(from));
        assert_eq!(v.to_date, None);
    }

    #[test]
    fn to_date_only_accepted() {
        let to = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let params = make_params_full("hello", "group", None, Some(to), None);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.from_date, None);
        assert_eq!(v.to_date, Some(to));
    }

    #[test]
    fn from_date_equal_to_date_accepted() {
        let ts = "2026-03-15T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let params = make_params_full("hello", "group", Some(ts), Some(ts), None);
        assert!(parse_and_validate(&params).is_ok());
    }

    #[test]
    fn from_date_before_to_date_accepted() {
        let from = "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let to = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let params = make_params_full("hello", "group", Some(from), Some(to), None);
        assert!(parse_and_validate(&params).is_ok());
    }

    #[test]
    fn from_date_after_to_date_rejected() {
        let from = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let to = "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let params = make_params_full("hello", "group", Some(from), Some(to), None);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn author_id_with_group_scope_accepted() {
        let author = Uuid::new_v4();
        let params = make_params_full("hello", "group", None, None, Some(author));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.author_id, Some(author));
    }

    #[test]
    fn author_id_with_chat_scope_accepted() {
        let author = Uuid::new_v4();
        let params = make_params_full("hello", "chat", None, None, Some(author));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.author_id, Some(author));
    }

    #[test]
    fn author_id_with_user_scope_rejected() {
        let author = Uuid::new_v4();
        // user scope does not support messages, so author_id is rejected.
        let params = SearchQuery {
            q: "hello".to_owned(),
            scope_type: "user".to_owned(),
            scope_id: Uuid::new_v4(),
            types: Some("memory".to_owned()),
            from_date: None,
            to_date: None,
            author_id: Some(author),
            has_attachment: None,
            sort_by: None,
            limit: None,
            offset: None,
        };
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn all_three_filters_together_accepted() {
        let from = "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let to = "2026-06-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let author = Uuid::new_v4();
        let params = make_params_full("hello", "group", Some(from), Some(to), Some(author));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.from_date, Some(from));
        assert_eq!(v.to_date, Some(to));
        assert_eq!(v.author_id, Some(author));
    }

    #[test]
    fn no_filter_params_has_none_defaults() {
        let params = make_params("hello", "group", None);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.from_date, None);
        assert_eq!(v.to_date, None);
        assert_eq!(v.author_id, None);
    }

    // ─── Slice 4: has_attachment filter tests ─────────────────────────────────

    #[test]
    fn has_attachment_true_with_messages_accepted() {
        let mut params = make_params("hello", "group", Some("messages"));
        params.has_attachment = Some(true);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.has_attachment, Some(true));
    }

    #[test]
    fn has_attachment_false_with_messages_accepted() {
        let mut params = make_params("hello", "group", Some("messages"));
        params.has_attachment = Some(false);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.has_attachment, Some(false));
    }

    #[test]
    fn has_attachment_none_default_accepted() {
        let params = make_params("hello", "group", Some("messages"));
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.has_attachment, None);
    }

    #[test]
    fn has_attachment_with_memory_only_rejected() {
        let mut params = make_params("hello", "group", Some("memory"));
        params.has_attachment = Some(true);
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn has_attachment_with_default_types_messages_memory_accepted() {
        // Default types = messages,memory — messages IS included, so has_attachment is OK.
        let mut params = make_params("hello", "group", None);
        params.has_attachment = Some(true);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.has_attachment, Some(true));
    }

    // ── Slice 5: types=files ──────────────────────────────────────────────────

    #[test]
    fn types_files_group_scope_accepted() {
        let params = make_params("report", "group", Some("files"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_files);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_files_mixed_with_messages_accepted() {
        let params = make_params("report", "group", Some("files,messages"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_files);
        assert!(v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_files_mixed_with_all_accepted() {
        let params = make_params("x", "group", Some("files,messages,memory"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_files);
        assert!(v.include_messages);
        assert!(v.include_memory);
    }

    #[test]
    fn types_files_chat_scope_rejected() {
        let params = make_params("x", "chat", Some("files"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_files_user_scope_rejected() {
        let params = make_params("x", "user", Some("files"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn default_types_does_not_include_files() {
        let params = make_params("hello", "group", None);
        let v = parse_and_validate(&params).unwrap();
        assert!(!v.include_files);
        assert!(v.include_messages);
        assert!(v.include_memory);
    }

    // ── Slice 8: sort_by parameter (GAR-713) ─────────────────────────────────

    #[test]
    fn sort_by_absent_defaults_to_relevance() {
        let params = make_params("hello", "group", None);
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.sort_by, SortBy::Relevance);
    }

    #[test]
    fn sort_by_relevance_explicit_accepted() {
        let mut params = make_params("hello", "group", None);
        params.sort_by = Some("relevance".to_owned());
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.sort_by, SortBy::Relevance);
    }

    #[test]
    fn sort_by_created_at_desc_accepted() {
        let mut params = make_params("hello", "group", None);
        params.sort_by = Some("created_at_desc".to_owned());
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.sort_by, SortBy::CreatedAtDesc);
    }

    #[test]
    fn sort_by_created_at_asc_accepted() {
        let mut params = make_params("hello", "group", None);
        params.sort_by = Some("created_at_asc".to_owned());
        let v = parse_and_validate(&params).unwrap();
        assert_eq!(v.sort_by, SortBy::CreatedAtAsc);
    }

    #[test]
    fn sort_by_invalid_value_rejected() {
        let mut params = make_params("hello", "group", None);
        params.sort_by = Some("random_order".to_owned());
        assert!(parse_and_validate(&params).is_err());
    }

    // ── Slice 9: types=folders (GAR-716) ─────────────────────────────────────

    #[test]
    fn types_folders_group_scope_accepted() {
        let params = make_params("reports", "group", Some("folders"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_folders);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
        assert!(!v.include_tasks);
        assert!(!v.include_task_comments);
    }

    #[test]
    fn types_folders_chat_scope_rejected() {
        let params = make_params("reports", "chat", Some("folders"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_folders_user_scope_rejected() {
        let params = make_params("reports", "user", Some("folders"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_folders_and_files_group_scope_accepted() {
        let params = make_params("docs", "group", Some("folders,files"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_folders);
        assert!(v.include_files);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_folders_and_tasks_group_scope_accepted() {
        let params = make_params("project", "group", Some("folders,tasks"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_folders);
        assert!(v.include_tasks);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_all_six_group_scope_accepted() {
        let params = make_params(
            "hello",
            "group",
            Some("messages,memory,files,tasks,task_comments,folders"),
        );
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_memory);
        assert!(v.include_files);
        assert!(v.include_tasks);
        assert!(v.include_task_comments);
        assert!(v.include_folders);
    }

    // ── Slice 10: types=chats (GAR-718) ──────────────────────────────────────

    #[test]
    fn types_chats_group_scope_accepted() {
        let params = make_params("general", "group", Some("chats"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_chats);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
        assert!(!v.include_tasks);
        assert!(!v.include_task_comments);
        assert!(!v.include_folders);
    }

    #[test]
    fn types_chats_chat_scope_rejected() {
        let params = make_params("general", "chat", Some("chats"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_chats_user_scope_rejected() {
        let params = make_params("general", "user", Some("chats"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_chats_and_folders_group_scope_accepted() {
        let params = make_params("project", "group", Some("chats,folders"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_chats);
        assert!(v.include_folders);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_chats_and_tasks_group_scope_accepted() {
        let params = make_params("sprint", "group", Some("chats,tasks"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_chats);
        assert!(v.include_tasks);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    // ── Slice 11: types=task_lists (GAR-721) ─────────────────────────────────

    #[test]
    fn types_task_lists_group_scope_accepted() {
        let params = make_params("sprint", "group", Some("task_lists"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_task_lists);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
        assert!(!v.include_tasks);
        assert!(!v.include_task_comments);
        assert!(!v.include_folders);
        assert!(!v.include_chats);
        assert!(!v.include_threads);
    }

    #[test]
    fn types_task_lists_chat_scope_rejected() {
        let params = make_params("sprint", "chat", Some("task_lists"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_task_lists_user_scope_rejected() {
        let params = make_params("sprint", "user", Some("task_lists"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_task_lists_and_tasks_group_scope_accepted() {
        let params = make_params("sprint", "group", Some("task_lists,tasks"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_task_lists);
        assert!(v.include_tasks);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_task_lists_and_folders_group_scope_accepted() {
        let params = make_params("project", "group", Some("task_lists,folders"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_task_lists);
        assert!(v.include_folders);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_all_nine_group_scope_accepted() {
        let params = make_params(
            "hello",
            "group",
            Some("messages,memory,files,tasks,task_comments,folders,chats,task_lists,threads"),
        );
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_memory);
        assert!(v.include_files);
        assert!(v.include_tasks);
        assert!(v.include_task_comments);
        assert!(v.include_folders);
        assert!(v.include_chats);
        assert!(v.include_task_lists);
        assert!(v.include_threads);
    }

    #[test]
    fn types_threads_group_scope_accepted() {
        let params = make_params("question", "group", Some("threads"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_threads);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
        assert!(!v.include_tasks);
        assert!(!v.include_task_comments);
        assert!(!v.include_folders);
        assert!(!v.include_chats);
        assert!(!v.include_task_lists);
    }

    #[test]
    fn types_threads_chat_scope_rejected() {
        let params = make_params("question", "chat", Some("threads"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_threads_user_scope_rejected() {
        let params = make_params("question", "user", Some("threads"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_threads_and_chats_group_scope_accepted() {
        let params = make_params("question", "group", Some("threads,chats"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_threads);
        assert!(v.include_chats);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    #[test]
    fn types_threads_and_task_lists_group_scope_accepted() {
        let params = make_params("question", "group", Some("threads,task_lists"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_threads);
        assert!(v.include_task_lists);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
    }

    // ── Slice 13: types=users ─────────────────────────────────────────────────

    #[test]
    fn types_users_group_scope_accepted() {
        let params = make_params("joao", "group", Some("users"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_users);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
        assert!(!v.include_tasks);
        assert!(!v.include_task_comments);
        assert!(!v.include_folders);
        assert!(!v.include_chats);
        assert!(!v.include_task_lists);
        assert!(!v.include_threads);
    }

    #[test]
    fn types_users_chat_scope_rejected() {
        let params = make_params("joao", "chat", Some("users"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_users_user_scope_rejected() {
        let params = make_params("joao", "user", Some("users"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_users_and_messages_group_scope_accepted() {
        let params = make_params("joao", "group", Some("messages,users"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_users);
        assert!(!v.include_memory);
        assert!(!v.include_files);
    }

    #[test]
    fn types_users_in_supported_types_error_message() {
        let params = make_params("joao", "group", Some("nonexistent_type"));
        let err = parse_and_validate(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("users"),
            "error message should list 'users' as supported type"
        );
    }

    #[test]
    fn types_all_with_users_group_scope_accepted() {
        let params = make_params(
            "hello",
            "group",
            Some(
                "messages,memory,files,tasks,task_comments,folders,chats,task_lists,threads,users",
            ),
        );
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_messages);
        assert!(v.include_memory);
        assert!(v.include_files);
        assert!(v.include_tasks);
        assert!(v.include_task_comments);
        assert!(v.include_folders);
        assert!(v.include_chats);
        assert!(v.include_task_lists);
        assert!(v.include_threads);
        assert!(v.include_users);
    }

    // ── Slice 14: types=groups ────────────────────────────────────────────────

    #[test]
    fn types_groups_user_scope_accepted() {
        let params = make_params("project", "user", Some("groups"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_groups);
        assert!(!v.include_messages);
        assert!(!v.include_memory);
        assert!(!v.include_files);
        assert!(!v.include_tasks);
        assert!(!v.include_task_comments);
        assert!(!v.include_folders);
        assert!(!v.include_chats);
        assert!(!v.include_task_lists);
        assert!(!v.include_threads);
        assert!(!v.include_users);
    }

    #[test]
    fn types_groups_group_scope_rejected() {
        let params = make_params("project", "group", Some("groups"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_groups_chat_scope_rejected() {
        let params = make_params("project", "chat", Some("groups"));
        assert!(parse_and_validate(&params).is_err());
    }

    #[test]
    fn types_groups_and_memory_user_scope_accepted() {
        let params = make_params("project", "user", Some("memory,groups"));
        let v = parse_and_validate(&params).unwrap();
        assert!(v.include_memory);
        assert!(v.include_groups);
        assert!(!v.include_messages);
        assert!(!v.include_files);
    }

    #[test]
    fn types_groups_in_supported_types_error_message() {
        let params = make_params("project", "user", Some("nonexistent_type"));
        let err = parse_and_validate(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("groups"),
            "error message should list 'groups' as supported type"
        );
    }

    #[test]
    fn types_eleven_with_groups_group_scope_rejected() {
        // groups requires user scope; combining with group scope must fail.
        let params = make_params(
            "hello",
            "group",
            Some(
                "messages,memory,files,tasks,task_comments,folders,chats,task_lists,threads,users,groups",
            ),
        );
        assert!(parse_and_validate(&params).is_err());
    }
}
