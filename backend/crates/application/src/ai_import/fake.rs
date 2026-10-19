//! Scripted [`ImportAgent`] for tests and for dev mode without an API key.

use std::sync::Arc;

use async_trait::async_trait;
use domain::ai_import::{ImportMode, Usage};

use super::tools::{
    self, GetRosterArgs, GetVendorArgs, ProposeRosterMembersArgs, ProposeVendorArgs,
};
use super::{AgentInput, AgentOutcome, AiError, ImportAgent, ImportToolContext};

/// One scripted tool invocation.
#[derive(Debug, Clone)]
pub enum FakeToolCall {
    ProposeDeliveryAddress(tools::ProposeDeliveryAddressArgs),
    ListRosters,
    GetRoster(GetRosterArgs),
    ListVendors,
    GetVendor(GetVendorArgs),
    ProposeRosterMembers(ProposeRosterMembersArgs),
    ProposeVendor(ProposeVendorArgs),
}

impl FakeToolCall {
    /// The upload mode whose agent has this tool (the real agent only gets the
    /// tools of the chosen mode).
    fn mode(&self) -> ImportMode {
        match self {
            Self::ProposeDeliveryAddress(_) => ImportMode::DeliveryAddress,
            Self::ListRosters | Self::GetRoster(_) | Self::ProposeRosterMembers(_) => {
                ImportMode::Roster
            }
            Self::ListVendors | Self::GetVendor(_) | Self::ProposeVendor(_) => ImportMode::Vendor,
        }
    }
}

/// Calls the real tool functions in order and returns a fixed summary. It
/// ignores the image and instruction. A call whose tool does not exist in the
/// run's mode fails like an unknown tool would.
#[derive(Debug, Clone)]
pub struct FakeImportAgent {
    pub script: Vec<FakeToolCall>,
    pub summary: String,
    pub usage: Usage,
}

impl FakeImportAgent {
    pub fn new(script: Vec<FakeToolCall>, summary: impl Into<String>) -> Self {
        Self {
            script,
            summary: summary.into(),
            usage: Usage {
                input_tokens: 1000,
                output_tokens: 200,
            },
        }
    }

    /// Agent that records nothing and says so (dev mode placeholder).
    pub fn empty() -> Self {
        Self::new(
            Vec::new(),
            "（未設定 AI 金鑰，這是模擬結果；沒有提出任何變更）",
        )
    }
}

#[async_trait]
impl ImportAgent for FakeImportAgent {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        for call in self.script.iter().cloned() {
            if call.mode() != input.mode {
                return Err(AiError::Unavailable {
                    retryable: false,
                    detail: format!("tool {call:?} is not available in {:?} mode", input.mode),
                });
            }
            let r = match call {
                FakeToolCall::ProposeDeliveryAddress(a) => {
                    tools::propose_delivery_address(&ctx, a).await.map(drop)
                }
                FakeToolCall::ListRosters => tools::list_rosters(&ctx).await.map(drop),
                FakeToolCall::GetRoster(a) => tools::get_roster(&ctx, a).await.map(drop),
                FakeToolCall::ListVendors => tools::list_vendors(&ctx).await.map(drop),
                FakeToolCall::GetVendor(a) => tools::get_vendor(&ctx, a).await.map(drop),
                FakeToolCall::ProposeRosterMembers(a) => {
                    tools::propose_roster_members(&ctx, a).await.map(drop)
                }
                FakeToolCall::ProposeVendor(a) => tools::propose_vendor(&ctx, a).await.map(drop),
            };
            r.map_err(|e| AiError::Unavailable {
                retryable: false,
                detail: e.to_string(),
            })?;
        }
        Ok(AgentOutcome {
            summary: self.summary.clone(),
            usage: self.usage,
            model: "fake".into(),
        })
    }
}
