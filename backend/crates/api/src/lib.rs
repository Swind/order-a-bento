//! HTTP interface adapter: axum routes, DTOs, error mapping and OpenAPI generation.

pub mod client;
pub mod error;
pub mod json;
pub mod logging;
pub mod openapi;
pub mod ratelimit;
pub mod routes;
pub mod session;
pub mod state;
#[cfg(test)]
pub(crate) mod testing;
pub mod version;
pub mod web;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa::openapi::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

pub use error::{AppError, ProblemDetails};
pub use ratelimit::RateLimitConfig;
pub use state::{AppState, WebSettings};

/// Path under which the generated spec is served.
pub const OPENAPI_PATH: &str = "/api/openapi.json";
/// Path of the Scalar UI.
pub const DOCS_PATH: &str = "/api/docs";

/// All routes registered with their OpenAPI metadata.
///
/// `dev_login` / `password_login` control whether `POST /auth/dev-login` /
/// `POST /auth/password-login` exist at all.
fn api_router(dev_login: bool, password_login: bool) -> OpenApiRouter<AppState> {
    // Further business routes are nested under `/api/v1` by later tasks.
    OpenApiRouter::with_openapi(openapi::ApiDoc::base())
        .merge(routes::health::router())
        .merge(routes::auth::router(dev_login, password_login))
        .nest(
            "/api/v1",
            routes::auth::v1_router()
                .merge(routes::me::router())
                .merge(routes::ai_import::router())
                .merge(routes::rosters::router())
                .merge(routes::vendors::router())
                .merge(routes::group_orders::router())
                .merge(routes::public::router()),
        )
}

/// The OpenAPI document (including the dev-only and password logins). Needs no database or environment.
pub fn openapi() -> OpenApi {
    api_router(true, true).split_for_parts().1
}

/// Build the full axum application serving the embedded SPA.
pub fn router(state: AppState) -> Router {
    router_with_assets(state, std::sync::Arc::new(web::EmbeddedAssets))
}

/// Like [`router`] with a custom static asset source (used by tests).
pub fn router_with_assets(state: AppState, assets: web::Assets) -> Router {
    let docs_enabled = state.docs_enabled;
    let (router, spec) =
        api_router(state.web.dev_login, state.web.password_login).split_for_parts();

    let spec_for_json = spec.clone();
    let mut app = router.route(
        OPENAPI_PATH,
        axum::routing::get(move || {
            let spec = spec_for_json.clone();
            async move { axum::Json(spec) }
        }),
    );
    if docs_enabled {
        app = app.merge(Scalar::with_url(DOCS_PATH, spec));
    }
    // `/g/{token}` is served here (OG meta injection), not by the plain SPA fallback.
    // The public rate limiter applies via `ClientCtx`.
    for path in ["/g/{token}", "/g/{token}/"] {
        let assets = assets.clone();
        app = app.route(
            path,
            axum::routing::get(
                move |State(state): State<AppState>,
                      _client: client::ClientCtx,
                      Path(token): Path<String>| {
                    let assets = assets.clone();
                    async move { web::share_page(&state, assets.as_ref(), &token).await }
                },
            ),
        );
    }
    app = app.fallback(move |method: axum::http::Method, uri: axum::http::Uri| {
        let assets = assets.clone();
        async move { web::serve(assets.as_ref(), &method, &uri) }
    });
    app.layer(axum::middleware::from_fn(session::refresh_cookie))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session::origin_check,
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(logging::trace_layer())
        .with_state(state)
}
