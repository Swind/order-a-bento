use super::*;
use application::ai_import::DraftTurnCmd;
use application::ports::ImportRunRepo;
use domain::ai_import::{DraftEdit, DraftState, ImportRun};

async fn draft_run(e: &Env) -> ImportRun {
    let a = FakeImportAgent::new(
        vec![FakeToolCall::ProposeVendor(vargs(json!({
            "target":"new", "info":{"name":"原店名"},
            "menuItems":[{"name":"雞腿飯","price":90},{"name":"排骨飯","price":80}], "warnings":[]
        })))],
        "辨識完成",
    );
    e.service(Arc::new(a), 100)
        .start(
            e.org,
            StartImportCmd {
                mode: ImportMode::Vendor,
                ..cmd()
            },
        )
        .await
        .unwrap()
}
fn edit_agent(price: i64) -> Arc<dyn ImportAgent> {
    Arc::new(FakeImportAgent::new(
        vec![FakeToolCall::SubmitDraftEdits(vec![
            DraftEdit::UpdateItem {
                item_id: "v0-m0".into(),
                name: None,
                price: Some(price),
                category: None,
                description: None,
            },
        ])],
        "價格已調整",
    ))
}
fn turn(s: &DraftState, id: &str) -> DraftTurnCmd {
    DraftTurnCmd {
        revision: s.revision,
        turn_id: id.into(),
        instruction: "雞腿飯價格調整".into(),
        vendors: s.vendors.clone(),
    }
}

#[tokio::test]
async fn identical_save_and_question_keep_the_last_ai_edit_undoable() {
    let e = env().await;
    let run = draft_run(&e).await;
    let svc = e.service(edit_agent(750), 100);
    let initial = svc.get_draft(e.org, run.id).await.unwrap();
    let edited = svc
        .turn(e.org, run.id, turn(&initial, "edit"))
        .await
        .unwrap();
    let saved = svc
        .save_draft(e.org, run.id, edited.revision, edited.vendors.clone())
        .await
        .unwrap();
    assert_eq!(saved.undo, edited.undo);
    let question = e.service(Arc::new(FakeImportAgent::new(vec![], "請問哪一項？")), 100);
    let asked = question
        .turn(e.org, run.id, turn(&saved, "question"))
        .await
        .unwrap();
    assert_eq!(asked.undo, edited.undo);
    assert!(asked.turns.last().unwrap().changes.is_empty());
    let restored = svc
        .undo_turn(e.org, run.id, asked.revision, "edit".into())
        .await
        .unwrap();
    assert_eq!(restored.vendors, initial.vendors);
}

#[tokio::test]
async fn saving_real_manual_changes_invalidates_old_ai_undo_without_losing_history() {
    let e = env().await;
    let run = draft_run(&e).await;
    let svc = e.service(edit_agent(750), 100);
    let initial = svc.get_draft(e.org, run.id).await.unwrap();
    let edited = svc
        .turn(e.org, run.id, turn(&initial, "edit"))
        .await
        .unwrap();
    let mut manual = edited.vendors.clone();
    manual[0].menu[0].price = Some(775);
    manual[0].menu[1].name = "已確認的手改".into();
    let saved = svc
        .save_draft(e.org, run.id, edited.revision, manual.clone())
        .await
        .unwrap();
    assert!(saved.undo.is_none());
    assert_eq!(saved.turns, edited.turns);
    assert!(matches!(
        svc.undo_turn(e.org, run.id, saved.revision, "edit".into())
            .await,
        Err(AiImportError::Conflict)
    ));
    assert_eq!(svc.get_draft(e.org, run.id).await.unwrap().vendors, manual);
}

#[tokio::test]
async fn turn_syncing_manual_changes_invalidates_old_undo_even_when_model_does_not_edit() {
    for fails in [false, true] {
        let e = env().await;
        let run = draft_run(&e).await;
        let svc = e.service(edit_agent(750), 100);
        let initial = svc.get_draft(e.org, run.id).await.unwrap();
        let edited = svc
            .turn(e.org, run.id, turn(&initial, "edit"))
            .await
            .unwrap();
        let mut cmd = turn(&edited, "next");
        cmd.vendors[0].menu[1].name = "新回合帶入的手改".into();
        let manual = cmd.vendors.clone();
        let script = if fails {
            vec![FakeToolCall::SubmitDraftEdits(vec![
                DraftEdit::DeleteItem {
                    item_id: "missing".into(),
                },
            ])]
        } else {
            vec![]
        };
        let next = e
            .service(Arc::new(FakeImportAgent::new(script, "請說明")), 100)
            .turn(e.org, run.id, cmd)
            .await
            .unwrap();
        assert!(next.undo.is_none());
        assert_eq!(next.vendors, manual);
        assert_eq!(
            next.turns.last().unwrap().status,
            if fails { "failed" } else { "completed" }
        );
        assert!(matches!(
            svc.undo_turn(e.org, run.id, next.revision, "edit".into())
                .await,
            Err(AiImportError::Conflict)
        ));
        assert_eq!(svc.get_draft(e.org, run.id).await.unwrap().vendors, manual);
    }
}

#[tokio::test]
async fn new_ai_edit_after_manual_sync_undo_restores_the_latest_manual_baseline() {
    let e = env().await;
    let run = draft_run(&e).await;
    let svc = e.service(edit_agent(750), 100);
    let initial = svc.get_draft(e.org, run.id).await.unwrap();
    let edited = svc
        .turn(e.org, run.id, turn(&initial, "edit"))
        .await
        .unwrap();
    let mut cmd = turn(&edited, "next");
    cmd.vendors[0].menu[1].name = "最新手改保留".into();
    let manual = cmd.vendors.clone();
    let next = e
        .service(edit_agent(775), 100)
        .turn(e.org, run.id, cmd)
        .await
        .unwrap();
    assert_eq!(next.undo.as_ref(), Some(&manual));
    assert!(matches!(
        svc.undo_turn(e.org, run.id, next.revision, "edit".into())
            .await,
        Err(AiImportError::Conflict)
    ));
    let restored = svc
        .undo_turn(e.org, run.id, next.revision, "next".into())
        .await
        .unwrap();
    assert_eq!(restored.vendors, manual);
}

#[tokio::test]
async fn manual_latest_draft_edit_replay_undo_and_confirm() {
    let e = env().await;
    let run = draft_run(&e).await;
    let svc = e.service(edit_agent(110), 100);
    let mut s = svc.get_draft(e.org, run.id).await.unwrap();
    s.vendors[0].info.name = "手動店名".into();
    s.vendors[0].menu[1].name = "手動排骨".into();
    s.vendors[0].menu[1].price = Some(87);
    s.vendors[0].menu[1].category = Some("保留分類".into());
    let before = s.vendors.clone();
    let request = turn(&s, "one");
    let s = svc.turn(e.org, run.id, request).await.unwrap();
    assert_eq!(s.vendors[0].menu[0].price, Some(110));
    assert_eq!(s.vendors[0].menu[1], before[0].menu[1]);
    assert_eq!(s.vendors[0].info.name, "手動店名");
    assert_eq!(e.vendors.list(e.org).await.unwrap().len(), 0);
    let replay = svc
        .turn(
            e.org,
            run.id,
            DraftTurnCmd {
                revision: 0,
                turn_id: "one".into(),
                instruction: "雞腿飯價格調整".into(),
                vendors: before.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(replay, s);
    assert_eq!(
        e.runs
            .count_since(e.org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        2
    );
    let undone = svc
        .undo_turn(e.org, run.id, s.revision, "one".into())
        .await
        .unwrap();
    assert_eq!(undone.vendors, before);
    assert!(undone.turns[0].undone);
    assert_eq!(
        svc.undo_turn(e.org, run.id, 0, "one".into()).await.unwrap(),
        undone
    );
    let s = svc.turn(e.org, run.id, turn(&undone, "two")).await.unwrap();
    assert_eq!(s.undo.as_ref().unwrap(), &before);
    let result = svc
        .apply_at_revision(
            e.org,
            run.id,
            ApplyCmd {
                save_photo: Some(false),
                accepted: vec![],
            },
            Some(s.revision),
        )
        .await
        .unwrap();
    let vendor = e
        .vendors
        .find(e.org, result.vendor_ids[0])
        .await
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(vendor.name().to_string(), "手動店名");
    assert_eq!(vendor.menu().len(), 2);
    assert!(svc.turn(e.org, run.id, turn(&s, "three")).await.is_err());
    assert!(
        svc.undo_turn(e.org, run.id, s.revision, "two".into())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn invalid_batch_timeout_and_question_never_lose_manual_draft() {
    let e = env().await;
    let run = draft_run(&e).await;
    let base = e.service(edit_agent(110), 100);
    let mut s = base.get_draft(e.org, run.id).await.unwrap();
    s.vendors[0].menu[1].name = "手改".into();
    let bad = Arc::new(FakeImportAgent::new(
        vec![FakeToolCall::SubmitDraftEdits(vec![
            DraftEdit::DeleteItem {
                item_id: "v0-m0".into(),
            },
            DraftEdit::DeleteItem {
                item_id: "missing".into(),
            },
        ])],
        "不應套用",
    ));
    let s = e
        .service(bad, 100)
        .turn(e.org, run.id, turn(&s, "bad"))
        .await
        .unwrap();
    assert_eq!(s.vendors[0].menu.len(), 2);
    assert_eq!(s.vendors[0].menu[1].name, "手改");
    assert_eq!(s.turns[0].status, "failed");
    let question = e.service(
        Arc::new(FakeImportAgent::new(vec![], "請問要改哪一項？")),
        100,
    );
    let q = question
        .turn(e.org, run.id, turn(&s, "question"))
        .await
        .unwrap();
    assert_eq!(q.vendors, s.vendors);
    assert!(q.turns[1].changes.is_empty());
    let mut slow = e.service(
        Arc::new(SlowDraft {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        }),
        100,
    );
    slow.config.timeout = Duration::from_millis(5);
    let timed = slow.turn(e.org, run.id, turn(&q, "timeout")).await.unwrap();
    assert_eq!(timed.vendors, s.vendors);
    assert_eq!(timed.turns[2].status, "failed");
    assert!(timed.pending.is_none());
}

struct SlowDraft {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait]
impl ImportAgent for SlowDraft {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        assert!(input.instruction.unwrap().contains("CURRENT DRAFT"));
        self.entered.notify_one();
        self.release.notified().await;
        ctx.submit_edits(vec![DraftEdit::DeleteItem {
            item_id: "v0-m0".into(),
        }])
        .await
        .unwrap();
        Ok(AgentOutcome {
            summary: "刪除".into(),
            usage: Usage::default(),
            model: "slow".into(),
        })
    }
}

#[tokio::test]
async fn late_response_cannot_overwrite_manual_save_or_race_another_turn() {
    let e = env().await;
    let run = draft_run(&e).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let svc = e.service(
        Arc::new(SlowDraft {
            entered: entered.clone(),
            release: release.clone(),
        }),
        100,
    );
    let s = svc.get_draft(e.org, run.id).await.unwrap();
    let work = svc.turn(e.org, run.id, turn(&s, "slow"));
    let concurrent = async {
        entered.notified().await;
        assert!(matches!(
            svc.turn(e.org, run.id, turn(&s, "race")).await,
            Err(AiImportError::Conflict)
        ));
        assert_eq!(
            svc.turn(e.org, run.id, turn(&s, "slow"))
                .await
                .unwrap()
                .pending
                .as_deref(),
            Some("slow")
        );
        let mut fresh = svc.get_draft(e.org, run.id).await.unwrap();
        fresh.vendors[0].menu[0].name = "較新的手改".into();
        let saved = svc
            .save_draft(e.org, run.id, fresh.revision, fresh.vendors)
            .await
            .unwrap();
        assert!(saved.pending.is_none());
        assert!(
            svc.apply_at_revision(e.org, run.id, ApplyCmd::default(), Some(s.revision))
                .await
                .is_err()
        );
        release.notify_one();
    };
    let (out, ()) = tokio::join!(work, concurrent);
    assert!(matches!(out, Err(AiImportError::Conflict)));
    let s = svc.get_draft(e.org, run.id).await.unwrap();
    assert_eq!(s.vendors[0].menu[0].name, "較新的手改");
    assert_eq!(s.vendors[0].menu.len(), 2);
    assert_eq!(
        e.runs
            .count_since(e.org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn owner_quota_expiry_validation_and_state_guards() {
    let e = env().await;
    let run = draft_run(&e).await;
    let svc = e.service(edit_agent(110), 2);
    let s = svc.get_draft(e.org, run.id).await.unwrap();
    assert!(matches!(
        svc.get_draft(e.other, run.id).await,
        Err(AiImportError::NotFound)
    ));
    assert!(
        svc.turn(e.other, run.id, turn(&s, "foreign"))
            .await
            .is_err()
    );
    assert!(
        svc.undo_turn(e.other, run.id, 0, "one".into())
            .await
            .is_err()
    );
    let mut bad = s.vendors.clone();
    bad[0].vendor = TargetRef::Existing {
        existing_id: VendorId::new(),
    };
    assert!(svc.save_draft(e.org, run.id, 0, bad).await.is_err());
    let mut long = turn(&s, "long");
    long.instruction = "字".repeat(501);
    assert!(svc.turn(e.org, run.id, long).await.is_err());
    let s = svc.turn(e.org, run.id, turn(&s, "one")).await.unwrap();
    assert!(matches!(
        svc.turn(e.org, run.id, turn(&s, "over")).await,
        Err(AiImportError::QuotaExceeded)
    ));
    assert_eq!(svc.get_draft(e.org, run.id).await.unwrap(), s);
    let mut start = cmd();
    start.mode = ImportMode::Vendor;
    assert!(matches!(
        svc.start(e.org, start).await,
        Err(AiImportError::QuotaExceeded)
    ));
    sqlx::query("UPDATE ai_import_runs SET created_at='2000-01-01T00:00:00.000000000Z' WHERE id=?")
        .bind(run.id.to_string())
        .execute(&e.db.write)
        .await
        .unwrap();
    assert!(svc.turn(e.org, run.id, turn(&s, "expired")).await.is_err());
    assert!(
        svc.undo_turn(e.org, run.id, s.revision, "one".into())
            .await
            .is_err()
    );
    assert!(
        svc.save_draft(e.org, run.id, s.revision, s.vendors)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn pending_lease_recovery_and_revoked_sharing_admin_are_guarded() {
    use application::sharing::SharingRepo;
    let e = env().await;
    let run = draft_run(&e).await;
    let svc = e.service(edit_agent(110), 100);
    let mut s = svc.get_draft(e.org, run.id).await.unwrap();
    s.pending = Some("abandoned".into());
    s.pending_until = Some(1);
    s.turns.push(domain::ai_import::DraftTurn {
        id: "abandoned".into(),
        instruction: "改價".into(),
        reply: String::new(),
        status: "pending".into(),
        changes: vec![],
        undone: false,
    });
    s.revision += 1;
    e.runs
        .save_draft(&run, &s, 0, SystemTime::now(), None)
        .await
        .unwrap();
    let recovered = svc.get_draft(e.org, run.id).await.unwrap();
    assert!(recovered.pending.is_none());
    assert_eq!(recovered.vendors, s.vendors);
    assert_eq!(recovered.turns[0].status, "failed");
    let groups = crate::sharing_repo::SqliteSharingRepo::new(e.db.clone());
    let g = groups.create(e.other, "team").await.unwrap();
    groups.add(e.other, g.id, e.org, "admin").await.unwrap();
    let scoped = e
        .service(Arc::new(FakeImportAgent::empty()), 100)
        .start(
            e.org,
            StartImportCmd {
                mode: ImportMode::Vendor,
                sharing_group_id: Some(g.id),
                ..cmd()
            },
        )
        .await
        .unwrap();
    let shared_draft = svc.get_draft(e.org, scoped.id).await.unwrap();
    groups
        .change(e.other, g.id, e.org, Some("member"))
        .await
        .unwrap();
    assert!(
        svc.turn(e.org, scoped.id, turn(&shared_draft, "revoked"))
            .await
            .is_err()
    );
    assert!(
        svc.undo_turn(e.org, scoped.id, shared_draft.revision, "abandoned".into())
            .await
            .is_err()
    );
    let mut next = shared_draft.clone();
    next.revision += 1;
    assert!(
        e.runs
            .save_draft(
                &scoped,
                &next,
                shared_draft.revision,
                SystemTime::now(),
                None
            )
            .await
            .is_err()
    );
}

struct RetryDraft(AtomicUsize);
#[async_trait]
impl ImportAgent for RetryDraft {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        assert!(input.instruction.unwrap().contains("手動店名"));
        let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
        ctx.submit_edits(vec![DraftEdit::UpdateItem {
            item_id: "v0-m0".into(),
            name: None,
            price: Some(if first { 999 } else { 110 }),
            category: None,
            description: None,
        }])
        .await
        .unwrap();
        if first {
            Err(AiError::Unavailable {
                retryable: true,
                detail: "mock retry".into(),
            })
        } else {
            Ok(AgentOutcome {
                summary: "完成".into(),
                usage: Usage::default(),
                model: "retry".into(),
            })
        }
    }
}
#[tokio::test]
async fn retry_resets_only_candidate_and_reference_image_respects_opt_in() {
    let e = env().await;
    let run = draft_run(&e).await;
    let agent = Arc::new(RetryDraft(AtomicUsize::new(0)));
    let svc = e.service(agent.clone(), 100);
    let mut s = svc.get_draft(e.org, run.id).await.unwrap();
    s.vendors[0].info.name = "手動店名".into();
    let out = svc.turn(e.org, run.id, turn(&s, "retry")).await.unwrap();
    assert_eq!(agent.0.load(Ordering::SeqCst), 2);
    assert_eq!(out.vendors[0].menu[0].price, Some(110));
    assert_eq!(out.undo.unwrap(), s.vendors);
    assert_eq!(
        e.runs
            .count_since(e.org, SystemTime::UNIX_EPOCH)
            .await
            .unwrap(),
        2
    );
    assert!(
        e.runs
            .draft_image(&run, SystemTime::now())
            .await
            .unwrap()
            .is_none()
    );
    let photo = photo_run(&e).await;
    let bytes = e
        .runs
        .draft_image(&photo, SystemTime::now())
        .await
        .unwrap()
        .unwrap();
    assert!(bytes.starts_with(&[0xff, 0xd8]));
    assert!(
        e.runs
            .draft_image(&photo, SystemTime::now() + Duration::from_secs(86401))
            .await
            .unwrap()
            .is_none()
    );
}
