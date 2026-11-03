//! SQLite persistence of AI import runs and the apply unit of work.
//!
//! # Unit of work
//!
//! [`ImportApplyRepo::apply`] runs the run's `proposed -> applied` flip and all
//! roster/vendor writes inside **one** `BEGIN IMMEDIATE` transaction on the
//! single-connection write pool. It reuses the transaction-level functions of
//! [`crate::directory_repo`] (`create_*_tx` / `modify_*_tx`), so each step has
//! exactly the semantics of the corresponding repository call; any failure
//! rolls everything back. The flip is a compare-and-set
//! (`WHERE status = 'proposed'`), so two concurrent applies cannot both win.

use std::time::SystemTime;

use application::ports::{
    ImportApplyRepo, ImportApplyStep, ImportRunRepo, PortError, PortResult, RepoError, RepoResult,
};
use async_trait::async_trait;
use domain::ai_import::{ImportMode, ImportRun, ImportRunId, ImportRunStatus, Proposal, Usage};
use domain::identity::OrganizerId;
use domain::shared::DomainError;

use crate::directory_repo::{
    create_roster_tx, create_vendor_tx, modify_roster_tx, modify_vendor_tx,
};
use crate::sqlite::Db;
use crate::timefmt::{from_text, to_text};

fn port(e: impl std::fmt::Display) -> PortError {
    PortError::new(e)
}

fn bad_status() -> RepoError {
    RepoError::Domain(DomainError::invalid("status", "狀態已變更，不能執行此操作"))
}

#[derive(Clone, Debug)]
pub struct SqliteImportRunRepo {
    db: Db,
}

impl SqliteImportRunRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct RunRow {
    draft_json: Option<String>,
    sharing_group_id: Option<String>,
    id: String,
    organizer_id: String,
    mode_hint: Option<String>,
    target_roster_id: Option<String>,
    target_vendor_id: Option<String>,
    instruction: Option<String>,
    status: String,
    proposal_json: Option<String>,
    summary: Option<String>,
    model: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    error: Option<String>,
    tool_calls_json: Option<String>,
    created_at: String,
    finished_at: Option<String>,
    applied_at: Option<String>,
    pending_photo_expires_at: Option<String>,
}

macro_rules! columns {
    () => {
        "id, organizer_id, mode_hint, target_roster_id, target_vendor_id, instruction, status, \
         proposal_json, summary, model, input_tokens, output_tokens, error, tool_calls_json, created_at, \
         finished_at, applied_at, sharing_group_id"
    };
}

fn opt_ts(s: Option<String>) -> PortResult<Option<SystemTime>> {
    s.map(|s| from_text(&s).map_err(port)).transpose()
}

impl RunRow {
    fn into_run(self) -> PortResult<ImportRun> {
        Ok(ImportRun {
            draft: self
                .draft_json
                .map(|j| serde_json::from_str(&j).map_err(port))
                .transpose()?,
            pending_photo_expires_at: opt_ts(self.pending_photo_expires_at)?,
            sharing_group_id: self
                .sharing_group_id
                .map(|s| s.parse().map_err(port))
                .transpose()?,
            id: self.id.parse().map_err(port)?,
            organizer_id: self.organizer_id.parse().map_err(port)?,
            mode: self
                .mode_hint
                .as_deref()
                .map(ImportMode::parse)
                .transpose()
                .map_err(port)?,
            target_roster_id: self
                .target_roster_id
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(port)?,
            target_vendor_id: self
                .target_vendor_id
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(port)?,
            instruction: self.instruction,
            status: ImportRunStatus::parse(&self.status).map_err(port)?,
            proposal: match self.proposal_json {
                Some(j) => serde_json::from_str::<Proposal>(&j).map_err(port)?,
                None => Proposal::default(),
            },
            summary: self.summary,
            model: self.model,
            usage: Usage {
                input_tokens: self.input_tokens.unwrap_or(0).max(0) as u64,
                output_tokens: self.output_tokens.unwrap_or(0).max(0) as u64,
            },
            error: self.error,
            tool_calls: self.tool_calls_json,
            created_at: from_text(&self.created_at).map_err(port)?,
            finished_at: opt_ts(self.finished_at)?,
            applied_at: opt_ts(self.applied_at)?,
        })
    }
}

#[async_trait]
impl ImportRunRepo for SqliteImportRunRepo {
    async fn draft(&self, run: &ImportRun) -> RepoResult<domain::ai_import::DraftState> {
        let state = domain::ai_import::DraftState::from_proposal(&run.proposal);
        // Initialization is idempotent; all later writes use a revision CAS.
        sqlx::query("INSERT OR IGNORE INTO ai_import_drafts(run_id,revision,state_json) SELECT id,0,? FROM ai_import_runs WHERE id=? AND organizer_id=? AND status='proposed'")
            .bind(serde_json::to_string(&state).map_err(port)?).bind(run.id.to_string()).bind(run.organizer_id.to_string()).execute(&self.db.write).await.map_err(port)?;
        let row: Option<(String,)> = sqlx::query_as("SELECT d.state_json FROM ai_import_drafts d JOIN ai_import_runs r ON r.id=d.run_id WHERE r.id=? AND r.organizer_id=? AND r.status='proposed'")
            .bind(run.id.to_string()).bind(run.organizer_id.to_string()).fetch_optional(&self.db.read).await.map_err(port)?;
        serde_json::from_str(&row.ok_or_else(bad_status)?.0).map_err(|e| port(e).into())
    }

    async fn save_draft(
        &self,
        run: &ImportRun,
        state: &domain::ai_import::DraftState,
        expected: i64,
        now: SystemTime,
        quota: Option<u32>,
    ) -> RepoResult<()> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        crate::directory_repo::scope_admin(&mut tx, run.organizer_id, run.sharing_group_id).await?;
        if let Some(quota) = quota {
            let since = now
                .checked_sub(std::time::Duration::from_secs(86400))
                .unwrap_or(std::time::UNIX_EPOCH);
            let (n,): (i64,) = sqlx::query_as("SELECT (SELECT count(*) FROM ai_import_runs WHERE organizer_id=? AND created_at>=?) + (SELECT count(*) FROM ai_import_turn_usage WHERE organizer_id=? AND created_at>=?)")
                .bind(run.organizer_id.to_string()).bind(to_text(since)).bind(run.organizer_id.to_string()).bind(to_text(since)).fetch_one(&mut *tx).await.map_err(port)?;
            if n >= i64::from(quota) {
                return Err(RepoError::Domain(DomainError::invalid(
                    "quota",
                    "daily AI quota exceeded",
                )));
            }
        }
        let since = now
            .checked_sub(std::time::Duration::from_secs(86400))
            .unwrap_or(std::time::UNIX_EPOCH);
        let done = sqlx::query("UPDATE ai_import_drafts SET revision=?,state_json=? WHERE run_id=? AND revision=? AND EXISTS(SELECT 1 FROM ai_import_runs r WHERE r.id=run_id AND r.organizer_id=? AND r.status='proposed' AND r.mode_hint='vendor' AND r.created_at>?)")
            .bind(state.revision).bind(serde_json::to_string(state).map_err(port)?).bind(run.id.to_string()).bind(expected).bind(run.organizer_id.to_string()).bind(to_text(since)).execute(&mut *tx).await.map_err(port)?;
        if done.rows_affected() != 1 {
            return Err(RepoError::UniqueViolation);
        }
        if quota.is_some() {
            sqlx::query("INSERT INTO ai_import_turn_usage(run_id,turn_id,organizer_id,created_at) VALUES(?,?,?,?)")
                .bind(run.id.to_string()).bind(state.pending.as_deref()).bind(run.organizer_id.to_string()).bind(to_text(now)).execute(&mut *tx).await.map_err(port)?;
        }
        tx.commit().await.map_err(port)?;
        Ok(())
    }

    async fn draft_image(&self, run: &ImportRun, now: SystemTime) -> PortResult<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as("SELECT p.data FROM ai_import_photos p JOIN ai_import_runs r ON r.id=p.run_id WHERE r.id=? AND r.organizer_id=? AND r.status='proposed' AND p.expires_at>?")
            .bind(run.id.to_string()).bind(run.organizer_id.to_string()).bind(to_text(now)).fetch_optional(&self.db.read).await.map_err(port)?;
        Ok(row.map(|r| r.0))
    }
    async fn create(
        &self,
        run: &ImportRun,
        photo: Option<application::ports::NormalizedVendorImage>,
    ) -> PortResult<()> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let proposal = serde_json::to_string(&run.proposal).map_err(port)?;
        sqlx::query(concat!(
            "INSERT INTO ai_import_runs (",
            columns!(),
            ") VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(run.id.to_string())
        .bind(run.organizer_id.to_string())
        .bind(run.mode.map(ImportMode::as_str))
        .bind(run.target_roster_id.map(|i| i.to_string()))
        .bind(run.target_vendor_id.map(|i| i.to_string()))
        .bind(&run.instruction)
        .bind(run.status.as_str())
        .bind(proposal)
        .bind(&run.summary)
        .bind(&run.model)
        .bind(run.usage.input_tokens as i64)
        .bind(run.usage.output_tokens as i64)
        .bind(&run.error)
        .bind(&run.tool_calls)
        .bind(to_text(run.created_at))
        .bind(run.finished_at.map(to_text))
        .bind(run.applied_at.map(to_text))
        .bind(run.sharing_group_id.map(|g| g.to_string()))
        .execute(&mut *tx)
        .await
        .map_err(port)?;
        if let Some(photo) = photo {
            sqlx::query("INSERT INTO ai_import_photos(run_id,data,width,height,expires_at) VALUES(?,?,?,?,?)")
                .bind(run.id.to_string()).bind(photo.data).bind(photo.width).bind(photo.height)
                .bind(to_text(run.created_at + std::time::Duration::from_secs(24 * 3600)))
                .execute(&mut *tx).await.map_err(port)?;
        }
        tx.commit().await.map_err(port)?;
        Ok(())
    }

    async fn find(&self, org: OrganizerId, id: ImportRunId) -> PortResult<Option<ImportRun>> {
        sqlx::query_as::<_, RunRow>(concat!(
            "SELECT ",
            columns!(),
            ", (SELECT state_json FROM ai_import_drafts d WHERE d.run_id=ai_import_runs.id) AS draft_json, (SELECT expires_at FROM ai_import_photos p WHERE p.run_id=ai_import_runs.id) AS pending_photo_expires_at FROM ai_import_runs WHERE id = ? AND organizer_id = ?"
        ))
        .bind(id.to_string())
        .bind(org.to_string())
        .fetch_optional(&self.db.read)
        .await
        .map_err(port)?
        .map(RunRow::into_run)
        .transpose()
    }

    async fn save(&self, run: &ImportRun, expected: ImportRunStatus) -> RepoResult<()> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let proposal = serde_json::to_string(&run.proposal).map_err(port)?;
        let done = sqlx::query(
            "UPDATE ai_import_runs SET status = ?, proposal_json = ?, summary = ?, model = ?, \
               input_tokens = ?, output_tokens = ?, error = ?, tool_calls_json = ?, finished_at = ?, applied_at = ? \
             WHERE id = ? AND organizer_id = ? AND status = ?",
        )
        .bind(run.status.as_str())
        .bind(proposal)
        .bind(&run.summary)
        .bind(&run.model)
        .bind(run.usage.input_tokens as i64)
        .bind(run.usage.output_tokens as i64)
        .bind(&run.error)
        .bind(&run.tool_calls)
        .bind(run.finished_at.map(to_text))
        .bind(run.applied_at.map(to_text))
        .bind(run.id.to_string())
        .bind(run.organizer_id.to_string())
        .bind(expected.as_str())
        .execute(&mut *tx)
        .await
        .map_err(port)?;
        if done.rows_affected() == 1 {
            if matches!(
                run.status,
                ImportRunStatus::Failed | ImportRunStatus::Discarded | ImportRunStatus::Applied
            ) {
                sqlx::query("DELETE FROM ai_import_photos WHERE run_id=?")
                    .bind(run.id.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(port)?;
            }
            tx.commit().await.map_err(port)?;
            return Ok(());
        }
        drop(tx);
        miss(&self.db, run).await
    }

    async fn count_since(&self, org: OrganizerId, since: SystemTime) -> PortResult<u64> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM ai_import_runs WHERE organizer_id = ? AND created_at >= ?) + (SELECT COUNT(*) FROM ai_import_turn_usage WHERE organizer_id = ? AND created_at >= ?)",
        )
        .bind(org.to_string())
        .bind(to_text(since))
        .bind(org.to_string())
        .bind(to_text(since))
        .fetch_one(&self.db.read)
        .await
        .map_err(port)?;
        Ok(n as u64)
    }
}

/// A guarded UPDATE touched no row: the run is gone or in another status.
async fn miss(db: &Db, run: &ImportRun) -> RepoResult<()> {
    let exists: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM ai_import_runs WHERE id = ? AND organizer_id = ?")
            .bind(run.id.to_string())
            .bind(run.organizer_id.to_string())
            .fetch_optional(&db.write)
            .await
            .map_err(port)?;
    Err(if exists.is_some() {
        bad_status()
    } else {
        RepoError::NotFound
    })
}

#[async_trait]
impl ImportApplyRepo for SqliteImportRunRepo {
    async fn apply(
        &self,
        run: &ImportRun,
        steps: Vec<ImportApplyStep>,
        photo_target: Option<domain::directory::VendorId>,
    ) -> RepoResult<()> {
        self.apply_draft(run, steps, photo_target, None).await
    }
    async fn apply_draft(
        &self,
        run: &ImportRun,
        steps: Vec<ImportApplyStep>,
        photo_target: Option<domain::directory::VendorId>,
        draft_revision: Option<i64>,
    ) -> RepoResult<()> {
        let now = run.applied_at.unwrap_or(run.created_at);
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let done = sqlx::query(
            "UPDATE ai_import_runs SET status = 'applied', applied_at = ? \
             WHERE id = ? AND organizer_id = ? AND status = 'proposed'",
        )
        .bind(to_text(now))
        .bind(run.id.to_string())
        .bind(run.organizer_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(port)?;
        if done.rows_affected() != 1 {
            drop(tx); // rollback
            return miss(&self.db, run).await;
        }
        let org = run.organizer_id;
        crate::directory_repo::scope_admin(&mut tx, org, run.sharing_group_id).await?;
        let draft: Option<(i64, String)> =
            sqlx::query_as("SELECT revision,state_json FROM ai_import_drafts WHERE run_id=?")
                .bind(run.id.to_string())
                .fetch_optional(&mut *tx)
                .await
                .map_err(port)?;
        if let Some((revision, json)) = draft {
            let state: domain::ai_import::DraftState = serde_json::from_str(&json).map_err(port)?;
            if draft_revision != Some(revision)
                || state.pending.is_some()
                || now >= run.created_at + std::time::Duration::from_secs(86400)
            {
                return Err(RepoError::UniqueViolation);
            }
        } else if draft_revision.is_some() {
            return Err(RepoError::UniqueViolation);
        }
        let mut photo_target_written = false;
        for step in steps {
            match step {
                ImportApplyStep::CreateDeliveryAddress(a) => {
                    if run.sharing_group_id.is_some() || a.organizer_id != org {
                        return Err(RepoError::NotFound);
                    }
                    crate::delivery_address_repo::create_tx(&mut tx, &a, now).await?;
                }
                ImportApplyStep::CreateRoster(r) => {
                    if r.sharing_group_id != run.sharing_group_id || r.organizer_id() != org {
                        return Err(RepoError::NotFound);
                    }
                    create_roster_tx(&mut tx, r, now).await?;
                }
                ImportApplyStep::ModifyRoster { id, mutation } => {
                    let scope: Option<(Option<String>,)> =
                        sqlx::query_as("SELECT sharing_group_id FROM rosters WHERE id=?")
                            .bind(id.to_string())
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(port)?;
                    if scope != Some((run.sharing_group_id.map(|g| g.to_string()),)) {
                        return Err(RepoError::NotFound);
                    }
                    modify_roster_tx(&mut tx, org, id, now, mutation).await?;
                }
                ImportApplyStep::CreateVendor(v) => {
                    if v.sharing_group_id != run.sharing_group_id || v.organizer_id() != org {
                        return Err(RepoError::NotFound);
                    }
                    let id = v.id();
                    create_vendor_tx(&mut tx, v, now).await?;
                    photo_target_written |= photo_target == Some(id);
                }
                ImportApplyStep::ModifyVendor { id, mutation } => {
                    let scope: Option<(Option<String>,)> =
                        sqlx::query_as("SELECT sharing_group_id FROM vendors WHERE id=?")
                            .bind(id.to_string())
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(port)?;
                    if scope != Some((run.sharing_group_id.map(|g| g.to_string()),)) {
                        return Err(RepoError::NotFound);
                    }
                    modify_vendor_tx(&mut tx, org, id, now, mutation).await?;
                    photo_target_written |= photo_target == Some(id);
                }
            }
        }
        if let Some(target) = photo_target {
            if !photo_target_written {
                return Err(DomainError::invalid(
                    "savePhoto",
                    "Photo target must be an accepted vendor in this scope",
                )
                .into());
            }
            let photo: Option<(Vec<u8>, u32, u32, String)> = sqlx::query_as(
                "SELECT data,width,height,expires_at FROM ai_import_photos WHERE run_id=?",
            )
            .bind(run.id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(port)?;
            let Some((data, width, height, expiry)) = photo else {
                return Err(DomainError::invalid(
                    "savePhoto",
                    "Pending photo missing; uncheck savePhoto or retry recognition",
                )
                .into());
            };
            if from_text(&expiry).map_err(port)? <= now {
                return Err(DomainError::invalid(
                    "savePhoto",
                    "Pending photo expired; uncheck savePhoto or retry recognition",
                )
                .into());
            }
            crate::directory_repo::put_image_tx(
                &mut tx,
                org,
                target,
                application::ports::NormalizedVendorImage {
                    data,
                    width,
                    height,
                },
                now,
            )
            .await?;
        }
        sqlx::query("DELETE FROM ai_import_photos WHERE run_id=?")
            .bind(run.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(port)?;
        tx.commit().await.map_err(port)?;
        Ok(())
    }
}
