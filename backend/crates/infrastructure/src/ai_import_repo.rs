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
}

macro_rules! columns {
    () => {
        "id, organizer_id, mode_hint, target_roster_id, target_vendor_id, instruction, status, \
         proposal_json, summary, model, input_tokens, output_tokens, error, tool_calls_json, created_at, \
         finished_at, applied_at"
    };
}

fn opt_ts(s: Option<String>) -> PortResult<Option<SystemTime>> {
    s.map(|s| from_text(&s).map_err(port)).transpose()
}

impl RunRow {
    fn into_run(self) -> PortResult<ImportRun> {
        Ok(ImportRun {
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
    async fn create(&self, run: &ImportRun) -> PortResult<()> {
        let proposal = serde_json::to_string(&run.proposal).map_err(port)?;
        sqlx::query(concat!(
            "INSERT INTO ai_import_runs (",
            columns!(),
            ") VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
        .execute(&self.db.write)
        .await
        .map_err(port)?;
        Ok(())
    }

    async fn find(&self, org: OrganizerId, id: ImportRunId) -> PortResult<Option<ImportRun>> {
        sqlx::query_as::<_, RunRow>(concat!(
            "SELECT ",
            columns!(),
            " FROM ai_import_runs WHERE id = ? AND organizer_id = ?"
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
        .execute(&self.db.write)
        .await
        .map_err(port)?;
        if done.rows_affected() == 1 {
            return Ok(());
        }
        miss(&self.db, run).await
    }

    async fn count_since(&self, org: OrganizerId, since: SystemTime) -> PortResult<u64> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM ai_import_runs WHERE organizer_id = ? AND created_at >= ?",
        )
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
    async fn apply(&self, run: &ImportRun, steps: Vec<ImportApplyStep>) -> RepoResult<()> {
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
        for step in steps {
            match step {
                ImportApplyStep::CreateRoster(r) => {
                    create_roster_tx(&mut tx, r, now).await?;
                }
                ImportApplyStep::ModifyRoster { id, mutation } => {
                    modify_roster_tx(&mut tx, org, id, now, mutation).await?;
                }
                ImportApplyStep::CreateVendor(v) => {
                    create_vendor_tx(&mut tx, v, now).await?;
                }
                ImportApplyStep::ModifyVendor { id, mutation } => {
                    modify_vendor_tx(&mut tx, org, id, now, mutation).await?;
                }
            }
        }
        tx.commit().await.map_err(port)?;
        Ok(())
    }
}
