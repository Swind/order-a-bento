# 實作計畫（order-a-bento）

> 依據：`docs/design.md` v0.3
> 執行方式：每個 task 由一個 subagent（Sonnet）實作；主 agent 撰寫任務說明、驗收、要求修正。驗收通過後以 `git add` 暫存作為檢查點（commit 由使用者決定）。

---

## 共通完成定義（Definition of Done，每個 task 都要符合）

後端（`backend/`）
- `cargo fmt --all --check` 通過
- `cargo clippy --workspace --all-targets -- -D warnings` 通過
- `cargo test --workspace` 通過
- 依賴方向符合 design §5.2：`domain` 不引用 sqlx / axum / rig / utoipa
- 有改 API 時重新產生 `frontend/openapi.json`（`cargo run -p server --bin export_openapi > ../frontend/openapi.json`）

前端（`frontend/`）
- `npm run gen:api`、`npm run typecheck`、`npm test -- --run`、`npm run build` 全部通過
- 手機寬度（390px）版面不破

共通
- 不留未實作的 stub 或 `todo!()`，除非任務說明明確允許並列在回報中
- 與 design 不一致之處必須在回報中列出理由
- 回報內容：完成項目、新增／修改的檔案、執行過的指令與結果、偏離設計之處、已知限制

---

## Phase 0：骨架

| Task | 內容 | 依賴 | 可平行 |
|------|------|------|--------|
| **T0.1 後端骨架** | Cargo workspace 五個 crate；config（env）；SQLite pool（WAL、busy_timeout、foreign_keys；寫入 pool 1 連線）；完整 migration（design §6）；axum server + `/healthz`；ProblemDetails 錯誤型別；utoipa `OpenApiRouter` + `/api/openapi.json` + `/api/docs`（Scalar）；`export_openapi` bin；tracing | — | 與 T0.2 平行 |
| **T0.2 前端骨架** | Vite + React + TS；Tailwind + shadcn/ui（button、card、input、label、dialog、sheet、tabs、switch、table、badge、sonner、skeleton）；React Router；TanStack Query；`openapi-typescript` + `openapi-fetch` client（`gen:api` script）；Vitest；dev proxy `/api` → `localhost:3000`；佔位頁面 | — | 與 T0.1 平行 |
| **T0.3 整合** | server 以 `rust-embed` 提供 SPA（未知路徑 fallback `index.html`，`/api/*` 不 fallback）；根目錄 `justfile` 或 `Makefile`（dev / test / gen-api / build）；`.github/workflows/ci.yml`（design §11）；多階段 `Dockerfile`；`.env.example` | T0.1、T0.2 | — |

**驗收**：`cargo run -p server` 後 `curl /healthz`、`/api/openapi.json` 正常；前端 build 後由 server 提供首頁；CI 檔案的指令在本機逐一執行皆通過。

---

## Phase 1：登入 + 主資料

| Task | 內容 | 依賴 |
|------|------|------|
| **T1.1 Domain：共通值物件 + Directory** | `Money`、`NameKey`（NFKC、trim、空白合併、小寫）、`PersonName`、`ItemName`、`Id<T>`、`DomainError`、`Clock`；`Roster` / `Vendor` / `MenuItem` 聚合與 `upsert_members`、`update_info`、`upsert_menu_items`、`UpsertReport`（created / updated / unchanged、空值不覆蓋）；完整 unit test（`NameKey` 加 proptest） | T0.1 |
| **T1.2 Auth** | Google OIDC（`openidconnect`：PKCE、state、nonce、驗 id_token、`email_verified`）；`oauth_states`、`sessions`（存 SHA-256）；`AuthenticatedOrganizer` extractor；`/auth/google/login`、`/callback`、`/logout`、`/me`；修改類請求 `Origin` 檢查；**開發用登入**：`DEV_LOGIN=true` 時提供 `POST /auth/dev-login {email}`（production 必須關閉，啟動時若 `APP_ENV=production` 且 DEV_LOGIN 開啟則拒絕啟動） | T0.1（可與 T1.1 平行） |
| **T1.3 Directory 應用層 + API** | use cases、sqlite repos（`ON CONFLICT(… name_key) DO UPDATE`）、design §7.2 Directory 全部端點、DTO + utoipa、`#[sqlx::test]` 整合測試、API 測試；重新產生 openapi.json | T1.1、T1.2 |
| **T1.4 前端：登入 + 主資料頁** | 登入頁（Google 按鈕；dev 模式另有 email 登入）、需登入的 layout、名單列表／編輯（貼上多行文字批次 upsert、顯示 UpsertReport）、店家列表／編輯（菜單表格、封存、刪除） | T1.3 |

**驗收**：dev-login 後可完整 CRUD；同名 upsert 的行為與 design §4.3(c) 一致（含全形／大小寫／空值不覆蓋）；未登入存取回 401；跨使用者存取回 404。

---

## Phase 2：開團 + 下單

| Task | 內容 | 依賴 |
|------|------|------|
| **T2.1 Domain：Ordering** | `ShareToken`、`GroupOrder`（快照、狀態機、截止判斷）、`Participant`、`OfferedItem`、`OrderSubmission` / `SubmissionLine`（空 = 取消、數量範圍、售完、最多 30 行）、`ClientInfo`、`Settlement`；unit test | T1.1 |
| **T2.2 開團 + 公開下單 API** | `POST /group-orders`（建立快照）、`GET /group-orders/{id}`；公開端點（§7.2 Public）；`ClientInfoExtractor`（`TRUSTED_PROXIES`、UA 解析含 LINE）；idempotency；`INSERT … WHERE EXISTS(status='open')`；`effective_submissions` view；併發測試（同時送出、同人兩次、關團競態） | T2.1、T1.3 |
| **T2.3 前端：開團精靈 + 下單頁** | 開團精靈、完成頁（複製連結、分享到 LINE）；`/g/:token` 手機下單頁（design §8.4 全部行為、`deviceInfo.ts`、隱私告知） | T2.2 |
| **T2.4 OG meta** | `/g/{token}` 回傳注入 `og:title`、`og:description`、`robots noindex` 的 index.html（HTML escape）；無效 token 仍回 SPA 但不洩漏資訊 | T2.2、T0.3 |

**驗收**：手機 viewport 實際走一遍下單、重新送出、取消；統計只算最新一筆；重送同 idempotencyKey 不產生新紀錄；關團後送出被拒。

---

## Phase 3：管理統計

| Task | 內容 | 依賴 |
|------|------|------|
| **T3.1 管理端 API** | 列表、PATCH（close / reopen / archive / 標題 / 截止）、rotate token、新增臨時團員、售完；summary、participants（應付、settlement、改單次數、可疑裝置）、某人送出歷史、開團者代為送出、settlement PUT | T2.2 |
| **T3.2 前端：管理頁** | 開團紀錄列表、團詳情 Tabs（統計 / 明細 / 領取＆繳費）、送出歷史抽屜（§8.6）、尚差／應退標籤、篩選 | T3.1 |

**驗收**：design §7.3 summary 範例的欄位齊全；繳費後重新送出會出現「尚差」；同裝置多名字有 ⚠。

---

## Phase 4：AI 匯入

| Task | 內容 | 依賴 |
|------|------|------|
| **T4.0 Spike：rig** | 在 `backend/spikes/rig-spike`（不進 workspace 成員或以 feature 隔離）用 Swind/rig（鎖定 git rev）建立：Anthropic client、1 個自訂 `Tool`、`attach_files` 圖片、`default_max_turns`；確認能編譯；若有 `ANTHROPIC_API_KEY` 則實跑一次；查明 refusal 的呈現方式與 usage 取得方式；寫 `docs/spikes/rig.md` | T0.1 |
| **T4.1 Domain + Application** | `ImportRun`、`Proposal`、`ProposedChange`；`application::ai_import::tools`（6 個 tools 的業務邏輯、驗證錯誤訊息、重複 propose 取代）；`ApplyImportRun`（重新驗證、重算差異、單一 transaction）；`ImportAgent` port + `FakeImportAgent`；測試 | T1.3、T4.0 |
| **T4.2 Infrastructure + API** | rig `Tool` 包裝、`RigImportAgent`、`prompts/import_agent.md`（design §9.4，常數替換）、`/ai/import-runs` 四個端點、圖片驗證（magic bytes、5MB）、逾時 90 秒、速率限制與每日額度 | T4.1 |
| **T4.3 前端：匯入確認畫面** | 上傳（壓縮）、指示輸入、等待動畫、design §8.7 確認畫面（勾選、編輯、issue 阻擋套用）、套用後導回 | T4.2 |
| **T4.4 Eval 腳本** | `tools/ai-eval`：讀取 `eval/cases/*/{image, expected.json}`，呼叫 `RigImportAgent`，逐欄比對並輸出報表；手動觸發 | T4.2 |

**驗收**：用 Fake agent 跑完整 UI 流程；有 API key 時以 2–3 張真實照片人工檢查；照片含指令文字時不影響結果。

---

## Phase 5：上線準備

| Task | 內容 | 依賴 |
|------|------|------|
| **T5.1 背景工作 + 部署** | 每日清除過期 session / oauth_states / ClientInfo；Litestream 設定範例；Dockerfile 定稿；部署說明 `docs/deploy.md` | Phase 3 |
| **T5.2 E2E** | Playwright（iPhone viewport）：dev-login → 建名單／店家 → 開團 → 兩位參與者下單 → 一位重新送出 → 管理端統計與 checklist | Phase 3 |
| **T5.3 打磨** | 截止倒數、空狀態、錯誤提示、a11y（label、focus）、LINE 內建瀏覽器實機檢查清單；T1.4 驗收發現：toast 在手機頂端蓋住標題列（改到不遮擋的位置）、`Switch` 的軌道顏色太淡看不出開關狀態；JS bundle 768 kB 需 code splitting；T2.3 驗收發現：下單頁底部隱私告知被 sticky bar 遮住（頁面需預留 bar 高度）；關團後唯讀狀態已於 T3.2 驗收時以瀏覽器確認；送出歷史的 lines 補回 `itemId`（目前代為修改以品名對回品項，依賴同團品名唯一） | Phase 3 |

---

## Phase 6：部署彈性（使用者回饋）

| Task | 內容 | 依賴 |
|------|------|------|
| **T6.1 登入方式可設定 + 本機管理員** | `GOOGLE_OAUTH_ENABLED`（未設＝有憑證才啟用）、`PASSWORD_LOGIN_ENABLED`；`organizers.google_sub` 改可空、加 `password_hash`（Argon2id）；`POST /auth/password-login`（限流、統一錯誤）；CLI `server admin create/set-password/list` 不需 Web UI 建帳號，另可用 `ADMIN_EMAIL`/`ADMIN_PASSWORD` 於容器啟動時建立；登入頁只顯示啟用的方式；E2E 以 CLI 建帳號後密碼登入 | Phase 1 |
| **T6.2 相容 Anthropic API 的閘道** | `AI_PROVIDER` 等設定，讓 AI 匯入可指向非 Claude 的相容閘道（如 Strata）；預設行為不變 | T4.2 |

---

## 進度

| Task | 狀態 | 備註 |
|------|------|------|
| T0.1 | ✅ 驗收通過 | |
| T0.2 | ✅ 驗收通過 | 3 個小問題併入 T0.3 修正 |
| T0.3 | ✅ 驗收通過 | release 版 curl 驗證、docker build 通過 |
| T1.1 | ✅ 驗收通過 | 18 個 domain 測試（含 proptest） |
| T1.2 | ✅ 驗收通過 | 實測 dev-login、/me、登出、Origin 403、production 拒絕啟動 |
| T1.3 | ✅ 驗收通過 | 93 個後端測試；實測同名 upsert、跨帳號 404、409 existingId、merge-patch |
| T1.4 | ✅ 驗收通過 | headless Chrome 390px 實測：登入導向、名單／店家建立、批次 upsert、409 連結、無橫向溢出 |
| T2.1 | ✅ 驗收通過 | 112 個後端測試；狀態機、送出驗證、token 編碼抽查 |
| T2.2 | ✅ 驗收通過 | 144 個後端測試；實測 XFF 偽造被忽略、LINE UA 解析、idempotency、log 無 token、429 |
| T2.3 | ✅ 驗收通過 | 55 個前端測試；headless Chrome 390px 走完：精靈開團、A 下單→重開記住→改單、B 冒名確認→改選、全形搜尋、A 取消；DB 確認 UA/裝置 ID 正確 |
| T3.1 | ✅ 驗收通過 | 161 個後端測試；實測統計只算有效訂單、事後改價不影響、尚差計算、可疑裝置、關團 409、輪替連結 |
| T3.2 | ✅ 驗收通過 | 77 個前端測試；headless Chrome 390px：列表、統計、繳費 Switch、尚差 $100、明細改單/同裝置標籤、歷史 sheet、關團後下單頁唯讀 |
| T4.0 | ✅ 驗收通過 | mock Anthropic API 跑 10 個情境；結論已寫回 design §9（strict tools、錯誤回饋、refusal hook、重試／逾時、fallback、rig rev） |
| T4.1 | ✅ 驗收通過 | 191 個後端測試；prompt 與 design §9.4 逐字一致；tools 皆以 organizer_id 限定；apply 單一 transaction 失敗全回滾 |
| T4.2 | ✅ 驗收通過 | 217 個後端測試；mock Anthropic 驗證請求格式；示範模式實測：上傳驗證、提案（覆蓋價格／缺價）、apply 422 指出欄位、補價後套用、重複套用 409、log 無圖片 |
| T4.3 | ✅ 驗收通過 | 106 個前端測試；headless Chrome（示範模式）：從店家頁上傳 → 確認畫面缺價時套用停用 → 補價 → 套用回店家頁；實測 2400×1600 PNG 782KB 壓成 1568×1045 JPEG 29KB |
| T5.1 | ✅ 驗收通過 | 218 個後端測試；維護工作（session、ClientInfo 90 天、卡住的 AI run）；Litestream v0.3.13；docker 實跑 non-root、production+DEV_LOGIN 拒絕啟動（真實 S3 備份還原未測）。註：Litestream 已於 2026-10-18 依 owner 決定移除，改為手動備份 volume |
| T5.3a | ✅ 驗收通過 | 111 個前端測試；bundle 865 kB → 最大 chunk 262 kB；瀏覽器實測 toast 在標題列下方、下單頁隱私告知露出於 sticky bar 上方 |
| T5.2 | ✅ 驗收通過 | 5 個 spec、mobile+desktop 共 7 個測試，連跑 3 次全過（約 5 秒）；CI 加 e2e job（workflow 本身尚未在 GitHub 執行過） |
| T4.4 | ✅ 驗收通過 | `make ai-eval`；7 組合成測試圖（名單、傳單、雜亂傳單、提示注入、非菜單、既有名單）對 Strata（qwen3.8-flash）7/7 PASS，平均 15 秒、約 1 萬 input tokens；prompt 補 3 條規則（核對總人數、note 不放廣告、分類原樣抄）。真實手機照片與 Claude 尚未跑 |
| T5.3b | ✅ 驗收通過 | 歷史與明細 API 加 itemId；代為修改改以 itemId 對應 |
| T2.4 | ✅ 驗收通過 | 151 個後端測試；XSS 跳脫測試、未知與格式錯誤 token 輸出相同；完整流程待與 T2.3 一起以瀏覽器驗 |
| T6.1 | ✅ 驗收通過 | 241 個後端、116 個前端測試、E2E 8 個；實測 CLI 結束碼（3 無效／4 已存在／5 找不到）、ADMIN_* 啟動建帳號、Google 關閉時 503、錯誤統一 401、第 7 次起 429、log 無密碼；0002 migration 以含資料的舊 DB 測過 |
| T6.2 | ✅ 驗收通過 | `AI_PROVIDER=anthropic-compatible`、`AI_STRICT_TOOLS`、`AI_REASONING`；Strata 實測接受所有 Claude 參數，相容模式預設送最小子集；252 個後端測試 |
| T4.5 | ✅ 驗收通過 | 使用者提供 3 張真實照片（劃掉品項的相簿截圖、手拿傳單、用過的送貨單，訂購人電話已塗掉）加入 eval；AI 逾時可設定（`AI_REQUEST_TIMEOUT_SECS`／`AI_RUN_TIMEOUT_SECS`，相容閘道預設 180／300 秒）；system prompt 與送給模型的文字改英文，給使用者看的仍為繁中；Strata 全套 10/10 PASS，送貨單剩鯖→鱈誤讀等模型本身的 OCR 錯誤；254 個後端測試 |
| T6.3 | ✅ 驗收通過 | `AI_PROVIDER=openai-compatible`（rig Chat Completions）、`AI_API_KEY`/`AI_BASE_URL`、`AI_EXTRA_HEADERS`、每次匯入一組 `AI_SESSION_HEADER`；opencode Go `deepseek-v4.1-flash` 兩輪 17/20 PASS、平均 13 秒（Qwen 24.5 秒），送貨單全對，偶有讀錯字與收進劃掉品項；265 個後端測試 |
| T6.4 | ✅ 驗收通過 | `AI_PROVIDER=openai-responses`（rig Responses client、`store:false` 回傳 reasoning）；opencode Go 7 張 core eval 各跑一次：Haiku 5.5 6/7 11.8 秒、DeepSeek v4.1 flash 7/7（收進劃掉名字）18.4 秒、GLM-5.3-flash 6/7 46.8 秒、GPT 6 Luna 6/7 16.6 秒；建議 Haiku；275 個後端測試 |
| T6.5 | ✅ 驗收通過 | 匯入必選 `mode`（roster／vendor，缺少回 422）；prompt 拆成名單版與菜單版（共用片段組裝，design §9.4 有一致性測試），每次只給該模式的 3 個 tool；劃掉規則限定「線劃過品名本身」、摘要強制繁中、eval 加 lang 欄；Haiku 7 張 6/7、全部繁中；281 個後端測試、E2E 8 個 |
| T7.1 | ✅ 驗收通過 | CI `docker` job 推雙架構映像到 GHCR（`latest`／`sha-`／semver）；arm64 在 x86 runner 交叉編譯 Rust（不跑 QEMU），Dockerfile 用 cargo-chef（pin 0.1.78，cook 與最終 build 皆 `--locked`）把依賴編譯做成一般 image layer，可被 GHA layer cache 保存（cache mount 不會被匯出）；本機驗 arm64 binary 為 aarch64、amd64 映像 healthz／admin list 正常；workflow 尚未在 GitHub 執行過、arm64 完整映像（runtime stage 需 QEMU）未本機建置 |
| T7.2 | ✅ 驗收通過 | 版本單一來源 `backend/Cargo.toml`（`scripts/version.sh get/check/bump`、`make version`／`make bump`，CI 與 `make check` 檢查前端版本一致）；`BUILD_NUMBER` 編進執行檔（`api::version`），`server --version` 與 `/healthz` 的 `version` 為 `X.Y.Z.<build>`；Dockerfile 在 cook 之後才宣告 `ARG BUILD_NUMBER`（本機驗證 cook 層仍 CACHED）；CI 映像 tag 改為 `X.Y.Z.<run_number>`／`X.Y.Z`／`latest`／`sha-`，移除 `v*` tag 觸發；workflow 尚未在 GitHub 執行過，`org.opencontainers.image.version` label 由 metadata-action 產生、本機未驗 |
