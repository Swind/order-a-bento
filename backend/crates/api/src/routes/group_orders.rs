//! `/group-orders`: opening a group order and reading it back (organizer).

use application::ports::{GroupListItem, ParticipantRow, SubmissionHistoryEntry};
use application::{GroupSummary, OpenGroupCmd, OrganizerSubmitCmd, UpdateGroupCmd};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domain::ordering::{GroupOrder, GroupOrderStatus};
use domain::shared::Patch;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::common::{domain_error, double_option, ordering_error, parse_id, time_text};
use super::public::{EffectiveOrderResponse, SubmitLineDto, parse_idempotency_key, parse_lines};
use crate::error::{AppError, ProblemDetails};
use crate::json::ApiJson;
use crate::openapi::TAG_GROUP_ORDERS;
use crate::session::AuthenticatedOrganizer;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupOrderRequest {
    /// Omitted/null = personal. Any current member may create in this group;
    /// snapshot directory ownership is independent. Immutable after creation.
    pub sharing_group_id: Option<String>,
    pub delivery_address_id: Option<String>,
    pub title: String,
    pub roster_id: String,
    pub vendor_id: String,
    /// RFC 3339 timestamp; must be in the future.
    #[schema(format = "date-time")]
    pub deadline_at: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorSnapshotDto {
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantDto {
    pub id: String,
    pub name: String,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OfferedItemDto {
    pub id: String,
    pub name: String,
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
    pub sort_order: i32,
    pub sold_out: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupOrderDetail {
    /// Current actor's management permission; archived orders remain immutable.
    pub can_manage: bool,
    #[schema(required = true)]
    pub sharing_group_id: Option<String>,
    #[schema(required = true)]
    pub sharing_group_name: Option<String>,
    pub delivery_address: Option<String>,
    pub id: String,
    pub title: String,
    /// `open`, `closed` or `archived`.
    pub status: String,
    #[schema(format = "date-time")]
    pub deadline_at: Option<String>,
    pub note: Option<String>,
    /// `APP_BASE_URL/g/<token>`; the link to post in the chat.
    pub share_url: String,
    pub vendor: VendorSnapshotDto,
    pub participants: Vec<ParticipantDto>,
    pub offered_items: Vec<OfferedItemDto>,
    #[schema(format = "date-time")]
    pub created_at: String,
    #[schema(format = "date-time")]
    pub closed_at: Option<String>,
}

pub(crate) fn detail(g: &GroupOrder, share_url: String) -> GroupOrderDetail {
    GroupOrderDetail {
        can_manage: g.can_manage,
        sharing_group_id: g.sharing_group_id.map(|id| id.to_string()),
        sharing_group_name: g.sharing_group_name.clone(),
        delivery_address: g.delivery_address().map(str::to_owned),
        id: g.id().to_string(),
        title: g.title().to_string(),
        status: g.status().as_str().into(),
        deadline_at: g.deadline_at().map(time_text),
        note: g.note().map(str::to_owned),
        share_url,
        vendor: VendorSnapshotDto {
            name: g.vendor().name.clone(),
            phone: g.vendor().phone.clone(),
            address: g.vendor().address.clone(),
        },
        participants: g
            .participants()
            .iter()
            .map(|p| ParticipantDto {
                id: p.id.to_string(),
                name: p.name.to_string(),
                sort_order: p.sort_order,
            })
            .collect(),
        offered_items: g
            .offered_items()
            .iter()
            .map(|i| OfferedItemDto {
                id: i.id.to_string(),
                name: i.name.to_string(),
                price: i.price.get(),
                description: i.description.clone(),
                category: i.category.clone(),
                sort_order: i.sort_order,
                sold_out: i.sold_out,
            })
            .collect(),
        created_at: time_text(g.created_at()),
        closed_at: g.closed_at().map(time_text),
    }
}

fn parse_deadline(s: &str) -> Result<std::time::SystemTime, AppError> {
    infrastructure::timefmt::from_text(s.trim()).map_err(|_| AppError::Problem {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "invalid_deadlineAt".into(),
        detail: "deadlineAt must be an RFC 3339 timestamp".into(),
        field: Some("deadlineAt".into()),
        existing_id: None,
    })
}

/// Open a group order: snapshots the roster and the vendor's active menu.
#[utoipa::path(post, path = "/group-orders", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    request_body = CreateGroupOrderRequest,
    responses(
        (status = 201, body = GroupOrderDetail),
        (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "Roster or vendor not found (also for another organizer's)", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "Validation failed: invalid_<field> (title, deadlineAt, vendorId, rosterId...) or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn create_group_order(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    ApiJson(req): ApiJson<CreateGroupOrderRequest>,
) -> Result<(StatusCode, Json<GroupOrderDetail>), AppError> {
    let deadline_at = req.deadline_at.as_deref().map(parse_deadline).transpose()?;
    let group = state
        .ordering
        .open_group(
            o.id,
            OpenGroupCmd {
                sharing_group_id: req.sharing_group_id.as_deref().map(parse_id).transpose()?,
                delivery_address_id: req
                    .delivery_address_id
                    .as_deref()
                    .map(parse_id)
                    .transpose()?,
                title: req.title,
                roster_id: parse_id(&req.roster_id)?,
                vendor_id: parse_id(&req.vendor_id)?,
                deadline_at,
                note: req.note,
            },
        )
        .await
        .map_err(ordering_error)?;
    let url = state.ordering.share_url(&group);
    Ok((StatusCode::CREATED, Json(detail(&group, url))))
}

/// A personal order or a group-owned order accessible through current membership.
#[utoipa::path(get, path = "/group-orders/{id}", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(("id" = String, Path, description = "Group order id")),
    responses(
        (status = 200, body = GroupOrderDetail),
        (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "Not found (also for another organizer's group order)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_group_order(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<GroupOrderDetail>, AppError> {
    let group = state
        .ordering
        .get_group(o.id, parse_id(&id)?)
        .await
        .map_err(ordering_error)?;
    let url = state.ordering.share_url(&group);
    Ok(Json(detail(&group, url)))
}

// ---------------------------------------------------------------------------
// Management
// ---------------------------------------------------------------------------

/// Lifecycle status of a group order.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum GroupStatusDto {
    Open,
    Closed,
    Archived,
}

impl From<GroupStatusDto> for GroupOrderStatus {
    fn from(s: GroupStatusDto) -> Self {
        match s {
            GroupStatusDto::Open => Self::Open,
            GroupStatusDto::Closed => Self::Closed,
            GroupStatusDto::Archived => Self::Archived,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListGroupsQuery {
    /// Only groups in this status; absent lists all (archived included).
    #[param(value_type = Option<GroupStatusDto>)]
    pub status: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupListItemDto {
    pub can_manage: bool,
    #[schema(required = true)]
    pub sharing_group_id: Option<String>,
    #[schema(required = true)]
    pub sharing_group_name: Option<String>,
    pub id: String,
    pub title: String,
    pub status: GroupStatusDto,
    pub vendor_name: String,
    #[schema(format = "date-time")]
    pub deadline_at: Option<String>,
    #[schema(format = "date-time")]
    pub created_at: String,
    #[schema(format = "date-time")]
    pub closed_at: Option<String>,
    pub participant_count: u32,
    /// Participants whose effective order has at least one line.
    pub ordered_count: u32,
    /// Sum of the effective orders.
    pub total_amount: i64,
}

fn status_dto(s: GroupOrderStatus) -> GroupStatusDto {
    match s {
        GroupOrderStatus::Open => GroupStatusDto::Open,
        GroupOrderStatus::Closed => GroupStatusDto::Closed,
        GroupOrderStatus::Archived => GroupStatusDto::Archived,
    }
}

impl From<GroupListItem> for GroupListItemDto {
    fn from(g: GroupListItem) -> Self {
        Self {
            can_manage: g.can_manage,
            sharing_group_id: g.sharing_group_id.map(|id| id.to_string()),
            sharing_group_name: g.sharing_group_name,
            id: g.id.to_string(),
            title: g.title,
            status: status_dto(g.status),
            vendor_name: g.vendor_name,
            deadline_at: g.deadline_at.map(time_text),
            created_at: time_text(g.created_at),
            closed_at: g.closed_at.map(time_text),
            participant_count: g.participant_count,
            ordered_count: g.ordered_count,
            total_amount: g.total_amount.get(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchGroupOrderRequest {
    /// Absent = keep the snapshot; null = clear; id = snapshot this saved address.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub delivery_address_id: Option<Option<String>>,
    pub title: Option<String>,
    /// Absent = keep, `null` = clear, RFC 3339 = set (must be in the future).
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>, format = "date-time")]
    pub deadline_at: Option<Option<String>>,
    /// Absent = keep, `null` or blank = clear.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub note: Option<Option<String>>,
    /// open -> closed -> open/archived. Same status is a no-op; anything else is 422 `invalid_status`.
    pub status: Option<GroupStatusDto>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddParticipantRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetSoldOutRequest {
    pub sold_out: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryItemDto {
    pub item_id: String,
    pub name: String,
    pub price: i64,
    pub quantity: u32,
    /// Sum of `quantity * unitPrice` of the effective lines (stored prices).
    pub subtotal: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryParticipantsDto {
    pub total: u32,
    /// Effective order has at least one line.
    pub ordered: u32,
    pub not_ordered: u32,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummarySettlementDto {
    /// Participants marked picked up.
    pub picked_up: u32,
    /// Participants marked paid.
    pub paid: u32,
    /// Sum of the `paidAmount` recorded when each participant was marked paid.
    pub paid_amount: i64,
    /// `totalAmount` minus the current amount due of the participants marked paid:
    /// what is still expected from those who have not paid. A paid participant whose
    /// order changed afterwards contributes only their current due here; the
    /// difference (尚差/應退) is their per-participant `balance`.
    pub unpaid_amount: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummaryDto {
    pub group_order_id: String,
    pub status: GroupStatusDto,
    /// Every offered item in menu order (quantity 0 included).
    pub items: Vec<SummaryItemDto>,
    pub total_quantity: u32,
    pub total_amount: i64,
    pub participants: SummaryParticipantsDto,
    pub settlement: SummarySettlementDto,
}

impl From<GroupSummary> for GroupSummaryDto {
    fn from(s: GroupSummary) -> Self {
        Self {
            group_order_id: s.group_id.to_string(),
            status: status_dto(s.status),
            items: s
                .items
                .into_iter()
                .map(|i| SummaryItemDto {
                    item_id: i.item_id.to_string(),
                    name: i.name,
                    price: i.price.get(),
                    quantity: i.quantity,
                    subtotal: i.subtotal.get(),
                })
                .collect(),
            total_quantity: s.total_quantity,
            total_amount: s.total_amount.get(),
            participants: SummaryParticipantsDto {
                total: s.participants_total,
                ordered: s.participants_ordered,
                not_ordered: s.participants_total - s.participants_ordered,
            },
            settlement: SummarySettlementDto {
                picked_up: s.picked_up,
                paid: s.paid,
                paid_amount: s.paid_amount.get(),
                unpaid_amount: s.unpaid_amount.get(),
            },
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantItemDto {
    /// The offered item this line orders.
    pub item_id: String,
    pub name: String,
    pub quantity: u32,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantRowDto {
    pub id: String,
    pub name: String,
    pub sort_order: i32,
    /// Total of the effective order (0 when none or cancelled).
    pub amount_due: i64,
    pub items: Vec<ParticipantItemDto>,
    pub order_note: Option<String>,
    /// When the effective order was submitted; `null` if never.
    #[schema(format = "date-time")]
    pub submitted_at: Option<String>,
    /// Number of submissions including cancellations.
    pub submission_count: u32,
    pub picked_up: bool,
    #[schema(format = "date-time")]
    pub picked_up_at: Option<String>,
    pub paid: bool,
    #[schema(format = "date-time")]
    pub paid_at: Option<String>,
    /// Amount due recorded when marked paid.
    pub paid_amount: Option<i64>,
    /// `amountDue - paidAmount`: positive = 尚差, negative = 應退, 0 = settled; `null` when unpaid.
    pub balance: Option<i64>,
    /// A device id used by this participant was also used for another participant in this group.
    pub suspicious_device: bool,
    /// First 3 characters of the latest submission's device id.
    pub device_short: Option<String>,
}

impl From<ParticipantRow> for ParticipantRowDto {
    fn from(r: ParticipantRow) -> Self {
        Self {
            id: r.id.to_string(),
            name: r.name,
            sort_order: r.sort_order,
            amount_due: r.amount_due.get(),
            items: r
                .items
                .into_iter()
                .map(|i| ParticipantItemDto {
                    item_id: i.item_id.to_string(),
                    name: i.name,
                    quantity: i.quantity,
                    note: i.note,
                })
                .collect(),
            order_note: r.order_note,
            submitted_at: r.submitted_at.map(time_text),
            submission_count: r.submission_count,
            picked_up: r.settlement.picked_up_at().is_some(),
            picked_up_at: r.settlement.picked_up_at().map(time_text),
            paid: r.settlement.paid_at().is_some(),
            paid_at: r.settlement.paid_at().map(time_text),
            paid_amount: r.settlement.paid_amount().map(|m| m.get()),
            balance: r.settlement.balance(r.amount_due),
            suspicious_device: r.suspicious_device,
            device_short: r.device_short,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryLineDto {
    /// The offered item this line orders.
    pub item_id: String,
    pub name: String,
    pub quantity: u32,
    pub unit_price: i64,
    pub note: Option<String>,
}

/// Device metadata of a submission. The raw `User-Agent` and the full device id are never exposed.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientViewDto {
    pub ip: Option<String>,
    pub ua_summary: Option<String>,
    /// First 3 characters of the device id.
    pub device_short: Option<String>,
    pub device_model: Option<String>,
    pub screen: Option<String>,
    pub language: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionHistoryDto {
    pub id: String,
    #[schema(format = "date-time")]
    pub submitted_at: String,
    /// `participant` or `organizer`.
    pub submitted_by: String,
    pub cancelled: bool,
    pub total: i64,
    pub note: Option<String>,
    pub lines: Vec<HistoryLineDto>,
    pub client: ClientViewDto,
    /// This is the participant's latest submission (the one that counts).
    pub effective: bool,
    /// Names of other participants that used the same device id in this group.
    pub shared_device_with: Vec<String>,
}

impl From<SubmissionHistoryEntry> for SubmissionHistoryDto {
    fn from(e: SubmissionHistoryEntry) -> Self {
        Self {
            id: e.id.to_string(),
            submitted_at: time_text(e.submitted_at),
            submitted_by: e.submitted_by.as_str().into(),
            cancelled: e.cancelled,
            total: e.total.get(),
            note: e.note,
            lines: e
                .lines
                .into_iter()
                .map(|l| HistoryLineDto {
                    item_id: l.item_id.to_string(),
                    name: l.name,
                    quantity: l.quantity,
                    unit_price: l.unit_price.get(),
                    note: l.note,
                })
                .collect(),
            client: ClientViewDto {
                ip: e.client.ip,
                ua_summary: e.client.ua_summary,
                device_short: e.client.device_short,
                device_model: e.client.device_model,
                screen: e.client.screen,
                language: e.client.language,
                timezone: e.client.timezone,
            },
            effective: e.effective,
            shared_device_with: e.shared_device_with,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerSubmitRequest {
    /// At most 30 lines; empty cancels the order.
    pub lines: Vec<SubmitLineDto>,
    pub note: Option<String>,
    /// One per "send" press; a retry with the same key is a no-op.
    #[schema(format = "uuid")]
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettlementRequest {
    pub picked_up: bool,
    pub paid: bool,
}

const UNAUTH: &str = "Not logged in";
const NOT_FOUND: &str = "Not found (also for another organizer's group order)";

/// All accessible orders, newest first, with counts and totals (one query).
#[utoipa::path(get, path = "/group-orders", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(ListGroupsQuery),
    responses(
        (status = 200, body = Vec<GroupListItemDto>),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "invalid_status", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list_group_orders(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Query(q): Query<ListGroupsQuery>,
) -> Result<Json<Vec<GroupListItemDto>>, AppError> {
    let status = q
        .status
        .as_deref()
        .map(GroupOrderStatus::parse)
        .transpose()
        .map_err(domain_error)?;
    let list = state
        .ordering
        .list_groups(o.id, status)
        .await
        .map_err(ordering_error)?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

/// Edit title, deadline, note and/or status (close / reopen / archive).
#[utoipa::path(patch, path = "/group-orders/{id}", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(("id" = String, Path, description = "Group order id")),
    request_body = PatchGroupOrderRequest,
    responses(
        (status = 200, body = GroupOrderDetail),
        (status = 403, description = "Current member without management permission", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = NOT_FOUND, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "invalid_status (illegal transition, or edit of an archived group), invalid_title, invalid_deadlineAt, invalid_note or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn patch_group_order(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<PatchGroupOrderRequest>,
) -> Result<Json<GroupOrderDetail>, AppError> {
    let deadline_at = match req.deadline_at {
        None => Patch::Keep,
        Some(None) => Patch::Clear,
        Some(Some(s)) => Patch::Set(parse_deadline(&s)?),
    };
    let group = state
        .ordering
        .update_group(
            o.id,
            parse_id(&id)?,
            UpdateGroupCmd {
                delivery_address_id: match req.delivery_address_id {
                    None => Patch::Keep,
                    Some(None) => Patch::Clear,
                    Some(Some(id)) => Patch::Set(parse_id(&id)?),
                },
                title: req.title,
                deadline_at,
                note: Patch::from_option(req.note),
                status: req.status.map(Into::into),
            },
        )
        .await
        .map_err(ordering_error)?;
    let url = state.ordering.share_url(&group);
    Ok(Json(detail(&group, url)))
}

/// Replace the share token; the old link stops working immediately.
#[utoipa::path(post, path = "/group-orders/{id}/share-token/rotate", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(("id" = String, Path, description = "Group order id")),
    responses(
        (status = 200, description = "Detail with the new shareUrl", body = GroupOrderDetail),
        (status = 403, description = "Current member without management permission", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "Archived order is immutable", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = NOT_FOUND, body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn rotate_share_token(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<GroupOrderDetail>, AppError> {
    let group = state
        .ordering
        .rotate_share_token(o.id, parse_id(&id)?)
        .await
        .map_err(ordering_error)?;
    let url = state.ordering.share_url(&group);
    Ok(Json(detail(&group, url)))
}

/// Add an ad-hoc participant to this group only.
#[utoipa::path(post, path = "/group-orders/{id}/participants", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(("id" = String, Path, description = "Group order id")),
    request_body = AddParticipantRequest,
    responses(
        (status = 201, body = GroupOrderDetail),
        (status = 403, description = "Current member without management permission", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = NOT_FOUND, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "name_taken: the normalized name already exists in this group", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "invalid_name, invalid_status (archived) or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn add_participant(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<AddParticipantRequest>,
) -> Result<(StatusCode, Json<GroupOrderDetail>), AppError> {
    let group = state
        .ordering
        .add_participant(o.id, parse_id(&id)?, req.name)
        .await
        .map_err(ordering_error)?;
    let url = state.ordering.share_url(&group);
    Ok((StatusCode::CREATED, Json(detail(&group, url))))
}

/// Mark an offered item sold out (or available again). Sold-out items are refused on submit.
#[utoipa::path(patch, path = "/group-orders/{id}/offered-items/{itemId}", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(
        ("id" = String, Path, description = "Group order id"),
        ("itemId" = String, Path, description = "Offered item id")),
    request_body = SetSoldOutRequest,
    responses(
        (status = 200, body = GroupOrderDetail),
        (status = 403, description = "Current member without management permission", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "Group order or item not found", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "invalid_status (archived) or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn set_item_sold_out(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, item_id)): Path<(String, String)>,
    ApiJson(req): ApiJson<SetSoldOutRequest>,
) -> Result<Json<GroupOrderDetail>, AppError> {
    let group = state
        .ordering
        .set_sold_out(o.id, parse_id(&id)?, parse_id(&item_id)?, req.sold_out)
        .await
        .map_err(ordering_error)?;
    let url = state.ordering.share_url(&group);
    Ok(Json(detail(&group, url)))
}

/// Item totals and settlement figures over the effective (latest, non-cancelled)
/// orders only. Item `subtotal` uses each line's stored `unitPrice`.
#[utoipa::path(get, path = "/group-orders/{id}/summary", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(("id" = String, Path, description = "Group order id")),
    responses(
        (status = 200, body = GroupSummaryDto),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = NOT_FOUND, body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_summary(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<GroupSummaryDto>, AppError> {
    let s = state
        .ordering
        .summary(o.id, parse_id(&id)?)
        .await
        .map_err(ordering_error)?;
    Ok(Json(s.into()))
}

/// Every participant with their effective order, amount due, pickup / payment
/// state and device flags (two queries regardless of the group size).
#[utoipa::path(get, path = "/group-orders/{id}/participants", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(("id" = String, Path, description = "Group order id")),
    responses(
        (status = 200, body = Vec<ParticipantRowDto>),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = NOT_FOUND, body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list_participants(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<Vec<ParticipantRowDto>>, AppError> {
    let rows = state
        .ordering
        .list_participants(o.id, parse_id(&id)?)
        .await
        .map_err(ordering_error)?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// All submissions, newest first. Read-only members receive order history with
/// client metadata and shared-device associations redacted.
#[utoipa::path(get, path = "/group-orders/{id}/participants/{pid}/submissions", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(
        ("id" = String, Path, description = "Group order id"),
        ("pid" = String, Path, description = "Participant id")),
    responses(
        (status = 200, body = Vec<SubmissionHistoryDto>),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "Group order or participant not found", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list_submissions(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, pid)): Path<(String, String)>,
) -> Result<Json<Vec<SubmissionHistoryDto>>, AppError> {
    let rows = state
        .ordering
        .submission_history(o.id, parse_id(&id)?, parse_id(&pid)?)
        .await
        .map_err(ordering_error)?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// Submit, correct or cancel an order on behalf of a participant. Allowed while
/// the group is closed or past its deadline; refused when archived. Stores no
/// client info. Returns the effective order afterwards.
#[utoipa::path(post, path = "/group-orders/{id}/participants/{pid}/submissions", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(
        ("id" = String, Path, description = "Group order id"),
        ("pid" = String, Path, description = "Participant id")),
    request_body = OrganizerSubmitRequest,
    responses(
        (status = 200, body = EffectiveOrderResponse),
        (status = 403, description = "Current member without management permission", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "Group order, participant or item not found", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 409, description = "group_archived, or sold_out (field offeredItemId)", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "Validation failed: invalid_quantity, invalid_idempotencyKey, limit_exceeded, invalid_note or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn submit_for_participant(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, pid)): Path<(String, String)>,
    ApiJson(req): ApiJson<OrganizerSubmitRequest>,
) -> Result<Json<EffectiveOrderResponse>, AppError> {
    let idempotency_key = parse_idempotency_key(&req.idempotency_key)?;
    let view = state
        .ordering
        .submit_as_organizer(
            o.id,
            OrganizerSubmitCmd {
                group_id: parse_id(&id)?,
                participant_id: parse_id(&pid)?,
                lines: parse_lines(req.lines)?,
                note: req.note,
                idempotency_key,
            },
        )
        .await
        .map_err(ordering_error)?;
    Ok(Json(EffectiveOrderResponse {
        order: Some(view.into()),
    }))
}

/// Set the pickup / payment flags (idempotent set, not a toggle). Marking paid
/// records the current amount due as `paidAmount`; repeating keeps the original
/// timestamps and amount. Returns the participant row.
#[utoipa::path(put, path = "/group-orders/{id}/participants/{pid}/settlement", tag = TAG_GROUP_ORDERS, security(("cookieAuth" = [])),
    params(
        ("id" = String, Path, description = "Group order id"),
        ("pid" = String, Path, description = "Participant id")),
    request_body = SettlementRequest,
    responses(
        (status = 200, body = ParticipantRowDto),
        (status = 403, description = "Current member without management permission", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "Archived order is immutable", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = UNAUTH, body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "Group order or participant not found", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn put_settlement(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, pid)): Path<(String, String)>,
    ApiJson(req): ApiJson<SettlementRequest>,
) -> Result<Json<ParticipantRowDto>, AppError> {
    let row = state
        .ordering
        .set_settlement(
            o.id,
            parse_id(&id)?,
            parse_id(&pid)?,
            req.picked_up,
            req.paid,
        )
        .await
        .map_err(ordering_error)?;
    Ok(Json(row.into()))
}

/// Mounted under `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_group_orders, create_group_order))
        .routes(routes!(get_group_order, patch_group_order))
        .routes(routes!(rotate_share_token))
        .routes(routes!(list_participants, add_participant))
        .routes(routes!(set_item_sold_out))
        .routes(routes!(get_summary))
        .routes(routes!(list_submissions, submit_for_participant))
        .routes(routes!(put_settlement))
}
