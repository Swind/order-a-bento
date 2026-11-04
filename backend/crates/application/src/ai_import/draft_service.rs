use super::*;
use domain::ai_import::{
    DraftState, DraftTurn, ImportMode, ImportRunId, ImportRunStatus, VendorDraft, validate_draft,
};
use domain::identity::OrganizerId;
use domain::shared::DomainError;
use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

pub struct DraftTurnCmd {
    pub revision: i64,
    pub turn_id: String,
    pub instruction: String,
    pub vendors: Vec<VendorDraft>,
}

impl AiImportService {
    async fn editable_run(
        &self,
        org: OrganizerId,
        id: ImportRunId,
    ) -> Result<ImportRunView, AiImportError> {
        let run = self.get(org, id).await?;
        self.rosters
            .check_scope(org, run.sharing_group_id, true)
            .await?;
        if run.status != ImportRunStatus::Proposed
            || run.mode != Some(ImportMode::Vendor)
            || self.clock.now() >= run.created_at + Duration::from_secs(86400)
        {
            return Err(AiImportError::InvalidState("草稿已到期或不可編輯".into()));
        }
        Ok(run)
    }

    pub async fn get_draft(
        &self,
        org: OrganizerId,
        id: ImportRunId,
    ) -> Result<DraftState, AiImportError> {
        let run = self.editable_run(org, id).await?;
        let mut state = self.runs.draft(&run).await?;
        let now = self
            .clock
            .now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if state.pending_until.is_some_and(|t| t <= now) {
            let expected = state.revision;
            cancel_pending(&mut state, "本輪等待逾時；草稿已保留");
            state.revision += 1;
            self.runs
                .save_draft(&run, &state, expected, self.clock.now(), None)
                .await?;
        }
        Ok(state)
    }

    pub async fn save_draft(
        &self,
        org: OrganizerId,
        id: ImportRunId,
        revision: i64,
        vendors: Vec<VendorDraft>,
    ) -> Result<DraftState, AiImportError> {
        let run = self.editable_run(org, id).await?;
        let mut state = self.get_draft(org, id).await?;
        if state.revision != revision {
            return Err(AiImportError::Conflict);
        }
        validate_draft(&vendors, &state.vendors)?;
        cancel_pending(&mut state, "本輪已因手動編輯取消");
        if vendors != state.vendors {
            state.undo = None;
        }
        state.vendors = vendors;
        state.revision += 1;
        self.runs
            .save_draft(&run, &state, revision, self.clock.now(), None)
            .await?;
        Ok(state)
    }

    #[tracing::instrument(name = "ai_draft_turn", skip_all, fields(run = %id, turn = tracing::field::Empty))]
    pub async fn turn(
        &self,
        org: OrganizerId,
        id: ImportRunId,
        cmd: DraftTurnCmd,
    ) -> Result<DraftState, AiImportError> {
        let request_started = std::time::Instant::now();
        let DraftTurnCmd {
            revision,
            turn_id,
            instruction,
            vendors,
        } = cmd;
        let run = self.editable_run(org, id).await?;
        let mut state = self.get_draft(org, id).await?;
        if let Some(turn) = state.turns.iter().find(|t| t.id == turn_id) {
            if turn.instruction != instruction.trim() {
                return Err(AiImportError::Conflict);
            }
            tracing::Span::current().record("turn", turn.id.as_str());
            // Replay does not imply another model execution or exactly-once provider billing.
            tracing::info!(
                event = "ai_turn_replay",
                quota_reserved = false,
                quota_unit = "internal_operation_24h",
                usage_kind = "not_reobserved",
                model = None::<&str>,
                input_tokens = None::<u64>,
                output_tokens = None::<u64>,
                duration_ms = request_started.elapsed().as_millis() as u64,
                execution = "not_started",
                outcome = "replay",
                error_code = None::<&str>,
            );
            return Ok(state); // A lost response never repeats model work or quota.
        }
        if state.revision != revision || state.pending.is_some() {
            return Err(AiImportError::Conflict);
        }
        let instruction =
            domain::shared::text::optional_text("instruction", Some(&instruction), 500)?
                .ok_or_else(|| DomainError::invalid("instruction", "請輸入指示"))?;
        if turn_id.is_empty() || turn_id.len() > 80 || state.turns.len() >= 20 {
            return Err(
                DomainError::invalid("turn", "每份草稿最多 20 輪；識別碼最多 80 字元").into(),
            );
        }
        validate_draft(&vendors, &state.vendors)?;
        tracing::Span::current().record("turn", turn_id.as_str());
        if vendors != state.vendors {
            state.undo = None;
        }
        state.vendors = vendors.clone();
        let history: Vec<_> = state.turns.iter().rev().take(6).rev().map(|t| serde_json::json!({"instruction":t.instruction, "reply":t.reply, "status":t.status, "undone":t.undone})).collect();
        let prompt = format!(
            "CURRENT DRAFT (authoritative, including manual edits and undo):\n{}\nRecent conversation (undone turns are not current state):\n{}\nOrganizer instruction: {}",
            serde_json::to_string(&vendors).map_err(|e| AiImportError::Internal(e.to_string()))?,
            serde_json::to_string(&history).map_err(|e| AiImportError::Internal(e.to_string()))?,
            instruction
        );
        state.pending = Some(turn_id.clone());
        state.pending_until = Some(
            (self.clock.now() + self.config.timeout + Duration::from_secs(30))
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        state.turns.push(DraftTurn {
            id: turn_id.clone(),
            instruction: instruction.clone(),
            reply: String::new(),
            status: "pending".into(),
            changes: vec![],
            undone: false,
        });
        state.revision += 1;
        self.runs
            .save_draft(
                &run,
                &state,
                revision,
                self.clock.now(),
                Some(self.config.daily_quota),
            )
            .await?;
        let reserved = state.revision;
        let started = std::time::Instant::now();
        let mut metrics = TurnMetrics {
            finished_at: self.clock.now(),
            usage: ObservedUsage::default(),
            model: None,
            duration: Duration::ZERO,
            execution: TurnExecution::Failed,
            draft: TurnDraftOutcome::Rejected,
            error_code: None,
        };
        tracing::info!(
            event = "ai_turn_started",
            quota_reserved = true,
            quota_unit = "internal_operation_24h",
            usage_kind = "observed_tokens_not_invoice",
            model = metrics.model.as_deref(),
            input_tokens = metrics.usage.input_tokens,
            output_tokens = metrics.usage.output_tokens,
            duration_ms = 0_u64,
            execution = "pending",
            outcome = "pending",
            error_code = None::<&str>,
        );
        let mut ctx = ImportToolContext::new(
            org,
            id,
            self.rosters.clone(),
            self.vendors.clone(),
            self.clock.clone(),
        );
        ctx.sharing_group_id = run.sharing_group_id;
        *ctx.editing.get_mut() = Some(EditSession {
            baseline: vendors.clone(),
            draft: vendors.clone(),
            changes: vec![],
            submitted: false,
        });
        let ctx = Arc::new(ctx);
        let outcome = async {
            let image = match self.runs.draft_image(&run, self.clock.now()).await {
                Ok(image) => image.unwrap_or_default(),
                Err(error) => {
                    metrics.error_code = Some("image_read");
                    cancel_pending(&mut state, "本輪讀取圖片失敗；草稿已保留");
                    state.revision += 1;
                    if self
                        .runs
                        .save_draft(&run, &state, reserved, self.clock.now(), None)
                        .await
                        .is_err()
                    {
                        tracing::error!(run = %id, turn = turn_id.as_str(), "AI turn image cleanup failed");
                    }
                    return Err(AiImportError::from(error));
                }
            };
            let result = self
                .run_agent(
                    &ctx,
                    AgentInput {
                        image: ImageInput {
                            bytes: image,
                            media_type: "image/jpeg".into(),
                        },
                        instruction: Some(prompt),
                        mode: ImportMode::Vendor,
                        target_roster_id: None,
                        target_vendor_id: None,
                    },
                )
                .await;
            metrics.usage = if ctx.turns() > 0 {
                ctx.observed_usage()
            } else if let Ok(out) = &result {
                ObservedUsage {
                    input_tokens: Some(out.usage.input_tokens),
                    output_tokens: Some(out.usage.output_tokens),
                }
            } else {
                ObservedUsage::default()
            };
            match &result {
                Ok(out) => {
                    metrics.execution = TurnExecution::Completed;
                    metrics.model = if ctx.turns() > 0 {
                        ctx.observed_model()
                    } else {
                        Some(out.model.clone())
                    };
                }
                Err(error) => {
                    metrics.model = ctx.observed_model();
                    metrics.error_code = Some(error.code());
                }
            }
            self.editable_run(org, id).await?;
            let turn = state.turns.last_mut().unwrap();
            match result {
                Ok(out) => {
                    let e = ctx.editing.lock().await;
                    let e = e.as_ref().unwrap();
                    if !e.changes.is_empty() {
                        state.undo = Some(vendors);
                    }
                    state.vendors = e.draft.clone();
                    turn.changes = e.changes.clone();
                    turn.reply = out.summary.chars().take(2000).collect();
                    turn.status = "completed".into();
                }
                Err(e) => {
                    turn.reply = format!("{e}；草稿已保留");
                    turn.status = "failed".into();
                }
            }
            state.pending = None;
            state.pending_until = None;
            state.revision += 1;
            let changed = !turn.changes.is_empty();
            self.runs
                .save_draft(&run, &state, reserved, self.clock.now(), None)
                .await?;
            metrics.draft = if changed {
                TurnDraftOutcome::Applied
            } else {
                TurnDraftOutcome::Unchanged
            };
            Ok(state)
        }
        .await;
        if let Err(error) = &outcome {
            metrics.error_code = Some(match error {
                AiImportError::NotFound => "access_revoked",
                AiImportError::Conflict => "stale",
                AiImportError::InvalidState(_) => "invalid_state",
                _ => metrics.error_code.unwrap_or("internal"),
            });
        }
        metrics.finished_at = self.clock.now();
        metrics.duration = started.elapsed();
        tracing::info!(
            event = "ai_turn_completed",
            quota_reserved = true,
            quota_unit = "internal_operation_24h",
            usage_kind = "observed_tokens_not_invoice",
            model = metrics.model.as_deref(),
            input_tokens = metrics.usage.input_tokens,
            output_tokens = metrics.usage.output_tokens,
            duration_ms = metrics.duration.as_millis() as u64,
            execution = metrics.execution.as_str(),
            outcome = metrics.draft.as_str(),
            error_code = metrics.error_code,
        );
        if self
            .runs
            .finish_turn(&run, &turn_id, &metrics)
            .await
            .is_err()
        {
            // Telemetry must not turn a committed draft into an HTTP failure.
            tracing::error!(run = %id, turn = turn_id.as_str(), "AI turn metrics persistence failed");
        }
        outcome
    }

    pub async fn undo_turn(
        &self,
        org: OrganizerId,
        id: ImportRunId,
        revision: i64,
        turn_id: String,
    ) -> Result<DraftState, AiImportError> {
        let run = self.editable_run(org, id).await?;
        let mut state = self.get_draft(org, id).await?;
        if state.turns.iter().any(|t| t.id == turn_id && t.undone) {
            return Ok(state);
        }
        if state.revision != revision || state.pending.is_some() {
            return Err(AiImportError::Conflict);
        }
        let t = state
            .turns
            .iter_mut()
            .rev()
            .find(|t| t.status == "completed" && !t.changes.is_empty() && !t.undone)
            .ok_or(AiImportError::Conflict)?;
        if t.id != turn_id {
            return Err(AiImportError::Conflict);
        }
        state.vendors = state.undo.take().ok_or(AiImportError::Conflict)?;
        t.undone = true;
        state.revision += 1;
        self.runs
            .save_draft(&run, &state, revision, self.clock.now(), None)
            .await?;
        Ok(state)
    }
}

fn cancel_pending(s: &mut DraftState, reason: &str) {
    if let Some(id) = s.pending.take()
        && let Some(t) = s.turns.iter_mut().find(|t| t.id == id)
    {
        t.status = "failed".into();
        t.reply = reason.into();
    }
    s.pending_until = None;
}

pub(super) fn accepted(vendors: &[VendorDraft]) -> Vec<AcceptedChange> {
    vendors
        .iter()
        .filter(|v| v.selected && (v.info_selected || v.menu.iter().any(|m| m.selected)))
        .map(|v| {
            AcceptedChange::Vendor(AcceptedVendor {
                vendor: v.vendor.clone(),
                info: v.info_selected.then(|| AcceptedVendorInfo {
                    name: Some(v.info.name.clone()),
                    phone: v.info.phone.clone(),
                    address: v.info.address.clone(),
                    note: v.info.note.clone(),
                }),
                menu: v
                    .menu
                    .iter()
                    .filter(|m| m.selected)
                    .map(|m| AcceptedMenuItem {
                        name: m.name.clone(),
                        price: m.price,
                        category: m.category.clone(),
                        description: m.description.clone(),
                    })
                    .collect(),
            })
        })
        .collect()
}
