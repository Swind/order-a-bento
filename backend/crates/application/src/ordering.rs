//! Ordering use cases: opening a group order and the public (token) ordering flow.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

use domain::directory::{RosterId, VendorId};
use domain::identity::OrganizerId;
use domain::ordering::{
    Actor, ClientInfo, GroupOrder, GroupOrderId, GroupOrderStatus, LineInput, OfferedItemId,
    ParticipantId, ShareToken,
};
use domain::shared::{DomainError, Money, Patch};
use uuid::Uuid;

use crate::ports::{
    AppendOutcome, Clock, EffectiveOrderView, GroupListItem, GroupOrderRepo, ItemStat,
    ManagementReadModel, OrderReadModel, ParticipantRow, PortError, RepoError, RosterRepo,
    SettlementRepo, SubmissionHistoryEntry, SubmissionRepo, TokenGenerator, VendorRepo,
};

#[derive(Debug, thiserror::Error)]
pub enum OrderingError {
    /// Unknown / foreign id, malformed or unknown token, unknown participant.
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Domain(DomainError),
    #[error("conflict")]
    Conflict,
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DomainError> for OrderingError {
    fn from(e: DomainError) -> Self {
        match e {
            DomainError::NotFound { .. } => Self::NotFound,
            other => Self::Domain(other),
        }
    }
}

impl From<PortError> for OrderingError {
    fn from(e: PortError) -> Self {
        Self::Internal(e.0)
    }
}

impl From<RepoError> for OrderingError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::NotFound => Self::NotFound,
            RepoError::Domain(d) => d.into(),
            RepoError::UniqueViolation => Self::Conflict,
            RepoError::Port(p) => p.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenGroupCmd {
    pub delivery_address_id: Option<domain::directory::DeliveryAddressId>,
    pub title: String,
    pub roster_id: RosterId,
    pub vendor_id: VendorId,
    pub deadline_at: Option<SystemTime>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SubmitOrderCmd {
    /// Raw token from the URL (parsed here so malformed == unknown == 404).
    pub share_token: String,
    pub participant_id: ParticipantId,
    /// Empty = cancel the order.
    pub lines: Vec<LineInput>,
    pub note: Option<String>,
    pub idempotency_key: Uuid,
    pub client_info: ClientInfo,
}

/// Organizer submission on behalf of a participant (no client info is stored).
#[derive(Debug, Clone)]
pub struct OrganizerSubmitCmd {
    pub group_id: GroupOrderId,
    pub participant_id: ParticipantId,
    /// Empty = cancel the order.
    pub lines: Vec<LineInput>,
    pub note: Option<String>,
    pub idempotency_key: Uuid,
}

/// Merge-patch of a group order; every part defaults to "keep".
#[derive(Debug, Clone, Default)]
pub struct UpdateGroupCmd {
    pub delivery_address_id: Patch<domain::directory::DeliveryAddressId>,
    pub title: Option<String>,
    pub deadline_at: Patch<SystemTime>,
    pub note: Patch<String>,
    /// Target status; the same status is a no-op, others go through the state machine.
    pub status: Option<GroupOrderStatus>,
}

/// Settlement figures of a group, computed over effective orders only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    pub group_id: GroupOrderId,
    pub status: GroupOrderStatus,
    pub items: Vec<ItemStat>,
    pub total_quantity: u32,
    pub total_amount: Money,
    pub participants_total: u32,
    pub participants_ordered: u32,
    pub picked_up: u32,
    pub paid: u32,
    /// Sum of `paid_amount` over participants marked paid.
    pub paid_amount: Money,
    /// `total_amount` minus the current `amount_due` of participants marked paid:
    /// what is still expected from everybody who has not paid. A paid
    /// participant's 尚差/應退 difference is not included (see their `balance`).
    pub unpaid_amount: Money,
}

/// What the public page needs: the group plus who has ordered.
#[derive(Debug, Clone)]
pub struct PublicGroupView {
    pub group: GroupOrder,
    pub ordered: HashSet<ParticipantId>,
    pub accepting_orders: bool,
}

pub struct OrderingService {
    pub delivery_addresses: Arc<dyn crate::ports::DeliveryAddressRepo>,
    pub groups: Arc<dyn GroupOrderRepo>,
    pub submissions: Arc<dyn SubmissionRepo>,
    pub read_model: Arc<dyn OrderReadModel>,
    pub management: Arc<dyn ManagementReadModel>,
    pub settlements: Arc<dyn SettlementRepo>,
    pub rosters: Arc<dyn RosterRepo>,
    pub vendors: Arc<dyn VendorRepo>,
    pub tokens: Arc<dyn TokenGenerator>,
    pub clock: Arc<dyn Clock>,
    /// `APP_BASE_URL`; share links are `<base>/g/<token>`.
    pub app_base_url: String,
}

impl OrderingService {
    pub fn share_url(&self, group: &GroupOrder) -> String {
        format!(
            "{}/g/{}",
            self.app_base_url.trim_end_matches('/'),
            group.share_token().as_str()
        )
    }

    /// Snapshot a roster and a vendor menu into a new open group order.
    pub async fn open_group(
        &self,
        org: OrganizerId,
        cmd: OpenGroupCmd,
    ) -> Result<GroupOrder, OrderingError> {
        let roster = self
            .rosters
            .find(org, cmd.roster_id)
            .await?
            .ok_or(OrderingError::NotFound)?;
        let vendor = self
            .vendors
            .find(org, cmd.vendor_id)
            .await?
            .ok_or(OrderingError::NotFound)?;
        let token = ShareToken::parse(&self.tokens.generate())?;
        let mut group = GroupOrder::open(
            org,
            &cmd.title,
            &roster.value,
            &vendor.value,
            cmd.deadline_at,
            cmd.note.as_deref(),
            token,
            self.clock.now(),
        )?;
        if let Some(id) = cmd.delivery_address_id {
            let address = self
                .delivery_addresses
                .find(org, id)
                .await?
                .ok_or(OrderingError::NotFound)?;
            group.set_delivery_address(Patch::Set(address.value.snapshot()))?;
        }
        self.groups.create(&group).await?;
        Ok(group)
    }

    pub async fn get_group(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
    ) -> Result<GroupOrder, OrderingError> {
        self.groups
            .find_by_id(org, id)
            .await?
            .ok_or(OrderingError::NotFound)
    }

    async fn group_by_token(&self, token: &str) -> Result<GroupOrder, OrderingError> {
        // Malformed and unknown tokens are indistinguishable to the caller.
        let token = ShareToken::parse(token).map_err(|_| OrderingError::NotFound)?;
        self.groups
            .find_by_token(&token)
            .await?
            .ok_or(OrderingError::NotFound)
    }

    pub async fn get_public_group(&self, token: &str) -> Result<PublicGroupView, OrderingError> {
        let group = self.group_by_token(token).await?;
        let ordered = self.read_model.ordered_participants(group.id()).await?;
        let accepting_orders = group.accepts_orders(self.clock.now());
        Ok(PublicGroupView {
            group,
            ordered,
            accepting_orders,
        })
    }

    pub async fn get_effective_order(
        &self,
        token: &str,
        participant: ParticipantId,
    ) -> Result<Option<EffectiveOrderView>, OrderingError> {
        let group = self.group_by_token(token).await?;
        self.ensure_participant(&group, participant)?;
        Ok(self
            .read_model
            .effective_order(group.id(), participant)
            .await?)
    }

    fn ensure_participant(
        &self,
        group: &GroupOrder,
        participant: ParticipantId,
    ) -> Result<(), OrderingError> {
        if group.participants().iter().any(|p| p.id == participant) {
            Ok(())
        } else {
            Err(OrderingError::NotFound)
        }
    }

    /// Design §5.3. Returns the participant's effective order after the submit
    /// (a replayed idempotency key returns it without inserting anything).
    pub async fn submit_order(
        &self,
        cmd: SubmitOrderCmd,
    ) -> Result<EffectiveOrderView, OrderingError> {
        let group = self.group_by_token(&cmd.share_token).await?;
        self.ensure_participant(&group, cmd.participant_id)?;
        // A retried request must succeed even if the group closed in between.
        if self
            .submissions
            .find_id_by_idempotency_key(group.id(), cmd.idempotency_key)
            .await?
            .is_some()
        {
            return self.effective_after(group.id(), cmd.participant_id).await;
        }
        let now = self.clock.now();
        let submission = group.new_submission(
            cmd.participant_id,
            &cmd.lines,
            cmd.note.as_deref(),
            Actor::Participant,
            cmd.idempotency_key,
            Some(cmd.client_info),
            now,
        )?;
        match self.submissions.append(&submission, true, now).await? {
            AppendOutcome::Inserted | AppendOutcome::Duplicate(_) => {}
            AppendOutcome::Rejected(reason) => {
                return Err(OrderingError::Domain(DomainError::Closed { reason }));
            }
        }
        self.effective_after(group.id(), cmd.participant_id).await
    }

    async fn effective_after(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> Result<EffectiveOrderView, OrderingError> {
        self.read_model
            .effective_order(group, participant)
            .await?
            .ok_or_else(|| OrderingError::Internal("submission vanished after append".into()))
    }

    // ---- organizer management ----

    pub async fn list_groups(
        &self,
        org: OrganizerId,
        status: Option<GroupOrderStatus>,
    ) -> Result<Vec<GroupListItem>, OrderingError> {
        Ok(self.management.list_groups(org, status).await?)
    }

    /// Edit title / deadline / note / status in one write transaction. Parts are
    /// applied in that order (so a new deadline is in place before a reopen).
    pub async fn update_group(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
        cmd: UpdateGroupCmd,
    ) -> Result<GroupOrder, OrderingError> {
        let now = self.clock.now();
        let delivery_address = match cmd.delivery_address_id {
            Patch::Keep => Patch::Keep,
            Patch::Clear => Patch::Clear,
            Patch::Set(id) => Patch::Set(
                self.delivery_addresses
                    .find(org, id)
                    .await?
                    .ok_or(OrderingError::NotFound)?
                    .value
                    .snapshot(),
            ),
        };
        Ok(self
            .groups
            .modify(
                org,
                id,
                Box::new(move |g| {
                    if let Some(title) = &cmd.title {
                        g.rename(title)?;
                    }
                    g.set_deadline(cmd.deadline_at, now)?;
                    g.set_note(cmd.note)?;
                    g.set_delivery_address(delivery_address)?;
                    match cmd.status {
                        Some(target) if target != g.status() => match target {
                            GroupOrderStatus::Open => g.reopen(now)?,
                            GroupOrderStatus::Closed => g.close(now)?,
                            GroupOrderStatus::Archived => g.archive()?,
                        },
                        _ => {}
                    }
                    Ok(())
                }),
            )
            .await?)
    }

    pub async fn rotate_share_token(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
    ) -> Result<GroupOrder, OrderingError> {
        let token = ShareToken::parse(&self.tokens.generate())?;
        Ok(self
            .groups
            .modify(
                org,
                id,
                Box::new(move |g| {
                    g.rotate_share_token(token);
                    Ok(())
                }),
            )
            .await?)
    }

    pub async fn add_participant(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
        name: String,
    ) -> Result<GroupOrder, OrderingError> {
        Ok(self
            .groups
            .modify(
                org,
                id,
                Box::new(move |g| g.add_participant(&name).map(|_| ())),
            )
            .await?)
    }

    pub async fn set_sold_out(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
        item: OfferedItemId,
        sold_out: bool,
    ) -> Result<GroupOrder, OrderingError> {
        Ok(self
            .groups
            .modify(org, id, Box::new(move |g| g.set_sold_out(item, sold_out)))
            .await?)
    }

    /// Two queries for the figures (items, participants) plus the ownership check.
    pub async fn summary(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
    ) -> Result<GroupSummary, OrderingError> {
        let group = self.get_group(org, id).await?;
        let items = self.management.item_stats(id).await?;
        let rows = self.management.participant_rows(id, None).await?;
        let total_quantity = items.iter().map(|i| i.quantity).sum();
        let total_amount: Money = items.iter().map(|i| i.subtotal).sum();
        let paid_rows = || rows.iter().filter(|r| r.settlement.paid_at().is_some());
        let paid_amount = paid_rows()
            .filter_map(|r| r.settlement.paid_amount())
            .sum::<Money>();
        let paid_due = paid_rows().map(|r| r.amount_due).sum::<Money>();
        Ok(GroupSummary {
            group_id: id,
            status: group.status(),
            items,
            total_quantity,
            total_amount,
            participants_total: rows.len() as u32,
            participants_ordered: rows.iter().filter(|r| !r.items.is_empty()).count() as u32,
            picked_up: rows
                .iter()
                .filter(|r| r.settlement.picked_up_at().is_some())
                .count() as u32,
            paid: paid_rows().count() as u32,
            paid_amount,
            unpaid_amount: Money::new(total_amount.get() - paid_due.get()),
        })
    }

    pub async fn list_participants(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
    ) -> Result<Vec<ParticipantRow>, OrderingError> {
        self.get_group(org, id).await?;
        Ok(self.management.participant_rows(id, None).await?)
    }

    pub async fn submission_history(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
        participant: ParticipantId,
    ) -> Result<Vec<SubmissionHistoryEntry>, OrderingError> {
        let group = self.get_group(org, id).await?;
        self.ensure_participant(&group, participant)?;
        Ok(self.management.submission_history(id, participant).await?)
    }

    /// The organizer submits (or corrects, or cancels) on behalf of a participant.
    /// Allowed while closed or past the deadline, refused when archived.
    pub async fn submit_as_organizer(
        &self,
        org: OrganizerId,
        cmd: OrganizerSubmitCmd,
    ) -> Result<EffectiveOrderView, OrderingError> {
        let group = self.get_group(org, cmd.group_id).await?;
        self.ensure_participant(&group, cmd.participant_id)?;
        if self
            .submissions
            .find_id_by_idempotency_key(group.id(), cmd.idempotency_key)
            .await?
            .is_some()
        {
            return self.effective_after(group.id(), cmd.participant_id).await;
        }
        let now = self.clock.now();
        let submission = group.new_submission(
            cmd.participant_id,
            &cmd.lines,
            cmd.note.as_deref(),
            Actor::Organizer,
            cmd.idempotency_key,
            None,
            now,
        )?;
        match self.submissions.append(&submission, false, now).await? {
            AppendOutcome::Inserted | AppendOutcome::Duplicate(_) => {}
            AppendOutcome::Rejected(reason) => {
                return Err(OrderingError::Domain(DomainError::Closed { reason }));
            }
        }
        self.effective_after(group.id(), cmd.participant_id).await
    }

    /// Idempotent set of the pickup / payment flags; returns the participant's row.
    pub async fn set_settlement(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
        participant: ParticipantId,
        picked_up: bool,
        paid: bool,
    ) -> Result<ParticipantRow, OrderingError> {
        self.settlements
            .set(org, id, participant, picked_up, paid, self.clock.now())
            .await?;
        self.management
            .participant_rows(id, Some(participant))
            .await?
            .into_iter()
            .next()
            .ok_or(OrderingError::NotFound)
    }
}
