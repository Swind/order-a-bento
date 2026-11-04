use std::io::Write;
use std::sync::{Arc, Mutex};

use application::ai_import::{
    AgentInput, AgentOutcome, AiError, FakeImportAgent, FakeToolCall, ImportAgent,
    ImportToolContext,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use infrastructure::sqlite::Db;
use serde_json::{Value, json};
use server::{
    config::{Config, LogFormat},
    logging::subscriber,
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::EnvFilter;

const PRIVATE: &str = "PRIVATE_PROMPT_QUERY_HEADER_TOKEN_ERROR";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}
impl Capture {
    fn dispatch(&self, format: LogFormat, filter: &str) -> tracing::Dispatch {
        tracing::Dispatch::new(subscriber(format, EnvFilter::new(filter), self.clone()))
    }
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
    fn json(&self) -> Vec<Value> {
        let text = self.text();
        assert!(!text.contains('\x1b'), "ANSI in JSON: {text}");
        assert!(!text.contains(PRIVATE), "private data leaked: {text}");
        let lines: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(!lines.is_empty());
        for line in &lines {
            assert!(line["timestamp"].as_str().unwrap().contains('T'));
            assert!(line["level"].is_string() && line["target"].is_string());
            if let Some(duration) = line["fields"].get("duration_ms") {
                assert!(duration.is_u64(), "non-numeric duration: {line}");
            }
        }
        lines
    }
}

#[test]
fn shared_formatter_preserves_types_metadata_newlines_hierarchy_and_filter() {
    for format in [LogFormat::Text, LogFormat::Json] {
        let capture = Capture::default();
        tracing::dispatcher::with_default(&capture.dispatch(format, "info"), || {
            let request = tracing::info_span!("request", request_id = "synthetic-request");
            let _request = request.enter();
            let turn = tracing::info_span!(
                "ai_draft_turn",
                run = "synthetic-run",
                turn = "synthetic-turn"
            );
            let _turn = turn.enter();
            tracing::info!(
                input_tokens = Some(123_u64),
                output_tokens = None::<u64>,
                model = Some("safe-model"),
                duration_ms = 12_u64,
                timestamp = "event-time",
                "message\nsecond line"
            );
            tracing::debug!("filtered-debug");
        });
        assert!(!capture.text().contains("filtered-debug"));
        if format == LogFormat::Json {
            let lines = capture.json();
            assert_eq!(lines.len(), 1);
            let line = &lines[0];
            assert_eq!(line["fields"]["message"], "message\nsecond line");
            assert_eq!(line["fields"]["timestamp"], "event-time");
            assert_eq!(line["fields"]["input_tokens"], 123);
            assert!(line["fields"]["output_tokens"].is_null());
            assert_eq!(line["fields"]["model"], "safe-model");
            assert_eq!(line["fields"]["duration_ms"], 12);
            assert_eq!(line["spans"][0]["request_id"], "synthetic-request");
            assert_eq!(line["spans"][1]["run"], "synthetic-run");
            assert_eq!(line["span"]["turn"], "synthetic-turn");
        } else {
            assert!(capture.text().contains("message\nsecond line"));
            assert!(capture.text().contains("input_tokens") && capture.text().contains("123"));
        }
    }
}

#[test]
fn native_turn_strings_are_exact_in_json_and_cannot_forge_text_lines() {
    let turn_id = "non-uuid-turn\nFORGED\r\n\"quoted\"\\tail";
    for format in [LogFormat::Text, LogFormat::Json] {
        let capture = Capture::default();
        tracing::dispatcher::with_default(&capture.dispatch(format, "info"), || {
            let span = tracing::info_span!("ai_draft_turn", turn = tracing::field::Empty);
            span.record("turn", turn_id);
            let _entered = span.enter();
            for message in [
                "AI turn image cleanup failed",
                "AI turn metrics persistence failed",
            ] {
                tracing::error!(turn = turn_id, "{message}");
            }
        });
        let text = capture.text();
        assert_eq!(text.lines().count(), 2, "forged physical line: {text}");
        assert!(!text.contains("\nFORGED") && !text.contains("\r\n"));
        if format == LogFormat::Json {
            for line in capture.json() {
                assert_eq!(line["fields"]["turn"], turn_id);
                assert_eq!(line["span"]["turn"], turn_id);
                assert_eq!(line["spans"][0]["turn"], turn_id);
            }
        } else {
            assert!(text.contains(r#"non-uuid-turn\nFORGED\r\n\"quoted\"\\tail"#));
        }
    }
}

struct LoggedAgent;
#[async_trait::async_trait]
impl ImportAgent for LoggedAgent {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        tokio::task::yield_now().await;
        if input
            .instruction
            .as_deref()
            .is_some_and(|s| s == "fail" || s.ends_with("Organizer instruction: fail"))
        {
            return Err(AiError::Unavailable {
                retryable: false,
                detail: PRIVATE.into(),
            });
        }
        ctx.observe_usage(Some(11), Some(3));
        ctx.observe_model("safe-model");
        let calls = if ctx.editing.lock().await.is_some() {
            vec![FakeToolCall::SubmitDraftEdits(vec![])]
        } else {
            vec![FakeToolCall::ProposeVendor(serde_json::from_value(json!({
                "target":"new", "info":{"name":"synthetic-vendor"}, "menuItems":[], "warnings":[]
            })).unwrap())]
        };
        FakeImportAgent::new(calls, PRIVATE).run(ctx, input).await
    }
}

async fn app() -> Router {
    // Never read dotenv or process credentials.
    let config = Config::from_map(
        &[
            ("DEV_LOGIN".into(), "true".into()),
            ("AI_DEMO".into(), "true".into()),
            ("TRUSTED_PROXIES".into(), "10.0.0.5".into()),
        ]
        .into(),
    )
    .unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let (mut service, _) = server::wiring::ai_import_service(&config, &db)
        .unwrap()
        .unwrap();
    service.agent = Arc::new(LoggedAgent);
    api::router(
        server::wiring::build_state(&config, db)
            .unwrap()
            .with_ai_import(service, false)
            .with_ai_rate_limit(api::RateLimitConfig {
                burst: 100,
                per_sec: 100.0,
            }),
    )
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    content_type: &str,
    body: impl Into<Body>,
) -> (StatusCode, Value, String, String) {
    let request = Request::builder()
        .method(method)
        .uri(format!("{path}?private={PRIVATE}"))
        .header(header::COOKIE, cookie)
        .header(header::AUTHORIZATION, PRIVATE)
        .header("x-request-id", PRIVATE)
        .header(header::CONTENT_TYPE, content_type)
        .body(body.into())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value, id, cookie)
}

async fn initial(
    app: &Router,
    cookie: &str,
    instruction: &str,
) -> (StatusCode, Value, String, String) {
    let mut body = b"--BOUNDARY\r\nContent-Disposition: form-data; name=\"image\"; filename=\"private.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n".to_vec();
    body.extend_from_slice(&[
        0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0xFF, 0xD9,
    ]);
    body.extend_from_slice(format!("\r\n--BOUNDARY\r\nContent-Disposition: form-data; name=\"mode\"\r\n\r\nvendor\r\n--BOUNDARY\r\nContent-Disposition: form-data; name=\"instruction\"\r\n\r\n{instruction}\r\n--BOUNDARY--\r\n").as_bytes());
    call(
        app,
        "POST",
        "/api/v1/ai/import-runs",
        cookie,
        "multipart/form-data; boundary=BOUNDARY",
        body,
    )
    .await
}

fn event<'a>(lines: &'a [Value], name: &str, id: &str) -> &'a Value {
    let matches: Vec<_> = lines
        .iter()
        .filter(|v| v["fields"]["event"] == name && v["spans"][0]["request_id"] == id)
        .collect();
    assert_eq!(matches.len(), 1, "{name} {id}: {lines:?}");
    matches[0]
}

#[tokio::test]
async fn api_logs_resolve_client_ip_only_through_trusted_proxies() {
    let capture = Capture::default();
    async {
        let app = app().await;
        for (peer, xff, expected) in [
            (
                Some("10.0.0.5:1234"),
                "192.0.2.99, 198.51.100.7",
                Some("198.51.100.7"),
            ),
            (
                Some("203.0.113.9:1234"),
                "198.51.100.7",
                Some("203.0.113.9"),
            ),
            (Some("10.0.0.5:1234"), "not-an-ip", Some("10.0.0.5")),
            (Some("10.0.0.5:1234"), "2001:db8::7", Some("2001:db8::7")),
            (None, "198.51.100.7", None),
        ] {
            let mut request = Request::builder()
                .uri("/api/v1/does-not-exist")
                .header("x-forwarded-for", xff)
                .body(Body::empty())
                .unwrap();
            if let Some(peer) = peer {
                request.extensions_mut().insert(axum::extract::ConnectInfo(
                    peer.parse::<std::net::SocketAddr>().unwrap(),
                ));
            }
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let id = response.headers()["x-request-id"].to_str().unwrap();
            let lines = capture.json();
            let line = lines
                .iter()
                .find(|v| v["span"]["request_id"] == id)
                .unwrap();
            assert_eq!(line["span"]["client_ip"].as_str(), expected);
        }
    }
    .with_subscriber(capture.dispatch(LogFormat::Json, "info"))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_http_ai_replay_failure_and_parallel_requests_keep_safe_context() {
    let capture = Capture::default();
    async {
        let app = app().await;
        let (status, _, _, cookie) = call(&app, "POST", "/auth/dev-login", "", "application/json", json!({"email":"synthetic@example.com", "name":"Synthetic"}).to_string()).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (a, b) = tokio::join!(initial(&app, &cookie, PRIVATE), initial(&app, &cookie, PRIVATE));
        assert_eq!(a.0, StatusCode::CREATED, "{:?}", a.1);
        assert_eq!(b.0, StatusCode::CREATED, "{:?}", b.1);
        assert_ne!(a.2, b.2);
        let run = a.1["id"].as_str().unwrap();
        let path = format!("/api/v1/ai/import-runs/{run}");
        let (_, draft, _, _) = call(&app, "GET", &format!("{path}/draft"), &cookie, "application/json", String::new()).await;
        let body = json!({"revision":draft["revision"], "turnId":"safe-turn\nFORGED", "instruction":PRIVATE, "vendors":draft["vendors"]}).to_string();
        let (status, edited, turn_id, _) = call(&app, "POST", &format!("{path}/turns"), &cookie, "application/json", body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, replay_id, _) = call(&app, "POST", &format!("{path}/turns"), &cookie, "application/json", body).await;
        assert_eq!(status, StatusCode::OK);
        let body = json!({"revision":edited["revision"], "turnId":"failed-turn", "instruction":"fail", "vendors":edited["vendors"]}).to_string();
        let (status, failed_draft, failed_turn_id, _) = call(&app, "POST", &format!("{path}/turns"), &cookie, "application/json", body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(failed_draft["turns"][1]["status"], "failed");
        let (status, _, failure_id, _) = initial(&app, &cookie, "fail").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        for (path, expected) in [("/healthz".into(), StatusCode::OK), (format!("/api/v1/public/g/{PRIVATE}"), StatusCode::NOT_FOUND)] {
            assert_eq!(call(&app, "GET", &path, &cookie, "application/json", String::new()).await.0, expected);
        }
        for path in ["/", "/assets/app.js", "/assets/app.css", "/favicon.ico", "/vendors", "/apiary"] {
            let (_, _, id, _) = call(&app, "GET", path, &cookie, "application/json", String::new()).await;
            assert!(!id.is_empty());
        }
        let lines = capture.json();
        assert!(!capture.text().contains(&cookie));
        for response in [&a, &b] {
            let completed = event(&lines, "ai_import_completed", &response.2);
            assert_eq!(completed["spans"][1]["run"], response.1["id"]);
            assert_eq!(completed["spans"][1]["turn"], "initial");
            assert_eq!(completed["fields"]["input_tokens"], 11);
            assert_eq!(completed["fields"]["output_tokens"], 3);
            assert_eq!(completed["fields"]["model"], "safe-model");
        }
        let turn = event(&lines, "ai_turn_completed", &turn_id);
        assert_eq!(turn["spans"][1]["run"], run);
        assert_eq!(turn["spans"][1]["turn"], "safe-turn\nFORGED");
        assert_eq!(turn["span"]["turn"], "safe-turn\nFORGED");
        let replay = event(&lines, "ai_turn_replay", &replay_id);
        assert_eq!(replay["fields"]["quota_reserved"], false);
        assert_eq!(replay["fields"]["execution"], "not_started");
        assert_eq!(replay["spans"][1]["turn"], "safe-turn\nFORGED");
        assert_eq!(replay["span"]["turn"], "safe-turn\nFORGED");
        assert!(!lines.iter().any(|v| v["spans"][0]["request_id"] == replay_id && v["fields"]["event"] == "ai_turn_started"));
        let failed = event(&lines, "ai_import_completed", &failure_id);
        assert_eq!(failed["fields"]["error_code"], "unavailable");
        assert!(lines.iter().any(|v| v["span"]["request_id"] == failure_id && v["fields"]["classification"] == "Status code: 502 Bad Gateway" && v["fields"]["duration_ms"].is_u64()));
        for field in ["input_tokens", "output_tokens", "model"] { assert!(failed["fields"][field].is_null()); }
        let failed = event(&lines, "ai_turn_completed", &failed_turn_id);
        assert_eq!(failed["fields"]["error_code"], "unavailable");
        assert_eq!(failed["fields"]["execution"], "failed");
        for field in ["input_tokens", "output_tokens", "model"] { assert!(failed["fields"][field].is_null()); }
        let responses: Vec<_> = lines.iter().filter(|v| v["fields"]["message"] == "finished processing request").collect();
        assert_eq!(responses.len(), 8); // two imports, draft, turn, replay, failed turn/import, public
        for line in responses {
            assert!(line["span"]["path"].as_str().unwrap().starts_with("/api/"));
            assert!(line["fields"]["status"].is_u64());
            assert!(line["fields"]["duration_ms"].is_u64());
            assert!(line["span"]["request_id"].is_string());
        }
        assert!(lines.iter().any(|v| v["span"]["path"] == "/api/v1/public/g/:token"));
        assert!(!capture.text().contains("\nFORGED"));
    }.with_subscriber(capture.dispatch(LogFormat::Json, "warn,tower_http=info,api::logging=info,application::ai_import=info")).await;
}
