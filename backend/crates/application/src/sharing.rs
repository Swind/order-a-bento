//! Local sharing membership, independent of ordering groups and OIDC claims.
use async_trait::async_trait;
use domain::identity::OrganizerId;
use domain::shared::DomainError;

pub use domain::sharing::SharingGroupId;

#[derive(Debug, Clone)]
pub struct SharingGroup {
    pub id: SharingGroupId,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct SharingUser {
    pub id: OrganizerId,
    pub display_name: String,
    pub email: String,
    pub identity_kind: String,
    pub role: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SharingError {
    #[error("not found")]
    NotFound,
    #[error("admin role required")]
    Forbidden,
    #[error("last administrator cannot be removed or demoted")]
    LastAdmin,
    #[error("already a member")]
    Conflict,
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Port(#[from] crate::ports::PortError),
}

pub fn role(raw: &str) -> Result<&str, SharingError> {
    match raw {
        "admin" | "member" => Ok(raw),
        _ => Err(DomainError::invalid("role", "must be admin or member").into()),
    }
}

#[async_trait]
pub trait SharingRepo: Send + Sync {
    async fn list(&self, actor: OrganizerId) -> Result<Vec<SharingGroup>, SharingError>;
    async fn create(&self, actor: OrganizerId, name: &str) -> Result<SharingGroup, SharingError>;
    async fn get(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
    ) -> Result<SharingGroup, SharingError>;
    async fn rename(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        name: &str,
    ) -> Result<SharingGroup, SharingError>;
    async fn members(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
    ) -> Result<Vec<SharingUser>, SharingError>;
    async fn lookup(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        email: &str,
    ) -> Result<Vec<SharingUser>, SharingError>;
    async fn add(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        user: OrganizerId,
        role: &str,
    ) -> Result<(), SharingError>;
    async fn change(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        user: OrganizerId,
        role: Option<&str>,
    ) -> Result<(), SharingError>;
}
