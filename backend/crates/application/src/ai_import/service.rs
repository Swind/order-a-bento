//! `AiImportService`: start / get / apply / discard an import run.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use domain::ai_import::{ImportMode, ImportRun, ImportRunId, ImportRunStatus, TargetRef, Usage};
use domain::directory::{
    MemberInput, MenuItemInput, Roster, RosterId, Vendor, VendorId, VendorInfoPatch,
};
use domain::identity::OrganizerId;
use domain::shared::text::{
    ADDRESS_MAX_LEN, CATEGORY_MAX_LEN, DESCRIPTION_MAX_LEN, NOTE_MAX_LEN, PHONE_MAX_LEN,
    optional_text,
};
use domain::shared::{DomainError, ItemName, Money, NameKey, PersonName, RosterName, VendorName};
use serde::{Deserialize, Serialize};

use super::{AgentInput, AiError, ImageInput, ImportAgent, ImportToolContext};
use crate::ports::{
    Clock, ImportApplyRepo, ImportApplyStep, ImportRunRepo, PortError, RepoError, RosterRepo,
    VendorRepo,
};

/// What the API returns for a run (the aggregate itself).
pub type ImportRunView = ImportRun;

/// Max length of the user's free-text instruction.
pub const INSTRUCTION_MAX_LEN: usize = 500;

#[derive(Debug, Clone)]
pub struct AiImportConfig {
    /// Model name recorded on a run before the agent reports the real one.
    pub model: String,
    /// `AI_DAILY_QUOTA_PER_USER`: runs created in the last 24 h per organizer.
    pub daily_quota: u32,
    /// Total time for the agent including the retry (design: 90 s).
    pub timeout: Duration,
    /// Pause before the single retry of a retryable failure (design: 2 s).
    pub retry_delay: Duration,
}

impl AiImportConfig {
    pub fn new(model: impl Into<String>, daily_quota: u32) -> Self {
        Self {
            model: model.into(),
            daily_quota,
            timeout: Duration::from_secs(90),
            retry_delay: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StartImportCmd {
    pub pending_photo: Option<crate::ports::NormalizedVendorImage>,
    pub sharing_group_id: Option<domain::sharing::SharingGroupId>,
    pub image: ImageInput,
    pub instruction: Option<String>,
    pub mode: ImportMode,
    pub target_roster_id: Option<RosterId>,
    pub target_vendor_id: Option<VendorId>,
}

// ---------------------------------------------------------------------------
// Apply command: what the confirmation screen sends back.
// ---------------------------------------------------------------------------

/// A confirmed (possibly edited) change to one roster or vendor. JSON:
/// `{ "type": "roster" | "vendor", ... }`, see the variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AcceptedChange {
    DeliveryAddress(AcceptedDeliveryAddress),
    Roster(AcceptedRoster),
    Vendor(AcceptedVendor),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedDeliveryAddress {
    pub name: String,
    pub address: String,
}

/// `{ "type":"roster", "roster": {"kind":"existing","existingId":".."} | {"kind":"new","name":".."},
///    "members": [ { "name", "note" } ] }`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedRoster {
    pub roster: TargetRef<RosterId>,
    pub members: Vec<AcceptedMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedMember {
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
}

/// `{ "type":"vendor", "vendor": <TargetRef>, "info": { "name", "phone", "address", "note" } | null,
///    "menu": [ { "name", "price", "description", "category" } ] }`
///
/// `info` null/absent = leave the vendor info alone (required for a `new`
/// vendor, whose name comes from `info.name`; for an existing vendor `name` is
/// ignored, vendors are never renamed by an import).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedVendor {
    pub vendor: TargetRef<VendorId>,
    #[serde(default)]
    pub info: Option<AcceptedVendorInfo>,
    #[serde(default)]
    pub menu: Vec<AcceptedMenuItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedVendorInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedMenuItem {
    pub name: String,
    /// Required (the user fills in `price_missing` items before applying).
    #[serde(default)]
    pub price: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ApplyCmd {
    pub save_photo: Option<bool>,
    pub accepted: Vec<AcceptedChange>,
}

/// Aggregates touched by an apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyResult {
    pub delivery_address_ids: Vec<domain::directory::DeliveryAddressId>,
    pub roster_ids: Vec<RosterId>,
    pub vendor_ids: Vec<VendorId>,
}

/// One invalid entry of an apply request. `path` points into the request,
/// e.g. `accepted[0].menu[2].price`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeError {
    pub path: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AiImportError {
    /// Unknown id, or an id owned by another organizer.
    #[error("not found")]
    NotFound,
    #[error("daily AI import quota exceeded")]
    QuotaExceeded,
    /// The agent failed; the run was persisted as `failed`.
    #[error("{error}")]
    Agent { run_id: ImportRunId, error: AiError },
    /// The run is not in the state the operation needs (`Proposed`).
    #[error("invalid run status: {0}")]
    InvalidState(String),
    /// Apply: the confirmed changes did not validate (API: 422 with the list).
    #[error("{} invalid change(s)", .0.len())]
    InvalidChanges(Vec<ChangeError>),
    #[error(transparent)]
    Domain(DomainError),
    #[error("conflict")]
    Conflict,
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<PortError> for AiImportError {
    fn from(e: PortError) -> Self {
        Self::Internal(e.0)
    }
}

impl From<DomainError> for AiImportError {
    fn from(e: DomainError) -> Self {
        match e {
            DomainError::NotFound { .. } => Self::NotFound,
            DomainError::Invalid {
                field: "status",
                reason,
            } => Self::InvalidState(reason),
            other => Self::Domain(other),
        }
    }
}

impl From<RepoError> for AiImportError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound | RepoError::Forbidden => Self::NotFound,
            RepoError::Domain(d) => d.into(),
            RepoError::UniqueViolation => Self::Conflict,
            RepoError::Port(p) => p.into(),
        }
    }
}

pub struct AiImportService {
    pub runs: Arc<dyn ImportRunRepo>,
    pub apply_repo: Arc<dyn ImportApplyRepo>,
    pub rosters: Arc<dyn RosterRepo>,
    pub vendors: Arc<dyn VendorRepo>,
    pub agent: Arc<dyn ImportAgent>,
    pub clock: Arc<dyn Clock>,
    pub config: AiImportConfig,
}

impl AiImportService {
    /// Run the agent on the photo and return the run in `Proposed` state.
    pub async fn start(
        &self,
        org: OrganizerId,
        cmd: StartImportCmd,
    ) -> Result<ImportRunView, AiImportError> {
        if cmd.pending_photo.is_some() && cmd.mode != ImportMode::Vendor {
            return Err(
                DomainError::invalid("savePhoto", "Only vendor imports can save a photo").into(),
            );
        }
        self.rosters
            .check_scope(org, cmd.sharing_group_id, true)
            .await?;
        if cmd.sharing_group_id.is_some() && cmd.mode == ImportMode::DeliveryAddress {
            return Err(
                DomainError::invalid("sharingGroupId", "delivery addresses are private").into(),
            );
        }
        let instruction = optional_text(
            "instruction",
            cmd.instruction.as_deref(),
            INSTRUCTION_MAX_LEN,
        )?;
        if let Some(id) = cmd.target_roster_id
            && self.rosters.find(org, id).await?.is_none_or(|s| {
                !s.value.can_manage || s.value.sharing_group_id != cmd.sharing_group_id
            })
        {
            return Err(AiImportError::NotFound);
        }
        if let Some(id) = cmd.target_vendor_id
            && self.vendors.find(org, id).await?.is_none_or(|s| {
                !s.value.can_manage || s.value.sharing_group_id != cmd.sharing_group_id
            })
        {
            return Err(AiImportError::NotFound);
        }

        let now = self.clock.now();
        let since = now
            .checked_sub(Duration::from_secs(24 * 3600))
            .unwrap_or(std::time::UNIX_EPOCH);
        if self.runs.count_since(org, since).await? >= u64::from(self.config.daily_quota) {
            return Err(AiImportError::QuotaExceeded);
        }

        let mut run = ImportRun::start(
            org,
            cmd.mode,
            cmd.target_roster_id,
            cmd.target_vendor_id,
            instruction.clone(),
            self.config.model.clone(),
            now,
        );
        run.sharing_group_id = cmd.sharing_group_id;
        if cmd.pending_photo.is_some() {
            run.pending_photo_expires_at = Some(now + Duration::from_secs(24 * 3600));
        }
        let image = match &cmd.pending_photo {
            Some(photo) => ImageInput {
                bytes: photo.data.clone(),
                media_type: "image/jpeg".into(),
            },
            None => cmd.image,
        };
        self.runs.create(&run, cmd.pending_photo).await?;

        let mut ctx = ImportToolContext::new(
            org,
            run.id,
            self.rosters.clone(),
            self.vendors.clone(),
            self.clock.clone(),
        );
        ctx.sharing_group_id = cmd.sharing_group_id;
        let ctx = Arc::new(ctx);
        let input = AgentInput {
            image,
            instruction,
            mode: cmd.mode,
            target_roster_id: cmd.target_roster_id,
            target_vendor_id: cmd.target_vendor_id,
        };

        match self.run_agent(&ctx, input).await {
            Ok(outcome) => {
                run.model = outcome.model;
                // Includes failed earlier attempts when the agent reports per turn.
                let usage = match ctx.usage() {
                    u if u != Usage::default() => u,
                    _ => outcome.usage,
                };
                run.tool_calls = ctx.tool_calls_json();
                run.propose(
                    ctx.snapshot().await,
                    outcome.summary,
                    usage,
                    self.clock.now(),
                )?;
                self.runs.save(&run, ImportRunStatus::Running).await?;
                // The agent can run for a long time; do not return shared
                // proposal data after the initiating actor was revoked.
                self.rosters
                    .check_scope(org, run.sharing_group_id, true)
                    .await?;
                self.hide_unavailable_photo(&mut run);
                Ok(run)
            }
            Err(e) => {
                tracing::warn!(run = %run.id, error = %e, "AI import failed");
                run.tool_calls = ctx.tool_calls_json();
                run.fail(e.to_string(), ctx.usage(), self.clock.now())?;
                self.runs.save(&run, ImportRunStatus::Running).await?;
                Err(AiImportError::Agent {
                    run_id: run.id,
                    error: e,
                })
            }
        }
    }

    /// The agent call with one retry of a retryable failure, all under the timeout.
    async fn run_agent(
        &self,
        ctx: &Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<super::AgentOutcome, AiError> {
        let attempts = async {
            let mut first = true;
            loop {
                match self.agent.run(ctx.clone(), input.clone()).await {
                    Err(AiError::Unavailable {
                        retryable: true, ..
                    }) if first => {
                        first = false;
                        tokio::time::sleep(self.config.retry_delay).await;
                        ctx.reset().await;
                    }
                    other => return other,
                }
            }
        };
        match tokio::time::timeout(self.config.timeout, attempts).await {
            Ok(r) => r,
            Err(_) => Err(AiError::Timeout),
        }
    }

    pub async fn get(
        &self,
        org: OrganizerId,
        id: ImportRunId,
    ) -> Result<ImportRunView, AiImportError> {
        let mut run = self
            .runs
            .find(org, id)
            .await?
            .ok_or(AiImportError::NotFound)?;
        self.rosters
            .check_scope(org, run.sharing_group_id, false)
            .await?;
        self.hide_unavailable_photo(&mut run);
        Ok(run)
    }

    fn hide_unavailable_photo(&self, run: &mut ImportRun) {
        if !matches!(
            run.status,
            ImportRunStatus::Running | ImportRunStatus::Proposed
        ) || run
            .pending_photo_expires_at
            .is_some_and(|t| t <= self.clock.now())
        {
            run.pending_photo_expires_at = None;
        }
    }

    pub async fn discard(
        &self,
        org: OrganizerId,
        id: ImportRunId,
    ) -> Result<ImportRunView, AiImportError> {
        let mut run = self.get(org, id).await?;
        run.discard()?;
        self.runs.save(&run, ImportRunStatus::Proposed).await?;
        Ok(run)
    }

    /// Write the user-confirmed changes in one transaction and mark the run
    /// applied. The proposal's own contents are not trusted: everything in
    /// `cmd` is validated with the domain value objects and diffed against the
    /// data as it is now (same-name overwrite, blank never overwrites).
    pub async fn apply(
        &self,
        org: OrganizerId,
        id: ImportRunId,
        cmd: ApplyCmd,
    ) -> Result<ApplyResult, AiImportError> {
        let mut run = self.get(org, id).await?;
        self.rosters
            .check_scope(org, run.sharing_group_id, true)
            .await?;
        if run.status != ImportRunStatus::Proposed {
            // Same error the domain transition gives.
            run.apply(self.clock.now())?;
        }

        let mut errors = Vec::new();
        let mut prepared = Vec::new();
        let mut seen_targets = HashSet::new();
        for (ci, change) in cmd.accepted.iter().enumerate() {
            let p = format!("accepted[{ci}]");
            match change {
                AcceptedChange::DeliveryAddress(a) => {
                    if run.sharing_group_id.is_some() {
                        return Err(DomainError::invalid(
                            "accepted",
                            "delivery addresses are private",
                        )
                        .into());
                    }
                    if !seen_targets.insert(format!(
                        "delivery-address:{}",
                        NameKey::from(a.name.as_str())
                    )) {
                        errors.push(err(format!("{p}.name"), "送餐地點重複"));
                    }
                    match domain::directory::DeliveryAddress::new(org, &a.name, &a.address) {
                        Ok(address) => prepared.push(Prepared::DeliveryAddress(address)),
                        Err(e) => errors.push(err(
                            format!(
                                "{p}.{}",
                                if matches!(
                                    e,
                                    DomainError::Invalid {
                                        field: "address",
                                        ..
                                    }
                                ) {
                                    "address"
                                } else {
                                    "name"
                                }
                            ),
                            reason(&e),
                        )),
                    }
                }
                AcceptedChange::Roster(r) => {
                    let key = match &r.roster {
                        TargetRef::Existing { existing_id } => format!("roster:{existing_id}"),
                        TargetRef::New { name } => {
                            format!("roster-new:{}", NameKey::from(name.as_str()))
                        }
                    };
                    if !seen_targets.insert(key) {
                        errors.push(err(format!("{p}.roster"), "同一個目標在 accepted 中重複"));
                    }
                    if let Some(v) = validate_roster(&p, r, &mut errors) {
                        prepared.push(Prepared::Roster(v));
                    }
                }
                AcceptedChange::Vendor(v) => {
                    let key = match &v.vendor {
                        TargetRef::Existing { existing_id } => format!("vendor:{existing_id}"),
                        TargetRef::New { .. } => format!(
                            "vendor-new:{}",
                            NameKey::from(
                                v.info
                                    .as_ref()
                                    .and_then(|i| i.name.as_deref())
                                    .unwrap_or("")
                            )
                        ),
                    };
                    if !seen_targets.insert(key) {
                        errors.push(err(format!("{p}.vendor"), "同一個目標在 accepted 中重複"));
                    }
                    if let Some(v) = validate_vendor(&p, v, &mut errors) {
                        prepared.push(Prepared::Vendor(v));
                    }
                }
            }
        }

        // Targets must exist now (and belong to the organizer).
        let mut steps = Vec::new();
        let mut result = ApplyResult::default();
        for item in prepared {
            match item {
                Prepared::DeliveryAddress(a) => {
                    result.delivery_address_ids.push(a.id);
                    steps.push(ImportApplyStep::CreateDeliveryAddress(a));
                }
                Prepared::Roster(r) => match r.target {
                    ResolvedRoster::Existing(rid) => {
                        if self.rosters.find(org, rid).await?.is_none_or(|s| {
                            !s.value.can_manage || s.value.sharing_group_id != run.sharing_group_id
                        }) {
                            errors.push(err(format!("{}.roster", r.path), "找不到這份名單"));
                            continue;
                        }
                        let members = r.members;
                        result.roster_ids.push(rid);
                        steps.push(ImportApplyStep::ModifyRoster {
                            id: rid,
                            mutation: Box::new(move |roster| {
                                roster.upsert_members(members).map(Some)
                            }),
                        });
                    }
                    ResolvedRoster::New(name) => {
                        let mut roster = Roster::new(org, &name)?;
                        roster.sharing_group_id = run.sharing_group_id;
                        roster
                            .upsert_members(r.members)
                            .map_err(|e| limit_err(&r.path, e))?;
                        result.roster_ids.push(roster.id());
                        steps.push(ImportApplyStep::CreateRoster(roster));
                    }
                },
                Prepared::Vendor(v) => {
                    let existing = match v.target {
                        ResolvedVendor::Existing(vid) => {
                            if self.vendors.find(org, vid).await?.is_none_or(|s| {
                                !s.value.can_manage
                                    || s.value.sharing_group_id != run.sharing_group_id
                            }) {
                                errors.push(err(format!("{}.vendor", v.path), "找不到這間店家"));
                                continue;
                            }
                            Some(vid)
                        }
                        // A "new" vendor whose name now exists is that vendor.
                        ResolvedVendor::New(ref name) => {
                            self.vendors
                                .find_scoped_name(
                                    org,
                                    run.sharing_group_id,
                                    &VendorName::new(name)?.key(),
                                )
                                .await?
                        }
                    };
                    let patch = v.patch;
                    let items = v.items;
                    match existing {
                        Some(vid) => {
                            result.vendor_ids.push(vid);
                            steps.push(ImportApplyStep::ModifyVendor {
                                id: vid,
                                mutation: Box::new(move |vendor| {
                                    vendor.update_info(patch)?;
                                    if items.is_empty() {
                                        Ok(None)
                                    } else {
                                        vendor.upsert_menu_items(items).map(Some)
                                    }
                                }),
                            });
                        }
                        None => {
                            let ResolvedVendor::New(name) = v.target else {
                                unreachable!()
                            };
                            let mut vendor = Vendor::new(org, &name)?;
                            vendor.sharing_group_id = run.sharing_group_id;
                            vendor.update_info(patch)?;
                            if !items.is_empty() {
                                vendor
                                    .upsert_menu_items(items)
                                    .map_err(|e| limit_err(&v.path, e))?;
                            }
                            result.vendor_ids.push(vendor.id());
                            steps.push(ImportApplyStep::CreateVendor(vendor));
                        }
                    }
                }
            }
        }
        if !errors.is_empty() {
            // Report in request order (target existence is checked after field validation).
            errors.sort_by_key(|e| {
                e.path
                    .strip_prefix("accepted[")
                    .and_then(|r| r.split(']').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(usize::MAX)
            });
            return Err(AiImportError::InvalidChanges(errors));
        }

        let save_photo = cmd
            .save_photo
            .unwrap_or(run.pending_photo_expires_at.is_some());
        let photo_target = if save_photo {
            let proposal_vendors = run
                .proposal
                .targets
                .iter()
                .filter(|t| matches!(t, domain::ai_import::ProposalTarget::Vendor(_)))
                .count();
            if run.mode != Some(ImportMode::Vendor) || run.pending_photo_expires_at.is_none() {
                return Err(DomainError::invalid(
                    "savePhoto",
                    "No available pending photo; retry recognition or uncheck savePhoto",
                )
                .into());
            }
            if proposal_vendors != 1 || result.vendor_ids.len() != 1 {
                return Err(DomainError::invalid(
                    "savePhoto",
                    "Exactly one proposed and accepted vendor is required; uncheck savePhoto",
                )
                .into());
            }
            Some(result.vendor_ids[0])
        } else {
            None
        };
        run.apply(self.clock.now())?;
        self.apply_repo.apply(&run, steps, photo_target).await?;
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Validation of accepted changes
// ---------------------------------------------------------------------------

fn err(path: impl Into<String>, message: impl Into<String>) -> ChangeError {
    ChangeError {
        path: path.into(),
        message: message.into(),
    }
}

/// Limit violations found while building a new aggregate are reported against its path.
fn limit_err(path: &str, e: DomainError) -> AiImportError {
    match e {
        DomainError::LimitExceeded { .. }
        | DomainError::Invalid { .. }
        | DomainError::Duplicate { .. } => {
            AiImportError::InvalidChanges(vec![err(path, e.to_string())])
        }
        other => other.into(),
    }
}

fn reason(e: &DomainError) -> String {
    match e {
        DomainError::Invalid { reason, .. } => reason.clone(),
        other => other.to_string(),
    }
}

enum ResolvedRoster {
    Existing(RosterId),
    New(String),
}
enum ResolvedVendor {
    Existing(VendorId),
    New(String),
}
struct ValidRoster {
    path: String,
    target: ResolvedRoster,
    members: Vec<MemberInput>,
}
struct ValidVendor {
    path: String,
    target: ResolvedVendor,
    patch: VendorInfoPatch,
    items: Vec<MenuItemInput>,
}
enum Prepared {
    DeliveryAddress(domain::directory::DeliveryAddress),
    Roster(ValidRoster),
    Vendor(ValidVendor),
}

fn check_text(
    errors: &mut Vec<ChangeError>,
    path: String,
    raw: Option<&str>,
    field: &'static str,
    max: usize,
) -> Option<String> {
    match optional_text(field, raw, max) {
        Ok(v) => v,
        Err(e) => {
            errors.push(err(path, reason(&e)));
            None
        }
    }
}

fn validate_roster(
    p: &str,
    r: &AcceptedRoster,
    errors: &mut Vec<ChangeError>,
) -> Option<ValidRoster> {
    let before = errors.len();
    let target = match &r.roster {
        TargetRef::Existing { existing_id } => ResolvedRoster::Existing(*existing_id),
        TargetRef::New { name } => match RosterName::new(name) {
            Ok(n) => ResolvedRoster::New(n.to_string()),
            Err(e) => {
                errors.push(err(format!("{p}.roster.name"), reason(&e)));
                ResolvedRoster::New(String::new())
            }
        },
    };
    let mut seen = HashSet::new();
    let mut members = Vec::with_capacity(r.members.len());
    for (i, m) in r.members.iter().enumerate() {
        let mp = format!("{p}.members[{i}]");
        match PersonName::new(&m.name) {
            Err(e) => errors.push(err(format!("{mp}.name"), reason(&e))),
            Ok(n) => {
                if !seen.insert(n.key()) {
                    errors.push(err(
                        format!("{mp}.name"),
                        format!("「{n}」重複（視為同一人）"),
                    ));
                }
            }
        }
        let note = check_text(
            errors,
            format!("{mp}.note"),
            m.note.as_deref(),
            "note",
            NOTE_MAX_LEN,
        );
        members.push(MemberInput {
            name: m.name.clone(),
            note,
        });
    }
    (errors.len() == before).then(|| ValidRoster {
        path: p.to_string(),
        target,
        members,
    })
}

fn validate_vendor(
    p: &str,
    v: &AcceptedVendor,
    errors: &mut Vec<ChangeError>,
) -> Option<ValidVendor> {
    let before = errors.len();
    let info = v.info.clone().unwrap_or_default();
    let target = match &v.vendor {
        TargetRef::Existing { existing_id } => ResolvedVendor::Existing(*existing_id),
        TargetRef::New { .. } => match info.name.as_deref().map(VendorName::new) {
            Some(Ok(n)) => ResolvedVendor::New(n.to_string()),
            Some(Err(e)) => {
                errors.push(err(format!("{p}.info.name"), reason(&e)));
                ResolvedVendor::New(String::new())
            }
            None => {
                errors.push(err(format!("{p}.info.name"), "新店家需要店名"));
                ResolvedVendor::New(String::new())
            }
        },
    };
    let ip = format!("{p}.info");
    let patch = VendorInfoPatch {
        name: None,
        phone: check_text(
            errors,
            format!("{ip}.phone"),
            info.phone.as_deref(),
            "phone",
            PHONE_MAX_LEN,
        ),
        address: check_text(
            errors,
            format!("{ip}.address"),
            info.address.as_deref(),
            "address",
            ADDRESS_MAX_LEN,
        ),
        note: check_text(
            errors,
            format!("{ip}.note"),
            info.note.as_deref(),
            "note",
            NOTE_MAX_LEN,
        ),
    };
    let mut seen = HashSet::new();
    let mut items = Vec::with_capacity(v.menu.len());
    for (i, m) in v.menu.iter().enumerate() {
        let mp = format!("{p}.menu[{i}]");
        match ItemName::new(&m.name) {
            Err(e) => errors.push(err(format!("{mp}.name"), reason(&e))),
            Ok(n) => {
                if !seen.insert(n.key()) {
                    errors.push(err(
                        format!("{mp}.name"),
                        format!("「{n}」重複（視為同一品項）"),
                    ));
                }
            }
        }
        let price = match m.price {
            None => {
                errors.push(err(format!("{mp}.price"), "價格必填"));
                0
            }
            Some(pr) => {
                if let Err(e) = Money::try_new(pr) {
                    errors.push(err(format!("{mp}.price"), reason(&e)));
                }
                pr
            }
        };
        items.push(MenuItemInput {
            name: m.name.clone(),
            price,
            description: check_text(
                errors,
                format!("{mp}.description"),
                m.description.as_deref(),
                "description",
                DESCRIPTION_MAX_LEN,
            ),
            category: check_text(
                errors,
                format!("{mp}.category"),
                m.category.as_deref(),
                "category",
                CATEGORY_MAX_LEN,
            ),
        });
    }
    (errors.len() == before).then(|| ValidVendor {
        path: p.to_string(),
        target,
        patch,
        items,
    })
}
