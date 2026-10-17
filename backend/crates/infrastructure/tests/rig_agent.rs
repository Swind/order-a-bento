//! `RigImportAgent` against a local mock of the Anthropic API.

mod support;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use application::ai_import::{AgentInput, AiError, ImageInput, ImportAgent, ImportToolContext};
use application::ports::OrganizerRepo;
use domain::ai_import::{ImportMode, ImportRunId, ProposalTarget};
use domain::identity::{Organizer, OrganizerId};
use domain::shared::SystemClock;
use infrastructure::ai::{
    AiProvider, AiReasoning, ExtraHeader, RigAgentConfig, RigImportAgent, import_tools_parameters,
};
use infrastructure::directory_repo::{SqliteRosterRepo, SqliteVendorRepo};
use infrastructure::identity_repo::SqliteOrganizerRepo;
use infrastructure::sqlite::{Db, TempDir};
use serde_json::{Value, json};
use support::{
    Mock, Reply, chat_end_turn, chat_reply, chat_tool_call, chat_tool_turn, end_turn, msg,
    responses_call, responses_reasoning, responses_reply, responses_text, start, start_openai,
    start_responses, tool_turn, tool_use,
};

const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0xFF, 0xD9,
];

struct Env {
    ctx: Arc<ImportToolContext>,
    _dir: TempDir,
}

async fn env() -> Env {
    let (db, dir) = Db::open_temp().await.unwrap();
    let o = Organizer::new("sub-a", "a@x.com", "a", None).unwrap();
    let org: OrganizerId = SqliteOrganizerRepo::new(db.clone())
        .upsert_from_identity(o, SystemTime::now())
        .await
        .unwrap()
        .id;
    let ctx = Arc::new(ImportToolContext::new(
        org,
        ImportRunId::new(),
        Arc::new(SqliteRosterRepo::new(db.clone())),
        Arc::new(SqliteVendorRepo::new(db.clone())),
        Arc::new(SystemClock),
    ));
    Env { ctx, _dir: dir }
}

fn input() -> AgentInput {
    AgentInput {
        image: ImageInput {
            bytes: JPEG.to_vec(),
            media_type: "image/jpeg".into(),
        },
        instruction: Some("這是午餐店的菜單".into()),
        mode: ImportMode::Vendor,
        target_roster_id: None,
        target_vendor_id: None,
    }
}

fn agent(url: &str, fallbacks: bool) -> RigImportAgent {
    let mut c = RigAgentConfig::new("sk-ant-test-key", "claude-opus-5-5");
    c.base_url = Some(url.to_string());
    c.fallbacks = fallbacks;
    c.request_timeout = Duration::from_secs(5);
    RigImportAgent::new(c).unwrap()
}

fn vendor_args() -> Value {
    json!({
        "target": "new",
        "info": { "name": "阿財排骨便當", "phone": "02-2345-6789", "address": null, "note": null },
        "menuItems": [
            { "name": "排骨飯", "price": 85, "description": null, "category": "飯類", "confidence": "high" },
            { "name": "雞腿飯", "price": null, "description": null, "category": "飯類", "confidence": "low" }
        ],
        "warnings": ["『雞腿飯』價格被摺痕遮住"]
    })
}

fn happy_script() -> Vec<Reply> {
    vec![
        Reply::Message(tool_turn(vec![
            json!({ "type": "text", "text": "先查現有店家。" }),
            tool_use("toolu_01", "list_vendors", json!({})),
        ])),
        Reply::Message(tool_turn(vec![tool_use(
            "toolu_02",
            "propose_vendor",
            vendor_args(),
        )])),
        Reply::Message(end_turn("已提出阿財排骨便當的菜單，請確認雞腿飯的價格。")),
    ]
}

fn first_body(mock: &Mock) -> Value {
    mock.bodies.lock().unwrap()[0].clone()
}

#[tokio::test]
async fn happy_path_records_proposal_usage_and_tool_calls() {
    let e = env().await;
    let (url, mock) = start(happy_script()).await;
    let out = agent(&url, true).run(e.ctx.clone(), input()).await.unwrap();

    assert_eq!(
        out.summary,
        "已提出阿財排骨便當的菜單，請確認雞腿飯的價格。"
    );
    assert_eq!(out.usage.input_tokens, 3000);
    assert_eq!(out.usage.output_tokens, 150);
    assert_eq!(out.model, "claude-opus-5-5");
    assert_eq!(mock.calls(), 3);
    // per-turn usage reached the context as well
    assert_eq!(e.ctx.usage().input_tokens, 3000);

    let p = e.ctx.snapshot().await;
    assert_eq!(p.targets.len(), 1);
    let ProposalTarget::Vendor(v) = &p.targets[0] else {
        panic!("vendor target expected")
    };
    assert_eq!(v.info.after.name, "阿財排骨便當");
    assert_eq!(v.menu.len(), 2);

    let calls: Vec<Value> = serde_json::from_str(&e.ctx.tool_calls_json().unwrap()).unwrap();
    let names: Vec<_> = calls.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list_vendors", "propose_vendor"]);
    assert_eq!(calls[1]["args"]["info"]["name"], "阿財排骨便當");
}

#[tokio::test]
async fn roster_upload_gets_only_roster_tools_and_the_roster_prompt() {
    let e = env().await;
    let (url, mock) = start(happy_script()).await;
    let mut inp = input();
    inp.mode = ImportMode::Roster;
    // the scripted model answers with vendor tool calls, which do not exist here
    let _ = agent(&url, true).run(e.ctx.clone(), inp).await;
    let body = first_body(&mock);
    let names: Vec<_> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["list_rosters", "get_roster", "propose_roster_members"]
    );
    let system = body["system"][0]["text"].as_str().unwrap();
    assert!(system.contains("## Roster rules") && !system.contains("## Vendor rules"));
    assert!(e.ctx.snapshot().await.targets.is_empty());
}

#[tokio::test]
async fn first_request_shape_with_fallbacks() {
    let e = env().await;
    let (url, mock) = start(happy_script()).await;
    agent(&url, true).run(e.ctx.clone(), input()).await.unwrap();

    let body = first_body(&mock);
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["output_config"]["effort"], "low");
    assert_eq!(body["fallbacks"], "default");
    let tools = body["tools"].as_array().unwrap();
    // input() is a vendor upload: only the vendor tools exist
    assert_eq!(tools.len(), 3);
    for t in tools {
        assert_eq!(t["strict"], true, "tool {} not strict", t["name"]);
        assert_eq!(t["input_schema"]["additionalProperties"], false);
    }
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["list_vendors", "get_vendor", "propose_vendor"] {
        assert!(names.contains(&n), "missing {n}");
    }
    for n in ["list_rosters", "get_roster", "propose_roster_members"] {
        assert!(!names.contains(&n), "unexpected {n}");
    }
    // system prompt + text and image in the user message
    assert!(
        body["system"][0]["text"]
            .as_str()
            .unwrap()
            .contains("data-entry assistant")
    );
    let content = body["messages"][0]["content"].as_array().unwrap();
    let text = content[0]["text"].as_str().unwrap();
    assert!(!text.contains("Mode hint"));
    assert!(text.contains("Organizer instruction: 這是午餐店的菜單"));
    let img = content
        .iter()
        .find(|c| c["type"] == "image")
        .expect("image block");
    assert_eq!(img["source"]["type"], "base64");
    assert_eq!(img["source"]["media_type"], "image/jpeg");

    let headers = mock.headers.lock().unwrap()[0].clone();
    assert!(
        headers["anthropic-beta"]
            .as_str()
            .unwrap()
            .contains("server-side-fallback-2026-07-01")
    );
    assert_eq!(headers["x-api-key"], "sk-ant-test-key");
}

#[tokio::test]
async fn no_fallbacks_when_off() {
    let e = env().await;
    let (url, mock) = start(happy_script()).await;
    agent(&url, false)
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    let body = first_body(&mock);
    assert!(body.get("fallbacks").is_none());
    let beta = mock.headers.lock().unwrap()[0]
        .get("anthropic-beta")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!beta.contains("server-side-fallback"));
}

fn compat_agent(url: &str, strict: bool, reasoning: AiReasoning) -> RigImportAgent {
    let mut c = RigAgentConfig::new("gw-test-key", "qwen-test");
    c.base_url = Some(url.to_string());
    c.provider = AiProvider::AnthropicCompatible;
    c.strict_tools = strict;
    c.reasoning = reasoning;
    // even a stray `true` must not leak the Anthropic-only feature
    c.fallbacks = true;
    c.request_timeout = Duration::from_secs(5);
    RigImportAgent::new(c).unwrap()
}

fn beta_header(mock: &Mock) -> String {
    mock.headers.lock().unwrap()[0]
        .get("anthropic-beta")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[tokio::test]
async fn compatible_provider_sends_the_plain_subset() {
    let e = env().await;
    let (url, mock) = start(happy_script()).await;
    compat_agent(&url, false, AiReasoning::Off)
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    let body = first_body(&mock);
    assert_eq!(body["model"], "qwen-test");
    assert!(body.get("fallbacks").is_none());
    assert!(body.get("output_config").is_none(), "{body}");
    assert!(body.get("thinking").is_none(), "{body}");
    assert_eq!(beta_header(&mock), "");
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    for t in tools {
        assert!(t.get("strict").is_none(), "tool {} strict", t["name"]);
        // our own schema is closed anyway
        assert_eq!(t["input_schema"]["additionalProperties"], false);
    }
    assert_eq!(mock.headers.lock().unwrap()[0]["x-api-key"], "gw-test-key");
    // the image still goes as a base64 block
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert!(content.iter().any(|c| c["type"] == "image"));
}

#[tokio::test]
async fn compatible_provider_opt_in_strict_and_effort() {
    let e = env().await;
    let (url, mock) = start(happy_script()).await;
    compat_agent(&url, true, AiReasoning::Low)
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    let body = first_body(&mock);
    assert_eq!(body["output_config"]["effort"], "low");
    assert!(body.get("fallbacks").is_none());
    assert!(!beta_header(&mock).contains("server-side-fallback"));
    for t in body["tools"].as_array().unwrap() {
        assert_eq!(t["strict"], true);
    }
}

#[tokio::test]
async fn compatible_provider_replays_unsigned_thinking() {
    let e = env().await;
    let (url, mock) = start(vec![
        Reply::Message(tool_turn(vec![
            json!({ "type": "thinking", "thinking": "need the vendors", "signature": "" }),
            tool_use("toolu_01", "list_vendors", json!({})),
        ])),
        Reply::Message(end_turn("沒有可匯入的內容。")),
    ])
    .await;
    let out = compat_agent(&url, false, AiReasoning::Off)
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    assert_eq!(out.summary, "沒有可匯入的內容。");
    let second = mock.bodies.lock().unwrap()[1].clone();
    let assistant = second["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "assistant")
        .expect("assistant turn replayed");
    let blocks = assistant["content"].as_array().unwrap();
    assert!(
        blocks
            .iter()
            .any(|b| b["type"] == "thinking" && b["thinking"] == "need the vendors"),
        "{assistant}"
    );
    assert!(blocks.iter().any(|b| b["type"] == "tool_use"));
}

#[tokio::test]
async fn anthropic_provider_is_unchanged_by_the_new_options() {
    let c = RigAgentConfig::new("k", "m");
    assert_eq!(c.provider, AiProvider::Anthropic);
    assert!(c.strict_tools && c.fallbacks);
    assert_eq!(c.reasoning, AiReasoning::Low);
}

#[tokio::test]
async fn refusal_maps_to_refused_with_category_and_keeps_usage() {
    let e = env().await;
    let (url, mock) = start(vec![Reply::Message(msg(
        json!([]),
        "refusal",
        json!({ "stop_details": { "type": "refusal", "category": "cyber", "explanation": "x" } }),
    ))])
    .await;
    let err = agent(&url, true)
        .run(e.ctx.clone(), input())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AiError::Refused {
            category: Some("cyber".into())
        }
    );
    assert_eq!(mock.calls(), 1);
    // the refused turn's usage is still reported
    assert_eq!(e.ctx.usage().input_tokens, 1000);
}

#[tokio::test]
async fn overloaded_and_server_errors_are_retryable_unavailable() {
    for (status, kind) in [(529, "overloaded_error"), (500, "api_error")] {
        let e = env().await;
        let (url, _) = start(vec![Reply::Error(status, kind)]).await;
        let err = agent(&url, true)
            .run(e.ctx.clone(), input())
            .await
            .unwrap_err();
        match err {
            AiError::Unavailable { retryable, detail } => {
                assert!(retryable, "{status}");
                assert!(!detail.contains("sk-ant"), "no secrets in detail");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn auth_failure_is_not_retryable() {
    let e = env().await;
    let (url, _) = start(vec![Reply::Error(401, "authentication_error")]).await;
    let err = agent(&url, true)
        .run(e.ctx.clone(), input())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        AiError::Unavailable {
            retryable: false,
            ..
        }
    ));
}

#[tokio::test]
async fn invalid_proposal_is_returned_to_the_model_and_fixed() {
    let e = env().await;
    let mut bad = vendor_args();
    bad["menuItems"][0]["price"] = json!(-5);
    let (url, mock) = start(vec![
        Reply::Message(tool_turn(vec![tool_use("toolu_01", "propose_vendor", bad)])),
        Reply::Message(tool_turn(vec![tool_use(
            "toolu_02",
            "propose_vendor",
            vendor_args(),
        )])),
        Reply::Message(end_turn("完成")),
    ])
    .await;
    agent(&url, true).run(e.ctx.clone(), input()).await.unwrap();
    // the second request carries the first tool_result with the error list
    let second = mock.bodies.lock().unwrap()[1].to_string();
    assert!(second.contains("tool_result"));
    assert!(second.contains("errors"));
    assert_eq!(e.ctx.snapshot().await.targets.len(), 1);
}

#[test]
fn tool_schemas_are_strict_compatible() {
    fn check(name: &str, v: &Value) {
        match v {
            Value::Object(m) => {
                if m.get("type") == Some(&json!("object")) {
                    assert_eq!(m.get("additionalProperties"), Some(&json!(false)), "{name}");
                    let props = m.get("properties").and_then(|p| p.as_object());
                    let req = m.get("required").and_then(|r| r.as_array());
                    assert_eq!(
                        props.map_or(0, |p| p.len()),
                        req.map_or(0, |r| r.len()),
                        "{name}"
                    );
                }
                assert!(
                    !m.contains_key("$ref") && !m.contains_key("$defs"),
                    "{name}"
                );
                m.values().for_each(|c| check(name, c));
            }
            Value::Array(a) => a.iter().for_each(|c| check(name, c)),
            _ => {}
        }
    }
    let tools = import_tools_parameters();
    assert_eq!(tools.len(), 6);
    for (name, schema) in &tools {
        check(name, schema);
    }
}

#[tokio::test]
async fn slow_model_call_reports_a_timeout_and_is_not_retryable() {
    let e = env().await;
    let (url, _) = start(vec![Reply::Slow(Duration::from_secs(3))]).await;
    let mut c = RigAgentConfig::new("sk-ant-test-key", "claude-opus-5-5");
    c.base_url = Some(url);
    c.request_timeout = Duration::from_secs(1);
    let err = RigImportAgent::new(c)
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap_err();
    match err {
        AiError::Unavailable { retryable, detail } => {
            assert!(!retryable);
            assert_eq!(detail, "model call timed out after 1 s");
        }
        other => panic!("unexpected {other:?}"),
    }
}

// ---- OpenAI-compatible Chat Completions ------------------------------------------------

fn oa_config(url: &str) -> RigAgentConfig {
    let mut c = RigAgentConfig::new("oa-test-key", "deepseek-v4.1-flash");
    c.base_url = Some(url.to_string());
    c.provider = AiProvider::OpenAiCompatible;
    c.strict_tools = false;
    c.reasoning = AiReasoning::Off;
    c.fallbacks = true; // must not leak into this provider
    c.request_timeout = Duration::from_secs(5);
    c
}

fn oa_happy_script() -> Vec<Reply> {
    vec![
        Reply::Message(chat_tool_turn(
            Some("need the vendors first"),
            vec![chat_tool_call("call_1", "list_vendors", json!({}))],
        )),
        Reply::Message(chat_tool_turn(
            None,
            vec![chat_tool_call("call_2", "propose_vendor", vendor_args())],
        )),
        Reply::Message(chat_end_turn(
            "已提出阿財排骨便當的菜單，請確認雞腿飯的價格。",
        )),
    ]
}

#[tokio::test]
async fn openai_happy_path_request_shape_and_usage() {
    let e = env().await;
    let (url, mock) = start_openai(oa_happy_script()).await;
    let out = RigImportAgent::new(oa_config(&url))
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    assert_eq!(
        out.summary,
        "已提出阿財排骨便當的菜單，請確認雞腿飯的價格。"
    );
    assert_eq!(out.model, "deepseek-v4.1-flash");
    assert_eq!(out.usage.input_tokens, 3000);
    assert_eq!(out.usage.output_tokens, 150);
    assert_eq!(e.ctx.usage().input_tokens, 3000);
    assert_eq!(mock.calls(), 3);
    let ProposalTarget::Vendor(v) = &e.ctx.snapshot().await.targets[0] else {
        panic!("vendor target expected")
    };
    assert_eq!(v.menu.len(), 2);

    let body = first_body(&mock);
    assert_eq!(body["model"], "deepseek-v4.1-flash");
    assert_eq!(body["max_tokens"], 4096);
    // nothing Anthropic-specific, no reasoning request by default
    for k in ["fallbacks", "output_config", "thinking", "reasoning_effort"] {
        assert!(body.get(k).is_none(), "{k} in {body}");
    }
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    for t in tools {
        assert_eq!(t["type"], "function");
        assert!(t["function"].get("strict").is_none(), "{t}");
        assert_eq!(t["function"]["parameters"]["additionalProperties"], false);
    }
    // system prompt, then the user message with text and the image as a data URL
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "system");
    assert!(messages[0].to_string().contains("data-entry assistant"));
    let user = messages.iter().find(|m| m["role"] == "user").unwrap();
    let parts = user["content"].as_array().unwrap();
    assert!(!parts[0]["text"].as_str().unwrap().contains("Mode hint"));
    let img = parts
        .iter()
        .find(|p| p["type"] == "image_url")
        .expect("image part");
    let data_url = img["image_url"]["url"].as_str().unwrap();
    assert!(
        data_url.starts_with("data:image/jpeg;base64,/9j/"),
        "{data_url}"
    );

    let h = mock.headers.lock().unwrap()[0].clone();
    assert_eq!(h["authorization"], "Bearer oa-test-key");
    assert!(h.get("anthropic-beta").is_none() && h.get("x-api-key").is_none());
}

#[tokio::test]
async fn openai_base_url_may_end_in_v1() {
    for suffix in ["", "/", "/v1", "/v1/"] {
        let e = env().await;
        let (url, mock) = start_openai(oa_happy_script()).await;
        RigImportAgent::new(oa_config(&format!("{url}{suffix}")))
            .unwrap()
            .run(e.ctx.clone(), input())
            .await
            .unwrap_or_else(|err| panic!("{suffix:?}: {err:?}"));
        assert_eq!(mock.calls(), 3, "{suffix:?}");
    }
}

#[tokio::test]
async fn openai_replays_reasoning_content_in_the_tool_loop() {
    let e = env().await;
    let (url, mock) = start_openai(oa_happy_script()).await;
    RigImportAgent::new(oa_config(&url))
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    let bodies = mock.bodies.lock().unwrap().clone();
    let assistants = |i: usize| -> Vec<Value> {
        bodies[i]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "assistant")
            .cloned()
            .collect()
    };
    let second = assistants(1);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["reasoning_content"], "need the vendors first");
    assert_eq!(
        second[0]["tool_calls"][0]["function"]["name"],
        "list_vendors"
    );
    // a later turn without thinking still carries the field (some servers require it)
    let third = assistants(2);
    assert_eq!(third.len(), 2);
    assert_eq!(third[1]["reasoning_content"], "");
}

#[tokio::test]
async fn openai_opt_in_strict_tools_and_reasoning_effort() {
    let e = env().await;
    let (url, mock) = start_openai(oa_happy_script()).await;
    let mut c = oa_config(&url);
    c.strict_tools = true;
    c.reasoning = AiReasoning::Low;
    RigImportAgent::new(c)
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    let body = first_body(&mock);
    assert_eq!(body["reasoning_effort"], "low");
    for t in body["tools"].as_array().unwrap() {
        assert_eq!(t["function"]["strict"], true, "{t}");
        assert_eq!(t["function"]["parameters"]["additionalProperties"], false);
    }
}

#[tokio::test]
async fn extra_and_session_headers_are_sent_with_a_fresh_session_id_per_run() {
    let c = |url: &str| {
        let mut c = oa_config(url);
        c.extra_headers = vec![ExtraHeader {
            name: "x-gateway-team".into(),
            value: "bento".into(),
        }];
        c.session_header = Some("x-opencode-session".into());
        c
    };
    let (url, mock) = start_openai(oa_happy_script()).await;
    let agent = RigImportAgent::new(c(&url)).unwrap();
    let e1 = env().await;
    agent.run(e1.ctx.clone(), input()).await.unwrap();
    let hs = mock.headers.lock().unwrap().clone();
    assert_eq!(hs.len(), 3);
    for h in &hs {
        assert_eq!(h["x-gateway-team"], "bento");
        // every request of one run shares the run id
        assert_eq!(h["x-opencode-session"], e1.ctx.run_id.to_string().as_str());
        assert_eq!(h["authorization"], "Bearer oa-test-key");
    }
    // another run through the same agent gets its own session id
    let (url2, mock2) = start_openai(oa_happy_script()).await;
    let agent2 = RigImportAgent::new(c(&url2)).unwrap();
    let e2 = env().await;
    agent2.run(e2.ctx.clone(), input()).await.unwrap();
    let h2 = mock2.headers.lock().unwrap()[0].clone();
    assert_ne!(h2["x-opencode-session"], hs[0]["x-opencode-session"]);
    // header values never reach Debug output
    let dbg = format!("{:?}", c(&url));
    assert!(
        !dbg.contains("bento") && !dbg.contains("oa-test-key"),
        "{dbg}"
    );
    assert!(dbg.contains("x-gateway-team"));
}

#[tokio::test]
async fn session_header_also_works_with_anthropic_providers() {
    let e = env().await;
    let (url, mock) = start(happy_script()).await;
    let mut c = RigAgentConfig::new("sk-ant-test-key", "claude-opus-5-5");
    c.base_url = Some(url);
    c.request_timeout = Duration::from_secs(5);
    c.session_header = Some("x-session".into());
    RigImportAgent::new(c)
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    assert_eq!(
        mock.headers.lock().unwrap()[0]["x-session"],
        e.ctx.run_id.to_string().as_str()
    );
}

#[tokio::test]
async fn openai_content_filter_maps_to_refused_and_keeps_usage() {
    let e = env().await;
    let (url, _) = start_openai(vec![Reply::Message(chat_reply(
        json!({ "role": "assistant", "content": null }),
        "content_filter",
    ))])
    .await;
    let err = RigImportAgent::new(oa_config(&url))
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap_err();
    assert_eq!(err, AiError::Refused { category: None });
    assert_eq!(e.ctx.usage().input_tokens, 1000);
}

#[tokio::test]
async fn openai_error_statuses_map_to_ai_errors() {
    let run = |reply: Reply| async move {
        let e = env().await;
        let (url, _) = start_openai(vec![reply]).await;
        RigImportAgent::new(oa_config(&url))
            .unwrap()
            .run(e.ctx.clone(), input())
            .await
            .unwrap_err()
    };
    // bad credentials: not retryable, nothing from the provider body in the detail
    match run(Reply::Raw(
        401,
        json!({ "error": { "type": "server_error", "message": "Upstream request failed: Invalid credential" } }),
    ))
    .await
    {
        AiError::Unavailable { retryable, detail } => {
            assert!(!retryable);
            assert!(detail.contains("authentication failed"), "{detail}");
            assert!(!detail.contains("Invalid credential") && !detail.contains("oa-test-key"));
        }
        other => panic!("unexpected {other:?}"),
    }
    // the image could not be decoded by the server
    let err = run(Reply::Raw(
        400,
        json!({ "error": { "type": "invalid_request_error", "code": "invalid_request_error",
            "message": "Upstream request failed: [invalid_request_error] .messages[0].image[0]: You have uploaded an unsupported image." } }),
    ))
    .await;
    assert!(matches!(err, AiError::InvalidImage(_)), "{err:?}");
    // a missing routing header is a configuration error, not an image problem
    match run(Reply::Raw(
        400,
        json!({ "type": "error", "error": { "type": "MissingSessionID", "message": "Request is missing x-opencode-session" } }),
    ))
    .await
    {
        AiError::Unavailable { retryable, .. } => assert!(!retryable),
        other => panic!("unexpected {other:?}"),
    }
    for status in [429, 500, 503] {
        match run(Reply::Error(status, "server_error")).await {
            AiError::Unavailable { retryable, .. } => assert!(retryable, "{status}"),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn openai_slow_call_reports_a_timeout() {
    let e = env().await;
    let (url, _) = start_openai(vec![Reply::Slow(Duration::from_secs(3))]).await;
    let mut c = oa_config(&url);
    c.request_timeout = Duration::from_secs(1);
    let err = RigImportAgent::new(c)
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AiError::Unavailable {
            retryable: false,
            detail: "model call timed out after 1 s".into()
        }
    );
}

fn rs_config(url: &str) -> RigAgentConfig {
    let mut c = oa_config(url);
    c.model = "gpt-6-luna".into();
    c.provider = AiProvider::OpenAiResponses;
    c
}

fn rs_happy_script() -> Vec<Reply> {
    vec![
        Reply::Message(responses_reply(json!([
            responses_reasoning("rs_1", "CIPHER1"),
            responses_call("call_1", "list_vendors", json!({})),
        ]))),
        Reply::Message(responses_reply(json!([responses_call(
            "call_2",
            "propose_vendor",
            vendor_args()
        )]))),
        Reply::Message(responses_text("已提出阿財排骨便當的菜單。")),
    ]
}

#[tokio::test]
async fn responses_happy_path_request_shape_loop_and_usage() {
    let e = env().await;
    let (url, mock) = start_responses(rs_happy_script()).await;
    let out = RigImportAgent::new(rs_config(&url))
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    assert_eq!(out.summary, "已提出阿財排骨便當的菜單。");
    assert_eq!(out.model, "gpt-6-luna");
    assert_eq!(out.usage.input_tokens, 3000);
    assert_eq!(out.usage.output_tokens, 150);
    assert_eq!(mock.calls(), 3);
    let ProposalTarget::Vendor(v) = &e.ctx.snapshot().await.targets[0] else {
        panic!("vendor target expected")
    };
    assert_eq!(v.menu.len(), 2);

    let body = first_body(&mock);
    assert_eq!(body["model"], "gpt-6-luna");
    assert_eq!(body["max_output_tokens"], 4096);
    assert_eq!(body["store"], false);
    assert!(
        body["include"]
            .to_string()
            .contains("reasoning.encrypted_content")
    );
    assert!(body.get("reasoning").is_none() && body.get("previous_response_id").is_none());
    assert!(
        body["instructions"]
            .as_str()
            .unwrap()
            .contains("data-entry assistant")
    );
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    for t in tools {
        assert_eq!(t["type"], "function");
        assert!(t.get("strict").is_none_or(|s| s == false), "{t}");
        assert_eq!(t["parameters"]["additionalProperties"], false);
    }
    // the photo is an input_image data URL next to the text
    let parts: Vec<&Value> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["content"].as_array().unwrap())
        .collect();
    assert!(parts.iter().any(|p| {
        p["type"] == "input_text"
            && p["text"]
                .as_str()
                .unwrap()
                .contains("Organizer instruction")
    }));
    let img = parts
        .iter()
        .find(|p| p["type"] == "input_image")
        .expect("image part");
    let data_url = img["image_url"].as_str().unwrap();
    assert!(
        data_url.starts_with("data:image/jpeg;base64,/9j/"),
        "{data_url}"
    );

    let h = mock.headers.lock().unwrap()[0].clone();
    assert_eq!(h["authorization"], "Bearer oa-test-key");

    // second request: the reasoning item (with its ciphertext), the call, then its output
    let bodies = mock.bodies.lock().unwrap();
    let items = bodies[1]["input"].as_array().unwrap();
    let reasoning = items
        .iter()
        .find(|i| i["type"] == "reasoning")
        .expect("reasoning replayed");
    assert_eq!(reasoning["encrypted_content"], "CIPHER1");
    let call_pos = items
        .iter()
        .position(|i| i["type"] == "function_call")
        .unwrap();
    assert_eq!(items[call_pos]["call_id"], "call_1");
    let out_item = items
        .iter()
        .find(|i| i["type"] == "function_call_output")
        .unwrap();
    assert_eq!(out_item["call_id"], "call_1");
    assert!(out_item["output"].is_string());
    assert!(items.iter().position(|i| i["type"] == "reasoning").unwrap() < call_pos);
    assert_eq!(bodies[1]["store"], false);
}

#[tokio::test]
async fn responses_base_url_may_end_in_v1() {
    for suffix in ["", "/", "/v1", "/v1/"] {
        let e = env().await;
        let (url, mock) = start_responses(rs_happy_script()).await;
        RigImportAgent::new(rs_config(&format!("{url}{suffix}")))
            .unwrap()
            .run(e.ctx.clone(), input())
            .await
            .unwrap_or_else(|err| panic!("{suffix:?}: {err:?}"));
        assert_eq!(mock.calls(), 3, "{suffix:?}");
    }
}

#[tokio::test]
async fn responses_opt_in_strict_tools_and_reasoning_effort() {
    let e = env().await;
    let (url, mock) = start_responses(rs_happy_script()).await;
    let mut c = rs_config(&url);
    c.strict_tools = true;
    c.reasoning = AiReasoning::Low;
    RigImportAgent::new(c)
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    let body = first_body(&mock);
    assert_eq!(body["reasoning"]["effort"], "low");
    for t in body["tools"].as_array().unwrap() {
        assert_eq!(t["strict"], true, "{t}");
    }
}

#[tokio::test]
async fn responses_sends_session_and_extra_headers() {
    let e = env().await;
    let (url, mock) = start_responses(rs_happy_script()).await;
    let mut c = rs_config(&url);
    c.session_header = Some("x-opencode-session".into());
    c.extra_headers = vec![ExtraHeader {
        name: "x-team".into(),
        value: "bento".into(),
    }];
    RigImportAgent::new(c)
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap();
    let headers = mock.headers.lock().unwrap();
    let sess = headers[0]["x-opencode-session"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(sess, e.ctx.run_id.to_string());
    for h in headers.iter() {
        assert_eq!(h["x-opencode-session"], sess.as_str());
        assert_eq!(h["x-team"], "bento");
    }
}

#[tokio::test]
async fn responses_content_filter_maps_to_refused_and_keeps_usage() {
    let e = env().await;
    let mut r = responses_reply(json!([]));
    r["status"] = json!("incomplete");
    r["incomplete_details"] = json!({ "reason": "content_filter" });
    let (url, _) = start_responses(vec![Reply::Message(r)]).await;
    let err = RigImportAgent::new(rs_config(&url))
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap_err();
    assert_eq!(err, AiError::Refused { category: None });
    assert_eq!(e.ctx.usage().input_tokens, 1000);
}

#[tokio::test]
async fn responses_error_statuses_map_to_ai_errors() {
    let run = |reply: Reply| async move {
        let e = env().await;
        let (url, _) = start_responses(vec![reply]).await;
        RigImportAgent::new(rs_config(&url))
            .unwrap()
            .run(e.ctx.clone(), input())
            .await
            .unwrap_err()
    };
    match run(Reply::Raw(
        401,
        json!({ "error": { "type": "server_error", "message": "Upstream request failed: Invalid credential" } }),
    ))
    .await
    {
        AiError::Unavailable { retryable, detail } => {
            assert!(!retryable);
            assert!(detail.contains("authentication failed"), "{detail}");
            assert!(!detail.contains("Invalid credential") && !detail.contains("oa-test-key"));
        }
        other => panic!("unexpected {other:?}"),
    }
    // the exact error text of the opencode Go endpoint for an undecodable image
    let err = run(Reply::Raw(
        400,
        json!({ "model": "gpt-6-luna", "error": { "message": "The image data you provided does not represent a valid image. Please check your input and try again with one of the supported image formats: ['image/jpeg', 'image/png', 'image/gif', 'image/webp'].",
            "type": "invalid_request_error", "param": "input", "code": "invalid_value" } }),
    ))
    .await;
    assert!(matches!(err, AiError::InvalidImage(_)), "{err:?}");
    for status in [429, 500, 503] {
        match run(Reply::Error(status, "server_error")).await {
            AiError::Unavailable { retryable, .. } => assert!(retryable, "{status}"),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn responses_slow_call_reports_a_timeout() {
    let e = env().await;
    let (url, _) = start_responses(vec![Reply::Slow(Duration::from_secs(3))]).await;
    let mut c = rs_config(&url);
    c.request_timeout = Duration::from_secs(1);
    let err = RigImportAgent::new(c)
        .unwrap()
        .run(e.ctx.clone(), input())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AiError::Unavailable {
            retryable: false,
            detail: "model call timed out after 1 s".into()
        }
    );
}
