# 店家參考照片：調查與實作計畫

狀態：**P0–P4 本輪已完成，主 agent 最終驗收通過（2026-10-29）**：實作、自動測試、手機／桌面截圖、44px 局部修正及本機持久化／備份還原均已核准；詳見 §14。第一版僅接受靜態 JPEG／PNG，不支援任何 WebP；前端直接傳原檔（5 MiB），後端負責驗證／正規化。64 MiB 是 decoder 可控制的 allocation／output budget，不是 JPEG scratch 或 process RSS 硬上限。Docker／真機、RSS 壓測與完整 interleaving 仍未驗，不因本輪簽收而視為完成。實際證據與未測項見 §10–§14；全部改動仍未 commit。

## 1. 已核准決策與範圍

已核准：**已建立的店家可持久保存一張參考照片（菜單或店面），可上傳、替換、刪除及放大預覽**。圖片跟隨店家的個人／共用群組權限，與 AI 辨識獨立，未設定 AI 也可用。第一版上傳來源僅 JPEG／PNG，輸出一律 JPEG。

以下預設已核准；若改成多張／公開點餐可見，應先修改契約，不讓實作者自行擴充：

1. **一店一張是否足夠？** 本輪不做相簿、分類、排序、標題、歷史版本、列表縮圖。現有文件沒有要求店家持久保存多張照片；`docs/design.md` §12 的多張照片問題是 AI 匯入正反面，不是相簿需求。
2. **僅管理端有權使用者可看，公開點餐及 LINE／OG 預覽不可看，是否符合預期？** 不把圖片加入團的歷史快照或公開 API。
3. **壓縮後參考圖，而非原檔典藏**：沿用長邊 1568 px、JPEG、最多 5 MiB；文字非常密集的菜單可能不夠清楚。若要高解析度原圖，先另訂像素與容量上限。
4. **併發採最後成功提交者生效**：兩人同時替換／刪除會依 DB 提交順序決定，不新增版本衝突 UI。這不會破壞店家文字或菜單，但不保護使用者以舊畫面覆蓋別人的新照片。若需禁止此情況，核准前改成 revision + 條件寫入（409）；不要假稱現有交易已提供樂觀鎖。

預設不做：AI 匯入自動留存照片、把已存照片送 AI、外部 URL 抓圖、S3/CDN、獨立圖片儲存服務、下載原檔、裁切、拖曳、進度百分比、批次上傳。上傳成功只改照片，不修改菜單。

## 2. 已讀指示及現況證據

本節保留實作前調查基線；P1 後新增的欄位、routes 與測試以 §10 為準。

- 已載入 `ponytail` skill：先追完整流程、重用既有能力、保留安全邊界，不新增預備性抽象。本 repo 與上層目錄未找到適用的 `AGENTS.md`。
- 目前未提交的使用者改動必須保留：
  - `frontend/src/features/group-orders/lib.ts`
  - `frontend/src/features/group-orders/lib.test.ts`
  - `frontend/src/features/group-orders/GroupOrderDetailPage.test.tsx`
- 架構：`domain` → `application`（use cases、ports）→ `infrastructure`（SQLx SQLite）／`api`（Axum、DTO、Utoipa）→ `server` wiring。圖片不要塞進 HTTP DTO 的 base64，也不要繞過 application ports 直接在 handler 寫 SQL。
- 店家入口：`api/src/routes/vendors.rs`，掛在 `/api/v1`。`VendorDetail` 有 `sharingGroupId`、`canManage`、`updatedAt` 及菜單，目前無圖片欄位。
- `application/src/directory.rs` 透過 `VendorRepo` 查改店家；`infrastructure/src/directory_repo.rs` 的 `load_vendor`／`access` 依 actor、真正 owner、群組成員資格授權。共用店家 creator 不可繞過 membership；管理員才能寫，一般成員只能讀，不足權限統一當 404。
- `directory_repo.rs` 既有 mutation 採單連線 write pool、`BEGIN IMMEDIATE`、load → mutate → persist；`sqlite.rs` read pool 最多 8 連線、WAL、foreign_keys=true。勿把圖片寫入混進舊 aggregate 的全欄覆寫，否則菜單／文字與照片更新可能互相干擾。
- `api/src/session.rs` 的 `AuthenticatedOrganizer` 使用 HttpOnly、SameSite=Lax cookie；全域 `origin_check` 已涵蓋 `/api/` 的非 GET/HEAD/OPTIONS。新端點須留在同一 router／middleware 範圍。
- `api/src/routes/ai_import.rs` 已有 multipart、5 MiB image／6 MiB body 上限、magic bytes 偵測與 ProblemDetails。但目前 magic bytes **不等同完整圖片解碼驗證**，其 parser 也允許重複 image 覆蓋；不要直接照抄這兩點到持久檔案入口。
- `frontend/src/shared/lib/compressImage.ts` 以原生 Canvas 縮至長邊 1568、JPEG quality 0.85 起降，超過 5 MiB 才逐步降品質，沒有保證最後一定小於上限；呼叫端須再驗大小。已有 EXIF 方向處理、Object URL 釋放及測試。
- `features/ai-import/ImportDialog.tsx`／`hooks.ts` 已示範選檔、本地預覽、壓縮、FormData、`credentials: include`、`toApiError`。AI 原圖只留記憶體轉送模型，不在 `ai_import_runs` 存檔（`docs/design.md` §9.6）；本輪不改此隱私行為。
- `VendorDetailPage.tsx` 組合基本資料與 `MenuSection`；依 `canManage` 顯示唯讀。`vendors/hooks.ts` 的 query key 為 `["vendor", id]` 及 `["vendors", scope]`；權限失效沿用 `invalidateSharingAccessOnError`。
- 部署：`Dockerfile` 非 root uid 10001，`DATABASE_URL=sqlite:///data/bento.db`、`VOLUME /data`；`docs/deploy.md` 要求單一實例，備份為停機打包整個 volume（含 WAL/SHM）。目前沒有檔案 storage adapter／uploads mount。

路徑縮寫：本文件後文 `api/`、`application/`、`infrastructure/`、`domain/` 都指 `backend/crates/` 下對應 crate。

## 3. 儲存取捨：選 SQLite BLOB

| 選項 | 優點 | 成本／風險 | 本輪決策 |
|---|---|---|---|
| SQLite 子表 BLOB | 圖片與 metadata 同交易；FK 級聯清理；無路徑穿越／孤兒檔；重啟、既有 volume 備份還原直接適用 | DB/WAL／備份變大；BLOB 寫入占用單寫入者；刪除不立即縮小 DB 檔 | **選用**，每店至多 5 MiB，符合小型單實例部署 |
| `/data/vendor-images` filesystem + DB metadata | 大檔可串流，DB 較小；整 volume 備份仍可含圖 | 要處理暫存檔、rename、DB/檔案非原子提交、替換失敗補償、孤兒清理、檔名安全、權限及還原一致性 | 暫不做；實際數量／寫入延遲證明 BLOB 不適合時再遷移 |

不要建立通用 `Storage` trait、storage driver 設定或第二種實作。擴充現有 `VendorRepo` 即可。即使未來改 filesystem，仍須授權 API 回傳，不能把 `/data` 掛成 public static。

### Migration（已實作）

已新增 `backend/migrations/0007_vendor_images.sql`（確認序號未占用），不改已套用 migration：

```sql
CREATE TABLE vendor_images (
  vendor_id TEXT PRIMARY KEY REFERENCES vendors(id) ON DELETE CASCADE,
  revision TEXT NOT NULL,
  content_type TEXT NOT NULL CHECK (content_type = 'image/jpeg'),
  data BLOB NOT NULL CHECK (length(data) BETWEEN 1 AND 5242880),
  width INTEGER NOT NULL CHECK (width BETWEEN 1 AND 1568),
  height INTEGER NOT NULL CHECK (height BETWEEN 1 AND 1568),
  updated_at TEXT NOT NULL
);
```

- `revision` 每次成功 PUT 由伺服器生成新 UUID（沿用 uuid v7），僅作 UI 載入識別，不是公開授權 token，也不是 CAS。
- `sizeBytes` 查詢用 `length(data)`，不重複儲存；無原檔名／上傳者副本，權限永遠取自 parent vendor。
- 舊店家無子表 row，API `image: null`；不回填／搬移 AI 原圖（不存在）。
- 圖片替換只 UPSERT 該 row；移除 DELETE 該 row；刪店家與現有 organizer cascade 一併移除圖片。不另設清理排程或 filesystem temp file。
- 修改圖片與 `vendors.updated_at` 同一交易；無圖重複刪除不更新 timestamp。DB 秒精度不可當圖片版本，使用 revision。
- 刪除後 BLOB 邏輯上不可讀，不保證磁碟頁面／WAL 或舊備份安全抹除；不在請求中執行 VACUUM。容量長期成長時由維運另行安排，備份仍需視為私密資料。

## 4. API 與資料契約（已核准）

全部使用 cookieAuth、Directory tag；path prefix `/api/v1/vendors/{vendorId}`。無額外 `sharingGroupId` 參數，不能讓 client 宣告資料所屬範圍。

### DTO

在 `VendorDetail` 新增必回傳、可空的 `image: VendorImageDto | null`；列表 DTO 不變。

```ts
type VendorImageDto = {
  revision: string;       // server UUID，不採 timestamp
  contentType: "image/jpeg";
  sizeBytes: number;      // 1..5242880，實際儲存 bytes
  width: number;          // 1..1568
  height: number;         // 1..1568
  updatedAt: string;      // 與現有 time_text 格式一致
};
```

不回傳 public URL、data URL、檔名或 BLOB。metadata 放在 domain 的小型值型別／Vendor 可空欄位，二進位 input/result 放 application ports；`Vendor::new/from_parts` 預設無圖，`load_vendor` 補讀 metadata（不讀 BLOB）。所有回傳 VendorDetail 的建立、patch、菜單 upsert 路徑都要保留一致的 `image` 值，避免編輯菜單後照片暫時消失。既有 mutation diff 不負責保存／刪除圖片欄位。

| 方法／路徑 | 輸入 | 成功回應 | 語意 |
|---|---|---|---|
| `PUT /image` | `multipart/form-data`，恰好一個必填 `image` binary part | 200 `VendorImageDto` | 建立或原子替換單張圖片；不回整份舊 vendor，避免 client 覆蓋同時編輯的欄位 |
| `GET /image` | 無 body／無需版本參數 | 200 `image/jpeg` binary | 每次以目前 session 與 parent vendor 權限讀取；有權但無圖也回 404 |
| `DELETE /image` | 無 body | 204 無 body | 有管理權且店家存在時冪等；本來沒照片也是 204 |

GET/PUT/DELETE 共通錯誤：401 `unauthorized`、404 `not_found`（不存在／無資格 indistinguishable）、500 `internal_error`。變更請求另有 403 `bad_origin`。PUT 另列：

- 400 `invalid_multipart`：不是合法 multipart、未知／重複 part；不使用最後一份覆蓋前一份。
- 413 `image_too_large`：image 超過 5 MiB 或 body 超過 6 MiB，包含沒有 Content-Length 的串流。
- 422 `invalid_image`（field=`image`）：缺圖、空檔、非允許格式、解碼失敗、動畫、像素／解碼記憶體超限。
- 429 `rate_limited` + `Retry-After`：圖片處理限流或同時處理額度不足（不可占住 DB 交易排隊）。

錯誤一律既有 `application/problem+json`；不得把 decoder 詳細錯誤、路徑或圖片內容放入 response／log。MultipartRejection、body limit rejection 也要映射，不能意外回 Axum 純文字。

### 圖片輸出與私密性

- GET 成功及圖片端點錯誤都設 `Cache-Control: private, no-store`；成功另設 `X-Content-Type-Options: nosniff`、`Cross-Origin-Resource-Policy: same-origin`。不做 public cache、ETag/304、Range 或可長期使用的簽名網址。
- GET 必須在同一短 read transaction 內檢查 parent 的 owner／membership 並讀 BLOB，不能先授權再跨交易拿圖；成員移除後的新請求必須失敗。已傳給有權使用者的 bytes 無法事後追回，這不是 DRM。
- 公開 `/g/{token}`、public ordering DTO、OG meta、LINE 訊息、團快照不得新增圖片資訊。持分享 token 不構成讀 vendor image 的權限。

### OpenAPI 流程

1. `api/src/routes/vendors.rs`（或小型同層圖片 route module）新增 DTO、Utoipa 註記及 routes；multipart 使用既有 `StartImportForm` 的 binary schema 樣式，GET 明列 binary JPEG response。
2. `VendorDetail.image` 標為 nullable，且實際每次 serialize 都存在。新端點列出上述成功／錯誤、cookieAuth、大小與併發語意。
3. 整合者在 repo root 執行 **`make gen-api`**：後端 `export_openapi` → `frontend/openapi.json` → `npm run gen:api` → `frontend/src/shared/api/schema.d.ts`。
4. `frontend/src/shared/api/types.ts` 由 generated components 匯出 `VendorImage`；不手改 schema、不保留平行手寫 DTO。
5. CI `make check` 會重匯 OpenAPI 並檢查 JSON 同步。生成後檢查 PUT multipart image 是 binary、GET 不是 JSON、nullable 正確；生成檔由單一整合者持有。

## 5. 安全上傳與依賴

### 邊界與處理順序

1. 全域 Origin check → session → parent vendor 可管理資格預檢 → 限流／取得處理額度 → 讀 multipart。登入／權限不足不先解碼大圖。
2. route-local body 上限 6 MiB（勿提高所有 API 上限）；image 限 5 MiB，採逐 chunk 累計，超限即停止；僅接受單一 `image` part，忽略 filename／聲稱的 MIME。multipart bytes 不落磁碟。
3. allowlist 僅靜態 JPEG、PNG；magic bytes 僅初篩。**必須完整 decode**，拒絕損毀、只有 header、SVG／HTML／GIF、APNG，以及所有 WebP（含靜態與動畫）；不要手寫 JPEG／PNG decoder。實作額外檢查容器 framing：PNG chunk CRC／IEND／動畫控制、JPEG marker／EOI，避免現成 decoder 寬容接受截斷檔；像素仍由現成 codec 完整解碼。
4. 解碼前限制 width/height ≤8192、總像素 ≤16,000,000；可控制的 decoder allocation／output budget 為 64 MiB，再完整 decode。這些是固定常數，不新增 env。PNG 使用 `PngDecoder::with_limits` 在初始化前套 allocation／dimensions 限制，IHDR 的總像素也先檢查；JPEG 直接使用既有 `zune-jpeg` strict decoder，在 `decode_headers` **之前**設定 8192 邊長及 32 scans，headers 完成後、配置像素 buffer 前檢查總像素及 output budget。`image 0.25.10` 的 JPEG adapter 自行關閉 strict mode 並放寬 header dimensions，因此不經該 adapter 先讀 JPEG headers。**64 MiB 不是整個處理程序 RSS 或 JPEG scratch 的硬上限**：zune-jpeg 無總 scratch budget API，scratch／CPU 以 5 MiB 輸入、8192 邊長、16MP、32 scans、兩個 processing jobs 有界控制；此技術邊界已由主 agent 明確接受，不引入 process sandbox。
5. 尊重 EXIF orientation、縮至長邊 ≤1568、扁平化透明背景為白色，重新編碼 JPEG（quality 85）；不複製 EXIF／GPS／ICC／comments／尾隨資料。不接受以「前端已壓縮」取代後端驗證。輸出再驗 1..5 MiB；超限不覆蓋原照片。
6. decode/encode 用 `tokio::task::spawn_blocking`；在 AppState 共用 `Semaphore` 固定 2 個額度，使用 try-acquire 忙碌就回 429，不建立無界等待／阻塞工作。沿用 `KeyedLimiter` 為照片 PUT 建獨立每 actor 限流（建議 burst 3、每分鐘補 3），不要與 AI quota 共桶。permit 須由 blocking job 持有直到真正完成，避免 HTTP abort 釋放後繼續累積工作。
7. 驗證成功後才開 `BEGIN IMMEDIATE`，**交易內重新授權**（處理時可能被降權／移除或店家已刪除），UPSERT BLOB + revision + parent timestamp → commit。失敗保留舊照片；不存在的 parent 不可被重建。

### 重用依賴，沒有新 storage 套件

- Axum multipart、SQLx BLOB (`Vec<u8>`)、uuid、Tokio、現有 limiter 與 Canvas 已可覆蓋主要流程。
- `image 0.25.10` 已成為 workspace／infrastructure 直接依賴，`default-features=false` 且只開 jpeg/png。為 strict JPEG decode 直接使用鎖檔已有的 `zune-core 0.5.3`／`zune-jpeg 0.5.15`；PNG 容器 CRC 使用已有的 `crc32fast`，沒有新增另一個影像函式庫。WebP 明確不支援、不開 WebP codec feature、不為它增加套件或 isolated process；即使 feature unification 帶入其他 codecs，入口仍僅允許 JPEG／PNG。
- 在 `infrastructure/src/vendor_image.rs` 放小型解碼／正規化 helper 及測試；HTTP 層透過 helper 使用，不新增只有一種實作的影像服務 trait。`api/Cargo.toml` 需將 tokio 列 runtime dependency 以 spawn_blocking／Semaphore，沿用 workspace 版本。
- 既有 `ai_import::sniff_image_type` 可重用初篩或移到小型共用 helper；本輪不順便改 AI 完整驗證／保存語意，不為此重構整個匯入流程。
- 版本確認結果：JPEG／PNG EXIF orientation 與 APNG 檢測可用，另有完整容器檢查；JPEG adapter 的 strict／header 限制問題已以上述同一底層 codec 直接呼叫處理。WebP adapter 不支援 allocation limit，底層也有不受該 limit 控制的配置，故經主 agent 決議排除第一版 WebP，而不是降為 magic bytes 驗證。

## 6. Repository 與併發

擴充 `VendorRepo`，不要新增第二份 owner/group 授權規則：

- `read_image(actor, vendorId)`：同一 read snapshot 授權 parent，再取可空 binary + metadata；actor 無權與無圖對外均 404。
- `put_image(actor, vendorId, normalizedImage, now)`：write pool／`BEGIN IMMEDIATE` 內使用現有 parent access，必須 `can_manage`；原子寫圖及 parent timestamp，回 metadata。
- `delete_image(actor, vendorId, now)`：同上；存在 parent 且有管理權才能冪等刪圖。
- 在 `DirectoryService` 提供薄 use cases，統一轉換 DirectoryError。讀取 metadata 走 domain Vendor，不讓普通 `find/list` 加載 BLOB，也不要 clone 圖片到所有 menu mutations。

現有 `directory_repo.rs::access` 為私有，可讓同 crate 的圖片 repo/helper 重用（`pub(crate)`），或將實作留在同檔；不必另造授權框架。只有符合 actor 的 parent 查詢可讀圖，不能 `SELECT data WHERE vendor_id=?` 後才在 handler 補檢查。

併發驗收語意：

- PUT/PUT：至多一列；最後 commit 的完整 JPEG 與 metadata 同時可見，不出現混合／半張圖。
- PUT/DELETE：依 commit 順序生效；GET 可見舊或新完整快照，不可見不一致 metadata。
- PUT 與 menu／vendor info patch：兩者資料均保留，圖片不被舊 aggregate 清空。
- PUT 與刪店家：先刪店家則 PUT 404，後刪店家則圖 cascade 清除；永無 orphan。
- 權限撤銷與 PUT：以取得寫交易後檢查到的資格決定，不靠上傳前那次檢查。

## 7. UI 狀態與整合

在店家詳情基本資料與菜單之間加入獨立 `VendorImageSection.tsx`；避免把檔案放進 `VendorInfoForm`，或把照片上傳誤作整張表單儲存。

- 無圖：顯示「尚未上傳參考照片」；管理者可選檔，唯讀成員沒有變更按鈕。
- 有圖：`object-contain` 不裁掉菜單，提供按鈕在既有 Dialog 放大預覽（有名稱、關閉、鍵盤操作、焦點回復），不要新加圖片燈箱套件。
- 選檔（P3 核准調整）：原生 file input，accept JPEG/PNG（`image/jpeg,image/png`），明確提示不支援 WebP；前端僅驗 MIME／非空／**原檔 ≤5 MiB** 作 UX，原始 bytes 直接 PUT，不使用 `compressImage` 或 Canvas。取消原先 20 MiB／瀏覽器壓縮方案，避免 APNG 被洗成靜態圖片而繞過後端規則，也避免透明 PNG 提前轉黑、重複 lossy 壓縮及前後端大小不一致。白底、EXIF、縮圖與靜態格式完整驗證完全由後端執行；本地 `<img>` 只供預覽，不決定上傳內容。
- 本地待上傳圖：預覽、重新選擇、「上傳／替換照片」、「取消」；明確說明會取代既有照片，確認前不送請求。Object URL 在換檔／取消／卸載時 revoke。
- 上傳中／刪除中：顯示忙碌文字、停用重複操作；不顯示假的百分比，也沒有前端壓縮階段。原照片在成功前保持可見，錯誤不清掉原圖（401/403/404 權限錯誤除外）。卸載後不更新本地狀態；中斷請求不代表伺服器未提交。
- 成功：清本地選檔、toast、invalidate `["vendor", id]` 與 `["vendors"]`；**不**以 PUT 回應拼裝整個 vendor。刪除用既有 `ConfirmDeleteButton`，取消確認不能送 DELETE。
- 讀圖推薦用帶 credentials 的 fetch → `blob()` → Object URL，query key 使用 `["vendor", id, "image", revision]`，effect 以 AbortController 清理；也可封在圖片 section 的 effect，不把 BLOB 長期放在 React Query cache。revision 改變才重載，關閉放大視窗不重上傳。
- 圖片讀取 401/403/404 時立即撤掉已顯示照片，沿用 `invalidateSharingAccessOnError` 重新查 parent；網路錯誤提供重試。parent 詳情失效或換帳號／卸載要 revoke URL，不能顯示上一家／上一個帳號的圖。一般成員被移除後的新請求無法看圖；沒有即時推送撤銷已下載 bytes 的保證。
- 沿用 `toApiError`／`errorMessage`；需為過大、無效圖片及忙碌提供繁中訊息。PUT 使用 FormData，不手設 multipart Content-Type（瀏覽器產生 boundary）。
- `VendorInfoForm` 目前 key 是 vendor.updatedAt，照片寫入後會觸發基本資料表單 remount。整合時須驗證未儲存文字不被照片上傳吃掉；最小修正是以實際基本資料值決定 reset/key，而不是所有 updatedAt 變動。不要趁機重寫表單系統。

## 8. 階段、檔案 ownership 與交接

原規劃的 ownership 如下；目前 P1 已由 **GPT-6.1 Sol** 實作，收尾階段另獲授權更新本文件。其餘 phase 的驗收不能由 P1 測試結果推定完成，也不表示要求額外啟動 agent。

| Phase／owner | 工作及獨占檔案 | 完成條件 |
|---|---|---|
| P0 主 agent | 核准 §1、API、驗證上限、last-commit-wins；凍結契約 | 不含糊宣稱多張／原圖／公開圖也支援 |
| P1 Backend Sol | `backend/**`：0007 migration；domain metadata；ports/use cases；repo；圖片 normalize helper；routes/DTO/OpenAPI annotations；AppState limiter/semaphore；必要 Cargo 宣告與測試 | API／儲存／安全／併發測試通過；產生器可跑。不碰 frontend generated files |
| P2 主 agent／整合者 | **獨占** `frontend/openapi.json`、`frontend/src/shared/api/schema.d.ts`、`frontend/src/shared/api/types.ts`，執行 `make gen-api` | JSON／TS 與後端同步後交前端，避免兩邊搶寫生成檔 |
| P3 Frontend Sol（可與 P1 為同一實作者） | `frontend/src/features/vendors/**`、必要 `frontend/src/shared/api/errors.ts`／測試；核准改為 5 MiB 原檔直傳，不改共享壓縮 helper | 上傳／替換／刪除／預覽、唯讀、錯誤與 draft 保留測試通過 |
| P4 主 agent／整合者 | `docs/deploy.md`、本文件驗收紀錄、必要 `e2e/**`，整合檢查 | 完整 check、手機／桌面驗收、重啟與備份還原；記錄未測項 |

若需要同時多名 agent，先取得明確分派；本設計階段不啟動任何 agent。每名 agent 只寫自己的 ownership；跨界變更請主 agent 排程，不互相覆寫。新 fixture 若因 `VendorDetail.image` 型別需要更新非 vendor feature 測試，由整合者針對 fixture 做最小改動。

**禁止碰觸前述三個未提交的 LINE 改動檔**；不 reset／checkout 使用者變更、不 commit、不 push、不部署。若總測試遇到該三檔既有失敗，只回報，不順便修。

## 9. 測試與驗收清單

### 後端自動測試

- [ ] migration：從 0006 帶店家／菜單／群組／訂單資料升級，舊資料完整、image 為 null；新 DB 可正常啟動，FK 有效。
- [ ] 真實小 JPEG／PNG 可上傳並回 JPEG，所有 WebP 均拒絕；GET 解碼成功、size/dimensions 符合 metadata。不能沿用 AI 測試只有 magic header 的假圖作成功 fixture。
- [ ] 缺圖／空檔／重複 part／未知 part／錯誤 multipart、聲稱 JPEG 的 HTML、SVG、GIF、截斷檔、header-only、動畫、巨幅低 bytes 圖均拒絕；偽造 MIME 不影響依 bytes 正確判定。
- [ ] 5 MiB／6 MiB 邊界、無 Content-Length 的 chunked body 超限、decoder allocation limit；失敗不改原圖／revision。輸入方向正確、透明底白色、輸出不含 EXIF/GPS／附加 payload。
- [ ] 未登入 GET/PUT/DELETE 401；私有 owner 可讀寫、另一 owner 404；群組 admin 可讀寫、member 可讀不可寫、非成員／已移除 creator 均 404。測試降權／撤銷發生在預檢之後、交易之前。
- [ ] CSRF bad Origin 被擋；圖片 GET 與錯誤 no-store、JPEG MIME、nosniff、CORP；公開 token 無法讀圖，公開 DTO／OG 不新增照片內容。
- [ ] 成功替換只剩一列；DELETE 無圖冪等；刪店家／organizer FK 清理；失敗交易 rollback；同時上傳／刪除／店家刪除／改菜單不損壞其他資料。
- [ ] limiter 429／Retry-After；兩個處理額度外不排無界工作，取消 HTTP 後 permit 仍維持至 blocking 工作結束。
- [ ] 重開 file-backed DB 仍讀到相同圖（不是只靠進程記憶體），metadata 查詢不讀 BLOB。

### 前端自動測試（Vitest + RTL + MSW）

- [ ] 無圖、有圖、放大預覽、loading／載入失敗重試、選檔取消、同檔再選；Object URL 於換檔／卸載釋放。
- [ ] 原檔 bytes 不變送單一 FormData image、POST 不誤用（契約為 PUT）、credentials、有 revision 後重新載圖；非 JPEG/PNG／空檔／超過 5 MiB 不送請求，APNG 原檔交後端拒絕、不經 Canvas 洗白。
- [ ] 替換與刪除成功才更新、失敗保留原圖；刪除取消不送請求；busy 防雙擊；網路失敗可重試。
- [ ] 唯讀 member 不顯示上傳／替換／刪除，但可預覽；API 404／權限失效撤圖；切 vendor／帳號不留舊圖。
- [ ] 上傳／刪圖後 query invalidation；基本資料未儲存草稿不因 updatedAt 改變而消失；既有菜單／AI 匯入入口不回歸。

### 整合驗收與命令

1. `cd backend`：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。
2. repo root：`make gen-api`；檢查 generated diff 及新 binary/multipart schema。
3. `cd frontend`：`npm run typecheck`、`npm test -- --run`、`npm run build`；最終由整合者執行 `make check`，先保存使用者 dirty 狀態的清單，勿用重設手段清除 diff。
4. 以現有 E2E harness／真實瀏覽器（不用真 AI key）驗收手機與桌面：新建店家→上傳→重新整理→預覽→替換→刪除；群組 member 唯讀、移除 membership 後讀取拒絕；開團與公開點餐無圖片洩漏。
5. 本機測試資料庫／測試 volume：停止服務→重啟後照片存在；依 `docs/deploy.md` 備份／還原到另一測試 volume 後相同照片可讀。不要接觸正式 volume 或現有 `backend/data` 使用者資料。
6. 更新 `docs/deploy.md` 說明圖片現在存 DB、備份容量與敏感性增加，但不新增 volume/env；`docs/design.md` 既有 AI「不保存原圖」敘述仍成立（若補說明須明確區分兩種功能）。

最終交付須列：實際通過的命令與測試、未測項、最終 storage／scope 決策、檔案清單；不得把本計畫核取方塊或預期行為當成已完成驗收。

## 10. P1 後端實際完成紀錄（2026-10-29）

### 實作與核准技術邊界

- 已完成 migration、domain metadata、application ports／use cases、SQLite repo、normalize helper、API routes／DTO／OpenAPI、AppState 專用 limiter／semaphore；每店一張管理端私有 SQLite BLOB，owner／group 權限與 last-commit-wins 均沿用核准設計。
- JPEG／PNG 完整解碼、EXIF orientation、長邊 1568、白底 JPEG quality 85、5 MiB input／output、6 MiB route-local body limit；拒絕空檔、損壞／截斷／header-only、APNG、所有 WebP 與其他未允許格式。客戶端 filename／宣稱 MIME 不決定格式。
- GET 在同一 read snapshot 授權並讀圖；PUT／DELETE 在 write transaction 重新授權並原子更新圖片與 parent timestamp。圖片端點成功／錯誤（含 session／Origin rejection）均 no-store，成功 GET 另有 nosniff／same-origin CORP。
- JPEG 初次 headers 已改為受限的 `zune-jpeg` strict decoder：8192 尺寸限制在 `decode_headers` 前設定；底層 `headers.rs` 會在解析 SOF 時檢查這兩個 options。總像素與 output budget 在 pixel buffer 配置前檢查；32 scans／兩個 blocking jobs 限制 CPU／併發。64 MiB 的核准定義見 §5，不是 JPEG scratch 或 RSS 硬 cap，**不再列為未解 integration blocker**。
- 新增後端測試涵蓋：JPEG／PNG 真圖與 EXIF、透明白底／縮圖／尾隨 payload 移除、WebP 明確拒絕、截斷與容器檢查、巨幅低 bytes headers、5／6 MiB 邊界與無 Content-Length stream、metadata 保留、私有與群組權限、上傳預檢後撤權、Origin／no-store／安全 headers、獨立 limiter、處理額度與取消後 permit、rollback／併發寫入／刪圖冪等／cascade、file-backed DB 另開 pool 讀回、v6 migration 舊資料保留及 OpenAPI binary／nullable 契約。

### 已執行且通過

- 收尾前：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（**331 passed，0 failed**）、獨立 `export_openapi` build／執行。
- 本次 JPEG／PNG 決議收尾後：`cargo fmt --all --check`；`cargo clippy -p api -p infrastructure --all-targets -- -D warnings`；`cargo test -p infrastructure -p api vendor_image`（**14 passed，0 failed**，含新增 JPEG header dimensions／pixels 測試）；`cargo run -p server --bin export_openapi`。依主 agent 指示未重跑無關的全 workspace tests，331 是收尾前紀錄，不宣稱此次完整重跑。
- OpenAPI 匯出至 `/tmp/opencode/vendor-images-openapi.json`；已檢查必回 nullable `VendorDetail.image`、單一 multipart binary image、GET JPEG binary response、contentType singleton enum。未寫入 frontend generated files，`make gen-api` 仍屬 P2。

### 磁碟與後續 agent 操作

原 repo 所在 `/home/swind/Program` filesystem 曾因編譯產物耗盡而令 export／build 失敗；這不是型別錯誤。只將本輪可辨識的 incremental build caches **保留搬移**至 `/tmp/opencode/vendor-images-incremental-cache*`，沒有刪改來源或 DB。最後 tests／export 改用另一 filesystem 的 target 後通過。

後續 agent 請沿用以下環境，**不要再預設寫入 repo 的 `backend/target`**：

```sh
export CARGO_TARGET_DIR=/tmp/opencode/vendor-images-target
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
# backend/ 下執行 cargo；repo root 的 make gen-api 也會繼承這些變數。
```

### 檔案與未驗收事項

後端變更（均以 `backend/` 為 prefix）：

- `Cargo.toml`、`Cargo.lock`、`migrations/0007_vendor_images.sql`
- `crates/domain/src/directory/{mod.rs,vendor.rs}`
- `crates/application/src/{ports.rs,directory.rs}`
- `crates/infrastructure/Cargo.toml`、`crates/infrastructure/src/{lib.rs,sqlite.rs,directory_repo.rs,vendor_image.rs}`
- `crates/api/Cargo.toml`、`crates/api/src/{lib.rs,state.rs}`、`crates/api/src/routes/{mod.rs,vendors.rs,vendor_images.rs,vendor_image_tests.rs}`

另經收尾授權更新本文件。未改 frontend 或三個 LINE 改動檔；未 commit／push／部署，未開啟或 migrate 現有使用者 DB，測試僅使用 temp DB。

仍待各 owner 驗收：P2 generated API／TS 整合、前端 JPEG／PNG accept／提示同步與 UI／草稿保留、完整 `make check`、手機／桌面 E2E、真實服務程序重啟及備份還原、完整 PUT／刪店家交錯壓測。另開 file-backed DB pool 讀回測試不等於程序重啟／備份還原；沒有執行 codec RSS 壓測，也不提供 64 MiB RSS 保證。§9 複合驗收核取方塊保留未勾，不能由本段部分自動測試推定全部完成。

## 11. P2/P3 前端實際完成紀錄（2026-10-29）

### 實作與設計調整

- P2 依 §10 的四個 Cargo 環境變數執行 `make gen-api` 成功，未往原 `backend/target` 重新編譯。更新 `frontend/openapi.json`、`frontend/src/shared/api/schema.d.ts`，並在 `types.ts` 匯出 generated `VendorImage`。確認 GET JPEG binary、PUT multipart binary 與必回 nullable `VendorDetail.image`；沒有平行手寫 DTO。
- P3 新增 `VendorImageSection.tsx`，於店家基本資料與菜單間整合。使用既有 Button／Dialog／ConfirmDeleteButton／toast；支援選檔、本地與已存圖放大預覽、確認上傳／替換、取消、刪除確認、唯讀、busy、錯誤重試。照片變更使用 generated typed API 與原有 `ApiError`／`errorMessage`，照片專用錯誤訊息不影響仍接受 WebP 的既有 AI 匯入。
- **依主 agent 偏好核准原檔直傳**：JPEG／PNG MIME 防呆、非空且最多 5 MiB，multipart 只含一個原始 `image`。不呼叫 Canvas／`compressImage`、不使用 20 MiB 上限、不做前端 APNG 清洗或額外解碼驗證。動畫、格式／像素／完整 decode、EXIF、白底與 1568 縮圖由後端負責。共享 helper 及其 AI callers 均未修改，沒有把透明 PNG 的 alpha 提前壓成黑底。
- `useVendorImageMutation` 成功後 invalidate `['vendor', id]` 與 `['vendors']`，不以圖片 metadata 拼裝／覆寫整份 vendor。文字表單 key 改為 vendor id + 基本資料實際值，不再以 updatedAt 重建；上傳與刪圖的 refetch 均保留未儲存文字。
- GET binary 僅留 component 記憶體，使用 credentials、AbortController 與 Object URL，不把 BLOB 放 React Query cache；依 revision 重載。換檔／取消／換 vendor／換帳號／權限變更／卸載時釋放 URL，卸載時中斷 GET／PUT／DELETE 等待。401/403/404 重新查 parent／sharing access；變更請求權限失效會立即撤掉原圖與本地選檔。中斷等待不保證伺服器未提交，UI 已明示。

### 已執行且通過

- repo root `make gen-api`（指定 `/tmp/opencode/vendor-images-target` 等四個環境變數）；另以 Node assertions 檢查生成 OpenAPI 的 binary／multipart／必回 image 契約。
- `frontend/`：`npm test -- --run`：**27 files、240 tests passed、0 failed**；其中新照片測試 **18 cases**，涵蓋原始 bytes PUT／credentials／boundary、取消與同檔再選、JPEG/PNG 限制／5 MiB、後端 invalid-image 拒絕、413／429／網路失敗可重試、刪除取消／失敗保留、唯讀預覽、GET／mutation 401/403/404、URL 釋放、abort、換 vendor／帳號、revision 載圖與 query invalidation、上傳及刪圖後未儲存基本資料保留。
- `npm run typecheck`、`npm run build`、`npm run lint` 成功。Lint 有 **10 個既有 warnings**（原有檔案），新照片實作沒有新增 warning。曾發現測試用 `FormData.keys()` 不在現有 TS lib 宣告中，已改用 `forEach` 並重跑通過；不需要修改 tsconfig。
- `git diff --check` 通過。未改三個禁止的 LINE 檔、backend source、e2e、shared compress helper；未 commit／push／部署。

### D／主 agent 仍需整合驗收

- 本階段未執行 `make check`／後端 full tests；請沿用 §10 Cargo target 環境，避免再次填滿原 filesystem。
- 尚未做真實瀏覽器手機／桌面操作或截圖；RTL 不代表實際圖片 rendering、字體密集菜單可讀性、焦點回復／視覺版面均已人工驗收。請補上傳→重新整理→放大→替換→刪除及群組權限流程。
- `docs/deploy.md`／必要 `docs/design.md` 備份與隱私說明、公開點餐／OG／LINE 無洩漏、真實程序重啟／備份還原、PUT 與刪店家交錯等 P4 驗收仍由整合者完成。§10 結尾列出的 P2/P3 待辦已由本節取代，其餘未測項不變。

本階段檔案：`frontend/openapi.json`、`frontend/src/shared/api/{schema.d.ts,types.ts}`、`frontend/src/features/vendors/{hooks.ts,VendorDetailPage.tsx,VendorImageSection.tsx,VendorImageSection.test.tsx}`，以及本文件。

### P3 手機觸控尺寸收尾（2026-10-29）

- 主 agent 完成產品與截圖審查後，局部補齊 44px 觸控基準：照片 section 的 descendant buttons 使用 `min-h-11`，涵蓋選檔／上傳／取消／重試，以及 ConfirmDeleteButton 初始／確認／取消；預覽 trigger 額外 `min-w-11 min-h-11`，圖片仍依原比例與既有最大尺寸顯示，不固定成 44px。
- 僅本 feature 的 DialogContent 以 `data-slot=dialog-close` selector 補關閉按鈕最小 44×44px；未修改 shared 元件預設或 LINE 檔。
- 本輪重跑 `npm test -- --run src/features/vendors/VendorImageSection.test.tsx`（**18 passed**）、`npm run typecheck`、`git diff --check` 均通過；未重跑無關 full suite／build，未 commit。
- 交 D 針對上述按鈕與 Dialog 關閉觸控區做局部瀏覽器複驗；本輪未自行重新截圖／做瀏覽器尺寸驗證。

## 12. D 獨立整合／驗證紀錄（2026-10-29，主要功能已由主 agent 驗收）

### 已實際通過

- 沿用 §10 四個 Cargo 環境變數執行 repo root `make check`：version sync、完整 `cargo fmt --all --check`、完整 `cargo clippy --workspace --all-targets -- -D warnings`、完整 `cargo test --workspace` 均通過，**332 tests passed、0 failed**（115 + 24 + 52 + 80 + 31 + 30）。Log：`/tmp/opencode/vendor-images-make-check.log`。
- `make check` **不是整條成功**：在 Makefile:36 的 `git diff --exit-code -- frontend/openapi.json` 因本功能尚未 commit 的預期生成差異停止，並非內容漂移。另行 `cargo run -q -p server --bin export_openapi > /tmp/opencode/vendor-images-openapi-verify.json` 後 `cmp` 與 working-tree JSON 完全相同；保存既有 TS 後再執行 `npm run gen:api`，`cmp` TS 也完全相同。未重設／暫存／commit 來繞過 dirty gate。
- 接續實際執行剩餘 frontend checks：`npm ci --prefer-offline --no-audit --no-fund`、`npm run gen:api`、`npm run typecheck`、`npm test -- --run`（**27 files／240 tests passed**）、`npm run build`、`npm run lint` 均成功；lint 仍為 10 個既有 warnings。
- frontend build 完成後，用相同 target dir 執行 `cargo build --release -p server`；E2E 透過新增的 `E2E_SERVER_BINARY` 指定 `/tmp/opencode/vendor-images-target/release/server`，不使用 repo 內舊 release。
- `e2e/` 的 `../frontend/node_modules/.bin/tsc --noEmit`、Playwright `--list` 通過；在既有 `auth-directory.spec.ts`／`sharing-groups.spec.ts` 擴充案例，因此 mobile、desktop 均被現有 testMatch 收錄。
- 完整 E2E 最終 **31 passed（55.8s）**：`cd e2e && CHROME_PATH=/usr/bin/google-chrome E2E_SERVER_BINARY=/tmp/opencode/vendor-images-target/release/server npx playwright test`。涵蓋手機 390×844 與桌面 1280×800 的選檔取消／同檔再選、確認前不存檔、上傳／reload／JPEG naturalWidth、放大與 Escape／焦點回復、替換後 revision／1568 縮圖、刪除取消／確認／reload，以及上傳／替換／刪除均不吃掉未保存電話草稿。群組 member 可看／預覽、沒有變更控制，直接 PUT／DELETE 也被 404 拒絕；移除 membership 後 GET 404、重新開頁撤圖。匿名公開 DTO 沒有 image metadata／URL，公開 HTML 不含 revision／圖片路徑，公開頁沒有圖片 section，持公開 token 未登入仍無法讀圖（401）。Log：`/tmp/opencode/vendor-images-e2e-final.log`。
- 首輪為 29 passed／2 新案例失敗，原因是測試 helper 的 1800×1200 noisy PNG 超過 5 MiB，UI 正確拒絕，並非產品錯誤；只將 fixture 改為 1800×600（仍驗證長邊縮至 1568）並加檔案容量 assertion，未放寬產品限制。修正後完整 suite 成功一次即停止，沒有再次重跑完整 suite。
- `cd e2e && node vendor-image-persistence.mjs /tmp/opencode/vendor-images-target/release/server` 通過：以 **獨立真實服務程序** 建店／存圖，SIGTERM 停機後重啟原 file DB，驗證相同 metadata 與 JPEG bytes；再停機 tar 整個測試資料目錄、還原至另一目錄、啟動新程序，session／metadata／圖片 bytes 仍一致。另比對服務提供的 index.html 及 entry JS 與新 `frontend/dist` bytes 一致，確認不是舊前端。沒有開啟或改動 `backend/data`。測試資料／備份：`/tmp/opencode/vendor-image-persistence-qFepz8/`；紀錄：`/tmp/opencode/vendor-images-persistence.log`。
- 停機後再以 Python stdlib SQLite 唯讀檢查 source／restored 兩 DB：`PRAGMA integrity_check` 均為 `ok`、`foreign_key_check` 均無違規，圖片 row 各一筆、revision／151916 bytes／1200×1000 相同，BLOB SHA-256 均為 `106af2ef7441dc56a5c94cc3bcb6bd8f7637a429798cb1c9056b1beb3df2e04f`。結果保留於上述測試目錄 `integrity.json`。

### 安全／範圍核對

- 已讀 routes、normalize、repo access／image transactions、前端選檔與表單整合。GET 同一 snapshot 檢查 parent membership 再讀 BLOB；PUT／DELETE 在 write transaction 重查權限；shared creator 不能繞過成員資格；metadata 查詢不取 BLOB。此為程式碼檢查，不能當成所有併發 interleaving 的實測。
- 最終契約未變：一店一張 SQLite 私有 JPEG BLOB、僅 JPEG／PNG ≤5 MiB 原檔直傳、後端長邊 1568、last-successful-commit-wins；沒有新增 storage/env/volume 或 public 圖片 URL。依賴變動是將原本已存在於鎖檔的 transitive image/JPEG/PNG codecs（及 crc32fast）新增為 workspace／crate 直接依賴，另調整 tokio runtime／測試依賴宣告；Cargo.toml／Cargo.lock 確有變動，不能概括宣稱「未新增直接依賴／Cargo 沒變」。D 的整合驗證本身沒有增加套件。
- `docs/deploy.md` 已補原檔／正規化／權限／AI 區分、備份容量及敏感性、邏輯刪除與舊備份還原風險。此次演練是本機程序與 tar archive，**沒有 Docker/uid 10001/正式 volume 演練**。

### 截圖與未驗項

- 首輪截圖保留於 `/tmp/opencode/vendor-images-review/`（mobile／desktop 的 saved、preview、readonly）；主 agent 回報已親看四張主要圖片，並審查後端 routes／repo transaction 與前端 state，主要功能與既有測試已驗收。這批 preview 為 full-page 截圖，固定 header／overlay 只覆蓋當時 viewport，不宜當作真實視窗構圖；§13 的新 viewport preview 取代其作為最終 44px 局部視覺證據。真機視覺仍未驗。
- 未實測 production Docker image／反向代理／實際 LINE app／真 AI key；未做 codec RSS/CPU 壓測或 PUT 與刪店家的完整交錯壓測。64 MiB 不代表 RSS 硬限制。
- §9 的複合核取方塊仍保留未勾；自動測試通過不代表全部視覺、密集菜單可讀性或未測維運情境已驗收。未 commit／push／部署，未修改 LINE 三檔或產品程式。

本階段改動僅 `e2e/playwright.config.ts`、`e2e/tests/{auth-directory.spec.ts,sharing-groups.spec.ts}`、`e2e/vendor-image-persistence.mjs`、`docs/deploy.md`、本計畫；不需要額外更新非 vendor fixture。最終 `git diff --check` 通過。

## 13. D 的 44px 局部收尾複驗（2026-10-29，待主 agent 最終簽收）

- 主 agent 已確認 §12 主要功能與既有測試驗收通過；P3 尺寸修正後的 18 RTL tests／typecheck 紀錄見 §11「手機觸控尺寸收尾」。D 未修改產品／shared UI／LINE 三檔。
- D 重跑 `frontend/ npm run build`（含 tsc），再沿用 §10 四個 Cargo 環境變數執行 `cargo build --release -p server --manifest-path ../backend/Cargo.toml`，使用新建的 `/tmp/opencode/vendor-images-target/release/server` 內嵌新 frontend/dist。Build log：`/tmp/opencode/vendor-images-44-release.log`。
- 永久擴充既有 `auth-directory.spec.ts`，mobile／desktop 各增加 1×1 PNG 案例：待上傳與已儲存 tiny trigger、兩種 dialog close、載圖失敗／重試，以及既有照片案例的空狀態、選檔確認／取消、已存照片、刪除確認／取消。全部按鈕以 **實際 boundingBox 的 width、height ≥44 CSS px** 斷言，沒有 epsilon／向上取整／改 viewport 逃避門檻。量測／preview 截圖前等待可見、兩個 animation frames、Web Animations 的 finished promises，再兩個 frames；不在縮放動畫中途量測或擷取。
- `e2e/ ../frontend/node_modules/.bin/tsc --noEmit` 通過。僅執行 `CHROME_PATH=/usr/bin/google-chrome E2E_SERVER_BINARY=/tmp/opencode/vendor-images-target/release/server npx playwright test --grep 'vendor reference photo|vendor image tiny|shared directory is read-only'`：**6 passed（8.2s）**，涵蓋 mobile 390×844／desktop 1280×800；沒有重跑完整 suite。先前完整紀錄仍為 §12 的 31 passed；新增 tiny 案例使總收錄數增加 2，不把這次局部 6 例冒稱為完整 33 例已跑。
- 初次局部跑測 5 passed／1 failed，原因是四個 worker 同時上傳碰到服務固定兩個處理 jobs 的合法 429（UI 正確呈現忙碌並保留待上傳圖）。已將 E2E 預設 workers 固定為 2，與原 CI／真實 processing budget 一致；沒有改產品 semaphore／limiter、跳過測試、放寬尺寸或自動 retry 掩蓋失敗。兩 worker 的六例全部通過，專門的 limiter／併發壓力驗證仍屬後端既有測試，不由這批 UI 案例取代。
- HTML report 的尺寸附件匯出至 `/tmp/opencode/vendor-images-review-44/touch-targets.json`：**20 組量測、50 個按鈕 bounding boxes 全數 ≥44×44**，兩平台的 tiny local／saved trigger 與 dialog close 均為 **44×44**；其餘操作按鈕高度均 44，寬度不小於 44。測試 log：`/tmp/opencode/vendor-images-44-e2e-final.log`。當次 HTML report 為 `e2e/playwright-report/index.html`（局部執行會覆蓋前次 report）。
- 新 preview 改為 `fullPage: false`，D 已親看以下四張動畫完成後的 **viewport** 截圖：
  - `/tmp/opencode/vendor-images-review-44/mobile-vendor-image-preview.png`
  - `/tmp/opencode/vendor-images-review-44/desktop-vendor-image-preview.png`
  - `/tmp/opencode/vendor-images-review-44/mobile-vendor-image-tiny-preview.png`
  - `/tmp/opencode/vendor-images-review-44/desktop-vendor-image-tiny-preview.png`
  手機圖為 780×1688 raster（DPR 2，CSS viewport 390×844），桌面為 1280×800。正常菜單圖完整，close 可見，modal 已穩定且不再有先前動畫中途的半透明／full-page fixed overlay 構圖誤導。tiny 圖放大為純深色方塊是 1×1 RGB fixture 本身，不是照片載入失敗。
- 本次沒有重跑後端完整 checks、前端完整 240 tests、整套 E2E、重啟／備份還原；這些沿用 §12 的實際紀錄，不宣稱尺寸修正後重新全跑。仍未驗 Docker／uid 10001／正式 volume／反向代理／實機 LINE／真 AI key／codec RSS/CPU 壓測／完整 PUT 與刪店家交錯。主 agent 尚需最終簽收本次 44px 與新 viewport 圖；沒有 commit／push／部署。

## 14. 最終主 agent 驗收紀錄（2026-10-29）

- 主 agent 已親看最新 mobile／desktop viewport preview，確認 44px 修正通過；已審查後端 routes／repo transaction、前端 state 及 E2E workers=2 變更（CI 原本即為 2，接受 local 對齊），並再次確認 `git diff --check` 通過。§13 的待簽收狀態由本節正式結案。
- **最終核准本輪 P0–P4 實作／自動測試／截圖／本機持久化與備份還原驗收**。測試證據沿用 §12 的後端 332 tests、前端 240 tests、完整 E2E 31 passed，以及 §11／§13 尺寸修正後的 18 RTL tests、typecheck／build、局部 E2E 6 passed 與 50 個按鈕 ≥44×44 量測；不宣稱最後尺寸修正後重跑完整 suite。
- 保留未驗界線：Docker／uid 10001／正式 volume、反向代理、真機／實際 LINE app、真 AI key、codec RSS/CPU 壓測及完整 PUT／DELETE／刪店家交錯；64 MiB 不提供 process RSS 硬上限保證。§9 複合核取方塊不以本輪範圍簽收一律勾滿。
- 使用者原有「LINE 不顯示未訂」未提交改動完整保留：`frontend/src/features/group-orders/lib.ts`、`frontend/src/features/group-orders/lib.test.ts`、`frontend/src/features/group-orders/GroupOrderDetailPage.test.tsx`；本輪整合者沒有覆寫、重設或代為提交。功能及文件改動亦維持未 commit／未 push／未部署。
- 本次最終收尾只更新本文件完成狀態與驗收紀錄，未更動其他檔案、未重跑測試、未 commit。

## 15. AI 菜單匯入預設同時保存照片（2026-10-29，Phase A–D 完成，主 agent 驗收通過）

**依主 agent 修正，採後端 opt-in 暫存＋apply 同交易保存。撤回先前 IDB／receipt／前端兩階段方案。** 本節取代 §1 舊版「不做 AI 自動留存」的後續範圍，但不改寫 §10–§14 的既有驗收。§15.1–15.5 保留實作前規劃基線（其中「目前」與「尚未執行」指規劃當時）；實際完成證據以 §15.6–15.7 為準。既有 backend／frontend／e2e／LINE dirty 工作完整保留，未 commit。

### 15.1 真實流程與可直接沿用的 cleanup hook

- 入口：`frontend/src/features/vendors/VendorsPage.tsx` 是 vendor create；`vendors/MenuSection.tsx` 是既有 vendor menu import。`ai-import/ImportDialog.tsx` 先 `compressImage(file)` 產 JPEG，再 `hooks.ts::startImport` POST；收到 run 後清掉 File 並導航 `/ai-import/{id}`。確認頁 `AiImportPage.tsx`／`lib.ts::buildApplyBody` 只送勾選變更，沒有跨 route 保留圖。後端 run 目前完全不存圖。
- `backend/crates/application/src/ai_import/service.rs::apply` 已解析真正目標 ID（新店 scoped 同名時會改用既有店家）；`infrastructure/src/ai_import_repo.rs::apply` 已以 `BEGIN IMMEDIATE` 將 proposed→applied CAS、scope／vendor 授權與全部資料變更包在一起。**直接把暫存圖移入同一交易即可**，不需要前端拿新店 ID 再 PUT，也不需要 apply receipt。
- 現有清理入口是 `application/src/maintenance.rs::Maintenance::run_once` → `MaintenanceRepo::fail_stuck_import_runs(created_before, now)` → `infrastructure/src/maintenance_repo.rs`。目前只將超過 **10 分鐘**的 running run 標為 failed/interrupted，**沒有 proposed/applied run retention 或圖片刪除**，所以不能假稱已有 24h／每人 3 張機制。
- 排程已在 `server/src/main.rs::maintenance_loop`：啟動約 30 秒後首次執行，再依 `MAINTENANCE_INTERVAL_HOURS`（`server/src/config.rs` 預設 **24 小時**）執行。最小擴充是在既有 `fail_stuck_import_runs` 的短交易內，標記 interrupted 後刪除到期或 terminal run 的暫存圖；更新 trait 註解即可，回傳值仍只計 failed runs。**不新增 scheduler、config、quota UI 或 maintenance report 欄位。**

### 15.2 最小儲存與 normalize 共用

**選一張 run FK 子表，而非把 BLOB 塞進一般 run aggregate**；避免每次 GET／clone／save run 都載入或覆寫圖。新增 `backend/migrations/0008_ai_import_photos.sql`（目前最高為 0007，實作時再確認）：

```sql
CREATE TABLE ai_import_photos (
  run_id TEXT PRIMARY KEY REFERENCES ai_import_runs(id) ON DELETE CASCADE,
  data BLOB NOT NULL CHECK (length(data) BETWEEN 1 AND 5242880),
  width INTEGER NOT NULL CHECK (width BETWEEN 1 AND 1568),
  height INTEGER NOT NULL CHECK (height BETWEEN 1 AND 1568),
  expires_at TEXT NOT NULL
);
```

- data 一律為已 normalize 的 JPEG，不存原檔、檔名、base64 或其他影像副本；expiry 固定 `run.created_at + 24h`，重試不延長。一 run 最多一張，owner／scope 沿用 parent，不另建圖片讀取 API，也不把 bytes 放 run DTO。
- 開始辨識前，僅當 vendor + `savePhoto=true` 時重用 `infrastructure/src/vendor_image.rs::normalize`；將 `api/src/routes/vendor_images.rs::start_normalization` 改成可供同層呼叫的 `pub(super)` helper，沿用現有 `spawn_blocking` 與 `AppState.image_processing` semaphore／permit 生命週期，不建新影像服務或 framework。忙碌回既有 429；normalize 失敗回 `invalid_image`，**不開始 AI、不建立含圖 run**。
- 瀏覽器仍以既有 `compressImage` 接 JPEG／PNG／WebP，送 JPEG；後端把 normalize 後的 JPEG **同時作為 AI ImageInput 與暫存 bytes**，apply 時直接存，不再次 decode／encode。opt-in API 輸入須可由現有 normalize 處理（JPEG／PNG）；直接傳 WebP 並 opt-in 時清楚拒絕，UI WebP 已先轉 JPEG 因而正常可用。不擴 vendor image endpoint 的 WebP codec；未 opt-in 的既有 AI JPEG／PNG／WebP 路徑不變。
- `compressImage` 的 canvas 目前未先填白底；最小補白底再 draw，避免透明 PNG 已在瀏覽器變黑後，後端無從復原。呼叫端驗非空、產物 ≤5 MiB；後端 normalize 仍是信任邊界。
- `ImportRunRepo::create` 增加 optional `NormalizedVendorImage` input，在同一交易 INSERT run＋photo。service 已有 scope／target 授權及 AI daily quota 檢查，保留在 create 前。一般 `find` 只補 expiry metadata（不 SELECT BLOB）；`save` 的 proposed 轉換保留圖，failed／discarded 轉換在同一 CAS 交易 DELETE 圖。
- **容量只沿用已有機制**：5 MiB 圖／6 MiB request、session auth、既有 AI actor rate limit／daily quota、24h 可用期限與既有 maintenance 清理。沒有每人 3 張機制，不新增該限制或管理 UI。取消等待是 HTTP abort，不保證 server 停止；未完成 run 由 interrupted hook 清理，已 proposed 的未確認圖由 TTL 清理，永不提前改 vendor image。
- 24h 是嚴格的**可使用期限**，不是精準磁碟抹除時間：apply 交易依 expiry 判斷，即使排程尚未刪 row 也不可存；正常到期圖於下一輪 maintenance 邏輯刪除（預設可能再等近 24h，停機時更久）。WAL／備份與現有 §3 一樣不保證安全抹除。UI 可寫「確認前暫存 24 小時，逾期無法自動保存」，部署說明另交代排程刪除延遲，不承諾精準 24h 實體移除。

### 15.3 API 與 atomic apply／discard

| 契約 | 最小變更 |
|---|---|
| `POST /ai/import-runs` multipart | 新增 `savePhoto` boolean，缺省 false（保留舊 callers 隱私行為）；UI vendor 預設傳 true。拒絕非法值／重複該欄；非 vendor 傳 true 回 422，不忽略並偷存。其他模式傳 false 或省略維持原行為 |
| `ImportRunDto` | 加 `hasPendingPhoto: boolean`、`pendingPhotoExpiresAt: string \| null`，只回 metadata；hasPendingPhoto 表示 running/proposed 且存在未到期暫存，service 以 clock 計算，不將過期但未清 row 當可用圖。舊 run 為 false/null |
| `POST /{runId}/apply` JSON | 保留 accepted，新增 optional `savePhoto`；省略時以**目前未到期 pending**為預設。UI 明確傳 checkbox 值；false 匯入資料但不保存照片，交易結束仍刪暫存。非 vendor 的 true／沒有可用圖的明確 true 回 422 |
| apply response／discard | 維持現有成功 response；不新增 receipt、圖片 PUT 或局部成功狀態。discard 仍回 204，但狀態 CAS 與暫存 DELETE 必須同交易 |

`service.rs::apply` 繼續既有驗證及建構 steps，只把「是否存圖＋已解析的唯一 VendorId」傳進 `ImportApplyRepo::apply`（例如 optional photo target，不新建通用工作流）。**savePhoto=true 僅允許 proposal 與 accepted 都有唯一 vendor target**；沒有／多個 vendor target 回明確 422，UI 停用保存並提示「本次有多個店家，請取消同時儲存照片後匯入」。不默認第一家；無選中資料變更時沿用原本 apply 禁用，不為照片單獨建立店家。

同一 `BEGIN IMMEDIATE` 的順序：

1. 沿用 proposed→applied CAS 與 scope／各 target 管理權重驗；執行原有 create／modify steps。
2. 若要保存，交易內讀這個 run 的暫存圖並重驗 `expires_at > now`；過期／缺圖使**整個交易 rollback**，回 422，提示重新辨識或取消勾選後重試資料匯入。不得沉默跳過照片卻回整體成功。
3. 在真正解析到的新建／scoped 同名／指定既有 vendor 上 UPSERT 圖，更新 revision／parent timestamp；重用既有圖片交易 SQL。將 `directory_repo.rs::put_image` 的授權＋寫圖段抽成 `pub(crate) put_image_tx`，原 PUT 及 AI apply 共用，不在 apply 裡呼叫會另開交易的 `VendorRepo::put_image`。新 vendor 已在此 tx 建好，可直接依同一授權規則查到。
4. **不論保存與否**，DELETE 該 run 暫存圖，commit。任一步出錯，狀態、店家／菜單、照片及暫存 DELETE 一起 rollback；舊照片與可重試 pending 均不受損。discard／AI failed 的 `save` 同樣 CAS＋DELETE 一起 commit。

refresh 直接 GET run metadata，確認前仍能預設勾選；不需要把 File 帶過 route。套用中斷／409 時 refetch：已 applied 表示整筆交易完成，proposed 才可再確認，GET 失敗只說尚無法確認結果。現有 CAS 防止重複建立菜單／店家；不要重新 start AI 猜結果。新店成功回應遺失後沿用既有完成頁回店家列表即可，不為「直達新店」另加 receipt。**本方案沒有「菜單成功照片失敗」：DB 失敗就是全 rollback，故不建立兩階段部分成功文案或重試按鈕。**

### 15.4 UI 與 ownership／phases

- `ImportDialog` 僅 vendor 顯示預設勾選「確認匯入後，同時儲存菜單照片」，說明「辨識用壓縮圖片會於伺服器暫存 24 小時；確認前不會改動店家照片」。開始時取消勾選即不暫存；關閉尚未送出的 Dialog 無副作用。
- 確認頁由 `hasPendingPhoto` 預設勾選，可取消後隨 apply 傳 false（不為 checkbox 另做 DELETE endpoint）；已有圖提示「將取代目前參考照片，可取消勾選」。查真正 existing proposal target 的 vendor metadata，不能只看入口 id；多 target 停用保存並提示。roster／address／null mode 不顯示。
- 已過期時不維持可保存假象；expiry 尚在可顯示到期提示，子表已清除則只說「沒有待存照片（未暫存或已到期）」，不為分辨歷史狀態加欄位。可取消照片後匯入資料；server 422 expiry 後 refetch metadata，失敗保留確認內容、可重試。互斥停用 apply／discard，成功沿用現有 invalidation／導航；不碰獨立 `VendorImageSection` 上傳流程及 auth logout。

以下是工作分配規劃，不代表授權再 spawn；Backend 路徑縮寫同 §2：

| Phase／owner | 具體檔案 | 完成條件 |
|---|---|---|
| A Backend owner | `backend/migrations/0008_ai_import_photos.sql`；`domain/src/ai_import/run.rs`（只加 expiry metadata，必要 `mod.rs`）；`application/src/{ports.rs,ai_import/service.rs}`；`infrastructure/src/{ai_import_repo.rs,directory_repo.rs}` | optional normalized photo 與 run 同建；單店真實 ID 解析；apply CAS＋圖＋菜單＋清理同交易；failed／discard 清理同交易；run GET 不讀 BLOB |
| A 同一 Backend owner（避免搶既有圖片檔） | `api/src/routes/{ai_import.rs,vendor_images.rs}`；`application/src/maintenance.rs`（更新既有 hook 註解）；`infrastructure/src/maintenance_repo.rs`；`infrastructure/src/ai_import_tests.rs`、`api/src/routes/ai_import_tests.rs` 及必要既有 image tests | normalize helper 最小共用、session／AI 限流不變、metadata／savePhoto OpenAPI；既有 maintenance hook 刪 expired／terminal 圖；不需改 server wiring/main/config |
| B 主 agent／整合者 | **獨占** `frontend/openapi.json`、`frontend/src/shared/api/schema.d.ts`、必要 `types.ts` | 沿 §10 Cargo 環境執行 `make gen-api`，savePhoto optional、metadata 必回／expiry nullable 與後端一致 |
| C Frontend owner | `frontend/src/features/ai-import/{ImportDialog.tsx,AiImportPage.tsx,hooks.ts}` 與既有 tests；`shared/lib/compressImage.ts`／`compressImage.test.ts` | 兩入口預設勾選、metadata refresh、替換／多店／到期提示、明確傳 bool、透明白底；沒有新持久 store／狀態機／vendor PUT |
| D 主 agent／驗收整合者 | `e2e/tests/ai-import.spec.ts`、必要既有 helpers；`docs/{design.md,deploy.md,vendor-images-plan.md}` | 手機／桌面 reload 與 atomic 契約驗收；文件改為僅 opt-in 正規化圖暫存，交代 TTL／maintenance／備份；記錄真正通過項 |

### 15.5 必要測試（尚未執行）

- **後端開始／讀取**：缺省／false、roster／address 不存；非 vendor true 拒絕。真 JPEG／PNG normalize 成 JPEG，失敗在 agent 呼叫前拒絕；agent 與 pending bytes 相同，不保存原檔；metadata 無 bytes、舊 run false/null、owner／scope 權限不回歸。
- **後端交易**：vendor create、scoped 同名及既有 menu import 各驗照片落在正確店家；apply false 只改資料並清 pending；多 vendor true／missing／expired pending 拒絕。注入圖片寫入失敗，確認 status／vendor／menu／舊圖／pending 全 rollback，重試成功；雙 apply／apply 對 discard 僅一個 CAS 勝出，不重建店家菜單。discard／failed 同交易清圖，刪 run FK cascade。
- **清理／持久化**：file-backed DB 重開後 proposed pending 仍可用；固定 clock 驗 24h 邊界不延長；既有 maintenance 能標記 stuck 並清圖、刪 expired／terminal 圖，但不刪未到期 proposed 圖、不刪已保存 vendor 圖。
- **前端既有 RTL／MSW**：vendor 預設及取消／替換警語、非 vendor 不顯示；metadata reload 與 expiry／多 target 提示；apply 明確 true/false；失敗可重試且不送 vendor image PUT；白底 JPEG、WebP→JPEG、最終 5 MiB 檢查。既有 AI／照片 section 不回歸。
- **現有 Playwright（無真 AI key）**：手機／桌面各驗新店及既有菜單：上傳→確認頁 reload→套用→詳情 reload 有圖；取消照片／discard 保留舊圖。apply 真提交後丟 response，再 GET applied 不重送、不增 vendor/menu；並驗 server 拒絕／rollback 時資料與照片皆未變。使用現有 harness，不加 framework。

實作後檢查沿用 §9／§10 指令與 Cargo target；dirty generated diff gate 依 §12 如實記錄，不 reset／commit 繞過。**本次只修訂方案，沒有改產品碼或跑產品測試；既有驗收不可當成本節已完成。**

### 15.6 Phase A 後端完成／前端交接（2026-10-29）

- 已實作 `0008_ai_import_photos.sql`；run 與 optional normalized JPEG 同交易建立，期限固定 created_at + 24h。一般 find 僅查 expiry，不讀 BLOB；service 以 clock 隱藏到期／terminal metadata。原未 opt-in AI WebP／magic-bytes 輸入行為保留。
- apply 共用 `directory_repo::put_image_tx` 的授權與寫圖，將原有 steps、run CAS、圖片取代及 pending DELETE 放同一交易；僅允許唯一 proposal／accepted vendor，且 photo target 必須實際在本交易 accepted steps 中寫入。缺圖／到期／圖片寫入失敗全部 rollback；false 可只匯入菜單並清 pending。failed／discard 的 CAS＋清圖、maintenance 的 interrupted＋expired／terminal 清圖亦各自同交易。
- **前端 contract**：start multipart `savePhoto` 僅接受字串 `true`／`false`，省略 false；重複欄位 400 `invalid_multipart`，非法值／非 vendor true 為 422 `invalid_save_photo`（field=`savePhoto`）。true 僅接受可 normalize 的靜態 JPEG／PNG；直接 WebP true 為 422 `invalid_image`，false／省略仍接受既有 AI WebP。處理額度與獨立照片 PUT 共用兩個 jobs，忙碌 429 `rate_limited`／Retry-After；AI actor limiter／quota 不變。
- **前端 contract**：start／GET run 必回 `hasPendingPhoto: boolean`、`pendingPhotoExpiresAt: string | null`；無 bytes／URL。apply JSON `{accepted, savePhoto?: boolean}`：省略依目前未到期 pending 預設；UI 應明確傳 true／false。true 缺圖、過期、非 vendor、無／多 vendor target 均 422 `invalid_save_photo`（field=`savePhoto`）；不會部分成功。成功 response 不變；409 refetch run，applied 表示全部已提交；discard 仍 204。取消照片請直接 apply false，不需再 PUT／DELETE 圖片。
- 新增 9 個 pending-photo tests，並將既有 agent timeout 測試改成帶 pending 以驗 failed 清理。實測包含：原 WebP／安全預設、normalize 前拒絕不建立 run／不耗 daily quota、共用處理額度、metadata reload／OpenAPI、scoped 同名與群組撤權、normalized agent bytes、24h 邊界、寫圖錯誤時 menu／run／原圖／pending rollback、雙 apply 與 apply/discard CAS、取消存圖、failed／discard／TTL／terminal 清理、FK cascade、另開 file-backed pool 可讀回 pending metadata。另開 pool 不等於完整程序重啟；真 AI、瀏覽器及 Phase C/D 整合尚未驗。
- 已沿 §10 四個環境變數實際通過：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test -p api -p infrastructure pending_photo`（9 passed）、`cargo test --workspace`（**341 passed，0 failed**）、`cargo run -q -p server --bin export_openapi`、`git diff --check`。完整 tests log `/tmp/opencode/vendor-images-phase-a-tests.log`；OpenAPI 匯出 `/tmp/opencode/vendor-images-phase-a-openapi.json`，可供整合者生成。未修改 frontend／generated／LINE 三檔、server scheduler/config；所有原 dirty 工作保留，未 commit／push。
- 本階段實際增修：`backend/migrations/0008_ai_import_photos.sql`、`domain/src/ai_import/run.rs`、`application/src/{ports.rs,ai_import/service.rs,maintenance.rs}`、`infrastructure/src/{ai_import_repo.rs,ai_import_tests.rs,directory_repo.rs,maintenance_repo.rs}`、`api/src/routes/{ai_import.rs,ai_import_tests.rs,vendor_images.rs}`，以及本 §15 結果紀錄。Phase B 的 generated files 仍由主 agent 獨占。

### 15.7 Phase B 生成與 C 前端完成／D 交接（2026-10-29）

- 依本次明確分派取得 B generated files 與 C frontend ownership；沿 §10 四個 Cargo 環境變數執行 `make gen-api` 成功，更新 `frontend/openapi.json` 與 `frontend/src/shared/api/schema.d.ts`。Node assertions 確認 start／apply `savePhoto` optional、run 兩個 metadata 必回且 expiry nullable；不手改 generated schema、不新增平行 DTO。
- 新建店家及既有菜單的共用 ImportDialog 僅 vendor 預設勾選照片；開始前可取消並傳 false，名單／地址／未指定類型不顯示且不提交 true。清楚提示只有勾選才在 server 暫存辨識用壓縮圖，24 小時後無法保存、確認前不改店家照片，保存可能取代已有照片；關閉尚未送出的 Dialog 無請求。
- 確認頁從 GET metadata＋expiry 初始化，可在無 File／本機 store 的全新頁面恢復勾選；到期 timer 阻止繼續存圖，可取消後僅匯入資料。查真正 proposal existing target 的 vendor metadata 顯示替換警語，不誤用入口 targetVendorId。proposal 與 accepted 均須唯一 vendor；多店家保留勾選但阻擋 apply，明確要求取消，不偷偷選第一家。依核准計畫不允許照片-only，至少須一筆有效資料變更。
- vendor apply 明確傳 true／false，不呼叫圖片 PUT／DELETE、不新增 IDB／receipt／兩階段／部分成功狀態。apply／discard 互斥停用（共用 ConfirmActionButton 的初始／取消按鈕也遵守既有 pending prop）。422 invalid_save_photo 後 refetch metadata、保留編輯，可取消照片重試；網路／409／5xx 失敗也 refetch，只有 GET applied 才顯示完成。查詢失敗保留確認草稿並停用套用，明示尚無法確認結果及提供重試查詢；401／403／404 仍撤掉確認內容，不顯示舊私密資料。
- `compressImage` 在 draw 前填白底，再編碼 JPEG；ImportDialog 驗產物為非空 JPEG 且 ≤5 MiB，失敗不 start。AI WebP 仍經 Canvas 轉 JPEG；獨立店家照片原檔上傳仍只允許 JPEG／PNG，本次未改其流程。新增 checkbox 的 label 最小高度 44px，操作沿用 h-11；ImportDialog close 局部補 44×44，不改 shared Dialog 預設。
- **隱私與清理實際語意**：server `savePhoto` 省略／false 不保存辨識圖，只有 vendor true 存正規化 JPEG 至私有 pending 子表；確認 apply 才持久保存／取代店家圖，false 匯入資料並清 pending。24h 是嚴格使用期限，不是精準實體抹除；過期 row 於下一次 maintenance 邏輯刪除（啟動約 30 秒首跑，預設每 24h，停機更久），WAL／磁碟頁面／舊備份不保證安全抹除。`docs/design.md` 與 `docs/deploy.md` 已同步，沒有每人三張或新 scheduler 的承諾。
- **實際通過**：`make gen-api`、OpenAPI Node assertions；frontend `npm test -- --run`（**27 files／263 tests passed，0 failed**，比既有 240 增加 23 cases）、`npm run typecheck`、`npm run build`、`npm run lint`（10 個既有 warnings，未新增 warning）、`git diff --check`。新增測試涵蓋新店／既有預設與取消、關閉無副作用、JPEG／5 MiB 邊界防呆、WebP 輸入、白底 draw 次序、metadata reload、真正 existing target、到期／過期 refresh／多 target／禁止 photo-only、422 保留草稿與 false 重試、lost response→GET applied、GET 亦失敗時不假成功且保留草稿，以及名單／地址不回歸。第一輪完整 suite 發現地址測試把 compressImage module 全 mock 而未提供新增使用的上限常數，已最小修正 fixture 為 partial mock，再完整重跑通過，未改地址產品契約。
- 本階段實際增修：`frontend/openapi.json`、`src/shared/api/schema.d.ts`、`src/features/ai-import/{ImportDialog.tsx,AiImportPage.tsx,hooks.ts,fixtures.ts,ImportDialog.test.tsx,AiImportPage.test.tsx}`、`src/features/delivery-addresses/DeliveryAddressesPage.test.tsx`（僅 mock／回歸斷言）、`src/shared/api/errors.ts`、`src/shared/components/ConfirmActionButton.tsx`、`src/shared/lib/{compressImage.ts,compressImage.test.ts}`；以及 `docs/{design.md,deploy.md,vendor-images-plan.md}`。frontend 路徑皆以 `frontend/` 為前綴。
- **未做／交 D**：未修改 backend／e2e／獨立 VendorImageSection／三個 LINE dirty 檔，未重跑 backend 341 tests 或 make check，未做本輪手機／桌面瀏覽器、reload 後真圖片 rendering／44px boundingBox、真 AI／持久化／備份還原。既有 §12–§14 的瀏覽器證據不能當作新 AI 保存流程的驗收；需由 D 使用本輪 build 與 Phase A backend 完成整合。不 commit／push／部署。

### 15.8 D 獨立整合驗收（2026-10-29，通過，主 agent 已簽收）

- 先讀 §15 contract、start／apply service、run repo 的 CAS／圖片同交易實作、existing target 授權、normalize／maintenance hook、前端 checkbox／metadata／錯誤恢復流程及現有 tests。D 僅擁有 `e2e/**` 與本驗收紀錄，不修改產品碼；所有既有 dirty／LINE 改動保留，未 commit。
- 沿 §10 四個環境變數執行 `make check`：version sync、fmt、完整 clippy、完整 backend tests **341 passed，0 failed**（119 + 24 + 52 + 85 + 31 + 30）。在 Makefile:36 的 OpenAPI dirty diff gate 停止，**不宣稱 make check 整條成功**。獨立匯出 `/tmp/opencode/vendor-images-phase-d-openapi.json` 與 working-tree JSON `cmp` 一致；另以 `openapi-typescript openapi.json` 的 stdout 與現有 schema bytes 比對一致，不改寫／重設 generated files。Log：`/tmp/opencode/vendor-images-phase-d-check.log`。
- 另行跑剩餘 frontend 驗證：typecheck、full Vitest **27 files／263 tests passed**、build、lint 通過；lint 為 10 個既有 warnings。這次未重跑 `npm ci`。frontend build 後執行新 `cargo build --release -p server`，E2E 明確指定 `E2E_SERVER_BINARY=/tmp/opencode/vendor-images-target/release/server`，不使用 repo 舊 binary。Release log：`/tmp/opencode/vendor-images-phase-d-release.log`。
- 永久擴充 `e2e/tests/ai-import.spec.ts`，且在 desktop testMatch 明確加入該檔；兩 viewport 都收錄新 AI cases。fixture 僅碰 harness 的 disposable `/tmp/bento-e2e-3990/e2e.db`（實際依 `os.tmpdir()`），不碰任何現有使用者 DB；Python stdlib 查 pending bytes／將指定 run 設到期，不加 dependency。
- 新增瀏覽器案例涵蓋：新店 WebP／既有店透明 PNG 的真實瀏覽器 Canvas→JPEG、開始與確認預設勾選、GET/reload 後恢復、確認前舊 revision 不變、替換提示、成功後詳情 reload、JPEG 尺寸／sizeBytes、白底 pixel RGB ≥245，且最終 GET 圖片 bytes 與 **server normalized pending BLOB 完全相同**（不是拿原始 Canvas JPEG 誤比）。AI apply 沒有額外圖片 PUT／DELETE。
- 另覆蓋：開始 opt-out 無 pending、確認取消照片只寫菜單／保留舊圖、discard 實際刪 pending row、server fixture 到期→真 422／菜單與舊圖不變→refetch 提示→取消照片後只寫菜單；500 fixture 不假成功且保留草稿／pending、apply 與查結果同時失敗時停用重送並保留草稿、真實提交後故意丟 response→GET applied→只送一次 apply／reload 不再改 revision。名單與地址無 checkbox、不傳 savePhoto、DB 無 pending；未登入 GET run 為 401。
- 已親看並保存 **8 張 viewport screenshots** 至 `/tmp/opencode/vendor-images-ai-review/`：`{mobile,desktop}-ai-{new,existing}-{confirm,saved}.png`。mobile CSS 390×844／DPR2（780×1688 raster），desktop 1280×800。確認畫面 checkbox／24h 說明／existing 替換警語與底部操作皆可見，保存後白底紅色 fixture 正常顯示；不是實際菜單辨識內容或密集文字可讀性的驗收。兩入口確認頁 checkbox label 實測 height ≥44 CSS px；既有照片 44px cases 也隨 full suite 再跑，但不宣稱所有新增 AI controls 都量測過。
- 新測試開發前兩輪失敗為 E2E 本身：未攔截時 Chromium multipart 的 `request.postDataBuffer()` 為 null，改用既有 route 攔截模式擷取；discard 確認按鈕實際名稱為「確定放棄」，修正 selector。未修改產品、放寬限制或加 retries；相關取消／結果無法確認案例局部複驗 **4 passed（9.2s）**。
- **最終僅執行一次完整 E2E：52 passed、0 failed（1.9m），workers=2、retries=0**。包含 `ai-import.spec.ts` 手機／桌面各 10 cases（合計 20，其中 9 個新增 cases ×2，既有 flyer case 也新增 desktop 收錄），以及全部既有目錄／照片／群組／點餐／UI cases。命令：`cd e2e && CHROME_PATH=/usr/bin/google-chrome E2E_SERVER_BINARY=/tmp/opencode/vendor-images-target/release/server npx playwright test`；log `/tmp/opencode/vendor-images-phase-d-full-e2e.log`。E2E `tsc --noEmit`、最後 `git diff --check` 均通過。沒有產品 bug 需要退修。
- 原子寫圖故障 rollback、AI failed 清理、CAS 競爭、TTL maintenance／terminal 清理、scoped 同名真實 target 與權限變更由本輪實跑的既有 backend tests 覆蓋；瀏覽器 500 是攔截 fixture，**不冒稱已於瀏覽器注入真 DB 故障**。本次未重跑既已完成的備份／還原維運，未驗新 pending 的完整程序重啟；file-backed pool 重開仍僅是後端既有測試。真 AI、真機／LINE、Docker／正式 volume、反向代理、RSS/CPU 壓測與完整 interleaving 仍未驗。
- D 本次只改 `e2e/playwright.config.ts`（desktop 收錄）、`e2e/tests/ai-import.spec.ts`、本節紀錄；config 既有 `E2E_SERVER_BINARY`／workers=2 與其他 dirty E2E 檔皆保留。未改 backend／frontend 產品或 LINE 三檔、未開啟既有 userdata、未 commit／push／部署。前端完整 log `/tmp/opencode/vendor-images-phase-d-frontend.log`，lint log `/tmp/opencode/vendor-images-phase-d-lint.log`。

### 15.9 主 agent 最終驗收（2026-10-29）

- 主 agent 已審查 frontend `ImportDialog`／`AiImportPage`／hooks diff 與 backend apply 交易內圖片寫入／暫存清除，親看 `mobile-ai-existing-confirm.png`、`desktop-ai-new-saved.png`，並親跑 `git diff --check` 通過；正式核准 §15 本輪功能、自動化與畫面驗收。
- 證據沿用 §15.8 的 backend 341 tests、frontend 263 tests、完整 E2E 52 passed 及截圖；保留真 AI、真機、Docker、新 pending 完整程序重啟、RSS 等未驗界線，不聲稱全測。本次僅更新本文件狀態與簽收紀錄，未改產品／測試、未重跑、未 commit／push。
