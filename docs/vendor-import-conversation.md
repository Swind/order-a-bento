# Vendor 匯入草稿對話（第一版）

## 使用方式

照片辨識後，仍在原確認頁手動改資料、勾選要套用的欄位，也能輸入文字修改草稿。每輪以**當下畫面上的手改與勾選狀態**為準；AI 只提交明確欄位修改，不重生整份菜單。指示模糊時可只提問、不改資料。成功後顯示本輪變更；「復原上一輪」回到該輪送出前的快照，對話保留並標記已復原。再次對話會使用復原後的草稿。

手改不做背景 autosave：按「儲存草稿」才可在重新開啟時恢復；送出對話也會先原子保存手改。已使用對話／保存草稿後，正式套用會先同步畫面資料，並按該 revision 的後端草稿提交，而非信任另一份 accepted payload。沒有使用新功能的舊匯入維持原流程。

Undo 指最近一次**有實際變更**的 AI 回合，單純提問／無變更不新增復原版本。未保存的手改會立即停用 Undo；後端在保存或新 turn 同步了不同草稿時清除舊快照，禁止舊 AI Undo 覆寫最新手改，相同內容保存則保留 Undo。若後續 AI 又修改，新的 Undo 只會回到該輪最新的手改基準，對話紀錄始終保留。

只有最後「套用」會改正式店家。刪除草稿品項表示不匯入該列，**不刪除既有店家的正式品項**；既有店家仍沿用匯入的同名 upsert 語義，既有店家改名請到店家頁操作。新店名可在草稿修改。第一版不新增多圖、語音、串流、背景工作或新相依套件。

## 分層與安全

- Domain：`ai_import/draft.rs`，run-owned vendor/item stable IDs、欄位型別與限長驗證、原子 edit batch。ID 不代表正式 vendor/menu item ID，既有目標不能被模型替換。
- Application：`draft_service.rs`，owner／共享管理權限／Vendor 模式／proposed 狀態／建立起 24 小時有效期，每個 turn、save、undo 都檢查。只保留最近一次可復原的修改前快照；對話最多 20 輪、每輪指示 500 字、回覆 2,000 字、修改 100 筆、總品項 500、資料文字總量 128 KB；模型只取得最近 6 輪對話（不攜帶舊草稿快照）。
- Infrastructure：migration `0009` 另建草稿與 turn quota ledger，SQLite transaction 內重查 scope、status、owner、expiry、revision。Turn 先保存最新手改並 reserve revision，再呼叫既有 agent；候選修改只在模型整輪成功且 revision 未變時寫回。失敗／逾時不改草稿，並保留失敗紀錄。
- Rig：沿用官方 0.46.0、既有四種 provider、token budget、timeout、一次 retry、auth/header 與 usage hook；對話模式只暴露 `submit_draft_edits`，沒有讀寫正式目錄工具。Retry 清除候選、恢復當輪 baseline，不清除最新手改。
- API：`GET/PUT /ai/import-runs/{runId}/draft`、`POST .../turns`、`POST .../undo`。JSON request 512 KB，所有 import response（包括拒絕）`private, no-store`。`GET run` 帶已保存的草稿供 reload。
- Frontend：等待中停用編輯，另外檢查送出時 editor identity，防止晚回應覆蓋較新的手改。409／不確定的網路結果保留畫面並阻擋提交，可重試原 turn ID 或明確確認後重新讀取伺服器版本。跨分頁／並行請求由後端 CAS 保護。

## 額度、重試、照片與失效

每個新 logical turn 與原本圖片匯入**共用** `AI_DAILY_QUOTA_PER_USER` 的 24 小時次數額度，含失敗。Turn reservation 與 quota ledger 在同一 transaction；相同 run + turn ID 重試只讀結果或 pending，不再次呼叫模型、套用或占用額度。既有 provider 的單次 retry 不另占 logical-turn 額度。Undo／手改保存不耗額度。API response 遺失可重試原 turn；不能把它當成新的指示再送。

程序中斷的 pending turn 在「run timeout + 30 秒」後可透過重新讀取草稿轉為失敗，草稿不變；晚到的結果不能通過 revision CAS。另一分頁手動保存可取消舊 pending；Undo／正式提交不能越過尚在進行的 turn。已套用、放棄、到期不可繼續對話或 Undo。

原圖只沿用原有勾選保存照片時的 run-owned JPEG 暫存，未勾選**不新增圖片保存**；有暫存且仍有效時會提供給 AI 作參考。原本 24 小時照片清理、首次圖片辨識失敗／放棄／正式提交時清除的政策不變；後續對話失敗不丟草稿或仍有效的參考圖。草稿與對話和既有 run 一樣保留作追溯；24 小時後禁止再編輯，並非聲稱自動刪除整個 run。

## 無付費驗證

金額沿用元數字的 JSON contract，內部改用整數分，支援 `7.5` 修價；migration 與帳務一致性見 [小數金額說明](decimal-money.md)。

Demo 模式輸入「示範改價」透過真正的 edit application tool 將第一項加 5 元，其他文字只提問。自動測試用 fake agent、SQLite 與本機四 provider mock，不呼叫真 AI。Playwright 的 AI 匯入測試包括手機／桌面對話、手改、Undo、提問、保存、reload 與最後正式提交，並輸出 `vendor-draft-conversation.png`。
