use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::{AppError, ProblemDetails};
use crate::openapi::TAG_SYSTEM;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    #[schema(example = "ok")]
    pub status: String,
}

/// Liveness/readiness probe; verifies the database answers `SELECT 1`.
#[utoipa::path(
    get,
    path = "/healthz",
    tag = TAG_SYSTEM,
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
        (status = 500, description = "Database unreachable", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn healthz(State(state): State<AppState>) -> Result<Json<HealthResponse>, AppError> {
    sqlx::query("SELECT 1").execute(&state.db.read).await?;
    Ok(Json(HealthResponse {
        status: "ok".into(),
    }))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(healthz))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::router;

    #[tokio::test]
    async fn healthz_ok() {
        let t = crate::testing::test_app(false, false).await;
        let app = router(t.state);
        let res = app
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn openapi_json_and_docs_gating() {
        let t = crate::testing::test_app(false, false).await;
        let mut dev_state = t.state.clone();
        let get = |app: axum::Router, path: &'static str| async move {
            app.oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status()
        };
        dev_state.docs_enabled = true;
        let dev = router(dev_state.clone());
        assert_eq!(get(dev.clone(), "/api/openapi.json").await, StatusCode::OK);
        assert_eq!(get(dev, "/api/docs").await, StatusCode::OK);
        dev_state.docs_enabled = false;
        let prod = router(dev_state);
        assert_eq!(get(prod.clone(), "/api/openapi.json").await, StatusCode::OK);
        assert_eq!(get(prod, "/api/docs").await, StatusCode::NOT_FOUND);
    }

    #[test]
    fn openapi_contains_healthz() {
        let doc = serde_json::to_value(crate::openapi()).unwrap();
        assert!(doc["paths"]["/healthz"]["get"].is_object());
        assert_eq!(doc["info"]["title"], "order-a-bento API");
        assert_eq!(doc["info"]["version"], "0.1.0");
        assert_eq!(
            doc["components"]["securitySchemes"]["cookieAuth"]["name"],
            "sid"
        );
    }
}
