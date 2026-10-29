CREATE TABLE ai_import_photos (
    run_id TEXT PRIMARY KEY REFERENCES ai_import_runs(id) ON DELETE CASCADE,
    data BLOB NOT NULL CHECK (length(data) BETWEEN 1 AND 5242880),
    width INTEGER NOT NULL CHECK (width BETWEEN 1 AND 1568),
    height INTEGER NOT NULL CHECK (height BETWEEN 1 AND 1568),
    expires_at TEXT NOT NULL
);
