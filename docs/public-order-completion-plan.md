# Public 訂餐完成畫面：調查與最小實作計畫

## 完成狀態：主 agent 已最終核准（2026-11-02）

- A/B 功能實作、D 真 backend 桌面／手機 E2E 與視覺退修複驗已完成，主 agent 已明確接受；以下調查、待驗／待核准文字保留為各階段歷史紀錄，以本節最終狀態為準。
- 主 agent 已審查 `OrderEditor.tsx`、`hooks.ts`、`PublicOrderPage.tsx` 功能 diff；親看初版 mobile 完成／取消與 desktop full-page 圖，並親看最新版 `/tmp/opencode/completion-visual-acceptance/screenshots/mobile/completion-full.png`，確認成功 green check、全寬修改 primary、低權重取消。
- 最終接受證據：前端 **319 tests**、完整 Playwright **66 tests**、視覺收尾前端 **99 targeted tests** 與 desktop/mobile lifecycle **2 tests** 通過；各階段 typecheck／build 與測試範圍、log、截圖路徑詳見下方紀錄。99 targeted 結果由 A／主 agent 交接確認，D 未重跑；66 為視覺退修前完整基準，視覺退修後只複驗兩個 lifecycle，不冒稱重新跑完整 suite。
- **保留未驗**：實體手機、實際 LINE WebView、實機 safe-area／軟鍵盤與 dark mode；Chromium mobile emulation 不等於上述實機驗證。原範圍外的跨裝置衝突策略、額外備註權限與姓名歷史快照不變。
- 本次最終交接只更新此文件，不改產品、不重跑測試、不 commit／push；原 ahead commit 保留。主 agent 核准不代表已提交或部署。

## 範圍／基準

- 基準 `ad9856f`，main 比 origin/main 多一個 commit；開始時 working tree 乾淨。本階段只新增此文件，不改產品／測試、不 commit／push。已讀 `ponytail`；repository（含隱藏路徑）與已查父層未找到 `AGENTS.md`。
- 核准流程：選餐 → 後端確認成功 →「訂單已送出」獨立完成畫面，註明「已登記至本團，並非店家已接單」。顯示本人姓名、實際已存餐點／數量／品項備註／訂單備註／金額／submittedAt；primary「修改訂單」，次要「取消我的訂單」。修改可儲存或放棄；取消成功清楚顯示已取消及「重新訂餐」。
- 保留大家訂了什麼 tab、記住身分、確認身分、換人、售完與截止／撤權防護；不新增路由、依賴、後端／schema／權限功能。

## 已確認接點

| 接點 | 事實與約束 |
| --- | --- |
| `frontend/src/features/public-order/PublicOrderPage.tsx` | TokenPage keyed token；OrderingPage 記住 participant（localStorage `bento:last:${token}`），驗證其仍在 group；兩 panel `keepMounted`。OrderStep 目前等 effective query 有 data 才 mount editor，但背景 GET error + data 時仍留 editor。切人須確保 editor 的所有本機狀態以 participant key 隔離。 |
| 同檔 `ReadOnlyOrder` | 截止時另 mount observer，要求 `isFetchedAfterMount` 且不 fetching，等 `useIsMutating(orderSubmissionKey)` 歸零才顯示；失敗有 retry，不能退回舊 snapshot。closed／archived／本機 deadline／overview metadata／409 都可觸發唯讀，forcedClosed latch 防晚到 open 重開。 |
| `OrderEditor.tsx` | qty、兩種 notes、展開、busy／sending ref、pendingKey、cancel dialog 都在本機；只在初始 mount 由 effective order 初始化。`send` 有 client preparation 鎖定、卸載／截止 readOnlyRef 防後續 POST。成功目前用 POST response reset 草稿並 toast，仍停 editor。 |
| 同檔 `SavedOrder` | 已完整顯示後端 lines.name／quantity／unitPrice／subtotal／note、order.note／total／submittedAt，包含非現行 menu 品項；目前只供唯讀使用，可直接共用其內容，別重算金額或從 menu 查名稱。只在 public page 呼叫 OrderEditor；管理端 OrganizerOrderEditor 是獨立元件。 |
| `hooks.ts` | effective key `[public-order, token, pid]`，staleTime/gcTime 0、關閉 window focus refetch；GET 消費 signal。submit onMutate cancel 舊 GET；**async onSuccess 先 cancel，再 setQueryData**；async onSettled 再 cancel effective／overview／group 並 await invalidate。`mutateAsync` resolve 前可有數次 cache render，mutation pending 涵蓋 settlement revalidation。 |
| `lib.ts` | orderContent 正規化 qty／trim notes，不把顯示快照當 dirty；pending idempotency key 同 payload 失敗重試沿用，變更 payload 換 key，成功清 key。tab 不得 reset key 或 draft。 |
| 取消 dialog／`focus.ts` | 既有 dialog 初始 focus「保留」、trap、Escape／背景／保留回 trigger；preparation/API pending 禁關／禁重複。成功目前回 status；inactive 關 portal，restoreOrderFocus 以 microtask 檢查 connected／hidden，隱藏時回當前 tab。 |
| `PublicOrderOverview.tsx`／shared tabs | overview 明確只 render 姓名、餐點數量、時間；不得重用含備註／金額的 SavedOrder。Base UI keepMounted panel 有 hidden/inert，但 effect／portal 仍存活，必須保留 active gating。 |
| TokenPage／privacy footer | 任一 public read/write 401/403/404 立即 token-scoped 阻斷並 cancel/remove caches，晚到成功不得復活。IP／裝置記錄提示在 own panel，完成／修改／取消也保留。身分確認只是防誤選，不是驗證：token + pid 既有 GET 原本就可讀備註／價格，不擴大隱私宣稱。 |

### 後端契約足夠，不改 backend

- `backend/crates/api/src/routes/public.rs:77–93,301–370`：GET／POST 同為 `{ order: EffectiveOrder | null }`；GET 的 null **明確表示從未送出**，POST 正常成功必有 Some(order)。取消也是同一 POST，送 `lines: []`（不帶 note）。
- `backend/crates/application/src/ordering.rs:279–323`：POST 回傳寫入後**當下有效訂單**；同 idempotency key 重播不新增，且可在團關閉後重播既有成功。回傳不保證一定是這次 payload（可能已有另一筆取代），UI 應依實際 response.cancelled／lines 顯示，不照 cancelling 參數宣稱成功狀態。
- `backend/crates/infrastructure/src/ordering_repo.rs:774–822`：effective read transaction 讀最新 submission + lines，空 lines 為 cancelled，價格來自已存 line，名稱來自團內 offered item。金額不可用前端 draft total 代替。
- EffectiveOrder 沒有 participant name；姓名使用 group API 中相同 participant id 的團內名單姓名（既有團內快照，不是搜尋字串或 draft）。不為顯示姓名擴充 API；若另要求「每次送出時姓名的歷史快照」才是新後端需求，不在本次範圍。

## 狀態責任與轉移（實作 owner：前端 agent；主 agent 核准／驗收）

不要新增全域 state machine。Query 是 server truth；editor 的小型 local mode 是「正在編輯」的使用者意圖，兩者不能混成一個 `hasOrdered` boolean。

| 狀態／owner | 進入條件／呈現 | 可做的轉移 |
| --- | --- | --- |
| unknown/loading/error — OrderStep + query | 尚無成功 response 時 skeleton／錯誤重試，**不 mount 空 editor、不顯示沒有訂餐**。`undefined` 與 `{order:null}` 不同；不能用 `data?.order ?? null` 當載入預設。 | GET 成功才分類為 saved／cancelled／new draft。已有 data 的背景錯誤保留本機 draft，標明更新失敗；不可把 cached null 當新鮮無單證據。 |
| new draft — OrderEditor | 成功 GET 明確 order:null；或已取消後主動重新訂餐。選餐與兩種 notes。 | 送出成功→saved 或實際 cancelled；失敗→原 draft／錯誤；不加無單的「放棄修改」。 |
| saved — OrderEditor 的 summary mode | 初次有效 GET、重新開鏈，或 POST 確認後；只顯示 authoritative order，不呈現菜單／sticky submit bar。 | 修改→從**當前已存 order**初始化 draft；取消→既有 dialog；換人→picker。 |
| editing — OrderEditor | 主動按修改，保留 authoritative saved 與獨立 draft。 | 儲存成功→依 server order 返回 summary；放棄→丟本次 draft、回目前 saved；失敗保留 draft。切 tab 不退出 editing。 |
| preparing/submitting — send + hook | sending ref 先同步鎖，busy 含 preparation→POST→settlement。不能以 cache 更新提早離開 editor。 | 成功的明確 handler 才切 summary；失敗保留當次內容。期間禁放棄／重新進編輯／重複提交等會 reset draft 的操作。 |
| cancelled — summary mode | authoritative order.cancelled，不是 local 點過取消。 | 顯示取消時間及 open 時「重新訂餐」；按下後清空 qty／兩 notes／展開。取消失敗留 dialog、原 saved 與 draft。 |
| readonly — OrderingPage + ReadOnlyOrder | 截止／closed／archived 優先於任何 local mode。 | 沿用 fresh GET + post-settlement gate；SavedOrder 顯示 saved／cancelled／明確無單；無修改／取消／重新訂餐。 |
| denied — TokenPage | 任一 401/403/404。 | 卸載所有互動與兩 tabs、清 cache；本 token 不以晚到成功解鎖。 |

## 最小改動方案

1. **`OrderEditor.tsx` 負責模式、draft、既有 send／cancel**：加 local `editing`（首次有 order，包括 cancelled，預設 false；成功明確 null 才預設 true）。其餘狀態由 query/order 與 readOnly 派生，不在 page 和 editor 各存一份 saved。保留元件本體以延續 pendingKey；用條件 render 呈現 editor 或 summary，不因 tab/query 更新換 key。
2. 共用現有 SavedOrder（可留同檔）：開放期 summary 標題「訂單已送出」+ 非店家接單說明 + 姓名／完整 saved；外層配置 primary 修改、次要取消。cancelled summary 提供重新訂餐。唯讀仍走 ReadOnlyOrder 的既有保護後使用同一內容，但不傳寫入 actions。
3. 抽同檔小型 resetDraft(saved)（qty／notes／noteOpen／orderNote）供進入修改／重新訂餐／放棄／成功使用，**只由明確事件呼叫，不用 `useEffect([order])` 自動覆蓋草稿**。放棄不 POST、不 setQueryData、不 cancel/invalidate server；直接回 query 的最新 saved。重進修改時再由最新 authoritative prop 初始化，不能把進編輯前閉包快照蓋回新 server order。保留現有 key 成功才清除規則。
4. send 仍 await 現有 mutateAsync；只有取得非 null authoritative response 才完成成功轉移，依 `response.order.cancelled` 決定完成／取消；非取消成功內容絕不從 payload 合成。異常 200/null 不當作完成或沒有訂餐，顯示可重試錯誤。POST 錯誤即使 settlement GET 成功更新了 cache，也不以 effect 強制退出 editing。toast 只作輔助，不能當完成的證據。
5. **cache render 競態**：onSuccess setQueryData 時 busy 仍 true，editing 不變；即使 cache 與 dirty 已改變也維持 pending 操作鎖，直到 await resolve 的明確 handler 切 summary。summary 讀目前 server cache，避免晚執行的 POST handler 寫回較新的 settlement GET。若 POST 成功但其後 open 狀態的 revalidation 5xx，可保留已確認 POST snapshot 並明示更新失敗；若 cutoff 同時發生，ReadOnlyOrder 錯誤／fresh gate 必須優先，不能改用 POST snapshot 繞過。
6. **`PublicOrderPage.tsx` 最小配合**：維持 data/error 區分與 active、key 隔離、readOnly observer mount、forcedClosed 和撤權；必要時傳 participant name。背景 GET failure + data 時留 draft／saved，但有明確 error，不當作 fresh empty。不要為完成畫面改 hooks async 次序、移除 query observer，或把 fetching 時的既有 editor 卸載掉。
7. **取消完整重用**：dialog 必須在 summary／editor 之外的共同位置 render，不能因切 summary 提早卸載而遺失 pending 或回焦。可在 editing 保留 saved 的取消入口（兼容「空 draft 仍可取消」）；成功才清 draft 並切 cancelled summary。保留／Escape／背景不動 draft；soldOut 不阻止取消。
8. **焦點／layout**：初始選身分仍 focus 身分標題；主動進修改 focus editor 標題，送出／放棄／取消成功 focus 可存活的完成／取消標題或 status（tabIndex=-1）。dialog 成功 finalFocus 用該新節點，非成功回 trigger；避免兩個 effect 與 dialog 同時搶焦。若 pending 期間切 overview，成功不得偷焦隱藏 own：沿用 restoreOrderFocus／active 判斷回當前 tab，返回 own 不自動再 focus。`barShown = active && !readOnly && editing`，相同條件控制 DOM、ResizeObserver、CSS variable；完成／取消不留固定列或底部多餘留白。

## 最小測試與完成判準

**實作 agent** 更新而非刪掉既有驗證；原本假設「回鏈直接 editor／成功留下 disabled submit」的測試改成先按修改與完成頁斷言。勿只把 locator 改到 toast。

- `frontend/src/features/public-order/PublicOrderPage.test.tsx`：延遲初次 GET／初次 5xx／retry 的全程沒有 empty editor／無單誤判；記住 participant 的有效／取消／null 三種回鏈；POST 未完成不顯成功。server response 與 draft 刻意不同（name／qty／notes／單價／total／submittedAt，含不在現行 menu 品項）證明完成畫面用 saved。修改回帶、儲存、失敗保留、放棄後重新修改不見舊 draft，取消成功／失敗／重新訂。
- `PublicOrderConcurrency.test.tsx`：以 deferred promises 拆開 POST success→setQueryData→pending settlement GET，確認不短暫重開／清空 editor、按鈕仍鎖、最終 latest server snapshot；放棄／再進修改遇 effective refetch 新 order 不回寫舊資料；POST error 但 GET 回不同 persisted order 不自行清 draft。保留 read-first/write-first cutoff、成功後 fresh GET 失敗 retry，以及 overview pending + success/failure/cancel 晚回案例（取消 fixture 應是真 cancelled，而非只有零數量的非取消 order）。
- `PublicOrderOverview.test.tsx`：new draft／editing 往返 tab 保 qty、兩 notes、展開與 retry key；summary 往返仍 summary；隱藏時 POST 成功不偷焦；完成／取消無 bar／CSS variable、回 editor 重新量測；保留 hidden cutoff、撤權矩陣、overview privacy allowlist，不把 SavedOrder 搬進 overview。
- `e2e/tests/ordering.spec.ts`、`e2e/tests/ui-improvements.spec.ts`：更新整條首次送出→完成→修改→放棄／儲存→取消→重新訂→重新開鏈流程；真瀏覽器 desktop/mobile 的 focus、dialog trap／回焦、tab hidden/inert、長姓名／備註／多品換行、固定列清除與重新出現、closed／archived／deadline 唯讀。管理端同名文案 locator 不受影響。

核准後建議交付次序：**A** 前端最小模式與現有測試調整 → **B** deferred race／初載錯誤／focus 回歸 → **C** 主 agent 審查與真瀏覽器驗收。A/B 可由同一實作 agent 做，無需後端任務。

完成需全部滿足：

1. 完成內容均有後端成功 response 根據；載入／讀失敗不假裝無單；完成不是 toast 或 disabled submit。
2. 編輯／放棄／取消只影響本次正確 draft，沒有 query-effect reset／stale server overwrite；取消、重試 key、tab 與 cutoff 既有安全性不退步。
3. `frontend/`：`npm run typecheck`、`npm test -- --run`、`npm run build`、`npm run lint`（既有 warning 分開報告）；root `git diff --check`。
4. `e2e/`：`npx tsc --noEmit`、ordering + ui-improvements Playwright（mobile／desktop；使用含目前 frontend 的新 build，不用舊 embedded binary），附完成／修改／取消畫面截圖供主 agent 目視。
5. Diff 僅 frontend、相應 e2e 與本文件；主 agent 明確核准前不 commit／push，尤其不擅自 push 現有 ahead commit。

本調查未跑產品測試／build／瀏覽器；這是待核准方案，不是實作完成證據。未擴大備註權限保護、未處理跨裝置同時修改的衝突解決（仍以後端最新 effective order 為準）。

## A + B 實作交付（2026-11-01，待主 agent 審查／D 瀏覽器驗收）

### 實際修改

- `frontend/src/features/public-order/OrderEditor.tsx`：新增 local `editing` 意圖與可重試 inline error，不複製 server saved state；初始 saved／cancelled 進 summary，初始明確 null 才開 draft。完成內容共用 SavedOrder，姓名取 group participant，餐點／兩種 notes／單價／小計／總額／時間全讀 persisted snapshot，附「已登記至本團，並非店家已接單」。
- 修改／重新訂餐／放棄皆由明確事件 reset draft；沒有 query-effect reset。放棄不呼叫 POST、不更新 cache、不 invalidate；再修改讀最新 prop。完整保留 dialog／pending key，取消失敗留 dialog 及草稿，錯誤在 dialog 內可讀，成功後才結束 editing。
- 成功 await `mutateAsync` settlement 並檢查 nonnull response 後才切 summary；完成頁繼續讀最新 query cache，不將較舊 POST response 蓋回 settlement GET。取消來自 summary 時，busy 期間只顯示處理中，不因 cache 先變 cancelled 提前宣稱完成。成功文案依 response.cancelled，而非取消參數。
- `frontend/src/features/public-order/hooks.ts`：在 mutationFn 邊界拒絕異常 POST 200/null，作為失敗走既有 settlement；不讓 onSuccess 將 false-empty 放進 cache。原本 cancel → setQueryData → await settlement invalidate 的順序不變，失敗保留 retry key。editor 另保留 nonnull 防線。此異常使用既有 errorMessage 的中文 fallback「發生未預期的錯誤，請稍後再試」。
- `frontend/src/features/public-order/PublicOrderPage.tsx`：背景 effective GET error 有 data 時明示「訂單資料更新失敗，暫顯示上次資料並保留草稿。」；未改 cutoff fresh GET／post-settlement gate、forcedClosed 或 token 撤權機制。
- `barShown = active && !readOnly && editing` 同時控制 DOM／observer／CSS variable。主動進編輯聚焦 editor 標題，事件完成回完成標題/status；取消由既有 dialog finalFocus 回完成 status，非成功回 trigger。hidden tab 成功不搶焦，回 own 不重新 autofocus。

### 測試證據

- 更新所有三份 public-order integration tests 的新互動（saved 先按修改、成功後驗 summary、取消後重新訂），保留原 read-first／write-first cutoff、fresh GET failure/retry、preparation lock、cancel trap／回焦／失敗 key、hidden cutoff、privacy allowlist、撤權矩陣與 overview 舊 response abort／晚回 cache 斷言；取消競態 fixture 改為真正 cancelled + 空 lines。
- 新增 20 個案例：初載 deferred GET + 5xx/retry 的 saved／cancelled／null 三分類；server snapshot 與 draft/menu 刻意不同；200/null 的新草稿／修改／取消失敗及 retry key；取消意圖與實際 response 狀態相反；POST success/error/null → cache render → deferred settlement GET，含 cache 與 draft 完全相同但仍 pending 的操作鎖；失敗 GET 更新為不同 persisted order 不清 draft；放棄後再修改不回寫舊 cache；summary 取消 pending gate 與較新 settlement 取代；open revalidation failure 保留確認 snapshot + 明確錯誤；撤權後晚到 POST 不復活；editing 與 new draft 跨 tab 保兩 notes／展開／key；summary/cancelled 跨 tab 與 bar 清除／重新量測。
- `frontend/ npm test -- --run`：**28 files / 319 tests 全通過**，無 unhandled errors。
- `frontend/ npm run typecheck`：通過。
- `frontend/ npm run build`：通過（產出目前 frontend 的 `frontend/dist/`；未建立或啟動新的 embedded backend binary）。
- `frontend/ npm run lint`：exit 0，**10 個 unrelated warnings**，本次修改檔無 warnings：`shared/components/ReportSummary.tsx:7,11`、`shared/ui/tabs.tsx:81`、`shared/ui/badge.tsx:51`、`shared/ui/button.tsx:57`、`app/AdminLayout.tsx:23`、`features/vendors/VendorInfoForm.tsx:32`、`features/vendors/MenuSection.tsx:28`（only-export-components）；`features/ai-import/ImportDialog.tsx:58`（set-state-in-effect）；`features/group-orders/NewGroupOrderPage.tsx:151`（purity/Date）。未修改這些檔案。
- root `git diff --check`：通過。完整 logs：`/tmp/opencode/frontend-tests.log`、`/tmp/opencode/frontend-build.log`、`/tmp/opencode/frontend-lint.log`。

### 範圍與 D 交接

- Diff 僅六個 `frontend/src/features/public-order/` 產品／測試檔與本文件；沒有 backend、e2e、shared/global、依賴或路由變更。未 commit／push；main 原本 ahead 1 不動。本文件原本即為 untracked，保留此狀態。
- **未驗項**：未修改或執行 `e2e/`、Playwright／真瀏覽器 desktop/mobile、截圖、實際 layout／safe-area 高度與視覺長文換行。D 需使用目前 frontend 的新 build，而非舊 embedded binary，更新／驗證 ordering + ui-improvements 的完成→修改→放棄／儲存→取消→重新訂→回鏈流程；核對 focus/trap／hidden/inert、bar 消失與重新出現、長姓名／備註／多品項，以及 closed／archived／deadline 唯讀。JSDOM tests 不替代這些瀏覽器驗收。
- 跨裝置衝突策略、姓名歷史快照、額外備註權限仍不在範圍；以 server 最新 effective cache 為準。

## D 獨立瀏覽器驗收（2026-11-01，待主 agent 最終核准）

### 永久 E2E 更新

- 只修改 `e2e/tests/ordering.spec.ts`、`e2e/tests/ui-improvements.spec.ts` 與本紀錄；D 未改產品／backend。舊「回鏈直接 editor／成功保留 disabled CTA」改為完成 summary 與明確按修改；原競態／撤權 401/403/404／overview privacy allowlist／關閉封存／deadline fresh read／取消 dialog trap、回焦與 busy／統計及四種寬度案例全部保留。
- 新增 `completion lifecycle uses persisted snapshots, explicit editing and remembered identity`，在既有 mobile（390×844、LINE UA、touch）與 desktop（1280×800）projects 各跑：首次 submit → saved 完成 → 修改 → 切 overview 保留數量及兩種 notes → 放棄回 saved（零 POST、GET snapshot 相同）→ 再修改儲存 → 取消 → 重訂 → 離頁重新開鏈仍完成、localStorage participant 不變。
- 使用真實 backend；延遲 `route.fetch()` 取得的真成功 POST response，確認送出中沒有完成內容／重複操作，切 overview 後成功不偷焦。POST 與初次／放棄後／修改後／取消／重訂 GET JSON 都附於 Playwright report；完成逐項對照 server name／quantity／unitPrice／subtotal／line note／order note／total／submittedAt，兩個長品項、長姓名及多行訂單備註。
- 另驗主動修改 heading、放棄／儲存／取消 status、重訂 heading、回 own tab 焦點；summary／cancelled／overview 無 sticky bar 或 `--order-bar-h`，重訂 editor 重新量測；既有 320/390/768/1280px 固定列維持 113px 並保留隱私 footer 上方留白檢查。

### 建置與檢查

- `export CARGO_TARGET_DIR=/tmp/opencode/vendor-images-target CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0; make build`：通過，frontend build + `cargo build --release -p server`，新 binary `/tmp/opencode/vendor-images-target/release/server`（2026-11-01 23:49:17 +0800），log `/tmp/opencode/completion-build.log`。所有本輪 Playwright 明確設定 `E2E_SERVER_BINARY` 為此新 binary，沒有用 repo 舊 server。
- `e2e/ npx tsc --noEmit`：通過。`git diff --check`：通過。
- 首輪 ordering + ui-improvements：28 passed／2 failed；兩個失敗均為新增測試的 unscoped hidden status locator 同時匹配 own 與 overview 更新訊息，已縮限 own panel，沒有修改產品。修正後新完整流程 desktop/mobile **2 passed (3.2s)**。
- `E2E_SERVER_BINARY=/tmp/opencode/vendor-images-target/release/server npx playwright test`：**66 passed (2.0m)，0 failed／flaky／skipped**，兩 workers，無重跑或 retry；包含 ordering + ui-improvements 的 **30** 個 desktop/mobile 案例。完整 log `/tmp/opencode/completion-full-e2e.log`，保留報告 `/tmp/opencode/completion-acceptance/full-report/index.html` 與 `full-results/`。
- 完整 suite 成功後只補兩個「等 transient toast 消退才截圖」斷言，再跑新 lifecycle desktop/mobile：**2 passed (11.7s)**，`/tmp/opencode/completion-clean-e2e.log`；final `npx tsc --noEmit` 通過。不重跑 full suite，產品與流程斷言未變；最終乾淨截圖、報告及 JSON 另存，不覆蓋完整 suite 證據。

### 截圖與 server 證據（D 已親看）

- 根目錄 `/tmp/opencode/completion-acceptance/`；`clean-report/index.html` 保留補拍 run，`snapshots/mobile/` 與 `snapshots/desktop/` 各有 `initial-post-server.json`、`initial-server.json`、`after-abandon-server.json`、`updated-server.json`、`cancelled-server.json`、`reordered-server.json`（由真 backend response report 匯出）。初次總額 $283、修改後 $406、取消 cancelled=true、重訂僅第二品 $160，放棄前後 snapshot 相同。
- **手機 viewport**：`screenshots/mobile/completion-viewport.png`、`screenshots/mobile/editing-viewport.png`、`screenshots/mobile/cancelled-viewport.png`；另有 `screenshots/mobile/completion-full.png`。
- **桌面 viewport**：`screenshots/desktop/completion-viewport.png`、`screenshots/desktop/editing-viewport.png`、`screenshots/desktop/cancelled-viewport.png`；另有 `screenshots/desktop/completion-full.png`。
- 已以 image read 逐張親看以上八張乾淨截圖：長姓名與長品名可換行；完成卡的單價／小計／總額、長餐點備註與多行訂單備註可讀；完成與取消沒有 sticky bar，取消有重訂及隱私提示；編輯有放棄、saved 摘要、數量與底部儲存列。viewport 內容超過高度時正常捲動（完成 actions 在 full-page 圖可見），沒有橫向溢位或 fixed bar 殘留。輸入中的單行品項備註長於輸入框時以原生水平捲動呈現，saved 備註完整換行。
- D 未發現需退修的產品 bug；首輪唯一失敗為上述新增測試 locator，已修復。驗收結論：D 範圍通過，**仍由主 agent 作最終核准**。

### 未驗／範圍界線

- 遵照交接，不重跑已通過的 frontend 319 tests／獨立 typecheck／lint；frontend build 自帶 `tsc -b`。無 backend 變更，不跑 backend full tests。
- Chromium mobile emulation 不是實體 iOS／Android 或 LINE WebView；未驗實體 safe-area／軟鍵盤／系統背景分頁。跨裝置寫入衝突、額外備註權限、姓名歷史快照仍不在範圍。
- 未 commit／push，原 ahead commit 保留；主 agent 尚待最終審查核准。

## D 視覺退修複驗（A 成功 summary 視覺調整後）

- 以相同四個 Cargo 環境變數重新 `make build`，frontend + embedded release 通過；新 binary 仍為 `/tmp/opencode/vendor-images-target/release/server`，mtime **2026-11-01 23:58:03 +0800**，build log `/tmp/opencode/completion-visual-build.log`。Playwright 明確使用此新 binary。
- 僅跑 `ordering.spec.ts --grep 'completion lifecycle'` 的 desktop/mobile 兩案：**2 passed (13.3s)**，log `/tmp/opencode/completion-visual-e2e.log`；E2E `npx tsc --noEmit` 與 `git diff --check` 通過。依要求不重跑完整 suite；上輪 66 passed 是退修前基準，不冒稱此次重新全驗。
- 在既有 lifecycle 加必要視覺斷言：完成 status 有淡綠底／深綠字與一個勾 icon；修改 CTA 穩定後高 **48px**、寬與 saved 卡相同（mobile **358px**／desktop **416px**）；summary 取消容器與 ghost 按鈕背景透明、文字 muted，無空綠卡；進 editing 後原 `bg-accent` saved 卡保留。取消 status 背景透明、無綠底／勾 icon；重訂 CTA 同樣全寬 48px。初次量測失敗源於測試假設不存在的 `data-variant` 與切模式時 `transition-all` 尚在 44→48px，已改 class／computed style 和 polling settled layout，不改產品。
- 成功 status 實測：mobile **358×62px**／desktop **416×62px**，背景 `oklch(0.982 0.018 155.826)`、文字 `oklch(0.393 0.095 152.535)`，詳 `/tmp/opencode/completion-visual-acceptance/snapshots/{mobile,desktop}/success-layout.json`；真 backend response snapshots 與 HTML report 一併保留於此證據目錄。
- 已親看 desktop/mobile 的新版完成 viewport、完成 full-page 與取消 viewport 共六張：成功區淡綠底、深綠字與勾 icon 明確；primary 修改為全寬深綠按鈕、取消降為中性文字，不再形成空綠卡；取消頁中性、重訂 primary 清楚。長姓名／兩個長品名／兩種 notes 仍可讀，完成與取消無 fixed bar，未見本次需退修的產品問題。busy／unknown 的視覺分支本輪未另製造場景，只沿用既有前端 tests 與程式分支；不擴稱本輪瀏覽器已驗此兩態。
- **主 agent 看圖路徑**（新圖獨立存放，前輪證據未覆蓋）：
  - `/tmp/opencode/completion-visual-acceptance/screenshots/mobile/completion-viewport.png`
  - `/tmp/opencode/completion-visual-acceptance/screenshots/mobile/completion-full.png`
  - `/tmp/opencode/completion-visual-acceptance/screenshots/mobile/cancelled-viewport.png`
  - `/tmp/opencode/completion-visual-acceptance/screenshots/desktop/completion-viewport.png`
  - `/tmp/opencode/completion-visual-acceptance/screenshots/desktop/completion-full.png`
  - `/tmp/opencode/completion-visual-acceptance/screenshots/desktop/cancelled-viewport.png`
- D 本輪只改 E2E 與驗收紀錄，未改產品、未 commit／push；仍待主 agent 最終核准，實機／dark mode 未驗。

## 主 agent 視覺退修（2026-11-01，待 D 局部截圖複驗）

- 主 agent 已通過功能 diff 審查並親看 D 完成圖；保留上輪 66 E2E 通過證據。本次僅修改 `OrderEditor.tsx` 的 icon／class 與相關測試斷言，沒有改動功能、狀態、焦點、dialog、mutation 或 cache 行為。
- summary 僅 `!busy && effective` 時顯示 `CircleCheck`（`aria-hidden="true"`）及深綠字／淡綠底／邊框的成功標題，保留「訂單已送出」文字與原 status 語意；busy／cancelled／unknown 不套用成功色或 icon。
- summary 的「修改訂單」（及既有重新訂餐）primary 改為全寬 48px；取消維持 ghost，summary 外層移除 accent 卡片／邊框，以置中低權重文字呈現。editing 的原 saved 摘要卡與取消入口樣式保留。SavedOrder 標題未改，避免擴及唯讀文案。
- 新增斷言：成功標題色與 icon 隱藏語意、primary 尺寸、summary 不留取消卡片背景／邊框、editing 保留摘要卡、cancelled 及 deferred busy 不誤呈現綠成功。
- 驗證：`frontend/ npm test -- --run src/features/public-order` **5 files / 99 tests 全通過**（log `/tmp/opencode/public-order-visual-tests.log`）；`npm run typecheck`、root `git diff --check` 通過。
- D 交接：只需以包含本輪 frontend 的新 build 局部補拍／親看 desktop、mobile 完成及取消畫面（必要時順看 editing 原卡片），確認成功辨識度、primary 全寬與取消低權重；不重跑完整 E2E。本次未重 build、未跑 full frontend suite／lint／E2E 或瀏覽器；先前驗證證據保留，局部視覺複驗仍待 D。未 commit／push，main 原 ahead 1 未動。
