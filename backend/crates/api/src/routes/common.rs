//! Helpers shared by the directory routes.

use std::time::SystemTime;

use application::{DirectoryError, OrderingError};
use axum::http::StatusCode;
use domain::shared::{DomainError, Id};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;

use crate::error::AppError;

/// Parse a path id; an invalid id can never exist, so it is a 404.
pub fn parse_id<T>(s: &str) -> Result<Id<T>, AppError> {
    s.parse()
        .map_err(|_| AppError::NotFound("resource not found".into()))
}

pub fn time_text(t: SystemTime) -> String {
    infrastructure::timefmt::to_text(t)
}

/// JSON merge-patch helper: absent -> `None`, `null` -> `Some(None)`, value -> `Some(Some(v))`.
pub fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

fn problem(
    status: StatusCode,
    code: String,
    detail: String,
    field: Option<&str>,
    existing_id: Option<String>,
) -> AppError {
    AppError::Problem {
        status,
        code,
        detail,
        field: field.map(str::to_owned),
        existing_id,
    }
}

pub fn domain_error(e: DomainError) -> AppError {
    let detail = e.to_string();
    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;
    match e {
        DomainError::Invalid { field, .. } => problem(
            unprocessable,
            format!("invalid_{field}"),
            detail,
            Some(field),
            None,
        ),
        DomainError::Duplicate { field, .. } => problem(
            unprocessable,
            format!("duplicate_{field}"),
            detail,
            Some(field),
            None,
        ),
        DomainError::LimitExceeded { field, .. } => problem(
            unprocessable,
            "limit_exceeded".into(),
            detail,
            Some(field),
            None,
        ),
        DomainError::Conflict { field, .. } => problem(
            StatusCode::CONFLICT,
            "name_taken".into(),
            detail,
            Some(field),
            None,
        ),
        DomainError::NotFound { .. } => AppError::NotFound(detail),
        DomainError::Closed { reason } => {
            problem(StatusCode::CONFLICT, reason.into(), detail, None, None)
        }
        DomainError::SoldOut { .. } => problem(
            StatusCode::CONFLICT,
            "sold_out".into(),
            detail,
            Some("offeredItemId"),
            None,
        ),
    }
}

pub fn directory_error(e: DirectoryError) -> AppError {
    match e {
        DirectoryError::NotFound => AppError::NotFound("resource not found".into()),
        DirectoryError::Domain(d) => domain_error(d),
        DirectoryError::VendorNameTaken { existing_id } => problem(
            StatusCode::CONFLICT,
            "vendor_name_taken".into(),
            "A vendor with this name already exists.".into(),
            Some("name"),
            existing_id.map(|i| i.to_string()),
        ),
        DirectoryError::Conflict => {
            AppError::Conflict("The resource was modified concurrently; retry.".into())
        }
        DirectoryError::Internal(d) => AppError::Internal(d),
    }
}

pub fn ordering_error(e: OrderingError) -> AppError {
    match e {
        OrderingError::NotFound => AppError::NotFound("resource not found".into()),
        OrderingError::Domain(d) => domain_error(d),
        OrderingError::Conflict => {
            AppError::Conflict("The resource was modified concurrently; retry.".into())
        }
        OrderingError::Internal(d) => AppError::Internal(d),
    }
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum OutcomeKind {
    Created,
    Updated,
    Unchanged,
}
