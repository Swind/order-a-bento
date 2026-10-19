//! HTTP route modules. Business routes live under `/api/v1`.

pub mod ai_import;
pub mod auth;
pub mod common;
pub mod delivery_addresses;
pub mod group_orders;
pub mod health;
pub mod me;
pub mod public;
pub mod rosters;
pub mod vendors;

#[cfg(test)]
mod ai_import_tests;
#[cfg(test)]
mod directory_tests;
#[cfg(test)]
mod management_tests;
#[cfg(test)]
mod ordering_tests;
