//! SQLite repositories for rosters, vendors and their children.
//!
//! # Transactions
//!
//! Every mutation runs *load -> mutate -> persist* inside one transaction on the
//! single-connection write pool, opened with `BEGIN IMMEDIATE`. The pool has
//! exactly one connection and this process is the only writer, so concurrent
//! `modify` calls queue on the pool and each one sees the previous commit; no
//! update can be lost. Reads use a short read transaction (consistent snapshot)
//! on the read pool.
//!
//! Persisting never deletes rows the in-memory aggregate did not load: the
//! aggregate loaded at the start of the transaction is diffed against the
//! mutated one; changed/new child rows are upserted by `id` and only child ids
//! that the mutation removed are deleted.

use std::collections::HashMap;
use std::time::SystemTime;

use application::ports::{
    Modified, PortError, PortResult, RepoError, RepoResult, RosterMutation, RosterRepo,
    RosterSummary, Stored, VendorMutation, VendorRepo, VendorSummary,
};
use async_trait::async_trait;
use domain::directory::{
    MenuItem, MenuItemId, Roster, RosterId, RosterMember, RosterMemberId, UpsertReport, Vendor,
    VendorId,
};
use domain::identity::OrganizerId;
use domain::shared::{ItemName, Money, NameKey, PersonName, RosterName, VendorName};
use sqlx::SqliteConnection;

use crate::sqlite::Db;
use crate::timefmt::{from_text, to_text};

fn port(e: impl std::fmt::Display) -> PortError {
    PortError::new(e)
}

fn repo(e: sqlx::Error) -> RepoError {
    if e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
    {
        RepoError::UniqueViolation
    } else {
        RepoError::Port(port(e))
    }
}

fn ts(s: &str) -> PortResult<SystemTime> {
    from_text(s).map_err(port)
}

/// `now` as it will be read back from the database (second precision).
fn stored_time(now: SystemTime) -> SystemTime {
    from_text(&to_text(now)).unwrap_or(now)
}

// ===========================================================================
// Rosters
// ===========================================================================

#[derive(Clone, Debug)]
pub struct SqliteRosterRepo {
    db: Db,
}

impl SqliteRosterRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct RosterRow {
    id: String,
    name: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    id: String,
    name: String,
    note: Option<String>,
    sort_order: i32,
}

async fn load_roster(
    conn: &mut SqliteConnection,
    org: OrganizerId,
    id: RosterId,
) -> PortResult<Option<Stored<Roster>>> {
    let Some(row) = sqlx::query_as::<_, RosterRow>(
        "SELECT id, name, updated_at FROM rosters WHERE id = ? AND organizer_id = ?",
    )
    .bind(id.to_string())
    .bind(org.to_string())
    .fetch_optional(&mut *conn)
    .await
    .map_err(port)?
    else {
        return Ok(None);
    };
    let members = sqlx::query_as::<_, MemberRow>(
        "SELECT id, name, note, sort_order FROM roster_members \
         WHERE roster_id = ? ORDER BY sort_order, id",
    )
    .bind(&row.id)
    .fetch_all(&mut *conn)
    .await
    .map_err(port)?
    .into_iter()
    .map(|m| {
        Ok(RosterMember {
            id: m.id.parse().map_err(port)?,
            name: PersonName::new(&m.name).map_err(port)?,
            note: m.note,
            sort_order: m.sort_order,
        })
    })
    .collect::<PortResult<Vec<_>>>()?;
    let roster = Roster::from_parts(
        row.id.parse().map_err(port)?,
        org,
        RosterName::new(&row.name).map_err(port)?,
        members,
    )
    .map_err(port)?;
    Ok(Some(Stored {
        value: roster,
        updated_at: ts(&row.updated_at)?,
    }))
}

async fn upsert_member(
    conn: &mut SqliteConnection,
    roster_id: RosterId,
    m: &RosterMember,
    now: &str,
) -> RepoResult<()> {
    sqlx::query(
        "INSERT INTO roster_members (id, roster_id, name, name_key, note, sort_order, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, name_key = excluded.name_key, \
           note = excluded.note, sort_order = excluded.sort_order, updated_at = excluded.updated_at",
    )
    .bind(m.id.to_string())
    .bind(roster_id.to_string())
    .bind(m.name.as_str())
    .bind(m.name.key().as_str())
    .bind(&m.note)
    .bind(m.sort_order)
    .bind(now)
    .execute(conn)
    .await
    .map_err(repo)?;
    Ok(())
}

/// Insert a roster with its members on an open write transaction.
pub(crate) async fn create_roster_tx(
    conn: &mut SqliteConnection,
    roster: Roster,
    now: SystemTime,
) -> RepoResult<Stored<Roster>> {
    let t = to_text(now);
    sqlx::query(
        "INSERT INTO rosters (id, organizer_id, name, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(roster.id().to_string())
    .bind(roster.organizer_id().to_string())
    .bind(roster.name().as_str())
    .bind(&t)
    .bind(&t)
    .execute(&mut *conn)
    .await
    .map_err(repo)?;
    for m in roster.members() {
        upsert_member(&mut *conn, roster.id(), m, &t).await?;
    }
    Ok(Stored {
        value: roster,
        updated_at: stored_time(now),
    })
}

/// Load -> mutate -> persist on an open write transaction (shared by
/// [`RosterRepo::modify`] and the AI import unit of work).
pub(crate) async fn modify_roster_tx(
    conn: &mut SqliteConnection,
    org: OrganizerId,
    id: RosterId,
    now: SystemTime,
    mutation: RosterMutation,
) -> RepoResult<Modified<Roster, UpsertReport<RosterMemberId>>> {
    let t = to_text(now);
    let before = load_roster(&mut *conn, org, id)
        .await?
        .ok_or(RepoError::NotFound)?;
    let mut after = before.value.clone();
    // On error the transaction is dropped, i.e. rolled back.
    let report = mutation(&mut after)?;

    let old: HashMap<RosterMemberId, &RosterMember> =
        before.value.members().iter().map(|m| (m.id, m)).collect();
    let new_ids: std::collections::HashSet<RosterMemberId> =
        after.members().iter().map(|m| m.id).collect();
    let mut changed = after.name() != before.value.name();
    // Deletes first so a removed name can be reused by a new row.
    for m in before.value.members() {
        if !new_ids.contains(&m.id) {
            sqlx::query("DELETE FROM roster_members WHERE id = ? AND roster_id = ?")
                .bind(m.id.to_string())
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(repo)?;
            changed = true;
        }
    }
    for m in after.members() {
        if old.get(&m.id).is_none_or(|o| *o != m) {
            upsert_member(&mut *conn, id, m, &t).await?;
            changed = true;
        }
    }
    let updated_at = if changed {
        sqlx::query("UPDATE rosters SET name = ?, updated_at = ? WHERE id = ?")
            .bind(after.name().as_str())
            .bind(&t)
            .bind(id.to_string())
            .execute(&mut *conn)
            .await
            .map_err(repo)?;
        stored_time(now)
    } else {
        before.updated_at
    };
    Ok(Modified {
        stored: Stored {
            value: after,
            updated_at,
        },
        report,
    })
}

#[async_trait]
impl RosterRepo for SqliteRosterRepo {
    async fn list(&self, org: OrganizerId) -> PortResult<Vec<RosterSummary>> {
        let rows = sqlx::query_as::<_, (String, String, i64, String)>(
            "SELECT r.id, r.name, (SELECT COUNT(*) FROM roster_members m WHERE m.roster_id = r.id), \
                    r.updated_at \
             FROM rosters r WHERE r.organizer_id = ? ORDER BY r.updated_at DESC, r.id DESC",
        )
        .bind(org.to_string())
        .fetch_all(&self.db.read)
        .await
        .map_err(port)?;
        rows.into_iter()
            .map(|(id, name, n, updated)| {
                Ok(RosterSummary {
                    id: id.parse().map_err(port)?,
                    name,
                    member_count: n as u32,
                    updated_at: ts(&updated)?,
                })
            })
            .collect()
    }

    async fn find(&self, org: OrganizerId, id: RosterId) -> PortResult<Option<Stored<Roster>>> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        load_roster(&mut tx, org, id).await
    }

    async fn create(&self, roster: Roster, now: SystemTime) -> RepoResult<Stored<Roster>> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let stored = create_roster_tx(&mut tx, roster, now).await?;
        tx.commit().await.map_err(port)?;
        Ok(stored)
    }

    async fn modify(
        &self,
        org: OrganizerId,
        id: RosterId,
        now: SystemTime,
        mutation: RosterMutation,
    ) -> RepoResult<Modified<Roster, UpsertReport<RosterMemberId>>> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let m = modify_roster_tx(&mut tx, org, id, now, mutation).await?;
        tx.commit().await.map_err(port)?;
        Ok(m)
    }

    async fn delete(&self, org: OrganizerId, id: RosterId) -> PortResult<bool> {
        let r = sqlx::query("DELETE FROM rosters WHERE id = ? AND organizer_id = ?")
            .bind(id.to_string())
            .bind(org.to_string())
            .execute(&self.db.write)
            .await
            .map_err(port)?;
        Ok(r.rows_affected() > 0)
    }
}

// ===========================================================================
// Vendors
// ===========================================================================

#[derive(Clone, Debug)]
pub struct SqliteVendorRepo {
    db: Db,
}

impl SqliteVendorRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct VendorRow {
    id: String,
    name: String,
    phone: Option<String>,
    address: Option<String>,
    note: Option<String>,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct ItemRow {
    id: String,
    name: String,
    price: i64,
    description: Option<String>,
    category: Option<String>,
    sort_order: i32,
    archived: bool,
}

async fn load_vendor(
    conn: &mut SqliteConnection,
    org: OrganizerId,
    id: VendorId,
) -> PortResult<Option<Stored<Vendor>>> {
    let Some(row) = sqlx::query_as::<_, VendorRow>(
        "SELECT id, name, phone, address, note, updated_at FROM vendors \
         WHERE id = ? AND organizer_id = ?",
    )
    .bind(id.to_string())
    .bind(org.to_string())
    .fetch_optional(&mut *conn)
    .await
    .map_err(port)?
    else {
        return Ok(None);
    };
    let menu = sqlx::query_as::<_, ItemRow>(
        "SELECT id, name, price, description, category, sort_order, archived FROM menu_items \
         WHERE vendor_id = ? ORDER BY sort_order, id",
    )
    .bind(&row.id)
    .fetch_all(&mut *conn)
    .await
    .map_err(port)?
    .into_iter()
    .map(|m| {
        Ok(MenuItem {
            id: m.id.parse().map_err(port)?,
            name: ItemName::new(&m.name).map_err(port)?,
            price: Money::try_new(m.price).map_err(port)?,
            description: m.description,
            category: m.category,
            sort_order: m.sort_order,
            archived: m.archived,
        })
    })
    .collect::<PortResult<Vec<_>>>()?;
    let vendor = Vendor::from_parts(
        row.id.parse().map_err(port)?,
        org,
        VendorName::new(&row.name).map_err(port)?,
        row.phone,
        row.address,
        row.note,
        menu,
    )
    .map_err(port)?;
    Ok(Some(Stored {
        value: vendor,
        updated_at: ts(&row.updated_at)?,
    }))
}

async fn upsert_item(
    conn: &mut SqliteConnection,
    vendor_id: VendorId,
    m: &MenuItem,
    now: &str,
) -> RepoResult<()> {
    sqlx::query(
        "INSERT INTO menu_items \
           (id, vendor_id, name, name_key, price, description, category, sort_order, archived, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, name_key = excluded.name_key, \
           price = excluded.price, description = excluded.description, category = excluded.category, \
           sort_order = excluded.sort_order, archived = excluded.archived, \
           updated_at = excluded.updated_at",
    )
    .bind(m.id.to_string())
    .bind(vendor_id.to_string())
    .bind(m.name.as_str())
    .bind(m.name.key().as_str())
    .bind(m.price.get())
    .bind(&m.description)
    .bind(&m.category)
    .bind(m.sort_order)
    .bind(m.archived)
    .bind(now)
    .execute(conn)
    .await
    .map_err(repo)?;
    Ok(())
}

/// Insert a vendor with its menu on an open write transaction.
pub(crate) async fn create_vendor_tx(
    conn: &mut SqliteConnection,
    vendor: Vendor,
    now: SystemTime,
) -> RepoResult<Stored<Vendor>> {
    let t = to_text(now);
    sqlx::query(
        "INSERT INTO vendors (id, organizer_id, name, name_key, phone, address, note, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(vendor.id().to_string())
    .bind(vendor.organizer_id().to_string())
    .bind(vendor.name().as_str())
    .bind(vendor.name().key().as_str())
    .bind(vendor.phone())
    .bind(vendor.address())
    .bind(vendor.note())
    .bind(&t)
    .bind(&t)
    .execute(&mut *conn)
    .await
    .map_err(repo)?;
    for m in vendor.menu() {
        upsert_item(&mut *conn, vendor.id(), m, &t).await?;
    }
    Ok(Stored {
        value: vendor,
        updated_at: stored_time(now),
    })
}

/// Load -> mutate -> persist on an open write transaction (shared by
/// [`VendorRepo::modify`] and the AI import unit of work).
pub(crate) async fn modify_vendor_tx(
    conn: &mut SqliteConnection,
    org: OrganizerId,
    id: VendorId,
    now: SystemTime,
    mutation: VendorMutation,
) -> RepoResult<Modified<Vendor, UpsertReport<MenuItemId>>> {
    let t = to_text(now);
    let before = load_vendor(&mut *conn, org, id)
        .await?
        .ok_or(RepoError::NotFound)?;
    let mut after = before.value.clone();
    let report = mutation(&mut after)?;

    let old: HashMap<MenuItemId, &MenuItem> =
        before.value.menu().iter().map(|m| (m.id, m)).collect();
    let new_ids: std::collections::HashSet<MenuItemId> =
        after.menu().iter().map(|m| m.id).collect();
    let b = &before.value;
    let mut changed = after.name() != b.name()
        || after.phone() != b.phone()
        || after.address() != b.address()
        || after.note() != b.note();
    for m in b.menu() {
        if !new_ids.contains(&m.id) {
            sqlx::query("DELETE FROM menu_items WHERE id = ? AND vendor_id = ?")
                .bind(m.id.to_string())
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(repo)?;
            changed = true;
        }
    }
    for m in after.menu() {
        if old.get(&m.id).is_none_or(|o| *o != m) {
            upsert_item(&mut *conn, id, m, &t).await?;
            changed = true;
        }
    }
    let updated_at = if changed {
        sqlx::query(
            "UPDATE vendors SET name = ?, name_key = ?, phone = ?, address = ?, note = ?, \
             updated_at = ? WHERE id = ?",
        )
        .bind(after.name().as_str())
        .bind(after.name().key().as_str())
        .bind(after.phone())
        .bind(after.address())
        .bind(after.note())
        .bind(&t)
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(repo)?;
        stored_time(now)
    } else {
        before.updated_at
    };
    Ok(Modified {
        stored: Stored {
            value: after,
            updated_at,
        },
        report,
    })
}

#[async_trait]
impl VendorRepo for SqliteVendorRepo {
    async fn list(&self, org: OrganizerId) -> PortResult<Vec<VendorSummary>> {
        let rows = sqlx::query_as::<_, (String, String, Option<String>, i64, String)>(
            "SELECT v.id, v.name, v.phone, \
                    (SELECT COUNT(*) FROM menu_items i WHERE i.vendor_id = v.id), v.updated_at \
             FROM vendors v WHERE v.organizer_id = ? ORDER BY v.updated_at DESC, v.id DESC",
        )
        .bind(org.to_string())
        .fetch_all(&self.db.read)
        .await
        .map_err(port)?;
        rows.into_iter()
            .map(|(id, name, phone, n, updated)| {
                Ok(VendorSummary {
                    id: id.parse().map_err(port)?,
                    name,
                    phone,
                    item_count: n as u32,
                    updated_at: ts(&updated)?,
                })
            })
            .collect()
    }

    async fn find(&self, org: OrganizerId, id: VendorId) -> PortResult<Option<Stored<Vendor>>> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        load_vendor(&mut tx, org, id).await
    }

    async fn find_id_by_name_key(
        &self,
        org: OrganizerId,
        key: &NameKey,
    ) -> PortResult<Option<VendorId>> {
        sqlx::query_as::<_, (String,)>(
            "SELECT id FROM vendors WHERE organizer_id = ? AND name_key = ?",
        )
        .bind(org.to_string())
        .bind(key.as_str())
        .fetch_optional(&self.db.read)
        .await
        .map_err(port)?
        .map(|(id,)| id.parse().map_err(port))
        .transpose()
    }

    async fn create(&self, vendor: Vendor, now: SystemTime) -> RepoResult<Stored<Vendor>> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let stored = create_vendor_tx(&mut tx, vendor, now).await?;
        tx.commit().await.map_err(port)?;
        Ok(stored)
    }

    async fn modify(
        &self,
        org: OrganizerId,
        id: VendorId,
        now: SystemTime,
        mutation: VendorMutation,
    ) -> RepoResult<Modified<Vendor, UpsertReport<MenuItemId>>> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let m = modify_vendor_tx(&mut tx, org, id, now, mutation).await?;
        tx.commit().await.map_err(port)?;
        Ok(m)
    }

    async fn delete(&self, org: OrganizerId, id: VendorId) -> PortResult<bool> {
        let r = sqlx::query("DELETE FROM vendors WHERE id = ? AND organizer_id = ?")
            .bind(id.to_string())
            .bind(org.to_string())
            .execute(&self.db.write)
            .await
            .map_err(port)?;
        Ok(r.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use application::ports::OrganizerRepo;
    use domain::directory::{MemberEdit, MemberInput, MenuItemEdit, MenuItemInput, UpsertOutcome};
    use domain::identity::Organizer;
    use domain::shared::{DomainError, Patch};

    use super::*;
    use crate::identity_repo::SqliteOrganizerRepo;
    use crate::sqlite::TempDir;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    async fn organizer(db: &Db, n: &str) -> OrganizerId {
        let o = Organizer {
            id: OrganizerId::new(),
            oidc_issuer: Some("https://id.example".into()),
            oidc_sub: Some(format!("sub-{n}")),
            email: format!("{n}@x.test"),
            display_name: n.into(),
            avatar_url: None,
        };
        SqliteOrganizerRepo::new(db.clone())
            .upsert_from_identity(o, at(0))
            .await
            .unwrap()
            .id
    }

    async fn setup() -> (Db, TempDir, OrganizerId) {
        let (db, dir) = Db::open_temp().await.unwrap();
        let org = organizer(&db, "a").await;
        (db, dir, org)
    }

    fn mi(n: &str, note: Option<&str>) -> MemberInput {
        MemberInput {
            name: n.into(),
            note: note.map(Into::into),
        }
    }

    fn item(n: &str, price: i64) -> MenuItemInput {
        MenuItemInput {
            name: n.into(),
            price,
            description: Some("d".into()),
            category: None,
        }
    }

    async fn member_updated_at(db: &Db, name: &str) -> String {
        sqlx::query_as::<_, (String,)>("SELECT updated_at FROM roster_members WHERE name = ?")
            .bind(name)
            .fetch_one(&db.read)
            .await
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn roster_roundtrip_list_and_scoping() {
        let (db, _d, org) = setup().await;
        let other = organizer(&db, "b").await;
        let repo = SqliteRosterRepo::new(db.clone());
        let mut r = Roster::new(org, "午餐組").unwrap();
        r.upsert_members(vec![mi("王小明", Some("素")), mi("Amy", None)])
            .unwrap();
        let created = repo.create(r.clone(), at(10)).await.unwrap();
        let found = repo.find(org, r.id()).await.unwrap().unwrap();
        assert_eq!(found.value, r);
        assert_eq!(found.updated_at, created.updated_at);
        assert_eq!(found.updated_at, at(10));

        let list = repo.list(org).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), list[0].member_count), ("午餐組", 2));

        // Another organizer sees nothing and cannot touch it.
        assert!(repo.find(other, r.id()).await.unwrap().is_none());
        assert!(repo.list(other).await.unwrap().is_empty());
        let err = repo
            .modify(other, r.id(), at(11), Box::new(|_| Ok(None)))
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
        assert!(!repo.delete(other, r.id()).await.unwrap());
        assert!(repo.delete(org, r.id()).await.unwrap());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM roster_members")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(n, 0, "members cascade");
    }

    #[tokio::test]
    async fn roster_list_newest_updated_first() {
        let (db, _d, org) = setup().await;
        let repo = SqliteRosterRepo::new(db);
        let a = repo
            .create(Roster::new(org, "A").unwrap(), at(1))
            .await
            .unwrap();
        let b = repo
            .create(Roster::new(org, "B").unwrap(), at(2))
            .await
            .unwrap();
        let names = |l: Vec<RosterSummary>| l.into_iter().map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(names(repo.list(org).await.unwrap()), ["B", "A"]);
        repo.modify(
            org,
            a.value.id(),
            at(3),
            Box::new(|r| r.rename("A2").map(|_| None)),
        )
        .await
        .unwrap();
        assert_eq!(names(repo.list(org).await.unwrap()), ["A2", "B"]);
        let _ = b;
    }

    #[tokio::test]
    async fn modify_persists_only_changed_and_removed_rows() {
        let (db, _d, org) = setup().await;
        let repo = SqliteRosterRepo::new(db.clone());
        let r = repo
            .create(Roster::new(org, "R").unwrap(), at(1))
            .await
            .unwrap();
        let id = r.value.id();
        let m1 = repo
            .modify(
                org,
                id,
                at(10),
                Box::new(|r| {
                    r.upsert_members(vec![mi("A", None), mi("B", None)])
                        .map(Some)
                }),
            )
            .await
            .unwrap();
        assert_eq!(m1.report.unwrap().created(), 2);
        assert_eq!(m1.stored.updated_at, at(10));
        let a_t = member_updated_at(&db, "A").await;

        // Touch only B: A's row keeps its timestamp.
        repo.modify(
            org,
            id,
            at(20),
            Box::new(|r| r.upsert_members(vec![mi("B", Some("n"))]).map(Some)),
        )
        .await
        .unwrap();
        assert_eq!(member_updated_at(&db, "A").await, a_t);
        assert_eq!(member_updated_at(&db, "B").await, to_text(at(20)));

        // A no-op upsert does not bump the roster.
        let noop = repo
            .modify(
                org,
                id,
                at(30),
                Box::new(|r| r.upsert_members(vec![mi("B", None)]).map(Some)),
            )
            .await
            .unwrap();
        assert_eq!(noop.report.unwrap().unchanged(), 1);
        assert_eq!(noop.stored.updated_at, at(20));

        // Removing deletes exactly that row.
        let b_id = noop
            .stored
            .value
            .members()
            .iter()
            .find(|m| m.name.as_str() == "B")
            .unwrap()
            .id;
        repo.modify(
            org,
            id,
            at(40),
            Box::new(move |r| r.remove_member(b_id).map(|_| None)),
        )
        .await
        .unwrap();
        let left = repo.find(org, id).await.unwrap().unwrap();
        assert_eq!(left.updated_at, at(40));
        assert_eq!(left.value.members().len(), 1);
        assert_eq!(left.value.members()[0].name.as_str(), "A");
        assert_eq!(member_updated_at(&db, "A").await, a_t);
    }

    #[tokio::test]
    async fn failed_mutation_rolls_back() {
        let (db, _d, org) = setup().await;
        let repo = SqliteRosterRepo::new(db);
        let r = repo
            .create(Roster::new(org, "R").unwrap(), at(1))
            .await
            .unwrap();
        let id = r.value.id();
        let err = repo
            .modify(
                org,
                id,
                at(2),
                Box::new(|r| {
                    r.rename("改名")?;
                    Err(DomainError::invalid("name", "boom"))
                }),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Domain(_)));
        assert_eq!(
            repo.find(org, id)
                .await
                .unwrap()
                .unwrap()
                .value
                .name()
                .as_str(),
            "R"
        );
    }

    #[tokio::test]
    async fn roster_unique_name_key_is_enforced_by_db() {
        let (db, _d, org) = setup().await;
        let repo = SqliteRosterRepo::new(db.clone());
        let mut r = Roster::new(org, "R").unwrap();
        r.upsert_members(vec![mi("Amy", None)]).unwrap();
        let r = repo.create(r, at(1)).await.unwrap().value;
        let dup = sqlx::query(
            "INSERT INTO roster_members (id, roster_id, name, name_key, sort_order, updated_at) \
             VALUES ('x', ?, 'ＡＭＹ', 'amy', 5, 't')",
        )
        .bind(r.id().to_string())
        .execute(&db.write)
        .await;
        assert!(dup.is_err());
        // In-memory manual edit collision surfaces as a domain conflict.
        let amy = r.members()[0].id;
        let r = repo
            .modify(
                org,
                r.id(),
                at(2),
                Box::new(|r| r.upsert_members(vec![mi("Bob", None)]).map(Some)),
            )
            .await
            .unwrap()
            .stored
            .value;
        let bob = r.members()[1].id;
        let err = repo
            .modify(
                org,
                r.id(),
                at(3),
                Box::new(move |r| {
                    r.edit_member(
                        bob,
                        MemberEdit {
                            name: Some("amy".into()),
                            note: Patch::Keep,
                        },
                    )
                    .map(|_| None)
                }),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RepoError::Domain(DomainError::Conflict { .. })
        ));
        let _ = amy;
    }

    #[tokio::test]
    async fn concurrent_upserts_do_not_lose_updates() {
        let (db, _d, org) = setup().await;
        let repo = Arc::new(SqliteRosterRepo::new(db));
        let r = repo
            .create(Roster::new(org, "R").unwrap(), at(1))
            .await
            .unwrap();
        let id = r.value.id();
        let mut tasks = Vec::new();
        for i in 0..8 {
            let repo = repo.clone();
            tasks.push(tokio::spawn(async move {
                repo.modify(
                    org,
                    id,
                    at(10 + i),
                    Box::new(move |r| {
                        r.upsert_members(vec![mi(&format!("人{i}"), None)])
                            .map(Some)
                    }),
                )
                .await
                .unwrap()
            }));
        }
        for t in tasks {
            let m = t.await.unwrap();
            assert!(matches!(
                m.report.unwrap().outcomes[0],
                UpsertOutcome::Created(_)
            ));
        }
        let r = repo.find(org, id).await.unwrap().unwrap().value;
        assert_eq!(r.members().len(), 8);
    }

    #[tokio::test]
    async fn vendor_roundtrip_and_name_uniqueness() {
        let (db, _d, org) = setup().await;
        let other = organizer(&db, "b").await;
        let repo = SqliteVendorRepo::new(db.clone());
        let mut v = Vendor::new(org, "池上便當").unwrap();
        v.edit_info(domain::directory::VendorInfoEdit {
            phone: Patch::Set("02-1".into()),
            ..Default::default()
        })
        .unwrap();
        v.upsert_menu_items(vec![item("排骨飯", 100), item("雞腿飯", 120)])
            .unwrap();
        repo.create(v.clone(), at(1)).await.unwrap();
        let found = repo.find(org, v.id()).await.unwrap().unwrap();
        assert_eq!(found.value, v);
        let list = repo.list(org).await.unwrap();
        assert_eq!(list[0].item_count, 2);
        assert_eq!(list[0].phone.as_deref(), Some("02-1"));

        // Same NameKey for the same organizer is rejected by the DB.
        let dup = Vendor::new(org, "ＰＡＮ").unwrap();
        repo.create(dup, at(2)).await.unwrap();
        let err = repo
            .create(Vendor::new(org, " pan ").unwrap(), at(3))
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::UniqueViolation));
        assert!(
            repo.find_id_by_name_key(org, &NameKey::from("pan"))
                .await
                .unwrap()
                .is_some()
        );
        // ... but another organizer may use it.
        repo.create(Vendor::new(other, "pan").unwrap(), at(4))
            .await
            .unwrap();
        assert!(repo.find(other, v.id()).await.unwrap().is_none());
        assert!(!repo.delete(other, v.id()).await.unwrap());
        assert!(repo.delete(org, v.id()).await.unwrap());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM menu_items")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn vendor_modify_items_only_touches_changes() {
        let (db, _d, org) = setup().await;
        let repo = SqliteVendorRepo::new(db.clone());
        let v = repo
            .create(Vendor::new(org, "V").unwrap(), at(1))
            .await
            .unwrap();
        let id = v.value.id();
        let m = repo
            .modify(
                org,
                id,
                at(10),
                Box::new(|v| {
                    v.upsert_menu_items(vec![item("A", 10), item("B", 20)])
                        .map(Some)
                }),
            )
            .await
            .unwrap();
        let (i0, i1) = (m.stored.value.menu()[0].id, m.stored.value.menu()[1].id);
        repo.modify(
            org,
            id,
            at(20),
            Box::new(move |v| {
                v.edit_item(
                    i1,
                    MenuItemEdit {
                        price: Some(25),
                        archived: Some(true),
                        ..Default::default()
                    },
                )
                .map(|_| None)
            }),
        )
        .await
        .unwrap();
        let rows = sqlx::query_as::<_, (String, i64, bool, String)>(
            "SELECT name, price, archived, updated_at FROM menu_items ORDER BY name",
        )
        .fetch_all(&db.read)
        .await
        .unwrap();
        assert_eq!(rows[0], ("A".into(), 10, false, to_text(at(10))));
        assert_eq!(rows[1], ("B".into(), 25, true, to_text(at(20))));
        // Item rename collision is a domain conflict; removal deletes one row.
        let err = repo
            .modify(
                org,
                id,
                at(30),
                Box::new(move |v| {
                    v.edit_item(
                        i1,
                        MenuItemEdit {
                            name: Some("a".into()),
                            ..Default::default()
                        },
                    )
                    .map(|_| None)
                }),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RepoError::Domain(DomainError::Conflict { .. })
        ));
        repo.modify(
            org,
            id,
            at(40),
            Box::new(move |v| v.remove_item(i0).map(|_| None)),
        )
        .await
        .unwrap();
        let left = repo.find(org, id).await.unwrap().unwrap();
        assert_eq!(left.value.menu().len(), 1);
        assert_eq!(left.value.menu()[0].price.get(), 25);
    }

    #[tokio::test]
    async fn vendor_rename_onto_other_vendor_is_unique_violation() {
        let (db, _d, org) = setup().await;
        let repo = SqliteVendorRepo::new(db);
        repo.create(Vendor::new(org, "A").unwrap(), at(1))
            .await
            .unwrap();
        let b = repo
            .create(Vendor::new(org, "B").unwrap(), at(2))
            .await
            .unwrap();
        let err = repo
            .modify(
                org,
                b.value.id(),
                at(3),
                Box::new(|v| {
                    v.edit_info(domain::directory::VendorInfoEdit {
                        name: Some("ａ".into()),
                        ..Default::default()
                    })
                    .map(|_| None)
                }),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::UniqueViolation));
        // The failed transaction left the vendor untouched.
        let b = repo.find(org, b.value.id()).await.unwrap().unwrap();
        assert_eq!(b.value.name().as_str(), "B");
    }
}
