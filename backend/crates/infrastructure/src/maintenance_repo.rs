//! SQL for the background maintenance job.

use std::time::SystemTime;

use application::MaintenanceRepo;
use application::ports::{PortError, PortResult};
use async_trait::async_trait;

use crate::sqlite::Db;
use crate::timefmt::to_text;

#[derive(Clone, Debug)]
pub struct SqliteMaintenanceRepo {
    db: Db,
}

impl SqliteMaintenanceRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn err(e: impl std::fmt::Display) -> PortError {
    PortError::new(e)
}

#[async_trait]
impl MaintenanceRepo for SqliteMaintenanceRepo {
    async fn clear_client_info(&self, closed_before: SystemTime) -> PortResult<u64> {
        let r = sqlx::query(
            "UPDATE order_submissions SET client_ip = NULL, user_agent = NULL, ua_summary = NULL, \
               device_id = NULL, device_model = NULL, screen = NULL, language = NULL, timezone = NULL \
             WHERE (client_ip IS NOT NULL OR user_agent IS NOT NULL OR ua_summary IS NOT NULL \
               OR device_id IS NOT NULL OR device_model IS NOT NULL OR screen IS NOT NULL \
               OR language IS NOT NULL OR timezone IS NOT NULL) \
             AND group_order_id IN (SELECT id FROM group_orders \
               WHERE status IN ('closed','archived') AND closed_at IS NOT NULL AND closed_at <= ?)",
        )
        .bind(to_text(closed_before))
        .execute(&self.db.write)
        .await
        .map_err(err)?;
        Ok(r.rows_affected())
    }

    async fn fail_stuck_import_runs(
        &self,
        created_before: SystemTime,
        now: SystemTime,
    ) -> PortResult<u64> {
        let r = sqlx::query(
            "UPDATE ai_import_runs SET status = 'failed', error = 'interrupted', finished_at = ? \
             WHERE status = 'running' AND created_at <= ?",
        )
        .bind(to_text(now))
        .bind(to_text(created_before))
        .execute(&self.db.write)
        .await
        .map_err(err)?;
        Ok(r.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use application::ports::{OAuthState, OAuthStateRepo, SessionRepo};
    use application::token::TokenHash;
    use application::{Maintenance, ports::Clock};
    use std::sync::Arc;

    use super::*;
    use crate::identity_repo::{SqliteOAuthStateRepo, SqliteOrganizerRepo, SqliteSessionRepo};
    use application::ports::OrganizerRepo;
    use domain::identity::Organizer;

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    struct Fixed;
    impl Clock for Fixed {
        fn now(&self) -> SystemTime {
            t(NOW)
        }
    }

    async fn seed_group(db: &Db, id: &str, org: &str, status: &str, closed_at: Option<u64>) {
        sqlx::query(
            "INSERT INTO group_orders (id, organizer_id, title, share_token, status, vendor_name, created_at, closed_at) \
             VALUES (?, ?, 't', ?, ?, 'v', ?, ?)",
        )
        .bind(id)
        .bind(org)
        .bind(format!("tok-{id}"))
        .bind(status)
        .bind(to_text(t(1)))
        .bind(closed_at.map(|c| to_text(t(c))))
        .execute(&db.write)
        .await
        .unwrap();
        sqlx::query("INSERT INTO participants (id, group_order_id, name) VALUES (?, ?, 'p')")
            .bind(format!("p-{id}"))
            .bind(id)
            .execute(&db.write)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO offered_items (id, group_order_id, name, price) VALUES (?, ?, 'i', 50)",
        )
        .bind(format!("i-{id}"))
        .bind(id)
        .execute(&db.write)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO order_submissions (id, group_order_id, participant_id, note, idempotency_key, \
               submitted_by, submitted_at, client_ip, user_agent, ua_summary, device_id, device_model, \
               screen, language, timezone) \
             VALUES (?, ?, ?, 'keep me', 'k', 'participant', ?, '1.2.3.4', 'UA', 'Chrome', 'dev1', 'Pixel', '1x1', 'zh', 'Asia/Taipei')",
        )
        .bind(format!("s-{id}"))
        .bind(id)
        .bind(format!("p-{id}"))
        .bind(to_text(t(2)))
        .execute(&db.write)
        .await
        .unwrap();
        sqlx::query("INSERT INTO submission_lines (submission_id, line_no, offered_item_id, quantity, unit_price) VALUES (?, 0, ?, 2, 50)")
            .bind(format!("s-{id}"))
            .bind(format!("i-{id}"))
            .execute(&db.write)
            .await
            .unwrap();
    }

    type Info = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );

    async fn ip_and_note(
        db: &Db,
        id: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) {
        let (a, b, c, d, e): Info = sqlx::query_as(
            "SELECT client_ip, user_agent, device_id, timezone, note FROM order_submissions WHERE id = ?",
        )
        .bind(format!("s-{id}"))
        .fetch_one(&db.read)
        .await
        .unwrap();
        (a, b, c, d, e.unwrap())
    }

    #[tokio::test]
    async fn maintenance_purges_clears_and_fails_stuck_runs() {
        let (db, _dir) = Db::open_temp().await.unwrap();
        let org = SqliteOrganizerRepo::new(db.clone())
            .upsert_from_identity(
                Organizer::new("https://id.example", "sub", "a@b.c", "A", None).unwrap(),
                t(0),
            )
            .await
            .unwrap();
        let org_id = org.id.to_string();

        // sessions / oauth states: one expired, one valid each
        let sessions = SqliteSessionRepo::new(db.clone());
        sessions
            .create(TokenHash::of("old"), org.id, t(0), t(NOW - 10), None)
            .await
            .unwrap();
        sessions
            .create(TokenHash::of("new"), org.id, t(0), t(NOW + 1000), None)
            .await
            .unwrap();
        let states = SqliteOAuthStateRepo::new(db.clone());
        for (s, exp) in [("old", NOW - 10), ("new", NOW + 1000)] {
            states
                .save(OAuthState {
                    state: s.into(),
                    pkce_verifier: "v".into(),
                    nonce: "n".into(),
                    return_to: "/".into(),
                    expires_at: t(exp),
                })
                .await
                .unwrap();
        }

        // groups
        seed_group(&db, "old_closed", &org_id, "closed", Some(NOW - 91 * DAY)).await;
        seed_group(&db, "old_arch", &org_id, "archived", Some(NOW - 200 * DAY)).await;
        seed_group(&db, "recent", &org_id, "closed", Some(NOW - 89 * DAY)).await;
        seed_group(&db, "open", &org_id, "open", None).await;

        // AI runs
        for (id, status, created) in [
            ("r_stuck", "running", NOW - 11 * 60),
            ("r_recent", "running", NOW - 5 * 60),
            ("r_done", "proposed", NOW - 3600),
        ] {
            sqlx::query("INSERT INTO ai_import_runs (id, organizer_id, status, model, created_at) VALUES (?, ?, ?, 'm', ?)")
                .bind(id)
                .bind(&org_id)
                .bind(status)
                .bind(to_text(t(created)))
                .execute(&db.write)
                .await
                .unwrap();
        }

        let m = Maintenance {
            sessions: Arc::new(sessions.clone()),
            oauth_states: Arc::new(states.clone()),
            repo: Arc::new(SqliteMaintenanceRepo::new(db.clone())),
            clock: Arc::new(Fixed),
            client_info_retention_days: 90,
        };
        let (r, e) = m.run_once().await;
        assert!(e.is_none());
        assert_eq!(
            (
                r.sessions_deleted,
                r.oauth_states_deleted,
                r.client_info_cleared,
                r.import_runs_failed
            ),
            (1, 1, 2, 1)
        );

        assert!(
            sessions
                .find_valid(TokenHash::of("old"), t(NOW))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            sessions
                .find_valid(TokenHash::of("new"), t(NOW))
                .await
                .unwrap()
                .is_some()
        );
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM oauth_states")
            .fetch_one(&db.read)
            .await
            .unwrap();
        assert_eq!(n, 1);

        for id in ["old_closed", "old_arch"] {
            let (ip, ua, dev, tz, note) = ip_and_note(&db, id).await;
            assert!(
                ip.is_none() && ua.is_none() && dev.is_none() && tz.is_none(),
                "{id}"
            );
            assert_eq!(note, "keep me");
            let (l,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM submission_lines WHERE submission_id = ?")
                    .bind(format!("s-{id}"))
                    .fetch_one(&db.read)
                    .await
                    .unwrap();
            assert_eq!(l, 1);
        }
        for id in ["recent", "open"] {
            let (ip, ua, dev, tz, _) = ip_and_note(&db, id).await;
            assert_eq!(
                (ip.as_deref(), ua.as_deref(), dev.as_deref(), tz.as_deref()),
                (
                    Some("1.2.3.4"),
                    Some("UA"),
                    Some("dev1"),
                    Some("Asia/Taipei")
                ),
                "{id}"
            );
        }

        let status = |id: &'static str| {
            let db = db.clone();
            async move {
                sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT status, error FROM ai_import_runs WHERE id = ?",
                )
                .bind(id)
                .fetch_one(&db.read)
                .await
                .unwrap()
            }
        };
        assert_eq!(
            status("r_stuck").await,
            ("failed".into(), Some("interrupted".into()))
        );
        assert_eq!(status("r_recent").await.0, "running");
        assert_eq!(status("r_done").await.0, "proposed");

        // idempotent
        let (r2, _) = m.run_once().await;
        assert_eq!(r2, Default::default());
    }
}
