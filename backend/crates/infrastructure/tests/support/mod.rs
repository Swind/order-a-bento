//! Local mocks of Anthropic's `POST /v1/messages` and OpenAI's
//! `POST /v1/chat/completions` and `POST /v1/responses` (unary), scripted per call.
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
    /// Error status with exactly this JSON body.
    Raw(u16, Value),
    /// Sleep this long, then answer 200 with an empty message (a slow gateway).
    Slow(std::time::Duration),
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

/// Like [`start`], but serving OpenAI Chat Completions at `/v1/chat/completions`.
pub async fn start_openai(script: Vec<Reply>) -> (String, Arc<Mock>) {
    let mock = Arc::new(Mock {
        script,
        bodies: Mutex::new(vec![]),
        headers: Mutex::new(vec![]),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(mock.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, mock)
}

/// Like [`start`], but serving the OpenAI Responses API at `/v1/responses`.
pub async fn start_responses(script: Vec<Reply>) -> (String, Arc<Mock>) {
    let mock = Arc::new(Mock {
        script,
        bodies: Mutex::new(vec![]),
        headers: Mutex::new(vec![]),
    });
    let app = Router::new()
        .route("/v1/responses", post(responses))
        .with_state(mock.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, mock)
}

#[derive(Clone, Copy, PartialEq)]
enum Wire {
    Messages,
    Chat,
    Responses,
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
    reply(mock, headers, body, Wire::Messages).await
}

async fn chat_completions(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    reply(mock, headers, body, Wire::Chat).await
}

async fn responses(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    reply(mock, headers, body, Wire::Responses).await
}

async fn reply(mock: Arc<Mock>, headers: HeaderMap, body: Value, wire: Wire) -> Response {
    let openai = wire != Wire::Messages;
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
    if let Some(Reply::Slow(d)) = mock.script.get(n) {
        tokio::time::sleep(*d).await;
    }
    match mock.script.get(n) {
        Some(Reply::Slow(_)) if wire == Wire::Responses => {
            Json(responses_reply(json!([]))).into_response()
        }
        Some(Reply::Slow(_)) if openai => Json(chat_reply(
            json!({ "role": "assistant", "content": "" }),
            "stop",
        ))
        .into_response(),
        Some(Reply::Slow(_)) => Json(msg(json!([]), "end_turn", json!({}))).into_response(),
        Some(Reply::Message(m)) => Json(m.clone()).into_response(),
        Some(Reply::Raw(status, body)) => {
            (StatusCode::from_u16(*status).unwrap(), Json(body.clone())).into_response()
        }
        Some(Reply::Error(status, kind)) if openai => (
            StatusCode::from_u16(*status).unwrap(),
            Json(json!({ "error": { "type": kind, "message": "mock error" } })),
        )
            .into_response(),
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

/// An OpenAI chat completion with `message` and `finish_reason`.
pub fn chat_reply(message: Value, finish_reason: &str) -> Value {
    json!({
        "id": "chatcmpl-mock", "object": "chat.completion", "created": 1, "model": "deepseek-v4.1-flash",
        "choices": [{ "index": 0, "message": message, "finish_reason": finish_reason }],
        "usage": {
            "prompt_tokens": 1000, "completion_tokens": 50, "total_tokens": 1050,
            "completion_tokens_details": { "reasoning_tokens": 20 }
        }
    })
}

pub fn chat_tool_call(id: &str, name: &str, args: Value) -> Value {
    json!({ "id": id, "type": "function", "function": { "name": name, "arguments": args.to_string() } })
}

/// An assistant turn that calls tools, with optional DeepSeek-style thinking.
pub fn chat_tool_turn(reasoning: Option<&str>, calls: Vec<Value>) -> Value {
    let mut m = json!({ "role": "assistant", "content": null, "tool_calls": calls });
    if let Some(r) = reasoning {
        m["reasoning_content"] = json!(r);
    }
    chat_reply(m, "tool_calls")
}

pub fn chat_end_turn(text: &str) -> Value {
    chat_reply(json!({ "role": "assistant", "content": text }), "stop")
}

/// An OpenAI Responses API reply with these `output` items.
pub fn responses_reply(output: Value) -> Value {
    json!({
        "id": "resp_mock", "object": "response", "created_at": 1, "status": "completed",
        "model": "gpt-6-luna", "output": output,
        "usage": {
            "input_tokens": 1000, "output_tokens": 50, "total_tokens": 1050,
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens_details": { "reasoning_tokens": 20 }
        }
    })
}

pub fn responses_call(id: &str, name: &str, args: Value) -> Value {
    json!({ "type": "function_call", "id": format!("fc_{id}"), "call_id": id, "name": name,
        "arguments": args.to_string(), "status": "completed" })
}

pub fn responses_reasoning(id: &str, ciphertext: &str) -> Value {
    json!({ "type": "reasoning", "id": id, "summary": [], "encrypted_content": ciphertext })
}

pub fn responses_text(text: &str) -> Value {
    responses_reply(json!([{
        "type": "message", "id": "msg_mock", "role": "assistant", "status": "completed",
        "content": [{ "type": "output_text", "text": text, "annotations": [] }]
    }]))
}
