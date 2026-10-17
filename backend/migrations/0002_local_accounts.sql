-- no-transaction
-- Local (password) accounts: google_sub becomes nullable and password_hash is added.
--
-- SQLite cannot drop NOT NULL, so `organizers` is rebuilt (the documented 12-step
-- procedure). sqlx wraps migrations in a transaction by default, but
-- `PRAGMA foreign_keys` is a no-op inside one, and with foreign keys ON
-- `DROP TABLE organizers` would cascade-delete sessions, rosters, vendors and group
-- orders. The first line makes sqlx run this file outside a transaction; the
-- transaction below is therefore explicit, and foreign keys are switched back on at
-- the end (the migrating connection is pooled and keeps its pragma state).
PRAGMA foreign_keys = OFF;

BEGIN;

CREATE TABLE organizers_new (
  id            TEXT PRIMARY KEY,
  google_sub    TEXT UNIQUE,                -- NULL = local (password) account
  email         TEXT NOT NULL,              -- local accounts: trimmed + lowercased by the app
  display_name  TEXT NOT NULL,
  avatar_url    TEXT,
  created_at    TEXT NOT NULL,
  last_login_at TEXT NOT NULL,
  password_hash TEXT,                       -- Argon2id PHC string
  CHECK (google_sub IS NOT NULL OR password_hash IS NOT NULL)  -- every account can log in somehow
);

INSERT INTO organizers_new (id, google_sub, email, display_name, avatar_url, created_at, last_login_at)
SELECT id, google_sub, email, display_name, avatar_url, created_at, last_login_at FROM organizers;

DROP TABLE organizers;
ALTER TABLE organizers_new RENAME TO organizers;

-- Emails are unique among local accounts only. Google/dev accounts are keyed by
-- google_sub, never linked to a local account by email, and may share an address.
CREATE UNIQUE INDEX idx_organizers_local_email ON organizers(email) WHERE google_sub IS NULL;
-- Case-insensitive lookups for the env bootstrap ("does any organizer have this email?").
CREATE INDEX idx_organizers_email_lower ON organizers(lower(email));

-- Must stay empty: every child table still points at `organizers`.
SELECT * FROM pragma_foreign_key_check;

COMMIT;

PRAGMA foreign_keys = ON;
