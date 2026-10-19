//! Local sharing groups; no global user directory or identity-provider roles.
use super::common::{domain_error, parse_id};
use crate::{
    error::{AppError, ProblemDetails},
    json::ApiJson,
    session::AuthenticatedOrganizer,
    state::AppState,
};
use application::sharing::{SharingError, SharingGroup, SharingRepo, SharingUser};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use infrastructure::sharing_repo::SqliteSharingRepo;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

pub fn sharing_error(e: SharingError) -> AppError {
    match e {
        SharingError::NotFound => AppError::NotFound("sharing group or user".into()),
        SharingError::Forbidden => AppError::Forbidden("group administrator required".into()),
        SharingError::LastAdmin => AppError::Coded {
            status: StatusCode::CONFLICT,
            code: "last_admin",
            detail: "Cannot remove or demote the last administrator".into(),
        },
        SharingError::Conflict => AppError::Coded {
            status: StatusCode::CONFLICT,
            code: "already_member",
            detail: "User already belongs to the group".into(),
        },
        SharingError::Domain(e) => domain_error(e),
        SharingError::Port(e) => AppError::Internal(e.0),
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharingGroupDto {
    pub id: String,
    pub name: String,
    pub role: String,
}
impl From<SharingGroup> for SharingGroupDto {
    fn from(g: SharingGroup) -> Self {
        Self {
            id: g.id.to_string(),
            name: g.name,
            role: g.role,
        }
    }
}
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharingUserDto {
    pub id: String,
    pub display_name: String,
    pub email: String,
    pub identity_kind: String,
    pub role: Option<String>,
}
impl From<SharingUser> for SharingUserDto {
    fn from(u: SharingUser) -> Self {
        Self {
            id: u.id.to_string(),
            display_name: u.display_name,
            email: u.email,
            identity_kind: u.identity_kind,
            role: u.role,
        }
    }
}
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharingGroupDetail {
    #[serde(flatten)]
    pub group: SharingGroupDto,
    pub members: Vec<SharingUserDto>,
}
#[derive(Debug, Deserialize, ToSchema)]
pub struct GroupNameRequest {
    pub name: String,
}
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddSharingMemberRequest {
    pub user_id: String,
    pub role: SharingRoleDto,
}
#[derive(Debug, Deserialize, ToSchema)]
pub struct SharingRoleRequest {
    pub role: SharingRoleDto,
}
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SharingRoleDto {
    Admin,
    Member,
}
impl SharingRoleDto {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }
}
#[derive(Debug, Deserialize, IntoParams)]
pub struct LookupQuery {
    pub email: String,
}
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct SharingScopeQuery {
    pub sharing_group_id: Option<String>,
}
impl SharingScopeQuery {
    pub fn group(&self) -> Result<Option<domain::sharing::SharingGroupId>, AppError> {
        self.sharing_group_id.as_deref().map(parse_id).transpose()
    }
}

#[utoipa::path(get,path="/sharing-groups",security(("cookieAuth"=[])),responses((status=200,body=Vec<SharingGroupDto>),(status=401,body=ProblemDetails)))]
pub async fn list(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
) -> Result<Json<Vec<SharingGroupDto>>, AppError> {
    Ok(Json(
        SqliteSharingRepo::new(s.db)
            .list(o.id)
            .await
            .map_err(sharing_error)?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}
#[utoipa::path(post,path="/sharing-groups",security(("cookieAuth"=[])),request_body=GroupNameRequest,responses((status=201,body=SharingGroupDto),(status=422,body=ProblemDetails)))]
pub async fn create(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    ApiJson(r): ApiJson<GroupNameRequest>,
) -> Result<(StatusCode, Json<SharingGroupDto>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(
            SqliteSharingRepo::new(s.db)
                .create(o.id, &r.name)
                .await
                .map_err(sharing_error)?
                .into(),
        ),
    ))
}
#[utoipa::path(get,path="/sharing-groups/{groupId}",security(("cookieAuth"=[])),params(("groupId"=String,Path)),responses((status=200,body=SharingGroupDetail),(status=404,body=ProblemDetails)))]
pub async fn get(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<SharingGroupDetail>, AppError> {
    let repo = SqliteSharingRepo::new(s.db);
    let id = parse_id(&id)?;
    let group = repo.get(o.id, id).await.map_err(sharing_error)?.into();
    let members = repo
        .members(o.id, id)
        .await
        .map_err(sharing_error)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(SharingGroupDetail { group, members }))
}
#[utoipa::path(patch,path="/sharing-groups/{groupId}",security(("cookieAuth"=[])),params(("groupId"=String,Path)),request_body=GroupNameRequest,responses((status=200,body=SharingGroupDto),(status=403,body=ProblemDetails)))]
pub async fn rename(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(r): ApiJson<GroupNameRequest>,
) -> Result<Json<SharingGroupDto>, AppError> {
    Ok(Json(
        SqliteSharingRepo::new(s.db)
            .rename(o.id, parse_id(&id)?, &r.name)
            .await
            .map_err(sharing_error)?
            .into(),
    ))
}
#[utoipa::path(get,path="/sharing-groups/{groupId}/members",security(("cookieAuth"=[])),params(("groupId"=String,Path)),responses((status=200,body=Vec<SharingUserDto>),(status=404,body=ProblemDetails)))]
pub async fn members(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<Vec<SharingUserDto>>, AppError> {
    Ok(Json(
        SqliteSharingRepo::new(s.db)
            .members(o.id, parse_id(&id)?)
            .await
            .map_err(sharing_error)?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}
#[utoipa::path(get,path="/sharing-groups/{groupId}/users/lookup",security(("cookieAuth"=[])),params(("groupId"=String,Path),LookupQuery),responses((status=200,body=Vec<SharingUserDto>),(status=403,body=ProblemDetails)))]
pub async fn lookup(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    Query(q): Query<LookupQuery>,
) -> Result<Json<Vec<SharingUserDto>>, AppError> {
    Ok(Json(
        SqliteSharingRepo::new(s.db)
            .lookup(o.id, parse_id(&id)?, &q.email)
            .await
            .map_err(sharing_error)?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}
#[utoipa::path(post,path="/sharing-groups/{groupId}/members",security(("cookieAuth"=[])),params(("groupId"=String,Path)),request_body=AddSharingMemberRequest,responses((status=204),(status=403,body=ProblemDetails),(status=409,body=ProblemDetails)))]
pub async fn add(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(r): ApiJson<AddSharingMemberRequest>,
) -> Result<StatusCode, AppError> {
    SqliteSharingRepo::new(s.db)
        .add(o.id, parse_id(&id)?, parse_id(&r.user_id)?, r.role.as_str())
        .await
        .map_err(sharing_error)?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(patch,path="/sharing-groups/{groupId}/members/{userId}",security(("cookieAuth"=[])),params(("groupId"=String,Path),("userId"=String,Path)),request_body=SharingRoleRequest,responses((status=204),(status=403,body=ProblemDetails),(status=409,body=ProblemDetails)))]
pub async fn change(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, user)): Path<(String, String)>,
    ApiJson(r): ApiJson<SharingRoleRequest>,
) -> Result<StatusCode, AppError> {
    SqliteSharingRepo::new(s.db)
        .change(
            o.id,
            parse_id(&id)?,
            parse_id(&user)?,
            Some(r.role.as_str()),
        )
        .await
        .map_err(sharing_error)?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(delete,path="/sharing-groups/{groupId}/members/{userId}",security(("cookieAuth"=[])),params(("groupId"=String,Path),("userId"=String,Path)),responses((status=204),(status=403,body=ProblemDetails),(status=409,body=ProblemDetails)))]
pub async fn remove(
    State(s): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path((id, user)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    SqliteSharingRepo::new(s.db)
        .change(o.id, parse_id(&id)?, parse_id(&user)?, None)
        .await
        .map_err(sharing_error)?;
    Ok(StatusCode::NO_CONTENT)
}
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, create))
        .routes(routes!(get, rename))
        .routes(routes!(members, add))
        .routes(routes!(lookup))
        .routes(routes!(change, remove))
}
