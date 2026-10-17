//! Shared kernel: ids, clock, errors, money, names.

mod clock;
mod error;
mod id;
mod money;
mod name;
mod patch;
pub mod text;

pub use clock::{Clock, FixedClock, SystemClock};
pub use error::DomainError;
pub use id::Id;
pub use money::Money;
pub use name::{ItemName, NameKey, PersonName, RosterName, VendorName};
pub use patch::{Patch, Resolved};
