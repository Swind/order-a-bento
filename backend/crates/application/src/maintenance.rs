//! Daily housekeeping (design §10/§11): expired logins, ClientInfo retention,
//! interrupted AI import runs.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::ports::{Clock, OAuthStateRepo, PortResult, SessionRepo};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A run still `running` after this long is considered interrupted (server restart, crash).
pub const STUCK_RUN_AFTER: Duration = Duration::from_secs(10 * 60);

#[async_trait]
pub trait MaintenanceRepo: Send + Sync {
    /// Set the ClientInfo columns of `order_submissions` to NULL for every group
    /// in status `closed`/`archived` whose `closed_at <= closed_before`.
    /// (Archiving keeps `closed_at`; reopened groups have it cleared, so open
    /// and reopened groups are never touched.) Returns the number of submissions changed.
    async fn clear_client_info(&self, closed_before: std::time::SystemTime) -> PortResult<u64>;
    /// Mark AI import runs `running` and created at or before `created_before`
    /// as `failed` with error `"interrupted"`. Returns the number of runs changed.
    async fn fail_stuck_import_runs(
        &self,
        created_before: std::time::SystemTime,
        now: std::time::SystemTime,
    ) -> PortResult<u64>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MaintenanceReport {
    pub sessions_deleted: u64,
    pub oauth_states_deleted: u64,
    pub client_info_cleared: u64,
    pub import_runs_failed: u64,
}

pub struct Maintenance {
    pub sessions: Arc<dyn SessionRepo>,
    pub oauth_states: Arc<dyn OAuthStateRepo>,
    pub repo: Arc<dyn MaintenanceRepo>,
    pub clock: Arc<dyn Clock>,
    pub client_info_retention_days: u32,
}

impl Maintenance {
    /// Run every step once. A failing step does not prevent the others; the first
    /// error is returned together with the counts gathered so far.
    pub async fn run_once(&self) -> (MaintenanceReport, Option<crate::ports::PortError>) {
        let now = self.clock.now();
        let mut r = MaintenanceReport::default();
        let mut first_err = None;
        let mut note = |e: crate::ports::PortError| {
            tracing::warn!(error = %e, "maintenance step failed");
            first_err.get_or_insert(e);
        };

        match self.sessions.delete_expired(now).await {
            Ok(n) => r.sessions_deleted = n,
            Err(e) => note(e),
        }
        match self.oauth_states.delete_expired(now).await {
            Ok(n) => r.oauth_states_deleted = n,
            Err(e) => note(e),
        }
        let cutoff = now - DAY * self.client_info_retention_days;
        match self.repo.clear_client_info(cutoff).await {
            Ok(n) => r.client_info_cleared = n,
            Err(e) => note(e),
        }
        match self
            .repo
            .fail_stuck_import_runs(now - STUCK_RUN_AFTER, now)
            .await
        {
            Ok(n) => r.import_runs_failed = n,
            Err(e) => note(e),
        }
        (r, first_err)
    }
}
