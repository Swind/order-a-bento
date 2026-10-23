# UI／UX 改善實作計畫

狀態：Phase 1–3 實作完成；Phase 4 自動化／桌面瀏覽器驗收完成，真機 LINE／軟鍵盤／safe area 待驗證

建立日期：2026-10-23

範圍：公開訂餐、開團流程、團單管理頁

## 目標與設計原則

讓訂餐者清楚分辨「正在修改」與「已送出」，讓主辦人在手機上快速閱讀統計，並依收單階段突出目前最重要的工作。

- 延用暖白底、深綠主色、既有間距與圓角，以及至少 44px 的主要觸控區域。
- 使用既有 React state、TanStack Query、Base UI Dialog 與 Tailwind 響應式樣式。
- 以現有 API 的訂單快照、權限及團單狀態為依據；預期可由前端完成，不需新增套件。
- 視覺狀態同時提供文字，不能只靠顏色；載入、失敗、唯讀都要有明確內容。
- 每個 phase 可獨立驗收；完成 task 後勾選，並在文末記錄驗證結果。

本計畫根據程式碼 review 制定；實際斷行、捲動、焦點與手機鍵盤行為須在瀏覽器確認。

## Phase 總覽

| Phase | 主題 | 對應 review | 相依性 | 交付成果 |
| --- | --- | --- | --- | --- |
| 1 | 訂單儲存狀態與身分確認 | 1、3 | 無 | 明確的未儲存狀態、可見且可用鍵盤操作的確認視窗 |
| 2 | 手機統計閱讀與管理頁優先順序 | 2、5 | 無；建議接續 Phase 1 | 雙行統計、依階段調整分享區與摘要位置 |
| 3 | 開團確認與截止後摘要 | 4、6 | 訂單摘要部分接續 Phase 1 | 開團選項摘要、以已送出資料顯示唯讀訂單 |
| 4 | 整合驗收與文件收尾 | 全部 | Phase 1–3 | 跨裝置驗證、必要回歸測試與驗收紀錄 |

建議交付順序：Phase 1 → Phase 2 → Phase 3 → Phase 4。優先完成前兩階段，涵蓋本次 review 的三項最高優先問題。

## Phase 1：訂單儲存狀態與身分確認

### P1-T1：定義草稿與已儲存訂單的比較方式

- [x] 在 `frontend/src/features/public-order/lib.ts` 加入最小必要的訂單內容正規化 helper。
- [x] 以 `itemId`、正數 quantity、trim 後的品項備註與訂單備註比較；項目依 ID 排序，忽略陣列順序、空字串與 null 的差異。
- [x] 已儲存基準取自 `useEffectiveOrder` 的有效訂單；已取消訂單視為沒有有效餐點。
- [x] dirty 狀態由草稿與基準衍生，不另外維護容易失同步的 `isDirty` state。
- [x] 補正規化行為測試：數量、品項備註、訂單備註改變，以及改回原值。

注意：沿用送出 payload 的備註 trim 與零數量排除規則；不要把送出時間、價格或顯示名稱納入 dirty 比較。

### P1-T2：實作明確的送出狀態

主要檔案：`frontend/src/features/public-order/OrderEditor.tsx`、`hooks.ts`。

- [x] 將已送出摘要明確標為「上次送出的訂單」，有變更時加上 `role="status"` 的「有尚未送出的修改」。
- [x] 依下表調整底部主按鈕；單純清空餐點仍走既有獨立取消訂單操作。
- [x] 開始送出到完成期間鎖定數量與備註編輯，避免回應與新的本地草稿互相覆蓋。
- [x] 將 `collectClientInfo()` 的準備階段納入 busy 狀態與例外處理，避免請求前仍可重複送出或失敗後卡住。
- [x] 成功後以伺服器回傳的訂單同步本地草稿與比較基準；失敗則保留草稿及未儲存提示。
- [x] 沿用既有 query cache 更新及 idempotency key 機制；避免為同步資料強制 remount 整個編輯器而丟失輸入。

| 情境 | 主按鈕 | 狀態提示 |
| --- | --- | --- |
| 尚無有效訂單、未選餐 | 送出訂單，停用 | 請先選擇餐點 |
| 尚無有效訂單、已選餐 | 送出訂單 | 尚未送出 |
| 已有訂單、沒有變更 | 訂單已送出，停用 | 顯示已送出時間 |
| 已有訂單、有變更 | 儲存修改 | 有尚未送出的修改 |
| 送出中 | 送出中…，停用 | `aria-busy` |
| 所選品項售完 | 停用送出 | 明確指出需移除或改選的品項 |

**驗收：** 修改後不會只看到成功狀態；改回原值後恢復已送出；API 失敗不清除輸入；成功後不因備註空白或陣列排序持續顯示 dirty。

### P1-T3：將身分確認改為 Dialog

主要檔案：`frontend/src/features/public-order/PublicOrderPage.tsx`。

- [x] 使用既有 `shared/ui/dialog.tsx`，以 `pending` participant 控制開關，取代清單頂端的 inline `alertdialog`。
- [x] 提供標題「確認身分」、姓名、已送出時間，以及「是我／選錯了」兩個明確操作。
- [x] Dialog 開啟後載入訂單；失敗顯示重試，不能把查詢失敗當成「已有訂單」的證據。
- [x] 如最新查詢發現訂單已取消或不存在，改用中性確認文字，不宣稱已送出。
- [x] 初始焦點放「選錯了」或適當的非破壞性操作；關閉時回到原名字按鈕，確認成功後移至訂餐區標題。
- [x] Escape、關閉按鈕、點背景皆視為取消選擇；不改寫記住的 participant。
- [x] 延續已記住本人及未訂餐者的既有直接選取行為。

**驗收：** 捲到長名單底部點選仍立即看見確認；Tab 不離開對話框；取消後回到原位置；只有確認成功才切換身分。

### P1-T4：行為回歸

- [x] 更新 `PublicOrderPage.test.tsx` 中成功後主按鈕的既有斷言。
- [x] 涵蓋 dirty → 成功／失敗、取消後重新訂餐、準備與送出期間防重複操作。
- [x] 改善測試 fixture 的回傳內容，包含實際送出的品項備註與訂單備註，避免假資料漏欄位導致誤判。
- [x] 涵蓋 Dialog 載入、失敗重試、取消、確認及焦點回復；真實捲動交給 Phase 4 驗證。

## Phase 2：手機統計閱讀與管理頁優先順序

### P2-T1：統計列響應式排版

主要檔案：`frontend/src/features/group-orders/StatsTab.tsx`。

- [x] 手機預設採兩行：第一行「品名／小計」，第二行「單價 × 數量／售完控制」。
- [x] `sm` 以上再使用較緊湊的橫向配置，以 CSS grid／flex 排版，使用同一組 DOM 與操作元件。
- [x] 品名允許換行；金額使用 `tabular-nums`，避免固定窄寬度讓大金額溢出。
- [x] 售完控制的 label 保持至少 44px 高可點區域，沿用唯讀與 mutation pending 限制。

**驗收：** 320px、390px 寬度下，長品名、多位數金額與售完控制不重疊、不造成整頁水平捲動；桌面可快速對齊比較數量與金額。

### P2-T2：依收單階段調整分享區

主要檔案：`GroupOrderDetailPage.tsx`、`SharePanel.tsx`，路徑皆在 `frontend/src/features/group-orders/`。

- [x] 以 `status` 與 `deadlineAt` 衍生展示階段，沿用 `useNow` 更新截止判定。
- [x] 收單中：維持分享區展開，「複製連結」為主要操作。
- [x] 已截止或 closed：將分享區折疊為「分享與訂購連結」，放在主要統計／明細／領取繳費內容之後。
- [x] archived：使用精簡分享入口及唯讀提示，提供查看既有訂單的能力。
- [x] 重新開放且截止時間仍在未來／未設定時，恢復收單中的分享呈現。
- [x] 保留目前 URL tab 與使用者選擇；階段切換不強制跳 tab，避免打斷正在收款的人。
- [x] 收單階段與 `canManage` 分開判斷；一般成員的唯讀權限仍由既有權限控制。

**驗收：** `status=open` 但截止時間已過時，不再把分享收單當作主要任務；關閉或封存後，主要內容前不再被大型分享區佔據。

### P2-T3：摘要前置

- [x] 在 `StatsTab.tsx` 將總份數、總金額、已訂／未訂人數移至品項清單上方。
- [x] 在 `ChecklistTab.tsx` 將已收／應收／未繳金額移至篩選與清單之前。
- [x] 沿用原本 summary query；摘要失敗不阻止可正常載入的領取／繳費清單操作。
- [x] 更新 `GroupOrderDetailPage.test.tsx`，涵蓋收單中、時間截止、關閉、封存、重新開放及唯讀成員。

**驗收：** 長清單仍能在開始操作前看見摘要；主辦人不需要捲到底才知道未收款總額。

## Phase 3：開團確認與截止後摘要

### P3-T1：開團設定頁加入選擇摘要

主要檔案：`frontend/src/features/group-orders/NewGroupOrderPage.tsx`。

- [x] 在第三步設定表單前加入 compact summary：名單名稱與人數、店家名稱、開團歸屬。
- [x] 由已選 ID 與既有 directory query 衍生顯示內容，不另外複製一份選擇狀態。
- [x] 名單與店家旁提供「修改」按鈕，回到對應步驟並保留標題、截止時間、地址及備註。
- [x] 個人／共用群組歸屬與名單、店家來源分別標示，避免誤認成同一個設定。
- [x] 名稱完整換行，不因 truncate 隱藏使用者需要核對的資料。

### P3-T2：精簡歸屬說明

- [x] 歸屬選擇保留直接可見的「開團後無法變更」。
- [x] 將較長的資料來源與權限說明放入原生 `details`：「誰可以查看與管理？」。
- [x] 群組名稱載入失敗時提供清楚狀態，避免把內部 ID 當作正常名稱。
- [x] 更新 `NewGroupOrderPage.test.tsx`，涵蓋摘要內容、返回修改保留設定、個人及共用歸屬。

**驗收：** 使用者在按「開團」之前，能在同一步核對名單、人數、店家與歸屬；返回修改不清空已填欄位。

### P3-T3：截止後改為已送出訂單摘要

主要檔案：`frontend/src/features/public-order/OrderEditor.tsx`、`PublicOrderPage.tsx`。

- [x] 將唯讀顯示拆成小型 presentational component（先 colocate，內容需要獨立維護時再抽檔）。
- [x] 已有有效訂單：顯示品名、數量、品項備註、小計、訂單備註、總額及送出時間。
- [x] 資料使用伺服器 `EffectiveOrder` 的 `lines`、`unitPrice`、`subtotal`、`total`，不由目前 menu 或本地草稿重新計價。
- [x] 沒有訂單：顯示「你沒有訂餐」；已取消：顯示「你的訂單已取消」與時間。
- [x] 唯讀區不渲染整份可編輯菜單、停用加減按鈕與停用輸入欄；總額直接置於摘要。
- [x] 保留換人查看、團單狀態及店家資訊；固定送出列移除時同步清除高度保留值。
- [x] 編輯途中截止或收到 409：明確提示「已截止，未送出的修改不會列入訂單」，刷新有效訂單後顯示最後成功送出的內容。
- [x] 刷新有效訂單失敗時提供重試，不把本地未送出內容當成已成立訂單。
- [x] 延續明確的 loading／error 分支，區分「查不到訂單」與「沒有訂餐」。

**驗收：** 草稿與已儲存訂單不同時，截止後只能把後者顯示為有效訂單；已售完或目前菜單不再提供的已訂品項仍依快照顯示。

### P3-T4：唯讀行為回歸

- [x] 在 `PublicOrderPage.test.tsx` 涵蓋有效訂單、未訂、取消、封存與 API 載入失敗。
- [x] 加入截止發生於編輯中與送出中遭 409 的案例，確認不顯示未送出的草稿金額。
- [x] 使用訂單快照價格與 menu 不同的 fixture，驗證摘要金額來源。

## Phase 4：整合驗收與文件收尾

### P4-T1：實際畫面驗收

- [x] 320px、390px、768px、1280px 寬度檢查；截圖至少保留公開訂餐 dirty 狀態、身分 Dialog、手機統計、開團摘要及截止後摘要。
- [x] 以長品名、長姓名、多行備註、大金額與長參與者清單檢查排版及捲動。
- [ ] 確認固定送出列不遮住最後內容、底部 safe area 正常，手機鍵盤開啟時仍可編輯備註。
- [x] 以鍵盤操作完整走過身分確認、選餐、開團步驟及管理頁 tabs；檢查可見焦點與返回焦點。
- [ ] 在實際手機瀏覽器／LINE 內建瀏覽器確認關鍵流程；Playwright 的 LINE user agent 模擬不能替代真機結果。

### P4-T2：必要的端到端回歸

- [x] 優先擴充既有 `e2e/tests/ordering.spec.ts`：選人 → 送出 → 修改 → 儲存 → 截止查看摘要。
- [x] 擴充既有 `e2e/tests/management.spec.ts`：第三步核對 → 開團 → 收單階段切換 → 統計及繳費摘要（第三步與額外摘要驗證由 ordering／ui-improvements spec 補足）。
- [x] 用長名單情境驗證 Dialog 真實可見性、捲動位置與焦點；用窄 viewport 驗證統計列無整頁水平溢出。
- [x] 避免只斷言 CSS class；驗證可見資訊、可操作性、訂單結果及實際 bounding box／overflow。

### P4-T3：執行檢查並記錄結果

各 phase 完成時執行受影響的測試檔及 typecheck；整合時執行完整前端檢查：

```sh
# 工作目錄：frontend/
npm run typecheck
npm run lint
npm test -- --run
npm run build
```

端到端測試使用專案現有流程，從 repository root 執行 `make e2e`。此命令會先建置內嵌前端的 release server，再啟動獨立測試資料庫；若使用系統 Chrome，可依環境設定 `CHROME_PATH`。

目前 Playwright 的 desktop project 只選取部分管理類測試；公開訂餐的桌面驗證需明確加入選取範圍或記錄手動驗收，不能將 mobile 通過當作 desktop 已驗證。

- [x] 完成上述檢查並記錄命令、日期與結果；環境阻礙明確標為未驗證。
- [x] 更新 `frontend/LINE_CHECKLIST.md` 的相關驗收項目及本文件 task 狀態。
- [x] 執行 `git diff --check`，檢視變更範圍。

## 完成判定

1. 六項 review 問題都有對應 task 完成及驗收記錄。
2. 訂單畫面能明確區分草稿、已送出、取消及唯讀狀態。
3. 窄螢幕上的關鍵資訊與操作均可閱讀、點擊及用鍵盤操作。
4. 開團確認內容與實際送出選項一致；團單管理頁符合目前收單階段。
5. 受影響行為的測試、型別檢查及建置通過；未完成的真機驗證有明確記錄。

## 驗收紀錄

| 日期 | Phase／Task | 驗證方式 | 結果與備註 |
| --- | --- | --- | --- |
| 2026-10-23 | 計畫建立 | 元件、API 型別、query hooks 與測試設定檢視 | 完成實作規劃；尚未進行 UI 實作或瀏覽器驗收 |
| 2026-10-23 | P1-T1–T4 | `orderContent.test.ts`、`PublicOrderPage.test.tsx`、Chrome E2E | 正規化／改回原值、成功同步 server notes、失敗保留草稿、取消再訂、準備階段失敗與防重送、送出 busy、Dialog loading／失敗重試／取消／焦點皆通過 |
| 2026-10-23 | P2-T1–T3 | `GroupOrderDetailPage.test.tsx`、`management.spec.ts`、`ui-improvements.spec.ts` | 雙行統計、大金額、44px label、摘要前置、summary 失敗仍可繳費、收單／過期／關閉／封存／重開／唯讀權限與 tab 保留通過 |
| 2026-10-23 | P3-T1–T4 | `NewGroupOrderPage.test.tsx`、公開訂餐測試與 E2E | 個人／共用歸屬與來源獨立；返回修改保留設定；唯讀快照、未訂／取消／封存、讀取失敗重試、下架品項／不同 menu 價格、編輯中自然截止與送出 409 後重新查詢通過 |
| 2026-10-23 | P4-T1／T2 | 系統 Chrome + Playwright，mobile 與 desktop projects | 四種寬度 × 兩個 project，45 人長名單、長姓名／品名、多行備註、$2,000,000；Dialog 可見及焦點限制／返回、鍵盤選餐與開團步驟／tabs、實際 overflow／bounding box 通過；已檢視代表性截圖 |
| 2026-10-23 | P4-T3 | `npm run typecheck` | 通過 |
| 2026-10-23 | P4-T3 | `npm run lint` | exit 0；10 個既有 warning（Fast Refresh exports、ImportDialog effect、NewGroupOrderPage 既有 render Date），未擴大範圍修整 |
| 2026-10-23 | P4-T3 | `npm test -- --run` | 25 個測試檔、203 個測試全部通過 |
| 2026-10-23 | P4-T3 | `npm run build`；`CHROME_PATH=/usr/bin/google-chrome make e2e` | 前端 production build 與 release server build 通過；完整 E2E 最終 25/25 通過（45.3s） |
| 2026-10-23 | P4-T3 | `git diff --check`、變更範圍檢視 | 通過；無新增套件、無 backend 修改、無 commit／push |
| 2026-10-23 | P4-T1 真機部分 | 無可用 iOS／Android LINE 真機 | **未驗證**：實際軟鍵盤與 IME、Home indicator safe area、LINE 原生分享／App 切換；相關兩項 task 保留未勾，詳見 LINE_CHECKLIST |

### 截圖及重跑

- 保存在 `e2e/test-results/ui-improvements-*/`（git ignored），各寬度／project 均有 `public-dirty.png`、`identity-dialog.png`、`statistics.png`、`wizard-summary.png`、`readonly-snapshot.png`，共 40 張；下次 E2E 會更新此目錄。
- desktop project 已明確加入 `ordering.spec.ts` 與 `ui-improvements.spec.ts`，不再僅憑 mobile 通過宣稱桌面完成。
- 初次 E2E 曾因立即讀取 Base UI 焦點循環的暫時 sentinel 狀態而失敗；改成等待穩定焦點後，完整套件連續兩次 25/25 通過，沒有移除焦點限制斷言。

### 實作決策

- `orderContent` 僅比較送出語意；busy 同時使用同步 ref 防重入與 state 呈現，涵蓋裝置資訊準備和 API 回應，成功才依 server response 重設草稿，保留既有 idempotency/cache。
- 截止時卸載可編輯草稿，建立新的唯讀 query observer；必須 `isFetchedAfterMount` 且已停止 fetching 才能呈現快照。**獨立 review 補正：這兩個旗標本身不足以處理 GET／POST 競態**；共用 mutation 在開始、成功寫 cache 前及 settled 後取消該 participant 的 effective-order 查詢，GET 傳入 AbortSignal，settled 後重新查詢；唯讀區同時等待該 mutation（含重新驗證）完成。失敗不讀舊 cache／草稿當有效訂單，保留重試；沿用 30 秒 `useNow` 檢查頻率。
- 分享展示階段不等於管理權限；保留所有既有 capability guards 和 URL tab。截止後使用原生 details，無新 state／套件。

### 獨立 review 修正（2026-10-23，交回原 reviewer 複驗）

初次獨立 review 暫不通過：既有 203 項單元測試與 25 項 E2E 未涵蓋以下競態／錯誤鏈；上述初次通過紀錄不代表這兩項原先已正確。

- [x] R1：自然截止 GET 在 POST 前讀到舊快照，延遲回應覆蓋較新 POST cache。已讀取 reviewer 的 `/tmp/opencode/ui-review-race-20261023.test.tsx` 及同名 log，依同樣的可控延遲順序建立永久 `PublicOrderConcurrency.test.tsx`。
- [x] R1 永久測試覆蓋 GET 先回／POST 先回，並讓被取消的舊 GET 在重新驗證摘要已顯示後才回；檢查 transport abort、mutation 期間不顯示舊摘要、POST 後新 GET 完成前仍 loading、最後 cache 與畫面均為已成立 2 份／$200。Fixture 在 POST 成立後更新 persisted snapshot，區分舊 GET 的 captured 1 份與新 GET 的 2 份。
- [x] R2：sold_out 後刷新 group 500 導致 editor 卸載、重試後草稿全失。已讀取 `/tmp/opencode/ui-review-refresh-20261023.test.tsx` 及同名 log；有 group data 且為暫時性錯誤時改為頁內非阻斷錯誤與重試，不卸載編輯器；首次載入沒有 data 時仍保留原本阻斷錯誤分支。403／404 的權威性失效依下方 P2 複驗修正阻斷。
- [x] R2 永久測試覆蓋 sold_out → GET 500 → 重試取得售完資訊；確認前後輸入 DOM identity、2 份數量、品項備註、訂單備註完全保留，售完後停用送出仍可減少數量。
- [x] 送出中測試 fixture 同步模擬持久化成功結果，支援 mutation settled 後真實重新查詢，而不是僅回傳 POST 假資料。

| 日期 | 命令／驗證 | 結果 |
| --- | --- | --- |
| 2026-10-23 | `npm test -- --run src/features/public-order` | 4 個檔案、33 個測試通過（含 R1 兩種回應順序與 R2 完整鏈） |
| 2026-10-23 | `npm run typecheck`、`npm run lint`、`npm test -- --run`、`npm run build` | 通過；26 個檔案、206 個測試；lint 仍為 10 個既有 warning |
| 2026-10-23 | `CHROME_PATH=/usr/bin/google-chrome make e2e` | review 修正後重新建置 release server，25/25 通過（45.5s）；含自然截止重新讀取與四種寬度 |
| 2026-10-23 | `git diff --check` | review 修正後通過；未 commit／push |

限制：此協調限定相同 QueryClient 與 token／participant key；不新增跨分頁即時同步。重新查詢仍以 API 的 persisted order 為真實來源，不推定跨伺服器 read replica 的一致性；重新驗證失敗會顯示錯誤與重試，不宣稱 cache 就是最新。真機 LINE／軟鍵盤／safe area 未驗證範圍不變。

### 複驗 P2：權威性失效不得沿用可操作 cache（2026-10-23）

原 reviewer 已確認 R1／R2 resolved，但背景 403／404 仍可操作，因此再次暫不通過；已讀取 `/tmp/opencode/ui-review-recheck-20261023.test.tsx` 及同名 log。

- [x] 最小修正 `PublicOrderPage.tsx`：API 403／404 即使有 cache 仍進入阻斷分支。403 顯示無權限；404 顯示連結無效／失效；皆提示向主辦人確認連結及權限。移除編輯器、送出／儲存／取消操作及固定列，不把授權失效說成保留選餐。
- [x] 永久已有 cache → 背景 403／404 測試：先建立可儲存的 dirty 訂單，再刷新失敗；驗證 cache 仍存在但操作／輸入均不呈現、訊息正確、固定列高度清除、沒有提交請求。此處選擇移除操作，而非留下停用按鈕，避免繼續呈現無權存取的舊資料。
- [x] 暫時性網路／500 分支維持 R2 行為，重跑 sold_out → GET 500 → 重試的數量與兩種備註保存測試，以及 R1 兩種回應順序競態測試。
- [x] 將 reviewer 額外驗證納入永久 `PublicOrderConcurrency.test.tsx`：POST 成功 2 份後，截止摘要重新查詢 500；mutation 必須結束、顯示可重試錯誤而非無限 loading，且不以 cache 冒充重新驗證快照；重試成功顯示 $200 且 fetchStatus 為 idle。

| 日期 | 命令／驗證 | 結果 |
| --- | --- | --- |
| 2026-10-23 | `npm test -- --run src/features/public-order` | 4 個檔案、36 個測試全部通過 |
| 2026-10-23 | `npm run typecheck`、`npm run lint`、`npm test -- --run`、`npm run build` | 通過；26 個檔案、209 個測試；lint 仍為 10 個既有 warning |
| 2026-10-23 | `CHROME_PATH=/usr/bin/google-chrome make e2e` | P2 修正後重新建置 release server，25/25 通過（45.4s） |
| 2026-10-23 | `git diff --check` | 通過；未 commit／push |

本次只改錯誤分類／阻斷分支，沒有重構 query／mutation 協調。已開始的送出流程仍由 API 判定權限；前端收到權威性失效後移除後續訂餐入口。真機未驗證項目不變，等待同一 reviewer 複驗。

### 獨立 reviewer 最終複驗（2026-10-23，GPT-6.1 Sol）

結論：**PASS，交回主 agent 最終驗收**。原 R1、R2 及後續 P2 均 resolved；本輪未發現新的阻擋問題。僅更新本驗收紀錄，未修改產品程式碼。

| 項目 | 獨立檢視／複驗結果 |
| --- | --- |
| P2：已有 cache 的背景 403／404 | 檢視 `PublicOrderPage.tsx:252–268` 及永久測試；確認 cache 仍存在時仍顯示對應失效／無權限訊息，移除輸入與送單／取消操作，清除固定列高度；不再誤用暫時性錯誤提示。測試通過。 |
| R1：截止 GET／POST 競態 | 重讀共用 hooks 的 AbortSignal、開始／成功寫 cache 前／settled 取消順序，以及唯讀 mutation 等待邏輯；兩種回應順序、舊 GET 最後返回及最終 cache／畫面一致測試通過。此次錯誤分類修正未改動此協調。 |
| R1：重新驗證失敗與恢復 | 永久測試確認成功 POST 後截止 GET 500 時 mutation 結束、不持續 loading、不以 cache 冒充已驗證快照，重試後顯示 2 份／$200 且 fetchStatus 為 idle。通過。 |
| R2：暫時性刷新失敗保留草稿 | sold_out → group GET 500 → 重試測試通過；數量、兩種備註及輸入 DOM identity 保留，重試取得售完資訊後仍可減少數量。 |

本輪由 reviewer 真正執行（工作目錄 `frontend/`，除另註）：

- `npm test -- --run src/features/public-order`：4 個檔案、36 個測試全部通過。
- `npm run typecheck`：通過。
- repository root 的 `git diff --check`：通過（含本紀錄更新後再檢查）。

本輪變更限於錯誤分類及相應測試，因此未無理由重跑完整 209 項單元測試、build、lint 或 E2E；前節的 209／25 與完整檢查結果屬實作者紀錄，不冒稱本輪 reviewer 已執行。前兩輪 reviewer 的完整單元測試、build／lint 與 25 項 E2E 結果詳見對話回報。

限制：**真機 LINE、iOS／Android 軟鍵盤與 IME、safe area、原生分享與 App 切換仍未驗證**，Phase 4 對應項目保持未勾選；未新增跨分頁同步或跨伺服器一致性保證。已開始的請求仍由後端執行權限檢查。

### 主 agent 最終驗收（2026-10-23）

結論：**程式實作與已執行的自動化驗收通過**；真機項目仍待驗證。

- 核對實作 diff、工作樹新增檔案、六項改善與獨立 reviewer 的三項缺失修復紀錄；未發現新的阻擋問題。
- 親自檢視 query／mutation 取消及重新驗證協調、唯讀等待條件、403／404 阻斷與暫時錯誤保留草稿分支，以及開團摘要、統計排版與分享階段變更。
- 親自執行 `git diff --check` 通過。209 項單元測試、25 項 E2E、build／lint 結果採用上方實作者紀錄，36 項公開訂餐測試與 typecheck 採用獨立 reviewer 最後複驗，沒有冒稱再次執行。
- 主 agent 僅進行最終核對及記錄，未修改產品程式碼。未 commit／push。
