//! Local (password) account management, used by the `server admin` CLI and the
//! `ADMIN_EMAIL`/`ADMIN_PASSWORD` startup bootstrap. No web UI creates accounts.

use std::sync::Arc;

use domain::identity::{Organizer, normalize_email, parse_local_email, validate_password};
use domain::shared::DomainError;

use crate::ports::{
    AccountSummary, Clock, CreateLocalOutcome, OrganizerRepo, PasswordHasher, PortError,
};

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error(transparent)]
    Invalid(#[from] DomainError),
    #[error("a local account with this email already exists")]
    EmailTaken,
    #[error("no local account with this email")]
    NotFound,
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<PortError> for AccountError {
    fn from(e: PortError) -> Self {
        Self::Internal(e.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapOutcome {
    Created,
    /// An organizer with this email already exists; nothing was changed.
    AlreadyExists,
}

#[derive(Clone)]
pub struct AccountService {
    pub organizers: Arc<dyn OrganizerRepo>,
    pub hasher: Arc<dyn PasswordHasher>,
    pub clock: Arc<dyn Clock>,
}

impl AccountService {
    /// Create a local account. Emails are unique among local accounts only: a
    /// Google account with the same address is a separate organizer (Google
    /// login never links to a local account).
    pub async fn create(
        &self,
        email: &str,
        display_name: &str,
        password: &str,
    ) -> Result<Organizer, AccountError> {
        let organizer = Organizer::new_local(email, display_name)?;
        validate_password(password)?;
        let hash = self.hasher.hash(password).await?;
        match self
            .organizers
            .create_local(organizer.clone(), hash, self.clock.now())
            .await?
        {
            CreateLocalOutcome::Created => Ok(organizer),
            CreateLocalOutcome::EmailTaken => Err(AccountError::EmailTaken),
        }
    }

    /// Replace the password of an existing local account.
    pub async fn set_password(&self, email: &str, password: &str) -> Result<(), AccountError> {
        let email = parse_local_email(email)?;
        validate_password(password)?;
        let account = self
            .organizers
            .find_local_by_email(&email)
            .await?
            .ok_or(AccountError::NotFound)?;
        let hash = self.hasher.hash(password).await?;
        if self
            .organizers
            .set_password_hash(account.organizer.id, hash)
            .await?
        {
            Ok(())
        } else {
            Err(AccountError::NotFound)
        }
    }

    pub async fn list(&self) -> Result<Vec<AccountSummary>, AccountError> {
        Ok(self.organizers.list_accounts().await?)
    }

    /// Env bootstrap: create the local admin unless an organizer with that
    /// email already exists. Never touches an existing account or password.
    pub async fn bootstrap_admin(
        &self,
        email: &str,
        password: &str,
    ) -> Result<BootstrapOutcome, AccountError> {
        let email = parse_local_email(email)?;
        if self.organizers.exists_by_email(&email).await? {
            return Ok(BootstrapOutcome::AlreadyExists);
        }
        let name = email.split('@').next().unwrap_or(&email);
        match self.create(&normalize_email(&email), name, password).await {
            Ok(_) => Ok(BootstrapOutcome::Created),
            // Lost a race with another instance starting at the same time.
            Err(AccountError::EmailTaken) => Ok(BootstrapOutcome::AlreadyExists),
            Err(e) => Err(e),
        }
    }
}
