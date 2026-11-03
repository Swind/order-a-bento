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
    #[serde(with = "domain::shared::amount")]
    #[schema(value_type = f64)]
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
    #[serde(with = "domain::shared::amount::optional")]
    #[schema(value_type = Option<f64>)]
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
    DeliveryAddress(ProposedDeliveryAddressDto),
    Roster(RosterTargetDto),
    Vendor(VendorTargetDto),
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProposedDeliveryAddressDto {
    pub name: String,
    pub address: String,
    pub confidence: ConfidenceDto,
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
    DeliveryAddress(AcceptedDeliveryAddressDto),
    Roster(AcceptedRosterDto),
    Vendor(AcceptedVendorDto),
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AcceptedDeliveryAddressDto {
    pub name: String,
    pub address: String,
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
    /// TWD dollars, at most two decimal places; fill in missing prices first.
    #[serde(default)]
    #[serde(with = "domain::shared::amount::optional")]
    #[schema(value_type = Option<f64>)]
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
    #[serde(rename = "deliveryAddress")]
    DeliveryAddress,
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
    #[schema(value_type = Option<DraftStateDto>, required = false)]
    pub draft: Option<serde_json::Value>,
    /// True only while running/proposed with an unexpired pending JPEG.
    pub has_pending_photo: bool,
    #[schema(required = true)]
    pub pending_photo_expires_at: Option<String>,
    pub sharing_group_id: Option<String>,
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
            draft: r
                .draft
                .as_ref()
                .map(|d| serde_json::to_value(d).expect("draft serializes")),
            has_pending_photo: r.pending_photo_expires_at.is_some(),
            pending_photo_expires_at: r.pending_photo_expires_at.map(time_text),
            id: r.id.to_string(),
            sharing_group_id: r.sharing_group_id.map(|g| g.to_string()),
            status: match r.status {
                ImportRunStatus::Running => ImportRunStatusDto::Running,
                ImportRunStatus::Proposed => ImportRunStatusDto::Proposed,
                ImportRunStatus::Applied => ImportRunStatusDto::Applied,
                ImportRunStatus::Discarded => ImportRunStatusDto::Discarded,
                ImportRunStatus::Failed => ImportRunStatusDto::Failed,
            },
            mode: r.mode.map(|m| match m {
                ImportMode::DeliveryAddress => ImportModeDto::DeliveryAddress,
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
/// required: `roster` for people, `vendor` for menus, `deliveryAddress` for delivery locations.
#[derive(Debug, ToSchema)]
#[allow(dead_code)]
#[schema(rename_all = "camelCase")]
pub struct StartImportForm {
    /// Opt-in, default false. Only vendor mode; requires static JPEG/PNG.
    save_photo: Option<bool>,
    sharing_group_id: Option<String>,
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
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    #[serde(default)]
    pub draft_revision: Option<i64>,
    /// Omitted defaults to the currently available pending photo. False imports data only.
    pub save_photo: Option<bool>,
    /// The changes the user confirmed (possibly edited); anything not listed is ignored.
    #[schema(value_type = Vec<AcceptedChangeDto>)]
    pub accepted: Vec<AcceptedChange>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResponse {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub delivery_address_ids: Vec<String>,
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
        AiImportError::Domain(domain::shared::DomainError::Invalid {
            field: "savePhoto",
            reason,
        }) => invalid_field("savePhoto", "invalid_save_photo", reason),
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
    let mut sharing_group = None;
    let mut save_photo = None;
    while let Some(field) = mp.next_field().await.map_err(multipart_error)? {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "savePhoto" => {
                if save_photo.is_some() {
                    return Err(coded(
                        StatusCode::BAD_REQUEST,
                        "invalid_multipart",
                        "Duplicate savePhoto field.",
                    ));
                }
                save_photo = Some(
                    match field.text().await.map_err(multipart_error)?.as_str() {
                        "true" => true,
                        "false" => false,
                        _ => {
                            return Err(invalid_field(
                                "savePhoto",
                                "invalid_save_photo",
                                "savePhoto must be true or false.",
                            ));
                        }
                    },
                );
            }
            "image" => {
                let bytes = field.bytes().await.map_err(multipart_error)?;
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(image_too_large());
                }
                image = Some(bytes.to_vec());
            }
            "instruction" | "mode" | "targetRosterId" | "targetVendorId" | "sharingGroupId" => {
                let text = blank_to_none(field.text().await.map_err(multipart_error)?);
                match name.as_str() {
                    "instruction" => instruction = text,
                    "mode" => mode = text,
                    "targetRosterId" => target_roster = text,
                    "sharingGroupId" => sharing_group = text,
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
            "mode is required (roster, vendor or deliveryAddress).",
        )
    })?;
    let mode = ImportMode::parse(&mode).map_err(|_| {
        invalid_field(
            "mode",
            "invalid_mode",
            "mode must be roster, vendor or deliveryAddress.",
        )
    })?;
    let target_roster_id = target_roster.map(|s| parse_id(&s)).transpose()?;
    let target_vendor_id = target_vendor.map(|s| parse_id(&s)).transpose()?;

    let pending_photo = if save_photo == Some(true) {
        if mode != ImportMode::Vendor {
            return Err(invalid_field(
                "savePhoto",
                "invalid_save_photo",
                "Only vendor imports can save a photo.",
            ));
        }
        let Ok(permit) = state.image_processing.clone().try_acquire_owned() else {
            return Ok(crate::client::rate_limited(std::time::Duration::from_secs(
                1,
            )));
        };
        Some(super::vendor_images::start_normalization(image.clone(), permit).await
            .map_err(|_| AppError::Internal("image processing task failed".into()))?
            .map_err(|_| invalid_field("image", "invalid_image", "Saving a photo requires a valid static JPEG or PNG; convert WebP to JPEG first."))?)
    } else {
        None
    };
    let image = ImageInput {
        bytes: image,
        media_type: media_type.into(),
    };

    let run = ai
        .start(
            o.id,
            StartImportCmd {
                pending_photo,
                sharing_group_id: sharing_group.as_deref().map(parse_id).transpose()?,
                image,
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
        (status = 422, description = "Invalid changes (invalid_changes, with errors[]) or unavailable/ambiguous pending photo (invalid_save_photo, field savePhoto); nothing is committed",
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
        .apply_at_revision(
            org(&o),
            parse_id(&run_id)?,
            ApplyCmd {
                save_photo: body.save_photo,
                accepted: body.accepted,
            },
            body.draft_revision,
        )
        .await
        .map_err(ai_error)?;
    Ok(Json(ApplyResponse {
        delivery_address_ids: r
            .delivery_address_ids
            .iter()
            .map(ToString::to_string)
            .collect(),
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
    upload
        .merge(
            OpenApiRouter::new()
                .routes(routes!(get_import))
                .routes(routes!(apply_import))
                .routes(routes!(discard_import)),
        )
        .merge(
            OpenApiRouter::new()
                .routes(routes!(get_draft, put_draft))
                .routes(routes!(draft_turn))
                .routes(routes!(undo_turn))
                .layer(DefaultBodyLimit::max(512 * 1024)),
        )
}

#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DraftItemDto {
    pub id: String,
    pub selected: bool,
    pub name: String,
    #[schema(value_type = Option<f64>)]
    pub price: Option<i64>,
    pub category: Option<String>,
    pub description: Option<String>,
}
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DraftVendorDto {
    pub id: String,
    pub vendor: TargetRefDto,
    pub selected: bool,
    pub info_selected: bool,
    pub info: VendorInfoSnapshotDto,
    pub menu: Vec<DraftItemDto>,
}
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DraftTurnDto {
    pub id: String,
    pub instruction: String,
    pub reply: String,
    pub status: String,
    pub changes: Vec<String>,
    pub undone: bool,
}
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DraftStateDto {
    pub revision: i64,
    pub vendors: Vec<DraftVendorDto>,
    pub turns: Vec<DraftTurnDto>,
    pub undo: Option<Vec<DraftVendorDto>>,
    pub pending: Option<String>,
    pub pending_until: Option<u64>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftRequest {
    pub revision: i64,
    #[schema(value_type = Vec<DraftVendorDto>)]
    pub vendors: Vec<domain::ai_import::VendorDraft>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnRequest {
    pub revision: i64,
    pub turn_id: String,
    pub instruction: String,
    #[schema(value_type = Vec<DraftVendorDto>)]
    pub vendors: Vec<domain::ai_import::VendorDraft>,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UndoRequest {
    pub revision: i64,
    pub turn_id: String,
}

#[utoipa::path(get, path = "/ai/import-runs/{runId}/draft", tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])), params(("runId" = String, Path)),
    responses((status = 200, body = DraftStateDto), (status = 409, body = ProblemDetails)))]
async fn get_draft(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Json<domain::ai_import::DraftState>, AppError> {
    Ok(Json(
        service(&state)?
            .get_draft(org(&o), parse_id(&id)?)
            .await
            .map_err(ai_error)?,
    ))
}
#[utoipa::path(put, path = "/ai/import-runs/{runId}/draft", tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])), params(("runId" = String, Path)), request_body = DraftRequest,
    responses((status = 200, body = DraftStateDto), (status = 409, body = ProblemDetails)))]
async fn put_draft(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<DraftRequest>,
) -> Result<Json<domain::ai_import::DraftState>, AppError> {
    Ok(Json(
        service(&state)?
            .save_draft(org(&o), parse_id(&id)?, body.revision, body.vendors)
            .await
            .map_err(ai_error)?,
    ))
}
#[utoipa::path(post, path = "/ai/import-runs/{runId}/turns", tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])), params(("runId" = String, Path)), request_body = TurnRequest,
    responses((status = 200, body = DraftStateDto), (status = 409, body = ProblemDetails), (status = 429, body = ProblemDetails)))]
async fn draft_turn(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<TurnRequest>,
) -> Result<Response, AppError> {
    let ai = service(&state)?;
    let id = parse_id(&id)?;
    let current = ai.get_draft(org(&o), id).await.map_err(ai_error)?;
    if !current.turns.iter().any(|t| t.id == body.turn_id)
        && let Err(retry) = state.ai_limiter.check(o.id.to_string(), Instant::now())
    {
        return Ok(crate::client::rate_limited(retry));
    }
    Ok(Json(
        ai.turn(
            org(&o),
            id,
            application::ai_import::DraftTurnCmd {
                revision: body.revision,
                turn_id: body.turn_id,
                instruction: body.instruction,
                vendors: body.vendors,
            },
        )
        .await
        .map_err(ai_error)?,
    )
    .into_response())
}
#[utoipa::path(post, path = "/ai/import-runs/{runId}/undo", tag = TAG_AI_IMPORT,
    security(("cookieAuth" = [])), params(("runId" = String, Path)), request_body = UndoRequest,
    responses((status = 200, body = DraftStateDto), (status = 409, body = ProblemDetails)))]
async fn undo_turn(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<UndoRequest>,
) -> Result<Json<domain::ai_import::DraftState>, AppError> {
    Ok(Json(
        service(&state)?
            .undo_turn(org(&o), parse_id(&id)?, body.revision, body.turn_id)
            .await
            .map_err(ai_error)?,
    ))
}
