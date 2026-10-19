//! Ports implemented by infrastructure adapters.

use std::collections::HashSet;
use std::time::SystemTime;

use async_trait::async_trait;
use domain::ai_import::{ImportRun, ImportRunId, ImportRunStatus};
use domain::directory::{
    MenuItemId, Roster, RosterId, RosterMemberId, UpsertReport, Vendor, VendorId,
};
use domain::identity::{Organizer, OrganizerId};
use domain::ordering::{
    Actor, GroupOrder, GroupOrderId, GroupOrderStatus, OfferedItemId, OrderSubmission,
    ParticipantId, ShareToken, SubmissionId,
};
use domain::settlement::Settlement;
use domain::shared::{DomainError, Money, NameKey};

use crate::token::TokenHash;

pub use domain::shared::Clock;

/// Failure of an adapter (database, network...). Details are for logs only.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

impl PortError {
    pub fn new(e: impl std::fmt::Display) -> Self {
        Self(e.to_string())
    }
}

pub type PortResult<T> = Result<T, PortError>;

#[async_trait]
pub trait OrganizerRepo: Send + Sync {
    async fn find_by_identity(&self, issuer: &str, sub: &str) -> PortResult<Option<Organizer>>;
    async fn find_by_id(&self, id: OrganizerId) -> PortResult<Option<Organizer>>;
    /// Insert `candidate`, or when its (issuer, sub) exists update email/name/avatar
    /// and `last_login_at`. Returns the stored organizer (with the existing id on update).
    async fn upsert_from_identity(
        &self,
        candidate: Organizer,
        now: SystemTime,
    ) -> PortResult<Organizer>;
    async fn touch_last_login(&self, id: OrganizerId, now: SystemTime) -> PortResult<()>;

    /// The local (password) account with this normalised email, with its PHC hash.
    async fn find_local_by_email(&self, email: &str) -> PortResult<Option<LocalAccount>>;
    /// Whether any organizer (OIDC, dev or local) has this email, ignoring case.
    async fn exists_by_email(&self, email: &str) -> PortResult<bool>;
    /// Insert a local account. `EmailTaken` when another local account has the email.
    async fn create_local(
        &self,
        organizer: Organizer,
        password_hash: String,
        now: SystemTime,
    ) -> PortResult<CreateLocalOutcome>;
    /// Replace the password hash of a local account; false when it does not exist.
    async fn set_password_hash(&self, id: OrganizerId, password_hash: String) -> PortResult<bool>;
    /// All organizers, oldest first, without secrets.
    async fn list_accounts(&self) -> PortResult<Vec<AccountSummary>>;
}

/// A local account together with its stored password hash (PHC string).
#[derive(Debug, Clone)]
pub struct LocalAccount {
    pub organizer: Organizer,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateLocalOutcome {
    Created,
    EmailTaken,
}

/// One row of `server admin list`; never contains the hash itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    pub email: String,
    pub display_name: String,
    /// An external identity is set (OIDC or development login).
    pub has_oidc: bool,
    pub has_password: bool,
    pub created_at: SystemTime,
}

/// Password hashing (Argon2id in production). Hashing is CPU-heavy, so adapters
/// run it off the async executor.
#[async_trait]
pub trait PasswordHasher: Send + Sync {
    /// Hash `password` into a self-describing PHC string with a fresh salt.
    async fn hash(&self, password: &str) -> PortResult<String>;
    /// Check `password` against a PHC string; `Ok(false)` on mismatch.
    async fn verify(&self, password: &str, hash: &str) -> PortResult<bool>;
    /// Burn the same work as [`Self::verify`] against a fixed hash, so that an
    /// unknown account takes as long as a wrong password. The result is discarded.
    async fn verify_dummy(&self, password: &str);
}

#[async_trait]
pub trait SessionRepo: Send + Sync {
    async fn create(
        &self,
        hash: TokenHash,
        organizer_id: OrganizerId,
        now: SystemTime,
        expires_at: SystemTime,
        user_agent: Option<&str>,
    ) -> PortResult<()>;
    /// The session's organizer and expiry, if it exists and `expires_at > now`.
    async fn find_valid(
        &self,
        hash: TokenHash,
        now: SystemTime,
    ) -> PortResult<Option<(OrganizerId, SystemTime)>>;
    async fn extend(&self, hash: TokenHash, new_expires_at: SystemTime) -> PortResult<()>;
    async fn delete(&self, hash: TokenHash) -> PortResult<()>;
    /// Returns the number of deleted rows.
    async fn delete_expired(&self, now: SystemTime) -> PortResult<u64>;
}

/// Pending login flow data (`oauth_states` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthState {
    pub state: String,
    pub pkce_verifier: String,
    pub nonce: String,
    pub return_to: String,
    pub expires_at: SystemTime,
}

#[async_trait]
pub trait OAuthStateRepo: Send + Sync {
    async fn save(&self, state: OAuthState) -> PortResult<()>;
    /// Consume the state exactly once; `None` if unknown, already used or expired.
    async fn take(&self, state: &str, now: SystemTime) -> PortResult<Option<OAuthState>>;
    async fn delete_expired(&self, now: SystemTime) -> PortResult<u64>;
}

/// Claims of a verified ID token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub issuer: String,
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("identity provider error: {0}")]
    Provider(String),
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    /// Authorization URL to redirect the browser to (PKCE S256 challenge supplied).
    async fn authorize_url(
        &self,
        state: &str,
        nonce: &str,
        pkce_challenge: &str,
    ) -> Result<String, IdentityError>;
    /// Exchange the code and verify the ID token (signature, iss, aud, exp, nonce).
    async fn exchange(
        &self,
        code: &str,
        pkce_verifier: &str,
        nonce: &str,
    ) -> Result<VerifiedIdentity, IdentityError>;
}

/// Cryptographically random URL-safe tokens.
pub trait TokenGenerator: Send + Sync {
    /// 32 random bytes encoded as base64url without padding (43 chars).
    fn generate(&self) -> String;
}

// ---------------------------------------------------------------------------
// Directory (rosters, vendors)
// ---------------------------------------------------------------------------

/// Failure of a directory repository operation.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// No such aggregate for this organizer (also used for another organizer's id).
    #[error("not found")]
    NotFound,
    /// The domain rejected the mutation; nothing was persisted.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// A `UNIQUE` constraint rejected the write (a race slipped past in-memory checks).
    #[error("unique constraint violated")]
    UniqueViolation,
    #[error(transparent)]
    Port(#[from] PortError),
}

pub type RepoResult<T> = Result<T, RepoError>;

/// An aggregate together with its persistence timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored<T> {
    pub value: T,
    pub updated_at: SystemTime,
}

#[async_trait]
pub trait DeliveryAddressRepo: Send + Sync {
    async fn list(
        &self,
        org: OrganizerId,
    ) -> PortResult<Vec<Stored<domain::directory::DeliveryAddress>>>;
    async fn find(
        &self,
        org: OrganizerId,
        id: domain::directory::DeliveryAddressId,
    ) -> PortResult<Option<Stored<domain::directory::DeliveryAddress>>>;
    /// Create or replace; updates must belong to `value.organizer_id`.
    async fn save(
        &self,
        value: domain::directory::DeliveryAddress,
        now: SystemTime,
        create: bool,
    ) -> RepoResult<Stored<domain::directory::DeliveryAddress>>;
    async fn delete(
        &self,
        org: OrganizerId,
        id: domain::directory::DeliveryAddressId,
    ) -> PortResult<bool>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterSummary {
    pub sharing_group_id: Option<domain::sharing::SharingGroupId>,
    pub can_manage: bool,
    pub id: RosterId,
    pub name: String,
    pub member_count: u32,
    pub updated_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorSummary {
    pub sharing_group_id: Option<domain::sharing::SharingGroupId>,
    pub can_manage: bool,
    pub id: VendorId,
    pub name: String,
    pub phone: Option<String>,
    pub item_count: u32,
    pub updated_at: SystemTime,
}

/// A mutation applied to a freshly loaded aggregate inside the repository's
/// write transaction. It may return an upsert report.
pub type RosterMutation = Box<
    dyn FnOnce(&mut Roster) -> Result<Option<UpsertReport<RosterMemberId>>, DomainError> + Send,
>;
pub type VendorMutation =
    Box<dyn FnOnce(&mut Vendor) -> Result<Option<UpsertReport<MenuItemId>>, DomainError> + Send>;

/// Result of a `modify`: the persisted aggregate and the mutation's report.
#[derive(Debug, Clone)]
pub struct Modified<T, R> {
    pub stored: Stored<T>,
    pub report: Option<R>,
}

/// Roster persistence. The organizer argument is the authenticated actor:
/// private data requires ownership, shared reads membership, shared writes admin.
///
/// Mutations go through [`RosterRepo::modify`]: the adapter loads the aggregate,
/// runs the closure and persists the difference (changed rows are upserted by
/// id, rows removed by the closure are deleted, nothing else is touched) inside
/// a single write transaction, so concurrent modifications never lose updates.
#[async_trait]
pub trait RosterRepo: Send + Sync {
    async fn check_scope(
        &self,
        actor: OrganizerId,
        group: Option<domain::sharing::SharingGroupId>,
        write: bool,
    ) -> RepoResult<()>;
    async fn list_scoped(
        &self,
        organizer_id: OrganizerId,
        group: Option<domain::sharing::SharingGroupId>,
    ) -> RepoResult<Vec<RosterSummary>>;
    async fn list(&self, organizer_id: OrganizerId) -> PortResult<Vec<RosterSummary>>;
    async fn find(
        &self,
        organizer_id: OrganizerId,
        id: RosterId,
    ) -> PortResult<Option<Stored<Roster>>>;
    /// Insert a new aggregate (with its members).
    async fn create(&self, roster: Roster, now: SystemTime) -> RepoResult<Stored<Roster>>;
    async fn modify(
        &self,
        organizer_id: OrganizerId,
        id: RosterId,
        now: SystemTime,
        mutation: RosterMutation,
    ) -> RepoResult<Modified<Roster, UpsertReport<RosterMemberId>>>;
    /// Returns whether a roster was deleted.
    async fn delete(&self, organizer_id: OrganizerId, id: RosterId) -> PortResult<bool>;
}

/// Vendor persistence; same contract as [`RosterRepo`].
#[async_trait]
pub trait VendorRepo: Send + Sync {
    async fn list_scoped(
        &self,
        organizer_id: OrganizerId,
        group: Option<domain::sharing::SharingGroupId>,
    ) -> RepoResult<Vec<VendorSummary>>;
    async fn find_scoped_name(
        &self,
        organizer_id: OrganizerId,
        group: Option<domain::sharing::SharingGroupId>,
        key: &NameKey,
    ) -> PortResult<Option<VendorId>>;
    async fn list(&self, organizer_id: OrganizerId) -> PortResult<Vec<VendorSummary>>;
    async fn find(
        &self,
        organizer_id: OrganizerId,
        id: VendorId,
    ) -> PortResult<Option<Stored<Vendor>>>;
    /// The vendor of this organizer whose name has this key, if any.
    async fn find_id_by_name_key(
        &self,
        organizer_id: OrganizerId,
        key: &NameKey,
    ) -> PortResult<Option<VendorId>>;
    async fn create(&self, vendor: Vendor, now: SystemTime) -> RepoResult<Stored<Vendor>>;
    async fn modify(
        &self,
        organizer_id: OrganizerId,
        id: VendorId,
        now: SystemTime,
        mutation: VendorMutation,
    ) -> RepoResult<Modified<Vendor, UpsertReport<MenuItemId>>>;
    async fn delete(&self, organizer_id: OrganizerId, id: VendorId) -> PortResult<bool>;
}

// ---------------------------------------------------------------------------
// Ordering (group orders, submissions)
// ---------------------------------------------------------------------------

/// A mutation applied to a freshly loaded group order inside the repository's
/// write transaction.
pub type GroupOrderMutation = Box<dyn FnOnce(&mut GroupOrder) -> Result<(), DomainError> + Send>;

/// Group order persistence. Organizer-facing lookups are scoped by `organizer_id`.
#[async_trait]
pub trait GroupOrderRepo: Send + Sync {
    /// Insert the group with its participants and offered items in one transaction.
    async fn create(&self, group: &GroupOrder) -> RepoResult<()>;
    async fn find_by_id(
        &self,
        organizer_id: OrganizerId,
        id: GroupOrderId,
    ) -> PortResult<Option<GroupOrder>>;
    /// Public lookup by share token (no organizer scope).
    async fn find_by_token(&self, token: &ShareToken) -> PortResult<Option<GroupOrder>>;
    /// Load -> mutate -> persist inside one write transaction (scoped by organizer).
    /// Persists status/title/deadline/note/token changes, newly added participants
    /// and changed `sold_out` flags.
    async fn modify(
        &self,
        organizer_id: OrganizerId,
        id: GroupOrderId,
        mutation: GroupOrderMutation,
    ) -> RepoResult<GroupOrder>;
}

/// Result of appending a submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Inserted,
    /// A submission with the same `(group, idempotency_key)` already exists; nothing was inserted.
    Duplicate(SubmissionId),
    /// The group does not accept this submission; the value is a stable code
    /// (`group_closed`, `deadline_passed` or `group_archived`).
    Rejected(&'static str),
}

#[async_trait]
pub trait SubmissionRepo: Send + Sync {
    /// Append-only insert (see `AppendOutcome`). When `require_open_for_participant`
    /// is true the open/deadline check happens atomically in the INSERT itself;
    /// otherwise only archived groups are refused (organizer submissions).
    async fn append(
        &self,
        submission: &OrderSubmission,
        require_open_for_participant: bool,
        now: SystemTime,
    ) -> RepoResult<AppendOutcome>;
    async fn find_id_by_idempotency_key(
        &self,
        group: GroupOrderId,
        key: uuid::Uuid,
    ) -> PortResult<Option<SubmissionId>>;
    /// The participant's latest submission (by id).
    async fn find_latest(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Option<OrderSubmission>>;
    /// All of the participant's submissions, newest first.
    async fn list_for_participant(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Vec<OrderSubmission>>;
}

/// One line of an effective order, with the item name resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLineView {
    pub item_id: OfferedItemId,
    pub name: String,
    pub quantity: u32,
    pub unit_price: Money,
    pub subtotal: Money,
    pub note: Option<String>,
}

/// A participant's effective (latest) submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveOrderView {
    pub submission_id: SubmissionId,
    pub submitted_at: SystemTime,
    pub note: Option<String>,
    /// Zero lines = the participant cancelled.
    pub cancelled: bool,
    pub total: Money,
    pub lines: Vec<OrderLineView>,
}

/// Read models over the `effective_submissions` view.
#[async_trait]
pub trait OrderReadModel: Send + Sync {
    /// Participants whose effective submission has at least one line.
    async fn ordered_participants(&self, group: GroupOrderId)
    -> PortResult<HashSet<ParticipantId>>;
    async fn effective_order(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Option<EffectiveOrderView>>;
}

// ---------------------------------------------------------------------------
// Management (organizer-facing read models and settlement)
// ---------------------------------------------------------------------------

/// One row of the organizer's group order list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupListItem {
    pub id: GroupOrderId,
    pub title: String,
    pub status: GroupOrderStatus,
    pub vendor_name: String,
    pub deadline_at: Option<SystemTime>,
    pub created_at: SystemTime,
    pub closed_at: Option<SystemTime>,
    pub participant_count: u32,
    /// Participants whose effective order has at least one line.
    pub ordered_count: u32,
    /// Sum of the effective orders (`quantity * unit_price`).
    pub total_amount: Money,
}

/// Per offered item totals over effective orders (zero rows included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStat {
    pub item_id: OfferedItemId,
    pub name: String,
    pub price: Money,
    pub quantity: u32,
    /// Sum of `quantity * unit_price` using each line's stored price.
    pub subtotal: Money,
}

/// A line of a participant's effective order as shown in the detail table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantLine {
    pub item_id: OfferedItemId,
    pub name: String,
    pub quantity: u32,
    pub note: Option<String>,
}

/// A participant with their effective order, amount due and settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantRow {
    pub id: ParticipantId,
    pub name: String,
    pub sort_order: i32,
    /// Sum of the effective order (0 when never ordered or cancelled).
    pub amount_due: Money,
    /// Lines of the effective order (empty when never ordered or cancelled).
    pub items: Vec<ParticipantLine>,
    pub order_note: Option<String>,
    /// When the effective (latest) submission was made.
    pub submitted_at: Option<SystemTime>,
    /// All submissions including cancellations.
    pub submission_count: u32,
    pub settlement: Settlement,
    /// Some `device_id` of this participant was also used for another participant.
    pub suspicious_device: bool,
    /// First 3 characters of the latest submission's `device_id`.
    pub device_short: Option<String>,
}

/// Client metadata of a submission; never carries the raw `User-Agent` or the full device id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientView {
    pub ip: Option<String>,
    pub ua_summary: Option<String>,
    pub device_short: Option<String>,
    pub device_model: Option<String>,
    pub screen: Option<String>,
    pub language: Option<String>,
    pub timezone: Option<String>,
}

/// One entry of a participant's submission history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionHistoryEntry {
    pub id: SubmissionId,
    pub submitted_at: SystemTime,
    pub submitted_by: Actor,
    pub cancelled: bool,
    pub total: Money,
    pub note: Option<String>,
    pub lines: Vec<OrderLineView>,
    pub client: ClientView,
    /// This is the participant's latest submission.
    pub effective: bool,
    /// Names of other participants that used this submission's `device_id` in the group.
    pub shared_device_with: Vec<String>,
}

/// Organizer-facing read models. Every method issues a fixed number of queries
/// regardless of the number of participants.
#[async_trait]
pub trait ManagementReadModel: Send + Sync {
    /// Newest first; `status = None` lists all. One query.
    async fn list_groups(
        &self,
        organizer_id: OrganizerId,
        status: Option<GroupOrderStatus>,
    ) -> PortResult<Vec<GroupListItem>>;
    /// Offered-item statistics in menu order. One query.
    async fn item_stats(&self, group: GroupOrderId) -> PortResult<Vec<ItemStat>>;
    /// All participants (`only = None`) or one, in display order. Two queries.
    async fn participant_rows(
        &self,
        group: GroupOrderId,
        only: Option<ParticipantId>,
    ) -> PortResult<Vec<ParticipantRow>>;
    /// The participant's submissions, newest first. Three queries.
    async fn submission_history(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Vec<SubmissionHistoryEntry>>;
}

/// Settlement persistence.
#[async_trait]
pub trait SettlementRepo: Send + Sync {
    /// Within one write transaction: read the participant's current amount due
    /// and settlement row, apply `Settlement::set` and upsert. `NotFound` when the
    /// group is not the organizer's or the participant is not in it.
    async fn set(
        &self,
        organizer_id: OrganizerId,
        group: GroupOrderId,
        participant: ParticipantId,
        picked_up: bool,
        paid: bool,
        now: SystemTime,
    ) -> RepoResult<()>;
}

/// Turns a raw `User-Agent` into a short human summary.
pub trait UaParser: Send + Sync {
    /// e.g. `iPhone · iOS 18.1 · LINE 14.17.0`; `None` when nothing useful is recognised.
    fn summarize(&self, ua: &str) -> Option<String>;
}

// ---------------------------------------------------------------------------
// AI import
// ---------------------------------------------------------------------------

/// `ai_import_runs` persistence. Every lookup is scoped by `organizer_id`.
#[async_trait]
pub trait ImportRunRepo: Send + Sync {
    async fn create(&self, run: &ImportRun) -> PortResult<()>;
    async fn find(
        &self,
        organizer_id: OrganizerId,
        id: ImportRunId,
    ) -> PortResult<Option<ImportRun>>;
    /// Compare-and-set save: persist the mutable columns of `run` only if the
    /// stored status is still `expected` (otherwise
    /// `RepoError::Domain(Invalid { field: "status" })`; a vanished row is
    /// `RepoError::NotFound`).
    async fn save(&self, run: &ImportRun, expected: ImportRunStatus) -> RepoResult<()>;
    /// Number of runs of this organizer created at or after `since`.
    async fn count_since(&self, organizer_id: OrganizerId, since: SystemTime) -> PortResult<u64>;
}

/// One write of an import apply (see [`ImportApplyRepo`]).
pub enum ImportApplyStep {
    CreateDeliveryAddress(domain::directory::DeliveryAddress),
    CreateRoster(Roster),
    ModifyRoster {
        id: RosterId,
        mutation: RosterMutation,
    },
    CreateVendor(Vendor),
    ModifyVendor {
        id: VendorId,
        mutation: VendorMutation,
    },
}

/// Unit of work for `ApplyImportRun`: every step plus the run's
/// `Proposed -> Applied` transition are committed in one write transaction.
#[async_trait]
pub trait ImportApplyRepo: Send + Sync {
    /// `run` is already in `Applied` state in memory. The adapter first flips
    /// the stored row from `proposed` to `applied` (failing with
    /// `RepoError::Domain(Invalid { field: "status" })` if it is not
    /// `proposed` any more, which makes concurrent/double applies safe), then
    /// runs the steps in order. Any error rolls everything back.
    async fn apply(&self, run: &ImportRun, steps: Vec<ImportApplyStep>) -> RepoResult<()>;
}
