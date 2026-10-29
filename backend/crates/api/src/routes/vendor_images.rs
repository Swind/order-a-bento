//! Session-protected, single private reference image per vendor.

use super::common::{directory_error, parse_id, time_text};
use crate::{
    error::{AppError, ProblemDetails},
    openapi::TAG_DIRECTORY,
    session::AuthenticatedOrganizer,
    state::AppState,
};
use axum::{
    Json,
    extract::{
        DefaultBodyLimit, Path, State,
        multipart::{Multipart, MultipartError, MultipartRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use domain::directory::VendorImage;
use infrastructure::vendor_image::{MAX_IMAGE_BYTES, normalize};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

const MAX_BODY_BYTES: usize = 6 * 1024 * 1024;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorImageDto {
    /// Server-generated UUID, not an authorization token or conditional-write version.
    pub revision: String,
    pub content_type: ImageContentType,
    #[schema(minimum = 1, maximum = 5242880)]
    pub size_bytes: u32,
    #[schema(minimum = 1, maximum = 1568)]
    pub width: u32,
    #[schema(minimum = 1, maximum = 1568)]
    pub height: u32,
    pub updated_at: String,
}

impl From<&VendorImage> for VendorImageDto {
    fn from(v: &VendorImage) -> Self {
        Self {
            revision: v.revision.clone(),
            content_type: ImageContentType::Jpeg,
            size_bytes: v.size_bytes,
            width: v.width,
            height: v.height,
            updated_at: time_text(v.updated_at),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub enum ImageContentType {
    #[serde(rename = "image/jpeg")]
    Jpeg,
}

pub struct ImageBinary;
impl utoipa::ToSchema for ImageBinary {}
impl utoipa::PartialSchema for ImageBinary {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::{KnownFormat, ObjectBuilder, SchemaFormat, SchemaType, Type};
        ObjectBuilder::new()
            .schema_type(SchemaType::Type(Type::String))
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
            .into()
    }
}

#[derive(ToSchema)]
pub struct VendorImageForm {
    #[schema(value_type = String, format = Binary)]
    pub image: Vec<u8>,
}

fn coded(status: StatusCode, code: &'static str) -> AppError {
    AppError::Coded {
        status,
        code,
        detail: match code {
            "image_too_large" => "Image exceeds 5 MiB or request exceeds 6 MiB.",
            _ => "Expected exactly one image multipart field.",
        }
        .into(),
    }
}
fn invalid_image() -> AppError {
    AppError::Problem {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "invalid_image".into(),
        detail:
            "Expected a valid static JPEG or PNG within the image limits; WebP is not supported."
                .into(),
        field: Some("image".into()),
        existing_id: None,
    }
}
fn multipart_error(e: MultipartError) -> AppError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        coded(StatusCode::PAYLOAD_TOO_LARGE, "image_too_large")
    } else {
        coded(StatusCode::BAD_REQUEST, "invalid_multipart")
    }
}

fn vendor_id(text: &str) -> Result<domain::directory::VendorId, AppError> {
    parse_id(text).map_err(|_| AppError::NotFound("Vendor not found.".into()))
}

pub(super) fn start_normalization(
    image: Vec<u8>,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> tokio::task::JoinHandle<
    Result<application::ports::NormalizedVendorImage, infrastructure::vendor_image::InvalidImage>,
> {
    tokio::task::spawn_blocking(move || {
        // Permit belongs to the job, not its awaiting HTTP future.
        let _permit = permit;
        normalize(&image)
    })
}

/// Atomically replace the image; last successful commit wins. 5 MiB image,
/// 6 MiB body, at most two processing jobs; busy requests are not queued.
/// Only static JPEG/PNG inputs are supported; WebP and animated formats are rejected.
#[utoipa::path(put, path = "/vendors/{vendorId}/image", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])), params(("vendorId" = String, Path)),
    request_body(content = VendorImageForm, content_type = "multipart/form-data"),
    responses((status = 200, body = VendorImageDto),
        (status = 400, description = "invalid_multipart", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 401, description = "unauthorized", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 403, description = "bad_origin", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "not_found", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 413, description = "image_too_large", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 422, description = "invalid_image", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 429, description = "rate_limited; Retry-After", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 500, description = "internal_error", body = ProblemDetails, content_type = "application/problem+json")))]
async fn put_image(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, AppError> {
    let id = vendor_id(&id)?;
    state
        .directory
        .check_image_manager(o.id, id)
        .await
        .map_err(directory_error)?;
    if let Err(retry) = state
        .image_limiter
        .check(o.id.to_string(), std::time::Instant::now())
    {
        return Ok(crate::client::rate_limited(retry));
    }
    let Ok(permit) = state.image_processing.clone().try_acquire_owned() else {
        return Ok(crate::client::rate_limited(std::time::Duration::from_secs(
            1,
        )));
    };
    let mut multipart =
        multipart.map_err(|_| coded(StatusCode::BAD_REQUEST, "invalid_multipart"))?;
    let mut image = None;
    while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
        if field.name() != Some("image") || image.is_some() {
            return Err(coded(StatusCode::BAD_REQUEST, "invalid_multipart"));
        }
        let mut data = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
            if chunk.len() > MAX_IMAGE_BYTES - data.len() {
                return Err(coded(StatusCode::PAYLOAD_TOO_LARGE, "image_too_large"));
            }
            data.extend_from_slice(&chunk);
        }
        image = Some(data);
    }
    let image = image.ok_or_else(invalid_image)?;
    let normalized = start_normalization(image, permit)
        .await
        .map_err(|_| AppError::Internal("image processing task failed".into()))?
        .map_err(|_| invalid_image())?;
    let metadata = state
        .directory
        .put_vendor_image(o.id, id, normalized)
        .await
        .map_err(directory_error)?;
    Ok(Json(VendorImageDto::from(&metadata)).into_response())
}

/// Read the current JPEG using current parent permissions, never a public URL.
#[utoipa::path(get, path = "/vendors/{vendorId}/image", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])), params(("vendorId" = String, Path)),
    responses((status = 200, body = ImageBinary, content_type = "image/jpeg"),
        (status = 401, description = "unauthorized", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "not_found (including no image)", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 500, description = "internal_error", body = ProblemDetails, content_type = "application/problem+json")))]
async fn get_image(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let image = state
        .directory
        .read_vendor_image(o.id, vendor_id(&id)?)
        .await
        .map_err(directory_error)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (
                header::HeaderName::from_static("cross-origin-resource-policy"),
                "same-origin",
            ),
        ],
        image.data,
    )
        .into_response())
}

/// Idempotent for an existing, manageable vendor (including no image).
#[utoipa::path(delete, path = "/vendors/{vendorId}/image", tag = TAG_DIRECTORY,
    security(("cookieAuth" = [])), params(("vendorId" = String, Path)),
    responses((status = 204, description = "Deleted"),
        (status = 401, description = "unauthorized", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 403, description = "bad_origin", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 404, description = "not_found", body = ProblemDetails, content_type = "application/problem+json"),
        (status = 500, description = "internal_error", body = ProblemDetails, content_type = "application/problem+json")))]
async fn delete_image(
    State(state): State<AppState>,
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .directory
        .delete_vendor_image(o.id, vendor_id(&id)?)
        .await
        .map_err(directory_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Outer middleware so session, origin and extractor rejections are private too.
pub async fn no_store(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let private_image =
        req.uri().path().starts_with("/api/v1/vendors/") && req.uri().path().ends_with("/image");
    let mut response = next.run(req).await;
    if private_image {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("private, no-store"),
        );
    }
    response
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(put_image, get_image, delete_image))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_waiter_does_not_release_a_queued_blocking_job_permit() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = runtime.spawn_blocking(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.recv().unwrap();
        runtime.block_on(async {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let job = start_normalization(vec![], permit);
            drop(job); // Equivalent to cancelling the handler while awaiting the job.
            assert_eq!(semaphore.available_permits(), 0);
            release_tx.send(()).unwrap();
            blocker.await.unwrap();
            let _finished = semaphore.acquire().await.unwrap();
        });
    }
}
