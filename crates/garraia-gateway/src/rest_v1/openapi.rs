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

use super::groups::{
    CreateGroupRequest, CreateInviteRequest, GroupReadResponse, GroupResponse, InviteResponse,
    UpdateGroupRequest,
};
use super::invites::AcceptInviteResponse;
use super::me::MeResponse;
use super::problem::ProblemDetails;

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
        super::invites::accept_invite,
    ),
    components(schemas(
        MeResponse,
        ProblemDetails,
        CreateGroupRequest,
        UpdateGroupRequest,
        CreateInviteRequest,
        GroupResponse,
        GroupReadResponse,
        InviteResponse,
        AcceptInviteResponse,
    )),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
