-- no-transaction
PRAGMA foreign_keys = OFF;
BEGIN;
CREATE TABLE sharing_groups (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  created_by TEXT NOT NULL REFERENCES organizers(id),
  created_at TEXT NOT NULL
);
CREATE TABLE sharing_group_members (
  sharing_group_id TEXT NOT NULL REFERENCES sharing_groups(id) ON DELETE CASCADE,
  organizer_id TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK(role IN ('admin', 'member')),
  PRIMARY KEY(sharing_group_id, organizer_id)
);
CREATE INDEX idx_sharing_members_user ON sharing_group_members(organizer_id);
ALTER TABLE rosters ADD COLUMN sharing_group_id TEXT REFERENCES sharing_groups(id);
CREATE TABLE vendors_new (
  id TEXT PRIMARY KEY,
  organizer_id TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  name_key TEXT NOT NULL,
  phone TEXT,
  address TEXT,
  note TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  sharing_group_id TEXT REFERENCES sharing_groups(id)
);
INSERT INTO vendors_new SELECT *, NULL FROM vendors;
DROP TABLE vendors;
ALTER TABLE vendors_new RENAME TO vendors;
CREATE UNIQUE INDEX idx_vendor_private_name ON vendors(organizer_id,name_key) WHERE sharing_group_id IS NULL;
CREATE UNIQUE INDEX idx_vendor_shared_name ON vendors(sharing_group_id,name_key) WHERE sharing_group_id IS NOT NULL;
ALTER TABLE ai_import_runs ADD COLUMN sharing_group_id TEXT REFERENCES sharing_groups(id);
CREATE INDEX idx_rosters_sharing_group ON rosters(sharing_group_id);
CREATE INDEX idx_vendors_sharing_group ON vendors(sharing_group_id);
CREATE TRIGGER roster_scope_immutable BEFORE UPDATE OF sharing_group_id ON rosters
WHEN OLD.sharing_group_id IS NOT NEW.sharing_group_id
BEGIN SELECT RAISE(ABORT, 'immutable sharing scope'); END;
CREATE TRIGGER vendor_scope_immutable BEFORE UPDATE OF sharing_group_id ON vendors
WHEN OLD.sharing_group_id IS NOT NEW.sharing_group_id
BEGIN SELECT RAISE(ABORT, 'immutable sharing scope'); END;
CREATE TRIGGER import_scope_immutable BEFORE UPDATE OF sharing_group_id, organizer_id ON ai_import_runs
WHEN OLD.sharing_group_id IS NOT NEW.sharing_group_id OR OLD.organizer_id != NEW.organizer_id
BEGIN SELECT RAISE(ABORT, 'immutable import scope'); END;
COMMIT;
PRAGMA foreign_keys = ON;
