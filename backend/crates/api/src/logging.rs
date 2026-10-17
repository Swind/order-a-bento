//! Request logging that never records share tokens.

use std::borrow::Cow;

use axum::body::Body;
use axum::http::Request;
use tower_http::trace::{DefaultOnResponse, OnRequest, TraceLayer};
use tracing::{Level, Span};

pub const TOKEN_PLACEHOLDER: &str = ":token";

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
        method = %req.method(),
        path = %redact_path(req.uri().path()),
    )
}

#[derive(Clone, Copy)]
pub struct NoRequestLog;

impl<B> OnRequest<B> for NoRequestLog {
    fn on_request(&mut self, _: &Request<B>, _: &Span) {}
}

pub type HttpTraceLayer = TraceLayer<
    tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>,
    fn(&Request<Body>) -> Span,
    NoRequestLog,
    DefaultOnResponse,
>;

/// HTTP trace layer: one INFO line per response, inside the redacted span.
pub fn trace_layer() -> HttpTraceLayer {
    TraceLayer::new_for_http()
        .make_span_with(make_span as fn(&Request<Body>) -> Span)
        .on_request(NoRequestLog)
        .on_response(DefaultOnResponse::new().level(Level::INFO))
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
