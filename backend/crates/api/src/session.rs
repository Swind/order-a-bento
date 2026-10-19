//! Session cookie handling, the `AuthenticatedOrganizer` extractor and the
//! Origin (CSRF) check.

use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use domain::identity::Organizer;

use application::AuthError;

use crate::error::AppError;
use crate::state::{AppState, WebSettings, origin_of};

pub const COOKIE_NAME: &str = "sid";

/// `Set-Cookie` value for a session cookie lasting until `expires_at`.
pub fn session_cookie(
    settings: &WebSettings,
    token: &str,
    expires_at: SystemTime,
    now: SystemTime,
) -> HeaderValue {
    let remaining = expires_at.duration_since(now).unwrap_or_default();
    // Round up so sub-second clock skew between calls never shaves a second off.
    let max_age = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    cookie_value(settings, token, max_age)
}

/// `Set-Cookie` value that removes the session cookie.
pub fn clear_cookie(settings: &WebSettings) -> HeaderValue {
    cookie_value(settings, "", 0)
}

fn cookie_value(settings: &WebSettings, token: &str, max_age: u64) -> HeaderValue {
    let secure = if settings.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure}"
    ))
    .expect("cookie value is ASCII")
}

/// The raw session token from the `Cookie` header, if any.
pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, v)| *k == COOKIE_NAME && !v.is_empty())
        .map(|(_, v)| v.to_owned())
}

pub fn auth_error(e: AuthError) -> AppError {
    match e {
        AuthError::OidcNotConfigured => AppError::Coded {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "oidc_not_configured",
            detail: "OIDC login is not configured on this server.".into(),
        },
        AuthError::Unauthenticated => AppError::Unauthorized("login required".into()),
        AuthError::InvalidCredentials => AppError::Coded {
            status: StatusCode::UNAUTHORIZED,
            code: "invalid_credentials",
            detail: "Invalid email or password.".into(),
        },
        AuthError::Invalid(d) => AppError::Validation(d.to_string()),
        AuthError::Provider(d) => {
            tracing::warn!(detail = %d, "identity provider failure");
            AppError::Coded {
                status: StatusCode::BAD_GATEWAY,
                code: "provider_error",
                detail: "The identity provider could not be reached.".into(),
            }
        }
        AuthError::InvalidState => AppError::Coded {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_state",
            detail: "Unknown, used or expired login state.".into(),
        },
        AuthError::EmailNotVerified => AppError::Coded {
            status: StatusCode::FORBIDDEN,
            code: "email_not_verified",
            detail: "The identity provider account email is not verified.".into(),
        },
        AuthError::Internal(d) => AppError::Internal(d),
    }
}

/// Slot the extractor fills when the sliding expiry was extended, so that
/// [`refresh_cookie`] can re-issue the cookie with the new `Max-Age`.
#[derive(Clone, Default)]
pub struct CookieRefresh(Arc<Mutex<Option<HeaderValue>>>);

/// Middleware: appends the refreshed session cookie requested by the extractor.
pub async fn refresh_cookie(mut req: Request, next: Next) -> Response {
    let slot = CookieRefresh::default();
    req.extensions_mut().insert(slot.clone());
    let mut res = next.run(req).await;
    if let Some(v) = slot.0.lock().expect("poisoned").take() {
        res.headers_mut().append(header::SET_COOKIE, v);
    }
    res
}

/// The logged-in organizer; rejects with a 401 problem otherwise.
#[derive(Debug, Clone)]
pub struct AuthenticatedOrganizer(pub Organizer);

impl FromRequestParts<AppState> for AuthenticatedOrganizer {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = session_token(&parts.headers)
            .ok_or_else(|| AppError::Unauthorized("login required".into()))?;
        let resolved = state
            .auth
            .resolve_session(&token)
            .await
            .map_err(auth_error)?;
        if resolved.extended
            && let Some(slot) = parts.extensions.get::<CookieRefresh>()
        {
            let v = session_cookie(
                &state.web,
                &token,
                resolved.expires_at,
                state.auth.clock.now(),
            );
            *slot.0.lock().expect("poisoned") = Some(v);
        }
        Ok(Self(resolved.organizer))
    }
}

fn is_guarded_path(path: &str) -> bool {
    path.starts_with("/api/") || path.starts_with("/auth/")
}

/// CSRF defence: state-changing requests carrying an `Origin` header must come
/// from an allowed origin. A missing `Origin` (non-browser client) is allowed.
pub async fn origin_check(State(state): State<AppState>, req: Request, next: Next) -> Response {
    use axum::response::IntoResponse;
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe
        && is_guarded_path(req.uri().path())
        && let Some(origin) = req.headers().get(header::ORIGIN)
    {
        let allowed = origin
            .to_str()
            .map(|o| {
                let o = origin_of(o);
                state.web.allowed_origins.contains(&o)
            })
            .unwrap_or(false);
        if !allowed {
            return AppError::Coded {
                status: StatusCode::FORBIDDEN,
                code: "bad_origin",
                detail: "Request origin is not allowed.".into(),
            }
            .into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(secure: bool) -> WebSettings {
        WebSettings {
            cookie_secure: secure,
            allowed_origins: vec![],
            dev_login: false,
            password_login: false,
            trusted_proxies: vec![],
            display_offset: time::UtcOffset::UTC,
        }
    }

    #[test]
    fn cookie_attributes() {
        let now = SystemTime::UNIX_EPOCH;
        let exp = now + std::time::Duration::from_secs(120);
        let c = session_cookie(&settings(false), "tok", exp, now);
        assert_eq!(
            c.to_str().unwrap(),
            "sid=tok; HttpOnly; SameSite=Lax; Path=/; Max-Age=120"
        );
        let c = session_cookie(&settings(true), "tok", exp, now);
        assert!(c.to_str().unwrap().ends_with("; Secure"));
        assert!(
            clear_cookie(&settings(false))
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }

    #[test]
    fn parses_cookie_header() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=b; sid=xyz; c=d"),
        );
        assert_eq!(session_token(&h).as_deref(), Some("xyz"));
        h.insert(header::COOKIE, HeaderValue::from_static("sid=; a=b"));
        assert_eq!(session_token(&h), None);
        assert_eq!(session_token(&HeaderMap::new()), None);
    }
}
