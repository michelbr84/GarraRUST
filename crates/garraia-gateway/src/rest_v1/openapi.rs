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
        let components = openapi
            .components
            .as_mut()
            .expect("ApiDoc always has components");
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
    paths(super::me::get_me),
    components(schemas(MeResponse, ProblemDetails)),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
