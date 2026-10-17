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
}
