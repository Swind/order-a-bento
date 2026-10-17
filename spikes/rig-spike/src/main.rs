mod mock;
mod tools;

use std::{sync::Arc, time::Duration};

use mock::{start, MockState, Scenario};
use rig::completion::{Effort, Reasoning};
use rig::message::{ImageMediaType, UserContent};
use rig::providers::anthropic::{
    extension::{AnthropicOptions, Fallbacks, SERVER_SIDE_FALLBACK_BETA},
    AnthropicConfig, CLAUDE_OPUS_5_5,
};
use rig::agent::{AgentHook, HookContext, ModelTurnAction, ModelTurnFinished};
use rig::AgentBuilder;
use tools::{ImportToolContext, ListVendors, ProposeVendor};

const PREAMBLE: &str = "你是「訂便當系統」的資料輸入助理。先用 list_vendors 查現有店家，再用 propose_vendor 提出內容，最後用繁體中文摘要。";
const FAKE_JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xFF, 0xD9];

/// Captures per-turn finish reason + raw provider response, so a refusal's
/// explanation (lost in the final PromptError text) can be recovered.
#[derive(Clone, Default)]
struct TurnSpy(Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>);

impl AgentHook for TurnSpy {
    fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> impl std::future::Future<Output = ModelTurnAction> + Send {
        let rec = (format!("{:?}", event.finish_reason), event.raw.clone());
        self.0.lock().unwrap().push(rec);
        async { ModelTurnAction::Continue }
    }
}

struct Opts {
    strict: bool,
    fallbacks: bool,
    /// use a custom reqwest client with a total request timeout
    client_timeout: Option<Duration>,
}

fn build_agent(base_url: &str, ctx: Arc<ImportToolContext>, o: &Opts, spy: TurnSpy) -> rig::Agent {
    // Q1: client with custom base URL (from_env equivalent: Anthropic::from_env() reads
    // ANTHROPIC_API_KEY + ANTHROPIC_BASE_URL).
    let mut cfg = AnthropicConfig::new("sk-ant-mock").with_base_url(base_url);
    if o.fallbacks {
        cfg = cfg.with_beta(SERVER_SIDE_FALLBACK_BETA);
    }
    let client = match o.client_timeout {
        Some(t) => cfg.connect(rig::http_client::ReqwestClient::from(
            reqwest::Client::builder().timeout(t).connect_timeout(Duration::from_secs(5)).build().unwrap(),
        )),
        None => cfg.client(),
    };
    let mut model = client.completion(CLAUDE_OPUS_5_5);
    if o.strict {
        model.wire = model.wire.with_strict_tools();
    }
    let mut b = AgentBuilder::new(model)
        .preamble(PREAMBLE)
        .reasoning(Reasoning::Effort(Effort::Low))
        .max_tokens(4096)
        .default_max_turns(6)
        .add_hook(spy);
    if o.fallbacks {
        b = b.provider_option(AnthropicOptions::default().fallbacks(Fallbacks::Default));
    }
    b.tool(ListVendors { ctx: ctx.clone() })
        .tool(ProposeVendor { ctx })
        .build()
}

async fn run(name: &str, scenario: Scenario, o: Opts, timeout: Option<Duration>) -> Arc<MockState> {
    println!("\n=== scenario: {name} ===");
    let (url, st) = start(scenario).await;
    let ctx = Arc::new(ImportToolContext { organizer_id: 42, run_id: "run-1".into(), ..Default::default() });
    let t0 = std::time::Instant::now();
    let spy = TurnSpy::default();
    let agent = build_agent(&url, ctx.clone(), &o, spy.clone());
    println!("  build agent: {:?}", t0.elapsed());
    let fut = async {
        agent
            .prompt("這是一張便當店傳單，請匯入。")
            .attach_files([UserContent::image_raw(FAKE_JPEG.to_vec(), Some(ImageMediaType::JPEG), None)])
            .expect("attach")
            .await
    };
    let res = match timeout {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(r) => r,
            Err(_) => {
                println!("  RESULT: tokio::time::timeout elapsed after {d:?}");
                dump(&st, name);
                return st;
            }
        },
        None => fut.await,
    };
    match res {
        Ok(r) => {
            println!("  RESULT: Ok output={:?}", r.output());
            println!("  usage={:?}", r.usage);
            println!("  completion_calls={}", r.completion_calls.len());
        }
        Err(e) => {
            println!("  RESULT: Err Display = {e}");
            println!("  RESULT: Err Debug   = {e:?}");
        }
    }
    if name == "refusal" {
        for (fr, raw) in spy.0.lock().unwrap().iter() {
            println!("  hook saw finish_reason={fr} raw={raw}");
        }
    }
    println!("  proposals collected = {}", ctx.proposals.lock().unwrap().len());
    println!("  HTTP requests to mock = {}", st.calls.load(std::sync::atomic::Ordering::SeqCst));
    dump(&st, name);
    st
}

fn dump(st: &MockState, name: &str) {
    let dir = std::path::Path::new("target/recorded");
    std::fs::create_dir_all(dir).unwrap();
    for (i, b) in st.bodies.lock().unwrap().iter().enumerate() {
        let mut b = b.clone();
        redact(&mut b);
        std::fs::write(dir.join(format!("{name}-{}.json", i + 1)), serde_json::to_string_pretty(&b).unwrap()).unwrap();
    }
    for (i, h) in st.headers.lock().unwrap().iter().enumerate() {
        std::fs::write(dir.join(format!("{name}-{}.headers.json", i + 1)), serde_json::to_string_pretty(h).unwrap()).unwrap();
    }
}

fn redact(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            if m.get("type").and_then(|t| t.as_str()) == Some("base64") {
                m.insert("data".into(), "<base64 redacted>".into());
            }
            m.values_mut().for_each(redact);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(redact),
        _ => {}
    }
}

#[tokio::main]
async fn main() {
    println!("propose_vendor parameters():\n{}", serde_json::to_string_pretty(&tools::strict_schema::<tools::ProposeVendorArgs>()).unwrap());
    let off = || Opts { strict: false, fallbacks: false, client_timeout: None };
    run("happy", Scenario::Happy, Opts { strict: true, fallbacks: false, client_timeout: None }, None).await;
    run("happy-nostrict", Scenario::Happy, off(), None).await;
    run("fallbacks", Scenario::Happy, Opts { strict: true, fallbacks: true, client_timeout: None }, None).await;
    run("refusal", Scenario::Refusal, off(), None).await;
    run("invalid-args", Scenario::InvalidArgs, off(), None).await;
    run("business-invalid", Scenario::BusinessInvalid, off(), None).await;
    let t = std::time::Instant::now();
    run("overloaded-529", Scenario::Overloaded529, off(), None).await;
    println!("  elapsed {:?}", t.elapsed());
    let t = std::time::Instant::now();
    run("server-500", Scenario::Server500, off(), None).await;
    println!("  elapsed {:?}", t.elapsed());
    run("hang-reqwest-timeout", Scenario::Hang, Opts { strict: false, fallbacks: false, client_timeout: Some(Duration::from_secs(2)) }, None).await;
    run("hang-timeout", Scenario::Hang, off(), Some(Duration::from_secs(2))).await;
}
