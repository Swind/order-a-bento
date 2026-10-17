//! `/public/g/{token}`: the participant-facing ordering flow (no login; the
//! unguessable share token is the credential).
//!
//! Every handler takes [`ClientCtx`] first: it rate-limits per client IP and
//! provides the IP / `User-Agent`. Handlers never log the token.

use application::SubmitOrderCmd;
use application::ports::EffectiveOrderView;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use domain::ordering::LineInput;
use serde::{Deserialize, Serialize};
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::common::{ordering_error, parse_id, time_text};
use super::group_orders::VendorSnapshotDto;
use crate::client::ClientCtx;
use crate::error::{AppError, ProblemDetails};
use crate::json::ApiJson;
use crate::openapi::TAG_PUBLIC;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicParticipantDto {
    pub id: String,
    pub name: String,
    /// The effective order has at least one line.
    pub has_ordered: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicMenuItemDto {
    pub id: String,
    pub name: String,
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
    pub sold_out: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicGroupDto {
    pub title: String,
    /// `open`, `closed` or `archived`.
    pub status: String,
    /// Open and not past the deadline.
    pub accepting_orders: bool,
    #[schema(format = "date-time")]
    pub deadline_at: Option<String>,
    pub note: Option<String>,
    pub vendor: VendorSnapshotDto,
    pub participants: Vec<PublicParticipantDto>,
    pub menu: Vec<PublicMenuItemDto>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrderLineDto {
    pub item_id: String,
    pub name: String,
    pub quantity: u32,
    pub unit_price: i64,
    pub subtotal: i64,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveOrderDto {
    pub submission_id: String,
    #[schema(format = "date-time")]
    pub submitted_at: String,
    /// The latest submission had no lines.
    pub cancelled: bool,
    pub note: Option<String>,
    pub total: i64,
    pub lines: Vec<OrderLineDto>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveOrderResponse {
    /// `null` when this participant never submitted.
    pub order: Option<EffectiveOrderDto>,
}

impl From<EffectiveOrderView> for EffectiveOrderDto {
    fn from(v: EffectiveOrderView) -> Self {
        Self {
            submission_id: v.submission_id.to_string(),
            submitted_at: time_text(v.submitted_at),
            cancelled: v.cancelled,
            note: v.note,
            total: v.total.get(),
            lines: v
                .lines
                .into_iter()
                .map(|l| OrderLineDto {
                    item_id: l.item_id.to_string(),
                    name: l.name,
                    quantity: l.quantity,
                    unit_price: l.unit_price.get(),
                    subtotal: l.subtotal.get(),
                    note: l.note,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitLineDto {
    pub item_id: String,
    /// 1..=20
    pub quantity: u32,
    pub note: Option<String>,
}

/// Client-reported (forgeable) device hints.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientHintsDto {
    pub device_id: Option<String>,
    pub device_model: Option<String>,
    pub screen: Option<String>,
    pub language: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitOrderRequest {
    /// At most 30 lines; empty cancels the order.
    pub lines: Vec<SubmitLineDto>,
    pub note: Option<String>,
    /// One per "send" press; a retry with the same key is a no-op.
    #[schema(format = "uuid")]
    pub idempotency_key: String,
    pub client: Option<ClientHintsDto>,
}

/// Parse the client-supplied idempotency key (422 `invalid_idempotencyKey`).
pub(super) fn parse_idempotency_key(s: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(s.trim()).map_err(|_| AppError::Problem {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "invalid_idempotencyKey".into(),
        detail: "idempotencyKey must be a UUID".into(),
        field: Some("idempotencyKey".into()),
        existing_id: None,
    })
}

pub(super) fn parse_lines(lines: Vec<SubmitLineDto>) -> Result<Vec<LineInput>, AppError> {
    lines
        .into_iter()
        .map(|l| {
            Ok(LineInput {
                offered_item_id: parse_id(&l.item_id)?,
                quantity: l.quantity,
                note: l.note,
            })
        })
        .collect()
}

const NOT_FOUND_DESC: &str = "Unknown or malformed share token, or unknown participant/item";

/// The group as participants see it.
#[utoipa::path(get, path = "/public/g/{token}", tag = TAG_PUBLIC,
    params(("token" = String, Path, description = "Share token")),
    responses(
        (status = 200, body = PublicGroupDto),
        (status = 404, description = NOT_FOUND_DESC, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 429, description = "Rate limited (code rate_limited)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_public_group(
    State(state): State<AppState>,
    _client: ClientCtx,
    Path(token): Path<String>,
) -> Result<Json<PublicGroupDto>, AppError> {
    let v = state
        .ordering
        .get_public_group(&token)
        .await
        .map_err(ordering_error)?;
    let g = &v.group;
    Ok(Json(PublicGroupDto {
        title: g.title().to_string(),
        status: g.status().as_str().into(),
        accepting_orders: v.accepting_orders,
        deadline_at: g.deadline_at().map(time_text),
        note: g.note().map(str::to_owned),
        vendor: VendorSnapshotDto {
            name: g.vendor().name.clone(),
            phone: g.vendor().phone.clone(),
            address: g.vendor().address.clone(),
        },
        participants: g
            .participants()
            .iter()
            .map(|p| PublicParticipantDto {
                id: p.id.to_string(),
                name: p.name.to_string(),
                has_ordered: v.ordered.contains(&p.id),
            })
            .collect(),
        menu: g
            .offered_items()
            .iter()
            .map(|i| PublicMenuItemDto {
                id: i.id.to_string(),
                name: i.name.to_string(),
                price: i.price.get(),
                description: i.description.clone(),
                category: i.category.clone(),
                sold_out: i.sold_out,
            })
            .collect(),
    }))
}

/// A participant's current (latest) order.
#[utoipa::path(get, path = "/public/g/{token}/participants/{pid}/order", tag = TAG_PUBLIC,
    params(
        ("token" = String, Path, description = "Share token"),
        ("pid" = String, Path, description = "Participant id")),
    responses(
        (status = 200, body = EffectiveOrderResponse),
        (status = 404, description = NOT_FOUND_DESC, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 429, description = "Rate limited (code rate_limited)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_effective_order(
    State(state): State<AppState>,
    _client: ClientCtx,
    Path((token, pid)): Path<(String, String)>,
) -> Result<Json<EffectiveOrderResponse>, AppError> {
    let order = state
        .ordering
        .get_effective_order(&token, parse_id(&pid)?)
        .await
        .map_err(ordering_error)?;
    Ok(Json(EffectiveOrderResponse {
        order: order.map(Into::into),
    }))
}

/// Submit (or replace, or cancel) a participant's order. Returns the effective
/// order afterwards; replaying an `idempotencyKey` inserts nothing.
#[utoipa::path(post, path = "/public/g/{token}/participants/{pid}/submissions", tag = TAG_PUBLIC,
    params(
        ("token" = String, Path, description = "Share token"),
        ("pid" = String, Path, description = "Participant id")),
    request_body = SubmitOrderRequest,
    responses(
        (status = 200, body = EffectiveOrderResponse),
        (status = 404, description = NOT_FOUND_DESC, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "group_closed, deadline_passed, or sold_out (field offeredItemId)", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "Validation failed: invalid_quantity, invalid_idempotencyKey, limit_exceeded, invalid_note or validation_failed", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 429, description = "Rate limited (code rate_limited)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn submit_order(
    State(state): State<AppState>,
    client: ClientCtx,
    Path((token, pid)): Path<(String, String)>,
    ApiJson(req): ApiJson<SubmitOrderRequest>,
) -> Result<Json<EffectiveOrderResponse>, AppError> {
    let idempotency_key = parse_idempotency_key(&req.idempotency_key)?;
    let participant_id = parse_id(&pid)?;
    let lines = parse_lines(req.lines)?;
    let mut info = client.client_info(&state);
    if let Some(h) = req.client {
        info.device_id = h.device_id;
        info.device_model = h.device_model;
        info.screen = h.screen;
        info.language = h.language;
        info.timezone = h.timezone;
    }
    // The domain sanitizes (`ClientInfo::sanitize`) when building the submission.
    let view = state
        .ordering
        .submit_order(SubmitOrderCmd {
            share_token: token,
            participant_id,
            lines,
            note: req.note,
            idempotency_key,
            client_info: info,
        })
        .await
        .map_err(ordering_error)?;
    Ok(Json(EffectiveOrderResponse {
        order: Some(view.into()),
    }))
}

/// Mounted under `/api/v1`. All responses are `noindex` and uncached.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_public_group))
        .routes(routes!(get_effective_order))
        .routes(routes!(submit_order))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("x-robots-tag"),
            HeaderValue::from_static("noindex"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
}
