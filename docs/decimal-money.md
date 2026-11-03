# 小數金額與整數分

支援單價 `7.5`、`7.50`、`7.25`，最多小數兩位。水餃 `7.5 × 3 = 22.5`；不四捨五入或截斷成整元。單價範圍改為 NT$0.01–100,000，0、負數、超精度、非有限值及溢位皆拒絕。

## 單位與相容性

- **Rust domain/application、Money、SQLite INTEGER：一律整數分**。`750` 代表 NT$7.50。所有加總、數量乘法、SQL 統計、已收款快照與欠／退款差額用整數計算，不用 `f64` 做金額算術。Rust 手寫 fake／command 的價格也用分。
- **HTTP API、AI tool JSON、proposal/draft JSON、eval JSON：仍然用元的 JSON number**。原本的 `{"price":100}` 仍是 100 元；新增 `{"price":7.5}`，不要求客戶端改成傳 `750` 或字串。回傳整元仍為 `100`、小數為 `7.5`／`7.25`。這是既有 API 的相容擴充，不是單位改版。
- `domain/shared/amount.rs` 處理元／分邊界。原始 API JSON 及 AI 參數在 tagged enum buffering 前驗證數字原文，因此連 `7.5000000000000001` 都不會先被浮點 parser 捨入成 `7.5` 而通過。只開啟既有 serde_json 的 `raw_value` feature；**沒有**開啟會影響 Rig 0.46.0 catalog 的全域 arbitrary precision，沒有新 crate／framework。
- OpenAPI／AI schema 的金額型別改為 `number`。Schema 的 `f64` 標示只描述 JSON number，非 Rust 帳務儲存或運算型別。
- 前端保留 API 的元數字，輸入以字串拆分成整數分驗證，訂單預覽先換成整數分乘加後才轉回元供顯示；不是浮點單價直接乘數量。`formatMoney` 顯示 `$7.5`／`$7.25`，保留原本整元與千分位風格。

## 既有資料 migration

`0010_money_cents.sql` 在 SQLx migration transaction 中一次完成：

1. 重建 leaf table `menu_items`，價格由舊整元乘 100，價格限制為 1–10,000,000 分且 SQLite storage class 必須是 integer。
2. **同時**將 `offered_items.price`（開團快照）、`submission_lines.unit_price`（每次歷史送單成交價）、`settlements.paid_amount`（實收快照，保留 NULL）乘 100。ID、數量、順序、時間、備註、照片、FK 與原訂單幣值都不變。歷史 100 元不是 1 元，也不使用今日菜單價重算歷史。
3. 原本 proposal/draft/undo/tool-call JSON 不乘 100，因為一直維持外部的「元」單位，新的 serde 邊界讀取時才轉分。
4. 對可能溢位或不合法的舊 SQLite storage class 先拒絕並 rollback，避免 SQLite 把乘法悄悄轉成 REAL。migration 版本保證只執行一次。

部署前備份並停止舊程式，再以新 binary 執行 migration；**不要讓舊 binary 存取 migration 後的 DB**。要回退舊版必須恢復成套的舊 DB 備份，不能只降 binary，否則舊程式會把分誤認成元。

## 已追蹤的路徑

手動菜單貼上新增與逐列編輯、AI 初始辨識、草稿手改、AI 多輪修價與 Undo、最後套用、開團菜單快照、公開／管理者訂單預覽、送單與歷史小計、列表／統計／領取繳費／差額均使用上述單位。既有複製店家訂單與訂餐確認列表只輸出品名／數量／備註，沒有另外的金額匯出或 CSV 路徑需要換算；eval 的價格比較與錯價報告也已使用同一邊界。

測試包含舊版 DB 與歷史付款遷移、migration 溢位 rollback、API 舊整元相容、7.5 手動新增編輯、真實 SQLite 訂單快照／22.5 小計／統計／歷史／結算、四 provider 本機 mock 的初始小數與草稿改價、前端精度陷阱與真實 embedded binary 手機／桌面 E2E。沒有付費 AI 呼叫。
