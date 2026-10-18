# order-a-bento 訂便當

團體訂便當系統：開團者建立名單和店家，開一團後把連結貼到 LINE 群組。大家不用登入就能點餐，開團者在同一頁看叫貨單、勾選領取和繳費。

- **開團者**：Google 登入或本機帳號（email + 密碼），管理名單、店家、團。
- **點餐者**：打開分享連結，選自己的名字點餐，不需要帳號。同一個名字再送一次會覆蓋前一筆，歷史紀錄保留。
- **AI 匯入**（選用）：拍名單或菜單照片，由模型讀出內容。AI 只會產生「待確認的提案」，開團者檢查、修改後按套用才寫入。

## 功能

| 頁面 | 說明 |
|---|---|
| 名單 | 成員清單，可手動輸入或從照片匯入 |
| 店家 | 菜單、價格、電話、地址，可手動輸入或從傳單匯入 |
| 開團 | 三步精靈：選名單 → 選店家 → 標題、截止時間、備註。開團時會複製一份名單和菜單，之後修改不影響已開的團 |
| 團頁面 | 分享連結（複製、分享到 LINE、重新產生）；**統計**（叫貨單、複製訂單文字、標示售完）；**明細**（每人訂單和改單歷史、代改）；**領取＆繳費**（每人勾選領取、繳費，計算已收和尚差） |
| 點餐頁 `/g/<token>` | 不需登入，手機優先。截止或關閉收單後唯讀 |

點餐會記錄 IP 和裝置資訊，團關閉超過 `CLIENT_INFO_RETENTION_DAYS`（預設 90 天）後自動清除。

## 架構

單一執行檔：Rust 後端在編譯時把前端的 `dist/` 內嵌進去，資料存在一個 SQLite 檔案。部署只需要一個容器加一個 volume。

```
┌────────────── server (single binary) ──────────────┐
│  axum HTTP  ─ /api/*  JSON API (OpenAPI via utoipa) │
│             ─ /auth/* Google OIDC / password login  │
│             ─ /*      embedded React SPA            │
│                                                     │
│  application use cases ─ domain                     │
│  infrastructure: SQLite (sqlx) · Argon2 · rig (AI)  │
└──────────────┬───────────────────────┬──────────────┘
               │                       │
         /data/bento.db          AI provider (optional)
               │                 Anthropic / OpenAI-compatible
```

### 技術

- **後端**：Rust、axum 0.8、sqlx 0.9（SQLite、WAL）、utoipa（OpenAPI，開發模式提供 `/api/docs`）、[rig](https://github.com/Swind/rig)（AI agent，Swind fork）
- **前端**：React 19、Vite、TypeScript、shadcn/ui（Base UI）、TanStack Query，API 型別由 OpenAPI 產生
- **測試**：cargo test、Vitest、Playwright E2E、AI 評測工具（`make ai-eval`）

### 目錄

```
backend/                 Cargo workspace（DDD + Clean Architecture）
  crates/domain/         實體、值物件、業務規則，不依賴任何框架
  crates/application/    use case、port（repository、clock、AI agent…）、AI 匯入的 prompt 和 tool
  crates/infrastructure/ port 的實作：SQLite repository、Argon2id、Google OIDC、rig AI agent
  crates/api/            axum 路由、DTO、驗證、CSRF、限流、OpenAPI
  crates/server/         設定、組裝（wiring）、main、CLI（server admin …）、ai_eval
  migrations/            SQLite migration（啟動時自動套用）
frontend/                React SPA
  src/app/               router、layout、providers
  src/features/          auth、rosters、vendors、group-orders、public-order、ai-import
  src/shared/            API client（openapi-typescript 產生）、共用元件、shadcn ui
e2e/                     Playwright E2E（對 release 執行檔跑，不連任何外部服務）
eval/cases/              AI 匯入評測案例（圖片 + 期望結果）
docs/                    design.md（設計）、plan.md（開發計畫）、deploy.md（部署細節）
```

依賴方向是 `api / infrastructure → application → domain`，`server` 負責把它們組起來。domain 和 application 不碰 HTTP、SQL 或 AI SDK，都透過 port 介面，所以 use case 可以用 fake 實作做單元測試。

## 本機開發

需要 Rust stable、Node 22。

```sh
cp .env.example .env              # 視需要修改；.env 不可提交
make dev-backend                  # 後端 :3000，DEV_LOGIN=true，DB 在 backend/data/dev.db
make dev-frontend                 # 前端 :5173，/api、/auth 代理到 :3000
```

開發模式可以用登入頁的開發登入捷徑。如果要測試密碼登入，先建立本機管理員：

```sh
make admin EMAIL=you@example.com NAME="你的名字"   # 互動輸入密碼
```

沒有 AI key 時可以在 `.env` 設 `AI_DEMO=true`，AI 匯入會回傳固定的範例資料（production 不允許）。

### 常用指令

| 指令 | 說明 |
|---|---|
| `make check` | 所有 CI 檢查：fmt、clippy、後端測試、OpenAPI 同步、前端 typecheck、測試、build |
| `make e2e` | 建置 release 執行檔後跑 Playwright（本機：`CHROME_PATH=/usr/bin/google-chrome make e2e`） |
| `make gen-api` | 後端 API 改動後，重新匯出 `openapi.json` 並產生前端型別 |
| `make ai-eval` | 用真實模型跑 AI 匯入評測（會花 token，不在 CI 中） |
| `make docker-build` | 建置 Docker 映像 `order-a-bento:latest` |

## 部署

完整說明（Google OAuth 申請、所有環境變數、反向代理、備份還原、安全檢查清單）見 [docs/deploy.md](docs/deploy.md)。以下是最短路徑。

**限制**：SQLite 只有一個寫入者，**只能跑單一實例**。

### 1. 建置映像

```sh
make docker-build
```

映像以非 root 使用者（uid 10001）執行，監聽 3000，資料在 `/data`，預設 `APP_ENV=production`。

### 2. 啟動

```sh
docker run -d --name bento --restart unless-stopped \
  -p 127.0.0.1:3000:3000 \
  -v bento-data:/data \
  -e APP_BASE_URL=https://bento.example.com \
  -e TRUSTED_PROXIES=172.17.0.1 \
  order-a-bento:latest
```

也可以用 `--env-file .env`。完整的環境變數說明見下方 [環境變數](#環境變數)；最少只需 `APP_BASE_URL`（對外網址，必設）與 `TRUSTED_PROXIES`。

### 3. 建立管理員

不用 Google 也能登入。用同一個執行檔的 CLI 建立帳號，密碼不會留在環境變數或 shell 歷史：

```sh
docker exec -it bento server admin create --email you@example.com --name "你的名字"
docker exec -it bento server admin set-password --email you@example.com
docker exec bento server admin list
```

### 4. 反向代理（HTTPS）

容器只綁 `127.0.0.1`，由反向代理提供 HTTPS，例如 Caddy：

```caddyfile
bento.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:3000
}
```

AI 匯入的請求最長會等 `AI_RUN_TIMEOUT_SECS`（預設 90 秒，compatible provider 300 秒），代理的讀取逾時要大於這個值。

### 5. AI provider 範例

```sh
# Anthropic 官方
AI_PROVIDER=anthropic
AI_API_KEY=...
AI_MODEL=claude-opus-5-5

# Anthropic Messages API 相容的 gateway（例：opencode Go 上的 Claude Haiku）
AI_PROVIDER=anthropic
AI_BASE_URL=https://opencode.ai/zen/go
AI_API_KEY=...
AI_MODEL=claude-haiku-5-5
AI_SESSION_HEADER=x-opencode-session
AI_FALLBACKS=off

# OpenAI Chat Completions 相容（DeepSeek、vLLM、LiteLLM…）
AI_PROVIDER=openai-compatible
AI_BASE_URL=https://example.com
AI_API_KEY=...
AI_MODEL=deepseek-v4.1-flash
```

只支援 `/v1/responses` 的模型用 `AI_PROVIDER=openai-responses`。換模型前建議先跑 `make ai-eval` 比較準確度。

### 6. 升級與備份

- 備份：映像沒有 `sqlite3`，建議短暫停機後直接打包整個 volume（`bento.db` 連同 `-wal`/`-shm` 一致地複製）：

```sh
docker stop bento
docker run --rm -v bento-data:/data -v "$PWD":/backup debian:bookworm-slim \
  tar czf /backup/bento-$(date +%F).tgz -C /data .
docker start bento
```

  不要在服務執行中只複製 `bento.db` 單檔（會漏掉 `-wal`）。還原方式見 [docs/deploy.md](docs/deploy.md) §7。
- 升級：建置新映像，停掉舊容器，用同樣的 volume 和參數啟動。migration 在啟動時自動執行。
- 健康檢查：`GET /healthz`，映像內建 Docker `HEALTHCHECK`。

## 環境變數

規則（適用所有變數）：

- **空字串、只有空白＝未設定**，一律採預設值（值會先去除前後空白）。
- 布林值（標為「布林」）接受 `true/1/yes/on` 與 `false/0/no/off`（不分大小寫），其他值會讓伺服器拒絕啟動。數字欄位填非數字亦拒絕啟動。
- 設定錯誤時伺服器啟動失敗（非零結束碼，原因印在日誌）。
- 非機密變數的「機密」欄為 no；標 **yes** 的應放進 Ansible vault／secret。
- 伺服器會嘗試讀取工作目錄的 `.env`（dotenvy），但容器內請用 `docker run -e` 或 `--env-file`。

### 伺服器

| 變數 | 必填 | 預設值（開發／映像內） | 可用值／格式 | 說明 | 機密 |
|---|---|---|---|---|---|
| `APP_ENV` | 否 | `development`／映像內 `production` | `development`、`production`（不分大小寫） | `production` 隱藏 `/api/docs`，並拒絕啟動若 `DEV_LOGIN=true` 或 `AI_DEMO=true` | no |
| `PORT` | 否 | `3000`／`3000` | 整數 | 容器內監聽埠 | no |
| `APP_BASE_URL` | **是（正式環境）** | `http://localhost:3000` | 完整 URL，無結尾路徑，例 `https://bento.example.com` | 公開網址：分享連結、OAuth 回呼（`/auth/google/callback`）、OG 預覽。`https://` 開頭時 session cookie 才帶 `Secure`。其 origin 自動列入 CSRF 允許清單 | no |
| `ALLOWED_ORIGINS` | 否 | 開發：`http://localhost:5173`；production：空 | 逗號分隔的 origin | 額外允許送出狀態變更請求的 Origin（CSRF 檢查）。通常不需設定 | no |
| `TRUSTED_PROXIES` | 建議 | 空（不信任任何來源） | 逗號分隔的 IP／CIDR，例 `172.17.0.1` | 只有來自這些位址的連線，其 `X-Forwarded-For` 才被採信（影響速率限制與記錄的 client IP）。不要填 `0.0.0.0/0` | no |
| `DEV_LOGIN` | 否 | `false` | 布林 | 開發用登入捷徑。**`APP_ENV=production` 時設為 true 會拒絕啟動** | no |
| `DISPLAY_UTC_OFFSET` | 否 | `+08:00` | `±HH:MM` | 連結預覽（OG）中截止時間顯示的時區 | no |
| `RUST_LOG` | 否 | `info,tower_http=info` | tracing filter 語法 | 日誌等級 | no |

### 資料庫

| 變數 | 必填 | 預設值（開發／映像內） | 可用值／格式 | 說明 | 機密 |
|---|---|---|---|---|---|
| `DATABASE_URL` | 否 | `sqlite://data/bento.db`／映像內 `sqlite:///data/bento.db` | `sqlite://<路徑>`（絕對路徑用三個斜線） | SQLite 檔。映像內為 `/data/bento.db`，請掛載 volume 到 `/data`，不要改動 | no |

### 登入：Google 與本機帳號

| 變數 | 必填 | 預設值（開發／映像內） | 可用值／格式 | 說明 | 機密 |
|---|---|---|---|---|---|
| `GOOGLE_CLIENT_ID` | 否 | 空 | Google OAuth client id | 與下一項需同時設定才會啟用 Google 登入；回呼網址 `APP_BASE_URL/auth/google/callback` | no |
| `GOOGLE_CLIENT_SECRET` | 否 | 空 | 字串 | 同上 | **yes** |
| `GOOGLE_OAUTH_ENABLED` | 否 | 空（自動） | 布林 | 空＝兩個憑證都設才啟用；`false`＝即使有憑證也停用（`/auth/google/*` 回 503、登入頁隱藏按鈕）；`true`＝必須兩個憑證都有，否則拒絕啟動 | no |
| `PASSWORD_LOGIN_ENABLED` | 否 | `true` | 布林 | 本機帳號（email + 密碼）登入；`false` 時 `POST /auth/password-login` 回 404 | no |
| `ADMIN_EMAIL` | 否 | 空 | email | 啟動時的管理員 bootstrap。**必須與 `ADMIN_PASSWORD` 同時設定或同時不設**，否則拒絕啟動。該 email 尚無帳號時才建立；已存在的帳號與密碼絕不覆寫 | no |
| `ADMIN_PASSWORD` | 否 | 空 | 10 到 256 字元 | 同上。建立後建議移除；偏好改用 CLI 建立（見下方重點）。`PASSWORD_LOGIN_ENABLED=false` 時設定它會被警告且無人可用 | **yes** |

### AI 匯入

沒有 key（`AI_API_KEY`）時隱藏 AI 匯入按鈕（`AI_DEMO` 除外）。

| 變數 | 必填 | 預設值（開發／映像內） | 可用值／格式 | 說明 | 機密 |
|---|---|---|---|---|---|
| `AI_PROVIDER` | 否 | `anthropic` | `anthropic`、`anthropic-compatible`、`openai-compatible`、`openai-responses`（不分大小寫） | `anthropic`＝Anthropic 官方 API；`anthropic-compatible`＝說 Messages API 的閘道；`openai-compatible`＝OpenAI Chat Completions（附加 `/v1/chat/completions`）；`openai-responses`＝只支援 Responses API（附加 `/v1/responses`） | no |
| `AI_API_KEY` | 否（要用 AI 才需要） | 空 | 字串 | 設定後啟用 AI 匯入。舊名 `ANTHROPIC_API_KEY` 只在 `anthropic`／`anthropic-compatible` 且 `AI_API_KEY` 未設時作為後備；兩個 openai provider 不讀它 | **yes** |
| `ANTHROPIC_API_KEY` | 否 | 空 | 字串 | 舊名，見上；新部署請用 `AI_API_KEY` | **yes** |
| `AI_BASE_URL` | 見說明 | 空；`anthropic` 時為 `https://api.anthropic.com` | URL（結尾有無 `/v1` 皆可） | `openai-compatible`／`openai-responses` **必填**。舊名 `ANTHROPIC_BASE_URL` 規則同 `ANTHROPIC_API_KEY` | no |
| `ANTHROPIC_BASE_URL` | 否 | 空 | URL | 舊名，見上 | no |
| `AI_MODEL` | 見說明 | `claude-opus-5-5` | 模型名稱 | 設了 key 的 `openai-compatible`／`openai-responses` **必填**（預設是 Claude 模型，不適用） | no |
| `AI_EXTRA_HEADERS` | 否 | 空 | `name=value,name2=value2`（值不可含逗號，須為可列印 ASCII，標頭名僅限字母數字 `-` `_`） | 每次模型請求附加的 HTTP 標頭；值視為機密，不寫入日誌 | **yes** |
| `AI_SESSION_HEADER` | 否 | 空 | 標頭名稱 | 以此標頭送出匯入 run id（供依 session 路由的閘道，例 `x-opencode-session`） | no |
| `AI_MAX_TURNS` | 否 | `8` | 正整數 | 每次匯入最多呼叫模型次數 | no |
| `AI_DAILY_QUOTA_PER_USER` | 否 | `30` | 整數 | 每位使用者每日 AI 匯入次數上限 | no |
| `AI_FALLBACKS` | 否 | `anthropic`：`default`；其他 provider：`off` | `default`、`off` | 請求被拒時是否讓 API 退回其他模型。**`default` 只在 `AI_PROVIDER=anthropic` 有效**，其他 provider 設 `default` 會拒絕啟動 | no |
| `AI_STRICT_TOOLS` | 否 | `anthropic`：`true`；其他：`false` | 布林 | 工具 schema 使用 strict 模式 | no |
| `AI_REASONING` | 否 | `anthropic`：`low`；其他：`off` | `low`、`off` | `low`＝送出低推理強度參數；`off`＝不送推理參數 | no |
| `AI_REQUEST_TIMEOUT_SECS` | 否 | `anthropic`：`60`；其他：`180` | 正整數（秒） | 單次模型 HTTP 請求逾時；逾時不重試。必須 > 0 且 <= `AI_RUN_TIMEOUT_SECS` | no |
| `AI_RUN_TIMEOUT_SECS` | 否 | `anthropic`：`90`；其他：`300` | 正整數（秒） | 整次匯入（含一次重試）逾時；必須 >= `AI_REQUEST_TIMEOUT_SECS`。匯入請求最長會保持開啟這麼久 | no |
| `AI_DEMO` | 否 | `false` | 布林 | 以腳本化假資料回應的示範 agent，僅供開發。**`APP_ENV=production` 時設為 true 會拒絕啟動** | no |

### Session、隱私、背景清理與速率限制

| 變數 | 必填 | 預設值（開發／映像內） | 可用值／格式 | 說明 | 機密 |
|---|---|---|---|---|---|
| `SESSION_TTL_DAYS` | 否 | `14` | 整數（天） | 登入 session 有效天數 | no |
| `CLIENT_INFO_RETENTION_DAYS` | 否 | `90` | 整數（天） | 團關閉超過此天數後，清除其訂單的 IP／UA／裝置資訊 | no |
| `MAINTENANCE_INTERVAL_HOURS` | 否 | `24` | 整數 >= 1 | 背景清理工作週期（啟動後 30 秒先跑一次）；`0` 會拒絕啟動 | no |
| `PUBLIC_RATE_LIMIT_BURST` | 否 | `30` | 整數 | 公開端點每個 client IP 的 token bucket 容量 | no |
| `PUBLIC_RATE_LIMIT_PER_SEC` | 否 | `2` | 整數 | 公開端點每個 client IP 每秒補充量 | no |

### 部署給自動化工具的重點

- 映像：`order-a-bento:latest`（自行 `docker build -t order-a-bento:latest .`）；入口為 `/usr/local/bin/server`，以非 root 的 `bento`（uid 10001）執行，容器埠 `3000`。
- Volume：掛載到 `/data`，目錄擁有者須為 uid 10001（命名 volume 會自動處理；bind mount 要 `chown 10001`）。
- 健康檢查：`GET /healthz` 回 200 與 `{"status":"ok"}`（映像已內建 `HEALTHCHECK`）。
- **只能跑單一實例**（SQLite 單寫入者），升級時先停舊容器再啟動新容器。
- `docker run --env-file` 格式：每行 `KEY=value`，**值不加引號、不可有行內註解**（`#` 之後的內容會成為值的一部分）；整行註解需獨立一行。
- 非互動建立管理員：

  ```sh
  printf '%s\n' "$PASSWORD" | docker exec -i bento server admin create --email you@example.com --name "名字" --password-stdin
  ```

  結束碼：`0` 成功、`1` 內部錯誤、`2` 用法錯誤、`3` 輸入無效（email 格式或密碼強度）、`4` 已存在、`5` 找不到。收到 `4` 時改用 `server admin set-password --email ... --password-stdin`（冪等做法：先 create，遇 4 再 set-password）。
- 反向代理的 read timeout 必須大於 `AI_RUN_TIMEOUT_SECS`（預設 90／300 秒），否則 AI 匯入會被代理切斷；並設定 `TRUSTED_PROXIES` 為代理連到容器時的來源位址。
- 備份：停機打包 volume，見上方「升級與備份」。

## License

[MIT](LICENSE)
