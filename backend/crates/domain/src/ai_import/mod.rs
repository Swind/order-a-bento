//! AI import context: `ImportRun` aggregate and the `Proposal` an agent builds.
//!
//! Everything here is plain data (serde derive) so that a proposal can be
//! stored as JSON in `ai_import_runs.proposal_json`.

mod proposal;
mod run;

pub use proposal::{
    ChangeAction, Confidence, Issue, MenuItemSnapshot, Proposal, ProposalTarget, ProposedMember,
    ProposedMenuItem, ProposedVendorInfo, RosterTarget, TargetRef, VendorInfoSnapshot,
    VendorTarget,
};
pub use run::{ImportMode, ImportRun, ImportRunId, ImportRunMarker, ImportRunStatus, Usage};
