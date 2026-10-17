//! `/ai/import-runs`: import rosters and vendor menus from a photo with AI.

use application::ai_import::{
    AcceptedChange, AiError, AiImportError, ApplyCmd, ImageInput, ImportRunView, StartImportCmd,
};
use axum::Json;
use axum::extract::multipart::{Multipart, MultipartError, MultipartRejection};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use domain::ai_import::{ImportMode, ImportRunStatus};
use domain::identity::OrganizerId;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::common::{domain_error, parse_id, time_text};
use crate::error::{AppError, ProblemDetails, ProblemFieldError};
use crate::json::ApiJson;
use crate::openapi::TAG_AI_IMPORT;
use crate::session::AuthenticatedOrganizer;
use crate::state::AppState;

/// Largest accepted image.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Request body limit for the upload route (image plus multipart framing).
pub const MAX_BODY_BYTES: usize = 6 * 1024 * 1024;
/// Longest accepted `instruction` (characters).
pub const MAX_INSTRUCTION_CHARS: usize = 200;

// ---------------------------------------------------------------------------
// DTOs. The domain types are plain serde data and must not depend on utoipa,
// so the API documents their JSON with mirror types (a test keeps them in sync).
// ---------------------------------------------------------------------------

/// `{ "kind": "existing", "existingId": ".." }` or `{ "kind": "new", "name": ".." }`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetRefDto {
    #[serde(rename_all = "camelCase")]
    Existing {
        existing_id: String,
    },
    New {
        name: String,
    },
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeActionDto {
    Create,
    Update,
    Unchanged,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceDto {
    High,
    Medium,
    Low,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueDto {
    PriceMissing,
    PriceOutOfRange,
    NameEmpty,
    NameTooLong,
    NoteTooLong,
    DescriptionTooLong,
    CategoryTooLong,
    DuplicateInInput,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposedMemberDto {
    pub action: ChangeActionDto,
    pub name: String,
    pub before_note: Option<String>,
    pub note: Option<String>,
    pub confidence: ConfidenceDto,
    pub issues: Vec<IssueDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RosterTargetDto {
    pub roster: TargetRefDto,
    pub members: Vec<ProposedMemberDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorInfoSnapshotDto {
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposedVendorInfoDto {
    pub action: ChangeActionDto,
    pub before: Option<VendorInfoSnapshotDto>,
    pub after: VendorInfoSnapshotDto,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemSnapshotDto {
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposedMenuItemDto {
    pub action: ChangeActionDto,
    pub name: String,
    pub before: Option<MenuItemSnapshotDto>,
    /// `null` = the price could not be read (issue `price_missing`).
    pub price: Option<i64>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub confidence: ConfidenceDto,
    pub issues: Vec<IssueDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorTargetDto {
    pub vendor: TargetRefDto,
    pub info: ProposedVendorInfoDto,
    pub menu: Vec<ProposedMenuItemDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProposalTargetDto {
    Roster(RosterTargetDto),
    Vendor(VendorTargetDto),
}

/// Pending changes proposed by the AI; nothing is written until applied.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposalDto {
    pub targets: Vec<ProposalTargetDto>,
    pub warnings: Vec<String>,
}

/// A confirmed (possibly edited) change sent to `apply`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AcceptedChangeDto {
    Roster(AcceptedRosterDto),
    Vendor(AcceptedVendorDto),
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedRosterDto {
    pub roster: TargetRefDto,
    pub members: Vec<AcceptedMemberDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedMemberDto {
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedVendorDto {
    pub vendor: TargetRefDto,
    /// `null`/absent leaves the vendor info alone (required for a new vendor,
    /// whose name is `info.name`; existing vendors are never renamed).
    #[serde(default)]
    pub info: Option<AcceptedVendorInfoDto>,
    #[serde(default)]
    pub menu: Vec<AcceptedMenuItemDto>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedVendorInfoDto {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedMenuItemDto {
    pub name: String,
    /// Whole TWD dollars; required (fill in `price_missing` items first).
    #[serde(default)]
    pub price: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImportRunStatusDto {
    Running,
    Proposed,
    Applied,
    Discarded,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImportModeDto {
    Roster,
    Vendor,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageDto {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRunDto {
    pub id: String,
    pub status: ImportRunStatusDto,
    /// What the user chose to upload. `null` only for runs created before the
    /// mode became required.
    pub mode: Option<ImportModeDto>,
    pub target_roster_id: Option<String>,
    pub target_vendor_id: Option<String>,
    pub instruction: Option<String>,
    /// The AI's closing summary (Traditional Chinese).
    pub summary: Option<String>,
    #[schema(value_type = ProposalDto)]
    pub proposal: serde_json::Value,
    pub usage: UsageDto,
    pub model: String,
    /// Why the run failed (`status = failed`).
    pub error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub applied_at: Option<String>,
}

impl From<&ImportRunView> for ImportRunDto {
    fn from(r: &ImportRunView) -> Self {
        Self {
            id: r.id.to_string(),
            status: match r.status {
                ImportRunStatus::Running => ImportRunStatusDto::Running,
                ImportRunStatus::Proposed => ImportRunStatusDto::Proposed,
                ImportRunStatus::Applied => ImportRunStatusDto::Applied,
                ImportRunStatus::Discarded => ImportRunStatusDto::Discarded,
                ImportRunStatus::Failed => ImportRunStatusDto::Failed,
            },
            mode: r.mode.map(|m| match m {
                ImportMode::Roster => ImportModeDto::Roster,
                ImportMode::Vendor => ImportModeDto::Vendor,
            }),
            target_roster_id: r.target_roster_id.map(|i| i.to_string()),
            target_vendor_id: r.target_vendor_id.map(|i| i.to_string()),
            instruction: r.instruction.clone(),
            summary: r.summary.clone(),
            proposal: serde_json::to_value(&r.proposal).unwrap_or(serde_json::Value::Null),
            usage: UsageDto {
                input_tokens: r.usage.input_tokens,
                output_tokens: r.usage.output_tokens,
            },
            model: r.model.clone(),
            error: r.error.clone(),
            created_at: time_text(r.created_at),
            finished_at: r.finished_at.map(time_text),
            applied_at: r.applied_at.map(time_text),
        }
    }
}

/// Multipart body of `POST /ai/import-runs` (documentation only). `mode` is
/// required: `roster` for a people roster photo, `vendor` for a vendor flyer / menu.
#[derive(Debug, ToSchema)]
#[allow(dead_code)]
#[schema(rename_all = "camelCase")]
pub struct StartImportForm {
    /// JPEG, PNG or WebP, at most 5 MB (detected from the bytes, not the file name).
    #[schema(value_type = String, format = Binary)]
    image: Vec<u8>,
    /// Free-text hint, at most 200 characters.
    instruction: Option<String>,
    #[schema(inline)]
    mode: ImportModeDto,
    /// Existing roster to add the members to.
    target_roster_id: Option<String>,
    /// Existing vendor to update.
    target_vendor_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyRequest {
    /// The changes the user confirmed (possibly edited); anything not listed is ignored.
    #[schema(value_type = Vec<AcceptedChangeDto>)]
    pub accepted: Vec<AcceptedChange>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResponse {
    pub roster_ids: Vec<String>,
    pub vendor_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

fn coded(status: StatusCode, code: &'static str, detail: impl Into<String>) -> AppError {
    AppError::Coded {
        status,
        code,
        detail: detail.into(),
    }
}

fn ai_not_configured() -> AppError {
    coded(
        StatusCode::SERVICE_UNAVAILABLE,
        "ai_not_configured",
        "AI import is not configured on this server.",
    )
}

fn invalid_field(field: &str, code: &str, detail: impl Into<String>) -> AppError {
    AppError::Problem {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: code.into(),
        detail: detail.into(),
        field: Some(field.into()),
        existing_id: None,
    }
}

fn image_too_large() -> AppError {
    coded(
        StatusCode::PAYLOAD_TOO_LARGE,
        "image_too_large",
        "The image must be at most 5 MB.",
    )
}

fn ai_error(e: AiImportError) -> AppError {
    match e {
        AiImportError::NotFound => AppError::NotFound("resource not found".into()),
        AiImportError::QuotaExceeded => quota_exceeded(),
        AiImportError::Agent { run_id, error } => {
            let run_id = Some(run_id.to_string());
            let (status, code, detail) = match error {
                AiError::Refused { .. } => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "ai_refused",
                    "The AI declined to process this image. Enter the data manually or try another photo.",
                ),
                AiError::InvalidImage(_) => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_image",
                    "The AI could not read this image.",
                ),
                AiError::QuotaExceeded => return quota_exceeded(),
                AiError::Unavailable { .. } | AiError::Timeout => (
                    StatusCode::BAD_GATEWAY,
                    "ai_unavailable",
                    "The AI service is temporarily unavailable. Try again later.",
                ),
            };
            AppError::Extended {
                status,
                code,
                detail: detail.into(),
                run_id,
                errors: vec![],
            }
        }
        AiImportError::InvalidState(d) => coded(StatusCode::CONFLICT, "invalid_state", d),
        AiImportError::InvalidChanges(list) => AppError::Extended {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "invalid_changes",
            detail: format!("{} invalid change(s).", list.len()),
            run_id: None,
            errors: list
                .into_iter()
                .map(|c| ProblemFieldError {
                    path: c.path,
                    message: c.message,
                })
                .collect(),
        },
        AiImportError::Domain(d) => domain_error(d),
        AiImportError::Conflict => {
            AppError::Conflict("The resource was modified concurrently; retry.".into())
        }
        AiImportError::Internal(d) => AppError::Internal(d),
    }
}

fn quota_exceeded() -> AppError {
    coded(
        StatusCode::TOO_MANY_REQUESTS,
        "ai_quota_exceeded",
        "The daily AI import quota is used up. Try again tomorrow.",
    )
}

// ---------------------------------------------------------------------------
// Image validation
// ---------------------------------------------------------------------------

/// Media type from the file's magic bytes; the client-declared type is ignored.
pub fn sniff_image_type(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some("image/png")
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn multipart_error(e: MultipartError) -> AppError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        image_too_large()
    } else {
        coded(e.status(), "invalid_multipart", "Malformed multipart body.")
    }
}

fn blank_to_none(s: String) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Upload a photo and let the AI propose roster members or a vendor menu.
///
/// Runs synchronously (up to 90 s) and returns the finished run. Nothing is
/// written to rosters or vendors until `apply`.
#[utoipa::path(
    post,
    path = "/ai/import-runs",
    tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])),
    request_body(content = StartImportForm, content_type = "multipart/form-data"),
    responses(
        (status = 201, description = "The run with the proposed changes", body = ImportRunDto),
        (status = 401, description = "Not logged in", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 404, description = "Target roster or vendor not found", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 413, description = "Image over 5 MB (code image_too_large)", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 422, description = "Invalid input (invalid_image, invalid_instruction, ...) or the AI refused (ai_refused, with runId)",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 429, description = "Rate limit (rate_limited) or daily quota (ai_quota_exceeded)",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 502, description = "AI service unavailable (ai_unavailable, with runId)",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 503, description = "AI import is not configured (ai_not_configured)",
         body = ProblemDetails, content_type = "application/problem+json"),
    )
)]
pub async fn start_import(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, AppError> {
    let ai = state.ai_import.clone().ok_or_else(ai_not_configured)?;
    if let Err(retry) = state.ai_limiter.check(o.id.to_string(), Instant::now()) {
        return Ok(crate::client::rate_limited(retry));
    }
    let mut mp = multipart.map_err(|e| {
        coded(
            e.status(),
            "invalid_multipart",
            "Expected multipart/form-data.",
        )
    })?;

    let mut image: Option<Vec<u8>> = None;
    let mut instruction = None;
    let mut mode = None;
    let mut target_roster = None;
    let mut target_vendor = None;
    while let Some(field) = mp.next_field().await.map_err(multipart_error)? {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "image" => {
                let bytes = field.bytes().await.map_err(multipart_error)?;
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(image_too_large());
                }
                image = Some(bytes.to_vec());
            }
            "instruction" | "mode" | "targetRosterId" | "targetVendorId" => {
                let text = blank_to_none(field.text().await.map_err(multipart_error)?);
                match name.as_str() {
                    "instruction" => instruction = text,
                    "mode" => mode = text,
                    "targetRosterId" => target_roster = text,
                    _ => target_vendor = text,
                }
            }
            _ => {} // unknown parts are ignored
        }
    }

    let image = image
        .filter(|b| !b.is_empty())
        .ok_or_else(|| invalid_field("image", "invalid_image", "An image file is required."))?;
    let media_type = sniff_image_type(&image).ok_or_else(|| {
        invalid_field(
            "image",
            "invalid_image",
            "The file must be a JPEG, PNG or WebP image.",
        )
    })?;
    if let Some(i) = &instruction
        && i.chars().count() > MAX_INSTRUCTION_CHARS
    {
        return Err(invalid_field(
            "instruction",
            "invalid_instruction",
            format!("The instruction must be at most {MAX_INSTRUCTION_CHARS} characters."),
        ));
    }
    let mode = mode.ok_or_else(|| {
        invalid_field(
            "mode",
            "invalid_mode",
            "mode is required (roster or vendor).",
        )
    })?;
    let mode = ImportMode::parse(&mode)
        .map_err(|_| invalid_field("mode", "invalid_mode", "mode must be roster or vendor."))?;
    let target_roster_id = target_roster.map(|s| parse_id(&s)).transpose()?;
    let target_vendor_id = target_vendor.map(|s| parse_id(&s)).transpose()?;

    let run = ai
        .start(
            o.id,
            StartImportCmd {
                image: ImageInput {
                    bytes: image,
                    media_type: media_type.into(),
                },
                instruction,
                mode,
                target_roster_id,
                target_vendor_id,
            },
        )
        .await
        .map_err(ai_error)?;
    Ok((StatusCode::CREATED, Json(ImportRunDto::from(&run))).into_response())
}

fn service(state: &AppState) -> Result<&application::AiImportService, AppError> {
    state.ai_import.as_deref().ok_or_else(ai_not_configured)
}

fn org(o: &domain::identity::Organizer) -> OrganizerId {
    o.id
}

/// One import run of the current organizer.
#[utoipa::path(
    get,
    path = "/ai/import-runs/{runId}",
    tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])),
    params(("runId" = String, Path, description = "Import run id")),
    responses(
        (status = 200, description = "The run", body = ImportRunDto),
        (status = 401, description = "Not logged in", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 404, description = "No such run", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 503, description = "AI import is not configured", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn get_import(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(run_id): Path<String>,
) -> Result<Json<ImportRunDto>, AppError> {
    let ai = service(&state)?;
    let run = ai
        .get(org(&o), parse_id(&run_id)?)
        .await
        .map_err(ai_error)?;
    Ok(Json(ImportRunDto::from(&run)))
}

/// Write the confirmed changes to rosters and vendors in one transaction.
#[utoipa::path(
    post,
    path = "/ai/import-runs/{runId}/apply",
    tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])),
    params(("runId" = String, Path, description = "Import run id")),
    request_body = ApplyRequest,
    responses(
        (status = 200, description = "Ids of the rosters and vendors written", body = ApplyResponse),
        (status = 401, description = "Not logged in", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 404, description = "No such run", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 409, description = "The run is not in the proposed state (invalid_state)",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "Invalid changes (invalid_changes, with errors[])",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 503, description = "AI import is not configured", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn apply_import(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(run_id): Path<String>,
    ApiJson(body): ApiJson<ApplyRequest>,
) -> Result<Json<ApplyResponse>, AppError> {
    let ai = service(&state)?;
    let r = ai
        .apply(
            org(&o),
            parse_id(&run_id)?,
            ApplyCmd {
                accepted: body.accepted,
            },
        )
        .await
        .map_err(ai_error)?;
    Ok(Json(ApplyResponse {
        roster_ids: r.roster_ids.iter().map(ToString::to_string).collect(),
        vendor_ids: r.vendor_ids.iter().map(ToString::to_string).collect(),
    }))
}

/// Throw the proposal away without writing anything.
#[utoipa::path(
    post,
    path = "/ai/import-runs/{runId}/discard",
    tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])),
    params(("runId" = String, Path, description = "Import run id")),
    responses(
        (status = 204, description = "Discarded"),
        (status = 401, description = "Not logged in", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 404, description = "No such run", body = ProblemDetails,
         content_type = "application/problem+json"),
        (status = 409, description = "The run is not in the proposed state (invalid_state)",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 503, description = "AI import is not configured", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn discard_import(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(run_id): Path<String>,
) -> Result<Response, AppError> {
    let ai = service(&state)?;
    ai.discard(org(&o), parse_id(&run_id)?)
        .await
        .map_err(ai_error)?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().remove(header::CONTENT_TYPE);
    Ok(res)
}

/// Mounted under `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    // The upload route allows a body slightly above the 5 MB image limit so
    // that the handler can answer `image_too_large` itself for small overshoots.
    let upload = OpenApiRouter::new()
        .routes(routes!(start_import))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));
    upload.merge(
        OpenApiRouter::new()
            .routes(routes!(get_import))
            .routes(routes!(apply_import))
            .routes(routes!(discard_import)),
    )
}
