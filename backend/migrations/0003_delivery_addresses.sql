CREATE TABLE delivery_addresses (
    id TEXT PRIMARY KEY NOT NULL,
    organizer_id TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 50),
    address TEXT NOT NULL CHECK (length(address) BETWEEN 1 AND 200),
    updated_at TEXT NOT NULL
);
CREATE INDEX delivery_addresses_organizer ON delivery_addresses(organizer_id, updated_at);

-- Snapshot, independent of subsequent edits/deletions in the address book.
ALTER TABLE group_orders ADD COLUMN delivery_address TEXT;

-- SQLite cannot alter a CHECK constraint in place. Preserve every existing run.
CREATE TABLE ai_import_runs_new (
    id TEXT PRIMARY KEY,
    organizer_id TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
    mode_hint TEXT CHECK (mode_hint IN ('roster','vendor','deliveryAddress')),
    target_roster_id TEXT,
    target_vendor_id TEXT,
    instruction TEXT,
    status TEXT NOT NULL CHECK (status IN ('running','proposed','applied','discarded','failed')),
    proposal_json TEXT,
    summary TEXT,
    tool_calls_json TEXT,
    model TEXT NOT NULL,
    input_tokens INTEGER,
    output_tokens INTEGER,
    error TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT,
    applied_at TEXT
);
INSERT INTO ai_import_runs_new SELECT * FROM ai_import_runs;
DROP TABLE ai_import_runs;
ALTER TABLE ai_import_runs_new RENAME TO ai_import_runs;
CREATE INDEX idx_import_runs_organizer ON ai_import_runs(organizer_id, created_at DESC);
