use std::{iter::Sum, ops::Add};

use super::DomainError;

/// TWD whole dollars. The type itself is unbounded (totals may exceed
/// `MAX_PRICE`); use [`Money::try_new`] to validate a *unit price*
/// (`MIN_PRICE..=MAX_PRICE`) and [`Money::new`] for unchecked amounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64);

impl Money {
    pub const MIN_PRICE: i64 = 1;
    pub const MAX_PRICE: i64 = 100_000;

    /// Unchecked amount (totals, differences).
    pub const fn new(v: i64) -> Self {
        Self(v)
    }
    pub const fn zero() -> Self {
        Self(0)
    }
    /// Validated unit price.
    pub fn try_new(v: i64) -> Result<Self, DomainError> {
        if (Self::MIN_PRICE..=Self::MAX_PRICE).contains(&v) {
            Ok(Self(v))
        } else {
            Err(DomainError::invalid(
                "price",
                format!("必須介於 {} 到 {}", Self::MIN_PRICE, Self::MAX_PRICE),
            ))
        }
    }
    pub const fn get(self) -> i64 {
        self.0
    }
    pub fn checked_mul(self, qty: i64) -> Option<Self> {
        self.0.checked_mul(qty).map(Self)
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, o: Money) -> Money {
        Money(self.0 + o.0)
    }
}
impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(i: I) -> Money {
        i.fold(Money::zero(), Add::add)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_bounds() {
        assert!(Money::try_new(0).is_err());
        assert!(Money::try_new(-5).is_err());
        assert_eq!(Money::try_new(1).unwrap().get(), 1);
        assert_eq!(Money::try_new(100_000).unwrap().get(), 100_000);
        assert!(Money::try_new(100_001).is_err());
    }

    #[test]
    fn arithmetic() {
        let m = Money::try_new(100_000).unwrap();
        assert_eq!(m.checked_mul(3), Some(Money::new(300_000)));
        assert_eq!(Money::new(i64::MAX).checked_mul(2), None);
        let s: Money = [Money::new(5), Money::new(7)].into_iter().sum();
        assert_eq!(s, Money::new(12));
        assert_eq!(Money::zero() + s, s);
    }
}
