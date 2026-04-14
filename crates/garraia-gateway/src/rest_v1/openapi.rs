//! OpenAPI 3.1 aggregator for the `/v1` surface (plan 0015).
//!
//! New endpoints go under `paths(...)` and their request/response DTOs
//! go under `components(schemas(...))`. The aggregated document is
//! exposed at `/v1/openapi.json` and rendered by Swagger UI at `/docs`.

use utoipa::OpenApi;

use super::me::MeResponse;
use super::problem::ProblemDetails;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "GarraIA REST /v1",
        version = "0.1.0",
        description = "Versioned GarraIA gateway REST surface (Fase 3.4)."
    ),
    paths(super::me::get_me),
    components(schemas(MeResponse, ProblemDetails))
)]
pub struct ApiDoc;
