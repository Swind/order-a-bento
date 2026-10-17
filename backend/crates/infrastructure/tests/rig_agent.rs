//! `RigImportAgent` against a local mock of the Anthropic API.

mod support;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use application::ai_import::{AgentInput, AiError, ImageInput, ImportAgent, ImportToolContext};
use application::ports::OrganizerRepo;
use domain::ai_import::{ImportMode, ImportRunId, ProposalTarget};
use domain::identity::{Organizer, OrganizerId};
use domain::shared::SystemClock;
use infrastructure::ai::{RigAgentConfig, RigImportAgent, import_tools_parameters};
use infrastructure::directory_repo::{SqliteRosterRepo, SqliteVendorRepo};
use infrastructure::identity_repo::SqliteOrganizerRepo;
use infrastructure::sqlite::{Db, TempDir};
use serde_json::{Value, json};
use support::{Mock, Reply, end_turn, msg, start, tool_turn, tool_use};

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
        mode_hint: Some(ImportMode::Vendor),
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
    assert_eq!(tools.len(), 6);
    for t in tools {
        assert_eq!(t["strict"], true, "tool {} not strict", t["name"]);
        assert_eq!(t["input_schema"]["additionalProperties"], false);
    }
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in [
        "list_rosters",
        "get_roster",
        "list_vendors",
        "get_vendor",
        "propose_roster_members",
        "propose_vendor",
    ] {
        assert!(names.contains(&n), "missing {n}");
    }
    // system prompt + text and image in the user message
    assert!(
        body["system"][0]["text"]
            .as_str()
            .unwrap()
            .contains("資料輸入助理")
    );
    let content = body["messages"][0]["content"].as_array().unwrap();
    let text = content[0]["text"].as_str().unwrap();
    assert!(text.contains("模式提示：vendor"));
    assert!(text.contains("開團者指示：這是午餐店的菜單"));
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
