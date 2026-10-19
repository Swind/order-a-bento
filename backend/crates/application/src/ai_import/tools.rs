//! Business logic of the agent's six tools (design 9.3), independent of any
//! AI framework: the rig wrappers (T4.2) only (de)serialize and delegate here.
//!
//! * Tools only ever see the context organizer's data (all repo calls carry
//!   `ctx.organizer_id`).
//! * Business problems (unknown id, invalid value, limit) are returned as
//!   *values* in the tool output (`{ "error": ... }` / `errors: [...]`) so the
//!   model can read them and fix its call; `Err(ToolError)` is reserved for
//!   infrastructure failures.
//! * `propose_*` never touch real data: they diff the input against the
//!   current data and record the result in `ctx.proposal`.

use std::collections::{HashMap, HashSet};

use domain::ai_import::{
    ChangeAction, Confidence, Issue, MenuItemSnapshot, ProposalTarget, ProposedMember,
    ProposedMenuItem, ProposedVendorInfo, RosterTarget, TargetRef, VendorInfoSnapshot,
    VendorTarget,
};
use domain::directory::{Roster, RosterId, Vendor, VendorId};
use domain::shared::text::{
    ADDRESS_MAX_LEN, CATEGORY_MAX_LEN, DESCRIPTION_MAX_LEN, NOTE_MAX_LEN, PHONE_MAX_LEN,
    collapse_whitespace, optional_text,
};
use domain::shared::{ItemName, Money, NameKey, PersonName, RosterName, VendorName};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ImportToolContext;
use crate::ports::PortError;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeDeliveryAddressArgs {
    pub name: String,
    /// Full delivery address, including floor/room when visible. Never invent missing parts.
    pub address: String,
    pub confidence: ConfidenceArg,
    pub warnings: Vec<String>,
}

pub async fn propose_delivery_address(
    ctx: &ImportToolContext,
    args: ProposeDeliveryAddressArgs,
) -> Result<serde_json::Value, ToolError> {
    // Record only a proposal: the user must review and explicitly apply it.
    let mut proposal = ctx.proposal.lock().await;
    proposal.replace_target(ProposalTarget::DeliveryAddress(
        domain::ai_import::ProposedDeliveryAddress {
            name: args.name.trim().chars().take(500).collect(),
            address: args.address.trim().chars().take(1000).collect(),
            confidence: args.confidence.into(),
        },
    ));
    proposal.add_warnings(args.warnings);
    Ok(serde_json::json!({"proposed": true}))
}

/// Infrastructure failure inside a tool (database...). Not shown to the model.
#[derive(Debug, thiserror::Error)]
#[error("tool failed: {0}")]
pub struct ToolError(pub String);

impl From<PortError> for ToolError {
    fn from(e: PortError) -> Self {
        Self(e.0)
    }
}

// ===========================================================================
// Read tools
// ===========================================================================

/// Business-level failure returned to the model as a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolMessage {
    pub error: String,
}

fn not_found(what: &str) -> ToolMessage {
    ToolMessage {
        error: format!("{what} not found; use an id returned by a list tool"),
    }
}

fn parse_id<I: std::str::FromStr>(s: &str) -> Option<I> {
    s.trim().parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RosterListItem {
    pub id: String,
    pub name: String,
    pub member_count: u32,
}

pub async fn list_rosters(ctx: &ImportToolContext) -> Result<Vec<RosterListItem>, ToolError> {
    Ok(ctx
        .rosters
        .list(ctx.organizer_id)
        .await?
        .into_iter()
        .map(|r| RosterListItem {
            id: r.id.to_string(),
            name: r.name,
            member_count: r.member_count,
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetRosterArgs {
    pub roster_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MemberView {
    pub name: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RosterDetail {
    pub id: String,
    pub name: String,
    pub members: Vec<MemberView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum GetRosterOutput {
    Found(RosterDetail),
    Error(ToolMessage),
}

pub async fn get_roster(
    ctx: &ImportToolContext,
    args: GetRosterArgs,
) -> Result<GetRosterOutput, ToolError> {
    let Some(id) = parse_id::<RosterId>(&args.roster_id) else {
        return Ok(GetRosterOutput::Error(not_found("roster")));
    };
    Ok(match ctx.rosters.find(ctx.organizer_id, id).await? {
        None => GetRosterOutput::Error(not_found("roster")),
        Some(s) => GetRosterOutput::Found(RosterDetail {
            id: id.to_string(),
            name: s.value.name().to_string(),
            members: s
                .value
                .members()
                .iter()
                .map(|m| MemberView {
                    name: m.name.to_string(),
                    note: m.note.clone(),
                })
                .collect(),
        }),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorListItem {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub item_count: u32,
}

pub async fn list_vendors(ctx: &ImportToolContext) -> Result<Vec<VendorListItem>, ToolError> {
    Ok(ctx
        .vendors
        .list(ctx.organizer_id)
        .await?
        .into_iter()
        .map(|v| VendorListItem {
            id: v.id.to_string(),
            name: v.name,
            phone: v.phone,
            item_count: v.item_count,
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetVendorArgs {
    pub vendor_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MenuItemView {
    pub name: String,
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorDetail {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
    /// Active (non-archived) items only.
    pub menu: Vec<MenuItemView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum GetVendorOutput {
    Found(VendorDetail),
    Error(ToolMessage),
}

pub async fn get_vendor(
    ctx: &ImportToolContext,
    args: GetVendorArgs,
) -> Result<GetVendorOutput, ToolError> {
    let Some(id) = parse_id::<VendorId>(&args.vendor_id) else {
        return Ok(GetVendorOutput::Error(not_found("vendor")));
    };
    Ok(match ctx.vendors.find(ctx.organizer_id, id).await? {
        None => GetVendorOutput::Error(not_found("vendor")),
        Some(s) => {
            let v = &s.value;
            GetVendorOutput::Found(VendorDetail {
                id: id.to_string(),
                name: v.name().to_string(),
                phone: v.phone().map(Into::into),
                address: v.address().map(Into::into),
                note: v.note().map(Into::into),
                menu: v
                    .menu()
                    .iter()
                    .filter(|m| !m.archived)
                    .map(|m| MenuItemView {
                        name: m.name.to_string(),
                        price: m.price.get(),
                        description: m.description.clone(),
                        category: m.category.clone(),
                    })
                    .collect(),
            })
        }
    })
}

// ===========================================================================
// Propose tools: shared types
// ===========================================================================

/// The model's own confidence in a read value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceArg {
    High,
    #[default]
    Medium,
    Low,
}

impl From<ConfidenceArg> for Confidence {
    fn from(c: ConfidenceArg) -> Self {
        match c {
            ConfidenceArg::High => Confidence::High,
            ConfidenceArg::Medium => Confidence::Medium,
            ConfidenceArg::Low => Confidence::Low,
        }
    }
}

/// A problem the model should fix, in Traditional Chinese.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProposalError {
    /// Index into the input array (`members` / `menuItems`); null for
    /// target-level and info problems.
    pub index: Option<usize>,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PriceChange {
    pub name: String,
    pub from: i64,
    pub to: i64,
}

/// Result of a `propose_*` call.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposalDiffSummary {
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub errors: Vec<ProposalError>,
    /// `propose_vendor`: existing items whose price changes (old -> new).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub price_changes: Vec<PriceChange>,
    /// `propose_vendor`: what happens to the vendor info
    /// (`create` / `update` / `unchanged`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    /// Explanations such as "treated as the existing vendor".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Whether the proposal was recorded (false when the call was rejected as
    /// a whole; fix `errors` and call again).
    pub recorded: bool,
}

impl ProposalDiffSummary {
    fn err(&mut self, index: Option<usize>, field: impl Into<String>, message: impl Into<String>) {
        self.errors.push(ProposalError {
            index,
            field: field.into(),
            message: message.into(),
        });
    }
    fn rejected(mut self, field: &str, message: impl Into<String>) -> Self {
        self.err(None, field, message);
        self
    }
    fn count(&mut self, a: ChangeAction) {
        match a {
            ChangeAction::Create => self.created += 1,
            ChangeAction::Update => self.updated += 1,
            ChangeAction::Unchanged => self.unchanged += 1,
        }
    }
}

fn name_issue(raw: &str, max: usize) -> Issue {
    if collapse_whitespace(raw).is_empty() {
        Issue::NameEmpty
    } else {
        debug_assert!(collapse_whitespace(raw).chars().count() > max);
        Issue::NameTooLong
    }
}

/// Validate an optional text field. On failure return the trimmed raw value
/// together with the issue/error so the entry can still be shown.
fn text_field(
    raw: Option<&str>,
    field: &'static str,
    max: usize,
) -> Result<Option<String>, (Option<String>, String)> {
    optional_text(field, raw, max).map_err(|_| {
        let raw = raw
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let n = raw.as_deref().map_or(0, |s| s.chars().count());
        (
            raw,
            format!("{field} must be at most {max} characters (currently {n}); shorten it"),
        )
    })
}

// ===========================================================================
// propose_roster_members
// ===========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExistingRosterRef {
    pub roster_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewRosterRef {
    pub new_roster_name: String,
}

/// `{ rosterId }` or `{ newRosterName }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RosterTargetArg {
    Existing(ExistingRosterRef),
    New(NewRosterRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberArg {
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub confidence: ConfidenceArg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeRosterMembersArgs {
    pub target: RosterTargetArg,
    pub members: Vec<MemberArg>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

pub async fn propose_roster_members(
    ctx: &ImportToolContext,
    args: ProposeRosterMembersArgs,
) -> Result<ProposalDiffSummary, ToolError> {
    let mut out = ProposalDiffSummary::default();

    // --- resolve the target ------------------------------------------------
    let (target_ref, existing): (TargetRef<RosterId>, Option<Roster>) = match &args.target {
        RosterTargetArg::Existing(r) => {
            let Some(id) = parse_id::<RosterId>(&r.roster_id) else {
                return Ok(out.rejected(
                    "target.rosterId",
                    "rosterId is malformed; use an id returned by list_rosters",
                ));
            };
            match ctx.rosters.find(ctx.organizer_id, id).await? {
                None => {
                    return Ok(out.rejected(
                        "target.rosterId",
                        "Roster not found; use an id returned by list_rosters, or use newRosterName to create a new roster",
                    ));
                }
                Some(s) => (TargetRef::Existing { existing_id: id }, Some(s.value)),
            }
        }
        RosterTargetArg::New(n) => match RosterName::new(&n.new_roster_name) {
            Ok(name) => (
                TargetRef::New {
                    name: name.to_string(),
                },
                None,
            ),
            Err(_) => {
                return Ok(out.rejected(
                    "target.newRosterName",
                    format!(
                        "newRosterName must be 1–{} characters; use 「新名單」 when the photo has no title",
                        RosterName::MAX_LEN
                    ),
                ));
            }
        },
    };

    // --- limit -------------------------------------------------------------
    let existing_members: HashMap<NameKey, &domain::directory::RosterMember> = existing
        .as_ref()
        .map(|r| r.members().iter().map(|m| (m.name.key(), m)).collect())
        .unwrap_or_default();
    let new_keys: HashSet<NameKey> = args
        .members
        .iter()
        .filter_map(|m| PersonName::new(&m.name).ok())
        .map(|n| n.key())
        .filter(|k| !existing_members.contains_key(k))
        .collect();
    if existing_members.len() + new_keys.len() > Roster::MAX_MEMBERS {
        return Ok(out.rejected(
            "members",
            format!(
                "A roster holds at most {} members ({} exist, adding {} would exceed that); reduce the number of members",
                Roster::MAX_MEMBERS,
                existing_members.len(),
                new_keys.len()
            ),
        ));
    }
    if args.members.is_empty() {
        return Ok(out.rejected(
            "members",
            "members is empty; do not call this tool when the photo has no people",
        ));
    }

    // --- per member --------------------------------------------------------
    let mut seen: HashMap<NameKey, usize> = HashMap::new();
    let mut proposed = Vec::with_capacity(args.members.len());
    for (i, m) in args.members.iter().enumerate() {
        let mut issues = Vec::new();
        let note = match text_field(m.note.as_deref(), "note", NOTE_MAX_LEN) {
            Ok(n) => n,
            Err((raw, msg)) => {
                issues.push(Issue::NoteTooLong);
                out.err(Some(i), "note", msg);
                raw
            }
        };
        let (action, name, before_note, note) = match PersonName::new(&m.name) {
            Err(_) => {
                let issue = name_issue(&m.name, PersonName::MAX_LEN);
                issues.push(issue);
                let msg = if issue == Issue::NameEmpty {
                    "name must not be empty; fill in the name or remove this entry".to_string()
                } else {
                    format!(
                        "name must be at most {} characters; keep only the person's name and put the rest in note",
                        PersonName::MAX_LEN
                    )
                };
                out.err(Some(i), "name", msg);
                (
                    ChangeAction::Create,
                    collapse_whitespace(&m.name),
                    None,
                    note,
                )
            }
            Ok(name) => {
                let key = name.key();
                if let Some(first) = seen.insert(key.clone(), i) {
                    issues.push(Issue::DuplicateInInput);
                    out.err(
                        Some(i),
                        "name",
                        format!(
                            "name 「{name}」 duplicates entry {} (treated as the same person); list it once",
                            first + 1
                        ),
                    );
                }
                match existing_members.get(&key) {
                    Some(em) => {
                        let changed = note
                            .as_deref()
                            .is_some_and(|n| em.note.as_deref() != Some(n));
                        if changed {
                            (
                                ChangeAction::Update,
                                em.name.to_string(),
                                em.note.clone(),
                                note,
                            )
                        } else {
                            // blank input never overwrites: the result is the existing note
                            let kept = if issues.contains(&Issue::NoteTooLong) {
                                note
                            } else {
                                em.note.clone()
                            };
                            (
                                ChangeAction::Unchanged,
                                em.name.to_string(),
                                em.note.clone(),
                                kept,
                            )
                        }
                    }
                    None => (ChangeAction::Create, name.to_string(), None, note),
                }
            }
        };
        if issues.is_empty() {
            out.count(action);
        }
        proposed.push(ProposedMember {
            action,
            name,
            before_note,
            note,
            confidence: m.confidence.into(),
            issues,
        });
    }

    // --- record ------------------------------------------------------------
    let mut p = ctx.proposal.lock().await;
    p.replace_target(ProposalTarget::Roster(RosterTarget {
        roster: target_ref,
        members: proposed,
    }));
    p.add_warnings(args.warnings);
    out.recorded = true;
    Ok(out)
}

// ===========================================================================
// propose_vendor
// ===========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExistingVendorRef {
    pub vendor_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum NewLiteral {
    #[serde(rename = "new")]
    New,
}

/// `{ vendorId }` or the string `"new"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum VendorTargetArg {
    Existing(ExistingVendorRef),
    New(NewLiteral),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VendorInfoArg {
    pub name: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MenuItemArg {
    pub name: String,
    /// New Taiwan dollars, integer. `null` when unreadable (never guess).
    #[serde(default)]
    pub price: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub confidence: ConfidenceArg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeVendorArgs {
    pub target: VendorTargetArg,
    pub info: VendorInfoArg,
    pub menu_items: Vec<MenuItemArg>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn snapshot(v: &Vendor) -> VendorInfoSnapshot {
    VendorInfoSnapshot {
        name: v.name().to_string(),
        phone: v.phone().map(Into::into),
        address: v.address().map(Into::into),
        note: v.note().map(Into::into),
    }
}

pub async fn propose_vendor(
    ctx: &ImportToolContext,
    args: ProposeVendorArgs,
) -> Result<ProposalDiffSummary, ToolError> {
    let mut out = ProposalDiffSummary::default();

    // --- info fields (validated first; invalid ones are dropped) ---------------
    let info_field =
        |out: &mut ProposalDiffSummary, raw: &Option<String>, field: &'static str, max: usize| {
            match text_field(raw.as_deref(), field, max) {
                Ok(v) => v,
                Err((_, msg)) => {
                    out.err(None, format!("info.{field}"), msg);
                    None
                }
            }
        };
    let phone = info_field(&mut out, &args.info.phone, "phone", PHONE_MAX_LEN);
    let address = info_field(&mut out, &args.info.address, "address", ADDRESS_MAX_LEN);
    let note = info_field(&mut out, &args.info.note, "note", NOTE_MAX_LEN);

    // --- resolve the target ------------------------------------------------
    let (target_ref, existing): (TargetRef<VendorId>, Option<Vendor>) = match &args.target {
        VendorTargetArg::Existing(r) => {
            let Some(id) = parse_id::<VendorId>(&r.vendor_id) else {
                return Ok(out.rejected(
                    "target.vendorId",
                    "vendorId is malformed; use an id returned by list_vendors",
                ));
            };
            match ctx.vendors.find(ctx.organizer_id, id).await? {
                None => {
                    return Ok(out.rejected(
                        "target.vendorId",
                        "Vendor not found; use an id returned by list_vendors, or use \"new\"",
                    ));
                }
                Some(s) => (TargetRef::Existing { existing_id: id }, Some(s.value)),
            }
        }
        VendorTargetArg::New(_) => {
            let name = match VendorName::new(&args.info.name) {
                Ok(n) => n,
                Err(_) => {
                    return Ok(out.rejected(
                        "info.name",
                        format!(
                            "info.name must be 1–{} characters; use 「（未知店家）」 when the vendor name cannot be found",
                            VendorName::MAX_LEN
                        ),
                    ));
                }
            };
            match ctx
                .vendors
                .find_id_by_name_key(ctx.organizer_id, &name.key())
                .await?
            {
                Some(id) => match ctx.vendors.find(ctx.organizer_id, id).await? {
                    Some(s) => {
                        out.notes.push(format!(
                            "A vendor named 「{}」 already exists, so that vendor is updated instead (no duplicate is created)",
                            s.value.name()
                        ));
                        (TargetRef::Existing { existing_id: id }, Some(s.value))
                    }
                    None => (
                        TargetRef::New {
                            name: name.to_string(),
                        },
                        None,
                    ),
                },
                None => (
                    TargetRef::New {
                        name: name.to_string(),
                    },
                    None,
                ),
            }
        }
    };

    // --- limit -------------------------------------------------------------
    let existing_items: HashMap<NameKey, &domain::directory::MenuItem> = existing
        .as_ref()
        .map(|v| v.menu().iter().map(|m| (m.name.key(), m)).collect())
        .unwrap_or_default();
    let new_keys: HashSet<NameKey> = args
        .menu_items
        .iter()
        .filter_map(|m| ItemName::new(&m.name).ok())
        .map(|n| n.key())
        .filter(|k| !existing_items.contains_key(k))
        .collect();
    if existing_items.len() + new_keys.len() > Vendor::MAX_MENU_ITEMS {
        return Ok(out.rejected(
            "menuItems",
            format!(
                "A menu holds at most {} items ({} exist, adding {} would exceed that); reduce the number of items",
                Vendor::MAX_MENU_ITEMS,
                existing_items.len(),
                new_keys.len()
            ),
        ));
    }

    // --- info diff -----------------------------------------------------------
    let (info_before, info_after, info_action) = match &existing {
        Some(v) => {
            let before = snapshot(v);
            if !args.info.name.trim().is_empty()
                && NameKey::from(args.info.name.as_str()) != v.name().key()
            {
                out.notes.push(format!(
                    "The vendor keeps its existing name 「{}」 and is not renamed; info.name is only used to identify the vendor",
                    v.name()
                ));
            }
            let after = VendorInfoSnapshot {
                name: before.name.clone(),
                phone: phone.or_else(|| before.phone.clone()),
                address: address.or_else(|| before.address.clone()),
                note: note.or_else(|| before.note.clone()),
            };
            let action = if after == before {
                ChangeAction::Unchanged
            } else {
                ChangeAction::Update
            };
            (Some(before), after, action)
        }
        None => {
            let TargetRef::New { name } = &target_ref else {
                unreachable!("no existing vendor implies a new target")
            };
            (
                None,
                VendorInfoSnapshot {
                    name: name.clone(),
                    phone,
                    address,
                    note,
                },
                ChangeAction::Create,
            )
        }
    };
    out.info = Some(info_action.as_str().to_string());

    // --- per item --------------------------------------------------------------
    let mut seen: HashMap<NameKey, usize> = HashMap::new();
    let mut proposed = Vec::with_capacity(args.menu_items.len());
    for (i, m) in args.menu_items.iter().enumerate() {
        let mut issues = Vec::new();
        let description =
            match text_field(m.description.as_deref(), "description", DESCRIPTION_MAX_LEN) {
                Ok(v) => v,
                Err((raw, msg)) => {
                    issues.push(Issue::DescriptionTooLong);
                    out.err(Some(i), "description", msg);
                    raw
                }
            };
        let category = match text_field(m.category.as_deref(), "category", CATEGORY_MAX_LEN) {
            Ok(v) => v,
            Err((raw, msg)) => {
                issues.push(Issue::CategoryTooLong);
                out.err(Some(i), "category", msg);
                raw
            }
        };
        let valid_price = match m.price {
            None => {
                // allowed: the user must fill it in before applying
                issues.push(Issue::PriceMissing);
                None
            }
            Some(p) => match Money::try_new(p) {
                Ok(m) => Some(m),
                Err(_) => {
                    issues.push(Issue::PriceOutOfRange);
                    out.err(
                        Some(i),
                        "price",
                        format!(
                            "price must be between {} and {} (got {p}); use null when it is unreadable, do not guess",
                            Money::MIN_PRICE,
                            Money::MAX_PRICE
                        ),
                    );
                    None
                }
            },
        };
        let mut fatal = issues.iter().any(|x| *x != Issue::PriceMissing);
        let (action, name, before, description, category) = match ItemName::new(&m.name) {
            Err(_) => {
                let issue = name_issue(&m.name, ItemName::MAX_LEN);
                issues.push(issue);
                fatal = true;
                let msg = if issue == Issue::NameEmpty {
                    "name must not be empty; fill in the item name or remove this entry".to_string()
                } else {
                    format!(
                        "name must be at most {} characters; put explanations in description",
                        ItemName::MAX_LEN
                    )
                };
                out.err(Some(i), "name", msg);
                (
                    ChangeAction::Create,
                    collapse_whitespace(&m.name),
                    None,
                    description,
                    category,
                )
            }
            Ok(name) => {
                let key = name.key();
                if let Some(first) = seen.insert(key.clone(), i) {
                    issues.push(Issue::DuplicateInInput);
                    fatal = true;
                    out.err(
                        Some(i),
                        "name",
                        format!(
                            "name 「{name}」 duplicates entry {} (treated as the same item); list it once or rename to tell them apart",
                            first + 1
                        ),
                    );
                }
                match existing_items.get(&key) {
                    Some(em) => {
                        let before = MenuItemSnapshot {
                            price: em.price.get(),
                            description: em.description.clone(),
                            category: em.category.clone(),
                        };
                        let changed = valid_price.is_some_and(|p| p != em.price)
                            || description
                                .as_deref()
                                .is_some_and(|d| em.description.as_deref() != Some(d))
                            || category
                                .as_deref()
                                .is_some_and(|c| em.category.as_deref() != Some(c))
                            || em.archived;
                        if let Some(p) = valid_price.filter(|p| *p != em.price) {
                            out.price_changes.push(PriceChange {
                                name: em.name.to_string(),
                                from: em.price.get(),
                                to: p.get(),
                            });
                        }
                        (
                            if changed {
                                ChangeAction::Update
                            } else {
                                ChangeAction::Unchanged
                            },
                            em.name.to_string(),
                            Some(before),
                            description.or_else(|| em.description.clone()),
                            category.or_else(|| em.category.clone()),
                        )
                    }
                    None => (
                        ChangeAction::Create,
                        name.to_string(),
                        None,
                        description,
                        category,
                    ),
                }
            }
        };
        if !fatal {
            out.count(action);
        }
        proposed.push(ProposedMenuItem {
            action,
            name,
            before,
            price: m.price,
            description,
            category,
            confidence: m.confidence.into(),
            issues,
        });
    }

    // --- record ------------------------------------------------------------
    let mut p = ctx.proposal.lock().await;
    p.replace_target(ProposalTarget::Vendor(VendorTarget {
        vendor: target_ref,
        info: ProposedVendorInfo {
            action: info_action,
            before: info_before,
            after: info_after,
        },
        menu: proposed,
    }));
    p.add_warnings(args.warnings);
    out.recorded = true;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn target_unions_parse() {
        let a: ProposeRosterMembersArgs = serde_json::from_value(json!({
            "target": { "rosterId": "abc" }, "members": [{ "name": "A" }]
        }))
        .unwrap();
        assert!(matches!(a.target, RosterTargetArg::Existing(_)));
        assert_eq!(a.members[0].confidence, ConfidenceArg::Medium);
        assert!(a.warnings.is_empty());
        let a: ProposeRosterMembersArgs = serde_json::from_value(json!({
            "target": { "newRosterName": "N" }, "members": []
        }))
        .unwrap();
        assert!(matches!(a.target, RosterTargetArg::New(_)));

        let v: ProposeVendorArgs = serde_json::from_value(json!({
            "target": "new", "info": { "name": "店" }, "menuItems": [{ "name": "a", "price": null }]
        }))
        .unwrap();
        assert_eq!(v.target, VendorTargetArg::New(NewLiteral::New));
        assert_eq!(v.menu_items[0].price, None);
        let v: ProposeVendorArgs = serde_json::from_value(json!({
            "target": { "vendorId": "x" }, "info": { "name": "店" }, "menuItems": []
        }))
        .unwrap();
        assert!(matches!(v.target, VendorTargetArg::Existing(_)));
    }

    #[test]
    fn strict_args_reject_unknown_fields() {
        assert!(
            serde_json::from_value::<ProposeVendorArgs>(json!({
                "target": "old", "info": { "name": "店" }, "menuItems": []
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ProposeRosterMembersArgs>(json!({
                "target": { "rosterId": "x", "organizerId": "other" }, "members": []
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ProposeRosterMembersArgs>(json!({
                "target": { "rosterId": "x" }, "members": [{ "name": "A", "extra": 1 }]
            }))
            .is_err()
        );
    }

    #[test]
    fn schemas_are_generated() {
        let s = serde_json::to_value(schemars::schema_for!(ProposeVendorArgs)).unwrap();
        assert_eq!(s["additionalProperties"], false);
        assert!(s["properties"]["menuItems"].is_object());
        let s = serde_json::to_value(schemars::schema_for!(ProposalDiffSummary)).unwrap();
        assert!(s["properties"]["priceChanges"].is_object());
    }
}
