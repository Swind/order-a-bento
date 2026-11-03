//! JSON body extractor that reports rejections as problem+json.

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use crate::error::AppError;

pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<Box<serde_json::value::RawValue>>::from_request(req, state).await {
            Ok(axum::Json(raw)) => {
                let invalid = |detail| AppError::Problem {
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                    code: "validation_failed".into(),
                    detail,
                    field: None,
                    existing_id: None,
                };
                domain::shared::amount::validate_prices(raw.get()).map_err(invalid)?;
                serde_json::from_str(raw.get())
                    .map(Self)
                    .map_err(|e| invalid(e.to_string()))
            }
            Err(e) => Err(match e {
                JsonRejection::JsonDataError(e) => AppError::Problem {
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                    code: "validation_failed".into(),
                    detail: e.body_text(),
                    field: None,
                    existing_id: None,
                },
                other => AppError::Coded {
                    status: other.status(),
                    code: "invalid_json",
                    detail: other.body_text(),
                },
            }),
        }
    }
}
