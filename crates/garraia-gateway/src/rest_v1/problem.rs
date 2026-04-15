//! RFC 9457 Problem Details for the `/v1` surface.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

/// RFC 9457 Problem Details body.
///
/// `type` defaults to `about:blank`, which per the RFC means the only
/// semantic information is carried by `status` + `title`. Future slices
/// can upgrade to concrete `type` URIs pointing at a public error
/// taxonomy.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub type_uri: &'static str,
    pub title: &'static str,
    pub status: u16,
    pub detail: String,
}

/// Canonical error type for the `/v1` surface.
///
/// Every variant maps to exactly one HTTP status + Problem Details body.
/// New variants must be added here — handlers never hand-roll responses.
#[derive(Debug, Error)]
pub enum RestError {
    #[error("authentication required")]
    Unauthenticated,
    #[error("forbidden")]
    Forbidden,
    /// Plan 0016 M4: request body fails validation (empty name,
    /// unknown enum value, header/path mismatch). The `{0}` detail
    /// is emitted to clients in the Problem Details body, so callers
    /// MUST NOT embed user-identifying data in it — write only
    /// structural errors like `"invalid group type"`, not
    /// `"user alice@example.com cannot pick ..."`.
    #[error("{0}")]
    BadRequest(String),
    /// Plan 0016 M4: resource missing (e.g. group deleted between
    /// extractor lookup and handler query). No payload — clients
    /// only see status 404 + title "Not Found".
    #[error("not found")]
    NotFound,
    #[error("authentication is not configured on this gateway")]
    AuthUnconfigured,
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
}

impl RestError {
    fn status(&self) -> StatusCode {
        match self {
            RestError::Unauthenticated => StatusCode::UNAUTHORIZED,
            RestError::Forbidden => StatusCode::FORBIDDEN,
            RestError::BadRequest(_) => StatusCode::BAD_REQUEST,
            RestError::NotFound => StatusCode::NOT_FOUND,
            RestError::AuthUnconfigured => StatusCode::SERVICE_UNAVAILABLE,
            RestError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            RestError::Unauthenticated => "Unauthenticated",
            RestError::Forbidden => "Forbidden",
            RestError::BadRequest(_) => "Bad Request",
            RestError::NotFound => "Not Found",
            RestError::AuthUnconfigured => "Service Unavailable",
            RestError::Internal(_) => "Internal Server Error",
        }
    }
}

impl IntoResponse for RestError {
    fn into_response(self) -> Response {
        let status = self.status();
        let detail = self.to_string();
        // Log internal errors before dropping the source. PII-safe:
        // `Display` on `RestError::Internal` returns the static string
        // "internal error", never the underlying anyhow::Error body.
        if let RestError::Internal(ref e) = self {
            tracing::error!(error = %e, "rest_v1 internal error");
        }
        let body = ProblemDetails {
            type_uri: "about:blank",
            title: self.title(),
            status: status.as_u16(),
            detail,
        };
        let json = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
        (
            status,
            [("content-type", "application/problem+json")],
            json,
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn unauthenticated_serializes_to_rfc9457_shape() {
        let resp = RestError::Unauthenticated.into_response();
        assert_eq!(resp.status(), 401);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/problem+json",
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["type"], "about:blank");
        assert_eq!(v["title"], "Unauthenticated");
        assert_eq!(v["status"], 401);
        assert!(v["detail"].is_string());
    }

    #[tokio::test]
    async fn service_unavailable_shape() {
        let resp = RestError::AuthUnconfigured.into_response();
        assert_eq!(resp.status(), 503);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], 503);
        assert_eq!(v["title"], "Service Unavailable");
    }

    #[tokio::test]
    async fn bad_request_shape_carries_detail() {
        let resp = RestError::BadRequest("invalid group type".into()).into_response();
        assert_eq!(resp.status(), 400);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], 400);
        assert_eq!(v["title"], "Bad Request");
        assert_eq!(v["detail"], "invalid group type");
    }

    #[tokio::test]
    async fn not_found_shape() {
        let resp = RestError::NotFound.into_response();
        assert_eq!(resp.status(), 404);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], 404);
        assert_eq!(v["title"], "Not Found");
        assert_eq!(v["detail"], "not found");
    }
}
