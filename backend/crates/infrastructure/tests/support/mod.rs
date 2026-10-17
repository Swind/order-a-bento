//! Local mock of Anthropic's `POST /v1/messages` (unary), scripted per call.
//! Records every request body and its headers.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};

pub enum Reply {
    /// 200 with this message.
    Message(Value),
    /// Error status with an Anthropic-style error body.
    Error(u16, &'static str),
}

pub struct Mock {
    script: Vec<Reply>,
    pub bodies: Mutex<Vec<Value>>,
    pub headers: Mutex<Vec<Value>>,
}

pub async fn start(script: Vec<Reply>) -> (String, Arc<Mock>) {
    let mock = Arc::new(Mock {
        script,
        bodies: Mutex::new(vec![]),
        headers: Mutex::new(vec![]),
    });
    let app = Router::new()
        .route("/v1/messages", post(messages))
        .with_state(mock.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, mock)
}

impl Mock {
    pub fn calls(&self) -> usize {
        self.bodies.lock().unwrap().len()
    }
}

async fn messages(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let n = {
        let mut b = mock.bodies.lock().unwrap();
        b.push(body);
        b.len() - 1
    };
    let h: serde_json::Map<String, Value> = headers
        .iter()
        .map(|(k, v)| (k.to_string(), json!(v.to_str().unwrap_or("?"))))
        .collect();
    mock.headers.lock().unwrap().push(Value::Object(h));
    match mock.script.get(n) {
        Some(Reply::Message(m)) => Json(m.clone()).into_response(),
        Some(Reply::Error(status, kind)) => (
            StatusCode::from_u16(*status).unwrap(),
            Json(json!({ "type": "error", "error": { "type": kind, "message": "mock error" } })),
        )
            .into_response(),
        None => (StatusCode::BAD_REQUEST, "mock: unexpected extra request").into_response(),
    }
}

pub fn msg(content: Value, stop: &str, extra: Value) -> Value {
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

pub fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({ "type": "tool_use", "id": id, "name": name, "input": input })
}

pub fn tool_turn(blocks: Vec<Value>) -> Value {
    msg(Value::Array(blocks), "tool_use", json!({}))
}

pub fn end_turn(text: &str) -> Value {
    msg(
        json!([{ "type": "text", "text": text }]),
        "end_turn",
        json!({}),
    )
}
