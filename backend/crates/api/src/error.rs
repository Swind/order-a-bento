//! RFC 7807 `application/problem+json` error responses.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const PROBLEM_JSON: &str = "application/problem+json";

/// Body of an error response (RFC 7807).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProblemDetails {
    /// URI reference identifying the problem type.
    #[serde(rename = "type")]
    #[schema(rename = "type", example = "about:blank")]
    pub type_: String,
    #[schema(example = "Not Found")]
    pub title: String,
    #[schema(example = 404)]
    pub status: u16,
    pub detail: String,
    /// Stable machine-readable error code.
    #[schema(example = "not_found")]
    pub code: String,
    /// Offending request field (validation and conflict errors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Id of the existing record that caused a uniqueness conflict.
    #[serde(
        default,
        rename = "existingId",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(rename = "existingId")]
    pub existing_id: Option<String>,
    /// Id of the AI import run that failed (so the UI can still show it).
    #[serde(default, rename = "runId", skip_serializing_if = "Option::is_none")]
    #[schema(rename = "runId")]
    pub run_id: Option<String>,
    /// Per-entry validation errors (apply of an AI import).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<ProblemFieldError>>,
}

/// One entry of [`ProblemDetails::errors`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProblemFieldError {
    /// Location in the request, e.g. `accepted[0].menu[2].price`.
    pub path: String,
    pub message: String,
}

/// Application-level error mapped to an HTTP problem response.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("internal error: {0}")]
    Internal(String),
    /// Arbitrary status with a custom machine-readable code (e.g. `bad_origin`).
    #[error("{code}: {detail}")]
    Coded {
        status: StatusCode,
        code: &'static str,
        detail: String,
    },
    /// Like `Coded` with a dynamic code and optional extension members.
    #[error("{code}: {detail}")]
    Problem {
        status: StatusCode,
        code: String,
        detail: String,
        field: Option<String>,
        existing_id: Option<String>,
    },
    /// A problem with the `runId` / `errors` extension members.
    #[error("{code}: {detail}")]
    Extended {
        status: StatusCode,
        code: &'static str,
        detail: String,
        run_id: Option<String>,
        errors: Vec<ProblemFieldError>,
    },
}

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Coded { status, .. }
            | Self::Problem { status, .. }
            | Self::Extended { status, .. } => *status,
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Validation(_) => "validation_failed",
            Self::Conflict(_) => "conflict",
            Self::Internal(_) => "internal_error",
            Self::Coded { code, .. } | Self::Extended { code, .. } => code,
            Self::Problem { code, .. } => code,
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::NotFound(d)
            | Self::Unauthorized(d)
            | Self::Forbidden(d)
            | Self::Validation(d)
            | Self::Conflict(d)
            | Self::Internal(d) => d,
            Self::Coded { detail, .. }
            | Self::Problem { detail, .. }
            | Self::Extended { detail, .. } => detail,
        }
    }

    pub fn to_problem(&self) -> ProblemDetails {
        let status = self.status();
        // Never leak internal details to clients.
        let detail = if matches!(self, Self::Internal(_)) {
            "An internal error occurred.".to_string()
        } else {
            self.detail().to_string()
        };
        ProblemDetails {
            type_: "about:blank".into(),
            title: status.canonical_reason().unwrap_or("Error").into(),
            status: status.as_u16(),
            detail,
            code: self.code().into(),
            field: match self {
                Self::Problem { field, .. } => field.clone(),
                _ => None,
            },
            existing_id: match self {
                Self::Problem { existing_id, .. } => existing_id.clone(),
                _ => None,
            },
            run_id: match self {
                Self::Extended { run_id, .. } => run_id.clone(),
                _ => None,
            },
            errors: match self {
                Self::Extended { errors, .. } if !errors.is_empty() => Some(errors.clone()),
                _ => None,
            },
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(d) = &self {
            tracing::error!(detail = %d, "internal error");
        }
        let status = self.status();
        let mut res = (status, Json(self.to_problem())).into_response();
        res.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
        res
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn renders_problem_json() {
        let res = AppError::Validation("bad name".into()).into_response();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(res.headers()[header::CONTENT_TYPE], PROBLEM_JSON);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["type"], "about:blank");
        assert_eq!(v["status"], 422);
        assert_eq!(v["code"], "validation_failed");
        assert_eq!(v["detail"], "bad name");
    }

    #[tokio::test]
    async fn internal_hides_detail() {
        let res = AppError::Internal("secret".into()).into_response();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&body).contains("secret"));
    }

    #[tokio::test]
    async fn extension_members_are_optional_and_camel_case() {
        let res = AppError::Problem {
            status: StatusCode::CONFLICT,
            code: "vendor_name_taken".into(),
            detail: "x".into(),
            field: Some("name".into()),
            existing_id: Some("abc".into()),
        }
        .into_response();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "vendor_name_taken");
        assert_eq!(v["field"], "name");
        assert_eq!(v["existingId"], "abc");
        let res = AppError::NotFound("x".into()).into_response();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v.get("field").is_none() && v.get("existingId").is_none());
    }

    #[test]
    fn statuses() {
        assert_eq!(AppError::NotFound(String::new()).status(), 404);
        assert_eq!(AppError::Unauthorized(String::new()).status(), 401);
        assert_eq!(AppError::Forbidden(String::new()).status(), 403);
        assert_eq!(AppError::Conflict(String::new()).status(), 409);
    }
}
