//! `/auth/*`: Google OIDC login, password login, logout and the dev-only login shortcut.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::client::{ClientCtx, rate_limited};
use crate::error::{AppError, ProblemDetails};
use crate::openapi::TAG_AUTH;
use crate::session::{auth_error, clear_cookie, session_cookie, session_token};
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query, rename_all = "camelCase")]
pub struct LoginQuery {
    /// Same-origin path to return to after login (default `/`).
    pub return_to: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Set by Google when the user denied access.
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevLoginRequest {
    pub email: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordLoginRequest {
    pub email: String,
    pub password: String,
}

// Never derive `Debug` for the request: it carries a password.

fn redirect(location: &str) -> Response {
    let mut res = StatusCode::FOUND.into_response();
    if let Ok(v) = HeaderValue::from_str(location) {
        res.headers_mut().insert(header::LOCATION, v);
    }
    res
}

fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
}

/// Start the Google login: redirects to Google (PKCE + state + nonce).
#[utoipa::path(
    get,
    path = "/auth/google/login",
    tag = TAG_AUTH,
    params(LoginQuery),
    responses(
        (status = 302, description = "Redirect to Google's authorization endpoint"),
        (status = 503, description = "Google login is not configured (code google_not_configured)",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 502, description = "Identity provider unreachable", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn google_login(
    State(state): State<AppState>,
    Query(q): Query<LoginQuery>,
) -> Result<Response, AppError> {
    let url = state
        .auth
        .begin_google_login(q.return_to.as_deref())
        .await
        .map_err(auth_error)?;
    Ok(redirect(&url))
}

/// OAuth callback: verifies the login, sets the session cookie and redirects.
/// Any failure redirects to `/login?error=<code>`.
#[utoipa::path(
    get,
    path = "/auth/google/callback",
    tag = TAG_AUTH,
    params(CallbackQuery),
    responses(
        (status = 302, description = "Redirect to the original page with a session cookie, or to /login?error=<code>"),
    )
)]
pub async fn google_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let fail = |code: &str| redirect(&format!("/login?error={code}"));
    if let Some(e) = q.error {
        let safe = !e.is_empty() && e.chars().all(|c| c.is_ascii_lowercase() || c == '_');
        return fail(if safe { &e } else { "login_failed" });
    }
    let (Some(code), Some(st)) = (q.code.as_deref(), q.state.as_deref()) else {
        return fail("invalid_request");
    };
    match state
        .auth
        .complete_google_login(st, code, user_agent(&headers))
        .await
    {
        Ok(done) => {
            let mut res = redirect(&done.return_to);
            res.headers_mut().append(
                header::SET_COOKIE,
                session_cookie(
                    &state.web,
                    &done.session.token,
                    done.session.expires_at,
                    state.auth.clock.now(),
                ),
            );
            res
        }
        Err(e) => {
            tracing::warn!(error = %e, "google login failed");
            fail(e.login_error_code())
        }
    }
}

/// Log out: deletes the session and clears the cookie.
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = TAG_AUTH,
    responses((status = 204, description = "Logged out (idempotent)"))
)]
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Some(token) = session_token(&headers) {
        state.auth.logout(&token).await.map_err(auth_error)?;
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .append(header::SET_COOKIE, clear_cookie(&state.web));
    Ok(res)
}

/// Development-only login without Google. Registered only when `DEV_LOGIN=true`.
#[utoipa::path(
    post,
    path = "/auth/dev-login",
    tag = TAG_AUTH,
    request_body = DevLoginRequest,
    responses(
        (status = 204, description = "Logged in; session cookie set"),
        (status = 422, description = "Invalid email", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn dev_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DevLoginRequest>,
) -> Result<Response, AppError> {
    let s = state
        .auth
        .dev_login(
            &body.email,
            body.display_name.as_deref(),
            user_agent(&headers),
        )
        .await
        .map_err(auth_error)?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().append(
        header::SET_COOKIE,
        session_cookie(&state.web, &s.token, s.expires_at, state.auth.clock.now()),
    );
    Ok(res)
}

/// Log in with a local account's email and password. Registered only when
/// `PASSWORD_LOGIN_ENABLED` (default true). Subject to a stricter per-IP limit.
#[utoipa::path(
    post,
    path = "/auth/password-login",
    tag = TAG_AUTH,
    request_body = PasswordLoginRequest,
    responses(
        (status = 204, description = "Logged in; session cookie set"),
        (status = 401, description = "Unknown email, wrong password or no password (code invalid_credentials)",
         body = ProblemDetails, content_type = "application/problem+json"),
        (status = 429, description = "Too many attempts (code rate_limited)", body = ProblemDetails,
         content_type = "application/problem+json"),
    )
)]
pub async fn password_login(
    State(state): State<AppState>,
    client: ClientCtx,
    headers: HeaderMap,
    Json(body): Json<PasswordLoginRequest>,
) -> Result<Response, AppError> {
    if let Err(retry) = state.login_limiter.check(client.ip, Instant::now()) {
        return Ok(rate_limited(retry));
    }
    let s = state
        .auth
        .password_login(&body.email, &body.password, user_agent(&headers))
        .await
        .map_err(auth_error)?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().append(
        header::SET_COOKIE,
        session_cookie(&state.web, &s.token, s.expires_at, state.auth.clock.now()),
    );
    Ok(res)
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigResponse {
    /// Whether Google login is enabled (`GOOGLE_OAUTH_ENABLED`) and configured.
    pub google_enabled: bool,
    /// Whether email + password login for local accounts is available.
    pub password_login_enabled: bool,
    /// Whether the development-only login form is available.
    pub dev_login_enabled: bool,
    /// Whether AI import is available (an API key or the demo agent is configured).
    pub ai_import_enabled: bool,
    /// Whether AI import runs on scripted demo data (show a "demo mode" badge).
    pub ai_demo: bool,
}

/// Which login methods are available (public).
#[utoipa::path(
    get,
    path = "/auth/config",
    tag = TAG_AUTH,
    responses((status = 200, description = "Available login methods", body = AuthConfigResponse))
)]
pub async fn auth_config(State(state): State<AppState>) -> Json<AuthConfigResponse> {
    Json(AuthConfigResponse {
        google_enabled: state.auth.identity.is_some(),
        password_login_enabled: state.web.password_login,
        dev_login_enabled: state.web.dev_login,
        ai_import_enabled: state.ai_import.is_some(),
        ai_demo: state.ai_import.is_some() && state.ai_demo,
    })
}

/// Public JSON endpoints mounted under `/api/v1`.
pub fn v1_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(auth_config))
}

/// `dev_login` and `password_login` are only included when their flag is true.
pub fn router(with_dev_login: bool, with_password_login: bool) -> OpenApiRouter<AppState> {
    let mut r = OpenApiRouter::new()
        .routes(routes!(google_login))
        .routes(routes!(google_callback))
        .routes(routes!(logout));
    if with_dev_login {
        r = r.routes(routes!(dev_login));
    }
    if with_password_login {
        r = r.routes(routes!(password_login));
    }
    r
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, Response, StatusCode, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::router;
    use crate::testing::test_app;

    async fn app(dev: bool, google: bool) -> (Router, crate::testing::TestApp) {
        let t = test_app(dev, google).await;
        (router(t.state.clone()), t)
    }

    async fn send(app: &Router, req: Request<Body>) -> Response<Body> {
        app.clone().oneshot(req).await.unwrap()
    }

    fn post_json(path: &str, body: &str) -> Request<Body> {
        Request::post(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    fn header_str(res: &Response<Body>, name: header::HeaderName) -> &str {
        res.headers()[name].to_str().unwrap()
    }

    /// `sid=<token>` pair from a Set-Cookie header.
    fn sid_pair(res: &Response<Body>) -> String {
        header_str(res, header::SET_COOKIE)
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }

    fn get_with_cookie(path: &str, cookie: &str) -> Request<Body> {
        Request::get(path)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    }

    async fn json(res: Response<Body>) -> serde_json::Value {
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }

    async fn dev_login(app: &Router) -> String {
        let res = send(app, post_json("/auth/dev-login", r#"{"email":"a@b.c"}"#)).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        sid_pair(&res)
    }

    #[tokio::test]
    async fn dev_login_then_me_then_logout() {
        let (app, _t) = app(true, false).await;
        let res = send(
            &app,
            post_json(
                "/auth/dev-login",
                r#"{"email":"a@b.c","displayName":"Ann"}"#,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let set = header_str(&res, header::SET_COOKIE).to_owned();
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax") && set.contains("Path=/"));
        assert!(set.contains("Max-Age=1209600"), "{set}");
        assert!(!set.contains("Secure"));
        let sid = sid_pair(&res);

        let me = send(&app, get_with_cookie("/api/v1/me", &sid)).await;
        assert_eq!(me.status(), StatusCode::OK);
        let v = json(me).await;
        assert_eq!(v["email"], "a@b.c");
        assert_eq!(v["displayName"], "Ann");
        assert!(v["avatarUrl"].is_null());
        assert!(v["id"].as_str().is_some());

        let out = send(
            &app,
            Request::post("/auth/logout")
                .header(header::COOKIE, &sid)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(out.status(), StatusCode::NO_CONTENT);
        assert!(header_str(&out, header::SET_COOKIE).contains("Max-Age=0"));

        let me = send(&app, get_with_cookie("/api/v1/me", &sid)).await;
        assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_requires_login_with_problem_json() {
        let (app, _t) = app(false, false).await;
        let res = send(
            &app,
            Request::get("/api/v1/me").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            header_str(&res, header::CONTENT_TYPE),
            "application/problem+json"
        );
        assert_eq!(json(res).await["code"], "unauthorized");
        let res = send(&app, get_with_cookie("/api/v1/me", "sid=garbage")).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_config_reports_methods() {
        for (dev, google) in [(true, false), (false, true)] {
            let (app, _t) = app(dev, google).await;
            let res = send(
                &app,
                Request::get("/api/v1/auth/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(res.status(), StatusCode::OK);
            let v = json(res).await;
            assert_eq!(v["devLoginEnabled"], dev);
            assert_eq!(v["googleEnabled"], google);
            assert_eq!(v["passwordLoginEnabled"], true);
        }
    }

    async fn make_admin(t: &crate::testing::TestApp, email: &str, password: &str) {
        application::AccountService {
            organizers: t.state.auth.organizers.clone(),
            hasher: t.state.auth.hasher.clone(),
            clock: t.state.auth.clock.clone(),
        }
        .create(email, "Root", password)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn password_login_then_me() {
        let (app, t) = app(false, false).await;
        make_admin(&t, "admin@example.com", "correct horse").await;
        let res = send(
            &app,
            post_json(
                "/auth/password-login",
                r#"{"email":" Admin@Example.com","password":"correct horse"}"#,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        // Same cookie as every other login method.
        let set = header_str(&res, header::SET_COOKIE).to_owned();
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax") && set.contains("Path=/"));
        assert!(set.contains("Max-Age=1209600"), "{set}");
        let sid = sid_pair(&res);

        let me = json(send(&app, get_with_cookie("/api/v1/me", &sid)).await).await;
        assert_eq!(me["email"], "admin@example.com");
        assert_eq!(me["displayName"], "Root");

        let (ll, ca): (String, String) =
            sqlx::query_as("SELECT last_login_at, created_at FROM organizers")
                .fetch_one(&t.state.db.read)
                .await
                .unwrap();
        assert!(ll >= ca);
    }

    #[tokio::test]
    async fn password_login_failures_are_identical() {
        let (app, t) = app(false, false).await;
        make_admin(&t, "admin@example.com", "correct horse").await;
        let mut bodies = Vec::new();
        for body in [
            r#"{"email":"admin@example.com","password":"wrong wrong wrong"}"#,
            r#"{"email":"nobody@example.com","password":"correct horse"}"#,
            r#"{"email":"","password":""}"#,
        ] {
            let res = send(&app, post_json("/auth/password-login", body)).await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                header_str(&res, header::CONTENT_TYPE),
                "application/problem+json"
            );
            assert!(res.headers().get(header::SET_COOKIE).is_none());
            bodies.push(json(res).await);
        }
        assert_eq!(bodies[0]["code"], "invalid_credentials");
        assert_eq!(bodies[0], bodies[1]);
        assert_eq!(bodies[0], bodies[2]);
    }

    #[tokio::test]
    async fn password_login_does_not_reach_google_accounts() {
        let (app, _t) = app(true, false).await;
        // A dev/Google organizer (no password) cannot be logged into with a password.
        dev_login(&app).await;
        let res = send(
            &app,
            post_json(
                "/auth/password-login",
                r#"{"email":"a@b.c","password":"whatever it is"}"#,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn password_login_not_registered_when_disabled() {
        let mut t = test_app(false, false).await;
        t.state.web = std::sync::Arc::new(
            crate::WebSettings::new("http://localhost:3000", &[], false).with_password_login(false),
        );
        let app = router(t.state.clone());
        let res = send(
            &app,
            post_json(
                "/auth/password-login",
                r#"{"email":"a@b.c","password":"x"}"#,
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let cfg = json(
            send(
                &app,
                Request::get("/api/v1/auth/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await,
        )
        .await;
        assert_eq!(cfg["passwordLoginEnabled"], false);
    }

    #[tokio::test]
    async fn password_login_is_rate_limited_per_ip_and_origin_checked() {
        let t = test_app(false, false).await;
        let state = t
            .state
            .clone()
            .with_login_rate_limit(crate::RateLimitConfig {
                burst: 3,
                per_sec: 0.001,
            });
        let app = router(state);
        let bad = r#"{"email":"a@b.c","password":"nope nope nope"}"#;
        for _ in 0..3 {
            let res = send(&app, post_json("/auth/password-login", bad)).await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
        let res = send(&app, post_json("/auth/password-login", bad)).await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(res.headers().contains_key(header::RETRY_AFTER));
        assert_eq!(json(res).await["code"], "rate_limited");

        let forged = Request::post("/auth/password-login")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "https://evil.com")
            .body(Body::from(bad))
            .unwrap();
        let res = send(&app, forged).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(json(res).await["code"], "bad_origin");
    }

    #[tokio::test]
    async fn google_endpoints_503_when_disabled() {
        let (app, _t) = app(false, false).await;
        let res = send(
            &app,
            Request::get("/auth/google/callback?code=good&state=x")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        // The callback reports failures by redirecting to the login page.
        assert_eq!(res.status(), StatusCode::FOUND);
        assert_eq!(
            header_str(&res, header::LOCATION),
            "/login?error=google_not_configured"
        );
        let cfg = json(
            send(
                &app,
                Request::get("/api/v1/auth/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await,
        )
        .await;
        assert_eq!(cfg["googleEnabled"], false);
    }

    #[tokio::test]
    async fn dev_login_not_registered_when_disabled() {
        let (app, _t) = app(false, false).await;
        let res = send(&app, post_json("/auth/dev-login", r#"{"email":"a@b.c"}"#)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn dev_login_rejects_bad_email() {
        let (app, _t) = app(true, false).await;
        let res = send(&app, post_json("/auth/dev-login", r#"{"email":"nope"}"#)).await;
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn full_google_flow_with_fake_provider() {
        let (app, _t) = app(false, true).await;
        let res = send(
            &app,
            Request::get("/auth/google/login?returnTo=/admin/orders")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FOUND);
        let loc = header_str(&res, header::LOCATION).to_owned();
        assert!(
            loc.starts_with("https://accounts.fake/auth?state="),
            "{loc}"
        );
        let state = loc
            .split("state=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();

        let cb = send(
            &app,
            Request::get(format!("/auth/google/callback?code=good&state={state}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(cb.status(), StatusCode::FOUND);
        assert_eq!(header_str(&cb, header::LOCATION), "/admin/orders");
        let sid = sid_pair(&cb);

        let me = json(send(&app, get_with_cookie("/api/v1/me", &sid)).await).await;
        assert_eq!(me["email"], "google@user.test");
        assert_eq!(me["displayName"], "Gina Google");
        assert_eq!(me["avatarUrl"], "https://pics.test/g.png");

        // The state is single-use.
        let again = send(
            &app,
            Request::get(format!("/auth/google/callback?code=good&state={state}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(
            header_str(&again, header::LOCATION),
            "/login?error=invalid_state"
        );
        assert!(again.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn open_redirect_return_to_is_neutralised() {
        let (app, _t) = app(false, true).await;
        let res = send(
            &app,
            Request::get("/auth/google/login?returnTo=//evil.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let loc = header_str(&res, header::LOCATION).to_owned();
        let state = loc
            .split("state=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        let cb = send(
            &app,
            Request::get(format!("/auth/google/callback?code=good&state={state}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(header_str(&cb, header::LOCATION), "/");
    }

    #[tokio::test]
    async fn callback_failures_redirect_to_login() {
        let (app, _t) = app(false, true).await;
        let loc = |res: &Response<Body>| header_str(res, header::LOCATION).to_owned();

        let res = send(
            &app,
            Request::get("/auth/google/callback?code=good&state=bogus")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FOUND);
        assert_eq!(loc(&res), "/login?error=invalid_state");

        let res = send(
            &app,
            Request::get("/auth/google/callback")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(loc(&res), "/login?error=invalid_request");

        let res = send(
            &app,
            Request::get("/auth/google/callback?error=access_denied")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(loc(&res), "/login?error=access_denied");

        let res = send(
            &app,
            Request::get("/auth/google/callback?error=%3Cscript%3E")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(loc(&res), "/login?error=login_failed");

        // Unverified email and provider failure.
        for (code, expected) in [
            ("unverified", "email_not_verified"),
            ("nope", "provider_error"),
        ] {
            let res = send(
                &app,
                Request::get("/auth/google/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            let l = header_str(&res, header::LOCATION).to_owned();
            let state = l.split("state=").nth(1).unwrap().split('&').next().unwrap();
            let res = send(
                &app,
                Request::get(format!("/auth/google/callback?code={code}&state={state}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(res.status(), StatusCode::FOUND);
            assert_eq!(loc(&res), format!("/login?error={expected}"));
        }
    }

    #[tokio::test]
    async fn google_not_configured_is_503() {
        let (app, _t) = app(false, false).await;
        let res = send(
            &app,
            Request::get("/auth/google/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json(res).await["code"], "google_not_configured");
    }

    #[tokio::test]
    async fn origin_check_on_state_changing_requests() {
        let (app, _t) = app(true, false).await;
        let req = |origin: Option<&str>| {
            let mut b =
                Request::post("/auth/dev-login").header(header::CONTENT_TYPE, "application/json");
            if let Some(o) = origin {
                b = b.header(header::ORIGIN, o);
            }
            b.body(Body::from(r#"{"email":"a@b.c"}"#)).unwrap()
        };
        let res = send(&app, req(Some("https://evil.com"))).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(json(res).await["code"], "bad_origin");
        assert_eq!(
            send(&app, req(Some("null"))).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(&app, req(Some("http://localhost:3000")))
                .await
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            send(&app, req(Some("http://localhost:5173")))
                .await
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(send(&app, req(None)).await.status(), StatusCode::NO_CONTENT);

        // Logout is guarded too; GET with a foreign Origin is not.
        let out = Request::post("/auth/logout")
            .header(header::ORIGIN, "https://evil.com")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, out).await.status(), StatusCode::FORBIDDEN);
        let get = Request::get("/api/v1/me")
            .header(header::ORIGIN, "https://evil.com")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, get).await.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn cookie_refreshed_when_session_past_half_life() {
        let (app, t) = app(true, false).await;
        let sid = dev_login(&app).await;
        // No refresh right after login.
        let res = send(&app, get_with_cookie("/api/v1/me", &sid)).await;
        assert!(res.headers().get(header::SET_COOKIE).is_none());
        // Age the session to 3 days remaining.
        sqlx::query("UPDATE sessions SET expires_at = ?")
            .bind(infrastructure::timefmt::to_text(
                std::time::SystemTime::now() + std::time::Duration::from_secs(3 * 86_400),
            ))
            .execute(&t.state.db.write)
            .await
            .unwrap();
        let res = send(&app, get_with_cookie("/api/v1/me", &sid)).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(header_str(&res, header::SET_COOKIE).contains("Max-Age=12096"));
    }

    #[test]
    fn openapi_documents_auth_routes() {
        let doc = serde_json::to_value(crate::openapi()).unwrap();
        for (p, m) in [
            ("/auth/google/login", "get"),
            ("/auth/google/callback", "get"),
            ("/auth/logout", "post"),
            ("/auth/dev-login", "post"),
            ("/auth/password-login", "post"),
            ("/api/v1/me", "get"),
        ] {
            assert!(doc["paths"][p][m].is_object(), "{p} {m}");
        }
    }
}
