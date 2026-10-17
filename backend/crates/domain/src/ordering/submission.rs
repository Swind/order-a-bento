use std::time::SystemTime;
use uuid::Uuid;

use super::{ClientInfo, GroupOrderId, OfferedItemId, ParticipantId};
use crate::shared::{DomainError, Id, Money};

pub struct SubmissionMarker;
pub type SubmissionId = Id<SubmissionMarker>;

/// Who made the submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Participant,
    Organizer,
}

impl Actor {
    pub fn as_str(self) -> &'static str {
        match self {
            Actor::Participant => "participant",
            Actor::Organizer => "organizer",
        }
    }
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "participant" => Ok(Actor::Participant),
            "organizer" => Ok(Actor::Organizer),
            _ => Err(DomainError::invalid("submittedBy", "未知的角色")),
        }
    }
}

/// Requested line (before validation / price lookup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineInput {
    pub offered_item_id: OfferedItemId,
    pub quantity: u32,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionLine {
    /// 1-based position within the submission.
    pub line_no: u32,
    pub offered_item_id: OfferedItemId,
    pub quantity: u32,
    /// Copied from the offered item at submission time; never changes.
    pub unit_price: Money,
    pub note: Option<String>,
}

impl SubmissionLine {
    pub fn subtotal(&self) -> Money {
        Money::new(self.unit_price.get() * i64::from(self.quantity))
    }
}

/// Append-only record of one "send" by a participant. Immutable: no mutators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSubmission {
    id: SubmissionId,
    group_order_id: GroupOrderId,
    participant_id: ParticipantId,
    lines: Vec<SubmissionLine>,
    note: Option<String>,
    submitted_by: Actor,
    submitted_at: SystemTime,
    idempotency_key: Uuid,
    client_info: Option<ClientInfo>,
}

impl OrderSubmission {
    pub const MAX_LINES: usize = 30;
    pub const MIN_QUANTITY: u32 = 1;
    pub const MAX_QUANTITY: u32 = 20;
    pub const LINE_NOTE_MAX_LEN: usize = 50;

    /// Reconstitute from storage (trusted).
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        id: SubmissionId,
        group_order_id: GroupOrderId,
        participant_id: ParticipantId,
        lines: Vec<SubmissionLine>,
        note: Option<String>,
        submitted_by: Actor,
        submitted_at: SystemTime,
        idempotency_key: Uuid,
        client_info: Option<ClientInfo>,
    ) -> Self {
        Self {
            id,
            group_order_id,
            participant_id,
            lines,
            note,
            submitted_by,
            submitted_at,
            idempotency_key,
            client_info,
        }
    }

    pub fn id(&self) -> SubmissionId {
        self.id
    }
    pub fn group_order_id(&self) -> GroupOrderId {
        self.group_order_id
    }
    pub fn participant_id(&self) -> ParticipantId {
        self.participant_id
    }
    pub fn lines(&self) -> &[SubmissionLine] {
        &self.lines
    }
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
    pub fn submitted_by(&self) -> Actor {
        self.submitted_by
    }
    pub fn submitted_at(&self) -> SystemTime {
        self.submitted_at
    }
    pub fn idempotency_key(&self) -> Uuid {
        self.idempotency_key
    }
    pub fn client_info(&self) -> Option<&ClientInfo> {
        self.client_info.as_ref()
    }

    pub fn total(&self) -> Money {
        self.lines.iter().map(SubmissionLine::subtotal).sum()
    }

    /// An empty submission cancels the participant's order.
    pub fn is_cancellation(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Read-model helper: the effective order is a participant's latest submission.
pub struct EffectiveOrder;

impl EffectiveOrder {
    /// Latest by id (UUIDv7 is time-ordered). Input order is irrelevant and
    /// should contain a single participant's submissions.
    pub fn latest(submissions: &[OrderSubmission]) -> Option<&OrderSubmission> {
        submissions.iter().max_by_key(|s| s.id)
    }
}
