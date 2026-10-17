use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, ProblemDetails};
use crate::openapi::TAG_AUTH;
use crate::session::AuthenticatedOrganizer;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// The currently logged-in organizer.
#[utoipa::path(
    get,
    path = "/me",
    tag = TAG_AUTH,
    security(("cookieAuth" = [])),
    responses(
        (status = 200, description = "Current organizer", body = MeResponse),
        (status = 401, description = "Not logged in", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn me(
    AuthenticatedOrganizer(o): AuthenticatedOrganizer,
) -> Result<Json<MeResponse>, AppError> {
    Ok(Json(MeResponse {
        id: o.id.to_string(),
        email: o.email,
        display_name: o.display_name,
        avatar_url: o.avatar_url,
    }))
}

/// Mounted under `/api/v1`.
pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(me))
}
