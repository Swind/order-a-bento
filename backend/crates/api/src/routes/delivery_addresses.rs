use super::common::{directory_error, parse_id, time_text};
use crate::{
    error::{AppError, ProblemDetails},
    json::ApiJson,
    openapi::TAG_DIRECTORY,
    session::AuthenticatedOrganizer,
    state::AppState,
};
use application::ports::Stored;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use domain::directory::DeliveryAddress;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryAddressDto {
    pub id: String,
    pub name: String,
    pub address: String,
    pub updated_at: String,
}

impl From<Stored<DeliveryAddress>> for DeliveryAddressDto {
    fn from(s: Stored<DeliveryAddress>) -> Self {
        Self {
            id: s.value.id.to_string(),
            name: s.value.name,
            address: s.value.address,
            updated_at: time_text(s.updated_at),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SaveDeliveryAddressRequest {
    pub name: String,
    pub address: String,
}

#[utoipa::path(get, path = "/delivery-addresses", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    responses((status = 200, body = Vec<DeliveryAddressDto>), (status = 401, body = ProblemDetails)))]
pub async fn list(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
) -> Result<Json<Vec<DeliveryAddressDto>>, AppError> {
    let list = state
        .directory
        .delivery_addresses
        .list(o.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

#[utoipa::path(post, path = "/delivery-addresses", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    request_body = SaveDeliveryAddressRequest,
    responses((status = 201, body = DeliveryAddressDto), (status = 401, body = ProblemDetails), (status = 422, body = ProblemDetails)))]
pub async fn create(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    ApiJson(req): ApiJson<SaveDeliveryAddressRequest>,
) -> Result<(StatusCode, Json<DeliveryAddressDto>), AppError> {
    let saved = state
        .directory
        .save_delivery_address(o.id, None, &req.name, &req.address)
        .await
        .map_err(directory_error)?;
    Ok((StatusCode::CREATED, Json(saved.into())))
}

#[utoipa::path(put, path = "/delivery-addresses/{id}", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    params(("id" = String, Path)), request_body = SaveDeliveryAddressRequest,
    responses((status = 200, body = DeliveryAddressDto), (status = 401, body = ProblemDetails), (status = 404, body = ProblemDetails), (status = 422, body = ProblemDetails)))]
pub async fn update(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<SaveDeliveryAddressRequest>,
) -> Result<Json<DeliveryAddressDto>, AppError> {
    let saved = state
        .directory
        .save_delivery_address(o.id, Some(parse_id(&id)?), &req.name, &req.address)
        .await
        .map_err(directory_error)?;
    Ok(Json(saved.into()))
}

#[utoipa::path(delete, path = "/delivery-addresses/{id}", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    params(("id" = String, Path)),
    responses((status = 204), (status = 401, body = ProblemDetails), (status = 404, body = ProblemDetails)))]
pub async fn delete(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if !state
        .directory
        .delivery_addresses
        .delete(o.id, parse_id(&id)?)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    {
        return Err(AppError::NotFound("delivery address not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, create))
        .routes(routes!(update, delete))
}
