//! [`ImportAgent`] backed by rig: Anthropic Messages, or an OpenAI-compatible
//! Chat Completions server (design 9).
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
use rig::agent::{AgentHook, HookContext, ModelTurnAction, ModelTurnFinished};
use rig::completion::{Effort, Reasoning};
use rig::message::{ImageMediaType, UserContent};
use rig::operation::Completion;
use rig::providers::anthropic::extension::{
    AnthropicOptions, Fallbacks, SERVER_SIDE_FALLBACK_BETA,
};
use rig::providers::anthropic::{ANTHROPIC, AnthropicConfig, Dialect, Quirks};
use rig::providers::openai::wire::OPENAI;
use rig::providers::openai::{OpenAIConfig, Route};
use rig::{AgentBuilder, DynModel};

use super::tools::{add_roster_tools, add_vendor_tools};

/// Cap on a single model turn (summary plus a few dozen tool-call items).
const MAX_TOKENS: u64 = 4096;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Which Messages-API server the agent talks to (`AI_PROVIDER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiProvider {
    /// Anthropic's own API: Claude defaults (strict tools, effort, fallbacks).
    #[default]
    Anthropic,
    /// A gateway that speaks the Messages API in front of another model
    /// (vLLM, LiteLLM, a self-hosted Qwen ...). Sends the plain subset: no
    /// fallbacks beta, and strict tools / effort only when asked for. Thinking
    /// blocks with an empty signature are replayed as thinking, as such
    /// servers return them.
    AnthropicCompatible,
    /// Any server that speaks OpenAI Chat Completions (`/v1/chat/completions`):
    /// OpenAI itself, DeepSeek, vLLM, LiteLLM, opencode ... Images go as
    /// `image_url` data URLs; `reasoning_content` is replayed to the server
    /// as DeepSeek-style thinking models expect.
    OpenAiCompatible,
    /// A server that speaks the OpenAI Responses API (`/v1/responses`), for
    /// models that do not take Chat Completions. Stateless: `store: false`,
    /// and the encrypted reasoning items are replayed in the tool loop.
    OpenAiResponses,
}

/// An extra HTTP request header (`AI_EXTRA_HEADERS`). The value may be a
/// credential, so `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct ExtraHeader {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for ExtraHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}=<redacted>", self.name)
    }
}

/// Reasoning request sent with every model call (`AI_REASONING`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiReasoning {
    /// `output_config.effort = "low"` (and adaptive thinking, which rig adds
    /// with its beta header).
    #[default]
    Low,
    /// Nothing: the server's default reasoning behaviour.
    Off,
}

#[derive(Clone)]
pub struct RigAgentConfig {
    pub provider: AiProvider,
    /// Send `strict: true` tool schemas (`AI_STRICT_TOOLS`).
    pub strict_tools: bool,
    pub reasoning: AiReasoning,
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
    /// Headers added to every request (`AI_EXTRA_HEADERS`).
    pub extra_headers: Vec<ExtraHeader>,
    /// Name of a header that carries the import run's id on every request of
    /// that run (`AI_SESSION_HEADER`, e.g. `x-opencode-session`): gateways that
    /// route or cache by session need one id per agent run.
    pub session_header: Option<String>,
}

impl RigAgentConfig {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: AiProvider::Anthropic,
            strict_tools: true,
            reasoning: AiReasoning::Low,
            api_key: api_key.into(),
            base_url: None,
            model: model.into(),
            max_turns: 8,
            fallbacks: true,
            request_timeout: Duration::from_secs(60),
            extra_headers: Vec::new(),
            session_header: None,
        }
    }
}

// The API key must never reach logs.
impl std::fmt::Debug for RigAgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RigAgentConfig")
            .field("provider", &self.provider)
            .field("strict_tools", &self.strict_tools)
            .field("reasoning", &self.reasoning)
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_turns", &self.max_turns)
            .field("fallbacks", &self.fallbacks)
            .field("request_timeout", &self.request_timeout)
            .field("extra_headers", &self.extra_headers)
            .field("session_header", &self.session_header)
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

    /// The HTTP client for one run. Without per-run headers it is the shared
    /// pool; with them (session id, extra headers) a client carrying those as
    /// default headers, so every request of the run has them.
    fn http_for_run(&self, session_id: &str) -> Result<reqwest::Client, AiError> {
        let cfg = &self.config;
        if cfg.extra_headers.is_empty() && cfg.session_header.is_none() {
            return Ok(self.http.clone());
        }
        let bad = || unavailable(false, "invalid AI_EXTRA_HEADERS / AI_SESSION_HEADER entry");
        let mut headers = reqwest::header::HeaderMap::new();
        let pairs = cfg
            .extra_headers
            .iter()
            .map(|h| (h.name.as_str(), h.value.as_str()))
            .chain(cfg.session_header.as_deref().map(|n| (n, session_id)));
        for (name, value) in pairs {
            let name =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| bad())?;
            let mut value = reqwest::header::HeaderValue::from_str(value).map_err(|_| bad())?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        reqwest::Client::builder()
            .timeout(cfg.request_timeout)
            .connect_timeout(CONNECT_TIMEOUT)
            .default_headers(headers)
            .build()
            .map_err(|e| unavailable(false, format!("building the HTTP client: {e}")))
    }
}

/// What the hook saw, shared with the code that maps the final result.
struct Seen {
    /// Start of the model call in flight: the run start, then each finished turn. rig's error
    /// report drops reqwest's `is_timeout()`, so a timeout is recognised by elapsed time.
    call_started: std::time::Instant,
    /// `Some(category)` once a turn ended with `stop_reason == "refusal"`.
    refusal: Option<Option<String>>,
    /// `model` of the last response (differs from the request after a fallback).
    model: Option<String>,
}

impl Default for Seen {
    fn default() -> Self {
        Self {
            call_started: std::time::Instant::now(),
            refusal: None,
            model: None,
        }
    }
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
        if let Some(r) = event.usage.reasoning_tokens {
            tracing::debug!(reasoning_tokens = r, "AI model turn reasoning");
        }
        let raw = event.raw;
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.call_started = std::time::Instant::now();
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

/// `url` without trailing slashes and without a final `/v1` segment.
fn origin_of(url: &str) -> &str {
    let url = url.trim_end_matches('/');
    url.strip_suffix("/v1").unwrap_or(url)
}

fn user_message(input: &AgentInput) -> String {
    let mut lines = vec![
        "Please process this photo.".to_string(),
        "Reply to the organizer in Traditional Chinese (zh-TW).".to_string(),
    ];
    if let Some(id) = input.target_roster_id {
        lines.push(format!("Target roster id: {id}"));
    }
    if let Some(id) = input.target_vendor_id {
        lines.push(format!("Target vendor id: {id}"));
    }
    if let Some(i) = &input.instruction {
        lines.push(format!("Organizer instruction: {i}"));
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

fn map_error(e: rig::completion::PromptError, seen: &Seen, request_timeout: Duration) -> AiError {
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
            // reqwest's client timeout surfaces as an opaque status-less Http error. Not retried: a second full timeout on a slow
            // gateway would only burn the run budget; the user can retry by hand.
            let timed_out = r.kind.code() == "timeout"
                || (status.is_none()
                    && r.kind.code() == "http"
                    && seen.call_started.elapsed() + Duration::from_millis(500) >= request_timeout);
            if timed_out {
                tracing::warn!(
                    timeout_secs = request_timeout.as_secs(),
                    "AI model call timed out (raise AI_REQUEST_TIMEOUT_SECS for slow gateways)"
                );
                return unavailable(
                    false,
                    format!("model call timed out after {} s", request_timeout.as_secs()),
                );
            }
            match status {
                Some(401 | 403) => {
                    tracing::error!(status, "AI provider rejected the credentials");
                    unavailable(
                        false,
                        format!("authentication failed (http {})", status.unwrap_or(0)),
                    )
                }
                // The server could not decode the picture (corrupt, or a
                // format it does not read): not retryable, the organizer must
                // upload another photo. The provider text is not passed on.
                Some(400)
                    if r.provider_response_body().is_some_and(|b| {
                        let b = b.to_ascii_lowercase();
                        b.contains("image") && (b.contains("invalid") || b.contains("unsupported"))
                    }) =>
                {
                    AiError::InvalidImage("the model could not read the image".into())
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

        let http = self.http_for_run(&ctx.run_id.to_string())?;
        // Fallbacks are an Anthropic-only feature.
        let fallbacks = cfg.fallbacks && cfg.provider == AiProvider::Anthropic;
        let model: DynModel<Completion> = match cfg.provider {
            AiProvider::OpenAiCompatible => {
                // The stock OpenAI dialect (typed options such as
                // `reasoning_effort`) on the Chat Completions route, plus the
                // `reasoning_content` field DeepSeek-style models want back.
                let mut dialect = OPENAI;
                dialect.quirks.completion_route = Route::Chat;
                dialect.quirks.reasoning_field = Some("reasoning_content");
                // The base URL is the server's origin, with or without the
                // OpenAI-style trailing `/v1`; the path below carries it.
                dialect.quirks.completion_path = "/v1/chat/completions";
                let mut provider = OpenAIConfig::with_key(&dialect, cfg.api_key.as_str());
                if let Some(url) = &cfg.base_url {
                    provider = provider.with_base_url(origin_of(url));
                }
                let client = provider.connect(rig::http_client::ReqwestClient::from(http));
                let mut model = client.chat(cfg.model.as_str());
                if cfg.strict_tools {
                    model.wire = model.wire.with_strict_tools();
                }
                model.into()
            }
            AiProvider::OpenAiResponses => {
                let mut dialect = OPENAI;
                dialect.quirks.completion_route = Route::Responses;
                // Origin with or without `/v1`; the path below carries it.
                dialect.quirks.responses.path = "/v1/responses";
                let mut provider = OpenAIConfig::with_key(&dialect, cfg.api_key.as_str());
                if let Some(url) = &cfg.base_url {
                    provider = provider.with_base_url(origin_of(url));
                }
                let client = provider.connect(rig::http_client::ReqwestClient::from(http));
                let mut model = client.responses(cfg.model.as_str());
                if cfg.strict_tools {
                    model.wire = model.wire.with_strict_tools();
                }
                model.into()
            }
            AiProvider::Anthropic | AiProvider::AnthropicCompatible => {
                let mut provider = AnthropicConfig::new(&cfg.api_key);
                if let Some(url) = &cfg.base_url {
                    provider = provider.with_base_url(url);
                }
                if cfg.provider == AiProvider::AnthropicCompatible {
                    let mut quirks = Quirks::gateway();
                    quirks.unsigned_thinking = true;
                    quirks.strict_tool_schemas = cfg.strict_tools;
                    provider.dialect = Dialect {
                        quirks,
                        ..ANTHROPIC
                    };
                }
                if fallbacks {
                    provider = provider.with_beta(SERVER_SIDE_FALLBACK_BETA);
                }
                let client = provider.connect(rig::http_client::ReqwestClient::from(http));
                let mut model = client.completion(cfg.model.as_str());
                if cfg.strict_tools {
                    model.wire = model.wire.with_strict_tools();
                }
                model.into()
            }
        };

        let seen = Arc::new(Mutex::new(Seen::default()));
        let hook = TurnHook {
            ctx: ctx.clone(),
            seen: seen.clone(),
        };
        let mut builder = AgentBuilder::new(model)
            .preamble(import_agent_system_prompt(input.mode))
            .max_tokens(MAX_TOKENS)
            .default_max_turns(cfg.max_turns)
            .add_hook(hook);
        if cfg.reasoning == AiReasoning::Low {
            builder = builder.reasoning(Reasoning::Effort(Effort::Low));
        }
        if cfg.provider == AiProvider::OpenAiResponses {
            // No server-side state: reasoning items travel with their
            // ciphertext (rig adds `include` for `store: false`).
            builder = builder.additional_params(serde_json::json!({ "store": false }));
        }
        if fallbacks {
            builder =
                builder.provider_option(AnthropicOptions::default().fallbacks(Fallbacks::Default));
        }
        let image = UserContent::image_raw(input.image.bytes.clone(), Some(media_type), None);
        // Only the chosen mode's tools exist, so a proposal of the other kind
        // cannot be made.
        macro_rules! prompt {
            ($agent:expr) => {
                $agent
                    .prompt(user_message(&input))
                    .attach_files([image])
                    .map_err(|e| AiError::InvalidImage(e.to_string()))?
                    .await
            };
        }
        let run = match input.mode {
            ImportMode::Roster => prompt!(add_roster_tools!(builder, ctx).build()),
            ImportMode::Vendor => prompt!(add_vendor_tools!(builder, ctx).build()),
        };

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
            Err(e) => Err(map_error(e, &seen, cfg.request_timeout)),
        }
    }
}
