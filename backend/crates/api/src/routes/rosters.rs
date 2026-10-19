//! `/rosters`: roster and member management.

use super::sharing_groups::SharingScopeQuery;
use application::ports::{Modified, RosterSummary, Stored};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domain::directory::{
    MemberEdit, MemberInput, Roster, RosterId, RosterMemberId, UpsertOutcome, UpsertReport,
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
pub struct RosterSummaryDto {
    pub sharing_group_id: Option<String>,
    pub can_manage: bool,
    pub id: String,
    pub name: String,
    pub member_count: u32,
    pub updated_at: String,
}

impl From<RosterSummary> for RosterSummaryDto {
    fn from(s: RosterSummary) -> Self {
        Self {
            id: s.id.to_string(),
            sharing_group_id: s.sharing_group_id.map(|g| g.to_string()),
            can_manage: s.can_manage,
            name: s.name,
            member_count: s.member_count,
            updated_at: time_text(s.updated_at),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberDto {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RosterDetail {
    pub sharing_group_id: Option<String>,
    pub can_manage: bool,
    pub id: String,
    pub name: String,
    pub updated_at: String,
    /// Sorted by `sortOrder`.
    pub members: Vec<MemberDto>,
}

impl From<&Stored<Roster>> for RosterDetail {
    fn from(s: &Stored<Roster>) -> Self {
        let r = &s.value;
        // The aggregate keeps insertion order; the API lists by sortOrder.
        let mut members: Vec<_> = r.members().iter().collect();
        members.sort_by_key(|m| m.sort_order);
        Self {
            id: r.id().to_string(),
            sharing_group_id: r.sharing_group_id.map(|g| g.to_string()),
            can_manage: r.can_manage,
            name: r.name().to_string(),
            updated_at: time_text(s.updated_at),
            members: members
                .into_iter()
                .map(|m| MemberDto {
                    id: m.id.to_string(),
                    name: m.name.to_string(),
                    note: m.note.clone(),
                    sort_order: m.sort_order,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberOutcome {
    /// The name as sent in the request.
    pub input: String,
    pub outcome: OutcomeKind,
    pub member_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberUpsertReport {
    pub created: u32,
    pub updated: u32,
    pub unchanged: u32,
    /// In request order.
    pub outcomes: Vec<MemberOutcome>,
}

fn report_dto(inputs: &[String], r: &UpsertReport<RosterMemberId>) -> MemberUpsertReport {
    MemberUpsertReport {
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
                MemberOutcome {
                    input: input.clone(),
                    outcome,
                    member_id: id.to_string(),
                }
            })
            .collect(),
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberInputDto {
    pub name: String,
    pub note: Option<String>,
}

fn inputs(members: Vec<MemberInputDto>) -> (Vec<String>, Vec<MemberInput>) {
    let names = members.iter().map(|m| m.name.clone()).collect();
    let inputs = members
        .into_iter()
        .map(|m| MemberInput {
            name: m.name,
            note: m.note,
        })
        .collect();
    (names, inputs)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRosterRequest {
    pub sharing_group_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub members: Vec<MemberInputDto>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedRoster {
    #[serde(flatten)]
    pub roster: RosterDetail,
    /// Present when `members` were given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<MemberUpsertReport>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameRosterRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertMembersRequest {
    pub members: Vec<MemberInputDto>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RosterUpsertResponse {
    pub roster: RosterDetail,
    pub report: MemberUpsertReport,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchMemberRequest {
    pub name: Option<String>,
    /// Absent = keep, `null` = clear, string = set.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub note: Option<Option<String>>,
}

fn rid(s: &str) -> Result<RosterId, AppError> {
    parse_id(s)
}

/// List rosters, newest updated first.
#[utoipa::path(get, path = "/rosters", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    params(SharingScopeQuery),
    responses((status = 200, body = Vec<RosterSummaryDto>), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list_rosters(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Query(scope): Query<SharingScopeQuery>,
) -> Result<Json<Vec<RosterSummaryDto>>, AppError> {
    let list = state
        .directory
        .rosters
        .list_scoped(o.id, scope.group()?)
        .await
        .map_err(|e| directory_error(e.into()))?;
    Ok(Json(list.into_iter().map(Into::into).collect()))
}

/// Create a roster, optionally with initial members.
#[utoipa::path(post, path = "/rosters", tag = TAG_DIRECTORY, security(("cookieAuth" = [])),
    request_body = CreateRosterRequest,
    responses((status = 201, body = CreatedRoster), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn create_roster(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    ApiJson(req): ApiJson<CreateRosterRequest>,
) -> Result<(StatusCode, Json<CreatedRoster>), AppError> {
    let (names, members) = inputs(req.members);
    let Modified { stored, report } = state
        .directory
        .create_roster_scoped(
            o.id,
            req.sharing_group_id.as_deref().map(parse_id).transpose()?,
            &req.name,
            members,
        )
        .await
        .map_err(directory_error)?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedRoster {
            roster: (&stored).into(),
            report: report.map(|r| report_dto(&names, &r)),
        }),
    ))
}

/// Roster with its members.
#[utoipa::path(get, path = "/rosters/{rosterId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("rosterId" = String, Path, description = "Roster id")),
    responses((status = 200, body = RosterDetail), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn get_roster(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<RosterDetail>, AppError> {
    let stored = state
        .directory
        .get_roster(o.id, rid(&id)?)
        .await
        .map_err(directory_error)?;
    Ok(Json((&stored).into()))
}

/// Rename a roster.
#[utoipa::path(patch, path = "/rosters/{rosterId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("rosterId" = String, Path, description = "Roster id")),
    request_body = RenameRosterRequest,
    responses((status = 200, body = RosterDetail), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn rename_roster(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<RenameRosterRequest>,
) -> Result<Json<RosterDetail>, AppError> {
    let m = state
        .directory
        .rename_roster(o.id, rid(&id)?, req.name)
        .await
        .map_err(directory_error)?;
    Ok(Json((&m.stored).into()))
}

/// Delete a roster and its members.
#[utoipa::path(delete, path = "/rosters/{rosterId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("rosterId" = String, Path, description = "Roster id")),
    responses((status = 204, description = "Deleted"), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete_roster(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .directory
        .delete_roster(o.id, rid(&id)?)
        .await
        .map_err(directory_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Upsert members by name (same `NameKey` overwrites; a blank note never clears).
#[utoipa::path(post, path = "/rosters/{rosterId}/members/upsert", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("rosterId" = String, Path, description = "Roster id")),
    request_body = UpsertMembersRequest,
    responses((status = 200, body = RosterUpsertResponse), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn upsert_members(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<UpsertMembersRequest>,
) -> Result<Json<RosterUpsertResponse>, AppError> {
    let (names, members) = inputs(req.members);
    let m = state
        .directory
        .upsert_members(o.id, rid(&id)?, members)
        .await
        .map_err(directory_error)?;
    let report = m
        .report
        .ok_or_else(|| AppError::Internal("upsert returned no report".into()))?;
    Ok(Json(RosterUpsertResponse {
        roster: (&m.stored).into(),
        report: report_dto(&names, &report),
    }))
}

/// Manually edit one member (`note: null` clears the note).
#[utoipa::path(patch, path = "/rosters/{rosterId}/members/{memberId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("rosterId" = String, Path, description = "Roster id"),
           ("memberId" = String, Path, description = "Member id")),
    request_body = PatchMemberRequest,
    responses((status = 200, body = RosterDetail), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "Name conflict: name_taken, vendor_name_taken (with existingId) or conflict", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "Validation failed: invalid_<field>, duplicate_name, limit_exceeded or validation_failed", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn patch_member(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, member)): Path<(String, String)>,
    ApiJson(req): ApiJson<PatchMemberRequest>,
) -> Result<Json<RosterDetail>, AppError> {
    let edit = MemberEdit {
        name: req.name,
        note: Patch::from_option(req.note),
    };
    let m = state
        .directory
        .edit_member(o.id, rid(&id)?, parse_id(&member)?, edit)
        .await
        .map_err(directory_error)?;
    Ok(Json((&m.stored).into()))
}

/// Remove one member.
#[utoipa::path(delete, path = "/rosters/{rosterId}/members/{memberId}", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])),
    params(("rosterId" = String, Path, description = "Roster id"),
           ("memberId" = String, Path, description = "Member id")),
    responses((status = 204, description = "Removed"), (status = 401, description = "Not logged in", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "Not found (also for another organizer's resource)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete_member(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, member)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    state
        .directory
        .remove_member(o.id, rid(&id)?, parse_id(&member)?)
        .await
        .map_err(directory_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Mounted under `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_rosters, create_roster))
        .routes(routes!(get_roster, rename_roster, delete_roster))
        .routes(routes!(upsert_members))
        .routes(routes!(patch_member, delete_member))
}
