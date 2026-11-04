//! Request logging that never records share tokens.

use std::borrow::Cow;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::trace::{OnFailure, OnRequest, OnResponse, TraceLayer};
use tracing::Span;

pub const TOKEN_PLACEHOLDER: &str = ":token";

#[derive(Clone, Copy)]
struct RequestId(uuid::Uuid);

/// Always generate locally; never trust a caller's correlation header.
pub async fn request_id(mut req: Request<Body>, next: Next) -> Response {
    let id = uuid::Uuid::now_v7();
    req.extensions_mut().insert(RequestId(id));
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        "x-request-id",
        id.to_string()
            .parse()
            .expect("UUID is a valid header value"),
    );
    response
}

/// Replace the share token in `/api/v1/public/g/{token}...` and `/g/{token}...`
/// with `:token`. Other paths are returned untouched (borrowed).
pub fn redact_path(path: &str) -> Cow<'_, str> {
    for prefix in ["/api/v1/public/g/", "/g/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            let (token, tail) = match rest.find('/') {
                Some(i) => rest.split_at(i),
                None => (rest, ""),
            };
            if token.is_empty() {
                return Cow::Borrowed(path);
            }
            return Cow::Owned(format!("{prefix}{TOKEN_PLACEHOLDER}{tail}"));
        }
    }
    Cow::Borrowed(path)
}

/// Span carrying the method and the *redacted* path (no query string).
pub fn make_span(req: &Request<Body>) -> Span {
    tracing::info_span!(
        "request",
        request_id = %req.extensions().get::<RequestId>().map(|id| id.0.to_string()).unwrap_or_default(),
        method = %req.method(),
        path = %redact_path(req.uri().path()),
    )
}

#[derive(Clone, Copy)]
pub struct NoRequestLog;

impl<B> OnRequest<B> for NoRequestLog {
    fn on_request(&mut self, _: &Request<B>, _: &Span) {}
}

#[derive(Clone, Copy)]
pub struct ResponseLog;

impl<B> OnResponse<B> for ResponseLog {
    fn on_response(
        self,
        response: &axum::http::Response<B>,
        latency: std::time::Duration,
        _: &Span,
    ) {
        tracing::info!(
            target: "tower_http::trace::on_response",
            status = response.status().as_u16(),
            duration_ms = latency.as_millis() as u64,
            "finished processing request"
        );
    }
}

#[derive(Clone, Copy)]
pub struct FailureLog;

impl OnFailure<ServerErrorsFailureClass> for FailureLog {
    fn on_failure(
        &mut self,
        failure: ServerErrorsFailureClass,
        latency: std::time::Duration,
        _: &Span,
    ) {
        // Service errors can contain private payloads; only status classifications are printable.
        let classification = match failure {
            ServerErrorsFailureClass::StatusCode(status) => format!("Status code: {status}"),
            ServerErrorsFailureClass::Error(_) => "service_error".into(),
        };
        tracing::error!(
            target: "tower_http::trace::on_failure",
            classification,
            duration_ms = latency.as_millis() as u64,
            "response failed"
        );
    }
}

pub type HttpTraceLayer = TraceLayer<
    tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>,
    fn(&Request<Body>) -> Span,
    NoRequestLog,
    ResponseLog,
    tower_http::trace::DefaultOnBodyChunk,
    tower_http::trace::DefaultOnEos,
    FailureLog,
>;

/// HTTP trace layer: one INFO line per response, inside the redacted span.
pub fn trace_layer() -> HttpTraceLayer {
    TraceLayer::new_for_http()
        .make_span_with(make_span as fn(&Request<Body>) -> Span)
        .on_request(NoRequestLog)
        .on_response(ResponseLog)
        .on_failure(FailureLog)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ-_01234";

    #[test]
    fn redacts_token_segments() {
        assert_eq!(
            redact_path(&format!("/api/v1/public/g/{T}")),
            "/api/v1/public/g/:token"
        );
        assert_eq!(
            redact_path(&format!("/api/v1/public/g/{T}/participants/p1/order")),
            "/api/v1/public/g/:token/participants/p1/order"
        );
        assert_eq!(redact_path(&format!("/g/{T}")), "/g/:token");
        assert_eq!(redact_path(&format!("/g/{T}/")), "/g/:token/");
        // Malformed tokens are redacted too.
        assert_eq!(redact_path("/g/short"), "/g/:token");
    }

    #[test]
    fn leaves_other_paths_alone() {
        for p in [
            "/",
            "/healthz",
            "/api/v1/rosters/123",
            "/api/v1/public/g/",
            "/g/",
            "/g",
            "/api/v1/group-orders/abc",
            "/x/g/abc",
        ] {
            assert!(matches!(redact_path(p), Cow::Borrowed(_)), "{p}");
            assert_eq!(redact_path(p), p);
        }
    }
}
