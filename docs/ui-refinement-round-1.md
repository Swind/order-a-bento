# UI 精緻化第一輪：設計與分階段任務

狀態：**P0–P4 自動化與畫面驗收已獲主 agent 核准；LINE 真機／IME／鍵盤／非零 safe area／原生分享／實際 zoom 仍待驗，不代表全面真機完成**。日期：2026-10-29。

本輪只處理團詳情資訊層級、公開點餐固定列、相關表單尺寸。沿用暖白／深綠、既有字型、Tailwind、Base UI、React 與 TanStack Query，不新增套件、API 或後端變更。

## 1. 依據、現況與驗證能力

已閱讀 `README.md`、`docs/design.md`、`docs/plan.md`、`docs/local-groups-plan.md`、`docs/ui-improvement-plan.md`、`frontend/LINE_CHECKLIST.md`，以及下述元件、hooks、測試與設定。專案與祖先目錄未發現適用的 `AGENTS.md`；已讀 ponytail、docs skills。本環境沒有 docs connector，依任務將文件存為 repository Markdown。

既有 `docs/ui-improvement-plan.md` 是上一輪的實作與驗收歷史，不重新打開其已完成 tasks。本文件是額外的第一輪視覺精緻化，**不能退回上一輪修正前的行為**。`docs/plan.md` 的舊 Sonnet 分工不適用本輪，改依第 6 節。

### 1.1 已實際調查／執行

| 項目 | 2026-10-29 結果與界線 |
| --- | --- |
| 環境 | Node v22.17.0、npm 10.9.2；frontend／e2e 依賴已存在；`backend/target/release/server` 存在，但未確認其內嵌前端是否為最新 |
| 元件測試 | 在 `frontend/` 執行 `npm test -- --run src/features/group-orders/GroupOrderDetailPage.test.tsx src/features/public-order`，5 檔、74 項通過 |
| 瀏覽器可用性 | 透過現有 `e2e/node_modules` 的 Playwright 實際啟動 `/usr/bin/google-chrome`（150.0.7871.128）；390×844 viewport 的簡單頁面可渲染並量測 44px 按鈕。這只證明瀏覽器可用，不是產品 UI 驗收 |
| 自動化設定 | Vitest + jsdom + Testing Library + MSW；jsdom 不驗證真實尺寸。Playwright `mobile` 為 Chromium + LINE UA，不是 iOS Safari／LINE 真機；`desktop` 已包含 ordering、management、sharing-groups 與 ui-improvements specs |
| E2E 隔離 | `e2e/playwright.config.ts` 用 :3990、獨立臨時 SQLite、CLI 建測試管理員、AI_DEMO，清空真實模型 key。不可同時跑兩個 E2E runner（會共用 port／DB 目錄） |
| 設計調查當時未執行 | 完整 build／lint／E2E、產品畫面截圖、真機測試；P0 後續 build／產品 before 證據見第 9 節，不代表本輪整體驗收 |
| 真機能力 | 此 session 沒有連接 iOS／Android／LINE 真機或桌面互動瀏覽器工具；可用 shell 執行現有 Playwright。軟鍵盤、IME、Home indicator safe area、原生 LINE 分享仍須另驗 |

## 2. 目標與不做事項

1. 團詳情從「多行 metadata + 大工具區」收斂成「標題／狀態 → 精簡資訊 → 分享工具 → tabs → 清楚摘要」。
2. 公開點餐固定列只服務「草稿合計 + 送出／儲存」，不常駐破壞性取消操作。
3. 本輪涉及表單的單行互動以 44px 為共同基準，保留 48px 主 CTA、多行 textarea 和 44×44px 數量控制。

不做品牌重製、色票／字型更新、全站導航／dashboard 重排、暗色模式重設、動畫系統、全站元件尺寸 migration、跨分頁同步或 query／mutation 重構。不修改訂單計價、權限、快照、idempotency、截止判定及歷史紀錄語意。

## 3. 團詳情設計

### 3.1 檔案與資訊層級

主要檔案皆在 `frontend/src/features/group-orders/`：

| 檔案 | 預期變更 |
| --- | --- |
| `GroupHeader.tsx` | 保留 TitleEditor／狀態 badge；metadata 改緊湊排版；不改現有管理操作與 mutation |
| `SharePanel.tsx` | 收斂邊框／padding 與預設工具列；完整連結及次要入口放原生 details |
| `GroupOrderDetailPage.tsx` | 配合區塊間距；保留 collecting 計算、分享上下位置與 URL tab |
| `StatsTab.tsx` | 現有摘要改有標籤的數字區，不改品項雙行排版或 summary query |
| `GroupOrderDetailPage.test.tsx` | 更新摘要語意及分享入口斷言，保留階段／權限／操作回歸 |

### 3.2 Header

- 標題仍為唯一 h1、24px、允許完整換行；狀態 badge 不擠壓標題。修改標題的 icon button 保留 44×44px。
- 店家名稱為次層重點，電話維持完整可讀。歸屬（個人／共用群組）作為較弱的 metadata；不移除權限相關訊息。
- 截止時間與送餐地址採可換行的 metadata 區：手機單欄，`sm`（640px）以上可兩欄；字級 14px。地址、群組名、備註不 truncate，長字串須能斷行。
- 一般狀態只顯示一份截止資料；「更多操作」內現有 DeadlineEditor 是編輯介面，可保留其當前值與修改入口。
- 備註使用 `whitespace-pre-wrap`／換行，不另做可收合介紹卡。未設定截止／地址仍有現行明確文案。
- 主要管理操作仍是「關閉收單」或「重新開放」；「更多操作」維持原生 details，取消目前 `flex-1` 造成的不必要大片工具框，可採內容寬度並允許展開面板佔滿一列。不能為精簡而把這些操作改成無標籤 icon。
- 封存與無管理權限的提示保留；不出現修改、關單、輪替等可操作入口。收單階段不是管理權限。

### 3.3 分享工具列

- 收單中仍在 tabs 前，但以緊湊工具列呈現：主操作「複製連結」、outline「分享到 LINE」。用小間距與低裝飾取代大型卡片感；不得縮小 44px 觸控高度。
- 「開啟訂購頁」和完整 URL 移至同一個原生 details（建議文案「連結與其他操作」）。預設只顯示此 44px 高 disclosure，不顯示長 URL。320px 下工具可自然換行，不硬塞一行。
- 自動複製失敗仍須展開 details、focus 並 select 唯讀 input，提示手動複製；保留成功 toast、LINE URL 產生方式、`target="_blank"`／`rel="noopener noreferrer"`。連結輪替後複製及 input 都使用最新 props。
- 截止、closed、archived 仍用既有「分享與訂購連結」外層 details 放在 tabs 內容之後；內部可沿用相同 SharePanel，容許次要連結再展開，不新增 presentation state 或自訂 dropdown。
- 分享階段沿用 `status === open` 且未過截止時間。重新開放但截止仍在過去時，不自動恢復上方分享；切換階段不能跳 tab 或重設 query string。

### 3.4 統計摘要

- 摘要仍只在「統計」tab 清單上方呈現，不新增每個 tab 都查詢的全域摘要；領取＆繳費現有應收／已收摘要維持原樣。
- 同一個低對比 muted 容器內分三組，以 `dl`／`dt`／`dd` 表達：**已訂人數、總份數、總金額**。未訂人數是已訂人數下的輔助文字，例如「未訂 1 人」。
- 使用 `s.participants.ordered`／`notOrdered`、`s.totalQuantity`、`s.totalAmount`。不可將參與者總數當作已訂人數，亦不從目前菜單重算總額。
- 320／390px 用兩欄：人數、份數在第一列，總金額佔第二列；`sm` 以上三欄。數值 20–24px、`tabular-nums`，標籤 14px。總金額是最高視覺權重，大金額完整顯示，可換行但不可溢出。
- 零訂單也呈現 0 人／0 份／$0 及既有空狀態。首次 loading 不假裝是零；error 保留重試，不能因排版改動讓舊 cache 冒充已驗證成功資料。
- 品項清單、售完 label 44px 可點區、唯讀限制、訂單文字預覽／複製行為不變。摘要文案由「共 N 個」改為明確「總份數 N 份」僅限本摘要，不全站替換「個」。

## 4. 公開點餐固定列與取消操作

### 4.1 範圍與佈局

主要修改 `frontend/src/features/public-order/OrderEditor.tsx`；只為焦點或測試必要時調整 `PublicOrderPage.tsx`。`hooks.ts`、`lib.ts` 的送出協調及 dirty 比較原則上不動。

- 固定列保留現有兩層：上層「共 N 個」與醒目草稿總額，下層 48px 全寬主 CTA。不改成金額與按鈕同排，避免 320px／大金額擠壓。
- 固定列內不再有取消入口、確認文案或第二個 CTA。可給此容器可辨識的 accessible name（例如「訂單送出操作」），利於語意與測試定位。
- 有效訂單摘要改成具名稱的 section（建議「上次送出的訂單」），保留 server 快照的品項、時間與提示，在摘要底部放 44px ghost／destructive 文字「取消我的訂單」。即使草稿已清空或尚未送出修改，入口仍存在，取消的是有效訂單，不是清除草稿。
- 無有效訂單／已取消不顯示取消入口；readOnly 沿用 SavedOrder，不新增任何取消／送出控制。

### 4.2 確認行為：沿用 Base UI Dialog

取消入口離開固定列後，以既有 `shared/ui/dialog.tsx` 代替搬移 inline `role="alertdialog"`，避免手機確認區擠成單行，也提供正確焦點管理。不新增共用確認 abstraction。

- DialogTitle「確認取消訂單」，DialogDescription「確定要取消你的訂單嗎？尚未送出的修改也會在取消成功後清除。」；兩個按鈕：「保留」（outline）、 「確定取消」（destructive）。
- 初始焦點在「保留」；Tab 限制在 Dialog；非 busy 時 Escape／背景關閉等同保留，回到摘要取消入口，草稿不變。
- 確定後只呼叫既有 `send([], "", true)`，沿用同步 `sending.current`、busy、client info 準備、idempotency key、cache 和錯誤處理。**不得另造 cancel mutation**。
- busy 時停用兩個 Dialog 按鈕與摘要入口，禁止 Escape／背景關閉（`onOpenChange` guard）；不顯示 close icon（使用 `showCloseButton={false}`），以兩個明確操作處理關閉。確認按鈕可顯示「取消中…」及 `aria-busy`。
- 保留目前共用 busy 導致固定列主 CTA 顯示「送出中…」的行為；取消時 modal 已使其不可互動，不為文案新增另一套 sending state。
- 取消成功才關閉並依 server response 清空數量及備註；入口此時消失，需把焦點移至穩定的訂餐狀態提示（加 `tabIndex={-1}` 與 ref 即可），避免回到已移除 trigger／body。失敗保留 Dialog、有效訂單和草稿，允許重試或保留，沿用錯誤 toast。
- 截止／關閉、換人、403／404 導致 editor 卸載時，Dialog 一併消失，不留下 overlay；既有 readOnly／權威性失效分支優先。不得重設錯誤旗標來保持對話框。

### 4.3 狀態契約（不得回歸）

| 狀態 | 固定列 | 摘要／取消 |
| --- | --- | --- |
| 無有效單、未選餐 | 草稿 $0；送出訂單停用 | 無取消 |
| 無有效單、已選餐 | 草稿合計；送出訂單可用 | 尚未送出 |
| 有效單、未修改 | 草稿合計；訂單已送出停用 | 上次送出快照；可取消 |
| 有效單、有修改 | 草稿合計；儲存修改 | 顯示未送出狀態；可取消 |
| 有效單、草稿清空 | $0；儲存修改停用 | 仍能取消有效訂單，不能用空草稿冒充一般送出 |
| 選中售完品項 | 停用送出，保留衝突提示與減量 | 有有效單時仍能取消 |
| client info 準備／mutation busy | 送出中、停用，所有可編輯欄鎖定 | 取消入口與確認停用、防重入 |
| 取消成功 | 草稿清空；送出訂單停用，可重新選餐 | 已取消時間；無取消入口 |
| 取消失敗／暫時性更新 500 | 原草稿及狀態保留 | 可重試；不能誤當取消成功 |
| 截止／closed／archived | 無固定列 | fresh server 快照／未訂／已取消，無取消 |
| 403／404 權威性失效 | 移除固定列及所有訂餐操作 | 不沿用可操作 cache |

### 4.4 Safe area 與 padding

- 保留 `barRef`、ResizeObserver 對實際 `offsetHeight` 的量測、`--order-bar-h` 發布與 cleanup；不是改成固定 100px padding。
- 保留固定列 `pb-[max(0.75rem,env(safe-area-inset-bottom))]`，以及 PublicOrderPage 的 `pb-[calc(var(--order-bar-h,0px)+1rem)]`。Dialog 不計入固定列高度。
- 高度改變後重新量測；readOnly／換人／權限失效／卸載移除 CSS variable。最後備註與隱私告知能捲到固定列上方；非零 safe area 不扣兩次。
- 保持 `PublicLayout.tsx` 的 max-w-md 與全頁固定列背景，不引入 VisualViewport／鍵盤偵測 JS。真機發現鍵盤問題再提出局部修正，不推測已解決。

## 5. 表單尺寸與可及性規範

### 5.1 採用局部補齊，不改全域預設

目前 `shared/ui/input.tsx` 與 `button.tsx` 的 default 是 32px，但主要訂餐／開團表單已在使用端覆寫為 `h-11`；Textarea 為 `min-h-16`、16px。第一輪**不直接更改所有 shared primitives 的預設尺寸**，也不新增 FormControl wrapper／尺寸 token 系統。

| 類型 | 本輪規範 | 套用位置 |
| --- | --- | --- |
| 單行 Input、native select | 44px、16px 字、rounded-lg、水平 padding 約 10–12px、可見 focus ring；`w-full min-w-0` 視容器保留 | GroupHeader 編輯、SharePanel URL、公開搜尋／備註、NewGroupOrderPage 設定 |
| Textarea | 至少 64px、多行、高度可成長；16px、rounded-lg、py-2；不可硬設 44px 或全移除 resize | 公開訂單備註、開團備註、統計預覽 |
| 一般／次要按鈕與 disclosure | min 44px 高，icon-only 44×44px；長文可換行時用 min-h 而非裁切 | 本輪相關操作、取消 Dialog、分享 disclosure、更多操作 |
| 主 CTA | 維持 48px，16px，全寬依現行配置 | 公開送出、開團 |
| Switch 等複合控制 | 不放大 Switch 軌道；維持含文字 label 的 44px 可點區 | 統計售完控制 |

具體最小修整：

- `features/sharing-groups/ScopeSelect.tsx`：保留原生 select，將現有 rounded-md 對齊 rounded-lg，明訂 text-base／min-w-0，補與既有 Input 一致的 focus-visible ring；此共用 select 的名單／店家／開團使用處要 smoke test，但不重排其頁面。
- `features/delivery-addresses/DeliveryAddressSelect.tsx`：select 已為 44px，不重做；補 focus／disabled 視覺一致性；「重試載入地址」目前未覆寫 32px，補 44px。
- 其他上述表單既有 h-11、h-12 與 textarea 高度大多符合：以驗證為主，不為「看起來有改」而搬動 props 或批次清理 class。
- 本輪新增的取消 Dialog 無 close icon；既有其他 Dialog／Sheet 的小 close icon 不在本輪全站改造範圍。

### 5.2 Responsive／accessibility 驗收

- 320、390、768、1280px 下無整頁水平溢出；長標題／地址／群組名／品名及 $2,000,000 完整可讀。保留 tabs 自有橫向捲動容器。
- 維持現有暖白、深綠與語意色 token，不用降低 opacity 讓 metadata／disabled 之外的資訊難以閱讀。狀態有文字，不只靠顏色。
- Input／textarea／select 有可辨識 label 或既有 aria-label；error、disabled、readOnly、required／maxLength 行為不變。使用 native select，不換自製 listbox。
- 桌面鍵盤順序符合視覺順序；details 用 Enter／Space、Tab 可進入內容；Dialog 焦點不外漏，保留／取消成功有確定返回點；主 CTA focus-visible 不被移除。
- 200% 縮放／窄 viewport 仍能閱讀及操作，不以 overflow-hidden 掩蓋金額；驗證實際 bounding box，不只斷言 Tailwind class。

## 6. Phases／tasks 與分工

**所有實作、補測、整合測試與缺陷修復交給 GPT-6.1 Sol subagents；主 agent 僅決定設計、分派與驗收，不修改功能程式碼或替 subagent 修錯。** 主 agent 已核准啟動並委派，模型為 `openai/gpt-6.1-sol`，採共用工作樹與不同檔案所有權；不因本文件而額外生成 subagent。

建議以少量明確責任的 subagents 執行：A（團詳情）、B（公開點餐）、C（表單補齊）、D（獨立整合／驗證）。主 agent 驗收不過，退回原責任 subagent 修正，再由 D 複驗。不得自動 git add／commit／push。

| Phase／Task | 責任／檔案 | 依賴 | 可驗收交付 |
| --- | --- | --- | --- |
| P0-T1 設計核准／基準封存 | 主 agent 核准；A/B/C 各自確認工作樹與既有測試，D 保存 before 截圖 | 無 | 確認第 8 節；基準命令／結果／截圖位置明列；不把本文件當完成證明 |
| P1-T1 Header 壓縮 | A：GroupHeader、GroupOrderDetailPage | P0 | 長資料換行、管理及唯讀操作完整；無重複一般截止行、無大型更多操作空框 |
| P1-T2 分享工具與統計摘要 | A：SharePanel、StatsTab、GroupOrderDetailPage.test | P1-T1 | 三組有標籤摘要、手機兩欄／總額全寬；分享失敗 fallback、階段與 URL tab 測試通過 |
| P2-T1 固定列／摘要取消 | B：OrderEditor、PublicOrderPage.test | P0；可與 P1 平行 | 固定列僅合計＋主 CTA；取消只在有效訂單摘要；狀態矩陣覆蓋 |
| P2-T2 取消 Dialog 與焦點 | B：同檔，必要時 PublicOrderPage | P2-T1 | 保留／Escape 返回、busy 禁止關閉／重入、取消成功焦點、失敗保留草稿／可重試；既有競態測試全過 |
| P3-T1 尺寸局部補齊 | C：ScopeSelect、DeliveryAddressSelect，必要的既有測試 | P1/P2（避免同時修改同檔） | 44px retry、native select 外觀／focus 一致；其餘符合項不重寫；列明實際改動與刻意保留 |
| P4-T1 瀏覽器回歸 | D：擴充 e2e/tests/ui-improvements.spec.ts、ordering.spec.ts，必要時 management.spec.ts | P1–P3 | 四種 viewport、位置／尺寸／overflow／Dialog 焦點、全流程與 before/after 截圖 |
| P4-T2 獨立檢視／完整檢查 | D：測試、此文件驗收紀錄；真機 checklist 若有證據才更新 | P4-T1 | 所有命令結果、缺陷清單、重測結果與未驗證界線；主 agent 據此最終驗收 |

### 6.1 檔案所有權與依賴約束

- P1／P2 可平行，只改各自 feature 及測試，不改共享 UI default、index.css 或 E2E。
- C 原則上只修改兩個 select 元件；若發現必須更動 A/B 檔案，等其完成後交接再改，不互相覆蓋。
- D 為 E2E 唯一寫入者；新增覆蓋優先擴充已在 mobile／desktop testMatch 的 specs，不隨意新增 desktop 不會執行的 spec。
- D 發現功能缺陷應寫出重現證據並交 A/B/C 修正，自己負責永久回歸與複驗。若使用不同 worktree，由 GPT-6.1 Sol 整合 subagent 整合，主 agent 不手動解功能衝突。
- 每次交接附：diff 檔案、完成 task、執行命令／結果、截圖／trace、偏離設計、既有 warning、未驗證項。禁止以「應該通過」代替實際結果。

## 7. 測試策略與可執行指令

### 7.1 必要測試覆蓋

**Vitest／MSW：**

- GroupOrderDetailPage：收單中／自然截止／closed／archived／重新開放（含過期 deadline）、管理員／唯讀、tab 保留、summary zero／loading／error、長名稱與正確人數／份數／總額；copy 成功與失敗展開選取、rotate 後新 URL。
- PublicOrderPage：取消入口屬於已送出摘要且不在固定列；無單／已取消／readOnly 不存在；dirty／草稿清空仍可取消；保留不更改草稿；取消 payload 仍空 lines／空 note；成功可再訂、失敗保留草稿與有效單。
- Dialog：保留初始焦點／Escape 返回；client info 延遲及 API pending 期間兩次確認只發一次請求、不能背景／Escape 關閉；取消成功焦點不落 body。busy／readOnly 防護不可只依賴視覺 disabled。
- 不刪減 `PublicOrderConcurrency.test.tsx`：GET／POST 競態、截止重新驗證失敗、sold_out → GET 500 保留草稿、403／404 阻斷均須持續通過。
- Select：選項與原 value／onChange／disabled／error retry 不變；使用既有 NewGroupOrderPage、SharingGroupsPage、Rosters、Vendors、DeliveryAddresses 測試回歸即可，不為 CSS 建立大量 snapshot。

**Playwright：**

- 重用 ui-improvements 的 45 人長名單、長名稱、多行備註、$2,000,000 fixture，在四種寬度驗證摘要數值、區塊順序與無 overflow，量測主要互動高度／icon 寬高。
- 檢查收單中分享工具預設緊湊、次要入口展開可用；closed／archived 分享仍在內容後，URL tab 不變。
- 公開頁先送出 → dirty → 摘要取消 → 保留 → 再取消成功 → 重新選餐／送出 → 關閉後唯讀；固定列 bounding box 內只含合計及主 CTA，取消 Dialog 不在固定列 DOM。
- 捲到最底，隱私告知的底緣在固定列上緣之上；量測 `--order-bar-h` 與固定列高度相符；取消入口移除後固定列不增高；readOnly／換人／403／404 後 variable 清除。
- 真正鍵盤驗證 Dialog 初始焦點、focus trap、保留返回及成功返回；200% 縮放以瀏覽器實測或等效縮小 CSS viewport 檢查 reflow，不能僅改 deviceScaleFactor 宣稱完成 zoom。
- 截圖至少包含 management-collecting、management-summary、public-dirty、cancel-confirm、public-cancelled、readonly，記錄 viewport／project，保存至 Playwright `info.outputPath()`（git ignored）。截圖須由驗收者實際檢視。
- 此輪 SharePanel 把 URL 放進 details 後，檢查既有 spec 用 `input[readonly]` 取得 share URL 的方式仍有效；若改 locator，採語意 label／fixture，不放寬斷言來掩蓋回歸。

### 7.2 命令

各指令依註明工作目錄分別執行。依賴已安裝時不必每個 phase 重跑 npm ci；缺依賴才於 `frontend/`、`e2e/` 各執行 `npm ci --prefer-offline --no-audit --no-fund`。

```sh
# frontend/：A 的最小檢查
npm test -- --run src/features/group-orders/GroupOrderDetailPage.test.tsx
npm run typecheck

# frontend/：B 的最小檢查（包括競態）
npm test -- --run src/features/public-order
npm run typecheck

# frontend/：C 的回歸
npm test -- --run src/features/group-orders/NewGroupOrderPage.test.tsx src/features/sharing-groups src/features/delivery-addresses src/features/rosters src/features/vendors
npm run typecheck

# frontend/：D 的完整檢查
npm run lint
npm run typecheck
npm test -- --run
npm run build
```

```sh
# repository root：先重新建置，E2E 不能使用舊 release 的內嵌前端
make build

# e2e/：局部瀏覽器回歸，勿與其他 E2E runner 同時執行
CHROME_PATH=/usr/bin/google-chrome npx playwright test tests/ui-improvements.spec.ts tests/ordering.spec.ts tests/management.spec.ts --project=mobile --project=desktop

# repository root：最後完整 E2E（自行重新 build 及安裝 e2e dependencies）
CHROME_PATH=/usr/bin/google-chrome make e2e
git diff --check
git status --short
```

若使用 Playwright 自帶 Chromium，移除 CHROME_PATH 並先確認對應 browser 已安裝。新環境的 browser install 必須記錄實際使用版本，不假設系統 Chrome 一定存在。本輪無 API 改動，不需 `gen:api`、AI eval 或完整後端檢查；若實作者發現必須改 API／後端，先停下回報超出範圍，而非逕自擴大。

### 7.3 真機與完成判定

- iOS 與 Android 的 LINE 內建瀏覽器，各驗備註輸入／IME、鍵盤開關、safe area、取消 Dialog、捲到底隱私文字、分享 fallback 與 App 切回；使用 `frontend/LINE_CHECKLIST.md` 記錄機型／OS／LINE 版本與證據。
- 自動化可通過並註記「真機待驗」，但不能宣稱 safe area／LINE 全面完成。不能用 CSS env 在桌面等於 0 或模擬 UA 代替真機結果。
- 本輪可交付門檻：三項目標達成、狀態契約／權限／競態未回歸、相關與完整前端測試／build／E2E 通過、diff 範圍正確、所有未驗證項明列。任何不可用環境均列阻礙，不勾成通過。

## 8. 主 agent 核准決議（2026-10-29）

1. **設計與啟動**：已核准本文件設計，先由 D 封存 P0 before，之後依 P1–P4 執行；核准不等於驗收完成。
2. **摘要與分享預設**：統計摘要只在 stats tab；「開啟訂購頁」與完整 URL 放次要原生 details，頂層保留複製／LINE。
3. **取消確認方案**：採 Base UI Dialog，依第 4 節維持 busy、取消 payload、焦點及錯誤契約。
4. **表單範圍**：僅局部 select／retry 尺寸與 focus 補齊，不改全域 32px defaults，不做全站 migration。
5. **派工能力與模型**：主 agent 已委派 `openai/gpt-6.1-sol`；共用工作樹，A/B/C/D 依第 6.1 節分檔所有權，不 commit。
6. **真機門檻**：目前無設備，iOS／Android LINE 明確待驗；自動化與產品截圖不能替代軟鍵盤、IME、非零 safe area、原生分享及 App 切回驗證。

## 9. 執行／驗收紀錄

| 日期 | Phase／責任 subagent | 命令／證據 | 結果／限制 | 主 agent 驗收 |
| --- | --- | --- | --- | --- |
| 2026-10-29 | 設計調查 | 相關 Vitest 74/74；Chrome 啟動與尺寸 smoke；見 1.1 | 僅新增本文件；未改功能程式、未 commit；產品瀏覽器／真機驗收尚未執行 | 待核准 |
| 2026-10-29 | P0-T1／D 獨立 before | repository root：`make build`；e2e：`CHROME_PATH=/usr/bin/google-chrome npx playwright test --config=/tmp/opencode/ui-round1-before.config.ts` | build 通過（Vite 2522 modules、release server 重建）；最小產品流程 mobile／desktop 2/2 通過、3.9s；10 張 before 截圖，路徑見下；沒有修改功能或永久 E2E | P0 已封存，交主 agent；P1–P4 未驗收 |
| 2026-10-29 | P1／A | 主 agent 交接：detail tests 42 項、typecheck、lint 通過，已審功能 diff；D 完整測試與真瀏覽器再驗 | Header、SharePanel、StatsTab 與 detail 測試已交付；D 未修改這些檔案 | 主 agent 已核准自動化與畫面驗收 |
| 2026-10-29 | P2／B | 主 agent 交接：public-order 43 項、typecheck、兩檔 lint 通過，已審 OrderEditor diff；D 真瀏覽器焦點再驗 | 取消位置、Base UI Dialog／焦點契約已交付；D 未修改功能檔 | 主 agent 已核准自動化與畫面驗收；真機仍待驗 |
| 2026-10-29 | P3／C | 主 agent 交接：44 項、typecheck 通過，已審兩個 select 局部 diff；D 加真瀏覽器尺寸／retry smoke | ScopeSelect／DeliveryAddressSelect 局部 class 與 44px retry 已交付 | 主 agent 已核准自動化與畫面驗收 |
| 2026-10-29 | P4-T1/T2／D | frontend：lint、typecheck、full Vitest、build；root：make build；e2e：tsc、完整 Playwright；詳 9.3 | 26 檔／220 Vitest 通過；最終完整 E2E 29/29，53.9s；80 張 after 圖；固定列 161→113px | 主 agent 已核准 P0–P4 自動化與畫面驗收；待驗界線見 9.5 |

### 9.1 P0 before 證據與交接

- 執行前 `git status --short` 僅有 `?? docs/ui-refinement-round-1.md`，未有功能變更；先 `make build` 確保 release 內嵌當前 frontend/dist，而非沿用舊 binary。
- 暫存設定 `/tmp/opencode/ui-round1-before.config.ts`，暫存測試 `/tmp/opencode/ui-round1-before-tests/before.spec.ts`。重用既有 E2E config、helpers、獨立 :3990 SQLite／測試帳號、清空模型 key 的 server env 與 teardown；未讀取任何秘密 env。初次啟動因暫存 config 的相對 `globalTeardown` 路徑失敗（尚未開始測試），改用既有 teardown 絕對路徑後 2/2 通過，未改 repository 設定。
- 瀏覽器 `/usr/bin/google-chrome`（150.0.7871.128）；mobile：390×844 CSS px、DPR 2、Chromium＋LINE UA；desktop：1280×800 CSS px。這不是 Safari／LINE 真機。
- 使用實際後端 fixture：3 位參與者、長店名／品名、每份 $100,000；先有效送出 20 份／$2,000,000，再減為草稿 19 份／$1,900,000 並加入兩行未送出備註。管理統計確認 $2,000,000、公開已送出 CTA 停用、dirty status 正確；兩 viewport 管理與公開頁均無水平 overflow。
- before 結果保留舊 UI：大型分享卡、展開前直接顯示開啟訂購頁、舊統計「共 20 個」、取消入口仍在固定列。實測固定列 mobile `y=683, h=161, bottom=844`；desktop `y=639, h=161, bottom=800`，兩者 CSS `--order-bar-h=161px`，列內文字「共 19 個 $1,900,000 儲存修改 取消我的訂單」。這是變更前證據，不是新版驗收。
- 截圖根目錄：`/tmp/opencode/ui-round1-before-results/`。以下兩個子目錄各有同名 5 張圖：
  - `before-P0-real-product-before-mobile/`
  - `before-P0-real-product-before-desktop/`
  - `management-collecting.png`（全頁管理詳情）、`management-summary.png`（統計 viewport）、`public-submitted.png`（全頁有效已送出）、`public-dirty.png`（全頁 dirty）、`public-dirty-fixed-bar.png`（捲底固定列 viewport）。
- D 已實際開圖檢視兩 viewport 的 `management-collecting.png`、`management-summary.png`、`public-dirty-fixed-bar.png`；可見真實產品內容／草稿金額／舊取消列，非空白 smoke 圖。其餘兩種全頁圖已保存供 P4 對照。
- 本階段刻意不重跑完整 E2E baseline、不改永久測試、不做功能修正、不 commit。P4 仍需完整前端檢查、永久 E2E 覆蓋、四寬度／Dialog／after 截圖；真機仍待設備，不更新 LINE checklist 為通過。

### 9.2 Phase／task 實際狀態

主 agent 已核准下列 P0–P4 的自動化與畫面驗收；未驗環境仍獨立列為待驗，不因核准而視為通過。

| Task | 狀態 | 實際證據 |
| --- | --- | --- |
| P0-T1 | 已完成，主 agent 已核准 | 主 agent 核准；重建內嵌前端；2 個最小產品流程、10 張 before 圖 |
| P1-T1 | 已完成，主 agent 已核准 | A 已交付；D 320／390／768／1280 長標題、長地址、兩行備註實際畫面無 overflow；更多操作不再佔滿空框 |
| P1-T2 | 已完成，主 agent 已核准 | 42 項 detail 測試（主 agent 交接）；D 驗三組 dl 數據／窄版兩欄、大金額完整、摘要僅 stats、分享 details／鍵盤／失敗選取、closed／輪替連結 |
| P2-T1 | 已完成，主 agent 已核准 | D 驗有效摘要內取消、固定列僅一個主 CTA、dirty／清空草稿可取消、取消後可再訂、關閉唯讀無控制 |
| P2-T2 | 已完成，主 agent 已核准 | D 真鍵盤驗保留初始焦點、Tab／Shift+Tab 限制、Escape／保留回入口、busy 按鈕停用／Escape／背景不可關閉、單次空 lines 且省略空 note、成功焦點到 status、500 失敗保留資料並可重試；完整 public-order 競態測試包含在 220 項內 |
| P3-T1 | 已完成，主 agent 已核准 | D 四寬度量測 native select 44px／16px、可選 value／focus ring；rosters／vendors 個人↔群組操作；開團地址 500 disabled、44px retry、成功恢復及選址 |
| P4-T1 | 已完成，主 agent 已核准自動化與畫面 | 永久修改既有 ui-improvements／ordering／management specs；四寬度、兩 projects、最終完整 E2E 29/29；80 張 after 圖，實際檢視見 9.4／9.5 |
| P4-T2 | 已完成，主 agent 已核准自動化與畫面 | lint／typecheck／220 tests／build／embedded build／E2E TS／29 E2E／diff check 通過；首跑測試問題及修正完整揭露於 9.3 |
| iOS／Android LINE 真機 | **待驗：無設備** | 軟鍵盤、IME、非零 safe area、原生 LINE 分享、App 切回、真實 Safari／LINE engine 均未驗；未勾 LINE checklist |
| 實際瀏覽器 zoom | **待驗** | 已驗窄 CSS viewport reflow，未操作真實 200% zoom；不得視為同一項已完成 |

### 9.3 D 實際命令、結果與缺陷界線

| 工作目錄 | 命令 | 實際結果 |
| --- | --- | --- |
| frontend | `npm run lint` | exit 0，0 errors、10 warnings（見下） |
| frontend | `npm run typecheck` | exit 0 |
| frontend | `npm test -- --run` | 26 files／220 tests 通過，12.79s |
| frontend | `npm run build` | TypeScript／Vite 通過，2522 modules |
| repository root | `make build` | 再建 frontend/dist 並 `cargo build --release -p server` 成功；確認本輪新 UI 已內嵌，未沿用 P0 binary |
| e2e | `npx tsc --noEmit` | exit 0（準備階段與最終整合均執行） |
| e2e | `CHROME_PATH=/usr/bin/google-chrome npx playwright test`（首跑） | 17/27 通過，10 failed；均為測試時機／locator 問題，見下，未修改產品 |
| e2e | `CHROME_PATH=/usr/bin/google-chrome npx playwright test tests/ui-improvements.spec.ts --grep 'UI regression\|share fallback'` | 修正測試後局部 10/10 通過，11.1s |
| e2e | `CHROME_PATH=/usr/bin/google-chrome npx playwright test tests/ui-improvements.spec.ts --grep 'UI regression\|native directory'` | 加入 select 實際尺寸／disabled／retry 後局部 10/10 通過，14.1s |
| e2e | `CHROME_PATH=/usr/bin/google-chrome npx playwright test`（最終完整） | **29/29 通過，53.9s**；mobile 16、desktop 13；原設定 4 workers；沒有第二個 runner 同時佔用 port／DB |
| repository root | `git diff --check` | 通過；未 commit，未覆寫 A/B/C 功能檔 |

- 首跑 8 個四寬度流程的 Dialog 按鈕因 opening transform 量到 43.56–43.96px，而非 settle 後 44px；改用 `expect.poll` 等穩定 bounding box，仍嚴格要求 ≥44px，不降門檻。另 2 個失敗是在 modal 開啟期間用預設 role locator 查被 inert／aria-hidden 的背景摘要；改成「保留」關閉後再檢查有效單及空草稿，仍驗證原資料保留。這是 Base UI 預期背景隔離，不是產品缺陷。
- 準備階段依主 agent 指正：空 trimmed note 的 payload 必須省略 `note`，測試使用 `not.toHaveProperty("note")`，沒有改送出協調。長品名／$2,000,000 不碰撞檢查在取消前執行；取消後再訂的 $100,000 檢查仍保留，不以小金額取代原覆蓋。
- 首跑 failure 截圖／trace 封存於 `/tmp/opencode/ui-round1-e2e-first-results/`；最終全部通過，預設 retain-on-failure 不產生成功 trace。測試目前未發現需退回 A/B/C 的產品問題；這不表示未覆蓋環境不存在問題。
- lint 的 10 個既有 warnings：`shared/ui/tabs.tsx`、`shared/components/ReportSummary.tsx`（2）、`features/vendors/VendorInfoForm.tsx`、`shared/ui/button.tsx`、`app/AdminLayout.tsx`、`shared/ui/badge.tsx`、`features/vendors/MenuSection.tsx` 的 only-export-components；`NewGroupOrderPage.tsx` 的 purity（Date）；`features/ai-import/ImportDialog.tsx` 的 set-state-in-effect。本輪未擴大修正。
- 瀏覽器為系統 Chrome 150.0.7871.128；兩 project 都由 Chromium 執行；UI spec 都明確設 viewport 為 320／390／768／1280×844 CSS px。mobile 有 LINE UA／touch／DPR 2，desktop 無 mobile 模擬。窄 CSS viewport 提供 reflow 驗證（320px 可對應 640px 在 200% 的 CSS 寬度），**未操作真實瀏覽器 200% zoom，也不是跨 engine／真機認證**。
- bar 在兩 projects／全部四寬度均為 **113px**（P0 mobile／desktop 皆 161px，減少 48px）；dirty 19 份草稿 $1,900,000，摘要仍是有效單 20 份；固定列只一個 48px CTA；CSS variable 與實際高度差 ≤1px、捲底 privacy 在列上方、取消前後列等高、closed／自然截止後 variable 清除均通過。safe-area env 此環境為 0，不能聲稱非零 inset 已驗。
- 403／404、sold-out 競態／cache 失效與 client-info 延遲在完整 Vitest 中驗證；本輪瀏覽器著重真實關閉 409、自然截止、取消 pending／500，未宣稱所有 API 錯誤組合皆做真瀏覽器注入。未新增套件、未讀秘密 env、未修改後端／shared UI defaults。

### 9.4 After 截圖、實際檢視與主 agent 入口

- Playwright 原始 `info.outputPath()` 在 `e2e/test-results/`（git ignored），完整成功 run 已另封存為 `/tmp/opencode/ui-round1-after-results/`，共 **80 張 PNG**。每個四寬度流程 10 張圖：`wizard-summary`、`management-collecting`、`management-summary`、`identity-dialog`、`public-dirty`、`public-dirty-fixed-bar`、`cancel-confirm`、`public-cancelled`、`readonly-snapshot`、`statistics`。
- 子目錄對應：`ui-improvements-UI-regress-0cbae-aft-snapshot-and-statistics-{mobile,desktop}`＝320；`6ae9d`＝390；`11b55`＝768；`da29b`＝1280（其餘前後綴相同）。
- D 已實際開圖檢視：390 mobile 與 1280 desktop 管理全頁／dirty 固定列／取消 Dialog；320 mobile 與 768 desktop 統計 viewport；390 mobile 取消成功與 readonly。可見長文字完整、手機統計兩欄＋總額下列、桌面三欄、緊湊分享、取消在摘要而不在列內、Dialog 文字／按鈕未溢出。取消成功／readonly 全頁圖保留自然短暫 toast，非空白或隱藏產品內容的替代 smoke。
- 以下為主要圖片**絕對路徑**；主 agent 已親看的子集與最終核准範圍見 9.5，不宣稱全部 80 張均由主 agent 檢視：
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-6ae9d-aft-snapshot-and-statistics-mobile/management-collecting.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-da29b-aft-snapshot-and-statistics-desktop/management-collecting.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-0cbae-aft-snapshot-and-statistics-mobile/management-summary.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-6ae9d-aft-snapshot-and-statistics-mobile/public-dirty-fixed-bar.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-da29b-aft-snapshot-and-statistics-desktop/public-dirty-fixed-bar.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-6ae9d-aft-snapshot-and-statistics-mobile/cancel-confirm.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-da29b-aft-snapshot-and-statistics-desktop/cancel-confirm.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-6ae9d-aft-snapshot-and-statistics-mobile/public-cancelled.png`
  - `/tmp/opencode/ui-round1-after-results/ui-improvements-UI-regress-6ae9d-aft-snapshot-and-statistics-mobile/readonly-snapshot.png`

### 9.5 主 agent 最終驗收決議（2026-10-29）

- 主 agent 已審 A/B/C 產品 diff、永久 E2E 主要契約，以及 D 首跑測試時機／locator 修正說明，親跑 `git diff --check` 通過。
- 主 agent 已親自讀取 after 四圖：390 mobile 的 `management-collecting.png`、`public-dirty-fixed-bar.png`、`cancel-confirm.png`，以及 1280 desktop 的 `management-collecting.png`（絕對路徑見 9.4）；另親看 P0 mobile 的 `management-collecting.png` 與 `public-dirty.png` 作前後對照（路徑見 9.1）。
- **核准本輪 P0–P4 自動化與畫面驗收**。此決議不擴張為全面真機完成：LINE 真機、IME／鍵盤、非零 safe area、原生分享、實際瀏覽器 zoom 仍明列待驗；其他未覆蓋界線維持 9.3，未新增真機證據或勾選 LINE checklist。
- 本次收尾僅更新本文件的狀態、task 驗收結果與最終紀錄；不修改產品／測試、不重跑測試、不 commit。D 另執行 `git diff --check` 與 `git status --short` 完成文件交接。
