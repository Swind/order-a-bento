//! `/vendors`: vendor and menu management.

use super::sharing_groups::SharingScopeQuery;
use application::NewVendor;
use application::ports::{Modified, Stored, VendorSummary};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domain::directory::{
    MenuItemEdit, MenuItemId, MenuItemInput, UpsertOutcome, UpsertReport, Vendor, VendorId,
    VendorInfoEdit,
};
use domain::shared::Patch;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::common::{OutcomeKind, directory_error, double_option, parse_id, time_text};
use crate::error::{AppError, ProblemDetails};
use crate::json::ApiJson;
use crate::openapi::TAG_DIRECTORY;
use crate::session::AuthenticatedOrganizer;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorSummaryDto {
    pub sharing_group_id: Option<String>,
    pub can_manage: bool,
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub item_count: u32,
    pub updated_at: String,
}

impl From<VendorSummary> for VendorSummaryDto {
    fn from(s: VendorSummary) -> Self {
        Self {
            id: s.id.to_string(),
            sharing_group_id: s.sharing_group_id.map(|g| g.to_string()),
            can_manage: s.can_manage,
            name: s.name,
            phone: s.phone,
            item_count: s.item_count,
            updated_at: time_text(s.updated_at),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemDto {
    pub id: String,
    pub name: String,
    /// Whole TWD dollars.
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
    pub sort_order: i32,
    pub archived: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorDetail {
    #[schema(required = true)]
    pub image: Option<super::vendor_images::VendorImageDto>,
    pub sharing_group_id: Option<String>,
    pub can_manage: bool,
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
    pub updated_at: String,
    /// Sorted by `sortOrder`.
    pub menu_items: Vec<MenuItemDto>,
}

impl From<&Stored<Vendor>> for VendorDetail {
    fn from(s: &Stored<Vendor>) -> Self {
        let v = &s.value;
        // The aggregate keeps insertion order; the API lists by sortOrder.
        let mut menu: Vec<_> = v.menu().iter().collect();
        menu.sort_by_key(|m| m.sort_order);
        Self {
            id: v.id().to_string(),
            image: v.image.as_ref().map(Into::into),
            sharing_group_id: v.sharing_group_id.map(|g| g.to_string()),
            can_manage: v.can_manage,
            name: v.name().to_string(),
            phone: v.phone().map(str::to_owned),
            address: v.address().map(str::to_owned),
            note: v.note().map(str::to_owned),
            updated_at: time_text(s.updated_at),
            menu_items: menu
                .into_iter()
                .map(|m| MenuItemDto {
                    id: m.id.to_string(),
                    name: m.name.to_string(),
                    price: m.price.get(),
                    description: m.description.clone(),
                    category: m.category.clone(),
                    sort_order: m.sort_order,
                    archived: m.archived,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemOutcome {
    /// The item name as sent in the request.
    pub input: String,
    pub outcome: OutcomeKind,
    pub item_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemUpsertReport {
    pub created: u32,
    pub updated: u32,
    pub unchanged: u32,
    /// In request order.
    pub outcomes: Vec<ItemOutcome>,
}

fn report_dto(inputs: &[String], r: &UpsertReport<MenuItemId>) -> ItemUpsertReport {
    ItemUpsertReport {
        created: r.created() as u32,
        updated: r.updated() as u32,
        unchanged: r.unchanged() as u32,
        outcomes: inputs
            .iter()
            .zip(&r.outcomes)
            .map(|(input, o)| {
                let (outcome, id) = match o {
                    UpsertOutcome::Created(id) => (OutcomeKind::Created, id),
                    UpsertOutcome::Updated(id) => (OutcomeKind::Updated, id),
                    UpsertOutcome::Unchanged(id) => (OutcomeKind::Unchanged, id),
                };
                ItemOutcome {
                    input: input.clone(),
                    outcome,
                    item_id: id.to_string(),
                }
            })
            .collect(),
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemInputDto {
    pub name: String,
    /// Whole TWD dollars, 1..=100000.
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
}

fn inputs(items: Vec<MenuItemInputDto>) -> (Vec<String>, Vec<MenuItemInput>) {
    let names = items.iter().map(|i| i.name.clone()).collect();
    let inputs = items
        .into_iter()
        .map(|i| MenuItemInput {
            name: i.name,
            price: i.price,
            description: i.description,
            category: i.category,
        })
        .collect();
    (names, inputs)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateVendorRequest {
    pub sharing_group_id: Option<String>,
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub menu_items: Vec<MenuItemInputDto>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedVendor {
    #[serde(flatten)]
    pub vendor: VendorDetail,
    /// Present when `menuItems` were given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<ItemUpsertReport>,
}

/// Merge-patch: absent = keep, `null` = clear, string = set.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchVendorRequest {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub address: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub note: Option<Option<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertItemsRequest {
    pub items: Vec<MenuItemInputDto>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorUpsertResponse {
    pub vendor: VendorDetail,
    pub report: ItemUpsertReport,
}

/// Merge-patch: absent = keep, `null` = clear (description/category only).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchItemRequest {
    pub name: Option<String>,
    pub price: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub category: Option<Option<String>>,
    pub archived: Option<bool>,
    pub sort_order: Option<i32>,
}

fn vid(s: &str) -> Result<VendorId, AppError> {
    parse_id(s)
}

/// List vendors, newest updated first.
#[utoipa::path(get, path = "/vendors", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    params(SharingScopeQuery),
    responses((status = 200, body = Vec<VendorSummaryDto>), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list_vendors(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Query(scope): Query<SharingScopeQuery>,
) -> Result<Json<Vec<VendorSummaryDto>>, AppError> {
    let list = state
        .directory
        .vendors
        .list_scoped(o.id, scope.group()?)
        .await
        .map_err(|e| directory_error(e.into()))?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

/// Create a vendor. A vendor with the same normalized name yields 409
/// `vendor_name_taken` carrying `existingId`.
#[utoipa::path(post, path = "/vendors", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    request_body = CreateVendorRequest,
    responses((status = 201, body = CreatedVendor), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "Name conflict: name_taken, vendor_name_taken (with existingId) or conflict", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn create_vendor(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    ApiJson(req): ApiJson<CreateVendorRequest>,
) -> Result<(StatusCode, Json<CreatedVendor>), AppError> {
    let (names, menu_items) = inputs(req.menu_items);
    let Modified { stored, report } = state
        .directory
        .create_vendor_scoped(
            o.id,
            req.sharing_group_id.as_deref().map(parse_id).transpose()?,
            NewVendor {
                name: req.name,
                phone: req.phone,
                address: req.address,
                note: req.note,
                menu_items,
            },
        )
        .await
        .map_err(directory_error)?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedVendor {
            vendor: (&stored).into(),
            report: report.map(|r| report_dto(&names, &r)),
        }),
    ))
}

/// Vendor with its menu.
#[utoipa::path(get, path = "/vendors/{vendorId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("vendorId" = String, Path, description = "Vendor id")),
    responses((status = 200, body = VendorDetail), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_vendor(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<VendorDetail>, AppError> {
    let stored = state
        .directory
        .get_vendor(o.id, vid(&id)?)
        .await
        .map_err(directory_error)?;
    Ok(Json((&stored).into()))
}

/// Manually edit vendor info (merge-patch; `null` clears a field).
#[utoipa::path(patch, path = "/vendors/{vendorId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("vendorId" = String, Path, description = "Vendor id")),
    request_body = PatchVendorRequest,
    responses((status = 200, body = VendorDetail), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "Name conflict: name_taken, vendor_name_taken (with existingId) or conflict", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn patch_vendor(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<PatchVendorRequest>,
) -> Result<Json<VendorDetail>, AppError> {
    let edit = VendorInfoEdit {
        name: req.name,
        phone: Patch::from_option(req.phone),
        address: Patch::from_option(req.address),
        note: Patch::from_option(req.note),
    };
    let m = state
        .directory
        .edit_vendor(o.id, vid(&id)?, edit)
        .await
        .map_err(directory_error)?;
    Ok(Json((&m.stored).into()))
}

/// Delete a vendor and its menu.
#[utoipa::path(delete, path = "/vendors/{vendorId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("vendorId" = String, Path, description = "Vendor id")),
    responses((status = 204, description = "Deleted"), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete_vendor(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .directory
        .delete_vendor(o.id, vid(&id)?)
        .await
        .map_err(directory_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Upsert menu items by name (price always overwrites; an archived item is revived).
#[utoipa::path(post, path = "/vendors/{vendorId}/menu-items/upsert", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("vendorId" = String, Path, description = "Vendor id")),
    request_body = UpsertItemsRequest,
    responses((status = 200, body = VendorUpsertResponse), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn upsert_menu_items(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<UpsertItemsRequest>,
) -> Result<Json<VendorUpsertResponse>, AppError> {
    let (names, items) = inputs(req.items);
    let m = state
        .directory
        .upsert_menu_items(o.id, vid(&id)?, items)
        .await
        .map_err(directory_error)?;
    let report = m
        .report
        .ok_or_else(|| AppError::Internal("upsert returned no report".into()))?;
    Ok(Json(VendorUpsertResponse {
        vendor: (&m.stored).into(),
        report: report_dto(&names, &report),
    }))
}

/// Manually edit one menu item (merge-patch).
#[utoipa::path(patch, path = "/vendors/{vendorId}/menu-items/{itemId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("vendorId" = String, Path, description = "Vendor id"),
           ("itemId" = String, Path, description = "Menu item id")),
    request_body = PatchItemRequest,
    responses((status = 200, body = VendorDetail), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "Name conflict: name_taken, vendor_name_taken (with existingId) or conflict", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn patch_menu_item(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, item)): Path<(String, String)>,
    ApiJson(req): ApiJson<PatchItemRequest>,
) -> Result<Json<VendorDetail>, AppError> {
    let edit = MenuItemEdit {
        name: req.name,
        price: req.price,
        description: Patch::from_option(req.description),
        category: Patch::from_option(req.category),
        archived: req.archived,
        sort_order: req.sort_order,
    };
    let m = state
        .directory
        .edit_menu_item(o.id, vid(&id)?, parse_id(&item)?, edit)
        .await
        .map_err(directory_error)?;
    Ok(Json((&m.stored).into()))
}

/// Delete one menu item.
#[utoipa::path(delete, path = "/vendors/{vendorId}/menu-items/{itemId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("vendorId" = String, Path, description = "Vendor id"),
           ("itemId" = String, Path, description = "Menu item id")),
    responses((status = 204, description = "Deleted"), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete_menu_item(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, item)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    state
        .directory
        .remove_menu_item(o.id, vid(&id)?, parse_id(&item)?)
        .await
        .map_err(directory_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Mounted under `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(super::vendor_images::router())
        .routes(routes!(list_vendors, create_vendor))
        .routes(routes!(get_vendor, patch_vendor, delete_vendor))
        .routes(routes!(upsert_menu_items))
        .routes(routes!(patch_menu_item, delete_menu_item))
}
