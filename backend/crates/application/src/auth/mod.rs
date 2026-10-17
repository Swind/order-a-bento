//! Authentication use cases: Google login, password login, dev login, session
//! resolution, logout, and local account management.

pub mod accounts;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use domain::identity::{Organizer, PASSWORD_MAX_LEN, normalize_email};
use domain::shared::DomainError;

use crate::ports::{
    Clock, IdentityProvider, OAuthState, OAuthStateRepo, OrganizerRepo, PasswordHasher, PortError,
    SessionRepo, TokenGenerator,
};
use crate::token::{TokenHash, pkce_challenge};

/// Lifetime of a pending OAuth login flow.
pub const OAUTH_STATE_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_RETURN_TO_LEN: usize = 2048;

#[derive(Debug, Clone, Copy)]
pub struct AuthConfig {
    pub session_ttl: Duration,
}

impl AuthConfig {
    pub fn from_days(days: u32) -> Self {
        Self {
            session_ttl: Duration::from_secs(u64::from(days) * 86_400),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("google login is not configured")]
    GoogleNotConfigured,
    #[error("unknown, used or expired login state")]
    InvalidState,
    #[error("google account email is not verified")]
    EmailNotVerified,
    #[error("identity provider failed: {0}")]
    Provider(String),
    /// Unknown email, wrong password or no password: deliberately indistinguishable.
    #[error("invalid email or password")]
    InvalidCredentials,
    #[error("not authenticated")]
    Unauthenticated,
    #[error(transparent)]
    Invalid(#[from] DomainError),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<PortError> for AuthError {
    fn from(e: PortError) -> Self {
        Self::Internal(e.0)
    }
}

impl AuthError {
    /// Stable code used in `/login?error=<code>` redirects.
    pub fn login_error_code(&self) -> &'static str {
        match self {
            Self::GoogleNotConfigured => "google_not_configured",
            Self::InvalidState => "invalid_state",
            Self::EmailNotVerified => "email_not_verified",
            Self::Provider(_) => "provider_error",
            Self::InvalidCredentials
            | Self::Unauthenticated
            | Self::Invalid(_)
            | Self::Internal(_) => "login_failed",
        }
    }
}

/// A freshly created session.
#[derive(Debug, Clone)]
pub struct NewSession {
    /// Raw token for the cookie; never persisted.
    pub token: String,
    pub expires_at: SystemTime,
}

#[derive(Debug, Clone)]
pub struct CompletedLogin {
    pub session: NewSession,
    pub return_to: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedSession {
    pub organizer: Organizer,
    pub expires_at: SystemTime,
    /// True when the sliding expiry was pushed out by this call.
    pub extended: bool,
}

/// Validate a post-login redirect target; anything that is not a plain
/// same-origin absolute path becomes `/`.
pub fn sanitize_return_to(raw: Option<&str>) -> String {
    match raw {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.contains('\\')
                && !p.chars().any(char::is_control)
                && p.len() <= MAX_RETURN_TO_LEN =>
        {
            p.to_owned()
        }
        _ => "/".to_owned(),
    }
}

#[derive(Clone)]
pub struct AuthService {
    pub organizers: Arc<dyn OrganizerRepo>,
    pub sessions: Arc<dyn SessionRepo>,
    pub oauth_states: Arc<dyn OAuthStateRepo>,
    /// Absent when Google credentials are not configured.
    pub identity: Option<Arc<dyn IdentityProvider>>,
    pub hasher: Arc<dyn PasswordHasher>,
    pub tokens: Arc<dyn TokenGenerator>,
    pub clock: Arc<dyn Clock>,
    pub config: AuthConfig,
}

impl AuthService {
    fn provider(&self) -> Result<&Arc<dyn IdentityProvider>, AuthError> {
        self.identity.as_ref().ok_or(AuthError::GoogleNotConfigured)
    }

    /// Start a Google login; returns the URL to redirect the browser to.
    pub async fn begin_google_login(&self, return_to: Option<&str>) -> Result<String, AuthError> {
        let provider = self.provider()?;
        let now = self.clock.now();
        let state = self.tokens.generate();
        let nonce = self.tokens.generate();
        let pkce_verifier = self.tokens.generate();
        let challenge = pkce_challenge(&pkce_verifier);
        self.oauth_states
            .save(OAuthState {
                state: state.clone(),
                pkce_verifier,
                nonce: nonce.clone(),
                return_to: sanitize_return_to(return_to),
                expires_at: now + OAUTH_STATE_TTL,
            })
            .await?;
        provider
            .authorize_url(&state, &nonce, &challenge)
            .await
            .map_err(|e| AuthError::Provider(e.to_string()))
    }

    /// Finish the Google login started by [`Self::begin_google_login`].
    pub async fn complete_google_login(
        &self,
        state: &str,
        code: &str,
        user_agent: Option<&str>,
    ) -> Result<CompletedLogin, AuthError> {
        let provider = self.provider()?;
        let now = self.clock.now();
        let saved = self
            .oauth_states
            .take(state, now)
            .await?
            .ok_or(AuthError::InvalidState)?;
        let identity = provider
            .exchange(code, &saved.pkce_verifier, &saved.nonce)
            .await
            .map_err(|e| AuthError::Provider(e.to_string()))?;
        if !identity.email_verified {
            return Err(AuthError::EmailNotVerified);
        }
        let display_name = identity
            .name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(identity.email.as_str());
        let candidate = Organizer::new(
            &identity.sub,
            &identity.email,
            display_name,
            identity.picture,
        )?;
        let organizer = self.organizers.upsert_from_identity(candidate, now).await?;
        let session = self.create_session(&organizer, user_agent, now).await?;
        Ok(CompletedLogin {
            session,
            return_to: sanitize_return_to(Some(&saved.return_to)),
        })
    }

    /// Dev-only shortcut: log in as `dev:<email>` without Google. The caller
    /// (composition root) must only expose this when `DEV_LOGIN=true`.
    pub async fn dev_login(
        &self,
        email: &str,
        display_name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<NewSession, AuthError> {
        let email = email.trim();
        if !email.contains('@') {
            return Err(DomainError::invalid("email", "格式不正確").into());
        }
        let now = self.clock.now();
        let name = display_name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| email.split('@').next().unwrap_or(email));
        let candidate = Organizer::new(&format!("dev:{email}"), email, name, None)?;
        let organizer = self.organizers.upsert_from_identity(candidate, now).await?;
        self.create_session(&organizer, user_agent, now).await
    }

    /// Log in a local account with email and password. Unknown email, wrong
    /// password and an account without a password all yield
    /// [`AuthError::InvalidCredentials`]; a missing account still costs one
    /// hash verification so the response time does not reveal it. The caller
    /// (composition root) must only expose this when `PASSWORD_LOGIN_ENABLED`.
    pub async fn password_login(
        &self,
        email: &str,
        password: &str,
        user_agent: Option<&str>,
    ) -> Result<NewSession, AuthError> {
        // Cheap guard against oversized inputs; no real password is this long.
        if password.chars().count() > PASSWORD_MAX_LEN {
            return Err(AuthError::InvalidCredentials);
        }
        let account = self
            .organizers
            .find_local_by_email(&normalize_email(email))
            .await?;
        let organizer = match account {
            Some(a) => match &a.password_hash {
                Some(hash) if self.hasher.verify(password, hash).await? => Some(a.organizer),
                Some(_) => None,
                None => {
                    self.hasher.verify_dummy(password).await;
                    None
                }
            },
            None => {
                self.hasher.verify_dummy(password).await;
                None
            }
        };
        let organizer = organizer.ok_or(AuthError::InvalidCredentials)?;
        let now = self.clock.now();
        self.organizers.touch_last_login(organizer.id, now).await?;
        self.create_session(&organizer, user_agent, now).await
    }

    async fn create_session(
        &self,
        organizer: &Organizer,
        user_agent: Option<&str>,
        now: SystemTime,
    ) -> Result<NewSession, AuthError> {
        let token = self.tokens.generate();
        let expires_at = now + self.config.session_ttl;
        self.sessions
            .create(
                TokenHash::of(&token),
                organizer.id,
                now,
                expires_at,
                user_agent,
            )
            .await?;
        Ok(NewSession { token, expires_at })
    }

    /// Resolve a raw session token to its organizer, sliding the expiry when
    /// less than half of the TTL remains.
    pub async fn resolve_session(&self, raw_token: &str) -> Result<ResolvedSession, AuthError> {
        let now = self.clock.now();
        let hash = TokenHash::of(raw_token);
        let (organizer_id, mut expires_at) = self
            .sessions
            .find_valid(hash, now)
            .await?
            .ok_or(AuthError::Unauthenticated)?;
        let organizer = self
            .organizers
            .find_by_id(organizer_id)
            .await?
            .ok_or(AuthError::Unauthenticated)?;
        let remaining = expires_at.duration_since(now).unwrap_or_default();
        let extended = remaining < self.config.session_ttl / 2;
        if extended {
            expires_at = now + self.config.session_ttl;
            self.sessions.extend(hash, expires_at).await?;
        }
        Ok(ResolvedSession {
            organizer,
            expires_at,
            extended,
        })
    }

    /// Delete the session (idempotent).
    pub async fn logout(&self, raw_token: &str) -> Result<(), AuthError> {
        self.sessions.delete(TokenHash::of(raw_token)).await?;
        Ok(())
    }

    /// Housekeeping for the daily background job (wired in a later task).
    pub async fn purge_expired(&self) -> Result<(), AuthError> {
        let now = self.clock.now();
        self.sessions.delete_expired(now).await?;
        self.oauth_states.delete_expired(now).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
