CREATE TABLE ai_import_drafts (
    run_id TEXT PRIMARY KEY REFERENCES ai_import_runs(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    state_json TEXT NOT NULL
);
CREATE TABLE ai_import_turn_usage (
    run_id TEXT NOT NULL REFERENCES ai_import_runs(id) ON DELETE CASCADE,
    turn_id TEXT NOT NULL,
    organizer_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(run_id, turn_id)
);
CREATE INDEX ai_import_turn_usage_quota ON ai_import_turn_usage(organizer_id, created_at);
