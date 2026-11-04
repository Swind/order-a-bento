-- NULL preserves unknown execution/usage for historical and interrupted turns.
ALTER TABLE ai_import_turn_usage ADD COLUMN finished_at TEXT;
ALTER TABLE ai_import_turn_usage ADD COLUMN input_tokens INTEGER CHECK(input_tokens >= 0);
ALTER TABLE ai_import_turn_usage ADD COLUMN output_tokens INTEGER CHECK(output_tokens >= 0);
ALTER TABLE ai_import_turn_usage ADD COLUMN model TEXT;
ALTER TABLE ai_import_turn_usage ADD COLUMN duration_ms INTEGER CHECK(duration_ms >= 0);
ALTER TABLE ai_import_turn_usage ADD COLUMN execution_outcome TEXT CHECK(execution_outcome IN ('completed','failed','interrupted'));
ALTER TABLE ai_import_turn_usage ADD COLUMN draft_outcome TEXT CHECK(draft_outcome IN ('applied','unchanged','rejected'));
ALTER TABLE ai_import_turn_usage ADD COLUMN error_code TEXT;
