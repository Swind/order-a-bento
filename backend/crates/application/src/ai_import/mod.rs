//! AI import use cases: the `ImportAgent` port, the agent's tools, the system
//! prompt and the `AiImportService` (start / get / apply / discard).

mod agent;
mod fake;
pub mod prompt;
mod service;
pub mod tools;

pub use agent::{
    AgentInput, AgentOutcome, AiError, EditSession, ImageInput, ImportAgent, ImportToolContext,
    ObservedUsage, TurnDraftOutcome, TurnExecution, TurnMetrics,
};
mod draft_service;
pub use draft_service::DraftTurnCmd;
pub use fake::{FakeImportAgent, FakeToolCall};
pub use service::{
    AcceptedChange, AcceptedDeliveryAddress, AcceptedMember, AcceptedMenuItem, AcceptedRoster,
    AcceptedVendor, AcceptedVendorInfo, AiImportConfig, AiImportError, AiImportService, ApplyCmd,
    ApplyResult, ChangeError, ImportRunView, StartImportCmd,
};
