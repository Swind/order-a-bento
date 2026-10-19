use crate::sqlite::Db;
use application::ports::PortError;
use application::sharing::{
    SharingError, SharingGroup, SharingGroupId, SharingRepo, SharingUser, role,
};
use async_trait::async_trait;
use domain::identity::OrganizerId;
use domain::shared::RosterName;
use sqlx::SqliteConnection;

#[derive(Clone)]
pub struct SqliteSharingRepo {
    db: Db,
}
// Membership mutations use BEGIN IMMEDIATE on the existing single-connection
// write pool, so the admin count, actor authorization and change are atomic.
impl SqliteSharingRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}
fn port(e: impl std::fmt::Display) -> SharingError {
    PortError::new(e).into()
}

pub(crate) async fn authorize(
    conn: &mut SqliteConnection,
    actor: OrganizerId,
    id: &str,
    admin: bool,
) -> Result<String, SharingError> {
    let r: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM sharing_group_members WHERE sharing_group_id = ? AND organizer_id = ?",
    )
    .bind(id)
    .bind(actor.to_string())
    .fetch_optional(conn)
    .await
    .map_err(port)?;
    let role = r.ok_or(SharingError::NotFound)?.0;
    if admin && role != "admin" {
        return Err(SharingError::Forbidden);
    }
    Ok(role)
}

type UserRow = (String, String, String, Option<String>, Option<String>);
fn user((id, display_name, email, issuer, role): UserRow) -> Result<SharingUser, SharingError> {
    Ok(SharingUser {
        id: id.parse().map_err(port)?,
        display_name,
        email,
        identity_kind: if issuer.is_some() { "oidc" } else { "local" }.into(),
        role,
    })
}

#[async_trait]
impl SharingRepo for SqliteSharingRepo {
    async fn list(&self, actor: OrganizerId) -> Result<Vec<SharingGroup>, SharingError> {
        let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT g.id,g.name,m.role FROM sharing_groups g JOIN sharing_group_members m ON m.sharing_group_id=g.id WHERE m.organizer_id=? ORDER BY g.name,g.id")
            .bind(actor.to_string()).fetch_all(&self.db.read).await.map_err(port)?;
        rows.into_iter()
            .map(|(id, name, role)| {
                Ok(SharingGroup {
                    id: id.parse().map_err(port)?,
                    name,
                    role,
                })
            })
            .collect()
    }
    async fn create(&self, actor: OrganizerId, name: &str) -> Result<SharingGroup, SharingError> {
        let name = RosterName::new(name)?.to_string();
        let id = SharingGroupId::new();
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        sqlx::query("INSERT INTO sharing_groups(id,name,created_by,created_at) VALUES(?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now'))")
            .bind(id.to_string()).bind(&name).bind(actor.to_string()).execute(&mut *tx).await.map_err(port)?;
        sqlx::query("INSERT INTO sharing_group_members VALUES(?,?,'admin')")
            .bind(id.to_string())
            .bind(actor.to_string())
            .execute(&mut *tx)
            .await
            .map_err(port)?;
        tx.commit().await.map_err(port)?;
        Ok(SharingGroup {
            id,
            name,
            role: "admin".into(),
        })
    }
    async fn get(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
    ) -> Result<SharingGroup, SharingError> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        let role = authorize(&mut tx, actor, &id.to_string(), false).await?;
        let (name,): (String,) = sqlx::query_as("SELECT name FROM sharing_groups WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(port)?;
        Ok(SharingGroup { id, name, role })
    }
    async fn rename(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        name: &str,
    ) -> Result<SharingGroup, SharingError> {
        let name = RosterName::new(name)?.to_string();
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let role = authorize(&mut tx, actor, &id.to_string(), true).await?;
        sqlx::query("UPDATE sharing_groups SET name=? WHERE id=?")
            .bind(&name)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(port)?;
        tx.commit().await.map_err(port)?;
        Ok(SharingGroup { id, name, role })
    }
    async fn members(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
    ) -> Result<Vec<SharingUser>, SharingError> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        authorize(&mut tx, actor, &id.to_string(), false).await?;
        let rows: Vec<UserRow> = sqlx::query_as("SELECT o.id,o.display_name,o.email,o.oidc_issuer,m.role FROM organizers o JOIN sharing_group_members m ON m.organizer_id=o.id WHERE m.sharing_group_id=? ORDER BY o.display_name,o.id")
            .bind(id.to_string()).fetch_all(&mut *tx).await.map_err(port)?;
        rows.into_iter().map(user).collect()
    }
    async fn lookup(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        email: &str,
    ) -> Result<Vec<SharingUser>, SharingError> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        authorize(&mut tx, actor, &id.to_string(), true).await?;
        let email = domain::identity::parse_local_email(email)?;
        if email.len() > 320 {
            return Err(
                domain::shared::DomainError::invalid("email", "full email required").into(),
            );
        }
        let rows: Vec<UserRow> = sqlx::query_as("SELECT o.id,o.display_name,o.email,o.oidc_issuer,m.role FROM organizers o LEFT JOIN sharing_group_members m ON m.organizer_id=o.id AND m.sharing_group_id=? WHERE lower(o.email)=lower(?) ORDER BY o.id")
            .bind(id.to_string()).bind(email).fetch_all(&mut *tx).await.map_err(port)?;
        rows.into_iter().map(user).collect()
    }
    async fn add(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        target: OrganizerId,
        raw: &str,
    ) -> Result<(), SharingError> {
        let role = role(raw)?;
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        authorize(&mut tx, actor, &id.to_string(), true).await?;
        let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM organizers WHERE id=?")
            .bind(target.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(port)?;
        if exists.is_none() {
            return Err(SharingError::NotFound);
        }
        let result = sqlx::query("INSERT INTO sharing_group_members VALUES(?,?,?)")
            .bind(id.to_string())
            .bind(target.to_string())
            .bind(role)
            .execute(&mut *tx)
            .await;
        match result {
            Err(e)
                if e.as_database_error()
                    .is_some_and(|e| e.is_unique_violation()) =>
            {
                return Err(SharingError::Conflict);
            }
            Err(e) => return Err(port(e)),
            Ok(_) => {}
        }
        tx.commit().await.map_err(port)?;
        Ok(())
    }
    async fn change(
        &self,
        actor: OrganizerId,
        id: SharingGroupId,
        target: OrganizerId,
        new_role: Option<&str>,
    ) -> Result<(), SharingError> {
        if let Some(r) = new_role {
            role(r)?;
        }
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        authorize(&mut tx, actor, &id.to_string(), true).await?;
        let old = authorize(&mut tx, target, &id.to_string(), false).await?;
        if old == "admin" && new_role != Some("admin") {
            let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM sharing_group_members WHERE sharing_group_id=? AND role='admin'").bind(id.to_string()).fetch_one(&mut *tx).await.map_err(port)?;
            if n <= 1 {
                return Err(SharingError::LastAdmin);
            }
        }
        if let Some(r) = new_role {
            sqlx::query("UPDATE sharing_group_members SET role=? WHERE sharing_group_id=? AND organizer_id=?").bind(r).bind(id.to_string()).bind(target.to_string()).execute(&mut *tx).await.map_err(port)?;
        } else {
            sqlx::query(
                "DELETE FROM sharing_group_members WHERE sharing_group_id=? AND organizer_id=?",
            )
            .bind(id.to_string())
            .bind(target.to_string())
            .execute(&mut *tx)
            .await
            .map_err(port)?;
        }
        tx.commit().await.map_err(port)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::ports::{OrganizerRepo, RosterRepo, VendorRepo};
    use domain::identity::Organizer;
    use std::time::SystemTime;

    async fn account(db: &Db, sub: &str) -> OrganizerId {
        crate::identity_repo::SqliteOrganizerRepo::new(db.clone())
            .upsert_from_identity(
                Organizer::new("https://id.example", sub, "same@example.com", sub, None).unwrap(),
                SystemTime::now(),
            )
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn concurrent_admin_demotions_and_removals_preserve_one_admin() {
        let (db, _dir) = Db::open_temp().await.unwrap();
        let a = account(&db, "a").await;
        let b = account(&db, "b").await;
        let repo = SqliteSharingRepo::new(db.clone());
        for remove in [false, true] {
            let g = repo.create(a, "Group").await.unwrap();
            repo.add(a, g.id, b, "admin").await.unwrap();
            let (x, y) = tokio::join!(
                repo.change(a, g.id, a, if remove { None } else { Some("member") }),
                repo.change(b, g.id, b, if remove { None } else { Some("member") })
            );
            assert_eq!(
                [x.is_ok(), y.is_ok()].into_iter().filter(|ok| *ok).count(),
                1
            );
            let loser = if x.is_err() { x } else { y };
            assert!(matches!(loser, Err(SharingError::LastAdmin)));
            let (n,):(i64,)=sqlx::query_as("SELECT count(*) FROM sharing_group_members WHERE sharing_group_id=? AND role='admin'").bind(g.id.to_string()).fetch_one(&db.read).await.unwrap();
            assert_eq!(n, 1);
        }
        // Two administrators each remove the other: only one can succeed.
        let g = repo.create(a, "Cross removals").await.unwrap();
        repo.add(a, g.id, b, "admin").await.unwrap();
        let (x, y) = tokio::join!(repo.change(a, g.id, b, None), repo.change(b, g.id, a, None));
        assert_ne!(x.is_ok(), y.is_ok());
    }

    #[tokio::test]
    async fn exact_email_keeps_local_and_oidc_accounts_distinct_without_secrets() {
        let (db, _dir) = Db::open_temp().await.unwrap();
        let a = account(&db, "oidc").await;
        let local = Organizer::new_local("same@example.com", "Local").unwrap();
        let local_id = local.id;
        crate::identity_repo::SqliteOrganizerRepo::new(db.clone())
            .create_local(local, "secret-hash".into(), SystemTime::now())
            .await
            .unwrap();
        let repo = SqliteSharingRepo::new(db);
        let g = repo.create(a, "G").await.unwrap();
        let users = repo.lookup(a, g.id, " SAME@example.com ").await.unwrap();
        assert_eq!(users.len(), 2);
        assert!(users.iter().any(|u| u.id == a && u.identity_kind == "oidc"));
        assert!(
            users
                .iter()
                .any(|u| u.id == local_id && u.identity_kind == "local")
        );
        assert!(
            repo.lookup(a, g.id, "%same@example.com")
                .await
                .unwrap()
                .is_empty()
        );
        repo.add(a, g.id, local_id, "member").await.unwrap();
        assert_eq!(repo.members(a, g.id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn order_insert_rechecks_membership_after_snapshot_reads() {
        use application::ports::GroupOrderRepo;
        let (db, _dir) = Db::open_temp().await.unwrap();
        let a = account(&db, "a").await;
        let b = account(&db, "b").await;
        let groups = SqliteSharingRepo::new(db.clone());
        let g = groups.create(a, "Team").await.unwrap();
        groups.add(a, g.id, b, "member").await.unwrap();
        let rosters = crate::directory_repo::SqliteRosterRepo::new(db.clone());
        let vendors = crate::directory_repo::SqliteVendorRepo::new(db.clone());
        let mut r = domain::directory::Roster::new(a, "R").unwrap();
        r.sharing_group_id = Some(g.id);
        r.upsert_members(vec![domain::directory::MemberInput {
            name: "Alice".into(),
            note: None,
        }])
        .unwrap();
        let r = rosters.create(r, SystemTime::now()).await.unwrap().value;
        let mut v = domain::directory::Vendor::new(a, "V").unwrap();
        v.sharing_group_id = Some(g.id);
        v.upsert_menu_items(vec![domain::directory::MenuItemInput {
            name: "Rice".into(),
            price: 80,
            description: None,
            category: None,
        }])
        .unwrap();
        let v = vendors.create(v, SystemTime::now()).await.unwrap().value;
        let r = rosters.find(b, r.id()).await.unwrap().unwrap();
        let v = vendors.find(b, v.id()).await.unwrap().unwrap();
        let order = domain::ordering::GroupOrder::open(
            b,
            "Lunch",
            &r.value,
            &v.value,
            None,
            None,
            domain::ordering::ShareToken::from_random_bytes([1; 32]),
            SystemTime::now(),
        )
        .unwrap();
        groups.change(a, g.id, b, None).await.unwrap();
        let orders = crate::ordering_repo::SqliteGroupOrderRepo::new(db);
        assert!(matches!(
            orders.create(&order).await,
            Err(application::ports::RepoError::NotFound)
        ));
        assert!(orders.find_by_id(b, order.id()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn migration_preserves_private_vendor_children_and_scope_cannot_change() {
        let dir = crate::sqlite::TempDir::new().unwrap();
        let db = Db::connect_path(&dir.path().join("migration.db"))
            .await
            .unwrap();
        crate::sqlite::MIGRATOR.run_to(4, &db.write).await.unwrap();
        let a = account(&db, "a").await;
        let mut v = domain::directory::Vendor::new(a, "Old").unwrap();
        v.upsert_menu_items(vec![domain::directory::MenuItemInput {
            name: "Rice".into(),
            price: 80,
            description: None,
            category: None,
        }])
        .unwrap();
        // Use the pre-migration schema directly to exercise the table rebuild.
        sqlx::query("INSERT INTO vendors(id,organizer_id,name,name_key,created_at,updated_at) VALUES(?,?,'Old','old','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").bind(v.id().to_string()).bind(a.to_string()).execute(&db.write).await.unwrap();
        let item = v.menu()[0].id;
        sqlx::query("INSERT INTO menu_items(id,vendor_id,name,name_key,price,sort_order,updated_at) VALUES(?,?,'Rice','rice',80,0,'2026-01-01T00:00:00Z')").bind(item.to_string()).bind(v.id().to_string()).execute(&db.write).await.unwrap();
        db.migrate().await.unwrap();
        let vendors = crate::directory_repo::SqliteVendorRepo::new(db.clone());
        let loaded = vendors.find(a, v.id()).await.unwrap().unwrap();
        assert_eq!(loaded.value, v);
        assert!(loaded.value.sharing_group_id.is_none());
        let g = SqliteSharingRepo::new(db.clone())
            .create(a, "G")
            .await
            .unwrap();
        assert!(
            sqlx::query("UPDATE vendors SET sharing_group_id=? WHERE id=?")
                .bind(g.id.to_string())
                .bind(v.id().to_string())
                .execute(&db.write)
                .await
                .is_err()
        );
        let rosters = crate::directory_repo::SqliteRosterRepo::new(db.clone());
        let r = rosters
            .create(
                domain::directory::Roster::new(a, "R").unwrap(),
                SystemTime::now(),
            )
            .await
            .unwrap();
        assert!(
            sqlx::query("UPDATE rosters SET sharing_group_id=? WHERE id=?")
                .bind(g.id.to_string())
                .bind(r.value.id().to_string())
                .execute(&db.write)
                .await
                .is_err()
        );
        assert!(vendors.delete(a, v.id()).await.unwrap());
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM menu_items")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(n, 0);
        let errors: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&db.read)
            .await
            .unwrap();
        assert!(errors.is_empty());
    }
}
