use super::*;
use application::ports::{Clock, ImportRunRepo, NormalizedVendorImage, PortResult, RepoResult};
use domain::ai_import::{DraftState, ImportRun, ImportRunId};
use tokio::sync::Barrier;

struct FixedClock(SystemTime);
impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

// Synchronize only reservations, not reads, finalization, or model calls.
struct GatedRepo {
    inner: SqliteImportRunRepo,
    barrier: Arc<Barrier>,
}

#[async_trait]
impl ImportRunRepo for GatedRepo {
    async fn finish_turn(
        &self,
        run: &ImportRun,
        turn_id: &str,
        metrics: &application::ai_import::TurnMetrics,
    ) -> PortResult<()> {
        self.inner.finish_turn(run, turn_id, metrics).await
    }
    async fn reserve_run(
        &self,
        run: &ImportRun,
        photo: Option<NormalizedVendorImage>,
        quota: u32,
    ) -> RepoResult<()> {
        self.barrier.wait().await;
        self.inner.reserve_run(run, photo, quota).await
    }

    async fn save_draft(
        &self,
        run: &ImportRun,
        state: &DraftState,
        expected: i64,
        now: SystemTime,
        quota: Option<u32>,
    ) -> RepoResult<()> {
        if quota.is_some() {
            self.barrier.wait().await;
        }
        self.inner
            .save_draft(run, state, expected, now, quota)
            .await
    }

    async fn draft(&self, run: &ImportRun) -> RepoResult<DraftState> {
        self.inner.draft(run).await
    }
    async fn draft_image(&self, run: &ImportRun, now: SystemTime) -> PortResult<Option<Vec<u8>>> {
        self.inner.draft_image(run, now).await
    }
    async fn find(&self, org: OrganizerId, id: ImportRunId) -> PortResult<Option<ImportRun>> {
        self.inner.find(org, id).await
    }
    async fn save(&self, run: &ImportRun, expected: ImportRunStatus) -> RepoResult<()> {
        self.inner.save(run, expected).await
    }
    async fn count_since(&self, org: OrganizerId, since: SystemTime) -> PortResult<u64> {
        self.inner.count_since(org, since).await
    }
}

fn photo_cmd() -> StartImportCmd {
    StartImportCmd {
        mode: ImportMode::Vendor,
        pending_photo: Some(pending_photo()),
        ..cmd()
    }
}

fn counting_agent() -> Arc<Flaky> {
    Arc::new(Flaky {
        calls: AtomicUsize::new(0),
        failures: vec![],
    })
}

// Separate pools ensure SQLite's transaction, not just the process's one-writer
// pool, serializes quota reservations.
async fn racing_services(e: &Env, quota: u32, agent: Arc<Flaky>) -> [AiImportService; 2] {
    let other_db = Db::connect_path(&e._dir.path().join("test.db"))
        .await
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut first = e.service(agent.clone(), quota);
    let mut second = e.service(agent, quota);
    for (svc, db) in [(&mut first, e.db.clone()), (&mut second, other_db)] {
        svc.config.timeout = Duration::from_secs(5);
        svc.runs = Arc::new(GatedRepo {
            inner: SqliteImportRunRepo::new(db),
            barrier: barrier.clone(),
        });
    }
    [first, second]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_initial_reservations_share_last_slot_and_only_call_model_once() {
    let e = env().await;
    let agent = counting_agent();
    let [first, second] = racing_services(&e, 1, agent.clone()).await;
    let org = e.org;
    let a = tokio::spawn(async move { first.start(org, photo_cmd()).await });
    let b = tokio::spawn(async move { second.start(org, photo_cmd()).await });
    let (a, b) = tokio::time::timeout(Duration::from_secs(10), async {
        (a.await.unwrap(), b.await.unwrap())
    })
    .await
    .unwrap();
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(
        a.err().or(b.err()),
        Some(AiImportError::QuotaExceeded)
    ));
    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        e.runs
            .count_since(org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        1
    );
    assert_eq!(photo_count(&e).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_initial_and_turn_share_last_slot_and_only_call_model_once() {
    let e = env().await;
    let run = draft_tests::draft_run(&e).await;
    let state = e.runs.draft(&run).await.unwrap();
    let request = application::ai_import::DraftTurnCmd {
        revision: state.revision,
        turn_id: "last-slot".into(),
        instruction: "請說明".into(),
        vendors: state.vendors,
    };
    let agent = counting_agent();
    let [first, second] = racing_services(&e, 2, agent.clone()).await;
    let org = e.org;
    let a = tokio::spawn(async move { first.start(org, photo_cmd()).await.map(|_| ()) });
    let b = tokio::spawn(async move { second.turn(org, run.id, request).await.map(|_| ()) });
    let (a, b) = tokio::time::timeout(Duration::from_secs(10), async {
        (a.await.unwrap(), b.await.unwrap())
    })
    .await
    .unwrap();
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(
        a.err().or(b.err()),
        Some(AiImportError::QuotaExceeded)
    ));
    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        e.runs
            .count_since(org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        2
    );
    let current = e.runs.draft(&run).await.unwrap();
    assert!(current.pending.is_none());
    let (runs, turns): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM ai_import_runs), (SELECT count(*) FROM ai_import_turn_usage)",
    )
    .fetch_one(&e.db.read)
    .await
    .unwrap();
    assert_eq!(runs + turns, 2);
    assert_eq!(photo_count(&e).await, runs - 1);
    assert_eq!(current.turns.len() as i64, turns);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_different_organizers_have_independent_quota() {
    let e = env().await;
    let agent = counting_agent();
    let [first, second] = racing_services(&e, 1, agent.clone()).await;
    let org = e.org;
    let other = e.other;
    let a = tokio::spawn(async move { first.start(org, cmd()).await });
    let b = tokio::spawn(async move { second.start(other, cmd()).await });
    tokio::time::timeout(Duration::from_secs(10), async {
        a.await.unwrap().unwrap();
        b.await.unwrap().unwrap();
    })
    .await
    .unwrap();
    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    for org in [org, other] {
        assert_eq!(
            e.runs
                .count_since(org, SystemTime::UNIX_EPOCH)
                .await
                .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn quota_rejection_and_photo_insert_failure_leave_no_run_or_photo() {
    let e = env().await;
    let agent = counting_agent();
    let svc = e.service(agent.clone(), 0);
    assert!(matches!(
        svc.start(e.org, photo_cmd()).await,
        Err(AiImportError::QuotaExceeded)
    ));
    assert!(e.run_status(e.org).await.is_empty());
    assert_eq!(photo_count(&e).await, 0);
    assert_eq!(agent.calls.load(Ordering::SeqCst), 0);

    // Fail after the photo INSERT so both writes must be rolled back.
    sqlx::query("CREATE TRIGGER reject_photo AFTER INSERT ON ai_import_photos BEGIN SELECT RAISE(ABORT, 'test photo failure'); END")
        .execute(&e.db.write).await.unwrap();
    let svc = e.service(agent.clone(), 1);
    assert!(matches!(
        svc.start(e.org, photo_cmd()).await,
        Err(AiImportError::Internal(_))
    ));
    assert!(e.run_status(e.org).await.is_empty());
    assert_eq!(photo_count(&e).await, 0);
    assert_eq!(agent.calls.load(Ordering::SeqCst), 0);
    sqlx::query("DROP TRIGGER reject_photo")
        .execute(&e.db.write)
        .await
        .unwrap();
    svc.start(e.org, photo_cmd()).await.unwrap();
    assert_eq!(photo_count(&e).await, 1);
    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rolling_quota_includes_exact_24h_boundary_for_runs_and_turns() {
    for is_turn in [false, true] {
        let e = env().await;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let since = now - Duration::from_secs(86400);
        let mut svc = e.service(Arc::new(FakeImportAgent::empty()), 1);
        svc.clock = Arc::new(FixedClock(since));
        let run = svc.start(e.org, cmd()).await.unwrap();
        if is_turn {
            sqlx::query("UPDATE ai_import_runs SET created_at=? WHERE id=?")
                .bind(crate::timefmt::to_text(since - Duration::from_secs(1)))
                .bind(run.id.to_string())
                .execute(&e.db.write)
                .await
                .unwrap();
            sqlx::query("INSERT INTO ai_import_turn_usage(run_id,turn_id,organizer_id,created_at) VALUES(?,?,?,?)")
                .bind(run.id.to_string()).bind("boundary").bind(e.org.to_string())
                .bind(crate::timefmt::to_text(since)).execute(&e.db.write).await.unwrap();
        }
        svc.clock = Arc::new(FixedClock(now));
        assert!(matches!(
            svc.start(e.org, cmd()).await,
            Err(AiImportError::QuotaExceeded)
        ));
        svc.clock = Arc::new(FixedClock(now + Duration::from_secs(1)));
        svc.start(e.org, cmd()).await.unwrap();
    }
}

#[tokio::test]
async fn retry_and_model_failure_each_consume_one_reservation() {
    for (failures, succeeds, attempts) in [
        (vec![unavailable(true)], true, 2),
        (vec![unavailable(true); 2], false, 2),
        (vec![unavailable(false)], false, 1),
    ] {
        let e = env().await;
        let agent = Arc::new(Flaky {
            calls: AtomicUsize::new(0),
            failures,
        });
        let svc = e.service(agent.clone(), 1);
        let result = svc.start(e.org, photo_cmd()).await;
        if succeeds {
            assert_eq!(result.unwrap().status, ImportRunStatus::Proposed);
        } else {
            assert!(matches!(result, Err(AiImportError::Agent { .. })));
        }
        assert_eq!(agent.calls.load(Ordering::SeqCst), attempts);
        assert_eq!(photo_count(&e).await, i64::from(succeeds));
        assert_eq!(
            e.runs
                .count_since(e.org, SystemTime::UNIX_EPOCH)
                .await
                .unwrap(),
            1
        );
        assert!(matches!(
            svc.start(e.org, cmd()).await,
            Err(AiImportError::QuotaExceeded)
        ));
        assert_eq!(agent.calls.load(Ordering::SeqCst), attempts);
    }
}
