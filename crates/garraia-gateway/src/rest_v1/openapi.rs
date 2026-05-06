//! OpenAPI 3.1 aggregator for the `/v1` surface (plan 0015 + M3).
//!
//! New endpoints go under `paths(...)` and their request/response DTOs
//! go under `components(schemas(...))`. The aggregated document is
//! exposed at `/v1/openapi.json` and rendered by Swagger UI at `/docs`.
//!
//! Plan 0016 M3 adds a `SecurityAddon` modifier that registers the
//! `"bearer"` HTTP security scheme (JWT-format) in `components.securitySchemes`.
//! Handlers reference it via `#[utoipa::path(..., security(("bearer" = [])))]`
//! — see `me::get_me`.

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use super::audit::{AuditEventSummary, ListAuditResponse};
use super::chats::{ChatListResponse, ChatResponse, ChatSummary, CreateChatRequest};
use super::groups::{
    CreateGroupRequest, CreateInviteRequest, GroupReadResponse, GroupResponse, InviteResponse,
    MemberResponse, SetRoleRequest, UpdateGroupRequest,
};
use super::invites::AcceptInviteResponse;
use super::me::MeResponse;
use super::memory::{
    CreateMemoryRequest, ListMemoryResponse, MemoryItemResponse, MemoryItemSummary,
};
use super::messages::{
    CreateThreadRequest, MessageListResponse, MessageResponse, MessageSummary, SendMessageRequest,
    ThreadResponse,
};
use super::problem::ProblemDetails;
use super::tasks::{
    CommentResponse, CreateCommentRequest, CreateTaskListRequest, CreateTaskRequest,
    ListCommentsResponse, ListTaskListsResponse, ListTasksResponse, PatchTaskListRequest,
    PatchTaskRequest, TaskListResponse, TaskListSummary, TaskResponse, TaskSummary,
};
use super::uploads::{CreateUploadRequest, CreateUploadResponse};

/// Plan 0016 M3-T1 — registers a bearer JWT `SecurityScheme` in the
/// generated OpenAPI document's `components.securitySchemes`. Applied
/// via `#[openapi(modifiers(&SecurityAddon))]` on [`ApiDoc`].
///
/// This is the standard `utoipa` pattern for declaring auth schemes
/// without tying the runtime validation to the declaration — the
/// actual verification still happens in `garraia_auth::Principal`.
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        // Use `get_or_insert_with(Default::default)` rather than
        // `.expect("...")` so this modifier is robust to any future
        // refactor that strips `components(schemas(...))` from the
        // `ApiDoc` derive. The current derive always yields
        // `Some(Components { .. })` at macro expansion time, but
        // the invariant is not compiler-enforced — a silent panic
        // at `GET /v1/openapi.json` in production would be a
        // 500-no-body regression that is trivial to prevent here.
        // Plan 0016 M3 review fix (security + code-reviewer).
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "GarraIA REST /v1",
        version = "0.1.0",
        description = "Versioned GarraIA gateway REST surface (Fase 3.4)."
    ),
    paths(
        super::me::get_me,
        super::groups::create_group,
        super::groups::get_group,
        super::groups::patch_group,
        super::groups::create_invite,
        super::groups::set_member_role,
        super::groups::delete_member,
        super::invites::accept_invite,
        super::uploads::create_upload,
        super::uploads::head_upload,
        super::uploads::patch_upload,
        super::uploads::options_uploads,
        super::chats::create_chat,
        super::chats::list_chats,
        super::messages::send_message,
        super::messages::list_messages,
        super::messages::create_thread,
        super::memory::list_memory,
        super::memory::create_memory,
        super::memory::delete_memory,
        super::tasks::create_task_list,
        super::tasks::list_task_lists,
        super::tasks::patch_task_list,
        super::tasks::delete_task_list,
        super::tasks::create_task,
        super::tasks::list_tasks,
        super::tasks::get_task,
        super::tasks::patch_task,
        super::tasks::delete_task,
        super::tasks::create_task_comment,
        super::tasks::list_task_comments,
        super::tasks::delete_task_comment,
        super::audit::list_audit,
    ),
    components(schemas(
        MeResponse,
        ProblemDetails,
        CreateGroupRequest,
        UpdateGroupRequest,
        CreateInviteRequest,
        SetRoleRequest,
        GroupResponse,
        GroupReadResponse,
        InviteResponse,
        MemberResponse,
        AcceptInviteResponse,
        CreateUploadRequest,
        CreateUploadResponse,
        CreateChatRequest,
        ChatResponse,
        ChatSummary,
        ChatListResponse,
        SendMessageRequest,
        MessageResponse,
        MessageSummary,
        MessageListResponse,
        CreateThreadRequest,
        ThreadResponse,
        CreateMemoryRequest,
        MemoryItemResponse,
        MemoryItemSummary,
        ListMemoryResponse,
        CreateTaskListRequest,
        TaskListResponse,
        TaskListSummary,
        ListTaskListsResponse,
        PatchTaskListRequest,
        CreateTaskRequest,
        TaskResponse,
        TaskSummary,
        ListTasksResponse,
        PatchTaskRequest,
        CreateCommentRequest,
        CommentResponse,
        ListCommentsResponse,
        AuditEventSummary,
        ListAuditResponse,
    )),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
