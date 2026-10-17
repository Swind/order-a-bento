# 部署指南

單一 Docker 映像（前端已內嵌於後端執行檔）+ 一個 SQLite 檔（`/data/bento.db`）。**只能跑單一實例**（SQLite 單寫入者）。

## 1. 前置需求

- Docker（或可執行 Linux 容器的環境）、一個對外的 HTTPS 網域（例：`https://bento.example.com`）。
- Google OAuth 用戶端（見 §3）。
- 選用：Anthropic API key（AI 匯入）、S3 相容 bucket（Litestream 備份）。

## 2. 建置映像

```sh
make docker-build            # 等同 docker build -t order-a-bento:latest .
```

映像以非 root 使用者 `bento`（uid 10001）執行，資料目錄為 `/data`（`VOLUME`），監聽 `3000`。

## 3. Google 登入設定

1. 開啟 Google Cloud Console，建立（或選擇）專案。
2. 「API 與服務 → OAuth 同意畫面」：User Type 選「外部」（或內部，若使用 Workspace），填應用程式名稱、支援信箱；範圍只需 `openid email profile`。測試階段需把使用者加入「測試使用者」，正式使用請發布應用程式。
3. 「憑證 → 建立憑證 → OAuth 用戶端 ID」，類型選「網頁應用程式」。
4. 「已授權的重新導向 URI」填入 `APP_BASE_URL/auth/google/callback`，例如 `https://bento.example.com/auth/google/callback`（必須與 `APP_BASE_URL` 完全一致，含 scheme）。
5. 把 Client ID／Client secret 設為 `GOOGLE_CLIENT_ID`／`GOOGLE_CLIENT_SECRET`。未設定時 `/auth/google/*` 回 503。

### 3.1 不使用 Google：本機帳號（email + 密碼）

測試、內網或尚未申請 Google OAuth 時，可以完全不設定 Google，改用本機帳號登入管理區：

- `GOOGLE_OAUTH_ENABLED`：留空＝自動（`GOOGLE_CLIENT_ID`／`GOOGLE_CLIENT_SECRET` 都有才啟用）；`false`＝即使有憑證也停用；`true` 但缺任一憑證會拒絕啟動。停用時 `/auth/google/*` 回 503，登入頁不顯示 Google 按鈕。
- `PASSWORD_LOGIN_ENABLED`（預設 `true`）：開放 `POST /auth/password-login`；設為 `false` 時該端點回 404。登入嘗試每個 client IP 限速（burst 10，之後約每 5 秒 1 次），失敗一律回相同的 401 `invalid_credentials`。
- 帳號只能用 CLI（或下面的環境變數 bootstrap）建立，網頁上沒有註冊功能。Google 登入不會依 email 自動綁定本機帳號：本機帳號的 email 只在本機帳號間唯一，同 email 的 Google 帳號是另一個開團者。

**建議用 CLI 建立帳號**（密碼不會出現在環境變數或 shell 歷史）。CLI 是同一個 `server` 執行檔，讀取相同的設定／`.env`，連到 `DATABASE_URL` 並先套用 migration：

```sh
# Docker（映像內 server 位於 /usr/local/bin/server；docker exec 會沿用容器的環境變數與使用者 bento）
docker exec -it bento server admin create --email you@example.com --name "你的名字"   # 互動輸入密碼兩次（不顯示）
docker exec -it bento server admin set-password --email you@example.com
docker exec bento server admin list    # email、名稱、登入方式（google/password）、建立時間；不含 hash

# 非互動（密碼由 stdin 讀一行；腳本／CI）
printf '%s\n' "$PASSWORD" | docker exec -i bento server admin create --email you@example.com --name "你的名字" --password-stdin

# 本機開發（預設資料庫 backend/data/dev.db，與 make dev-backend 相同）
make admin EMAIL=you@example.com NAME="你的名字"
```

密碼長度 10～256 字元，以 Argon2id 雜湊儲存。結束碼：`0` 成功、`1` 內部錯誤、`2` 參數錯誤、`3` 輸入不合法（email 格式、密碼太短…）、`4` 帳號已存在、`5` 找不到帳號。`create` 對已存在的 email 會失敗，請改用 `set-password`。

**容器用的環境變數 bootstrap**：設定 `ADMIN_EMAIL` 與 `ADMIN_PASSWORD`（必須同時設定）時，啟動後若資料庫裡沒有任何該 email 的開團者，就會建立本機管理員；已存在則只記一行 info 日誌，**不會**變更既有帳號或覆寫密碼。密碼不會寫進日誌。因為密碼放在環境變數，建議首次啟動後移除 `ADMIN_PASSWORD`，並優先使用上面的 CLI。

## 4. 環境變數

| 變數 | 預設 | 說明 |
|---|---|---|
| `APP_ENV` | `development`（映像內為 `production`） | `production` 會隱藏 `/api/docs`，並拒絕 `DEV_LOGIN`／`AI_DEMO` |
| `PORT` | `3000` | 監聽埠 |
| `APP_BASE_URL` | `http://localhost:3000` | **必設**。公開網址，用於 OAuth 回呼、分享連結；`https://` 開頭時 cookie 才帶 `Secure` |
| `ALLOWED_ORIGINS` | production 為空 | 額外允許送出狀態變更請求的 Origin（逗號分隔，CSRF 檢查）；`APP_BASE_URL` 的 origin 一律允許，通常不需設定 |
| `DATABASE_URL` | `sqlite://data/bento.db`（映像：`sqlite:///data/bento.db`） | SQLite 路徑。使用 Litestream 時必須是 `/data/bento.db` |
| `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` | 空 | Google OAuth（§3） |
| `GOOGLE_OAUTH_ENABLED` | 空（自動） | 空＝有 Google 憑證才啟用；`false`＝停用；`true`＝必須有憑證，否則拒絕啟動（§3.1） |
| `PASSWORD_LOGIN_ENABLED` | `true` | 本機帳號 email + 密碼登入；`false` 時 `/auth/password-login` 回 404 |
| `ADMIN_EMAIL` / `ADMIN_PASSWORD` | 空 | 成對設定；啟動時若無該 email 的帳號就建立本機管理員，不覆寫既有帳號（§3.1，優先用 CLI） |
| `DEV_LOGIN` | `false` | 開發用登入捷徑。**正式環境不可開**（production 下設為 true 會拒絕啟動） |
| `AI_API_KEY` | 空 | 設定後啟用 AI 匯入；未設定則隱藏 AI 匯入按鈕。所有 `AI_PROVIDER` 通用 |
| `AI_BASE_URL` | 空 | API 位址（gateway／測試）。`openai-compatible` 必填；`anthropic` 空＝官方位址 |
| `ANTHROPIC_API_KEY` / `ANTHROPIC_BASE_URL` | 空 | 舊名稱，仍可用，但**只**給 `anthropic`／`anthropic-compatible`；`AI_API_KEY`／`AI_BASE_URL` 有設時以後者為準。`openai-compatible` 不會讀它們（只設了 `ANTHROPIC_API_KEY` 會拒絕啟動，避免誤以為生效） |
| `AI_EXTRA_HEADERS` | 空 | 每個 model 請求額外帶的 header：`name=value,name2=value2`（值內不可有逗號）。值視為機密，不寫入日誌 |
| `AI_SESSION_HEADER` | 空 | 帶「匯入 run id」的 header 名稱（例如 `x-opencode-session`）。每次匯入產生一個新 id，該次所有 model 呼叫共用；需要 session 路由的 gateway 才要設 |
| `AI_MODEL` | `claude-opus-5-5` | 使用的模型；`openai-compatible` 必須明確設定 |
| `AI_MAX_TURNS` | `8` | 每次匯入最多呼叫模型次數 |
| `AI_DAILY_QUOTA_PER_USER` | `30` | 每位開團者每日匯入次數上限 |
| `AI_PROVIDER` | `anthropic` | `anthropic`＝Anthropic 官方 API（Claude 預設值）；`anthropic-compatible`＝相容 Messages API 的 gateway 後面接其他模型（見下方「改用 Anthropic 相容 gateway」）；`openai-compatible`＝相容 OpenAI Chat Completions 的伺服器（DeepSeek、vLLM、LiteLLM、opencode…，見下方「改用 OpenAI 相容伺服器」） |
| `AI_FALLBACKS` | `default`（compatible 為 `off`） | `default`＝被拒絕時允許 API 端 fallback；`off`＝關閉。fallback 是 Anthropic 專屬功能，compatible provider 設 `default` 會拒絕啟動 |
| `AI_STRICT_TOOLS` | `true`（compatible 為 `false`） | 是否送出 `strict: true` 的 tool schema |
| `AI_REASONING` | `low`（compatible 為 `off`） | `low`＝Anthropic 送 `output_config.effort=low` 與 adaptive thinking，`openai-compatible` 送 `reasoning_effort=low`；`off`＝不送任何 reasoning 參數（用伺服器預設） |
| `AI_DEMO` | `false` | 腳本化假資料，僅限開發；production 拒絕啟動 |
| `SESSION_TTL_DAYS` | `14` | 登入 session 有效天數（過半後滑動延長） |
| `TRUSTED_PROXIES` | 空 | 可信反向代理 IP／CIDR（逗號分隔，如 `127.0.0.1,172.16.0.0/12`）。僅來自這些位址的連線才採信 `X-Forwarded-For`；**放在反向代理後面時必須設定**，否則所有人的 IP 都會是代理的 IP，且公開端點的 rate limit 會共用同一桶 |
| `CLIENT_INFO_RETENTION_DAYS` | `90` | 團關閉超過此天數後，該團送出紀錄的 IP／UA／裝置資訊清為 NULL |
| `MAINTENANCE_INTERVAL_HOURS` | `24` | 背景清理工作週期（啟動後 30 秒先跑一次）：刪除過期 session／oauth_states、清除過期 ClientInfo、把卡住超過 10 分鐘的 AI 匯入標為 `failed`（`interrupted`） |
| `PUBLIC_RATE_LIMIT_BURST` / `PUBLIC_RATE_LIMIT_PER_SEC` | `30` / `2` | 公開端點每個 client IP 的 token bucket |
| `DISPLAY_UTC_OFFSET` | `+08:00` | 連結預覽（OG）中截止時間顯示的時區 |
| `RUST_LOG` | `info,tower_http=info` | 日誌等級 |
| `LITESTREAM_BUCKET` | 空 | 設定後啟用 Litestream（§7） |
| `LITESTREAM_PATH` | `bento` | bucket 內前綴 |
| `LITESTREAM_ENDPOINT` | 空 | S3 相容服務 endpoint（AWS S3 留空） |
| `LITESTREAM_REGION` | 空 | region（R2 用 `auto`） |
| `LITESTREAM_ACCESS_KEY_ID` / `LITESTREAM_SECRET_ACCESS_KEY` | 空 | bucket 憑證 |

清除 ClientInfo 的依據是 `group_orders.closed_at`（關閉時寫入；封存只能從已關閉狀態進行且保留 `closed_at`；重新開啟會清空 `closed_at`）。因此「開放中」與「重新開啟」的團不會被清除，已關閉／已封存且關閉時間早於保存期限的團會被清除。訂單內容、備註與品項明細不受影響。

### 改用 Anthropic 相容 gateway（非 Claude 模型）

AI 匯入走 Anthropic Messages API（`POST {AI_BASE_URL}/v1/messages`；舊的 `ANTHROPIC_BASE_URL`／`ANTHROPIC_API_KEY` 仍可用）。若有 gateway 在前面提供同樣的介面、後面接別的模型（例如自架 vLLM／LiteLLM 上的 Qwen），不需改程式，只要設定：

```sh
-e AI_PROVIDER=anthropic-compatible \
-e AI_BASE_URL=https://gateway.example.com \   # 不含 /v1/messages
-e AI_API_KEY=... \
-e AI_MODEL=<gateway 上的模型名稱> \
```

`anthropic-compatible` 只送 Messages API 的基本子集：不帶 server-side fallback 的 beta header 與 `fallbacks` 欄位、不送 `strict` tool schema、不送 `output_config`／`thinking`；gateway 回傳的 `thinking` 區塊（`signature` 為空字串）會在下一輪原樣送回。若 gateway 確定支援，可用 `AI_STRICT_TOOLS=true`、`AI_REASONING=low` 個別開啟。不合法的組合（`anthropic-compatible` + `AI_FALLBACKS=default`）會在啟動時拒絕。

注意事項：
- 模型必須支援**圖片輸入**與 **tool use**；否則匯入會失敗或回傳空提案。
- gateway 若放在 Cloudflare 等 WAF 後面，可能擋掉非瀏覽器的請求（`403 error code: 1010`）；請對後端伺服器的出口 IP 放行。
- **逾時**：`anthropic-compatible` 預設單次 model 呼叫 180 秒（`AI_REQUEST_TIMEOUT_SECS`）、整個匯入 300 秒（`AI_RUN_TIMEOUT_SECS`）；`anthropic` 為 60／90 秒。慢的 gateway（如量化模型）可再調高，需滿足 0 < 單次 ≤ 整體。單次呼叫逾時會顯示「model call timed out after N s」且不重試。
- 匯入是單一同步請求，最久會開著 `AI_RUN_TIMEOUT_SECS` 秒，**反向代理的讀取逾時必須比它長**：Caddy 的 `reverse_proxy` 預設不限；nginx 請設 `proxy_read_timeout 330s;`（預設 60 秒會先回 504）；Cloudflare 代理約 100 秒就會回 524（免費方案無法調高），此時請把 `AI_RUN_TIMEOUT_SECS` 設在 100 秒以下，或讓此服務不經 Cloudflare 代理。
- 錯誤會以 Anthropic 格式（`{"error":{"type":...}}`）回傳；401/403 顯示為「authentication failed」，5xx／連線錯誤視為可重試。
- 換模型後建議先用 `make ai-eval` 跑一次評測（見下）；較小的量化模型可能漏列、改寫標點，開團者的確認畫面仍是最後防線。

### 改用 OpenAI 相容伺服器（`AI_PROVIDER=openai-compatible`）

AI 匯入也可走 OpenAI Chat Completions（`POST {AI_BASE_URL}/v1/chat/completions`）：

```sh
-e AI_PROVIDER=openai-compatible \
-e AI_BASE_URL=https://api.example.com \   # 有沒有結尾的 /v1 都可以
-e AI_API_KEY=... \
-e AI_MODEL=<伺服器上的模型名稱> \          # 必填
-e AI_SESSION_HEADER=x-opencode-session \   # 只有需要 session 路由的 gateway 才設
```

- 圖片以 `image_url` data URL 傳送；模型必須支援圖片輸入與 function calling。
- 預設只送基本子集：不送 `strict` tool schema、不送 reasoning 參數（`AI_STRICT_TOOLS=true`、`AI_REASONING=low`→`reasoning_effort=low` 可個別開啟；`AI_FALLBACKS=default` 會拒絕啟動）。
- 模型回傳的 `reasoning_content`（DeepSeek 的 thinking）會在同一次 run 的後續呼叫原樣送回，其餘 assistant 訊息帶空的 `reasoning_content`，DeepSeek 系列要求如此。
- 逾時預設與 `anthropic-compatible` 相同（180／300 秒）。
- 錯誤：401/403 顯示為「authentication failed」；圖片被伺服器判定無法解碼的 400 視為「圖片無效」；429／5xx／連線錯誤視為可重試。
- opencode Go（`https://opencode.ai/zen/go`）要求每個請求帶 `x-opencode-session`，否則回 `400 MissingSessionID`；請設 `AI_SESSION_HEADER=x-opencode-session`（每次匯入一個 id）。該站位於 Cloudflare 後面，非瀏覽器的 User-Agent（如 Python urllib）會被擋（`403 error code: 1010`），後端使用的 reqwest 不受影響。

**評測**：`make ai-eval`（手動，不屬於 CI／`make check`）會對 `eval/cases/*` 的合成圖片跑與正式環境相同的 agent，輸出每案例的名稱 precision／recall、價格完全一致率、token、耗時，並寫出 `eval/report.json`（`AI_REPORT=路徑` 可改，相對於 `backend/`，建議用絕對路徑）。連線設定與伺服器相同（`AI_API_KEY`／`AI_BASE_URL`／`AI_MODEL`／`AI_PROVIDER`…），另有兩個捷徑：
- `make ai-eval AI_PROVIDER_FILE=provider.json AI_MODEL=deepseek-v4.1-flash AI_SESSION_HEADER=x-opencode-session`：從 JSON 檔（`{"base_url": "...", "api_key": "..."}`，可選 `model`、`provider`、`session_header`）讀入 `AI_BASE_URL`／`AI_API_KEY`，預設 `AI_PROVIDER=openai-compatible`；金鑰不會印出（`provider.json` 已列入 .gitignore）。在 make 命令列給的 `AI_MODEL`／`AI_PROVIDER` 優先於 `.env`。
- 沒有設定 `AI_API_KEY`／`ANTHROPIC_API_KEY` 時，會把 `.env` 或環境中的 `STRATA_API_KEY`／`STRATA_ANTHROPIC_BASE_URL`／`STRATA_MODEL` 對應到 `ANTHROPIC_*`／`AI_MODEL` 並使用 `AI_PROVIDER=anthropic-compatible`。

可加參數：`make ai-eval ARGS="--filter 03 --repeat 3"`。每次執行會呼叫真實模型，請留意費用。

## 5. 執行

```sh
docker run -d --name bento --restart unless-stopped \
  -p 127.0.0.1:3000:3000 \
  -v bento-data:/data \
  -e APP_BASE_URL=https://bento.example.com \
  -e GOOGLE_CLIENT_ID=... -e GOOGLE_CLIENT_SECRET=... \
  -e AI_API_KEY=... \
  -e TRUSTED_PROXIES=172.17.0.1 \
  order-a-bento:latest
```

或使用 `--env-file .env`（參考 `.env.example`；不要提交 `.env`）。Volume 的擁有者須為 uid 10001（具名 volume 會自動處理；bind mount 請先 `chown 10001 <dir>`）。

`TRUSTED_PROXIES` 填反向代理「連到容器時所用的來源位址」。代理在同一台主機、容器走預設 bridge 時是 bridge 閘道（常見 `172.17.0.1`，以 `docker network inspect bridge` 確認）；用 `--network host` 或同一 compose 網路時請填對應位址／網段。

## 6. 反向代理（Caddy 範例）

```caddyfile
bento.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:3000 {
        # Caddy 預設即會設定 X-Forwarded-For/-Proto/-Host；明確寫出以便確認
        header_up X-Forwarded-For {remote_host}
        header_up X-Forwarded-Proto {scheme}
    }
}
```

Caddy 自動申請並續期 HTTPS 憑證。應用程式只信任 `TRUSTED_PROXIES` 內的連線所帶的 `X-Forwarded-For`，其他來源的該標頭會被忽略。

## 7. 備份：Litestream

設定 `LITESTREAM_BUCKET`（及 endpoint／憑證）即啟用，映像內為 Litestream v0.3.13（建置時驗證 sha256）。啟動流程（`deploy/entrypoint.sh`）：

1. 若 `/data/bento.db` 不存在且 replica 有資料 → `litestream restore` 還原；
2. `litestream replicate -exec server`：Litestream 啟動並監督伺服器，持續把 WAL 複寫到 bucket（每 10 秒同步，快照保留 7 天；設定見 `deploy/litestream.yml`）。

未設定 `LITESTREAM_BUCKET` 時直接執行伺服器，不經過 Litestream。

```sh
docker run -d --name bento -v bento-data:/data \
  -e LITESTREAM_BUCKET=my-bucket -e LITESTREAM_PATH=bento \
  -e LITESTREAM_ENDPOINT=https://<account>.r2.cloudflarestorage.com -e LITESTREAM_REGION=auto \
  -e LITESTREAM_ACCESS_KEY_ID=... -e LITESTREAM_SECRET_ACCESS_KEY=... \
  ... order-a-bento:latest
```

**災難還原**：在全新主機（空 volume）用相同的 `LITESTREAM_*` 啟動容器即自動還原。手動還原：

```sh
docker run --rm -v bento-data:/data --env-file litestream.env --entrypoint litestream \
  order-a-bento:latest restore -config /etc/litestream.yml -if-replica-exists /data/bento.db
```

注意事項：

- SQLite 必須為 WAL 模式（本應用預設即是），**只能有一個應用程式實例，且不可有其他程式寫入該 DB 檔**；兩個實例共用同一個 replica 會互相破壞。
- 不要直接複製 `bento.db` 單檔當備份（會漏掉 `-wal`）；要手動備份請用 `sqlite3 bento.db ".backup out.db"`。
- Litestream 為非同步複寫，崩潰時最多損失數秒資料。
- 定期（例如每季）實際演練一次還原。

## 8. 健康檢查

`GET /healthz`（會查詢資料庫）成功回 200 與 `{"status":"ok"...}`。映像已內建 `HEALTHCHECK`（每 30 秒）；`docker ps` 顯示 `healthy`。外部監控請對公開網址呼叫 `/healthz`。

## 9. 升級

```sh
docker build -t order-a-bento:latest .   # 或拉取新映像
docker stop bento && docker rm bento
docker run ...                           # 同樣的 volume 與參數
```

資料庫 migration 於啟動時自動執行（向前相容，無降版 migration）；升級前建議先確認最近一次 Litestream 複寫正常，或手動 `.backup`。停機時間約為容器重啟時間。

## 10. 安全檢查清單

- [ ] `APP_ENV=production`（映像預設值）。
- [ ] `DEV_LOGIN` 未開、`AI_DEMO` 未開（production 下開啟會拒絕啟動）。
- [ ] 對外只經 HTTPS；`APP_BASE_URL` 以 `https://` 開頭，session cookie 才會帶 `Secure`。容器埠只綁 `127.0.0.1`，由反向代理對外。
- [ ] `TRUSTED_PROXIES` 只包含自己的反向代理；不要填 `0.0.0.0/0`。
- [ ] 本機管理員密碼夠長且不重複使用；用 CLI 建立，或首次啟動後移除 `ADMIN_PASSWORD`。不需要密碼登入時設 `PASSWORD_LOGIN_ENABLED=false`。
- [ ] Google 重新導向 URI 與 `APP_BASE_URL` 一致；OAuth 用戶端 secret 不進版控。
- [ ] 使用非 Claude 的模型時 `AI_PROVIDER=anthropic-compatible`／`openai-compatible`（並設 `AI_BASE_URL`、`AI_MODEL`），且已跑過 `make ai-eval`。
- [ ] `AI_API_KEY`、Litestream 憑證以 env／secret 管理；Litestream bucket 設為私有並使用最小權限金鑰。
- [ ] 已啟用並演練備份還原；`CLIENT_INFO_RETENTION_DAYS` 符合隱私承諾（預設 90 天）。
- [ ] 以單一實例執行，`/data` 為持久 volume。
