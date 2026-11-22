//! Per-group walk of `GET /v1/me/export`: re-points RLS at each group the
//! subject belongs to and gathers the content they wrote or received there.

// Request-path (#1569): panic aqui e controlavel por quem manda a requisicao.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::rest_v1::problem::RestError;

use super::types::{
    EXPORT_MAX_FILES, EXPORT_MAX_MEMORY, EXPORT_MAX_MENTIONS, EXPORT_MAX_MESSAGES,
    EXPORT_MAX_TASKS, EXPORT_MENTION_EXCERPT_CHARS, ExportMeFile, ExportMeGroupMembership,
    ExportMeMemoryItem, ExportMeMentionReceived, ExportMeMessage, ExportMeTask, ExportMemoryRow,
    map_memory_row,
};

/// What the walk gathered, section by section. Each section may still hold
/// the one sentinel row past its cap: the handler's `cap_section` drops it
/// and records the truncation.
pub(super) struct GroupContent {
    pub(super) messages: Vec<ExportMeMessage>,
    pub(super) mentions_received: Vec<ExportMeMentionReceived>,
    pub(super) files: Vec<ExportMeFile>,
    pub(super) memory_items: Vec<ExportMeMemoryItem>,
    pub(super) tasks: Vec<ExportMeTask>,
}

/// Content, group by group.
///
/// `memory_items` arrives already holding the personal slice (read by the
/// caller under the nil group GUC): the group-memory budget counts it, so
/// personal and group memory share one cap.
///
/// **GUC contract.** This function points `app.current_group_id` at each
/// visited group and, before returning `Ok`, points it back at the nil uuid —
/// whoever re-points RLS at a tenant is the one that restores it. The
/// caller's next write (the user-scoped audit INSERT) depends on that. On
/// `Err` the caller drops the transaction, so no GUC outlives it.
pub(super) async fn collect_group_content(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    group_memberships: &[ExportMeGroupMembership],
    mut memory_items: Vec<ExportMeMemoryItem>,
) -> Result<GroupContent, RestError> {
    // `set_config(..., true)` is transaction-local, so re-issuing it per
    // group re-points RLS for the remainder of this transaction. Only groups
    // with an active membership are visited (that is what the caller's
    // `group_memberships` query filtered on), and every query additionally pins the
    // subject's own id — belt and braces, because a bug in either layer
    // alone must not leak another tenant.
    let mut messages: Vec<ExportMeMessage> = Vec::new();
    let mut mentions_received: Vec<ExportMeMentionReceived> = Vec::new();
    let mut files: Vec<ExportMeFile> = Vec::new();
    let mut tasks: Vec<ExportMeTask> = Vec::new();

    for membership in group_memberships {
        let gid = membership.group_id;

        sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
            .bind(gid.to_string())
            .execute(&mut **tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

        // Messages the subject WROTE. Full body: their own content.
        // cap + 1: sentinel row, see the cap constants in `types.rs`.
        let budget = EXPORT_MAX_MESSAGES + 1 - messages.len() as i64;
        if budget > 0 {
            type MsgRow = (
                Uuid,
                Uuid,
                Uuid,
                String,
                String,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
                Option<DateTime<Utc>>,
            );
            let rows: Vec<MsgRow> = sqlx::query_as(
                "SELECT m.id, m.chat_id, m.group_id, c.name, m.body, \
                        m.created_at, m.edited_at, m.deleted_at \
                 FROM messages m \
                 JOIN chats c ON c.id = m.chat_id \
                 WHERE m.sender_user_id = $1 AND m.group_id = $2 \
                 ORDER BY m.created_at DESC, m.id DESC \
                 LIMIT $3",
            )
            .bind(user_id)
            .bind(gid)
            .bind(budget)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            messages.extend(rows.into_iter().map(
                |(
                    message_id,
                    chat_id,
                    group_id,
                    chat_name,
                    body,
                    created_at,
                    edited_at,
                    deleted_at,
                )| ExportMeMessage {
                    message_id,
                    chat_id,
                    group_id,
                    chat_name,
                    body,
                    created_at,
                    edited_at,
                    deleted_at,
                },
            ));
        }

        // @mentions the subject RECEIVED — data about them, authored by
        // someone else, so excerpt only.
        // cap + 1: sentinel row, see the cap constants in `types.rs`.
        let budget = EXPORT_MAX_MENTIONS + 1 - mentions_received.len() as i64;
        if budget > 0 {
            type MentionRow = (Uuid, Uuid, Uuid, Uuid, String, String, DateTime<Utc>);
            let rows: Vec<MentionRow> = sqlx::query_as(
                "SELECT mm.message_id, m.chat_id, mm.group_id, \
                        m.sender_user_id, m.sender_label, \
                        LEFT(m.body, $4::int) AS body_excerpt, mm.created_at \
                 FROM message_mentions mm \
                 JOIN messages m ON mm.message_id = m.id \
                 WHERE mm.mentioned_user_id = $1 AND mm.group_id = $2 \
                 ORDER BY mm.created_at DESC, mm.message_id DESC \
                 LIMIT $3",
            )
            .bind(user_id)
            .bind(gid)
            .bind(budget)
            .bind(EXPORT_MENTION_EXCERPT_CHARS)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            mentions_received.extend(rows.into_iter().map(
                |(
                    message_id,
                    chat_id,
                    group_id,
                    sender_user_id,
                    sender_label,
                    body_excerpt,
                    created_at,
                )| ExportMeMentionReceived {
                    message_id,
                    chat_id,
                    group_id,
                    sender_user_id,
                    sender_label,
                    body_excerpt,
                    created_at,
                },
            ));
        }

        // Files the subject uploaded. Metadata only — see `exclusions`.
        // cap + 1: sentinel row, see the cap constants in `types.rs`.
        let budget = EXPORT_MAX_FILES + 1 - files.len() as i64;
        if budget > 0 {
            type FileExportRow = (
                Uuid,
                Uuid,
                String,
                String,
                i64,
                Option<Uuid>,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
            );
            let rows: Vec<FileExportRow> = sqlx::query_as(
                "SELECT id, group_id, name, mime_type, size_bytes, folder_id, \
                        created_at, deleted_at \
                 FROM files \
                 WHERE created_by = $1 AND group_id = $2 \
                 ORDER BY created_at DESC, id DESC \
                 LIMIT $3",
            )
            .bind(user_id)
            .bind(gid)
            .bind(budget)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            files.extend(rows.into_iter().map(
                |(
                    file_id,
                    group_id,
                    name,
                    mime_type,
                    size_bytes,
                    folder_id,
                    created_at,
                    deleted_at,
                )| ExportMeFile {
                    file_id,
                    group_id,
                    name,
                    mime_type,
                    size_bytes,
                    folder_id,
                    created_at,
                    deleted_at,
                },
            ));
        }

        // Group-scoped memory the subject created (branch 1 of the dual
        // policy, which needs the real group GUC set above).
        // cap + 1: sentinel row (already spent on the personal slice if that
        // one filled up), see the cap constants in `types.rs`.
        let budget = EXPORT_MAX_MEMORY + 1 - memory_items.len() as i64;
        if budget > 0 {
            let rows: Vec<ExportMemoryRow> = sqlx::query_as(
                "SELECT id, scope_type, group_id, kind, sensitivity, content, \
                        ttl_expires_at, created_at, deleted_at \
                 FROM memory_items \
                 WHERE created_by = $1 AND group_id = $2 \
                 ORDER BY created_at DESC, id DESC \
                 LIMIT $3",
            )
            .bind(user_id)
            .bind(gid)
            .bind(budget)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            memory_items.extend(rows.into_iter().map(map_memory_row));
        }

        // Tasks the subject created OR is assigned to. `authored` /
        // `assigned` tell the two apart, because a task someone else created
        // and assigned to the subject is shared work-tracking, not their
        // authored content.
        // cap + 1: sentinel row, see the cap constants in `types.rs`.
        let budget = EXPORT_MAX_TASKS + 1 - tasks.len() as i64;
        if budget > 0 {
            type TaskExportRow = (
                Uuid,
                Uuid,
                Uuid,
                String,
                String,
                String,
                bool,
                bool,
                Option<DateTime<Utc>>,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
            );
            let rows: Vec<TaskExportRow> = sqlx::query_as(
                "SELECT t.id, t.list_id, t.group_id, t.title, t.status, t.priority, \
                        (t.created_by = $1) AS authored, \
                        EXISTS ( \
                            SELECT 1 FROM task_assignees ta \
                            WHERE ta.task_id = t.id AND ta.user_id = $1 \
                        ) AS assigned, \
                        t.due_at, t.created_at, t.deleted_at \
                 FROM tasks t \
                 WHERE t.group_id = $2 \
                   AND (t.created_by = $1 OR EXISTS ( \
                        SELECT 1 FROM task_assignees ta2 \
                        WHERE ta2.task_id = t.id AND ta2.user_id = $1 \
                   )) \
                 ORDER BY t.created_at DESC, t.id DESC \
                 LIMIT $3",
            )
            .bind(user_id)
            .bind(gid)
            .bind(budget)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| RestError::Internal(e.into()))?;

            tasks.extend(rows.into_iter().map(
                |(
                    task_id,
                    list_id,
                    group_id,
                    title,
                    status,
                    priority,
                    authored,
                    assigned,
                    due_at,
                    created_at,
                    deleted_at,
                )| ExportMeTask {
                    task_id,
                    list_id,
                    group_id,
                    title,
                    status,
                    priority,
                    authored,
                    assigned,
                    due_at,
                    created_at,
                    deleted_at,
                },
            ));
        }
    }

    // Back to the nil uuid: the caller's audit INSERT is a user-scoped event
    // and its RLS branch matches `group_id = app.current_group_id`. Leaving
    // the GUC on the last visited group would file the event under that group.
    sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
        .bind(Uuid::nil().to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(GroupContent {
        messages,
        mentions_received,
        files,
        memory_items,
        tasks,
    })
}
