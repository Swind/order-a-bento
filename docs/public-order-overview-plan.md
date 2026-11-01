# Public 點餐頁：大家訂了什麼

## 範圍與調查基準

- 基準 commit：`d55a89f`；調查開始時 working tree 乾淨。repository 及已查的父層未找到 `AGENTS.md`；已讀 `ponytail` skill。本階段只新增本文件，產品實作與測試交 Sol，主 agent 驗收；不 commit/push。
- 同一 `/g/:token` 頁面新增「我要點餐／大家訂了什麼」兩個 tab，預設前者。後者不必先選身分，只顯示已訂者姓名、餐點名稱 × 數量、每人最後送出時間，以及整頁最後成功更新時間。
- 未訂及最新一筆取消者不列；空資料顯示「目前還沒有人訂餐」。列表不可點選／代選人，不改現有 ParticipantPicker、確認身分或換人互動。
- 清楚提示：「持有此訂購連結的人都能查看已送出的姓名與餐點；不顯示備註、繳費及裝置資訊。」不新增設定開關或權限層級。
- 截止、closed、archived、品項售完仍可看已送出的 overview；不能因此開放任何寫入。切 tab 不丟未送草稿；真正截止仍沿用現有「只呈現已送出訂單」規則。

## 已確認的接點與限制

| 實際檔案 | 現況／實作接點 |
| --- | --- |
| `backend/crates/api/src/routes/public.rs` | 已有 group GET、逐人 effective order GET、submission POST；`ClientCtx` 限流；router 統一 `Cache-Control: no-store`、`X-Robots-Tag: noindex`。新 GET 必須掛同 router。 |
| `backend/crates/application/src/ordering.rs` | `group_by_token` 驗證 token，格式錯誤／不存在同為 404；`get_public_group` 另讀 ordered set，並非跨查詢 atomic snapshot。`rotate_share_token` 替換既有 token。讀取不檢查 accepts_orders，寫入才檢查。 |
| `backend/crates/application/src/ports.rs` | `OrderReadModel` 只有 ordered set 與單人 effective order；不要 N 次呼叫，也不要公開 management DTO。 |
| `backend/crates/infrastructure/src/ordering_repo.rs` | `SqliteOrderReadModel` 是唯一 OrderReadModel 實作；既有 `GroupRow`／`GROUP_COLUMNS`／`assemble_group` 可在同檔重用，read transaction 是既有慣例。management projection 含付款／裝置／備註，不適合作 public 回傳。 |
| `backend/migrations/0001_init.sql` | `effective_submissions` 以每人 `MAX(id)` 選最新一筆，空 lines 即取消；不能先排除取消再找上一筆。沿用此 view，無需 migration。 |
| `frontend/src/features/public-order/{PublicOrderPage.tsx,OrderEditor.tsx,hooks.ts}` | 草稿在 editor local state；目前頁面 403/404 才整頁阻斷，401 及其他 public 請求尚未統一。送單已有 cancel 舊 GET → set data → settled 再 cancel/invalidate 的競態保護，不可移除。 |
| `frontend/src/shared/ui/tabs.tsx` | 既有 Base UI Tabs wrapper 透傳 Panel props；已核對安裝版本支援 `keepMounted`，隱藏 panel 帶 hidden/inert。直接重用，不新增 tabs primitive。 |
| `frontend/src/app/PublicLayout.tsx` | `max-w-md` 行動版容器；editor 固定 bottom bar，透過 ResizeObserver 設 document root `--order-bar-h`，頁面據此留白。 |
| `frontend/src/shared/api/{types.ts,schema.d.ts}`、`frontend/openapi.json` | DTO alias 與產生碼接點；schema 用 `make gen-api`，不手改生成檔。 |

**權限事實：**既有逐人 GET 本來就接受 token + participant id，且回傳該訂單備註、價格；身分確認是 UI 防誤選，並非身分驗證。新 endpoint 不增加 token 的管理／修改權限，也不擴大既有 GET。此工作只能保證 **overview response** 不帶私密欄位，不能宣稱持 token 原本就無法讀到備註；若要收緊舊端點須另立需求。

## 最小後端方案

新增 `GET /api/v1/public/g/{token}/orders`，專用 allowlist DTO：

```ts
{
  status: "open" | "closed" | "archived";
  acceptingOrders: boolean;
  deadlineAt: string | null;
  orders: Array<{
    participantId: string; // 穩定 React key；不是選人入口
    name: string;
    submittedAt: string;
    lines: Array<{ itemId: string; name: string; quantity: number }>;
  }>;
}
```

- `submittedAt` 是各有效訂單最後送出時間；整頁「最後更新」使用該 query 最後成功取得回應的 `dataUpdatedAt`，標明是資料更新時間，不假裝是全團最後變更時間（取消／零狀態也可正確顯示）。
- 在 `ports.rs` 增加精簡 overview view 與 `OrderReadModel::public_order_overview(&ShareToken) -> PortResult<Option<...>>`；內部 view 可含 `GroupOrder` + 安全 orders。`ordering.rs` 新 use case 先 parse token（失敗 404），呼叫該方法（None 404），用既有 clock／group.accepts_orders 計算可否下單。
- repo **同一 read transaction**：按 token 讀 `GroupRow`、重用 `assemble_group` 取得 group；一次 join `effective_submissions`、`participants`、`submission_lines`、`offered_items` 拉出全團安全欄位，再依 participant 組裝。沿用 `sort_order, id` 與 `line_no` 排序。inner join lines 自然排除 never-submitted／cancelled；沒訂單但 group 存在回 `orders: []`。
- 查詢次數固定，不隨人數增長；重用既有 aggregate assembly 不另造 repository/framework。不要用 `get_public_group` 再逐人讀取，也不要先驗 token 後跨 transaction 讀 orders。token、group 狀態、訂單 lines 都在同 snapshot，避免取消／換 token 穿插造成混合資料。
- 新 SQL 不選 order/line notes、價格／total、client/IP/device、paid/picked、history、submission id；API 只逐欄投影，禁止 serialize domain／management object 或展開既有 EffectiveOrderDto。
- `public.rs` 加 DTO、OpenAPI 註記與 router 註冊，沿用 ClientCtx／ordering_error／404／429 及 no-store/noindex。401/403 是前端防禦處理，不代表要新增登入要求。
- 換 token 後才開始的舊 token 請求應 404；換 token 前已開始的合法 snapshot 可能完成，與現有讀取語意一致，不承諾追溯撤回已交付資料。

## 最小前端方案

1. `PublicOrderPage.tsx` 由 page 控制 tab state；共用 header／截止 banner／權限錯誤放 tab 外。用既有 Tabs/List/Trigger/Content，tab 觸控高度至少 44px，維持窄版容器。新增小型 `PublicOrderOverview.tsx` 負責唯讀列表／空／載入／刷新／錯誤，不重用包含私密欄位的 SavedOrder。
2. 「我要點餐」panel `keepMounted`，保持 participant id、qty、兩種 notes、note 展開及 pending idempotency key。不要用 tab 當 editor key，也不要以 tab 分支條件卸載 editor。新 overview 資料不可回填或 reset 草稿。
3. 傳 `active` 至 OrderStep／OrderEditor；`barShown = active && !readOnly`，render bar 與 ResizeObserver effect 都依同一變數控制，離開 own tab 即 cleanup `--order-bar-h`，返回再量測。不可拿 `readOnly=true` 代替 inactive，否則 editor／截止流程會改變。頁面 padding 只在 own tab 留底部 bar 空間。
4. 注意 keepMounted **不會**停止 effect 或 portal：初次停在 overview 時不得讓隱藏 OrderStep 自動 focus 標題；回 tab 也不搶 tab 焦點。取消／身分確認 Dialog 只可在 own tab 開啟；隱藏時關閉或不 render portal，不能覆蓋 overview；需保留的草稿不受影響。
5. `hooks.ts` 增 `publicOrdersKey(token)`／`usePublicOrders(token, active)`；GET 傳 AbortSignal，`staleTime: 0`、`gcTime: 0`、不重試 4xx。每次啟用 tab 取 fresh，tab active 且 document visible 時每 30 秒刷新，恢復 visible 立即刷新；隱藏文件／離開 tab 停止 interval。用 TanStack Query 既有 enabled/refetchInterval 與小型 visibility listener，不加 websocket／依賴／config。
6. 手動刷新按鈕取新 overview 並 revalidate public group（更新 soldOut／deadline／closed／archived／picker 狀態）；自動週期與啟用 tab 同樣 revalidate group。group queryFn 補 signal，避免過期 group GET 最後覆蓋新狀態。overview metadata 可即時提升 readOnly，但不要以一個較舊回應把已知截止狀態重開；menu 仍以 group GET 為準。既有本機 useNow 截止及 onClosed 保護持續運作，own tab 隱藏也不能停掉。
7. submit／replace／cancel 的 `onSettled` 保留原有效訂單競態處理，再 cancel overview 舊 GET 並 invalidate overview key（inactive 只標 stale，active 取 fresh）。不可在 onSuccess 以草稿自行組整團列表；不論成功失敗都以 settled 後新 read 收斂。重新進 tab 與手動刷新也不可讓較早請求最後覆蓋新 read。
8. 在 page 統一接收 group／overview／effective（含 IdentityConfirm、ReadOnlyOrder）／submit 的 401/403/404，任一權威錯誤即 token-scoped latch：整頁兩 tab 阻斷、卸載 editor/dialog/bar、停輪詢並取消／清除該 token public queries，舊成功回應不能解鎖。可用本 feature 內 callbacks／query error effect，不造全站 auth framework；切 token 需 reset page 狀態與 latch，不能沿用上一 token 草稿。既有 404/403 文案保留，401 同樣要求確認連結存取權。
9. 5xx／網路錯誤非撤權：保留草稿與最後成功 overview，顯示更新失敗／舊資料和最後成功時間，提供重試；不要把失敗當「目前沒有人訂餐」。重新啟用 tab 時可保留舊列表但必須明示正在更新。

## 分階段交付與驗收（Sol 實作，主 agent review）

### 1. 後端與契約

- 修改上述 ports／service／SQLite repo／public routes，必要時 `application/src/lib.rs` export；不改 DB schema、舊逐人 endpoint 或權限模型。生成 OpenAPI 與 frontend alias。
- `backend/crates/api/src/routes/ordering_tests.rs`：valid/malformed/unknown/rotated token、跨團隔離、empty、有效最新訂單／replace／cancel／再訂、同名人及複數餐點排序、closed/archive/deadline/soldOut 可讀；assert 精確巢狀 JSON keys，放入私密 notes/client/settlement fixture 證明 response 無洩漏；驗 no-store/noindex。
- `backend/crates/infrastructure/src/ordering_repo.rs` 測試：transaction snapshot 不混合並發 replace/cancel 的新舊 lines，舊 token 後續失效；query 不逐人迴圈讀 DB。沿用現有測試 fixture 與 SQLite transaction 模式。

### 2. Tabs、資料生命週期與回歸

- 擴充 `PublicOrderPage.test.tsx`，可另建 `PublicOrderOverview.test.tsx`：不用選身分即可看、零狀態、只讀、隱私說明、每次進 tab fresh、手動刷新、visible 30s／hidden 不 poll／恢復即更新、5xx 保留舊資料與成功時間。
- 草稿數量／item note／order note／idempotency 在往返 tab 後保留；overview 無送出列、無底部多餘高度／隱藏 dialog；返回恢復 bar 與 ResizeObserver；截止即清 bar、只顯示持久化訂單。
- 擴充 `PublicOrderConcurrency.test.tsx`：保留現有 read-first/write-first cutoff 案例；加入 overview GET pending 時 POST success/failure/cancel、離 tab、撤權後舊成功最後到的交錯。401/403/404 從各讀／寫入口都阻斷兩 tab 並清 stale cache；5xx 不丟草稿。
- `e2e/tests/ordering.spec.ts`：mobile/desktop 真瀏覽器驗 tab 鍵盤切換與 hidden/inert、固定列沒有遮 overview、切回草稿完整、取消後消失、截止可讀。jsdom 不能代替 fixed positioning／實際高度驗收。

### 3. 指令與最終驗收

```sh
# repository root：生成契約（實作 DTO 後）
make gen-api

# backend/：針對性測試，再完整檢查
cargo test -p api ordering_tests
cargo test -p infrastructure ordering_repo
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings

# frontend/：同資料夾新增測試會一併涵蓋
npm run typecheck
npm test -- --run src/features/public-order

# root：整體驗收；須將生成檔納入預期 diff
make check
make build
# e2e/（已有依賴；CHROME_PATH 依本機可用瀏覽器指定）
npx playwright test tests/ordering.spec.ts
```

本調查未執行產品測試／build／瀏覽器驗收。風險重點是既有 token 本來就具逐人讀單能力，以及 tab keepMounted 後仍存活的 effect／portal；上述測試通過才算完成，無需大型 framework、額外設定或 share 權限擴張。

## Phase 2 實作紀錄（frontend + gen-api）

- 已完整閱讀本 plan；此批僅修改 `frontend/**` 與本紀錄，保留既有 backend Phase 1 工作，未改 backend／e2e、未 commit。新增依賴：無。
- `PublicOrderPage.tsx`：重用 Base UI Tabs，兩個 panel keepMounted；token key 切換時重置 tab／身分／草稿／撤權 latch。共用截止 banner 持續使用本機時鐘，group／overview 任一已知截止狀態會鎖定 readOnly，較晚的 open snapshot 不會重開。
- `PublicOrderOverview.tsx`：不需選身分的唯讀姓名／餐點數量／每人送出時間／最後成功資料更新時間，附隱私說明、empty／loading／manual refresh／error。5xx 保留成功列表與時間並明示舊資料；明確逐欄 render，不使用 SavedOrder，也不展示 notes／付款／client／價格。
- `hooks.ts`：overview 與 group GET 使用 AbortSignal；active + document visible 時每 30 秒刷新，離 tab／hidden 停止並取消 overview，恢復 visible／進 tab 重新讀取。overview 每次讀取也取消舊 group GET 並 revalidate group；POST settled 後取消舊 effective／overview／group GET，再 invalidate。保留原 effective POST success／cutoff 競態保護。
- 撤權使用 feature-local context callbacks（沒有全站 auth framework）：group／overview／effective（含確認身分與 cutoff read）／submit 任一 401/403/404 同步鎖定該 token，卸載兩 tab／editor／portal／bar，取消並清掉 token public queries。pending 成功回應與後續 cache success 都不能解鎖；5xx 不撤權。
- `OrderEditor.tsx`：active 與 readOnly 分離，同一 barShown 控制 render／ResizeObserver／CSS variable cleanup。切 tab 保留數量、兩種 notes、展開狀態、pending idempotency key；截止／卸載發生於 client preparation 期間時不再啟動 POST。
- `focus.ts`：Dialog cleanup 早於 hidden attribute 更新，故 queueMicrotask 後檢查 target 是否仍存在／可見；隱藏時回到當前選中 tab，不回焦隱藏 panel。確認身分／取消 portal 離開 own tab 即關閉，回 tab 不自動重開；OrderStep 只在初次 active mount 對標題 focus。
- 產生碼：以指定 `CARGO_TARGET_DIR=/tmp/opencode/vendor-images-target`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_INCREMENTAL=0` 執行 `make gen-api` 成功；更新 `frontend/openapi.json`、`src/shared/api/schema.d.ts`，並在 `types.ts` 新增 `PublicOrderOverview = PublicOrdersDto` alias（沿用核准的後端 DTO 命名）。

### 實際驗證

- `frontend/ npm test -- --run`：**28 files、299 tests 全過**（最終完整重跑）。
- `frontend/ npm run typecheck`：通過。
- `frontend/ npm run build`：通過，Vite production build 成功。
- `frontend/ npm run lint`：exit 0，**10 個既有 warnings**（shared UI／admin／vendors／ai-import／new-group-order），修改的 public-order 檔案無新增 warning。
- `git diff --check`：通過。
- 測試檔：新增 `PublicOrderOverview.test.tsx`，擴充 `PublicOrderConcurrency.test.tsx`、`PublicOrderPage.test.tsx`。覆蓋無身分可讀／private wire fields 不 render／empty／entry + manual fresh／visible 30s／hidden & inactive pause／visible resume／5xx stale time；草稿 notes 與 retry key 保留／ResizeObserver disconnect 與重掛／CSS variable cleanup／hidden-inert／portal 回焦；hidden own tab 本機截止、overview metadata 關閉、遲到 open 不重開；讀寫各入口 × 401/403/404、clear cache 與舊成功不解鎖；POST success/failure/cancel 與 pending overview GET 晚回、離 tab 與 active 撤權取消 GET。原 cutoff read-first/write-first 測試仍通過。

### 偏差與交接 D

- 產品範圍無刻意偏離；每人送出時間保留。為處理實測的 portal cleanup 順序新增小型 feature-local `focus.ts`，沒有修改 shared dialog 或 tabs primitive。
- 本批未執行／修改 backend tests、root `make check`／release server build，也未改／執行 e2e。Phase 1 後端結果由原負責 session 提供，不在此重複宣稱。
- **D 尚須真瀏覽器驗證 mobile／desktop fixed bar 實際高度與遮擋、tab 鍵盤操作／真 hidden-inert 行為／portal focus。** jsdom 僅驗 DOM、focus callbacks、ResizeObserver 生命週期與 CSS variable 清理，不代表真實 fixed layout 驗收。
- 安全限制未改：舊逐人 endpoint 原本即可透過 token + participant id 讀 notes／價格，本批只保證新 overview DTO／UI 的精簡欄位，不是舊 endpoint 權限收緊。

## Phase 3 D 獨立驗收（主 agent 已核准）

- 只修改 `e2e/tests/ordering.spec.ts`、`e2e/tests/ui-improvements.spec.ts` 與本紀錄；未改產品實作、未 commit/push、未呼叫真 AI。
- 已讀 plan、public-order page/editor/hooks/overview、既有 frontend overview/concurrency tests、backend API overview tests 接點，以及原 ordering/UI E2E。新增 6 個 E2E 案例（每個 mobile/desktop 各跑一次），另擴充既有 deadline 案例。
- 新增永久驗證：未選人即可看 empty／多人多品長字；overview 精確巢狀 wire keys、no-store/noindex、私密 notes 不顯示；tab keyboard、hidden/inert、Base UI panel tab stop、qty/item note/order note/展開狀態保留；真瀏覽器 fixed bar 高度與 footer 不遮擋、離 tab 移除 bar／root CSS variable／page padding、返回恢復。
- ordering 生命週期：entry fresh／manual refresh／visible 30s poll／hidden & inactive 不 poll／visible resume；5xx 保留舊 overview/成功時間與草稿；取消後消失、再送後更新；closed/archived 仍可讀且 own 無寫入列；本機倒數在 overview 期間過期，返回 own 僅持久化訂單。401/403/404 overview 撤權時有 effective GET pending，晚送舊成功也不能恢復任何 tab/dialog/bar。
- 真瀏覽器 portal 焦點由既有 UI regression 驗證初始 focus、focus trap、Escape／保留回焦、取消成功 status focus；tab 切換不偷焦、hidden panel 不在鍵盤路徑由新增案例驗證。外部狀態切 tab 清 portal 的 cancel/identity 分支及各讀寫入口撤權矩陣已有 frontend tests；未為每個分支重造瀏覽器矩陣。

### 完整 checks 與 build

- 指定 `CARGO_TARGET_DIR=/tmp/opencode/vendor-images-target`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_INCREMENTAL=0`。
- 一次完整 `make check` 等價檢查通過：version sync、fmt、clippy `-D warnings`、workspace **350 tests**、OpenAPI 內容同步、`npm ci`、schema 生成同步、frontend typecheck、**28 files / 299 tests**、全新 production frontend build；接續新 embedded release build 通過。
- 不直接使用 Makefile 的 `git diff --exit-code frontend/openapi.json`，因契約是本 feature 預期未提交變更；實際用 export 到暫存檔後 `cmp` 對比目前契約，schema 同樣比較生成前後內容，沒有略過同步驗證。完整 log：`/tmp/opencode/overview-checks.log`。
- `e2e/ npx tsc --noEmit`、`git diff --check` 通過。E2E 指定 `CHROME_PATH=/usr/bin/google-chrome`、`E2E_SERVER_BINARY=/tmp/opencode/vendor-images-target/release/server`，不使用舊 binary。
- 針對性測試初次失敗為測試假設：Base UI active panel 本身也是一個 tab stop，與 `已送出訂單` substring locator 同時匹配 overview region；已修正永久 E2E，不是產品修改。
- **唯一一次完整 E2E：64 passed（2.0m），0 failed／0 retry**；無需重跑。完整 log：`/tmp/opencode/overview-full-e2e.log`。既有 UI regression 320/390/768/1280 的 fixed bar 實測均為 **113px**，footer 位置與 viewport bottom 檢查通過；倒數 hidden-own cutoff 在 mobile/desktop 均通過。
- 8 張 viewport 截圖已保存於 `/tmp/opencode/public-order-overview/`，每個 `mobile-`／`desktop-` 各有 `overview-empty-viewport.png`、`overview-multiple-viewport.png`、`own-draft-viewport.png`、`overview-no-fixed-bar-viewport.png`。原圖亦在 `e2e/test-results/ui-improvements-public-ove-67a66--without-a-hidden-fixed-bar-{mobile,desktop}/`；暫存副本避免後續 Playwright run 清掉證據。D 已目視 mobile/desktop 多人總覽及 mobile own draft，長字換行、多人多品、無 overview 固定列遮擋、草稿固定列與 footer 均正常；主 agent 已親看 mobile/desktop 多人總覽截圖。
- 本次未發現需回報原實作 agent 修復的 fixed layout／portal／dialog focus 產品錯誤。

### 未驗邊界／安全限制

- visibility 用真 Chromium + Playwright clock 與 synthetic `visibilitychange`，不宣稱已驗 OS 背景 tab／手機休眠節流。裝置為 Chromium 模擬 mobile 390×844 與 desktop 1280×800，既有 UI regression 另涵蓋 320/390/768/1280；未跑實體 iOS Safari／LINE WebView。
- 原逐人 endpoint 本就含備註與價格；本次只驗新 overview allowlist，不聲稱整個 token 具備備註保密性。未新增權限模型、未改產品、未使用真 AI。

### 主 agent 最終驗收紀錄

- **Phase 1–3 功能、測試與畫面驗收完成，主 agent 核准。** 主 agent 已親看 `mobile-overview-multiple-viewport.png`／`desktop-overview-multiple-viewport.png`，審閱 read transaction repo SQL、public response 契約、`PublicOrderOverview`／hooks／`TokenPage` 與 keepMounted 實作，並再次確認 `git diff --check` 通過。
- 保留上述未驗邊界：真機 LINE 與真 OS visibility 未驗；隱私承諾僅指 overview，不修改舊逐人 endpoint。
- 本次結案僅更新本文件，未改產品或測試、未重跑檢查、未 commit/push。
