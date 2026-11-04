use super::*;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::instrument::WithSubscriber;

const PROMPT: &str = "PRIVATE_PROMPT_CONTENT";
const IMAGE: &str = "PRIVATE_IMAGE_CONTENT";
const QUERY: &str = "PRIVATE_QUERY_CONTENT";
const AUTH: &str = "PRIVATE_AUTHORIZATION_CONTENT";
const COOKIE: &str = "PRIVATE_COOKIE_CONTENT";
const KEY: &str = "PRIVATE_AI_KEY_CONTENT";
const CLIENT_ID: &str = "UNTRUSTED_CLIENT_REQUEST_ID";

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
    fn dispatch(&self) -> tracing::Dispatch {
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .without_time()
                .with_ansi(false)
                .with_max_level(tracing::Level::INFO)
                .with_writer(self.clone())
                .finish(),
        )
    }
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn request(c: &Client, method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(format!("{path}?diagnostic={QUERY}"))
        .header(header::COOKIE, format!("{}; private={COOKIE}", c.cookie))
        .header(header::AUTHORIZATION, format!("Bearer {AUTH}"))
        .header("x-request-id", CLIENT_ID)
}
fn response_id(headers: &axum::http::HeaderMap) -> String {
    let id = headers["x-request-id"].to_str().unwrap();
    assert_ne!(id, CLIENT_ID);
    assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
    id.into()
}
async fn initial(c: &Client, instruction: &str) -> (StatusCode, Value, String) {
    let image = [JPEG, IMAGE.as_bytes()].concat();
    let body = multipart(&[
        ("image", Some("private.jpg"), &image),
        ("mode", None, b"vendor"),
        ("instruction", None, instruction.as_bytes()),
    ]);
    let req = request(c, "POST", "/api/v1/ai/import-runs")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    let (status, body, headers) = raw(&c.app, req).await;
    (status, body, response_id(&headers))
}
async fn chat(c: &Client, path: &str, body: &Value) -> (StatusCode, Value, String) {
    let req = request(c, "POST", path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, body, headers) = raw(&c.app, req).await;
    (status, body, response_id(&headers))
}
fn assert_private(log: &str, c: &Client) {
    for secret in [
        PROMPT,
        IMAGE,
        QUERY,
        AUTH,
        COOKIE,
        KEY,
        CLIENT_ID,
        c.cookie.as_str(),
    ] {
        assert!(!log.contains(secret), "secret leaked: {secret}\n{log}");
    }
}
fn event<'a>(log: &'a str, name: &str, request_id: &str) -> &'a str {
    let lines: Vec<_> = log
        .lines()
        .filter(|line| line.contains(name) && line.contains(request_id))
        .collect();
    assert_eq!(lines.len(), 1, "{name} for {request_id}: {log}");
    lines[0]
}

#[test]
fn http_service_failure_does_not_print_raw_error() {
    use tower_http::classify::ServerErrorsFailureClass;
    use tower_http::trace::OnFailure;
    let capture = Capture::default();
    tracing::dispatcher::with_default(&capture.dispatch(), || {
        crate::logging::FailureLog.on_failure(
            ServerErrorsFailureClass::Error(format!("provider echoed {KEY} {PROMPT}")),
            std::time::Duration::from_millis(12),
            &tracing::Span::none(),
        );
    });
    let log = capture.text();
    assert!(log.contains("service_error") && log.contains("duration_ms=12"));
    assert!(!log.contains(KEY) && !log.contains(PROMPT));
}

struct LoggedAgent(AtomicUsize);
#[async_trait]
impl ImportAgent for LoggedAgent {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        ctx.observe_usage(Some(11), Some(3));
        ctx.observe_model("safe-model");
        if ctx.editing.lock().await.is_none() {
            fake_vendor_agent().run(ctx.clone(), input).await?;
        }
        Ok(AgentOutcome {
            summary: PROMPT.into(),
            usage: Default::default(),
            model: "safe-model".into(),
        })
    }
}

struct UnreportedDraftFailure;
#[async_trait]
impl ImportAgent for UnreportedDraftFailure {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        tokio::task::yield_now().await;
        if ctx.editing.lock().await.is_some() {
            Err(AiError::Unavailable {
                retryable: false,
                detail: format!("provider echoed {PROMPT} {IMAGE} {KEY}"),
            })
        } else {
            fake_vendor_agent().run(ctx, input).await
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_ai_and_replay_events_are_correlated_without_payloads_and_telemetry_errors_keep_context()
 {
    let capture = Capture::default();
    let dispatch = capture.dispatch();
    // Bind on every poll, not a thread-local guard held across await.
    async {
        let agent = Arc::new(LoggedAgent(AtomicUsize::new(0)));
        let c = client_with(test_app_ai(agent.clone(), 20, None, false).await).await;
        let (status, run, initial_id) = initial(&c, &format!("{PROMPT} {KEY}")).await;
        assert_eq!(status, StatusCode::CREATED, "{run}");
        let run_id = run["id"].as_str().unwrap();
        let path = format!("/api/v1/ai/import-runs/{run_id}/turns");
        let (_, draft) = c.get(&format!("/ai/import-runs/{run_id}/draft")).await;
        let body = json!({"revision":draft["revision"],"turnId":"safe-turn","instruction":PROMPT,"vendors":draft["vendors"]});
        let (status, edited, chat_id) = chat(&c, &path, &body).await;
        assert_eq!(status, StatusCode::OK, "{edited}");
        let (status, replay, replay_id) = chat(&c, &path, &body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(edited, replay);
        assert_ne!(initial_id, chat_id);
        assert_ne!(chat_id, replay_id);
        assert_eq!(agent.0.load(Ordering::SeqCst), 2);

        sqlx::query("CREATE TRIGGER reject_metrics BEFORE UPDATE ON ai_import_turn_usage BEGIN SELECT RAISE(ABORT, 'test telemetry failure'); END")
            .execute(&c._t.as_ref().unwrap().state.db.write).await.unwrap();
        let body = json!({"revision":edited["revision"],"turnId":"telemetry-turn","instruction":PROMPT,"vendors":edited["vendors"]});
        let (status, _, telemetry_id) = chat(&c, &path, &body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(agent.0.load(Ordering::SeqCst), 3);

        let log = capture.text();
        for (name, id) in [("ai_import_started", &initial_id), ("ai_import_completed", &initial_id), ("ai_turn_started", &chat_id), ("ai_turn_completed", &chat_id), ("ai_turn_replay", &replay_id)] {
            let line = event(&log, name, id);
            assert!(line.contains(run_id), "{line}");
            for field in ["duration_ms=", "execution=", "outcome="] { assert!(line.contains(field), "missing {field}: {line}"); }
        }
        let completed = event(&log, "ai_turn_completed", &chat_id);
        for value in ["safe-turn", "safe-model", "input_tokens=11", "output_tokens=3", "completed", "unchanged", "observed_tokens_not_invoice"] { assert!(completed.contains(value), "{completed}"); }
        let replay = event(&log, "ai_turn_replay", &replay_id);
        assert!(replay.contains("quota_reserved=false"));
        assert!(replay.contains("not_started"));
        assert!(!log.lines().any(|line| line.contains(&replay_id) && (line.contains("ai_turn_started") || line.contains("ai_turn_completed"))));
        let failure = event(&log, "AI turn metrics persistence failed", &telemetry_id);
        assert!(failure.contains(run_id) && failure.contains("telemetry-turn"));
        assert_private(&log, &c);
    }.with_subscriber(dispatch).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_initial_and_turn_requests_log_codes_and_unknown_usage_not_raw_provider_errors() {
    let capture = Capture::default();
    async {
        let detail = format!("provider echoed {PROMPT} {IMAGE} {KEY}");
        let c = client_with(test_app_ai(Arc::new(Failing(AiError::Unavailable { retryable: false, detail })), 10, None, false).await).await;
        let (status, body, id) = initial(&c, PROMPT).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        let log = capture.text();
        let line = event(&log, "ai_import_completed", &id);
        assert!(line.contains(body["runId"].as_str().unwrap()));
        assert!(line.contains("unavailable") && line.contains("rejected") && line.contains("failed"));
        assert!(line.contains("input_tokens=700"));
        assert!(!line.contains("model="));
        assert!(log.lines().any(|line| line.contains(&id) && line.contains("status=502")));
        assert_private(&log, &c);

        let c = client_with(test_app_ai(Arc::new(UnreportedDraftFailure), 10, None, false).await).await;
        let (_, run, _) = initial(&c, PROMPT).await;
        let run_id = run["id"].as_str().unwrap();
        let (_, draft) = c.get(&format!("/ai/import-runs/{run_id}/draft")).await;
        let body = json!({"revision":draft["revision"],"turnId":"failed-turn","instruction":PROMPT,"vendors":draft["vendors"]});
        let (status, out, id) = chat(&c, &format!("/api/v1/ai/import-runs/{run_id}/turns"), &body).await;
        assert_eq!(status, StatusCode::OK); // A failed model preserves the editable draft.
        assert_eq!(out["turns"][0]["status"], "failed");
        let log = capture.text();
        let line = event(&log, "ai_turn_completed", &id);
        for value in [run_id, "failed-turn", "unavailable", "unchanged", "failed"] { assert!(line.contains(value), "{line}"); }
        for field in ["input_tokens=", "output_tokens=", "model="] { assert!(!line.contains(field), "{line}"); }
        assert_private(&log, &c);
    }.with_subscriber(capture.dispatch()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn newline_turn_id_is_escaped_in_both_cleanup_and_metrics_failure_events() {
    let capture = Capture::default();
    async {
        let c = client().await;
        let (_, run, _) = initial(&c, PROMPT).await;
        let run_id = run["id"].as_str().unwrap();
        let (_, draft) = c.get(&format!("/ai/import-runs/{run_id}/draft")).await;
        let db = &c._t.as_ref().unwrap().state.db;
        sqlx::query("INSERT INTO ai_import_photos(run_id,data,width,height,expires_at) VALUES(?,?,1,1,'2100-01-01T00:00:00Z')")
            .bind(run_id).bind(real_png()).execute(&db.write).await.unwrap();
        // Reservation succeeds; image read, pending cleanup and metrics writes fail.
        sqlx::raw_sql("ALTER TABLE ai_import_photos RENAME TO saved_photos;
            CREATE VIEW ai_import_photos AS SELECT run_id,width,height,expires_at,abs(-9223372036854775808) AS data FROM saved_photos;
            CREATE TRIGGER reject_cleanup BEFORE UPDATE ON ai_import_drafts WHEN json_extract(NEW.state_json,'$.pending') IS NULL BEGIN SELECT RAISE(ABORT, 'test cleanup failure'); END;
            CREATE TRIGGER reject_metrics BEFORE UPDATE ON ai_import_turn_usage BEGIN SELECT RAISE(ABORT, 'test metrics failure'); END;")
            .execute(&db.write).await.unwrap();
        let turn_id = "safe-turn\nFORGED_LOG_LINE";
        let body = json!({"revision":draft["revision"],"turnId":turn_id,"instruction":PROMPT,"vendors":draft["vendors"]});
        let (status, _, id) = chat(&c, &format!("/api/v1/ai/import-runs/{run_id}/turns"), &body).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let log = capture.text();
        for message in ["AI turn image cleanup failed", "AI turn metrics persistence failed"] {
            let line = event(&log, message, &id);
            assert!(line.contains(run_id));
            assert!(line.contains(r"safe-turn\nFORGED_LOG_LINE"), "{line}");
            assert!(line.contains(&format!("turn={turn_id:?}")), "{line}");
        }
        assert!(!log.contains(turn_id), "raw newline in turn ID: {log}");
        assert!(!log.contains("\nFORGED_LOG_LINE"), "forged physical line: {log}");
        assert_private(&log, &c);
    }.with_subscriber(capture.dispatch()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_subscribers_are_isolated_and_success_error_http_ids_keep_redacted_paths() {
    let a = Capture::default();
    let b = Capture::default();
    let mut c = client().await;
    c.app = crate::router_with_assets(
        c._t.as_ref().unwrap().state.clone(),
        Arc::new(crate::web::TestAssets(Some(
            "<html><head></head><body>test</body></html>",
        ))),
    );
    let token = "PRIVATE_PUBLIC_SHARE_TOKEN";
    let good = async {
        tokio::task::yield_now().await;
        let (status, _, headers) = raw(
            &c.app,
            request(&c, "GET", "/healthz").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        response_id(&headers)
    }
    .with_subscriber(a.dispatch());
    let bad = async {
        tokio::task::yield_now().await;
        let mut ids = Vec::new();
        for (path, expected) in [
            (format!("/api/v1/public/g/{token}"), StatusCode::NOT_FOUND),
            (format!("/g/{token}"), StatusCode::OK),
        ] {
            let (status, _, headers) = raw(
                &c.app,
                request(&c, "GET", &path).body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(status, expected);
            ids.push(response_id(&headers));
        }
        let (status, _, headers) = raw(
            &c.app,
            request(&c, "POST", "/api/v1/rosters")
                .header(header::ORIGIN, format!("https://{PROMPT}.test"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        ids.push(response_id(&headers));
        ids
    }
    .with_subscriber(b.dispatch());
    let (good_id, bad_ids) = tokio::join!(good, bad);
    let a = a.text();
    let b = b.text();
    assert!(a.contains(&good_id) && !b.contains(&good_id));
    for (i, bad_id) in bad_ids.iter().enumerate() {
        assert_ne!(&good_id, bad_id);
        assert!(!bad_ids[..i].contains(bad_id));
        assert!(b.contains(bad_id) && !a.contains(bad_id));
    }
    assert!(a.contains("method=GET") && a.contains("status=200") && a.contains("duration_ms="));
    assert!(b.contains("/api/v1/public/g/:token") && b.contains("status=404"));
    assert!(b.contains("/g/:token") && b.contains("status=403"));
    for log in [&a, &b] {
        assert!(!log.contains(token));
        assert_private(log, &c);
    }
}
