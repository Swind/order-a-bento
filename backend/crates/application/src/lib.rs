//! Application layer: use cases and ports (traits) built on top of `domain`.
//!
//! Ports use `#[async_trait]` (object-safe, `Send` futures) so adapters can be
//! injected as `Arc<dyn Trait>` and replaced by fakes in tests.

pub mod ai_import;
pub mod auth;
pub mod directory;
pub mod maintenance;
pub mod ordering;
pub mod ports;
pub mod token;

pub use ai_import::{
    AcceptedChange, AiError, AiImportConfig, AiImportError, AiImportService, ApplyCmd, ApplyResult,
    ImportAgent, ImportRunView, ImportToolContext, StartImportCmd,
};
pub use auth::accounts::{AccountError, AccountService, BootstrapOutcome};
pub use auth::{AuthConfig, AuthError, AuthService};
pub use directory::{DirectoryError, DirectoryService, NewVendor};
pub use maintenance::{Maintenance, MaintenanceRepo, MaintenanceReport};
pub use ordering::{
    GroupSummary, OpenGroupCmd, OrderingError, OrderingService, OrganizerSubmitCmd,
    PublicGroupView, SubmitOrderCmd, UpdateGroupCmd,
};
pub mod sharing;
