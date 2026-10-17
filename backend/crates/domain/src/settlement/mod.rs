//! Settlement context: pickup / payment tracking per participant.

use std::time::SystemTime;

use crate::ordering::{GroupOrderId, ParticipantId};
use crate::shared::Money;

/// Key = `(group_order_id, participant_id)`. Only the organizer edits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    group_order_id: GroupOrderId,
    participant_id: ParticipantId,
    picked_up_at: Option<SystemTime>,
    paid_at: Option<SystemTime>,
    paid_amount: Option<Money>,
}

impl Settlement {
    pub fn new(group_order_id: GroupOrderId, participant_id: ParticipantId) -> Self {
        Self {
            group_order_id,
            participant_id,
            picked_up_at: None,
            paid_at: None,
            paid_amount: None,
        }
    }

    pub fn from_parts(
        group_order_id: GroupOrderId,
        participant_id: ParticipantId,
        picked_up_at: Option<SystemTime>,
        paid_at: Option<SystemTime>,
        paid_amount: Option<Money>,
    ) -> Self {
        Self {
            group_order_id,
            participant_id,
            picked_up_at,
            paid_at,
            paid_amount,
        }
    }

    pub fn group_order_id(&self) -> GroupOrderId {
        self.group_order_id
    }
    pub fn participant_id(&self) -> ParticipantId {
        self.participant_id
    }
    pub fn picked_up_at(&self) -> Option<SystemTime> {
        self.picked_up_at
    }
    pub fn paid_at(&self) -> Option<SystemTime> {
        self.paid_at
    }
    pub fn paid_amount(&self) -> Option<Money> {
        self.paid_amount
    }

    /// Idempotent set (not a toggle).
    ///
    /// - `true` when already set: the original timestamp (and, for payment,
    ///   `paid_amount`) is kept, so a later change in the amount due stays
    ///   visible as 尚差/應退 rather than being silently re-baselined.
    /// - `true` when unset: records `now` (and `amount_due` as `paid_amount`).
    /// - `false`: clears the state (payment clears both `paid_at` and `paid_amount`).
    pub fn set(&mut self, picked_up: bool, paid: bool, amount_due: Money, now: SystemTime) {
        match (picked_up, self.picked_up_at) {
            (true, None) => self.picked_up_at = Some(now),
            (false, _) => self.picked_up_at = None,
            _ => {}
        }
        match (paid, self.paid_at) {
            (true, None) => {
                self.paid_at = Some(now);
                self.paid_amount = Some(amount_due);
            }
            (false, _) => {
                self.paid_at = None;
                self.paid_amount = None;
            }
            _ => {}
        }
    }

    /// `amount_due - paid_amount` when paid: positive = 尚差, negative = 應退,
    /// 0 = settled. `None` when unpaid.
    pub fn balance(&self, amount_due: Money) -> Option<i64> {
        self.paid_amount.map(|p| amount_due.get() - p.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t(s: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(s)
    }
    fn st() -> Settlement {
        Settlement::new(GroupOrderId::new(), ParticipantId::new())
    }

    #[test]
    fn set_is_idempotent_and_clears() {
        let mut s = st();
        s.set(true, true, Money::new(100), t(10));
        assert_eq!(s.picked_up_at(), Some(t(10)));
        assert_eq!(s.paid_at(), Some(t(10)));
        assert_eq!(s.paid_amount(), Some(Money::new(100)));
        s.set(true, true, Money::new(150), t(20));
        assert_eq!(s.picked_up_at(), Some(t(10)));
        assert_eq!(s.paid_at(), Some(t(10)));
        assert_eq!(s.paid_amount(), Some(Money::new(100)));
        s.set(true, false, Money::new(150), t(30));
        assert_eq!(s.picked_up_at(), Some(t(10)));
        assert_eq!((s.paid_at(), s.paid_amount()), (None, None));
        s.set(false, false, Money::zero(), t(40));
        assert_eq!(s.picked_up_at(), None);
        s.set(false, true, Money::new(150), t(50));
        assert_eq!(s.paid_amount(), Some(Money::new(150)));
    }

    #[test]
    fn balance_cases() {
        let mut s = st();
        assert_eq!(s.balance(Money::new(100)), None);
        s.set(false, true, Money::new(100), t(1));
        assert_eq!(s.balance(Money::new(100)), Some(0));
        assert_eq!(s.balance(Money::new(130)), Some(30));
        assert_eq!(s.balance(Money::new(0)), Some(-100));
    }
}
