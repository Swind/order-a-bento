//! Directory use cases: rosters and vendors (master data).
//!
//! Every mutating use case is expressed as a closure over the aggregate handed
//! to the repository's `modify`, which runs load -> mutate -> persist in one
//! write transaction.

use std::sync::Arc;

use domain::directory::{
    MemberEdit, MemberInput, MenuItemEdit, MenuItemId, MenuItemInput, Roster, RosterId,
    RosterMemberId, UpsertReport, Vendor, VendorId, VendorInfoEdit,
};
use domain::identity::OrganizerId;
use domain::shared::{DomainError, NameKey, VendorName};

use crate::ports::{
    Clock, Modified, PortError, RepoError, RosterRepo, RosterSummary, Stored, VendorRepo,
    VendorSummary,
};

#[derive(Debug, thiserror::Error)]
pub enum DirectoryError {
    /// Unknown id, or an id owned by another organizer.
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Domain(DomainError),
    /// Another vendor of this organizer already has this name key.
    #[error("vendor name already exists")]
    VendorNameTaken { existing_id: Option<VendorId> },
    /// A uniqueness race that the in-memory checks did not catch.
    #[error("conflict")]
    Conflict,
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DomainError> for DirectoryError {
    fn from(e: DomainError) -> Self {
        match e {
            DomainError::NotFound { .. } => Self::NotFound,
            other => Self::Domain(other),
        }
    }
}

impl From<PortError> for DirectoryError {
    fn from(e: PortError) -> Self {
        Self::Internal(e.0)
    }
}

impl From<RepoError> for DirectoryError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound => Self::NotFound,
            RepoError::Domain(d) => d.into(),
            RepoError::UniqueViolation => Self::Conflict,
            RepoError::Port(p) => p.into(),
        }
    }
}

pub type RosterModified = Modified<Roster, UpsertReport<RosterMemberId>>;
pub type VendorModified = Modified<Vendor, UpsertReport<MenuItemId>>;

/// Data for creating a vendor.
#[derive(Debug, Clone, Default)]
pub struct NewVendor {
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
    pub menu_items: Vec<MenuItemInput>,
}

pub struct DirectoryService {
    pub delivery_addresses: Arc<dyn crate::ports::DeliveryAddressRepo>,
    pub rosters: Arc<dyn RosterRepo>,
    pub vendors: Arc<dyn VendorRepo>,
    pub clock: Arc<dyn Clock>,
}

impl DirectoryService {
    pub async fn save_delivery_address(
        &self,
        org: OrganizerId,
        id: Option<domain::directory::DeliveryAddressId>,
        name: &str,
        address: &str,
    ) -> Result<Stored<domain::directory::DeliveryAddress>, DirectoryError> {
        let mut value = domain::directory::DeliveryAddress::new(org, name, address)?;
        if let Some(id) = id {
            value.id = id;
        }
        Ok(self
            .delivery_addresses
            .save(value, self.clock.now(), id.is_none())
            .await?)
    }
    // ----- rosters ---------------------------------------------------------

    pub async fn list_rosters(
        &self,
        org: OrganizerId,
    ) -> Result<Vec<RosterSummary>, DirectoryError> {
        Ok(self.rosters.list(org).await?)
    }

    pub async fn get_roster(
        &self,
        org: OrganizerId,
        id: RosterId,
    ) -> Result<Stored<Roster>, DirectoryError> {
        self.rosters
            .find(org, id)
            .await?
            .ok_or(DirectoryError::NotFound)
    }

    pub async fn create_roster(
        &self,
        org: OrganizerId,
        name: &str,
        members: Vec<MemberInput>,
    ) -> Result<RosterModified, DirectoryError> {
        let mut roster = Roster::new(org, name)?;
        let report = if members.is_empty() {
            None
        } else {
            Some(roster.upsert_members(members)?)
        };
        let stored = self.rosters.create(roster, self.clock.now()).await?;
        Ok(Modified { stored, report })
    }

    async fn modify_roster(
        &self,
        org: OrganizerId,
        id: RosterId,
        mutation: crate::ports::RosterMutation,
    ) -> Result<RosterModified, DirectoryError> {
        Ok(self
            .rosters
            .modify(org, id, self.clock.now(), mutation)
            .await?)
    }

    pub async fn rename_roster(
        &self,
        org: OrganizerId,
        id: RosterId,
        name: String,
    ) -> Result<RosterModified, DirectoryError> {
        self.modify_roster(
            org,
            id,
            Box::new(move |r| {
                r.rename(&name)?;
                Ok(None)
            }),
        )
        .await
    }

    pub async fn delete_roster(
        &self,
        org: OrganizerId,
        id: RosterId,
    ) -> Result<(), DirectoryError> {
        if self.rosters.delete(org, id).await? {
            Ok(())
        } else {
            Err(DirectoryError::NotFound)
        }
    }

    pub async fn upsert_members(
        &self,
        org: OrganizerId,
        id: RosterId,
        members: Vec<MemberInput>,
    ) -> Result<RosterModified, DirectoryError> {
        self.modify_roster(
            org,
            id,
            Box::new(move |r| r.upsert_members(members).map(Some)),
        )
        .await
    }

    pub async fn edit_member(
        &self,
        org: OrganizerId,
        id: RosterId,
        member: RosterMemberId,
        edit: MemberEdit,
    ) -> Result<RosterModified, DirectoryError> {
        self.modify_roster(
            org,
            id,
            Box::new(move |r| {
                r.edit_member(member, edit)?;
                Ok(None)
            }),
        )
        .await
    }

    pub async fn remove_member(
        &self,
        org: OrganizerId,
        id: RosterId,
        member: RosterMemberId,
    ) -> Result<(), DirectoryError> {
        self.modify_roster(
            org,
            id,
            Box::new(move |r| {
                r.remove_member(member)?;
                Ok(None)
            }),
        )
        .await
        .map(|_| ())
    }

    // ----- vendors ---------------------------------------------------------

    pub async fn list_vendors(
        &self,
        org: OrganizerId,
    ) -> Result<Vec<VendorSummary>, DirectoryError> {
        Ok(self.vendors.list(org).await?)
    }

    pub async fn get_vendor(
        &self,
        org: OrganizerId,
        id: VendorId,
    ) -> Result<Stored<Vendor>, DirectoryError> {
        self.vendors
            .find(org, id)
            .await?
            .ok_or(DirectoryError::NotFound)
    }

    /// Map a unique violation on a vendor write to `VendorNameTaken`, looking up
    /// the vendor that owns the colliding name when it is known.
    async fn name_taken(
        &self,
        org: OrganizerId,
        name: Option<&str>,
        own_id: Option<VendorId>,
    ) -> DirectoryError {
        let existing_id = match name.and_then(|n| VendorName::new(n).ok()) {
            Some(n) => self
                .vendors
                .find_id_by_name_key(org, &n.key())
                .await
                .ok()
                .flatten()
                .filter(|found| Some(*found) != own_id),
            None => None,
        };
        match existing_id {
            Some(_) => DirectoryError::VendorNameTaken { existing_id },
            // The unique violation came from somewhere else (e.g. a menu item race).
            None => DirectoryError::Conflict,
        }
    }

    pub async fn create_vendor(
        &self,
        org: OrganizerId,
        new: NewVendor,
    ) -> Result<VendorModified, DirectoryError> {
        let mut vendor = Vendor::new(org, &new.name)?;
        vendor.edit_info(VendorInfoEdit {
            name: None,
            phone: new
                .phone
                .map_or_else(Default::default, domain::shared::Patch::Set),
            address: new
                .address
                .map_or_else(Default::default, domain::shared::Patch::Set),
            note: new
                .note
                .map_or_else(Default::default, domain::shared::Patch::Set),
        })?;
        let report = if new.menu_items.is_empty() {
            None
        } else {
            Some(vendor.upsert_menu_items(new.menu_items)?)
        };
        // Cheap pre-check for a good error; the UNIQUE constraint remains the
        // authority and is handled below.
        let key: NameKey = vendor.name().key();
        if let Some(existing) = self.vendors.find_id_by_name_key(org, &key).await? {
            return Err(DirectoryError::VendorNameTaken {
                existing_id: Some(existing),
            });
        }
        match self.vendors.create(vendor, self.clock.now()).await {
            Ok(stored) => Ok(Modified { stored, report }),
            Err(RepoError::UniqueViolation) => {
                Err(self.name_taken(org, Some(&new.name), None).await)
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn modify_vendor(
        &self,
        org: OrganizerId,
        id: VendorId,
        new_name: Option<String>,
        mutation: crate::ports::VendorMutation,
    ) -> Result<VendorModified, DirectoryError> {
        match self
            .vendors
            .modify(org, id, self.clock.now(), mutation)
            .await
        {
            Ok(m) => Ok(m),
            Err(RepoError::UniqueViolation) => {
                Err(self.name_taken(org, new_name.as_deref(), Some(id)).await)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn edit_vendor(
        &self,
        org: OrganizerId,
        id: VendorId,
        edit: VendorInfoEdit,
    ) -> Result<VendorModified, DirectoryError> {
        let new_name = edit.name.clone();
        self.modify_vendor(
            org,
            id,
            new_name,
            Box::new(move |v| {
                v.edit_info(edit)?;
                Ok(None)
            }),
        )
        .await
    }

    pub async fn delete_vendor(
        &self,
        org: OrganizerId,
        id: VendorId,
    ) -> Result<(), DirectoryError> {
        if self.vendors.delete(org, id).await? {
            Ok(())
        } else {
            Err(DirectoryError::NotFound)
        }
    }

    pub async fn upsert_menu_items(
        &self,
        org: OrganizerId,
        id: VendorId,
        items: Vec<MenuItemInput>,
    ) -> Result<VendorModified, DirectoryError> {
        self.modify_vendor(
            org,
            id,
            None,
            Box::new(move |v| v.upsert_menu_items(items).map(Some)),
        )
        .await
    }

    pub async fn edit_menu_item(
        &self,
        org: OrganizerId,
        id: VendorId,
        item: MenuItemId,
        edit: MenuItemEdit,
    ) -> Result<VendorModified, DirectoryError> {
        self.modify_vendor(
            org,
            id,
            None,
            Box::new(move |v| {
                v.edit_item(item, edit)?;
                Ok(None)
            }),
        )
        .await
    }

    pub async fn remove_menu_item(
        &self,
        org: OrganizerId,
        id: VendorId,
        item: MenuItemId,
    ) -> Result<(), DirectoryError> {
        self.modify_vendor(
            org,
            id,
            None,
            Box::new(move |v| {
                v.remove_item(item)?;
                Ok(None)
            }),
        )
        .await
        .map(|_| ())
    }
}
