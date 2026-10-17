use std::time::SystemTime;

use super::Proposal;
use crate::directory::{RosterId, VendorId};
use crate::identity::OrganizerId;
use crate::shared::{DomainError, Id};

pub struct ImportRunMarker;
pub type ImportRunId = Id<ImportRunMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportRunStatus {
    Running,
    Proposed,
    Applied,
    Discarded,
    Failed,
}

impl ImportRunStatus {
    /// Value stored in `ai_import_runs.status`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Proposed => "proposed",
            Self::Applied => "applied",
            Self::Discarded => "discarded",
            Self::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        Ok(match s {
            "running" => Self::Running,
            "proposed" => Self::Proposed,
            "applied" => Self::Applied,
            "discarded" => Self::Discarded,
            "failed" => Self::Failed,
            _ => return Err(DomainError::invalid("status", "unknown import run status")),
        })
    }
}

/// The user's hint of what the photo contains (`ai_import_runs.mode_hint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    Roster,
    Vendor,
}

impl ImportMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Roster => "roster",
            Self::Vendor => "vendor",
        }
    }
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "roster" => Ok(Self::Roster),
            "vendor" => Ok(Self::Vendor),
            _ => Err(DomainError::invalid("modeHint", "必須是 roster 或 vendor")),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// One AI import attempt. Fields are public (plain data); state changes go
/// through the transition methods, which enforce the state machine
/// `Running -> Proposed -> Applied | Discarded`, `Running -> Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRun {
    pub id: ImportRunId,
    pub organizer_id: OrganizerId,
    pub mode_hint: Option<ImportMode>,
    pub target_roster_id: Option<RosterId>,
    pub target_vendor_id: Option<VendorId>,
    pub instruction: Option<String>,
    pub status: ImportRunStatus,
    pub proposal: Proposal,
    pub summary: Option<String>,
    pub model: String,
    pub usage: Usage,
    pub error: Option<String>,
    /// JSON array of the tool calls the agent made (`{name, args}`), for debugging.
    pub tool_calls: Option<String>,
    pub created_at: SystemTime,
    pub finished_at: Option<SystemTime>,
    pub applied_at: Option<SystemTime>,
}

impl ImportRun {
    /// A new run in `Running` state.
    pub fn start(
        organizer_id: OrganizerId,
        mode_hint: Option<ImportMode>,
        target_roster_id: Option<RosterId>,
        target_vendor_id: Option<VendorId>,
        instruction: Option<String>,
        model: String,
        now: SystemTime,
    ) -> Self {
        Self {
            id: ImportRunId::new(),
            organizer_id,
            mode_hint,
            target_roster_id,
            target_vendor_id,
            instruction,
            status: ImportRunStatus::Running,
            proposal: Proposal::default(),
            summary: None,
            model,
            usage: Usage::default(),
            error: None,
            tool_calls: None,
            created_at: now,
            finished_at: None,
            applied_at: None,
        }
    }

    fn require(&self, from: ImportRunStatus) -> Result<(), DomainError> {
        if self.status == from {
            Ok(())
        } else {
            Err(DomainError::invalid(
                "status",
                format!("目前狀態為 {}，不能執行此操作", self.status.as_str()),
            ))
        }
    }

    /// `Running -> Proposed`.
    pub fn propose(
        &mut self,
        proposal: Proposal,
        summary: String,
        usage: Usage,
        now: SystemTime,
    ) -> Result<(), DomainError> {
        self.require(ImportRunStatus::Running)?;
        self.status = ImportRunStatus::Proposed;
        self.proposal = proposal;
        self.summary = Some(summary);
        self.usage = usage;
        self.finished_at = Some(now);
        Ok(())
    }

    /// `Running -> Failed`.
    pub fn fail(
        &mut self,
        error: String,
        usage: Usage,
        now: SystemTime,
    ) -> Result<(), DomainError> {
        self.require(ImportRunStatus::Running)?;
        self.status = ImportRunStatus::Failed;
        self.error = Some(error);
        self.usage = usage;
        self.finished_at = Some(now);
        Ok(())
    }

    /// `Proposed -> Applied`.
    pub fn apply(&mut self, now: SystemTime) -> Result<(), DomainError> {
        self.require(ImportRunStatus::Proposed)?;
        self.status = ImportRunStatus::Applied;
        self.applied_at = Some(now);
        Ok(())
    }

    /// `Proposed -> Discarded`.
    pub fn discard(&mut self) -> Result<(), DomainError> {
        self.require(ImportRunStatus::Proposed)?;
        self.status = ImportRunStatus::Discarded;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn run() -> ImportRun {
        ImportRun::start(
            OrganizerId::new(),
            Some(ImportMode::Vendor),
            None,
            None,
            None,
            "m".into(),
            SystemTime::UNIX_EPOCH,
        )
    }
    fn t(s: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(s)
    }
    fn is_status_err<T: std::fmt::Debug>(r: Result<T, DomainError>) -> bool {
        matches!(
            r,
            Err(DomainError::Invalid {
                field: "status",
                ..
            })
        )
    }

    #[test]
    fn status_strings_round_trip() {
        for s in [
            ImportRunStatus::Running,
            ImportRunStatus::Proposed,
            ImportRunStatus::Applied,
            ImportRunStatus::Discarded,
            ImportRunStatus::Failed,
        ] {
            assert_eq!(ImportRunStatus::parse(s.as_str()).unwrap(), s);
        }
        assert!(ImportRunStatus::parse("x").is_err());
        assert_eq!(ImportMode::parse("roster").unwrap(), ImportMode::Roster);
        assert!(ImportMode::parse("x").is_err());
    }

    #[test]
    fn happy_path_apply() {
        let mut r = run();
        assert!(is_status_err(r.apply(t(1))));
        assert!(is_status_err(r.discard()));
        let u = Usage {
            input_tokens: 5,
            output_tokens: 7,
        };
        r.propose(Proposal::default(), "摘要".into(), u, t(2))
            .unwrap();
        assert_eq!(r.status, ImportRunStatus::Proposed);
        assert_eq!(r.usage, u);
        assert_eq!(r.finished_at, Some(t(2)));
        assert!(is_status_err(r.propose(
            Proposal::default(),
            "x".into(),
            u,
            t(3)
        )));
        assert!(is_status_err(r.fail("e".into(), u, t(3))));
        r.apply(t(4)).unwrap();
        assert_eq!(r.applied_at, Some(t(4)));
        assert!(is_status_err(r.apply(t(5))));
        assert!(is_status_err(r.discard()));
    }

    #[test]
    fn fail_and_discard() {
        let mut r = run();
        r.fail("boom".into(), Usage::default(), t(1)).unwrap();
        assert_eq!(r.status, ImportRunStatus::Failed);
        assert_eq!(r.error.as_deref(), Some("boom"));
        assert!(is_status_err(r.apply(t(2))));
        assert!(is_status_err(r.discard()));

        let mut r = run();
        r.propose(Proposal::default(), "s".into(), Usage::default(), t(1))
            .unwrap();
        r.discard().unwrap();
        assert_eq!(r.status, ImportRunStatus::Discarded);
        assert!(is_status_err(r.apply(t(2))));
    }
}
