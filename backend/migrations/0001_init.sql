-- Identity -------------------------------------------------------------
CREATE TABLE organizers (
  id            TEXT PRIMARY KEY,
  google_sub    TEXT NOT NULL UNIQUE,
  email         TEXT NOT NULL,
  display_name  TEXT NOT NULL,
  avatar_url    TEXT,
  created_at    TEXT NOT NULL,
  last_login_at TEXT NOT NULL
);

CREATE TABLE sessions (
  token_hash    BLOB PRIMARY KEY,          -- SHA-256(session token)，DB 外洩也無法冒用
  organizer_id  TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
  created_at    TEXT NOT NULL,
  expires_at    TEXT NOT NULL,
  user_agent    TEXT
);
CREATE INDEX idx_sessions_organizer ON sessions(organizer_id);

CREATE TABLE oauth_states (                -- 登入流程暫存（10 分鐘過期）
  state         TEXT PRIMARY KEY,
  pkce_verifier TEXT NOT NULL,
  nonce         TEXT NOT NULL,
  return_to     TEXT,
  expires_at    TEXT NOT NULL
);

-- Directory（名稱為 key，同名覆蓋）-------------------------------------
CREATE TABLE rosters (
  id            TEXT PRIMARY KEY,
  organizer_id  TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE TABLE roster_members (
  id            TEXT PRIMARY KEY,
  roster_id     TEXT NOT NULL REFERENCES rosters(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,             -- 顯示用原名
  name_key      TEXT NOT NULL,             -- NameKey 正規化結果
  note          TEXT,
  sort_order    INTEGER NOT NULL DEFAULT 0,
  updated_at    TEXT NOT NULL,
  UNIQUE (roster_id, name_key)
);

CREATE TABLE vendors (
  id            TEXT PRIMARY KEY,
  organizer_id  TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  name_key      TEXT NOT NULL,
  phone         TEXT,
  address       TEXT,
  note          TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  UNIQUE (organizer_id, name_key)
);

CREATE TABLE menu_items (
  id            TEXT PRIMARY KEY,
  vendor_id     TEXT NOT NULL REFERENCES vendors(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  name_key      TEXT NOT NULL,
  price         INTEGER NOT NULL CHECK (price BETWEEN 1 AND 100000),
  description   TEXT,
  category      TEXT,
  sort_order    INTEGER NOT NULL DEFAULT 0,
  archived      INTEGER NOT NULL DEFAULT 0,
  updated_at    TEXT NOT NULL,
  UNIQUE (vendor_id, name_key)
);

-- Ordering -------------------------------------------------------------
CREATE TABLE group_orders (
  id                TEXT PRIMARY KEY,
  organizer_id      TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
  title             TEXT NOT NULL,
  share_token       TEXT NOT NULL UNIQUE,
  status            TEXT NOT NULL CHECK (status IN ('open','closed','archived')),
  deadline_at       TEXT,
  note              TEXT,
  source_roster_id  TEXT,                  -- 僅供追溯，不設 FK（主資料可刪）
  source_vendor_id  TEXT,
  vendor_name       TEXT NOT NULL,         -- 快照
  vendor_phone      TEXT,
  vendor_address    TEXT,
  created_at        TEXT NOT NULL,
  closed_at         TEXT
);
CREATE INDEX idx_group_orders_organizer ON group_orders(organizer_id, created_at DESC);

CREATE TABLE participants (                -- 團員快照
  id                TEXT PRIMARY KEY,
  group_order_id    TEXT NOT NULL REFERENCES group_orders(id) ON DELETE CASCADE,
  name              TEXT NOT NULL,
  sort_order        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE offered_items (               -- 菜單快照
  id                TEXT PRIMARY KEY,
  group_order_id    TEXT NOT NULL REFERENCES group_orders(id) ON DELETE CASCADE,
  name              TEXT NOT NULL,
  price             INTEGER NOT NULL CHECK (price > 0),
  description       TEXT,
  category          TEXT,
  sort_order        INTEGER NOT NULL DEFAULT 0,
  sold_out          INTEGER NOT NULL DEFAULT 0
);

-- 送出紀錄：只新增。每人最新一筆 = 有效訂單；lines 為空 = 取消
CREATE TABLE order_submissions (
  id                TEXT PRIMARY KEY,      -- UUIDv7，排序即時間順序
  group_order_id    TEXT NOT NULL REFERENCES group_orders(id) ON DELETE CASCADE,
  participant_id    TEXT NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
  note              TEXT,
  idempotency_key   TEXT NOT NULL,
  submitted_by      TEXT NOT NULL CHECK (submitted_by IN ('participant','organizer')),
  submitted_at      TEXT NOT NULL,
  -- ClientInfo（團關閉 90 天後清為 NULL）
  client_ip         TEXT,
  user_agent        TEXT,
  ua_summary        TEXT,
  device_id         TEXT,
  device_model      TEXT,
  screen            TEXT,
  language          TEXT,
  timezone          TEXT,
  UNIQUE (group_order_id, idempotency_key)
);
CREATE INDEX idx_submissions_latest  ON order_submissions(group_order_id, participant_id, id DESC);
CREATE INDEX idx_submissions_device  ON order_submissions(group_order_id, device_id);

CREATE TABLE submission_lines (
  submission_id     TEXT NOT NULL REFERENCES order_submissions(id) ON DELETE CASCADE,
  line_no           INTEGER NOT NULL,
  offered_item_id   TEXT NOT NULL REFERENCES offered_items(id),
  quantity          INTEGER NOT NULL CHECK (quantity BETWEEN 1 AND 20),
  unit_price        INTEGER NOT NULL,      -- 送出當下價格
  note              TEXT,                  -- 單一品項備註（不要辣、飯少）
  PRIMARY KEY (submission_id, line_no)
);

-- 有效訂單：每位參與者最新一筆送出紀錄
CREATE VIEW effective_submissions AS
SELECT s.*
FROM order_submissions s
WHERE s.id = (SELECT MAX(s2.id) FROM order_submissions s2
              WHERE s2.group_order_id = s.group_order_id
                AND s2.participant_id = s.participant_id);

-- Settlement -----------------------------------------------------------
CREATE TABLE settlements (
  group_order_id    TEXT NOT NULL REFERENCES group_orders(id) ON DELETE CASCADE,
  participant_id    TEXT NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
  picked_up_at      TEXT,
  paid_at           TEXT,
  paid_amount       INTEGER,               -- 勾選繳費時的應付金額
  updated_at        TEXT NOT NULL,
  PRIMARY KEY (group_order_id, participant_id)
);

-- AI Import ------------------------------------------------------------
CREATE TABLE ai_import_runs (
  id                TEXT PRIMARY KEY,
  organizer_id      TEXT NOT NULL REFERENCES organizers(id) ON DELETE CASCADE,
  mode_hint         TEXT CHECK (mode_hint IN ('roster','vendor')),
  target_roster_id  TEXT,                  -- 使用者從某名單頁發起時帶入
  target_vendor_id  TEXT,
  instruction       TEXT,                  -- 使用者附的一句指示
  status            TEXT NOT NULL CHECK (status IN ('running','proposed','applied','discarded','failed')),
  proposal_json     TEXT,                  -- Proposal（待確認變更）
  summary           TEXT,                  -- agent 最後的繁中摘要
  tool_calls_json   TEXT,                  -- agent 呼叫過的 tools 與參數（除錯用）
  model             TEXT NOT NULL,
  input_tokens      INTEGER,
  output_tokens     INTEGER,
  error             TEXT,
  created_at        TEXT NOT NULL,
  finished_at       TEXT,
  applied_at        TEXT
);
CREATE INDEX idx_import_runs_organizer ON ai_import_runs(organizer_id, created_at DESC);
