//! [`ImportAgent`] backed by rig + Anthropic (design 9).
//!
//! The agent is built per request (cheap: ~0.4 ms, the HTTP pool is shared by
//! rig). The tools hold the request's `Arc<ImportToolContext>`, so they can only
//! reach the requesting organizer's data and record into that run's proposal.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use application::ai_import::prompt::import_agent_system_prompt;
use application::ai_import::{AgentInput, AgentOutcome, AiError, ImportAgent, ImportToolContext};
use async_trait::async_trait;
use domain::ai_import::{ImportMode, Usage};
use rig::AgentBuilder;
use rig::agent::{AgentHook, HookContext, ModelTurnAction, ModelTurnFinished};
use rig::completion::{Effort, Reasoning};
use rig::message::{ImageMediaType, UserContent};
use rig::providers::anthropic::AnthropicConfig;
use rig::providers::anthropic::extension::{
    AnthropicOptions, Fallbacks, SERVER_SIDE_FALLBACK_BETA,
};

use super::tools::add_import_tools;

/// Cap on a single model turn (summary plus a few dozen tool-call items).
const MAX_TOKENS: u64 = 4096;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct RigAgentConfig {
    pub api_key: String,
    /// Override of the API origin (tests, gateways); `None` = Anthropic.
    pub base_url: Option<String>,
    pub model: String,
    /// Total model-call budget per run (`AI_MAX_TURNS`).
    pub max_turns: usize,
    /// Ask the API to fall back to another model on a refusal
    /// (`server-side-fallback` beta, `AI_FALLBACKS`).
    pub fallbacks: bool,
    /// Timeout of one HTTP request to the API.
    pub request_timeout: Duration,
}

impl RigAgentConfig {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: None,
            model: model.into(),
            max_turns: 8,
            fallbacks: true,
            request_timeout: Duration::from_secs(60),
        }
    }
}

// The API key must never reach logs.
impl std::fmt::Debug for RigAgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RigAgentConfig")
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_turns", &self.max_turns)
            .field("fallbacks", &self.fallbacks)
            .finish()
    }
}

pub struct RigImportAgent {
    config: RigAgentConfig,
    http: reqwest::Client,
}

impl RigImportAgent {
    pub fn new(config: RigAgentConfig) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| format!("building the HTTP client: {e}"))?;
        Ok(Self { config, http })
    }
}

/// What the hook saw, shared with the code that maps the final result.
#[derive(Default)]
struct Seen {
    /// `Some(category)` once a turn ended with `stop_reason == "refusal"`.
    refusal: Option<Option<String>>,
    /// `model` of the last response (differs from the request after a fallback).
    model: Option<String>,
}

#[derive(Clone)]
struct TurnHook {
    ctx: Arc<ImportToolContext>,
    seen: Arc<Mutex<Seen>>,
}

impl AgentHook for TurnHook {
    fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> impl std::future::Future<Output = ModelTurnAction> + Send {
        // Usage per turn, so that failed runs still report what they cost.
        self.ctx.add_usage(Usage {
            input_tokens: event.usage.input_tokens.unwrap_or(0),
            output_tokens: event.usage.output_tokens.unwrap_or(0),
        });
        let raw = event.raw;
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = raw.get("model").and_then(|m| m.as_str()) {
            seen.model = Some(m.to_owned());
        }
        let refused = raw.get("stop_reason").and_then(|s| s.as_str()) == Some("refusal")
            || matches!(
                event.finish_reason,
                Some(rig::completion::FinishReason::ContentFilter)
            );
        if refused {
            let category = raw
                .pointer("/stop_details/category")
                .and_then(|c| c.as_str())
                .map(str::to_owned);
            seen.refusal = Some(category);
        }
        async { ModelTurnAction::Continue }
    }
}

fn user_message(input: &AgentInput) -> String {
    let mut lines = vec!["請處理這張照片。".to_string()];
    if let Some(mode) = input.mode_hint {
        lines.push(format!(
            "模式提示：{}",
            match mode {
                ImportMode::Roster => "roster",
                ImportMode::Vendor => "vendor",
            }
        ));
    }
    if let Some(id) = input.target_roster_id {
        lines.push(format!("目標名單 id：{id}"));
    }
    if let Some(id) = input.target_vendor_id {
        lines.push(format!("目標店家 id：{id}"));
    }
    if let Some(i) = &input.instruction {
        lines.push(format!("開團者指示：{i}"));
    }
    lines.join("\n")
}

fn image_media_type(media_type: &str) -> Result<ImageMediaType, AiError> {
    match media_type {
        "image/jpeg" => Ok(ImageMediaType::JPEG),
        "image/png" => Ok(ImageMediaType::PNG),
        "image/webp" => Ok(ImageMediaType::WEBP),
        other => Err(AiError::InvalidImage(format!("unsupported type {other}"))),
    }
}

/// Error text safe to store and show: no provider body, no secrets.
fn unavailable(retryable: bool, detail: impl Into<String>) -> AiError {
    AiError::Unavailable {
        retryable,
        detail: detail.into(),
    }
}

fn map_error(e: rig::completion::PromptError, seen: &Seen) -> AiError {
    if let Some(category) = &seen.refusal {
        return AiError::Refused {
            category: category.clone(),
        };
    }
    use rig::completion::PromptError;
    match e {
        PromptError::Report(r) => {
            let status = r.http_status;
            let code = r.code.as_deref().unwrap_or("-");
            match status {
                Some(401 | 403) => {
                    tracing::error!(status, "AI provider rejected the credentials");
                    unavailable(
                        false,
                        format!("authentication failed (http {})", status.unwrap_or(0)),
                    )
                }
                _ => unavailable(
                    r.retryable,
                    format!(
                        "provider error ({:?}, http {}, code {code})",
                        r.kind,
                        status.map_or("-".to_string(), |s| s.to_string())
                    ),
                ),
            }
        }
        PromptError::MaxTurns { max_turns, .. } => {
            unavailable(false, format!("agent exceeded {max_turns} model calls"))
        }
        other => {
            // Message only: provider variants never carry the request body.
            let text = other.to_string();
            if text.contains("finish_reason=ContentFilter") {
                return AiError::Refused { category: None };
            }
            unavailable(false, format!("agent failed: {text}"))
        }
    }
}

#[async_trait]
impl ImportAgent for RigImportAgent {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        let cfg = &self.config;
        let media_type = image_media_type(&input.image.media_type)?;

        let mut provider = AnthropicConfig::new(&cfg.api_key);
        if let Some(url) = &cfg.base_url {
            provider = provider.with_base_url(url);
        }
        if cfg.fallbacks {
            provider = provider.with_beta(SERVER_SIDE_FALLBACK_BETA);
        }
        let client = provider.connect(rig::http_client::ReqwestClient::from(self.http.clone()));
        let mut model = client.completion(cfg.model.as_str());
        model.wire = model.wire.with_strict_tools();

        let seen = Arc::new(Mutex::new(Seen::default()));
        let hook = TurnHook {
            ctx: ctx.clone(),
            seen: seen.clone(),
        };
        let mut builder = AgentBuilder::new(model)
            .preamble(import_agent_system_prompt())
            .reasoning(Reasoning::Effort(Effort::Low))
            .max_tokens(MAX_TOKENS)
            .default_max_turns(cfg.max_turns)
            .add_hook(hook);
        if cfg.fallbacks {
            builder =
                builder.provider_option(AnthropicOptions::default().fallbacks(Fallbacks::Default));
        }
        let agent = add_import_tools!(builder, ctx).build();

        let image = UserContent::image_raw(input.image.bytes.clone(), Some(media_type), None);
        let run = agent
            .prompt(user_message(&input))
            .attach_files([image])
            .map_err(|e| AiError::InvalidImage(e.to_string()))?
            .await;

        let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
        match run {
            Ok(r) => {
                if seen.refusal.is_some() {
                    // Defensive: a refusal as the last turn normally errors above.
                    return Err(AiError::Refused {
                        category: seen.refusal.clone().flatten(),
                    });
                }
                Ok(AgentOutcome {
                    summary: r.output().trim().to_string(),
                    usage: Usage {
                        input_tokens: r.usage.input_tokens.unwrap_or(0),
                        output_tokens: r.usage.output_tokens.unwrap_or(0),
                    },
                    model: seen.model.clone().unwrap_or_else(|| cfg.model.clone()),
                })
            }
            Err(e) => Err(map_error(e, &seen)),
        }
    }
}
