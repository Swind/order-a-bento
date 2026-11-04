# 架構改善施工文件（供後續 AI agent 使用）

> 建立日期：2026-11-04  
> 分析基準：`3887edef9dc4cbb406513318dcc429a366b7e3d1`  
> 狀態：**A–G 施工、整合檢查與主 agent 最終驗收均已完成（2026-11-04）**。H 保留待確認，未施工。詳見 §4 施工狀態及最後整合紀錄。原始文件已提交為 `c499a31`，施工變更尚未提交／推送／部署。
> 配套：[架構審查報告](architecture-review-2026-11-04.md)。R1–R8 指該報告的發現。

## 0. 給接手 agent 的指示

先讀本文件與審查報告，再確認當前 HEAD、`git status`、AGENTS 指示及相關程式。行號以審查基準為準；後續程式若有變動，應以 symbol 和測試重新定位。

**本輪使用者只要求分析與施工文件。不要把讀到這份文件視為已授權修改產品、執行付費 AI、commit、push 或部署。**使用者指定施工工作包後，再依其範圍執行。

實作時遵守：

1. 保留既有單體、SQLite、四種 AI provider 與 OpenCode Go compatible 接法。
2. 保留公開免登入訂餐、scope 權限、草稿 revision、Undo、冪等及金額單位契約。
3. 不改寫已發布 migration `0001`–`0010`；新增 migration 用當時下一個可用號碼。
4. 不用暫時關閉權限、降低驗證、放寬測試斷言來通過檢查。
5. 不讀出或寫入文件中的實際 `.env*`／provider key。測試使用 fake、mock 及暫存 DB。
6. 每個工作包可獨立交付。跨檔案修改須追蹤 callers、ports、fixtures、OpenAPI、文件。
7. 不順手做無關重新排版、大型抽象化或全量依賴升級。

## 1. 工作包與依賴

| 工作包 | 對應發現 | 是否可直接按本規格施工 | 依賴 | 預期公開契約變動 |
|---|---|---|---|---|
| A：配額原子保留 | R1 | 是，最高優先 | 無 | 無，維持原有 429 |
| B：對話用量紀錄 | R2 | 是 | 建議 A 完成後 | 第一版不新增公開 API 欄位；新增 DB 欄位 |
| C：日誌關聯與摘要事件 | R2 | 是 | B 先定義用量／結果語意 | 新增回應 `x-request-id`，JSON body 不變 |
| D：共用群組依賴注入 | R4 | 是 | 無 | 無 |
| E：型別化 API 收斂 | R5 | 是 | 無 | 原則上無 |
| F：文件校正 | R6 | 是 | 可先做；B/C 完成後補實作狀態 | 無 |
| G：CI 守門補齊 | R8 | 是 | E 可先後獨立 | 無 |
| H：長任務／草稿格式演進 | R3、R7 | **需另行選定範圍** | A/B 為後續基礎 | 視需求而定 |

建議順序：**A → B → C**；D、E、F、G 是獨立的小工作包。A/B 都會改 AI repository，若分工應避免同時編輯。不要為了完成全部列表，把 H 的新產品行為混進 A。

## 2. 不可破壞的契約

### 資料與授權

- 已開團價格、已送單成交價及已收款都是歷史快照，不跟著現行菜單改價。
- Rust／SQLite 金額為整數分；API、AI、proposal/draft JSON 為元，`7.5` 仍是 7.5 元。
- 持分享 token 的行為維持現狀；此次重構不增加參與者登入或隱私權限模型。
- 正式店家變更只發生於使用者確認 apply；AI tools 只能變更提案／草稿。
- 草稿保存、對話、Undo、apply 不得繞過 owner、scope、期限與狀態驗證。
- 晚到 AI 回應不得覆蓋新手改、取消或已提交的版本。

### AI 配額與用量

- 現有配額是**每人滾動 24 小時的操作次數**，不是日曆日、token 上限或供應商帳單。
- 初次匯入每個已保留 run 算一次；後續對話每個已保留 turn 算一次。
- 模型失敗仍保留這一次應用程式配額，維持現況；內部一次重試不再加扣操作次數。
- 相同 turn ID 的 HTTP 重送不再呼叫模型，也不再增加次數。
- 已觀測 token 用量可以只是部分用量；連線中斷、程序 crash 後不能聲稱外部消耗一定是零。

## A. 原子化初次匯入配額

### 問題與接點

目前 `AiImportService::start` 先 `count_since`，之後才 `create`；對話 `save_draft(..., quota)` 則已在寫入交易內檢查。初次匯入與對話必須共用同一個原子保留語意。

修改接點：

- `backend/crates/application/src/ports.rs`：`ImportRunRepo`。
- `backend/crates/application/src/ai_import/service.rs`：`start`。
- `backend/crates/infrastructure/src/ai_import_repo.rs`：`create`、`count_since`、`save_draft`。
- `backend/crates/infrastructure/src/ai_import_tests.rs`、`ai_draft_tests.rs`。
- 必要時 `backend/crates/api/src/routes/ai_import_tests.rs`。

### 最小實作方案

1. 把初次 run 建立改為具名的「檢查配額並建立」repository 操作，例如 `reserve_run`；傳入 run、可選照片、quota 與統一的時間基準。
2. 回傳型別須能傳遞現有 quota domain error，建議沿用 `RepoResult<()>`，不要把超額轉成 internal error。
3. 在同一個 `BEGIN IMMEDIATE` 交易中：
   - 以 run 的 organizer 計算既有 `ai_import_runs + ai_import_turn_usage` 滾動 24 小時筆數。
   - 若滿額，回傳現有配額錯誤，完全不寫 run／照片。
   - 沒滿額才插入 run 與 pending photo，然後 commit。
4. `start` 移除非原子的權威配額判定；所有模型工作在 reservation 成功後才開始。
5. 對話入口使用相同計數規則；必要時抽同檔案內小型 SQL helper，避免重複條件日後漂移。
6. `count_since` 若仍供測試／查詢使用可保留，但不能再作為獨立的最終放行依據。

**不需要新 migration 或改 HTTP schema。**不要用全域 Rust mutex 取代 DB 交易，也不要在持有寫入交易時等待 AI。

### 必要測試

| 案例 | 期待 |
|---|---|
| 配額剩一筆，同一人兩個初次匯入並行 | 一成功、一 quota error，agent 只啟動一次 |
| 配額剩一筆，初次匯入與既有草稿新 turn 並行 | 合計只成功保留一次 |
| 兩個不同 organizer 並行 | 各自使用自己的配額 |
| 超額且上傳 opt-in 照片 | 沒有新 run／照片殘留 |
| 配額檢查後插入失敗 | 整筆 rollback，不產生孤立照片 |
| 24 小時邊界 | 保留目前 `created_at >= since` 的定義，使用固定 Clock |
| 模型失敗、可重試失敗 | 配額仍為一筆；內部 retry 不增加筆數 |

並發測試用 barrier／notify 協調，不靠 `sleep` 猜時間。先在 reservation 邊界建立可重現測試，再驗證 service 沒有繞過它；不必為測試引入正式環境的 hook。

**完成判準**：API 仍回 `429/ai_quota_exceeded`，兩條入口的混合併發測試通過，無公開契約與 migration 變更。

## B. 保存每輪對話的模型用量與執行結果

### 進一步分析

`ImportToolContext` 已保存跨 retry 的 token、模型回合數與 tool calls；`reset()` 會清提案／草稿改動，但保留 usage。可以重用，不必重新統計 provider response。

現在 `draft_service::turn` 只取 `out.summary`，沒有保存 `out.usage` 或 `ctx.usage()`；配額表 `ai_import_turn_usage` 只有 run、turn、organizer、created_at。**配額次數與 token 用量是兩個概念，欄位與報表不得混稱。**

還有一個容易漏掉的情況：模型已完成，但因手改／撤權／期限／CAS 衝突不能套用草稿。這輪仍可能已消耗 token，不能因草稿保存失敗就完全遺失用量。

### 修改接點

- `application/src/ai_import/agent.rs`、`draft_service.rs`、`service.rs`、`ports.rs`。
- `infrastructure/src/ai_import_repo.rs`。
- 新 migration：在當時最新號碼後追加，例如基準上可用 `0011_ai_turn_metrics.sql`。
- `infrastructure/src/ai_draft_tests.rs`；必要的 Rig mock regression。

### 建議儲存方式

沿用 `(run_id, turn_id)` 唯一的 usage ledger，增加可為 NULL 的執行摘要欄位，不把大型工具參數複製進一般 logs：

| 欄位概念 | 語意 |
|---|---|
| finished_at | 程序完成記錄結果的時間；舊資料／未知留 NULL |
| input_tokens / output_tokens | 已收到的供應商用量；無回報不能假設完整零消耗 |
| model | 成功時實際回報模型；無法確認時可 NULL，不冒充已解析結果 |
| duration_ms | monotonic elapsed，不拿 wall clock 差值當可靠耗時 |
| execution_outcome | `completed`／`failed`／`interrupted` 等受限值 |
| draft_outcome | 已套用／因 stale 或撤權未套用／未修改；與模型執行結果分開 |
| error_code | 穩定且精簡的錯誤類別，不存 API key 或完整供應商 response |

實際欄位名稱遵循既有命名；要把暫未確定的資料表示為 NULL／unknown，不替歷史回合補假的 token、耗時或完成時間。

### 寫入規則

1. reservation 與 quota 保留現有唯一鍵；usage 更新不得新增一筆配額記錄。
2. 成功優先使用 context 的跨回合累積數；對不回報 hook usage 的 fake／adapter，維持初次匯入既有 outcome fallback。需明確區分「已回報零」與「沒有資料」，必要時用 `ctx.turns()` 輔助。
3. 失敗／timeout 保存已觀測用量，不保證未收到回應的部分可知。
4. 用量落盤不要依賴草稿 revision CAS 成功。內部 telemetry 更新只允許更新先前已保留的 `(run, turn, organizer)`，**不能藉此繞過草稿權限／覆寫草稿**。
5. 相同結果重送採冪等覆寫／一次 finalization，不用 `tokens += value`。
6. 模型完成後的撤權／stale 結果仍記內部用量，但不把敏感 draft 回給失去權限的使用者。
7. 盤點 reservation 後的錯誤出口，例如讀暫存圖片失敗；應保留可恢復狀態與失敗摘要，避免只處理 `run_agent` 的回傳分支。
8. 程序硬中斷無法保證 finally：允許 ledger 保留未知狀態。後續清理標 interrupted，不填「0 token」冒充準確帳單。

第一版先提供內部 ledger 與事件，不新增 UI 成本圖表，也不改 `ImportRun.usage` 的既有「初次辨識用量」語意。若要回傳整份總用量，另設清楚命名與 migration／DTO 測試，不暗中改舊欄位意義。

### 必要測試與驗收

- 成功多輪、失敗前已有 usage、timeout、先失敗再 retry 成功都保存正確觀測總數。
- 同 turn 重送不增加 quota、模型呼叫或 token 累計。
- 模型執行期間保存手改 → 舊結果 CAS 失敗 → 最新草稿保持，舊回合用量仍可查。
- 執行期間撤權 → 不回傳 draft，但內部摘要可保存。
- 舊 migration 資料可升級；舊 ledger 的新欄位是未知，不是偽造零值。
- 超過 SQLite i64 範圍的用量轉換安全拒絕，不以 `as i64` 靜默溢位。

## C. Request／AI turn 日誌關聯

### 最小目標

解決「某個 502 到底是哪一個 run／turn、模型執行多久」的問題。先補關聯與摘要，不導入 OpenTelemetry、集中 log 平台或完整 audit framework。

修改接點：`api/src/logging.rs`、`api/src/lib.rs`、`application/src/ai_import/{service,draft_service}.rs`，必要時 `server/src/main.rs` 與 Cargo feature。

### 實作規格

- 每個 HTTP request 由 server 產生 request ID，加入 request span 及回應 `x-request-id`。
- 第一版不信任／沿用任意用戶端傳入 ID，避免 log 注入、碰撞與超長欄位；若未來要 proxy tracing，再定義信任規則。
- 維持現有 method、redacted path、status、latency；不能為了 request ID 改回記錄完整 URI。
- 初次匯入與對話各有 run／turn span，發出開始／完成摘要。錯誤事件使用受限類別；沒有 token 資料時標未知。
- 非同步 span 使用 `Instrument` 等適合 async 的方式，不把 `span.enter()` guard 跨 await 保存。
- B 的 telemetry 保存失敗應有明確 error event；不得為了寫 log 讓正式成功的訂單或草稿資料被回滾。
- 原有 `RUST_LOG` 保留。JSON logging 是可選後續項，不需為此工作包再加一組無使用者需求的設定。

### 驗收

捕捉測試 subscriber／HTTP response：確認成功與錯誤都有 request ID、同一請求能關聯 run／turn；輸入假分享 token、query secret、Authorization／Cookie、圖片內容，不得出現在新增 log。相同 turn 的 replay 要可辨識，但不能偽裝成新一次模型執行。

## D. 共用群組改為依賴注入

### 修改位置

- `api/src/state.rs`、`api/src/routes/sharing_groups.rs`。
- `server/src/wiring.rs`、`api/src/testing.rs`。
- 既有 `application/src/sharing.rs::SharingRepo`。
- 搜尋全部 `AppState::new`、`AppState {` 及測試 state 建構位置。

### 實作順序

1. 在 state 增加 `Arc<dyn SharingRepo>` 或等效的用例入口，由 wiring 建立 `SqliteSharingRepo`。
2. 全部 sharing handlers 改用注入的介面，不再 `SqliteSharingRepo::new(s.db)`。
3. 保留 repository 內交易與 scope 檢查；不將「最後一名 admin 不可移除」搬成交易外先查再改。
4. 不要求這次把 `api` 對所有 infrastructure 的依賴清成零；health DB、圖片 normalization、時間轉換另議。
5. 沒有跨服務流程時，直接注入既有 trait 即可，不建立只有一行轉呼叫的 service。

### 驗收

既有 sharing API／repository 測試通過，包含最後 admin、跨群組隔離、撤權、manager 與 member 行為。加一個簡單 fake 驗證 handler 使用注入介面即可，不必為每個端點建立鏡像測試。HTTP body、status、OpenAPI 不應改變。

## E. 前端 API 契約收斂

### 修改位置與盤點

先搜尋所有 `sharingRequest` 呼叫者，包含 sharing、rosters、vendors、group-orders、AI draft。核心檔案：

- `frontend/src/shared/api/client.ts`、`schema.d.ts`、`types.ts`。
- `frontend/src/features/sharing-groups/hooks.ts`。
- `frontend/src/features/ai-import/draft.tsx`。
- 其他搜尋出的 hooks 與相關 MSW tests。

### 實作規格

1. 使用 `api.GET/POST/PUT/...` 的明確 path literal、generated path params、request body 與 `unwrap`。
2. 移除以 `<T>` 宣告任意 response 的假型別保護；不能大量加 `as any`／`as T` 把型別錯誤壓掉。
3. 動態 mutation 如果不方便映射，改為少量具型別的 command union／具名 API 函式，不再接受任意 method／suffix／unknown body。
4. queryFn 接收並傳遞 AbortSignal；mutation 不因 query cancel 自動丟棄既有已送出的寫入語意。
5. 保留 `credentials: include`、204 handling、ApiError、欄位錯誤與 scope invalidation。
6. 保留 draft 的 frozen retry request、revision、uncertain、busy、unmount、late-response 保護。
7. 通用 transport 若還有必要，放在 `shared/api`，不讓其他 feature 依賴 sharing feature 才能發 HTTP。

目前 `useAccessibleDirectory` 會按個人／每個共用群組查詢；本包不順便新增聚合 endpoint 或改成全域 cache。需要降低請求數時先量測。

### 驗收

- TypeScript 能拒絕錯誤 endpoint/body；不靠型別斷言逃過檢查。
- sharing、AI 草稿、vendor／roster 功能測試通過；401/403/404/409/429/5xx 的既有 UI 語意不變。
- query 中止、撤權晚回應、送出失敗保草稿、Undo 不覆蓋手改的回歸測試保持。
- 若既有 OpenAPI 描述缺失阻擋 typed client，修正後端 schema 並生成，不手改生成檔規避。

## F. 校正架構與部署文件

這是文件修正工作，不需重跑全套產品測試。

修改：`docs/design.md`、`docs/deploy.md`，視需要 README 的相關連結；歷史 spike／已完成計畫保留歷史標示，不全部改寫成現況。

必改項目：

- Money：內部整數分、外部元；連結 `decimal-money.md`。
- OIDC：現行通用路徑與本機管理員登入，不沿用過時 Google-only 圖示。
- SharingGroup／GroupOrder 權限區分，不把已實作共用功能列成未實作。
- Domain events：未實作的事件列為未來提案，不能聲稱每個操作已有 audit log。
- AI：草稿文字修正、最近歷史／24h 圖片／手動保存／Undo／quota 語意。
- 部署：`0010` 是資料單位切換；停舊服務、備份與 image+DB 成套回退；不能聲稱任何升級只花重啟時間。
- Health：`SELECT 1` 的能力範圍，不保證寫入／AI／備份成功。

驗收：每項敘述能對照實際程式；連結存在；文件不含 secret；只有確實已實作的工作包才能寫「已完成」。

## G. CI 守門補齊

修改 `.github/workflows/ci.yml` 與 `Makefile` 的 `check`，使本機與 CI 規則一致：

1. `npm run gen:api` 後新增 `git diff --exit-code -- frontend/src/shared/api/schema.d.ts`；注意 CI frontend job 的工作目錄不同，path 要依執行位置調整。
2. 加入既有 `npm run lint`。先記錄目前 warnings；不隨手加 `--deny-warnings` 導致無關舊碼阻塞。
3. 保留既有後端 OpenAPI 差異檢查、typecheck、tests、build、E2E → docker 依賴。
4. 不變更 image tags、發布條件或 secrets 權限。

驗收：乾淨且已同步的 checkout 可通過；刻意讓提交的生成型別落後會被差異檢查抓到。驗證可在暫存 worktree／fixture 完成，不留測試用修改。新 API 開發中的預期 generated diff 應一併納入提交，不靠關閉 CI 解決。

## H. 需另行確認範圍的後續工作

### H1. 初次匯入冪等與長任務恢復（R3）

開始施工前，先請使用者選擇：

- **維持同步、只改善重送**：multipart 增加 client operation ID、持久化 request fingerprint、保留相同 key 的結果查詢。
- **要求離開頁面／重啟仍完成**：改 `202 + run ID` 與 durable worker，影響 frontend polling、輸入持久化及 shutdown/recovery。

不論哪一種，都先定義同 key 不同內容回 409、running replay 的回應、期限、授權、quota 與圖片儲存政策。單純把 future 放進 `tokio::spawn` 不是持久化工作佇列；初次匯入目前未必保存原圖，不能假設重啟後一定能重跑。

需要先檢視：`StartImportCmd`、multipart handler、frontend upload hooks、run DTO、maintenance 的十分鐘常數與 `AI_RUN_TIMEOUT_SECS`。沒有明確選擇前不要改同步 API。

### H2. 草稿格式版本（R7）

等下一次真的要改 draft 結構時實作。讀取現有 `state_json`，列出舊格式 fixture；先選 additive defaults，遇 breaking change 才加 schema version／migration。禁止以 revision 值猜 JSON 格式，也不能因 draft 已到期就把無法解析的舊 JSON 當空草稿。

### H3. 容量、保留期限與資料庫擴充

先蒐集：DB/WAL 大小、寫入等待、每日圖片／對話數、備份時間與可接受停機。再決定 usage／對話保存天數、圖片是否外移、是否改 PostgreSQL。沒有這些數據，不把 SQLite 換掉當作預設施工項。

## 3. 建議驗證命令

從專案根目錄執行，使用既有 target 以避免重建造成磁碟不足：

```sh
export CARGO_TARGET_DIR=/tmp/opencode/vendor-images-target
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0

# A/B：先針對 AI repository／service，再驗整個後端
cargo test --manifest-path backend/Cargo.toml -p infrastructure ai_import
cargo test --manifest-path backend/Cargo.toml -p infrastructure ai_draft
cargo test --manifest-path backend/Cargo.toml -p api ai_import
cargo fmt --manifest-path backend/Cargo.toml --all --check
cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml --workspace

# D：共用權限回歸
cargo test --manifest-path backend/Cargo.toml -p infrastructure sharing
cargo test --manifest-path backend/Cargo.toml -p api sharing

# 有公開契約變更才生成；確認生成 diff 是預期修改
make gen-api

# E/G 與整合收尾
npm --prefix frontend run typecheck
npm --prefix frontend run lint
npm --prefix frontend test -- --run
npm --prefix frontend run build
scripts/version.sh check
git diff --check
```

測試 filter 在後續版本可能改名；**若顯示 0 tests，要修正 filter，不可當成測試通過證據。**先 targeted，再於整合里程碑跑 full；同一版 checks 全過後不要無理由反覆重跑。

需要 E2E 時先 build 新 frontend 與 embedded server，不能使用舊 binary：

```sh
make build
export E2E_SERVER_BINARY="$CARGO_TARGET_DIR/release/server"
```

再從 `e2e/` 執行：

```sh
npx tsc --noEmit
npx playwright test tests/ai-import.spec.ts tests/sharing-groups.spec.ts
```

涉及路由、session、全域 HTTP middleware 或跨功能 client 的整合交付，再跑完整 Playwright suite。E2E config 應維持 demo／空 AI key；不要用 `.env.production` 跑自動化。真 AI 評測需要另外取得使用者授權，不是這些重構的預設驗收。

## 4. 每個工作包的交付格式

接手 agent 完成後，必須回報：

1. 基準 commit、完成的工作包，及實際修改檔案。
2. 已維持／變動的契約；migration 編號及舊資料處理方式。
3. 新增哪些行為測試，實際命令、測試數、結果及 log 路徑。
4. 尚未驗證的內容，與任何偏離本規格的決策。
5. 工作目錄狀態；未經使用者要求不 commit／push／部署。

施工狀態追蹤：

- [x] A 配額原子保留（2026-11-04，主 agent 已驗收核准）
- [x] B 對話用量紀錄（2026-11-04，主 agent 已驗收核准）
- [x] C Request／turn 日誌關聯（2026-11-04，主 agent 已接受轉義退修證據並核准）
- [x] D 共用群組依賴注入（2026-11-04，主 agent 已驗收核准）
- [x] E 前端型別化 API（2026-11-04，主 agent 已驗收核准，進入整合）
- [x] F 文件校正（2026-11-04，主 agent 最終驗收核准）
- [x] G CI 守門（2026-11-04，主 agent 最終驗收核准）
- [ ] H 產品／營運範圍另行確認

### A 施工紀錄（2026-11-04）

- 基準：`c499a31aa21a315baaa6a481e8e5c5d7375cd500`。本次不 commit／push／部署。
- `application/src/ports.rs` 的 `create` 改為 `reserve_run(run, photo, quota) -> RepoResult<()>`，以 `run.created_at` 作統一時間基準；`application/src/ai_import/service.rs::start` 不再先查配額，reservation 成功後才執行模型。
- `infrastructure/src/ai_import_repo.rs` 在同一個 `BEGIN IMMEDIATE` 內檢查配額、插入 run／照片並 commit；初次匯入與 `save_draft` 共用計數／檢查 helper，`count_since` 僅供查詢。保留 `created_at >= since`、每人滾動 24h、失敗仍扣一次與內部 retry 不重扣的語意。
- 已追蹤所有 `ImportRunRepo` 實作／呼叫：正式實作僅 SQLite，production caller 僅 `start`；`infrastructure/src/ai_import_tests.rs` 三處直接建立 fixture 已改用 reservation，`ai_draft_tests.rs` 的草稿 fixture 開放給同層 quota 測試重用。server wiring／API testing 的組裝不需變更。
- 新增 `infrastructure/src/ai_quota_tests.rs` 六項測試：初次／初次及初次／對話剩一名額並發（各一成功、模型僅一次）、不同 organizer 並發、超額照片零殘留／照片 INSERT 失敗整筆 rollback、固定 Clock 下 run／turn 的 24h 包含邊界、retry 成功／失敗及不可重試失敗仍各算一次。並發使用 reservation barrier、獨立 writer pools、實際 SQLite 與多執行緒任務，不以 sleep 猜排程；沒有 production hook。
- `api/src/routes/ai_import_tests.rs` 新增混合配額測試，兩入口皆維持 `429/ai_quota_exceeded`。HTTP schema／OpenAPI／migration 均未變更，無舊資料轉換。
- 檢查使用 §3 的四個 Cargo 環境變數，全部通過：`cargo test --manifest-path backend/Cargo.toml -p infrastructure ai_import`（46 tests，含 draft／quota）；`cargo test --manifest-path backend/Cargo.toml -p api ai_import`（25 tests）；`cargo fmt --manifest-path backend/Cargo.toml --all --check`；`cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings`；`cargo test --manifest-path backend/Cargo.toml --workspace`（完整後端一次，385 tests）；`git diff --check`。
- 本機 logs：`/tmp/opencode/workpackage-a-{infrastructure,api,fmt,clippy,workspace}.log`。未執行前端／E2E、真 AI 或正式 DB／部署驗證；未進入 B–H，待主 agent 驗收後安排後續工作。
- 後續驗收：主 agent 已審查 `reserve_run`／`check_quota` diff 與真並發測試，核准 A 並授權接續 B。

### B 施工紀錄（2026-11-04）

- 基準仍為 `c499a31aa21a315baaa6a481e8e5c5d7375cd500` 加上已核准、未 commit 的 A 工作目錄。本次僅執行 B，未進入 C，未 commit／push／部署。
- 新增 `backend/migrations/0011_ai_turn_metrics.sql`，對既有 `ai_import_turn_usage` 追加 nullable 欄位，保留 `(run_id, turn_id)` 唯一鍵及原配額計數：
  - `finished_at`：本次 finalization 的 wall-clock 時間；未完成或歷史資料為 NULL。
  - `input_tokens`／`output_tokens`：各自獨立的已觀測累計值，可以是部分用量；未知為 NULL，已明確回報零才存 0。
  - `model`：provider 回應中最後觀測的 model；無 hook 的成功 adapter 沿用 outcome 回報值，無法確認為 NULL。
  - `duration_ms`：從 reservation 成功到模型／草稿處理結束的 monotonic elapsed，不含 telemetry 寫入耗時。
  - `execution_outcome`：`completed`／`failed`；DB 另容許未來使用 `interrupted`，本次不新增回收器。未知仍 NULL。
  - `draft_outcome`：`applied`（AI edits 已套用草稿）／`unchanged`（草稿未被 AI 改動，包含失敗後保留草稿）／`rejected`（無法完成套用）。與正式店家 apply 無關。
  - `error_code`：受限內部類別，如 `timeout`、`unavailable`、`image_read`、`stale`、`access_revoked`、`invalid_state`；不保存 provider response／key／工具參數。
- `application/src/ai_import/agent.rs` 增加 `ObservedUsage`、`TurnMetrics` 與結果 enums；context 跨 retry 保存每個 token 欄位及已觀測 model。累計採飽和加法避免 u64 panic／wrap，超出 SQLite 範圍會由落盤端拒絕。`infrastructure/src/ai/rig_agent.rs` hook 不再把缺少的 usage 欄位提前變成零。
- `application/src/ports.rs` 新增 `finish_turn(run, turn_id, metrics) -> PortResult<()>`；`infrastructure/src/ai_import_repo.rs` 僅 UPDATE 已存在且 organizer 相符的 reservation，第一次 finalization 生效，重送不累加。不讀寫草稿，也不重新要求已被撤銷的 scope；不能藉此建立 ledger 或改他人的紀錄。token／duration 均用 checked i64 轉換；同 repository 的初次 run usage 寫入也改為 checked 轉換，未改其用量語意。
- `application/src/ai_import/draft_service.rs` 將 prompt 序列化提前至 reservation 前；所有 reservation 後的可回傳錯誤均經獨立 finalization。模型完成但 CAS／撤權／期限阻擋時，仍保存模型結果／已觀測用量並回原錯誤，不回傳不可存取的草稿。相同 turn replay 不再次模型呼叫、扣 quota 或寫入用量。
- `draft_image` 讀取失敗：記 `failed/image_read` 與未知 token／model，嘗試 CAS 清除 pending 並將該 turn 標 failed；若清除失敗，保留既有 pending lease 回收機制。telemetry 寫入失敗（包含 overflow）只發出靜態 error event，保留原操作結果，不回滾已成功草稿或掩蓋原錯誤；ledger 摘要可能繼續為 NULL。程序硬中斷同樣允許未知，未宣稱零消耗或完整帳單。
- 維持 `ImportRun.usage` 僅表示初次辨識用量；公開 JSON body、OpenAPI、前端及配額 429 契約皆未變更。既有 migrations 未改寫，舊 ledger 新欄位全部為 NULL，不回填假資料。
- 新增 `infrastructure/src/ai_turn_metrics_tests.rs` 八項 tests（由 `ai_draft_tests.rs` 掛入並重用 scoped fixture），涵蓋成功多模型回合／多個使用者回合、retry 累計、無 hook fallback、零／缺欄位／部分欄位、未知失敗、timeout、replay、Notify 協調的 manual CAS／撤權／到期、圖片讀取失敗、telemetry DB 寫入失敗、token／duration overflow、owner／reservation 限制、finalization 冪等、0010 舊 ledger 升級及初次 usage 溢位 rollback。`infrastructure/tests/rig_agent.rs` 新增真 wire mock hook 測試，確認缺少 usage 與明確零可區分；四 provider 既有 mock regressions 保持通過。A 的 test-only `GatedRepo` 已補齊 port forwarding，無新增 production hook。
- 使用 §3 的四個 Cargo 資源環境變數，檢查全部通過：`cargo test --manifest-path backend/Cargo.toml -p infrastructure ai_import`（54 tests）；`cargo test --manifest-path backend/Cargo.toml -p infrastructure --test rig_agent`（37 tests）；`cargo test --manifest-path backend/Cargo.toml -p api ai_import`（25 tests）；`cargo fmt --manifest-path backend/Cargo.toml --all --check`；`cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings`；`cargo test --manifest-path backend/Cargo.toml --workspace`（完整後端一次，394 tests）；`git diff --check`。
- 本機 logs：`/tmp/opencode/workpackage-b-{infrastructure,rig,api,fmt,clippy,workspace}.log`。未執行前端／E2E、真 AI、正式 DB／部署驗證；後續由主 agent 驗收 B，再另行安排 C。
- 後續驗收：主 agent 已審查 B 的 agent／context／draft service、0011 欄位及文件語意，核准 B 並授權接續 C。

### C 施工紀錄（2026-11-04）

- 基準仍為 `c499a31aa21a315baaa6a481e8e5c5d7375cd500` 加上已核准、未 commit 的 A／B 工作目錄。本次僅執行 C，未進入其他工作包，未 commit／push／部署。
- `backend/crates/api/src/logging.rs` 新增最外層 request ID middleware，為每個 HTTP request 產生 UUID v7，存於私有 extension 並覆寫回應 `x-request-id`；不沿用／記錄任意用戶端 `x-request-id`。`api/src/lib.rs` 將它放在既有 HTTP trace／安全 middleware 之外，因此成功、404、Origin 403、AI 502 都有 server ID。
- 既有 request span 只新增 `request_id`，保留 method、redacted path、status、latency；仍不記 query／Authorization／Cookie，也保留 `/api/v1/public/g/:token` 與 `/g/:token` 遮罩。
- `application/src/ai_import/service.rs`／`draft_service.rs` 使用 `#[tracing::instrument(skip_all, ...)]` 建立初次 run／draft turn span；async instrumentation 逐次 poll 進出 span，不把 `span.enter()` guard 跨 await。run 為內部 ID，draft turn 僅在 validation 或既有 replay 識別後記錄；不自動記 cmd、prompt、圖片、草稿或結果 body。
- reservation 成功後發出 `ai_import_started`／`ai_turn_started`，處理結束發出 `ai_import_completed`／`ai_turn_completed`；各有 run／turn span context，以及 model、duration_ms、nullable input／output tokens、execution、outcome、受限 error_code。初次 turn 標 `initial`；初次 outcome 的 `proposed/rejected` 表示該操作能否成功返回 proposal，不改寫資料庫 run 狀態語意。未被 admission 放行的請求只有 HTTP trace，不假裝啟動模型。
- `quota_reserved=true` 與 `quota_unit=internal_operation_24h` 表示這一次應用程式操作 reservation，不是供應商帳單；`usage_kind=observed_tokens_not_invoice` 表示可能不完整的已觀測用量。文字日誌以 `None` 表示未知，明確回報零才是 `Some(0)`。model 不冒用 request config 作已觀測結果。
- 相同 turn 重送僅發 `ai_turn_replay`，附 `quota_reserved=false`、`execution=not_started`、`usage_kind=not_reobserved`，不再發 started／completed 或把舊用量當成本次新增消耗；其 duration 是本次 replay 查詢耗時。不宣稱供應商 billing exactly-once。
- 移除初次失敗時 `error=%e` 的 raw error 日誌，改用固定分類；`agent.rs` 集中 `AiError::code` 與 execution／draft outcome 字串語意，repository 重用相同 enum 字串，不改 ledger 值。保留 B 的靜態 telemetry persistence／image cleanup failure error event，它們現在也繼承 request／run／turn span，不影響原操作結果。
- 新增 `api/src/routes/ai_logging_tests.rs` 三項 captured-subscriber integration tests，由 `ai_import_tests.rs` 掛入：驗證 server UUID 覆蓋 client ID、成功／502／Origin 403 回應 ID、request 與 initial／draft／replay 關聯、未知用量／受限錯誤分類、telemetry failure 關聯、兩種 public token 路徑遮罩，以及假 query／Authorization／Cookie／prompt／image／AI key／provider raw error 不出現在捕捉日誌。多執行緒 runtime 下以 `WithSubscriber` 逐次 poll 綁定各自 subscriber，另並行驗證兩個 capture 無交叉；不用全域 subscriber、跨 await 的 thread-local guard 或 sleep。分享頁使用 test assets，不依賴舊 frontend build。
- `api/Cargo.toml` 僅增加既有 workspace `tracing-subscriber` 作 dev dependency，`backend/Cargo.lock` 只補該依賴關係；無新 production dependency、JSON logging 開關、OTel、migration 或公開 JSON body 變動。既有 `RUST_LOG` 保留。
- 使用 §3 的四個 Cargo 資源環境變數，檢查全部通過：`cargo test --manifest-path backend/Cargo.toml -p api ai_import`（28 tests，含新增 3 項 logging integration tests）；`cargo test --manifest-path backend/Cargo.toml -p infrastructure ai_import`（54 tests）；`cargo fmt --manifest-path backend/Cargo.toml --all --check`；`cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings`；`cargo test --manifest-path backend/Cargo.toml --workspace`（完整後端一次，397 tests）；`git diff --check`。
- 本機 logs：`/tmp/opencode/workpackage-c-{api,infrastructure,fmt,clippy,workspace}.log`。本輪未執行真 AI、正式 DB／部署驗證。因修改全域 middleware，前端全新 build、全新 embedded binary 與 full E2E 依主 agent 指示留待 D／E 整合里程碑，不以舊 binary 驗收。
- 退修／核准條件紀錄：主 agent 已確認 request ID layer 順序及 span `skip_all`／events，要求兩處 B 保留的 client turn ID error field 改用 debug escape。`draft_service.rs` 的 image cleanup／metrics persistence failure 均已改為 `turn = ?turn_id`，不增加 API 格式限制。新增 captured regression，以含換行的同一 turn ID 同時觸發 image read、cleanup 與 metrics 寫入失敗，確認兩筆 error event 皆含 request／run 關聯及 escaped `\n`，不含原始換行 ID 或偽造實體行。`cargo test --manifest-path backend/Cargo.toml -p api logging_tests` 共 4 tests 通過，log：`/tmp/opencode/workpackage-c-newline.log`；亦納入 D 的完整後端驗證。已滿足本輪主 agent 指定的 C 退修核准條件，交由主 agent 確認。

### D 施工紀錄（2026-11-04）

- 基準仍為 `c499a31aa21a315baaa6a481e8e5c5d7375cd500` 加上既有 A–C 未 commit 工作目錄。除上述 C 指定退修外，本次僅執行 D；未進入 E–H，未 commit／push／部署。
- `backend/crates/api/src/state.rs` 新增必要依賴 `sharing: Arc<dyn application::sharing::SharingRepo>`，由 `AppState::new` 接收，不提供會暗中回退 SQLite 的預設實作或空轉 service。
- 已搜尋全部 `AppState::new`、`AppState {` 與 `SqliteSharingRepo::new`：constructor 呼叫僅 `backend/crates/server/src/wiring.rs::build_state` 及 `backend/crates/api/src/testing.rs::test_app_with` 兩處，均明確建立並注入 `Arc<SqliteSharingRepo>`；其餘測試沿用共用 fixture 或 clone state。完整 workspace 編譯／測試驗證 caller 一致。
- `backend/crates/api/src/routes/sharing_groups.rs` 的九個 handlers（list／create／get／rename／members／lookup／add／change／remove）全部改用 `s.sharing`，不再 import／建立具體 SQLite adapter；authenticated actor、ID／body 驗證與 error mapping 不變。
- `application/src/sharing.rs::SharingRepo` 與 `infrastructure/src/sharing_repo.rs` 不需修改。scope、權限、最後 admin 計數／修改仍在原 SQLite 交易內；沒有搬成交易外檢查。保留其他既有 API infrastructure 依賴、Db、UA parser 及圖片處理，沒有做全量分層清理。
- `backend/crates/api/src/routes/sharing_tests.rs` 僅新增一個 test-local fake／測試 `sharing_list_uses_injected_repository_with_authenticated_actor`：斷言 handler 傳入 authenticated actor、呼叫 fake 一次、回傳 fake 的群組／role，且 SQLite `sharing_groups` 仍是空表，證明 handler 使用注入介面；不建立每個端點的鏡像 fake tests。
- 無 migration、HTTP body／status／OpenAPI、資料或授權契約變更；既有 sharing API／repository regressions 包含 last admin、併發 demotion／removal、scope 隔離、撤權、manager／member 與 OpenAPI 契約全部保持通過。
- 使用 §3 的四個 Cargo 資源環境變數，檢查全部通過：`cargo test --manifest-path backend/Cargo.toml -p api sharing`（10 tests）；`cargo test --manifest-path backend/Cargo.toml -p infrastructure sharing`（5 tests）；`cargo fmt --manifest-path backend/Cargo.toml --all --check`；`cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings`；`cargo test --manifest-path backend/Cargo.toml --workspace`（完整後端一次，399 tests，含 C 換行退修及 D fake）；`git diff --check`。
- 本機 logs：`/tmp/opencode/workpackage-d-{api,infrastructure,fmt,clippy,workspace}.log`。保留既有 A–C 未 commit 修改；未執行真 AI、正式 DB／部署或前端／E2E。全新 frontend build、embedded binary 與 full E2E 依主 agent 最新指示留待 E＋F＋G 後最終整合驗收。

## 5. 可直接交給後續 agent 的提示詞

```text
請以 docs/architecture-improvement-workplan.md 為施工規格，
先閱讀 docs/architecture-review-2026-11-04.md，完成工作包【填 A/B/C…】。

先確認當前 git 狀態與 AGENTS 指示；依實際程式重新定位，不依賴舊行號。
實作最小完整變更，追蹤所有 callers／fixtures／schema，遵守文件中的不可破壞契約。
優先完成列出的回歸案例，再執行適當 checks。
不做 H 類未核准的擴充，不使用真 AI key，不修改正式資料庫。
不 commit／push／部署。完成後依文件 §4 回報，供主 agent 驗收。
```

### E 施工紀錄（2026-11-04）

- 基準：`c499a31aa21a315baaa6a481e8e5c5d7375cd500` 加上既有 A–D 未 commit 工作目錄。本次完成 E 最小實作，待主 agent 驗收；只追加本紀錄，未改其他工作包狀態，未 commit／push／部署。
- 已搜尋並遷移全部 `sharingRequest` callers：sharing list／detail／members／lookup／commands、roster／vendor list／detail／create、開團跨 scope 目錄、AI run 查詢／確認頁 vendor 查詢，以及 draft save／turn／retry／undo／reload，全部使用既有 `api` 的 path literal、generated params／request body 與 `unwrap`。移除任意字串 transport、`scopePath`、可由 caller 指定 response 的泛型與動態 suffix／method／unknown body；沒有新增 `any`／`as T` 型別逃生口或依賴。
- 通用 scope invalidation 移至 `frontend/src/shared/api/access.ts`，名單／店家 typed list 函式放在 `shared/api/directory.ts`；同步所有使用者（含 group-order hooks、vendor image section 與 invalidation tests）。sharing mutation 改為五種 generated-body command union：create／rename／addMember／changeRole／removeMember。開團使用 `useAccessibleRosters`／`useAccessibleVendors`，內部 generic 只從實際 typed list 函式推導資料，不再由 caller 宣告任意 API 回傳型別。保留個人＋各群組的獨立 query key／cache／請求數，未新增聚合 endpoint。
- 所有遷移的 queryFn 都傳入 AbortSignal；mutation 不借用 query signal。保留既有 client `credentials: include`、204／ApiError／欄位錯誤、403／404 全 scope invalidation。draft request ref 改用 generated `TurnRequest`；frozen request／revision／uncertain／busy／unmount／晚回應保護及 Undo 不覆蓋手改保持原行為。
- **必要 schema 修正（已先回報）**：typed client 揭露 sharing list／create 與 delivery-address 的重複 `operationId`，造成生成型別把 sharing 端點指到 delivery-address response／body；另 `LookupQuery.email` 與 `SharingScopeQuery.sharingGroupId` 誤標為 Path。只在 `backend/crates/api/src/routes/sharing_groups.rs` 新增唯一 `list_sharing_groups`／`create_sharing_group` operation IDs 及兩個 `#[into_params(parameter_in = Query)]`；保留 D 的注入及所有 handler 程式。重新生成 `frontend/openapi.json`／`frontend/src/shared/api/schema.d.ts`，修正既有型別契約描述，**HTTP path／body／status／權限均未變更**；無 migration。`sharing_tests.rs` 只新增 schema regression，不覆寫 D fake。
- 實際改檔：`frontend/openapi.json`；`frontend/src/shared/api/{access.ts,directory.ts,client.test.ts,query-contract.test.tsx,schema.d.ts}`；`features/sharing-groups/{hooks.ts,hooks.test.ts,SharingGroupsPage.tsx,SharingGroupDetailPage.tsx}`；`features/rosters/hooks.ts`；`features/vendors/{hooks.ts,VendorImageSection.tsx}`；`features/group-orders/{hooks.ts,NewGroupOrderPage.tsx}`；`features/ai-import/{hooks.ts,AiImportPage.tsx,draft.tsx,draft.test.tsx}`；上述兩個 backend schema／test 檔案及本施工紀錄。
- 新增前端 34 項測試：編譯期錯誤 endpoint／path／scope／body／command／role 的負向斷言；query 編碼與真 fetch 邊界的 include credentials；204；401／403／404／409／429／503 problem details 與欄位錯誤、非 JSON 502；八種 hook 的 AbortSignal／cancel 後不填入晚回應；五種 command 的 method／path／exact body／scope invalidation；lookup email query；403／404 mutation 全 query-family invalidation；draft 401／403／404／429 保留手改及新 turn ID、unmount 後成功／失敗不採用且不 abort 已送 mutation、晚 server error 保留新手改與完整 frozen retry body。原有 retry 測試加驗 revision／instruction；既有 Undo／晚成功／撤權／vendor／roster／公開訂餐競態測試全保留。
- 檢查全通過（所有 Cargo 命令使用 §3 四個環境變數）：`npm --prefix frontend run typecheck`；`npm --prefix frontend run lint`（12 個既存 warnings，0 errors，未擴大範圍修 lint）；`npm --prefix frontend test -- --run`（33 files、367 tests，原有 333 全通過）；`npm --prefix frontend run build`；`cargo test --manifest-path backend/Cargo.toml -p api sharing_openapi`（2 tests，含新 regression）；`cargo fmt --manifest-path backend/Cargo.toml --all --check`；`cargo clippy --manifest-path backend/Cargo.toml -p api --all-targets -- -D warnings`；`git diff --check`。
- `make gen-api` 已執行；第二次完整生成以 SHA-256 驗證 `openapi.json` 及 `schema.d.ts` 逐 byte 相同，沒有額外 drift（本次預期 schema 修正已保留在 diff）。logs：`/tmp/opencode/workpackage-e-{gen-api,gen-api-repro,typecheck,lint,tests,build,backend-schema,fmt,clippy}.log`；施工中的 targeted 40 tests 亦通過，log：`/tmp/opencode/workpackage-e-targeted.log`，最後新增的 late-error regression 已包含於完整 367 tests。
- 未重跑整個後端（沿用 D 的 399 tests 驗證，加上本次 targeted schema checks），未執行真 AI、正式 DB／部署或 E2E；全新 embedded server 與 full E2E 依指示留待 F／G 完成後主 agent 指定，不使用舊 binary。既有 A–D 修改保持，無新套件、endpoint、migration 或產品行為擴充。
- 後續核准：主 agent 已確認 D 的 state／wiring／九個 handlers 注入，接受 C 換行轉義證據；亦已審 E 的 typed commands、draft frozen retry、directory 函式及 schema 差異，核准 A–E 進入 F／G 最終整合。

### F 施工紀錄（2026-11-04）

- 基準為 `c499a31aa21a315baaa6a481e8e5c5d7375cd500` 加上已核准 A–E 未提交工作目錄；只修改 `docs/design.md`、`docs/deploy.md` 與必要 `README.md` 敘述，不修改另一 agent 的產品檔案。
- 校正 Money（Rust／DB 分、API／AI JSON 元，連結 decimal-money）、通用 OIDC／本機備援入口與非全域管理員語意、SharingGroup／GroupOrder／公開 token 權限區分、已實作群組及撤權、domain events 尚屬未實作提案。早期 schema／概念目錄／里程碑明確標為歷史示意，不拿舊 Google 欄位或價格 CHECK 冒充現行 schema；未改歷史 prompt 原文。
- 補齊 draft 文字修正／最近六輪／24h opt-in 圖片／手動保存／Undo／CAS／turn replay，以及滾動 24h 原子內部配額與失敗／retry 語意；保留初次 `ImportRun.usage`，說明 turn ledger nullable 觀測用量、model、duration／outcomes、CAS／撤權後仍可記內部摘要、telemetry 失敗及 crash 未知，不宣稱零消耗或供應商帳單 exactly-once。
- 說明 server `x-request-id`、遮罩 HTTP span、AI 摘要及 replay，保留文字 `RUST_LOG`，不是完整 audit log／OTel；明示同步 HTTP／無初次操作冪等、無 durable worker、歷史 ledger 無整體保存期限。Health 僅 read pool `SELECT 1`，不保證可寫、AI 或備份／還原成功。
- 部署說明區分 `0010` 金額轉換與 `0011` nullable metrics 擴充；先停舊服務、一致備份整個 volume 與舊 image／設定，再啟新版本。停機含備份／migration／驗證，跨 0010 必須 image＋DB 成套回退，0011 additive 不等於已保證舊 binary 可直接降版。
- README 同步生成檔守門及預期未提交差異的說明。對 README／design／deploy 的 22 個本機 Markdown 路徑與 heading targets 做存在檢查，全通過；內容均為程式可對照敘述或明確歷史／未實作限制，未讀取實際 `.env`／key。

### G 施工紀錄（2026-11-04）

- `.github/workflows/ci.yml` 在 frontend job 的 `npm run gen:api` 後加入 `git diff --exit-code -- src/shared/api/schema.d.ts`（該 job cwd 為 frontend），並加入既有 `npm run lint`；保留原 typecheck／tests／build、backend OpenAPI 守門、E2E → docker needs、tags／發布條件／permissions／secrets。
- `Makefile::check` 在同一生成步驟後加入根目錄相對路徑 `git diff --exit-code -- frontend/src/shared/api/schema.d.ts`，另加 `cd frontend && npm run lint`，不增加 ignore／放寬任何 lint 規則。實際 lint 保持 E 的 12 個既存 warnings、0 errors，不以 deny-warnings 擴大本輪範圍。
- 暫存驗證腳本 `/tmp/opencode/verify-fg-schema.py` 在 `/tmp/opencode` 的隔離 Git fixture 複製目前 spec／schema，沿用實際 `gen:api` 與兩條守門命令：同步生成後 root／frontend cwd 都 exit 0；刻意令 fixture index 的 schema stale，再生成後兩者都 exit 1；將已生成正確檔納入 fixture index 後兩者再 exit 0。未建立任何 commit，前後比對產品 index entries 完全相同，沒有 stage 或刪改產品檔案。
- Backend OpenAPI 連續匯出兩次，frontend types 在 fixture 連續生成兩次，均逐 byte 等同目前 E 的預期工作檔：OpenAPI SHA-256 `793539d0e976f0d516c1b2c3e56349e6a9835a897cd33da05f18d2c5c5a96ede`；schema SHA-256 `d0f110f13a7cda11cdb5bc76661bd726a478b3ffb83c5f5e1daa3d99a493a3d4`。證據：`/tmp/opencode/workpackage-fg-schema.log`。
- 沒有直接把本工作目錄的 `make check` 當成可全綠 checkout：E 刻意保留未 stage 的 generated diff，現有 OpenAPI 與新增 schema gate 比較 index 時預期會阻擋，不能為此關掉守門或偷偷 stage。整合採逐項相同檢查及隔離 fixture 驗證；前端完整 367 tests 沿用 E 已通過結果，本輪未改前端產品而不重跑。

### A–G 最終整合紀錄（2026-11-04，主 agent 已核准）

- 使用 `CARGO_TARGET_DIR=/tmp/opencode/vendor-images-target`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_INCREMENTAL=0`。因 E 新增 schema annotations／regression，此次在最新合併工作目錄重跑完整後端一次：`cargo test --manifest-path backend/Cargo.toml --workspace` **400 tests 通過**（API 135、application 25、domain 57、infrastructure 113、Rig mocks 37、server 33；0 failed）。
- `cargo fmt --manifest-path backend/Cargo.toml --all --check`、`cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings`、`scripts/version.sh check`（0.1.0）、`git diff --check` 全通過。前端 `npm --prefix frontend run typecheck`／`run lint` 通過，lint 仍是 12 個已知 warnings、0 errors；本輪未改前端產品，完整 unit suite 沿用 E 的 **33 files／367 tests**，未無理由重跑。
- 生成／守門證據見 G：兩次 backend 匯出及兩次 frontend types 生成均等同工作檔，隔離 fixture 對 root／frontend cwd 分別驗證同步 exit 0、stale exit 1、重新同步 exit 0。只操作暫存 fixture index，產品 index entries 前後一致；保留 E 的預期未提交 generated diff。
- `make build` 在上述 target 先重建當前 frontend（Vite 2529 modules），再成功 `cargo build --release -p server`，包含重新編譯 api／server，使用新的 embedded assets。沒有用舊 release binary 或 dev server 代驗。
- 從 `e2e/` 執行 `npx tsc --noEmit` 通過；再以 `E2E_SERVER_BINARY=/tmp/opencode/vendor-images-target/release/server`、`TMPDIR=/tmp/opencode`、`CHROME_PATH=/usr/bin/google-chrome` 執行完整 `npx playwright test`：**70／70 通過，2 workers，2.4 分鐘，一次完整執行無失敗／重試**（mobile／desktop）。沿用 config 的 fresh SQLite、`AI_DEMO=true` 及空的兩種 AI key，不連真 AI；測試 DB 由既有 teardown 清理。無視覺產品變更，未額外拍攝 UI 截圖。
- 本次實際修改僅 `.github/workflows/ci.yml`、`Makefile`、`README.md`、`docs/design.md`、`docs/deploy.md`、本施工文件，以及歷史審查報告的後續狀態 banner；保留報告所有原始發現與原基準，明示 A–G 工作目錄未提交／未部署及 H 未施工。A–E 產品與測試檔案未被本輪改寫。
- Logs：`/tmp/opencode/workpackage-fg-{workspace,fmt,clippy,typecheck,lint,schema,version,build,e2e-tsc,e2e,docs-ci,artifacts,diff}.log`；schema fixture 腳本 `/tmp/opencode/verify-fg-schema.py`。前端 unit 既有證據為 `/tmp/opencode/workpackage-e-tests.log`。文件 local links／headings 共 22 項存在檢查通過，另驗證 CI 僅加入 types guard／lint、審查報告除 dated banner 外逐 byte 未變；未讀取實際 `.env`／provider key。
- 交付狀態：A–G done，**待主 agent 最終核准**；H 不在本輪。HEAD 仍為原文件 commit `c499a31aa21a315baaa6a481e8e5c5d7375cd500`（相對 origin/main 唯一 ahead commit），main ahead 1，所有施工修改保持未 commit／push／stage。未付費 AI、未部署、未執行正式 DB／實機 OIDC、Docker 雙架構發布、滲透測試或正式備份還原演練。CI YAML 守門已本機驗證，但未在遠端 Actions 執行。
- 主 agent 最終核准：已審原子 quota reservation 與並發測試、nullable usage／獨立 telemetry finalization、request ID middleware 順序與敏感字串轉義、SharingRepo 注入、typed API／草稿重送保護、schema 標註修正、CI／Makefile 守門及部署文件；已核對最終 backend、frontend、E2E、schema fixture 與 build logs，並再次通過 diff check。接受後端 400、前端 367、E2E 70 項測試證據及 12 個既有 lint warnings。以上「待核准」為實作者交接當時狀態，現已完成驗收；未驗邊界與未提交狀態維持如上。
