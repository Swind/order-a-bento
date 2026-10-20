use std::{collections::HashSet, fmt, time::SystemTime};
use uuid::Uuid;

use super::{
    Actor, ClientInfo, LineInput, OrderSubmission, ShareToken, SubmissionId, SubmissionLine,
};
use crate::directory::{Roster, RosterId, Vendor, VendorId};
use crate::identity::OrganizerId;
use crate::shared::{
    DomainError, Id, ItemName, Money, Patch, PersonName,
    text::{NOTE_MAX_LEN, collapse_whitespace, optional_text},
};

pub struct GroupOrderMarker;
pub struct ParticipantMarker;
pub struct OfferedItemMarker;
pub type GroupOrderId = Id<GroupOrderMarker>;
pub type ParticipantId = Id<ParticipantMarker>;
pub type OfferedItemId = Id<OfferedItemMarker>;

/// Group order title, 1..=50 chars (whitespace collapsed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTitle(String);

impl GroupTitle {
    pub const MAX_LEN: usize = 50;

    pub fn new(s: &str) -> Result<Self, DomainError> {
        let v = collapse_whitespace(s);
        let n = v.chars().count();
        if n == 0 {
            return Err(DomainError::invalid("title", "不可為空"));
        }
        if n > Self::MAX_LEN {
            return Err(DomainError::invalid(
                "title",
                format!("最多 {} 個字元", Self::MAX_LEN),
            ));
        }
        Ok(Self(v))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GroupTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupOrderStatus {
    Open,
    Closed,
    Archived,
}

impl GroupOrderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Archived => "archived",
        }
    }
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "open" => Ok(Self::Open),
            "closed" => Ok(Self::Closed),
            "archived" => Ok(Self::Archived),
            _ => Err(DomainError::invalid("status", "未知的狀態")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: ParticipantId,
    pub name: PersonName,
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedItem {
    pub id: OfferedItemId,
    pub name: ItemName,
    pub price: Money,
    pub description: Option<String>,
    pub category: Option<String>,
    pub sort_order: i32,
    pub sold_out: bool,
}

/// Vendor contact info frozen at open time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorSnapshot {
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
}

/// Storage-shaped input for [`GroupOrder::from_parts`].
#[derive(Debug, Clone)]
pub struct GroupOrderParts {
    pub sharing_group_id: Option<crate::sharing::SharingGroupId>,
    pub delivery_address: Option<String>,
    pub id: GroupOrderId,
    pub organizer_id: OrganizerId,
    pub title: GroupTitle,
    pub share_token: ShareToken,
    pub status: GroupOrderStatus,
    pub deadline_at: Option<SystemTime>,
    pub note: Option<String>,
    pub source_roster_id: Option<RosterId>,
    pub source_vendor_id: Option<VendorId>,
    pub vendor: VendorSnapshot,
    pub created_at: SystemTime,
    pub closed_at: Option<SystemTime>,
    pub participants: Vec<Participant>,
    pub offered_items: Vec<OfferedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOrder {
    pub sharing_group_id: Option<crate::sharing::SharingGroupId>,
    /// Actor-specific read metadata, never persisted.
    pub sharing_group_name: Option<String>,
    pub can_manage: bool,
    delivery_address: Option<String>,
    id: GroupOrderId,
    organizer_id: OrganizerId,
    title: GroupTitle,
    share_token: ShareToken,
    status: GroupOrderStatus,
    deadline_at: Option<SystemTime>,
    note: Option<String>,
    source_roster_id: Option<RosterId>,
    source_vendor_id: Option<VendorId>,
    vendor: VendorSnapshot,
    created_at: SystemTime,
    closed_at: Option<SystemTime>,
    participants: Vec<Participant>,
    offered_items: Vec<OfferedItem>,
}

impl GroupOrder {
    /// Snapshot a roster and a vendor menu into a new open group order.
    ///
    /// Requires an orderable vendor and at least one roster member. Only
    /// non-archived menu items are offered. A deadline must lie after `now`.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        organizer_id: OrganizerId,
        title: &str,
        roster: &Roster,
        vendor: &Vendor,
        deadline_at: Option<SystemTime>,
        note: Option<&str>,
        share_token: ShareToken,
        now: SystemTime,
    ) -> Result<Self, DomainError> {
        let title = GroupTitle::new(title)?;
        let note = optional_text("note", note, NOTE_MAX_LEN)?;
        if let Some(d) = deadline_at
            && d <= now
        {
            return Err(DomainError::invalid("deadlineAt", "截止時間必須晚於現在"));
        }
        if !vendor.is_orderable() {
            return Err(DomainError::invalid("vendorId", "店家沒有可訂購的品項"));
        }
        if roster.members().is_empty() {
            return Err(DomainError::invalid("rosterId", "名單沒有任何成員"));
        }
        let participants = roster
            .members()
            .iter()
            .map(|m| Participant {
                id: ParticipantId::new(),
                name: m.name.clone(),
                sort_order: m.sort_order,
            })
            .collect();
        let offered_items = vendor
            .menu()
            .iter()
            .filter(|m| !m.archived)
            .map(|m| OfferedItem {
                id: OfferedItemId::new(),
                name: m.name.clone(),
                price: m.price,
                description: m.description.clone(),
                category: m.category.clone(),
                sort_order: m.sort_order,
                sold_out: false,
            })
            .collect();
        Ok(Self {
            id: GroupOrderId::new(),
            sharing_group_id: None,
            sharing_group_name: None,
            can_manage: true,
            delivery_address: None,
            organizer_id,
            title,
            share_token,
            status: GroupOrderStatus::Open,
            deadline_at,
            note,
            source_roster_id: Some(roster.id()),
            source_vendor_id: Some(vendor.id()),
            vendor: VendorSnapshot {
                name: vendor.name().to_string(),
                phone: vendor.phone().map(str::to_owned),
                address: vendor.address().map(str::to_owned),
            },
            created_at: now,
            closed_at: None,
            participants,
            offered_items,
        })
    }

    /// Reconstitute from storage (trusted); participant names must be unique.
    pub fn from_parts(p: GroupOrderParts) -> Result<Self, DomainError> {
        let mut seen = HashSet::new();
        for pt in &p.participants {
            if !seen.insert(pt.name.key()) {
                return Err(DomainError::Duplicate {
                    field: "name",
                    value: pt.name.to_string(),
                });
            }
        }
        Ok(Self {
            id: p.id,
            sharing_group_id: p.sharing_group_id,
            sharing_group_name: None,
            can_manage: true,
            delivery_address: p.delivery_address,
            organizer_id: p.organizer_id,
            title: p.title,
            share_token: p.share_token,
            status: p.status,
            deadline_at: p.deadline_at,
            note: p.note,
            source_roster_id: p.source_roster_id,
            source_vendor_id: p.source_vendor_id,
            vendor: p.vendor,
            created_at: p.created_at,
            closed_at: p.closed_at,
            participants: p.participants,
            offered_items: p.offered_items,
        })
    }

    pub fn id(&self) -> GroupOrderId {
        self.id
    }
    pub fn organizer_id(&self) -> OrganizerId {
        self.organizer_id
    }
    pub fn title(&self) -> &GroupTitle {
        &self.title
    }
    pub fn share_token(&self) -> &ShareToken {
        &self.share_token
    }
    pub fn status(&self) -> GroupOrderStatus {
        self.status
    }
    pub fn deadline_at(&self) -> Option<SystemTime> {
        self.deadline_at
    }
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
    pub fn delivery_address(&self) -> Option<&str> {
        self.delivery_address.as_deref()
    }

    pub fn set_delivery_address(&mut self, address: Patch<String>) -> Result<(), DomainError> {
        self.ensure_not_archived()?;
        address
            .resolve("deliveryAddress", 252)?
            .apply(&mut self.delivery_address);
        Ok(())
    }
    pub fn source_roster_id(&self) -> Option<RosterId> {
        self.source_roster_id
    }
    pub fn source_vendor_id(&self) -> Option<VendorId> {
        self.source_vendor_id
    }
    pub fn vendor(&self) -> &VendorSnapshot {
        &self.vendor
    }
    pub fn created_at(&self) -> SystemTime {
        self.created_at
    }
    pub fn closed_at(&self) -> Option<SystemTime> {
        self.closed_at
    }
    pub fn participants(&self) -> &[Participant] {
        &self.participants
    }
    pub fn offered_items(&self) -> &[OfferedItem] {
        &self.offered_items
    }

    // ---- state machine: Open -> Closed -> Archived, Closed -> Open ----

    fn bad_transition(&self, to: &str) -> DomainError {
        DomainError::invalid(
            "status",
            format!("無法從 {} 變更為 {to}", self.status.as_str()),
        )
    }

    pub fn close(&mut self, now: SystemTime) -> Result<(), DomainError> {
        if self.status != GroupOrderStatus::Open {
            return Err(self.bad_transition("closed"));
        }
        self.status = GroupOrderStatus::Closed;
        self.closed_at = Some(now);
        Ok(())
    }

    /// Closed -> Open. The deadline is kept: if it already passed, the group
    /// stays unable to accept participant orders until the deadline is
    /// changed (see [`GroupOrder::accepts_orders`]).
    pub fn reopen(&mut self, _now: SystemTime) -> Result<(), DomainError> {
        if self.status != GroupOrderStatus::Closed {
            return Err(self.bad_transition("open"));
        }
        self.status = GroupOrderStatus::Open;
        self.closed_at = None;
        Ok(())
    }

    /// Closed -> Archived only (an open group must be closed first).
    pub fn archive(&mut self) -> Result<(), DomainError> {
        if self.status != GroupOrderStatus::Closed {
            return Err(self.bad_transition("archived"));
        }
        self.status = GroupOrderStatus::Archived;
        Ok(())
    }

    /// Open and the deadline (if any) has not passed.
    pub fn accepts_orders(&self, now: SystemTime) -> bool {
        self.ensure_accepting_orders(now).is_ok()
    }

    /// `Closed { reason }` with `group_closed` (closed or archived) or
    /// `deadline_passed`.
    pub fn ensure_accepting_orders(&self, now: SystemTime) -> Result<(), DomainError> {
        if self.status != GroupOrderStatus::Open {
            return Err(DomainError::Closed {
                reason: "group_closed",
            });
        }
        if self.deadline_at.is_some_and(|d| now >= d) {
            return Err(DomainError::Closed {
                reason: "deadline_passed",
            });
        }
        Ok(())
    }

    fn ensure_not_archived(&self) -> Result<(), DomainError> {
        if self.status == GroupOrderStatus::Archived {
            return Err(DomainError::invalid("status", "團購已封存，無法修改"));
        }
        Ok(())
    }

    // ---- edits ----

    /// Replace the share token; the old link stops working immediately.
    pub fn rotate_share_token(&mut self, new: ShareToken) {
        self.share_token = new;
    }

    pub fn rename(&mut self, title: &str) -> Result<(), DomainError> {
        self.title = GroupTitle::new(title)?;
        Ok(())
    }

    /// Merge-patch the group note (`Set` of blank text clears it).
    pub fn set_note(&mut self, note: Patch<String>) -> Result<(), DomainError> {
        note.resolve("note", NOTE_MAX_LEN)?.apply(&mut self.note);
        Ok(())
    }

    /// `Set` must be after `now`; `Clear` removes the deadline.
    pub fn set_deadline(
        &mut self,
        deadline: Patch<SystemTime>,
        now: SystemTime,
    ) -> Result<(), DomainError> {
        self.ensure_not_archived()?;
        match deadline {
            Patch::Keep => {}
            Patch::Clear => self.deadline_at = None,
            Patch::Set(d) if d > now => self.deadline_at = Some(d),
            Patch::Set(_) => {
                return Err(DomainError::invalid("deadlineAt", "截止時間必須晚於現在"));
            }
        }
        Ok(())
    }

    /// Add an ad-hoc participant; `NameKey` must be unique in this group.
    pub fn add_participant(&mut self, name: &str) -> Result<ParticipantId, DomainError> {
        self.ensure_not_archived()?;
        let name = PersonName::new(name)?;
        if self.participants.iter().any(|p| p.name.key() == name.key()) {
            return Err(DomainError::Conflict {
                field: "name",
                value: name.to_string(),
            });
        }
        let id = ParticipantId::new();
        let sort_order = self
            .participants
            .iter()
            .map(|p| p.sort_order)
            .max()
            .map_or(0, |m| m + 1);
        self.participants.push(Participant {
            id,
            name,
            sort_order,
        });
        Ok(id)
    }

    pub fn set_sold_out(&mut self, item: OfferedItemId, sold_out: bool) -> Result<(), DomainError> {
        self.ensure_not_archived()?;
        let it =
            self.offered_items
                .iter_mut()
                .find(|i| i.id == item)
                .ok_or(DomainError::NotFound {
                    entity: "offered_item",
                })?;
        it.sold_out = sold_out;
        Ok(())
    }

    /// Validate and build a new (append-only) submission.
    ///
    /// Participants need the group to be accepting orders; the organizer may
    /// submit in any status except Archived. Empty `lines` = cancellation.
    /// Everything is validated before anything is built.
    #[allow(clippy::too_many_arguments)]
    pub fn new_submission(
        &self,
        participant_id: ParticipantId,
        lines: &[LineInput],
        note: Option<&str>,
        actor: Actor,
        idempotency_key: Uuid,
        client_info: Option<ClientInfo>,
        now: SystemTime,
    ) -> Result<OrderSubmission, DomainError> {
        if !self.participants.iter().any(|p| p.id == participant_id) {
            return Err(DomainError::NotFound {
                entity: "participant",
            });
        }
        match actor {
            Actor::Participant => self.ensure_accepting_orders(now)?,
            Actor::Organizer if self.status == GroupOrderStatus::Archived => {
                return Err(DomainError::Closed {
                    reason: "group_archived",
                });
            }
            Actor::Organizer => {}
        }
        if lines.len() > OrderSubmission::MAX_LINES {
            return Err(DomainError::LimitExceeded {
                field: "lines",
                max: OrderSubmission::MAX_LINES,
            });
        }
        let note = optional_text("note", note, NOTE_MAX_LEN)?;
        let mut out = Vec::with_capacity(lines.len());
        for (i, l) in lines.iter().enumerate() {
            let item = self
                .offered_items
                .iter()
                .find(|o| o.id == l.offered_item_id)
                .ok_or(DomainError::NotFound {
                    entity: "offered_item",
                })?;
            if item.sold_out {
                return Err(DomainError::SoldOut {
                    item: item.name.to_string(),
                });
            }
            if !(OrderSubmission::MIN_QUANTITY..=OrderSubmission::MAX_QUANTITY)
                .contains(&l.quantity)
            {
                return Err(DomainError::invalid(
                    "quantity",
                    format!(
                        "必須介於 {} 到 {}",
                        OrderSubmission::MIN_QUANTITY,
                        OrderSubmission::MAX_QUANTITY
                    ),
                ));
            }
            out.push(SubmissionLine {
                line_no: (i + 1) as u32,
                offered_item_id: item.id,
                quantity: l.quantity,
                unit_price: item.price,
                note: optional_text(
                    "note",
                    l.note.as_deref(),
                    OrderSubmission::LINE_NOTE_MAX_LEN,
                )?,
            });
        }
        let client_info = client_info
            .map(ClientInfo::sanitize)
            .filter(|c| !c.is_empty());
        Ok(OrderSubmission::from_parts(
            SubmissionId::new(),
            self.id,
            participant_id,
            out,
            note,
            actor,
            now,
            idempotency_key,
            client_info,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::{MemberInput, MenuItemInput};
    use std::time::Duration;

    fn t(s: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(s)
    }
    fn token() -> ShareToken {
        ShareToken::from_random_bytes([7; 32])
    }
    fn roster(names: &[&str]) -> Roster {
        let mut r = Roster::new(OrganizerId::new(), "名單").unwrap();
        r.upsert_members(
            names
                .iter()
                .map(|n| MemberInput {
                    name: (*n).into(),
                    note: None,
                })
                .collect(),
        )
        .unwrap();
        r
    }
    fn vendor() -> Vendor {
        let mut v = Vendor::new(OrganizerId::new(), "池上便當").unwrap();
        v.upsert_menu_items(vec![
            MenuItemInput {
                name: "排骨飯".into(),
                price: 100,
                description: Some("香".into()),
                category: None,
            },
            MenuItemInput {
                name: "雞腿飯".into(),
                price: 120,
                description: None,
                category: None,
            },
            MenuItemInput {
                name: "舊品項".into(),
                price: 90,
                description: None,
                category: None,
            },
        ])
        .unwrap();
        let old = v
            .menu()
            .iter()
            .find(|m| m.name.as_str() == "舊品項")
            .unwrap()
            .id;
        v.archive_item(old).unwrap();
        v
    }
    fn group() -> GroupOrder {
        GroupOrder::open(
            OrganizerId::new(),
            " 週五  午餐 ",
            &roster(&["小明", "小華"]),
            &vendor(),
            None,
            Some("note"),
            token(),
            t(100),
        )
        .unwrap()
    }
    fn line(g: &GroupOrder, idx: usize, q: u32) -> LineInput {
        LineInput {
            offered_item_id: g.offered_items()[idx].id,
            quantity: q,
            note: None,
        }
    }
    fn submit(
        g: &GroupOrder,
        lines: &[LineInput],
        actor: Actor,
        now: u64,
    ) -> Result<OrderSubmission, DomainError> {
        g.new_submission(
            g.participants()[0].id,
            lines,
            None,
            actor,
            Uuid::now_v7(),
            None,
            t(now),
        )
    }

    #[test]
    fn set_note_is_a_merge_patch() {
        let mut g = group();
        g.set_note(Patch::Keep).unwrap();
        assert_eq!(g.note(), Some("note"));
        g.set_note(Patch::Set("  新備註 ".into())).unwrap();
        assert_eq!(g.note(), Some("新備註"));
        assert!(g.set_note(Patch::Set("字".repeat(500))).is_err());
        assert_eq!(g.note(), Some("新備註"));
        g.set_note(Patch::Clear).unwrap();
        assert_eq!(g.note(), None);
    }

    #[test]
    fn open_snapshots() {
        let g = group();
        assert_eq!(g.title().as_str(), "週五 午餐");
        assert_eq!(g.status(), GroupOrderStatus::Open);
        assert_eq!(g.participants().len(), 2);
        assert_eq!(g.offered_items().len(), 2);
        assert!(
            g.offered_items()
                .iter()
                .all(|i| i.name.as_str() != "舊品項")
        );
        assert_eq!(g.vendor().name, "池上便當");
        assert_eq!(g.created_at(), t(100));
        assert_ne!(g.participants()[0].id, g.participants()[1].id);
    }

    #[test]
    fn open_rejections() {
        let open = |title: &str, r: &Roster, v: &Vendor, d| {
            GroupOrder::open(OrganizerId::new(), title, r, v, d, None, token(), t(100))
        };
        let (r, v) = (roster(&["a"]), vendor());
        assert!(open("ok", &r, &v, Some(t(101))).is_ok());
        assert!(matches!(
            open("ok", &roster(&[]), &v, None),
            Err(DomainError::Invalid {
                field: "rosterId",
                ..
            })
        ));
        let empty_vendor = Vendor::new(OrganizerId::new(), "x").unwrap();
        assert!(matches!(
            open("ok", &r, &empty_vendor, None),
            Err(DomainError::Invalid {
                field: "vendorId",
                ..
            })
        ));
        assert!(matches!(
            open("ok", &r, &v, Some(t(100))),
            Err(DomainError::Invalid {
                field: "deadlineAt",
                ..
            })
        ));
        assert!(open("ok", &r, &v, Some(t(50))).is_err());
        assert!(open("  ", &r, &v, None).is_err());
        assert!(open(&"字".repeat(51), &r, &v, None).is_err());
        assert!(open(&"字".repeat(50), &r, &v, None).is_ok());
    }

    #[test]
    fn state_machine() {
        let mut g = group();
        assert!(g.reopen(t(1)).is_err());
        assert!(g.archive().is_err());
        g.close(t(200)).unwrap();
        assert_eq!(g.status(), GroupOrderStatus::Closed);
        assert_eq!(g.closed_at(), Some(t(200)));
        assert!(g.close(t(201)).is_err());
        g.reopen(t(300)).unwrap();
        assert_eq!(g.status(), GroupOrderStatus::Open);
        assert_eq!(g.closed_at(), None);
        g.close(t(400)).unwrap();
        g.archive().unwrap();
        assert_eq!(g.status(), GroupOrderStatus::Archived);
        for r in [g.close(t(1)), g.reopen(t(1)), g.archive()] {
            assert!(matches!(
                r,
                Err(DomainError::Invalid {
                    field: "status",
                    ..
                })
            ));
        }
    }

    #[test]
    fn status_strings() {
        for s in ["open", "closed", "archived"] {
            assert_eq!(GroupOrderStatus::parse(s).unwrap().as_str(), s);
        }
        assert!(GroupOrderStatus::parse("x").is_err());
        assert_eq!(Actor::Organizer.as_str(), "organizer");
        assert_eq!(Actor::parse("participant").unwrap(), Actor::Participant);
    }

    #[test]
    fn accepts_orders_with_deadline() {
        let mut g = group();
        assert!(g.accepts_orders(t(1_000_000)));
        g.set_deadline(Patch::Set(t(500)), t(100)).unwrap();
        assert!(g.accepts_orders(t(499)));
        assert!(!g.accepts_orders(t(500)));
        assert_eq!(
            g.ensure_accepting_orders(t(600)),
            Err(DomainError::Closed {
                reason: "deadline_passed"
            })
        );
        g.close(t(300)).unwrap();
        assert_eq!(
            g.ensure_accepting_orders(t(400)),
            Err(DomainError::Closed {
                reason: "group_closed"
            })
        );
        assert!(g.set_deadline(Patch::Set(t(50)), t(100)).is_err());
        g.set_deadline(Patch::Clear, t(100)).unwrap();
        assert_eq!(g.deadline_at(), None);
    }

    #[test]
    fn submission_happy_path_and_cancellation() {
        let g = group();
        let mut l2 = line(&g, 1, 1);
        l2.note = Some("  不要辣 ".into());
        let s = submit(&g, &[line(&g, 0, 2), l2], Actor::Participant, 150).unwrap();
        assert_eq!(s.total(), Money::new(320));
        assert_eq!(s.lines()[0].line_no, 1);
        assert_eq!(s.lines()[1].line_no, 2);
        assert_eq!(s.lines()[1].note.as_deref(), Some("不要辣"));
        assert!(!s.is_cancellation());
        assert_eq!(s.submitted_at(), t(150));
        let c = submit(&g, &[], Actor::Participant, 160).unwrap();
        assert!(c.is_cancellation());
        assert_eq!(c.total(), Money::zero());
        // same item on several lines is fine
        assert!(submit(&g, &[line(&g, 0, 1), line(&g, 0, 1)], Actor::Participant, 1).is_ok());
    }

    #[test]
    fn submission_validation() {
        let mut g = group();
        // foreign participant / item
        assert_eq!(
            g.new_submission(
                ParticipantId::new(),
                &[],
                None,
                Actor::Participant,
                Uuid::now_v7(),
                None,
                t(150)
            ),
            Err(DomainError::NotFound {
                entity: "participant"
            })
        );
        let other = group();
        assert_eq!(
            submit(&g, &[line(&other, 0, 1)], Actor::Participant, 150),
            Err(DomainError::NotFound {
                entity: "offered_item"
            })
        );
        // quantity
        for q in [0, 21] {
            assert!(matches!(
                submit(&g, &[line(&g, 0, q)], Actor::Participant, 150),
                Err(DomainError::Invalid {
                    field: "quantity",
                    ..
                })
            ));
        }
        assert!(submit(&g, &[line(&g, 0, 20)], Actor::Participant, 150).is_ok());
        // lines limit
        let l30: Vec<_> = (0..30).map(|_| line(&g, 0, 1)).collect();
        assert!(submit(&g, &l30, Actor::Participant, 150).is_ok());
        let l31: Vec<_> = (0..31).map(|_| line(&g, 0, 1)).collect();
        assert!(matches!(
            submit(&g, &l31, Actor::Participant, 150),
            Err(DomainError::LimitExceeded {
                field: "lines",
                max: 30
            })
        ));
        // notes
        let mut ln = line(&g, 0, 1);
        ln.note = Some("字".repeat(51));
        assert!(submit(&g, &[ln], Actor::Participant, 150).is_err());
        assert!(
            g.new_submission(
                g.participants()[0].id,
                &[],
                Some(&"a".repeat(201)),
                Actor::Participant,
                Uuid::now_v7(),
                None,
                t(150)
            )
            .is_err()
        );
        // sold out
        let id = g.offered_items()[0].id;
        g.set_sold_out(id, true).unwrap();
        assert!(matches!(
            submit(&g, &[line(&g, 0, 1)], Actor::Participant, 150),
            Err(DomainError::SoldOut { .. })
        ));
        assert!(submit(&g, &[line(&g, 1, 1)], Actor::Participant, 150).is_ok());
        g.set_sold_out(id, false).unwrap();
        assert!(submit(&g, &[line(&g, 0, 1)], Actor::Participant, 150).is_ok());
        assert!(g.set_sold_out(OfferedItemId::new(), true).is_err());
    }

    #[test]
    fn status_gates_submissions() {
        let mut g = group();
        let l = [line(&g, 0, 1)];
        g.close(t(200)).unwrap();
        assert!(matches!(
            submit(&g, &l, Actor::Participant, 210),
            Err(DomainError::Closed { .. })
        ));
        assert!(submit(&g, &l, Actor::Organizer, 210).is_ok());
        g.archive().unwrap();
        assert!(submit(&g, &l, Actor::Participant, 220).is_err());
        assert_eq!(
            submit(&g, &l, Actor::Organizer, 220),
            Err(DomainError::Closed {
                reason: "group_archived"
            })
        );
        // organizer bypasses an expired deadline
        let mut g = group();
        let l = [line(&g, 0, 1)];
        g.set_deadline(Patch::Set(t(150)), t(100)).unwrap();
        assert!(submit(&g, &l, Actor::Participant, 200).is_err());
        assert!(submit(&g, &l, Actor::Organizer, 200).is_ok());
    }

    #[test]
    fn unit_price_is_copied() {
        let mut g = group();
        let s = submit(&g, &[line(&g, 0, 2)], Actor::Participant, 150).unwrap();
        g.offered_items[0].price = Money::new(999);
        assert_eq!(s.lines()[0].unit_price, Money::new(100));
        assert_eq!(s.total(), Money::new(200));
        let s2 = submit(&g, &[line(&g, 0, 1)], Actor::Participant, 150).unwrap();
        assert_eq!(s2.total(), Money::new(999));
    }

    #[test]
    fn client_info_is_sanitized_on_submission() {
        let g = group();
        let ci = ClientInfo {
            device_id: Some("a/b".into()),
            ..Default::default()
        };
        let s = g
            .new_submission(
                g.participants()[0].id,
                &[],
                None,
                Actor::Participant,
                Uuid::now_v7(),
                Some(ci),
                t(150),
            )
            .unwrap();
        assert_eq!(s.client_info().unwrap().device_id.as_deref(), Some("ab"));
        let s = g
            .new_submission(
                g.participants()[0].id,
                &[],
                None,
                Actor::Participant,
                Uuid::now_v7(),
                Some(ClientInfo::default()),
                t(150),
            )
            .unwrap();
        assert!(s.client_info().is_none());
    }

    #[test]
    fn effective_order_is_latest_created() {
        let g = group();
        let subs: Vec<_> = (0..4)
            .map(|i| submit(&g, &[line(&g, 0, 1 + i)], Actor::Participant, 150).unwrap())
            .collect();
        let mut shuffled = vec![
            subs[2].clone(),
            subs[3].clone(),
            subs[0].clone(),
            subs[1].clone(),
        ];
        assert_eq!(
            super::super::EffectiveOrder::latest(&shuffled)
                .unwrap()
                .id(),
            subs[3].id()
        );
        shuffled.retain(|s| s.id() != subs[3].id());
        assert_ne!(
            super::super::EffectiveOrder::latest(&shuffled)
                .unwrap()
                .id(),
            subs[3].id()
        );
        assert!(super::super::EffectiveOrder::latest(&[]).is_none());
    }

    #[test]
    fn participants_and_token() {
        let mut g = group();
        assert!(matches!(
            g.add_participant("小明 "),
            Err(DomainError::Conflict { field: "name", .. })
        ));
        let pid = g.add_participant("臨時").unwrap();
        assert_eq!(g.participants().last().unwrap().id, pid);
        assert_eq!(g.participants().last().unwrap().sort_order, 2);
        let old = g.share_token().clone();
        g.rotate_share_token(ShareToken::from_random_bytes([9; 32]));
        assert_ne!(*g.share_token(), old);
        g.rename("新名稱").unwrap();
        assert!(g.rename("").is_err());
        g.close(t(1)).unwrap();
        g.archive().unwrap();
        assert!(g.add_participant("另一個").is_err());
    }
}
