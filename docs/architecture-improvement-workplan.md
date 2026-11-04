# 架構改善施工文件（供後續 AI agent 使用）

> 建立日期：2026-11-04  
> 分析基準：`3887edef9dc4cbb406513318dcc429a366b7e3d1`  
> 狀態：**施工規格，尚未實作**。本文不是已完成工作的紀錄。  
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

- [ ] A 配額原子保留
- [ ] B 對話用量紀錄
- [ ] C Request／turn 日誌關聯
- [ ] D 共用群組依賴注入
- [ ] E 前端型別化 API
- [ ] F 文件校正
- [ ] G CI 守門
- [ ] H 產品／營運範圍另行確認

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
