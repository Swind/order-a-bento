//! AI import adapters: the rig agent (Anthropic / OpenAI-compatible), its tool wrappers and the
//! demo agent used for local development without an API key.
//!
//! Only this module (and the `server` wiring) knows about rig.

mod demo;
mod rig_agent;
mod tools;

pub use demo::DemoImportAgent;
pub use rig_agent::{AiProvider, AiReasoning, ExtraHeader, RigAgentConfig, RigImportAgent};
pub use tools::{import_tools_parameters, strict_schema};
