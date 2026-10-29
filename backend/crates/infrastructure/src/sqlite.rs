//! SQLite connection pools and migrations.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

/// Embedded migrations from `backend/migrations/`.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

const READ_POOL_MAX: u32 = 8;

/// A read pool (many connections) and a write pool (exactly one connection).
#[derive(Clone, Debug)]
pub struct Db {
    pub read: SqlitePool,
    pub write: SqlitePool,
}

/// Create the parent directory of a file-backed database if it is missing.
fn ensure_parent_dir(path: &Path) -> Result<(), sqlx::Error> {
    if path.as_os_str().is_empty() || path.to_str().is_some_and(|p| p.starts_with(':')) {
        return Ok(()); // in-memory
    }
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => std::fs::create_dir_all(dir)?,
        _ => {}
    }
    Ok(())
}

fn options(opts: SqliteConnectOptions) -> SqliteConnectOptions {
    opts.journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(5000))
        .foreign_keys(true)
        .create_if_missing(true)
}

impl Db {
    /// Connect using a `sqlite:` URL (e.g. `sqlite:///data/bento.db`).
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let opts = SqliteConnectOptions::from_str(database_url)?;
        ensure_parent_dir(opts.get_filename())?;
        Self::from_options(options(opts)).await
    }

    /// Connect to a database file path.
    pub async fn connect_path(path: &Path) -> Result<Self, sqlx::Error> {
        ensure_parent_dir(path)?;
        Self::from_options(options(SqliteConnectOptions::new().filename(path))).await
    }

    async fn from_options(opts: SqliteConnectOptions) -> Result<Self, sqlx::Error> {
        // The write pool goes first so that it creates the file (and enables WAL).
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts.clone())
            .await?;
        let read = SqlitePoolOptions::new()
            .max_connections(READ_POOL_MAX)
            .connect_with(opts)
            .await?;
        Ok(Self { read, write })
    }

    /// Apply all pending migrations.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(&self.write).await
    }

    /// Open a fresh, migrated database in a temporary directory (for tests).
    ///
    /// The returned `TempDir` guard must be kept alive while the DB is used.
    pub async fn open_temp() -> Result<(Self, TempDir), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let db = Self::connect_path(&dir.path().join("test.db")).await?;
        db.migrate().await?;
        Ok((db, dir))
    }
}

/// Minimal self-deleting temporary directory (avoids an extra dependency).
#[derive(Debug)]
pub struct TempDir(std::path::PathBuf);

impl TempDir {
    pub fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "bento-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p)?;
        Ok(Self(p))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn vendor_image_migration_preserves_v6_data_and_enforces_constraints() {
        let dir = TempDir::new().unwrap();
        let db = Db::connect_path(&dir.path().join("before-images.db"))
            .await
            .unwrap();
        MIGRATOR.run_to(6, &db.write).await.unwrap();
        sqlx::raw_sql("INSERT INTO organizers(id,oidc_issuer,oidc_sub,email,display_name,created_at,last_login_at) VALUES('o','issuer','sub','a@b.test','A','t','t');
            INSERT INTO sharing_groups(id,name,created_by,created_at) VALUES('team','Team','o','t');
            INSERT INTO sharing_group_members(sharing_group_id,organizer_id,role) VALUES('team','o','admin');
            INSERT INTO vendors(id,organizer_id,name,name_key,note,created_at,updated_at) VALUES('v','o','Lunch','lunch','keep','t','t');
            INSERT INTO menu_items(id,vendor_id,name,name_key,price,updated_at) VALUES('m','v','Rice','rice',100,'t');
            INSERT INTO group_orders(id,organizer_id,title,share_token,status,vendor_name,source_vendor_id,created_at) VALUES('g','o','Order','token','open','Lunch','v','t');
            INSERT INTO participants(id,group_order_id,name) VALUES('p','g','Person');
            INSERT INTO offered_items(id,group_order_id,name,price) VALUES('i','g','Rice',100);
            INSERT INTO order_submissions(id,group_order_id,participant_id,idempotency_key,submitted_by,submitted_at) VALUES('s','g','p','once','participant','t');
            INSERT INTO submission_lines(submission_id,line_no,offered_item_id,quantity,unit_price) VALUES('s',0,'i',1,100);")
            .execute(&db.write).await.unwrap();
        db.migrate().await.unwrap();
        let (note, price, title, quantity, images): (String, i64, String, i64, i64) = sqlx::query_as("SELECT v.note,m.price,g.title,l.quantity,(SELECT count(*) FROM vendor_images) FROM vendors v JOIN menu_items m ON m.vendor_id=v.id JOIN group_orders g ON g.source_vendor_id=v.id JOIN submission_lines l ON l.offered_item_id='i'")
            .fetch_one(&db.read).await.unwrap();
        assert_eq!(
            (note, price, title, quantity, images),
            ("keep".into(), 100, "Order".into(), 1, 0)
        );
        let (role,): (String,) = sqlx::query_as("SELECT role FROM sharing_group_members WHERE sharing_group_id='team' AND organizer_id='o'").fetch_one(&db.read).await.unwrap();
        assert_eq!(role, "admin");
        for (vendor, content_type, data, width) in [
            ("missing", "image/jpeg", vec![1], 1),
            ("v", "image/png", vec![1], 1),
            ("v", "image/jpeg", vec![], 1),
            ("v", "image/jpeg", vec![1], 1569),
        ] {
            assert!(sqlx::query("INSERT INTO vendor_images(vendor_id,revision,content_type,data,width,height,updated_at) VALUES(?,'r',?,?,?,1,'t')")
                .bind(vendor).bind(content_type).bind(data).bind(width).execute(&db.write).await.is_err());
        }
        sqlx::query("INSERT INTO vendor_images(vendor_id,revision,content_type,data,width,height,updated_at) VALUES('v','r','image/jpeg',?,1,1,'t')").bind(vec![1u8]).execute(&db.write).await.unwrap();
        sqlx::query("DELETE FROM vendors WHERE id='v'")
            .execute(&db.write)
            .await
            .unwrap();
        let (images,): (i64,) = sqlx::query_as("SELECT count(*) FROM vendor_images")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(images, 0);
        let (orders,): (i64,) = sqlx::query_as("SELECT count(*) FROM group_orders")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(orders, 1);
    }

    #[tokio::test]
    async fn delivery_address_migration_preserves_import_runs() {
        let dir = TempDir::new().unwrap();
        let db = Db::connect_path(&dir.path().join("before-addresses.db"))
            .await
            .unwrap();
        MIGRATOR.run_to(2, &db.write).await.unwrap();
        sqlx::query("INSERT INTO organizers (id, google_sub, email, display_name, created_at, last_login_at) VALUES ('o1', 'g1', 'a@b.c', 'A', 't', 't')").execute(&db.write).await.unwrap();
        sqlx::query("INSERT INTO ai_import_runs (id, organizer_id, mode_hint, status, proposal_json, model, input_tokens, created_at) VALUES ('run1', 'o1', 'vendor', 'proposed', '{\"targets\":[],\"warnings\":[]}', 'test-model', 123, 't')").execute(&db.write).await.unwrap();
        db.migrate().await.unwrap();
        let row: (String, String, String, i64) = sqlx::query_as("SELECT mode_hint, proposal_json, model, input_tokens FROM ai_import_runs WHERE id = 'run1'").fetch_one(&db.read).await.unwrap();
        assert_eq!(
            row,
            (
                "vendor".into(),
                "{\"targets\":[],\"warnings\":[]}".into(),
                "test-model".into(),
                123
            )
        );
        sqlx::query("INSERT INTO ai_import_runs (id, organizer_id, mode_hint, status, model, created_at) VALUES ('run2', 'o1', 'deliveryAddress', 'running', 'test', 't')").execute(&db.write).await.unwrap();
        sqlx::query("DELETE FROM organizers WHERE id = 'o1'")
            .execute(&db.write)
            .await
            .unwrap();
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ai_import_runs")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn oidc_migration_preserves_local_passwords_and_owned_data() {
        let dir = TempDir::new().unwrap();
        let db = Db::connect_path(&dir.path().join("before-oidc.db"))
            .await
            .unwrap();
        MIGRATOR.run_to(3, &db.write).await.unwrap();
        for sql in [
            "INSERT INTO organizers (id, email, display_name, created_at, last_login_at, password_hash) VALUES ('admin', 'admin@example.com', 'Admin', 't', 't', '$existing-hash')",
            "INSERT INTO sessions VALUES (x'0102', 'admin', 't', '2099-01-01T00:00:00Z', NULL)",
            "INSERT INTO delivery_addresses VALUES ('a1', 'admin', '教室', '地址', 't')",
            "INSERT INTO ai_import_runs (id, organizer_id, mode_hint, status, model, created_at) VALUES ('run1', 'admin', 'deliveryAddress', 'proposed', 'test', 't')",
        ] {
            sqlx::query(sql).execute(&db.write).await.unwrap();
        }
        db.migrate().await.unwrap();
        let row: (Option<String>, Option<String>, String) = sqlx::query_as(
            "SELECT oidc_issuer, oidc_sub, password_hash FROM organizers WHERE id = 'admin'",
        )
        .fetch_one(&db.read)
        .await
        .unwrap();
        assert_eq!(row, (None, None, "$existing-hash".into()));
        for table in ["sessions", "delivery_addresses", "ai_import_runs"] {
            let (count,): (i64,) =
                sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
                    .fetch_one(&db.read)
                    .await
                    .unwrap();
            assert_eq!(count, 1);
        }
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&db.write)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn migrations_apply_and_view_exists() {
        let (db, _dir) = Db::open_temp().await.unwrap();
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='view' AND name='effective_submissions'",
        )
        .fetch_one(&db.read)
        .await
        .unwrap();
        assert_eq!(n, 1);
        let (t,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN \
             ('organizers','sessions','oauth_states','rosters','roster_members','vendors','menu_items',\
              'group_orders','participants','offered_items','order_submissions','submission_lines',\
              'settlements','ai_import_runs')",
        )
        .fetch_one(&db.read)
        .await
        .unwrap();
        assert_eq!(t, 14);
        // The view is queryable.
        sqlx::query("SELECT * FROM effective_submissions")
            .fetch_all(&db.read)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn creates_missing_parent_dir() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a/b/bento.db");
        let url = format!("sqlite://{}", file.display());
        Db::connect(&url).await.unwrap();
        assert!(file.exists());
        let file2 = dir.path().join("c/d.db");
        Db::connect_path(&file2).await.unwrap();
        assert!(file2.exists());
    }

    #[tokio::test]
    async fn pragmas_applied() {
        let (db, _dir) = Db::open_temp().await.unwrap();
        let (fk,): (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&db.write)
            .await
            .unwrap();
        assert_eq!(fk, 1);
        let (mode,): (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&db.write)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(db.write.options().get_max_connections(), 1);
    }

    /// Migration 0002 rebuilds `organizers`; data and the foreign keys pointing at it must survive.
    #[tokio::test]
    async fn local_accounts_migration_preserves_data_and_cascades() {
        let dir = TempDir::new().unwrap();
        let db = Db::connect_path(&dir.path().join("old.db")).await.unwrap();
        MIGRATOR.run_to(1, &db.write).await.unwrap();
        for sql in [
            "INSERT INTO organizers VALUES ('o1','g-1','a@b.c','Ann','http://p/a.png','2024-01-01T00:00:00Z','2024-02-01T00:00:00Z')",
            "INSERT INTO organizers VALUES ('o2','dev:x','x@b.c','Xi',NULL,'2024-01-02T00:00:00Z','2024-02-02T00:00:00Z')",
            "INSERT INTO sessions VALUES (x'0102','o1','2024-01-01T00:00:00Z','2099-01-01T00:00:00Z','UA')",
            "INSERT INTO rosters VALUES ('r1','o1','Team','2024-01-01T00:00:00Z','2024-01-01T00:00:00Z')",
            "INSERT INTO vendors (id, organizer_id, name, name_key, created_at, updated_at) VALUES ('v1','o2','V','v','2024-01-01T00:00:00Z','2024-01-01T00:00:00Z')",
        ] {
            sqlx::query(sql).execute(&db.write).await.unwrap();
        }

        db.migrate().await.unwrap();

        type Row = (
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            String,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, oidc_sub, email, avatar_url, password_hash, last_login_at \
                 FROM organizers ORDER BY id",
        )
        .fetch_all(&db.read)
        .await
        .unwrap();
        assert_eq!(
            rows,
            [
                (
                    "o1".into(),
                    Some("g-1".into()),
                    "a@b.c".into(),
                    Some("http://p/a.png".into()),
                    None,
                    "2024-02-01T00:00:00Z".into()
                ),
                (
                    "o2".into(),
                    Some("dev:x".into()),
                    "x@b.c".into(),
                    None,
                    None,
                    "2024-02-02T00:00:00Z".into()
                ),
            ]
        );
        let issuers: Vec<String> =
            sqlx::query_scalar("SELECT oidc_issuer FROM organizers ORDER BY id")
                .fetch_all(&db.read)
                .await
                .unwrap();
        assert_eq!(
            issuers,
            ["https://accounts.google.com", "urn:order-a-bento:dev"]
        );
        // Child rows survived the rebuild, the schema is consistent and enforcement is back on.
        for (table, n) in [("sessions", 1), ("rosters", 1), ("vendors", 1)] {
            let (c,): (i64,) =
                sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
                    .fetch_one(&db.read)
                    .await
                    .unwrap();
            assert_eq!(c, n, "{table}");
        }
        let violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&db.write)
            .await
            .unwrap();
        assert!(violations.is_empty());
        let (fk,): (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&db.write)
            .await
            .unwrap();
        assert_eq!(fk, 1);
        // FKs still reference `organizers` (not a renamed leftover) and reject orphans.
        let orphan = sqlx::query(
            "INSERT INTO rosters VALUES ('r2','nobody','T','2024-01-01T00:00:00Z','2024-01-01T00:00:00Z')",
        )
        .execute(&db.write)
        .await;
        assert!(orphan.is_err());
        let (refs,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE sql LIKE '%organizers_new%' OR sql LIKE '%organizers_old%'",
        )
        .fetch_one(&db.read)
        .await
        .unwrap();
        assert_eq!(refs, 0);

        // ON DELETE CASCADE still works from the rebuilt table.
        sqlx::query("DELETE FROM organizers WHERE id = 'o1'")
            .execute(&db.write)
            .await
            .unwrap();
        for table in ["sessions", "rosters"] {
            let (c,): (i64,) =
                sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
                    .fetch_one(&db.read)
                    .await
                    .unwrap();
            assert_eq!(c, 0, "{table} should cascade");
        }
        let (c,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vendors")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(c, 1);

        // Local passwords and their uniqueness survive both migrations.
        sqlx::query(
            "INSERT INTO organizers (id, oidc_sub, email, display_name, created_at, last_login_at, password_hash) \
             VALUES ('o3', NULL, 'l@b.c', 'L', 't', 't', '$h')",
        )
        .execute(&db.write)
        .await
        .unwrap();
        let dup = sqlx::query(
            "INSERT INTO organizers (id, oidc_sub, email, display_name, created_at, last_login_at, password_hash) \
             VALUES ('o4', NULL, 'l@b.c', 'L', 't', 't', '$h')",
        )
        .execute(&db.write)
        .await;
        assert!(dup.is_err());
    }
}
