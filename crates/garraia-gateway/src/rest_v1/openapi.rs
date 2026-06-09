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
use super::chats::{
    ChatListResponse, ChatMemberDetailResponse, ChatResponse, ChatSummary, CreateChatRequest,
    PatchChatMemberRequest, PatchThreadRequest, ThreadDetailResponse,
};
use super::files::{
    CreateFolderRequest, FileCreatedResponse, FileListResponse, FileSummary,
    FileVersionListResponse, FileVersionResponse, FileVersionSummary, FolderListResponse,
    FolderSummary, PatchFileRequest, PatchFolderRequest,
};
use super::groups::{
    CreateGroupRequest, CreateInviteRequest, GroupReadResponse, GroupResponse, InviteResponse,
    InviteSummary, ListInvitesResponse, MemberResponse, SetRoleRequest, UpdateGroupRequest,
};
use super::invites::AcceptInviteResponse;
use super::me::{
    AcceptMyInviteResponse, ChatMembershipSummary, MeResponse, MentionSummary,
    MentionsListResponse, MyChatsMembershipResponse, MyFileSummary, MyFilesResponse,
    MyInvitesResponse, MyMemoryResponse, MyMemorySummary, MyReactionSummary, MyReactionsResponse,
    MyThreadSummary, MyThreadsResponse, PatchMeRequest, PatchMeResponse, PendingInviteSummary,
    TaskAssignmentSummary, TasksListResponse,
};
use super::memory::{
    CreateMemoryRequest, ListMemoryResponse, MemoryItemResponse, MemoryItemSummary,
    PatchMemoryRequest, PinMemoryResponse,
};
use super::messages::{
    CreateThreadRequest, MessageListResponse, MessageResponse, MessageSummary, SendMessageRequest,
    ThreadMessagesResponse, ThreadResponse,
};
use super::problem::ProblemDetails;
use super::search::{SearchResponse, SearchResult, SearchResultType};
use super::tasks::{
    AddAssigneeRequest, AssignTaskLabelRequest, AssigneeResponse, CommentResponse,
    CreateCommentRequest, CreateTaskLabelRequest, CreateTaskListRequest, CreateTaskRequest,
    EditCommentRequest, EditedCommentResponse, LabelAssignmentResponse, ListCommentsResponse,
    ListSubtasksResponse, ListTaskListsResponse, ListTasksResponse, MoveTaskRequest,
    PatchSubscriptionRequest, PatchTaskLabelRequest, PatchTaskListRequest, PatchTaskRequest,
    SubscriptionResponse, TaskLabelResponse, TaskListResponse, TaskListSummary, TaskResponse,
    TaskSummary,
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
        super::me::patch_me,
        super::me::list_my_mentions,
        super::me::list_my_tasks,
        super::me::list_my_chats,
        super::me::list_my_files,
        super::me::list_my_memory,
        super::me::list_my_invites,
        super::me::decline_invite,
        super::me::accept_my_invite,
        super::me::list_my_reactions,
        super::me::list_my_threads,
        super::groups::create_group,
        super::groups::list_groups,
        super::groups::get_group,
        super::groups::patch_group,
        super::groups::create_invite,
        super::groups::list_invites,
        super::groups::get_invite,
        super::groups::revoke_invite,
        super::groups::list_members,
        super::groups::get_member,
        super::groups::set_member_role,
        super::groups::delete_member,
        super::invites::accept_invite,
        super::uploads::create_upload,
        super::uploads::head_upload,
        super::uploads::patch_upload,
        super::uploads::options_uploads,
        super::chats::create_chat,
        super::chats::list_chats,
        super::chats::get_thread,
        super::chats::patch_thread,
        super::chats::patch_chat_member,
        super::chats::typing_indicator,
        super::messages::send_message,
        super::messages::list_messages,
        super::messages::create_thread,
        super::messages::get_message,
        super::messages::list_thread_messages,
        super::memory::list_memory,
        super::memory::create_memory,
        super::memory::delete_memory,
        super::memory::get_memory,
        super::memory::patch_memory,
        super::memory::pin_memory,
        super::memory::unpin_memory,
        super::tasks::task_lists::create_task_list,
        super::tasks::task_lists::list_task_lists,
        super::tasks::task_lists::get_task_list,
        super::tasks::task_lists::patch_task_list,
        super::tasks::task_lists::delete_task_list,
        super::tasks::create_task,
        super::tasks::list_tasks,
        super::tasks::get_task,
        super::tasks::patch_task,
        super::tasks::delete_task,
        super::tasks::comments::create_task_comment,
        super::tasks::comments::list_task_comments,
        super::tasks::comments::get_task_comment,
        super::tasks::comments::delete_task_comment,
        super::tasks::comments::patch_task_comment,
        super::tasks::assignees::add_task_assignee,
        super::tasks::assignees::list_task_assignees,
        super::tasks::assignees::remove_task_assignee,
        super::tasks::labels::create_task_label,
        super::tasks::labels::list_task_labels,
        super::tasks::labels::get_task_label,
        super::tasks::labels::delete_task_label,
        super::tasks::labels::patch_task_label,
        super::tasks::labels::assign_task_label,
        super::tasks::labels::list_task_label_assignments,
        super::tasks::labels::remove_task_label_from_task,
        super::tasks::subscriptions::subscribe_to_task,
        super::tasks::subscriptions::list_task_subscriptions,
        super::tasks::subscriptions::unsubscribe_from_task,
        super::tasks::subscriptions::patch_task_subscription,
        super::tasks::move_task,
        super::tasks::list_subtasks,
        super::audit::list_audit,
        super::search::search,
        super::files::list_files,
        super::files::create_file,
        super::files::list_folders,
        super::files::delete_file,
        super::files::patch_file,
        super::files::get_file,
        super::files::get_folder,
        super::files::patch_folder,
        super::files::create_folder,
        super::files::delete_folder,
        super::files::download_file,
        super::files::list_file_versions,
        super::files::post_new_version,
    ),
    components(schemas(
        MeResponse,
        MentionSummary,
        MentionsListResponse,
        MyChatsMembershipResponse,
        ChatMembershipSummary,
        MyFilesResponse,
        MyFileSummary,
        MyMemoryResponse,
        MyMemorySummary,
        MyInvitesResponse,
        PendingInviteSummary,
        AcceptMyInviteResponse,
        MyReactionsResponse,
        MyReactionSummary,
        MyThreadsResponse,
        MyThreadSummary,
        PatchMeRequest,
        PatchMeResponse,
        TaskAssignmentSummary,
        TasksListResponse,
        ProblemDetails,
        CreateGroupRequest,
        UpdateGroupRequest,
        CreateInviteRequest,
        SetRoleRequest,
        GroupResponse,
        GroupReadResponse,
        InviteResponse,
        InviteSummary,
        ListInvitesResponse,
        MemberResponse,
        AcceptInviteResponse,
        CreateUploadRequest,
        CreateUploadResponse,
        CreateChatRequest,
        ChatResponse,
        ChatSummary,
        ChatListResponse,
        PatchThreadRequest,
        ThreadDetailResponse,
        PatchChatMemberRequest,
        ChatMemberDetailResponse,
        SendMessageRequest,
        MessageResponse,
        MessageSummary,
        MessageListResponse,
        CreateThreadRequest,
        ThreadResponse,
        ThreadMessagesResponse,
        CreateMemoryRequest,
        PatchMemoryRequest,
        MemoryItemResponse,
        MemoryItemSummary,
        ListMemoryResponse,
        PinMemoryResponse,
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
        EditCommentRequest,
        EditedCommentResponse,
        AddAssigneeRequest,
        AssigneeResponse,
        CreateTaskLabelRequest,
        PatchTaskLabelRequest,
        TaskLabelResponse,
        AssignTaskLabelRequest,
        LabelAssignmentResponse,
        SubscriptionResponse,
        PatchSubscriptionRequest,
        MoveTaskRequest,
        ListSubtasksResponse,
        AuditEventSummary,
        ListAuditResponse,
        SearchResult,
        SearchResponse,
        SearchResultType,
        FileSummary,
        FileListResponse,
        FileCreatedResponse,
        FileVersionResponse,
        FolderSummary,
        FolderListResponse,
        PatchFileRequest,
        PatchFolderRequest,
        CreateFolderRequest,
        FileVersionSummary,
        FileVersionListResponse,
    )),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
