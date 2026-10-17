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
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(Self(v)),
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
