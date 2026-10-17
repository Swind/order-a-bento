//! Serves the embedded React SPA (design §3).
//!
//! Routing rules for requests that matched no API route:
//! - `/api/*`, `/auth/*`, `/healthz` never fall back to the SPA (ProblemDetails 404).
//! - an existing embedded file is served with its MIME type; files under `assets/`
//!   (content-hashed names) are cached immutably.
//! - everything else gets `index.html` with `Cache-Control: no-cache`.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::SystemTime;

use axum::body::Body;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use time::{OffsetDateTime, UtcOffset};

use crate::error::AppError;
use crate::routes::common::ordering_error;
use crate::state::AppState;

/// The built frontend. `allow_missing` keeps the backend compiling without `frontend/dist`.
#[derive(RustEmbed)]
#[folder = "../../../frontend/dist"]
#[allow_missing = true]
struct Dist;

pub const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";
pub const CACHE_NO_CACHE: &str = "no-cache";

/// Source of static files; abstracted so routing can be tested without a built frontend.
pub trait AssetSource: Send + Sync + 'static {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>>;
}

/// Assets embedded from `frontend/dist`.
pub struct EmbeddedAssets;

impl AssetSource for EmbeddedAssets {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        Dist::get(path).map(|f| f.data)
    }
}

/// True for paths that belong to the server and must never be answered by the SPA.
pub fn is_reserved_path(path: &str) -> bool {
    ["/api", "/auth"]
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
        || path == "/healthz"
}

/// `Cache-Control` for a static file path (without leading slash).
pub fn cache_control_for(file_path: &str) -> &'static str {
    if file_path.starts_with("assets/") {
        CACHE_IMMUTABLE
    } else {
        CACHE_NO_CACHE
    }
}

/// The raw `index.html`, or `None` when the frontend has not been built.
///
/// Used by [`share_page`] (`/g/{token}`), which injects Open Graph meta tags.
pub fn index_html(assets: &dyn AssetSource) -> Option<String> {
    assets
        .get("index.html")
        .map(|d| String::from_utf8_lossy(&d).into_owned())
}

/// Build an `index.html` response from an (optionally transformed) HTML string.
pub fn index_response(html: String) -> Response {
    let mut res = Response::new(Body::from(html));
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_NO_CACHE),
    );
    res
}

pub const SITE_NAME: &str = "訂便當";

/// Values injected into the share page's `<head>` (all escaped on output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OgMeta {
    pub title: String,
    pub description: Option<String>,
    /// Replacement text for `<title>`.
    pub document_title: String,
}

impl OgMeta {
    /// Identical for malformed and unknown tokens (reveals nothing).
    pub fn generic() -> Self {
        Self {
            title: SITE_NAME.into(),
            description: None,
            document_title: SITE_NAME.into(),
        }
    }

    pub fn for_group(
        title: &str,
        vendor: &str,
        deadline: Option<SystemTime>,
        accepting_orders: bool,
        offset: UtcOffset,
    ) -> Self {
        let mut desc = vendor.to_owned();
        if let Some(d) = deadline {
            let d = OffsetDateTime::from(d).to_offset(offset);
            desc.push_str(&format!(
                " · 截止 {}/{} {:02}:{:02}",
                u8::from(d.month()),
                d.day(),
                d.hour(),
                d.minute()
            ));
        }
        if !accepting_orders {
            desc.push_str(" · 已截止");
        }
        Self {
            title: title.into(),
            description: Some(desc),
            document_title: format!("{title} · {SITE_NAME}"),
        }
    }
}

/// Escape for HTML text and double/single-quoted attribute values.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Insert the meta tags before `</head>` and replace `<title>`. Pure.
///
/// No token and no `og:url` are ever emitted: the link itself is the secret.
pub fn inject_meta(index_html: &str, meta: &OgMeta) -> String {
    let mut tags = String::new();
    tags.push_str(&format!(
        "<meta property=\"og:title\" content=\"{}\">",
        escape_html(&meta.title)
    ));
    if let Some(d) = &meta.description {
        tags.push_str(&format!(
            "<meta property=\"og:description\" content=\"{}\">",
            escape_html(d)
        ));
    }
    tags.push_str(&format!(
        "<meta property=\"og:type\" content=\"website\"><meta property=\"og:site_name\" content=\"{}\">\
         <meta name=\"robots\" content=\"noindex, nofollow\"><meta name=\"referrer\" content=\"no-referrer\">",
        escape_html(SITE_NAME)
    ));
    let title_tag = format!("<title>{}</title>", escape_html(&meta.document_title));

    let mut html = index_html.to_owned();
    let mut had_title = false;
    if let Some(start) = html.find("<title>")
        && let Some(len) = html[start..].find("</title>")
    {
        html.replace_range(start..start + len + "</title>".len(), &title_tag);
        had_title = true;
    }
    if !had_title {
        tags.push_str(&title_tag);
    }
    match html.find("</head>") {
        Some(i) => html.insert_str(i, &tags),
        None => html.push_str(&tags),
    }
    html
}

/// `GET /g/{token}`: `index.html` with Open Graph tags for the group (or generic
/// tags when the token is malformed/unknown). Never logs or echoes the token.
pub async fn share_page(state: &AppState, assets: &dyn AssetSource, token: &str) -> Response {
    let meta = match state.ordering.get_public_group(token).await {
        Ok(v) => {
            let g = &v.group;
            OgMeta::for_group(
                &g.title().to_string(),
                &g.vendor().name,
                g.deadline_at(),
                v.accepting_orders,
                state.web.display_offset,
            )
        }
        Err(application::OrderingError::NotFound) => OgMeta::generic(),
        Err(e) => return ordering_error(e).into_response(),
    };
    let mut res = match index_html(assets) {
        Some(html) => index_response(inject_meta(&html, &meta)),
        None => not_built(),
    };
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::HeaderName::from_static("x-robots-tag"),
        HeaderValue::from_static("noindex"),
    );
    res
}

fn not_built() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "frontend not built: run `npm run build` in frontend/ and rebuild the server\n",
    )
        .into_response()
}

/// Fallback handler logic (see module docs).
pub fn serve(assets: &dyn AssetSource, method: &Method, uri: &Uri) -> Response {
    let path = uri.path();
    if is_reserved_path(path) {
        return AppError::NotFound(format!("no route for {path}")).into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return AppError::NotFound(format!("no route for {path}")).into_response();
    }
    let file = path.trim_start_matches('/');
    if !file.is_empty()
        && file != "index.html"
        && let Some(data) = assets.get(file)
    {
        let mime = mime_guess::from_path(file).first_or_octet_stream();
        let mut res = Response::new(Body::from(data.into_owned()));
        let h = res.headers_mut();
        if let Ok(v) = HeaderValue::from_str(mime.as_ref()) {
            h.insert(header::CONTENT_TYPE, v);
        }
        h.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control_for(file)),
        );
        return res;
    }
    match index_html(assets) {
        Some(html) => index_response(html),
        None => not_built(),
    }
}

/// Shared handle used by the router.
pub type Assets = Arc<dyn AssetSource>;

/// Fixed-`index.html` asset source for router tests elsewhere in the crate.
#[cfg(test)]
pub(crate) struct TestAssets(pub Option<&'static str>);

#[cfg(test)]
impl AssetSource for TestAssets {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        (path == "index.html")
            .then_some(self.0)
            .flatten()
            .map(|s| Cow::Borrowed(s.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use tower::ServiceExt;

    struct Fake(HashMap<&'static str, &'static str>);
    impl AssetSource for Fake {
        fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
            self.0.get(path).map(|s| Cow::Borrowed(s.as_bytes()))
        }
    }

    fn fake() -> Assets {
        Arc::new(Fake(HashMap::from([
            ("index.html", "<html>spa</html>"),
            ("assets/app-abc.js", "console.log(1)"),
            ("favicon.svg", "<svg/>"),
        ])))
    }

    fn app(assets: Assets) -> axum::Router {
        axum::Router::new().fallback(move |m: Method, u: Uri| {
            let a = assets.clone();
            async move { serve(a.as_ref(), &m, &u) }
        })
    }

    async fn get(app: axum::Router, path: &str) -> (Response, String) {
        let res = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = res.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        (
            Response::from_parts(parts, Body::empty()),
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    #[tokio::test]
    async fn spa_fallback_serves_index() {
        for p in ["/", "/rosters/123", "/g/abc"] {
            let (res, body) = get(app(fake()), p).await;
            assert_eq!(res.status(), 200, "{p}");
            assert_eq!(body, "<html>spa</html>");
            assert_eq!(res.headers()[header::CACHE_CONTROL], CACHE_NO_CACHE);
            assert_eq!(
                res.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
        }
    }

    #[tokio::test]
    async fn hashed_assets_are_immutable_with_mime() {
        let (res, body) = get(app(fake()), "/assets/app-abc.js").await;
        assert_eq!(res.status(), 200);
        assert_eq!(body, "console.log(1)");
        assert_eq!(res.headers()[header::CACHE_CONTROL], CACHE_IMMUTABLE);
        assert!(
            res.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .contains("javascript")
        );
        let (res, _) = get(app(fake()), "/favicon.svg").await;
        assert_eq!(res.headers()[header::CACHE_CONTROL], CACHE_NO_CACHE);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "image/svg+xml");
    }

    #[tokio::test]
    async fn reserved_paths_never_fall_back() {
        for p in ["/api/v1/nope", "/api", "/auth/whatever", "/healthz/x"] {
            let (res, body) = get(app(fake()), p).await;
            // `/healthz/x` is not reserved itself; only exact `/healthz` is.
            if p == "/healthz/x" {
                assert_eq!(res.status(), 200);
                continue;
            }
            assert_eq!(res.status(), 404, "{p}");
            assert_eq!(
                res.headers()[header::CONTENT_TYPE],
                "application/problem+json"
            );
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["code"], "not_found");
        }
    }

    #[tokio::test]
    async fn missing_frontend_is_503() {
        let (res, body) = get(app(Arc::new(Fake(HashMap::new()))), "/").await;
        assert_eq!(res.status(), 503);
        assert!(body.contains("frontend not built"));
    }

    const PAGE: &str = "<html><head><title>old</title></head><body></body></html>";

    #[test]
    fn inject_adds_tags_and_replaces_title() {
        let m = OgMeta::for_group(
            "週五午餐",
            "好吃便當",
            None,
            false,
            UtcOffset::from_hms(8, 0, 0).unwrap(),
        );
        let out = inject_meta(PAGE, &m);
        assert!(out.contains("<meta property=\"og:title\" content=\"週五午餐\">"));
        assert!(out.contains("content=\"好吃便當 · 已截止\""));
        assert!(out.contains("og:type\" content=\"website\""));
        assert!(out.contains("og:site_name\" content=\"訂便當\""));
        assert!(out.contains("name=\"robots\" content=\"noindex, nofollow\""));
        assert!(out.contains("name=\"referrer\" content=\"no-referrer\""));
        assert!(out.contains("<title>週五午餐 · 訂便當</title>"));
        assert!(!out.contains("old") && !out.contains("og:url"));
        assert!(out.find("og:title").unwrap() < out.find("</head>").unwrap());
    }

    #[test]
    fn inject_escapes_values() {
        let m = OgMeta::for_group(
            "\"><script>alert(1)</script>",
            "a&b'c",
            None,
            true,
            UtcOffset::UTC,
        );
        let out = inject_meta(PAGE, &m);
        assert!(!out.contains("<script>"));
        assert!(out.contains("content=\"&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;\""));
        assert!(out.contains("a&amp;b&#39;c"));
    }

    #[test]
    fn inject_without_title_or_head() {
        let out = inject_meta("<head></head>", &OgMeta::generic());
        assert!(out.contains("<title>訂便當</title>") && out.contains("og:title"));
        let out = inject_meta("<p>x</p>", &OgMeta::generic());
        assert!(out.contains("og:title") && out.starts_with("<p>x</p>"));
    }

    #[test]
    fn deadline_uses_display_offset() {
        // 2099-01-01T00:00:00Z
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(4_070_908_800);
        let m = OgMeta::for_group(
            "t",
            "v",
            Some(t),
            true,
            UtcOffset::from_hms(8, 0, 0).unwrap(),
        );
        assert_eq!(m.description.unwrap(), "v · 截止 1/1 08:00");
    }

    #[test]
    fn helpers() {
        assert!(is_reserved_path("/api/v1/x") && is_reserved_path("/healthz"));
        assert!(!is_reserved_path("/apix") && !is_reserved_path("/rosters"));
        assert_eq!(cache_control_for("assets/a.css"), CACHE_IMMUTABLE);
        assert_eq!(cache_control_for("index.html"), CACHE_NO_CACHE);
    }

    #[tokio::test]
    async fn full_router_unknown_api_is_problem_404() {
        let t = crate::testing::test_app(false, false).await;
        let app = crate::router_with_assets(t.state.clone(), fake());
        let (res, _) = get(app.clone(), "/api/v1/nope").await;
        assert_eq!(res.status(), 404);
        assert_eq!(
            res.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
        let (res, body) = get(app.clone(), "/rosters/123").await;
        assert_eq!(res.status(), 200);
        assert_eq!(body, "<html>spa</html>");
        let (res, _) = get(app, "/healthz").await;
        assert_eq!(res.status(), 200);
    }
}
