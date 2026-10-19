//! Infrastructure layer: adapters implementing application ports.

pub mod ai;
pub mod ai_import_repo;
pub mod delivery_address_repo;
pub mod directory_repo;
pub mod identity_repo;
pub mod maintenance_repo;
pub mod oidc;
pub mod ordering_repo;
pub mod password;
pub mod sqlite;
pub mod timefmt;
pub mod tokens;
pub mod ua;

#[cfg(test)]
mod ai_import_tests;
