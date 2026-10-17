//! Minimal mock of POST /v1/messages (unary + SSE).
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scenario {
    Happy,
    Refusal,
    /// propose_vendor with price:"abc" (schema violation), then a valid call, then end_turn
    InvalidArgs,
    /// propose_vendor with price:-5 (passes serde, fails business validation)
    BusinessInvalid,
    Overloaded529,
    Server500,
    /// never answers (for timeout test)
    Hang,
}

pub struct MockState {
    pub scenario: Scenario,
    pub calls: AtomicUsize,
    pub bodies: Mutex<Vec<Value>>,
    pub headers: Mutex<Vec<Value>>,
}

pub async fn start(scenario: Scenario) -> (String, Arc<MockState>) {
    let state = Arc::new(MockState {
        scenario,
        calls: AtomicUsize::new(0),
        bodies: Mutex::new(vec![]),
        headers: Mutex::new(vec![]),
    });
    let app = Router::new().route("/v1/messages", post(messages)).with_state(state.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, state)
}

fn msg(content: Value, stop: &str, extra: Value) -> Value {
    let mut m = json!({
        "id": "msg_mock", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
        "content": content, "stop_reason": stop, "stop_sequence": null,
        "usage": { "input_tokens": 1000, "output_tokens": 50 }
    });
    if let (Some(o), Some(e)) = (m.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            o.insert(k.clone(), v.clone());
        }
    }
    m
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({ "type": "tool_use", "id": id, "name": name, "input": input })
}

fn good_args() -> Value {
    json!({
        "vendorId": 7,
        "info": { "name": "阿財排骨便當", "phone": "02-2345-6789", "address": null, "note": "滿 10 個外送" },
        "menuItems": [
            { "name": "排骨飯", "price": 85, "description": null, "category": "飯類", "confidence": "high" },
            { "name": "雞腿飯", "price": null, "description": "價格被摺痕遮住", "category": "飯類", "confidence": "low" }
        ],
        "warnings": ["『雞腿飯』價格被摺痕遮住"]
    })
}

fn pick(s: Scenario, n: usize) -> Option<Value> {
    let propose = |id: &str, args: Value| msg(json!([tool_use(id, "propose_vendor", args)]), "tool_use", json!({}));
    Some(match (s, n) {
        (Scenario::Happy, 1) => msg(
            json!([{ "type": "text", "text": "先查現有店家。" }, tool_use("toolu_01", "list_vendors", json!({}))]),
            "tool_use",
            json!({}),
        ),
        (Scenario::Happy, 2) => propose("toolu_02", good_args()),
        (Scenario::Refusal, _) => msg(
            json!([]),
            "refusal",
            json!({ "stop_details": { "type": "refusal", "category": "cyber", "explanation": "mock: image triggered classifier" } }),
        ),
        (Scenario::InvalidArgs, 1) => {
            let mut a = good_args();
            a["menuItems"][0]["price"] = json!("abc");
            propose("toolu_01", a)
        }
        (Scenario::InvalidArgs, 2) | (Scenario::BusinessInvalid, 2) => propose("toolu_02", good_args()),
        (Scenario::BusinessInvalid, 1) => {
            let mut a = good_args();
            a["menuItems"][0]["price"] = json!(-5);
            propose("toolu_01", a)
        }
        (Scenario::Happy, 3) | (Scenario::InvalidArgs, 3) | (Scenario::BusinessInvalid, 3) => pick_end(),
        _ => return None,
    })
}

async fn messages(State(st): State<Arc<MockState>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let n = st.calls.fetch_add(1, Ordering::SeqCst) + 1;
    st.bodies.lock().unwrap().push(body.clone());
    let h: serde_json::Map<String, Value> = headers
        .iter()
        .map(|(k, v)| (k.to_string(), json!(if k == "x-api-key" { "<redacted>" } else { v.to_str().unwrap_or("?") })))
        .collect();
    st.headers.lock().unwrap().push(Value::Object(h));
    match st.scenario {
        Scenario::Overloaded529 => {
            return (
                StatusCode::from_u16(529).unwrap(),
                Json(json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } })),
            )
                .into_response()
        }
        Scenario::Server500 => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "type": "error", "error": { "type": "api_error", "message": "Internal server error" } })),
            )
                .into_response()
        }
        Scenario::Hang => {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
        _ => {}
    }
    let Some(m) = pick(st.scenario, n) else {
        return (StatusCode::BAD_REQUEST, "mock: unexpected extra request").into_response();
    };
    if body.get("stream").and_then(Value::as_bool) == Some(true) {
        return sse(&m);
    }
    Json(m).into_response()
}

/// Render a complete message as an Anthropic SSE stream.
fn sse(m: &Value) -> Response {
    let mut out = String::new();
    let mut ev = |name: &str, data: Value| out.push_str(&format!("event: {name}\ndata: {data}\n\n"));
    let mut start = m.clone();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    start["usage"] = json!({ "input_tokens": m["usage"]["input_tokens"], "output_tokens": 1 });
    ev("message_start", json!({ "type": "message_start", "message": start }));
    for (i, block) in m["content"].as_array().unwrap().iter().enumerate() {
        match block["type"].as_str().unwrap() {
            "text" => {
                ev("content_block_start", json!({ "type": "content_block_start", "index": i, "content_block": { "type": "text", "text": "" } }));
                ev("content_block_delta", json!({ "type": "content_block_delta", "index": i, "delta": { "type": "text_delta", "text": block["text"] } }));
            }
            _ => {
                ev("content_block_start", json!({ "type": "content_block_start", "index": i, "content_block": { "type": "tool_use", "id": block["id"], "name": block["name"], "input": {} } }));
                ev("content_block_delta", json!({ "type": "content_block_delta", "index": i, "delta": { "type": "input_json_delta", "partial_json": block["input"].to_string() } }));
            }
        }
        ev("content_block_stop", json!({ "type": "content_block_stop", "index": i }));
    }
    let mut delta = json!({ "stop_reason": m["stop_reason"], "stop_sequence": null });
    if let Some(d) = m.get("stop_details") {
        delta["stop_details"] = d.clone();
    }
    ev("message_delta", json!({ "type": "message_delta", "delta": delta, "usage": { "output_tokens": m["usage"]["output_tokens"] } }));
    ev("message_stop", json!({ "type": "message_stop" }));
    ([("content-type", "text/event-stream")], out).into_response()
}

fn pick_end() -> Value {
    msg(json!([{ "type": "text", "text": "已提出阿財排骨便當的菜單，請確認雞腿飯的價格。" }]), "end_turn", json!({}))
}
