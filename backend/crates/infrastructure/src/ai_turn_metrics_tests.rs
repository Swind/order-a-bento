use super::*;
use application::ai_import::{ObservedUsage, TurnDraftOutcome, TurnExecution, TurnMetrics};
use application::sharing::SharingRepo;

#[derive(Debug, PartialEq, Eq)]
enum Behavior {
    Success,
    Failure,
    Timeout,
    Retry,
    Fallback,
    Missing,
    Partial,
    UnreportedFailure,
    Zero,
    Overflow,
}

struct Metered {
    calls: AtomicUsize,
    behavior: Behavior,
}
impl Metered {
    fn new(behavior: Behavior) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            behavior,
        })
    }
}
#[async_trait]
impl ImportAgent for Metered {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        _: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        let attempt = self.calls.fetch_add(1, Ordering::SeqCst);
        match self.behavior {
            Behavior::Fallback => {}
            Behavior::Missing => ctx.observe_usage(None, None),
            Behavior::Partial => ctx.observe_usage(Some(7), None),
            Behavior::UnreportedFailure => {}
            Behavior::Zero => ctx.add_usage(Usage::default()),
            Behavior::Overflow => {
                ctx.add_usage(Usage {
                    input_tokens: u64::MAX,
                    output_tokens: 1,
                });
                ctx.add_usage(Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                });
            }
            _ => {
                ctx.add_usage(Usage {
                    input_tokens: 4,
                    output_tokens: 2,
                });
                ctx.add_usage(Usage {
                    input_tokens: 6,
                    output_tokens: 3,
                });
            }
        }
        if self.behavior == Behavior::Timeout {
            return std::future::pending().await;
        }
        if matches!(
            self.behavior,
            Behavior::Failure | Behavior::UnreportedFailure
        ) || (self.behavior == Behavior::Retry && attempt == 0)
        {
            return Err(unavailable(self.behavior == Behavior::Retry));
        }
        ctx.submit_edits(vec![DraftEdit::UpdateItem {
            item_id: "v0-m0".into(),
            name: None,
            price: Some(775),
            category: None,
            description: None,
        }])
        .await
        .unwrap();
        if self.behavior != Behavior::Missing {
            ctx.observe_model("observed-model");
        }
        Ok(AgentOutcome {
            summary: "完成".into(),
            model: "observed-model".into(),
            usage: Usage {
                input_tokens: 90,
                output_tokens: 40,
            },
        })
    }
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct MetricsRow {
    finished_at: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    model: Option<String>,
    duration_ms: Option<i64>,
    execution_outcome: Option<String>,
    draft_outcome: Option<String>,
    error_code: Option<String>,
}
async fn metrics(e: &Env, run: &ImportRun, turn: &str) -> MetricsRow {
    sqlx::query_as("SELECT finished_at,input_tokens,output_tokens,model,duration_ms,execution_outcome,draft_outcome,error_code FROM ai_import_turn_usage WHERE run_id=? AND turn_id=?")
        .bind(run.id.to_string()).bind(turn).fetch_one(&e.db.read).await.unwrap()
}

#[tokio::test]
async fn success_retry_fallback_zero_and_unknown_usage_are_distinct_and_replay_is_free() {
    for (behavior, input, output, attempts) in [
        (Behavior::Success, Some(10), Some(5), 1),
        (Behavior::Retry, Some(20), Some(10), 2),
        (Behavior::Fallback, Some(90), Some(40), 1),
        (Behavior::Zero, Some(0), Some(0), 1),
        (Behavior::Missing, None, None, 1),
        (Behavior::Partial, Some(7), None, 1),
    ] {
        let e = env().await;
        let run = draft_run(&e).await;
        let agent = Metered::new(behavior);
        let svc = e.service(agent.clone(), 100);
        let state = svc.get_draft(e.org, run.id).await.unwrap();
        let request = turn(&state, "metered");
        let out = svc.turn(e.org, run.id, request).await.unwrap();
        assert_eq!(out.vendors[0].menu[0].price, Some(775));
        let row = metrics(&e, &run, "metered").await;
        assert_eq!((row.input_tokens, row.output_tokens), (input, output));
        assert_eq!(
            row.model.as_deref(),
            if agent.behavior == Behavior::Missing {
                None
            } else {
                Some("observed-model")
            }
        );
        assert_eq!(row.execution_outcome.as_deref(), Some("completed"));
        assert_eq!(row.draft_outcome.as_deref(), Some("applied"));
        assert!(row.finished_at.is_some());
        assert!(row.duration_ms.unwrap() >= 0);
        assert_eq!(row.error_code, None);
        assert_eq!(
            svc.turn(e.org, run.id, turn(&state, "metered"))
                .await
                .unwrap(),
            out
        );
        assert_eq!(metrics(&e, &run, "metered").await, row);
        assert_eq!(agent.calls.load(Ordering::SeqCst), attempts);
        assert_eq!(
            e.runs
                .count_since(e.org, SystemTime::UNIX_EPOCH)
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            e.runs.find(e.org, run.id).await.unwrap().unwrap().usage,
            run.usage
        );
        if agent.behavior == Behavior::Success {
            let next = svc.turn(e.org, run.id, turn(&out, "second")).await.unwrap();
            assert_eq!(next.turns.len(), 2);
            let second = metrics(&e, &run, "second").await;
            assert_eq!(
                (second.input_tokens, second.output_tokens),
                (Some(10), Some(5))
            );
            assert_eq!(second.draft_outcome.as_deref(), Some("unchanged"));
            assert_eq!(metrics(&e, &run, "metered").await, row);
            assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
            assert_eq!(
                e.runs
                    .count_since(e.org, SystemTime::UNIX_EPOCH)
                    .await
                    .unwrap(),
                3
            );
            assert_eq!(
                e.runs.find(e.org, run.id).await.unwrap().unwrap().usage,
                run.usage
            );
        }
    }
}

#[tokio::test]
async fn failure_and_timeout_preserve_observed_partial_usage_and_unknown_model() {
    for behavior in [
        Behavior::Failure,
        Behavior::Timeout,
        Behavior::UnreportedFailure,
    ] {
        let e = env().await;
        let run = draft_run(&e).await;
        let is_timeout = behavior == Behavior::Timeout;
        let unknown = behavior == Behavior::UnreportedFailure;
        let agent = Metered::new(behavior);
        let mut svc = e.service(agent, 100);
        svc.config.timeout = Duration::from_millis(10);
        let state = svc.get_draft(e.org, run.id).await.unwrap();
        let out = svc
            .turn(e.org, run.id, turn(&state, "failed"))
            .await
            .unwrap();
        assert_eq!(out.vendors, state.vendors);
        assert_eq!(out.turns[0].status, "failed");
        let row = metrics(&e, &run, "failed").await;
        assert_eq!(
            (row.input_tokens, row.output_tokens),
            if unknown {
                (None, None)
            } else {
                (Some(10), Some(5))
            }
        );
        assert_eq!(row.model, None);
        assert_eq!(row.execution_outcome.as_deref(), Some("failed"));
        assert_eq!(row.draft_outcome.as_deref(), Some("unchanged"));
        assert_eq!(
            row.error_code.as_deref(),
            Some(if is_timeout { "timeout" } else { "unavailable" })
        );
    }
}

struct GatedModel {
    agent: Arc<Metered>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait]
impl ImportAgent for GatedModel {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        let result = self.agent.run(ctx, input).await;
        self.entered.notify_one();
        self.release.notified().await;
        result
    }
}

#[tokio::test]
async fn completed_model_usage_survives_manual_cas_revocation_and_expiry_without_exposing_draft() {
    for action in ["manual", "revoked", "expired"] {
        let e = env().await;
        let groups = crate::sharing_repo::SqliteSharingRepo::new(e.db.clone());
        let group = groups.create(e.other, "metrics").await.unwrap();
        groups.add(e.other, group.id, e.org, "admin").await.unwrap();
        let run = draft_run_in_scope(&e, (action == "revoked").then_some(group.id)).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let svc = e.service(
            Arc::new(GatedModel {
                agent: Metered::new(Behavior::Success),
                entered: entered.clone(),
                release: release.clone(),
            }),
            100,
        );
        let state = svc.get_draft(e.org, run.id).await.unwrap();
        let work = svc.turn(e.org, run.id, turn(&state, "late"));
        let concurrent = async {
            entered.notified().await;
            match action {
                "manual" => {
                    let current = svc.get_draft(e.org, run.id).await.unwrap();
                    let mut vendors = current.vendors;
                    vendors[0].menu[0].price = Some(880);
                    svc.save_draft(e.org, run.id, current.revision, vendors)
                        .await
                        .unwrap();
                }
                "revoked" => {
                    groups.change(e.other, group.id, e.org, None).await.unwrap();
                }
                _ => {
                    sqlx::query(
                        "UPDATE ai_import_runs SET created_at='2000-01-01T00:00:00Z' WHERE id=?",
                    )
                    .bind(run.id.to_string())
                    .execute(&e.db.write)
                    .await
                    .unwrap();
                }
            }
            release.notify_one();
        };
        let (out, ()) = tokio::join!(work, concurrent);
        assert!(
            matches!(
                (&out, action),
                (Err(AiImportError::Conflict), "manual")
                    | (Err(AiImportError::NotFound), "revoked")
                    | (Err(AiImportError::InvalidState(_)), "expired")
            ),
            "{out:?}"
        );
        let row = metrics(&e, &run, "late").await;
        assert_eq!((row.input_tokens, row.output_tokens), (Some(10), Some(5)));
        assert_eq!(row.execution_outcome.as_deref(), Some("completed"));
        assert_eq!(row.draft_outcome.as_deref(), Some("rejected"));
        assert_eq!(
            row.error_code.as_deref(),
            Some(match action {
                "manual" => "stale",
                "revoked" => "access_revoked",
                _ => "invalid_state",
            })
        );
        let stored = e.runs.draft(&run).await.unwrap();
        assert_eq!(
            stored.vendors[0].menu[0].price,
            Some(if action == "manual" { 880 } else { 9000 })
        );
    }
}

#[tokio::test]
async fn image_read_failure_is_finalized_without_model_work_and_clears_pending() {
    let e = env().await;
    let run = photo_run(&e).await;
    // Reads of expiry still work; only materializing image data raises an error.
    sqlx::raw_sql("ALTER TABLE ai_import_photos RENAME TO saved_photos; CREATE VIEW ai_import_photos AS SELECT run_id,width,height,expires_at,abs(-9223372036854775808) AS data FROM saved_photos;")
        .execute(&e.db.write).await.unwrap();
    let agent = Metered::new(Behavior::Success);
    let svc = e.service(agent.clone(), 100);
    let state = svc.get_draft(e.org, run.id).await.unwrap();
    assert!(matches!(
        svc.turn(e.org, run.id, turn(&state, "image")).await,
        Err(AiImportError::Internal(_))
    ));
    let row = metrics(&e, &run, "image").await;
    assert_eq!(row.execution_outcome.as_deref(), Some("failed"));
    assert_eq!(row.error_code.as_deref(), Some("image_read"));
    assert_eq!(
        (row.input_tokens, row.output_tokens, row.model),
        (None, None, None)
    );
    let recovered = svc.get_draft(e.org, run.id).await.unwrap();
    assert!(recovered.pending.is_none());
    assert_eq!(recovered.turns[0].status, "failed");
    assert_eq!(
        svc.turn(e.org, run.id, turn(&state, "image"))
            .await
            .unwrap(),
        recovered
    );
    assert_eq!(agent.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        e.runs
            .count_since(e.org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn initial_run_usage_also_rejects_sql_integer_overflow_without_changing_stored_usage() {
    let e = env().await;
    let mut run = draft_run(&e).await;
    let initial = run.usage;
    run.usage.input_tokens = u64::MAX;
    assert!(e.runs.save(&run, ImportRunStatus::Proposed).await.is_err());
    assert_eq!(
        e.runs.find(e.org, run.id).await.unwrap().unwrap().usage,
        initial
    );
    run.id = domain::ai_import::ImportRunId::new();
    assert!(
        e.runs
            .reserve_run(&run, Some(pending_photo()), 100)
            .await
            .is_err()
    );
    assert!(e.runs.find(e.org, run.id).await.unwrap().is_none());
    assert_eq!(photo_count(&e).await, 0);
}

#[tokio::test]
async fn telemetry_database_failure_does_not_rollback_successful_draft_or_repeat_model() {
    let e = env().await;
    let run = draft_run(&e).await;
    sqlx::query("CREATE TRIGGER reject_metrics BEFORE UPDATE ON ai_import_turn_usage BEGIN SELECT RAISE(ABORT, 'test metrics write failure'); END")
        .execute(&e.db.write).await.unwrap();
    let agent = Metered::new(Behavior::Success);
    let svc = e.service(agent.clone(), 100);
    let state = svc.get_draft(e.org, run.id).await.unwrap();
    let out = svc
        .turn(e.org, run.id, turn(&state, "db-failure"))
        .await
        .unwrap();
    assert_eq!(out.vendors[0].menu[0].price, Some(775));
    assert_eq!(svc.get_draft(e.org, run.id).await.unwrap(), out);
    assert_eq!(
        svc.turn(e.org, run.id, turn(&state, "db-failure"))
            .await
            .unwrap(),
        out
    );
    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    let row = metrics(&e, &run, "db-failure").await;
    assert_eq!(row.finished_at, None);
    assert_eq!(row.input_tokens, None);
    assert_eq!(
        e.runs
            .count_since(e.org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn overflow_is_rejected_without_failing_draft_and_finalization_is_scoped_and_idempotent() {
    let e = env().await;
    let run = draft_run(&e).await;
    let svc = e.service(Metered::new(Behavior::Overflow), 100);
    let state = svc.get_draft(e.org, run.id).await.unwrap();
    let out = svc
        .turn(e.org, run.id, turn(&state, "overflow"))
        .await
        .unwrap();
    assert_eq!(out.vendors[0].menu[0].price, Some(775));
    assert_eq!(metrics(&e, &run, "overflow").await.finished_at, None);
    let mut m = TurnMetrics {
        finished_at: SystemTime::now(),
        usage: ObservedUsage {
            input_tokens: Some(i64::MAX as u64 + 1),
            output_tokens: Some(0),
        },
        model: None,
        duration: Duration::ZERO,
        execution: TurnExecution::Completed,
        draft: TurnDraftOutcome::Applied,
        error_code: None,
    };
    assert!(e.runs.finish_turn(&run, "overflow", &m).await.is_err());
    m.usage.input_tokens = Some(0);
    m.usage.output_tokens = Some(u64::MAX);
    assert!(e.runs.finish_turn(&run, "overflow", &m).await.is_err());
    m.usage.output_tokens = Some(0);
    m.duration = Duration::from_secs(u64::MAX);
    assert!(e.runs.finish_turn(&run, "overflow", &m).await.is_err());
    m.duration = Duration::ZERO;
    let mut wrong_owner = run.clone();
    wrong_owner.organizer_id = e.other;
    assert!(
        e.runs
            .finish_turn(&wrong_owner, "overflow", &m)
            .await
            .is_err()
    );
    assert!(e.runs.finish_turn(&run, "missing", &m).await.is_err());
    assert_eq!(metrics(&e, &run, "overflow").await.finished_at, None);
    m.usage.input_tokens = Some(i64::MAX as u64);
    e.runs.finish_turn(&run, "overflow", &m).await.unwrap();
    let finalized = metrics(&e, &run, "overflow").await;
    assert_eq!(finalized.input_tokens, Some(i64::MAX));
    m.usage.input_tokens = Some(1);
    e.runs.finish_turn(&run, "overflow", &m).await.unwrap();
    assert_eq!(metrics(&e, &run, "overflow").await, finalized);
    assert_eq!(
        e.runs
            .count_since(e.org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn v10_ledger_migrates_with_unknown_metrics_without_changing_quota() {
    let dir = TempDir::new().unwrap();
    let db = Db::connect_path(&dir.path().join("legacy.db"))
        .await
        .unwrap();
    crate::sqlite::MIGRATOR.run_to(10, &db.write).await.unwrap();
    let org = organizer(&db, "legacy").await;
    let repo = SqliteImportRunRepo::new(db.clone());
    let run = ImportRun::start(
        org,
        ImportMode::Vendor,
        None,
        None,
        None,
        "initial".into(),
        SystemTime::now(),
    );
    repo.reserve_run(&run, None, 10).await.unwrap();
    sqlx::query(
        "INSERT INTO ai_import_turn_usage(run_id,turn_id,organizer_id,created_at) VALUES(?,?,?,?)",
    )
    .bind(run.id.to_string())
    .bind("legacy")
    .bind(org.to_string())
    .bind(crate::timefmt::to_text(run.created_at))
    .execute(&db.write)
    .await
    .unwrap();
    db.migrate().await.unwrap();
    db.migrate().await.unwrap();
    let row: MetricsRow = sqlx::query_as("SELECT finished_at,input_tokens,output_tokens,model,duration_ms,execution_outcome,draft_outcome,error_code FROM ai_import_turn_usage")
        .fetch_one(&db.read).await.unwrap();
    assert_eq!(
        row,
        MetricsRow {
            finished_at: None,
            input_tokens: None,
            output_tokens: None,
            model: None,
            duration_ms: None,
            execution_outcome: None,
            draft_outcome: None,
            error_code: None
        }
    );
    assert_eq!(
        repo.count_since(org, SystemTime::UNIX_EPOCH).await.unwrap(),
        2
    );
    assert_eq!(
        repo.find(org, run.id).await.unwrap().unwrap().usage,
        run.usage
    );
}
