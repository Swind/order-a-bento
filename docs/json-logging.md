# JSON 日誌

`LOG_FORMAT=text`（預設）保留開發用文字格式；部署可設：

```sh
LOG_FORMAT=json RUST_LOG=info,tower_http=info /usr/local/bin/server
```

`LOG_FORMAT` 僅接受精確 `text`／`json`，其他值（含空字串）啟動失敗。
兩種格式共用 `RUST_LOG` 等級／target filter。JSON 使用 tracing-subscriber 標準 formatter，
輸出至 stdout，每個事件一行合法 JSON，無 ANSI；內容換行以 JSON escape 表示。
CLI admin 的表格／操作結果與啟動失敗的 stderr 訊息不是這個事件 stream。

## 欄位與關聯

- 頂層 `timestamp`、`level`、`target`；`fields` 裝事件欄位與 `message`（若事件有訊息）。不 flatten，避免事件名稱撞到 metadata。
- `span` 是目前 span；`spans` 按根到葉列出完整父鏈。HTTP `request` 有 server-generated `request_id`、method、遮罩 path（不含 query）。
- HTTP request span 的 `client_ip` 共用限流的來源 IP 解析：只有 TCP peer 在 `TRUSTED_PROXIES` 內才解析 `X-Forwarded-For`，由右向左取第一個非可信代理 IP；否則記錄 peer IP。沒有連線資訊時省略欄位。
- cloudflared 部署需將 Bento 實際看到的 cloudflared peer IP 加入 `TRUSTED_PROXIES`，例如 `TRUSTED_PROXIES=10.0.0.5`（請換成實際位址）。若中間還有反向代理，也需按實際代理鏈設定；不要信任所有來源。此設定也影響既有 per-IP 限流。不直接信任 `CF-Connecting-IP`。
- AI 初次匯入的 `ai_import` 與草稿 `ai_draft_turn` span 有 `run`、`turn`；從 `spans` 同時取 request/run/turn，不要只看葉 `span`。跨 async 輪詢維持關聯，不信任 client `x-request-id`。
- turn ID 可為非 UUID 字串；使用原生 tracing 字串值。JSON 解析後與原始 ID 精確相等（不多包引號或預先轉義），text formatter 則安全轉義換行，避免偽造實體日誌行。
- 僅 `/api` 與 `/api/...` 的 HTTP 回應記錄存取事件；靜態資源、前端頁面、`/healthz` 與 `/auth/...` 不記錄 HTTP 存取／failure 日誌（text／json 皆適用）。所有回應仍有 `x-request-id`。
- 每個 API 回應只有一個 `fields.event=http_request_completed` 事件，`fields.message` 為易讀的 `GET /api/v1/vendors`（method + 遮罩後、不含 query 的 path），取代原本固定的 `finished processing request`。以 `event` 聚合事件類型，不依賴 message；`fields.status` 是數字、`fields.duration_ms` 是 monotonic 整數毫秒。
- API 的 server-error classification 日誌仍保留，標記 `fields.event=http_request_failed`；不另加 request-start 行。統計請求數時只計算 `http_request_completed`，避免 5xx 的 failure 事件重複計數。
- AI `fields.event` 為 `ai_import_started/completed`、`ai_turn_started/completed/replay`。tokens、`duration_ms` 為數字；可用 model／error_code 是普通字串。未知值省略（消費端視為 null），不是 `0` 或 `Some(...)`；開始事件的 duration 0 表示當下已過時間，而非已完成耗時。
- `quota_unit=internal_operation_24h` 是內部操作配額；`usage_kind=observed_tokens_not_invoice` 不是帳單。replay 不重跑模型，tokens 不重複觀測。

以下為合成範例（省略 timestamp 的實際精度，不含真實資料）：

```json
{"timestamp":"2026-11-05T00:00:00Z","level":"INFO","fields":{"event":"ai_turn_completed","model":"safe-model","input_tokens":11,"output_tokens":3,"duration_ms":12,"execution":"completed","outcome":"unchanged"},"target":"application::ai_import::draft_service","span":{"name":"ai_draft_turn","run":"synthetic-run","turn":"synthetic-turn"},"spans":[{"name":"request","request_id":"synthetic-request","method":"POST","path":"/api/v1/ai/import-runs/synthetic-run/turns"},{"name":"ai_draft_turn","run":"synthetic-run","turn":"synthetic-turn"}]}
```

新增診斷不記 token、query、header 值、prompt、圖片或 provider raw error。不要在正式環境任意開第三方套件 TRACE／DEBUG，或加入會印私密 payload 的 tracing。
本功能只提供 JSON 輸出，沒有配置 VictoriaLogs／collector，也不是 durable worker 或完整 audit log。
