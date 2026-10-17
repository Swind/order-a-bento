//! SQLite repositories for `organizers`, `sessions` and `oauth_states`.

use std::time::SystemTime;

use application::ports::{
    OAuthState, OAuthStateRepo, OrganizerRepo, PortError, PortResult, SessionRepo,
};
use application::token::TokenHash;
use async_trait::async_trait;
use domain::identity::{Organizer, OrganizerId};

use crate::sqlite::Db;
use crate::timefmt::{from_text, to_text};

fn err(e: impl std::fmt::Display) -> PortError {
    PortError::new(e)
}

#[derive(sqlx::FromRow)]
struct OrganizerRow {
    id: String,
    google_sub: String,
    email: String,
    display_name: String,
    avatar_url: Option<String>,
}

impl OrganizerRow {
    fn into_domain(self) -> PortResult<Organizer> {
        Ok(Organizer {
            id: self.id.parse().map_err(err)?,
            google_sub: self.google_sub,
            email: self.email,
            display_name: self.display_name,
            avatar_url: self.avatar_url,
        })
    }
}

#[derive(Clone, Debug)]
pub struct SqliteOrganizerRepo {
    db: Db,
}

impl SqliteOrganizerRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OrganizerRepo for SqliteOrganizerRepo {
    async fn find_by_google_sub(&self, google_sub: &str) -> PortResult<Option<Organizer>> {
        sqlx::query_as::<_, OrganizerRow>(
            "SELECT id, google_sub, email, display_name, avatar_url FROM organizers WHERE google_sub = ?",
        )
        .bind(google_sub)
        .fetch_optional(&self.db.read)
        .await
        .map_err(err)?
        .map(OrganizerRow::into_domain)
        .transpose()
    }

    async fn find_by_id(&self, id: OrganizerId) -> PortResult<Option<Organizer>> {
        sqlx::query_as::<_, OrganizerRow>(
            "SELECT id, google_sub, email, display_name, avatar_url FROM organizers WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.db.read)
        .await
        .map_err(err)?
        .map(OrganizerRow::into_domain)
        .transpose()
    }

    async fn upsert_from_identity(&self, c: Organizer, now: SystemTime) -> PortResult<Organizer> {
        let now = to_text(now);
        sqlx::query_as::<_, OrganizerRow>(
            "INSERT INTO organizers (id, google_sub, email, display_name, avatar_url, created_at, last_login_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(google_sub) DO UPDATE SET \
               email = excluded.email, display_name = excluded.display_name, \
               avatar_url = excluded.avatar_url, last_login_at = excluded.last_login_at \
             RETURNING id, google_sub, email, display_name, avatar_url",
        )
        .bind(c.id.to_string())
        .bind(&c.google_sub)
        .bind(&c.email)
        .bind(&c.display_name)
        .bind(&c.avatar_url)
        .bind(&now)
        .bind(&now)
        .fetch_one(&self.db.write)
        .await
        .map_err(err)?
        .into_domain()
    }

    async fn touch_last_login(&self, id: OrganizerId, now: SystemTime) -> PortResult<()> {
        sqlx::query("UPDATE organizers SET last_login_at = ? WHERE id = ?")
            .bind(to_text(now))
            .bind(id.to_string())
            .execute(&self.db.write)
            .await
            .map_err(err)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SqliteSessionRepo {
    db: Db,
}

impl SqliteSessionRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SessionRepo for SqliteSessionRepo {
    async fn create(
        &self,
        hash: TokenHash,
        organizer_id: OrganizerId,
        now: SystemTime,
        expires_at: SystemTime,
        user_agent: Option<&str>,
    ) -> PortResult<()> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, organizer_id, created_at, expires_at, user_agent) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(hash.as_bytes())
        .bind(organizer_id.to_string())
        .bind(to_text(now))
        .bind(to_text(expires_at))
        .bind(user_agent)
        .execute(&self.db.write)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn find_valid(
        &self,
        hash: TokenHash,
        now: SystemTime,
    ) -> PortResult<Option<(OrganizerId, SystemTime)>> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT organizer_id, expires_at FROM sessions WHERE token_hash = ? AND expires_at > ?",
        )
        .bind(hash.as_bytes())
        .bind(to_text(now))
        .fetch_optional(&self.db.read)
        .await
        .map_err(err)?;
        row.map(|(o, e)| Ok((o.parse().map_err(err)?, from_text(&e).map_err(err)?)))
            .transpose()
    }

    async fn extend(&self, hash: TokenHash, new_expires_at: SystemTime) -> PortResult<()> {
        sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
            .bind(to_text(new_expires_at))
            .bind(hash.as_bytes())
            .execute(&self.db.write)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn delete(&self, hash: TokenHash) -> PortResult<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(hash.as_bytes())
            .execute(&self.db.write)
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn delete_expired(&self, now: SystemTime) -> PortResult<u64> {
        let r = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(to_text(now))
            .execute(&self.db.write)
            .await
            .map_err(err)?;
        Ok(r.rows_affected())
    }
}

#[derive(Clone, Debug)]
pub struct SqliteOAuthStateRepo {
    db: Db,
}

impl SqliteOAuthStateRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OAuthStateRepo for SqliteOAuthStateRepo {
    async fn save(&self, s: OAuthState) -> PortResult<()> {
        sqlx::query(
            "INSERT INTO oauth_states (state, pkce_verifier, nonce, return_to, expires_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&s.state)
        .bind(&s.pkce_verifier)
        .bind(&s.nonce)
        .bind(&s.return_to)
        .bind(to_text(s.expires_at))
        .execute(&self.db.write)
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn take(&self, state: &str, now: SystemTime) -> PortResult<Option<OAuthState>> {
        // DELETE .. RETURNING consumes the row atomically, so a state can never be used twice.
        let row: Option<(String, String, String, Option<String>, String)> = sqlx::query_as(
            "DELETE FROM oauth_states WHERE state = ? \
             RETURNING state, pkce_verifier, nonce, return_to, expires_at",
        )
        .bind(state)
        .fetch_optional(&self.db.write)
        .await
        .map_err(err)?;
        let Some((state, pkce_verifier, nonce, return_to, expires_at)) = row else {
            return Ok(None);
        };
        let expires_at = from_text(&expires_at).map_err(err)?;
        if expires_at <= now {
            return Ok(None);
        }
        Ok(Some(OAuthState {
            state,
            pkce_verifier,
            nonce,
            return_to: return_to.unwrap_or_else(|| "/".into()),
            expires_at,
        }))
    }

    async fn delete_expired(&self, now: SystemTime) -> PortResult<u64> {
        let r = sqlx::query("DELETE FROM oauth_states WHERE expires_at <= ?")
            .bind(to_text(now))
            .execute(&self.db.write)
            .await
            .map_err(err)?;
        Ok(r.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    async fn setup() -> (Db, crate::sqlite::TempDir, Organizer) {
        let (db, dir) = Db::open_temp().await.unwrap();
        let o = SqliteOrganizerRepo::new(db.clone())
            .upsert_from_identity(Organizer::new("sub1", "a@b.c", "A", None).unwrap(), t(0))
            .await
            .unwrap();
        (db, dir, o)
    }

    #[tokio::test]
    async fn organizer_upsert_keeps_id_and_updates_profile() {
        let (db, _d, first) = setup().await;
        let repo = SqliteOrganizerRepo::new(db);
        let second = repo
            .upsert_from_identity(
                Organizer::new("sub1", "new@b.c", "New", Some("http://p/x.png".into())).unwrap(),
                t(10),
            )
            .await
            .unwrap();
        assert_eq!(second.id, first.id);
        assert_eq!(second.email, "new@b.c");
        assert_eq!(second.avatar_url.as_deref(), Some("http://p/x.png"));
        assert_eq!(repo.find_by_id(first.id).await.unwrap().unwrap(), second);
        assert_eq!(
            repo.find_by_google_sub("sub1").await.unwrap().unwrap(),
            second
        );
        assert!(repo.find_by_google_sub("zzz").await.unwrap().is_none());
        repo.touch_last_login(first.id, t(20)).await.unwrap();
        let (ll,): (String,) = sqlx::query_as("SELECT last_login_at FROM organizers WHERE id = ?")
            .bind(first.id.to_string())
            .fetch_one(&repo.db.read)
            .await
            .unwrap();
        assert_eq!(ll, to_text(t(20)));
    }

    #[tokio::test]
    async fn session_stored_as_hash_and_expiry_honoured() {
        let (db, _d, o) = setup().await;
        let repo = SqliteSessionRepo::new(db.clone());
        let raw = "raw-token-value";
        let h = TokenHash::of(raw);
        repo.create(h, o.id, t(0), t(100), Some("UA"))
            .await
            .unwrap();

        // Only the hash is stored, never the raw token.
        let (stored,): (Vec<u8>,) = sqlx::query_as("SELECT token_hash FROM sessions")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(stored, h.as_bytes());
        assert_ne!(stored, raw.as_bytes());

        let (oid, exp) = repo.find_valid(h, t(50)).await.unwrap().unwrap();
        assert_eq!((oid, exp), (o.id, t(100)));
        assert!(repo.find_valid(h, t(100)).await.unwrap().is_none());
        assert!(repo.find_valid(h, t(500)).await.unwrap().is_none());

        repo.extend(h, t(1000)).await.unwrap();
        assert_eq!(
            repo.find_valid(h, t(500)).await.unwrap().unwrap().1,
            t(1000)
        );

        let h2 = TokenHash::of("other");
        repo.create(h2, o.id, t(0), t(10), None).await.unwrap();
        assert_eq!(repo.delete_expired(t(20)).await.unwrap(), 1);
        repo.delete(h).await.unwrap();
        assert!(repo.find_valid(h, t(0)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn oauth_state_is_consumed_once_and_expires() {
        let (db, _d) = Db::open_temp().await.unwrap();
        let repo = SqliteOAuthStateRepo::new(db);
        let s = |name: &str, exp| OAuthState {
            state: name.into(),
            pkce_verifier: "v".into(),
            nonce: "n".into(),
            return_to: "/x".into(),
            expires_at: exp,
        };
        repo.save(s("a", t(100))).await.unwrap();
        repo.save(s("b", t(100))).await.unwrap();
        assert_eq!(
            repo.take("a", t(50)).await.unwrap().unwrap(),
            s("a", t(100))
        );
        assert!(repo.take("a", t(50)).await.unwrap().is_none());
        assert!(repo.take("b", t(150)).await.unwrap().is_none());
        assert!(repo.take("b", t(50)).await.unwrap().is_none()); // expired take still consumed
        repo.save(s("c", t(10))).await.unwrap();
        assert_eq!(repo.delete_expired(t(20)).await.unwrap(), 1);
    }
}
