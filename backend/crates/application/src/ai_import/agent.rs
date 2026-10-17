//! The `ImportAgent` port and the context shared with the agent's tools.

use std::sync::Arc;

use async_trait::async_trait;
use domain::ai_import::{ImportMode, ImportRunId, Proposal, Usage};
use domain::directory::{RosterId, VendorId};
use domain::identity::OrganizerId;

use crate::ports::{Clock, RosterRepo, VendorRepo};

/// State handed to the agent's tools. `organizer_id` and `run_id` are injected
/// by the backend, never tool parameters: the model cannot address other
/// organizers' data.
pub struct ImportToolContext {
    pub organizer_id: OrganizerId,
    pub run_id: ImportRunId,
    pub rosters: Arc<dyn RosterRepo>,
    pub vendors: Arc<dyn VendorRepo>,
    pub clock: Arc<dyn Clock>,
    /// Pending changes; the mutex serializes concurrent `propose_*` calls so
    /// "latest proposal for a target wins" stays well defined.
    pub proposal: tokio::sync::Mutex<Proposal>,
    /// Token usage reported so far (all attempts). Agents add to it as turns
    /// finish so that a failed or timed-out run still has its usage.
    usage: std::sync::Mutex<Usage>,
    /// Tool calls made so far, as `{ "name", "args" }` (debug log).
    tool_calls: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl ImportToolContext {
    pub fn new(
        organizer_id: OrganizerId,
        run_id: ImportRunId,
        rosters: Arc<dyn RosterRepo>,
        vendors: Arc<dyn VendorRepo>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            organizer_id,
            run_id,
            rosters,
            vendors,
            clock,
            proposal: tokio::sync::Mutex::new(Proposal::default()),
            usage: std::sync::Mutex::new(Usage::default()),
            tool_calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Add the usage of one finished model turn.
    pub fn add_usage(&self, u: Usage) {
        let mut g = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        g.input_tokens += u.input_tokens;
        g.output_tokens += u.output_tokens;
    }

    /// Usage reported through [`Self::add_usage`] (all attempts).
    pub fn usage(&self) -> Usage {
        *self.usage.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record a tool invocation (name and arguments as the model sent them).
    pub fn record_tool_call(&self, name: &str, args: serde_json::Value) {
        self.tool_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(serde_json::json!({ "name": name, "args": args }));
    }

    /// The recorded tool calls as a JSON array, `None` when there were none.
    pub fn tool_calls_json(&self) -> Option<String> {
        let g = self.tool_calls.lock().unwrap_or_else(|e| e.into_inner());
        (!g.is_empty()).then(|| serde_json::Value::Array(g.clone()).to_string())
    }

    /// Copy of the proposal accumulated so far.
    pub async fn snapshot(&self) -> Proposal {
        self.proposal.lock().await.clone()
    }

    /// Drop everything proposed so far (used before retrying the agent).
    /// Usage and the tool-call log are kept: they describe what happened.
    pub async fn reset(&self) {
        *self.proposal.lock().await = Proposal::default();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImageInput {
    pub bytes: Vec<u8>,
    /// `image/jpeg`, `image/png` or `image/webp`.
    pub media_type: String,
}

impl std::fmt::Debug for ImageInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ImageInput({} bytes, {})",
            self.bytes.len(),
            self.media_type
        )
    }
}

#[derive(Debug, Clone)]
pub struct AgentInput {
    pub image: ImageInput,
    pub instruction: Option<String>,
    pub mode_hint: Option<ImportMode>,
    pub target_roster_id: Option<RosterId>,
    pub target_vendor_id: Option<VendorId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOutcome {
    /// The agent's closing summary (Traditional Chinese, 1-3 sentences).
    pub summary: String,
    pub usage: Usage,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    /// The model refused (safety classifier); `category` when known.
    #[error("the model refused to process the image")]
    Refused { category: Option<String> },
    /// Provider/network failure. `retryable`: 529, 5xx, connection errors.
    #[error("AI service unavailable (retryable={retryable}): {detail}")]
    Unavailable { retryable: bool, detail: String },
    #[error("AI import timed out")]
    Timeout,
    #[error("invalid image: {0}")]
    InvalidImage(String),
    #[error("AI quota exceeded")]
    QuotaExceeded,
}

/// The AI agent. It looks at the image, calls the tools in
/// [`super::tools`] against `ctx` (read data, record proposals) and returns
/// its closing summary. Implementations: rig-based (infrastructure, T4.2) and
/// [`super::FakeImportAgent`].
#[async_trait]
pub trait ImportAgent: Send + Sync {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError>;
}
