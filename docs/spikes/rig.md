# Spike T4.0：rig + Anthropic

- 程式：`spikes/rig-spike/`（獨立 Cargo project，`[workspace]` 自成一體，不屬於 `backend/` workspace）
- 鎖定版本：`rig = { git = "https://github.com/Swind/rig", rev = "36f287befa9e6013579d7ba8a957a27f604d7d50" }`（facade crate `rig` 0.44.0，預設 features 即含 `agent`、`derive`、`reqwest`、`rustls`）
- 驗證方式：沒有 `ANTHROPIC_API_KEY`，所有 agent loop 都對本機 axum mock（`POST /v1/messages`）實跑；`cargo run` 依序跑 9 個情境並把每個 request body 存到 `spikes/rig-spike/target/recorded/`。
- 以下 rig 原始碼路徑皆相對於 `~/.cargo/git/checkouts/rig-*/36f287b/crates/`。
- **未能驗證的部分**（需要真實 API）：真實 Anthropic 是否接受 rig 送出的 `thinking: adaptive` + beta header、`strict: true` schema、`fallbacks` 欄位——mock 只能證明「rig 送了什麼」，不能證明「API 收不收」。T4.4 有 key 時必須實跑一次。另外 mock 的 SSE 路徑（`.stream()`）沒有跑，本 spike 只用 unary。

## 執行結果摘要

| 情境 | 結果 |
|---|---|
| happy（list_vendors → propose_vendor → end_turn） | `Ok`，3 次 HTTP，usage 為三次加總（in 3000 / out 150），proposals 收集到 1 筆，`organizer_id`/`run_id` 注入成功 |
| refusal（`stop_reason:"refusal"`） | `Err(PromptError::Provider(ProviderError::Response("…finish_reason=ContentFilter…")))`，1 次 HTTP |
| invalid-args（`price:"abc"`） | agent loop 不中斷：rig 回 `is_error:true` 的 tool_result，model 第二次修正後成功（3 次 HTTP） |
| business-invalid（`price:-5`，通過 serde 但違反業務規則） | 同上，錯誤文字原樣回給 model |
| overloaded 529 | `Err(PromptError::Report(ErrorReport{ retryable:true, http_status:Some(529), code:Some("overloaded_error") }))`，**只 1 次 HTTP（無內建重試）** |
| server 500 | 同上，`http_status:Some(500)`、`code:Some("api_error")` |
| hang + `tokio::time::timeout(2s)` | 2 秒後 `Elapsed` |
| hang + reqwest client timeout 2s | `Err(PromptError::Report(ErrorReport{ kind: Http, retryable:true }))` |

## Q1 建 client（env / 自訂 base URL）與 model id

```rust
use rig::providers::anthropic::{Anthropic, AnthropicConfig, CLAUDE_OPUS_5_5, CLAUDE_SONNET_5_5};

// (a) 純 env：讀 ANTHROPIC_API_KEY，以及（可選）ANTHROPIC_BASE_URL
let client = Anthropic::from_env()?;                      // client/anthropic.rs:31

// (b) 明確指定 key + base URL（測試 mock、或之後走 gateway）
let client = AnthropicConfig::new("sk-ant-...")           // providers/anthropic/wire.rs:285
    .with_base_url("http://127.0.0.1:1234")               // wire.rs:340；會自動去掉結尾 /v1、/v1/messages
    .client();                                            // 共用 reqwest client

// (c) env key + 程式指定 base URL / beta
let client = AnthropicConfig::from_env()?.with_base_url(url).with_beta("...").client();

let model = client.completion(CLAUDE_OPUS_5_5);           // Model<Messages>
```

- 常數（`providers/anthropic/completion.rs:34-60`，皆 re-export 於 `rig::providers::anthropic`）：`CLAUDE_OPUS_5_5 = "claude-opus-5-5"`、`CLAUDE_SONNET_5_5 = "claude-sonnet-5-5"`（另有 `CLAUDE_OPUS_5`、`CLAUDE_SONNET_5`、`CLAUDE_FABLE_5(_1)` 等）。`client.completion(impl Into<String>)`，所以 `AI_MODEL` 環境變數的字串可直接傳入，不必對應常數。
- 沒有 `ANTHROPIC_API_KEY` 時 `from_env()` 回 `EnvError`（可在啟動時 fail-fast 或視為「AI 未啟用」）。
- 請求打到 `{base_url}/v1/messages`，header：`x-api-key`、`anthropic-version: 2023-06-01`（可用 `with_version` 改）。

## Q2 實際送出的 request body

Turn 1（base64 已遮蔽；`strict_tools` 開啟、`Reasoning::Effort(Effort::Low)`、`.max_tokens(4096)`）。完整檔案：`spikes/rig-spike/target/recorded/happy-1.json`。

```json
{
  "model": "claude-opus-5-5",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "這是一張便當店傳單，請匯入。"
        },
        {
          "type": "image",
          "source": {
            "type": "base64",
            "media_type": "image/jpeg",
            "data": "<base64 redacted>"
          }
        }
      ]
    }
  ],
  "max_tokens": 4096,
  "system": [
    {
      "type": "text",
      "text": "你是「訂便當系統」的資料輸入助理。先用 list_vendors 查現有店家，再用 propose_vendor 提出內容，最後用繁體中文摘要。"
    }
  ],
  "tools": [
    {
      "name": "list_vendors",
      "description": "列出目前登入者的所有店家。",
      "input_schema": {
        "type": "object",
        "properties": {},
        "additionalProperties": false
      },
      "strict": true,
      "eager_input_streaming": true
    },
    {
      "name": "propose_vendor",
      "description": "提出要新增或覆蓋的店家資訊與菜單品項。只會寫入待確認變更，使用者確認後才會生效。",
      "input_schema": {
        "type": "object",
        "properties": {
          "vendorId": {
            "type": [
              "integer",
              "null"
            ],
            "description": "Existing vendor id, or null to create a new vendor.\n\n{format: int64}"
          },
          "info": {
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "phone": {
                "type": [
                  "string",
                  "null"
                ]
              },
              "address": {
                "type": [
                  "string",
                  "null"
                ]
              },
              "note": {
                "type": [
                  "string",
                  "null"
                ]
              }
            },
            "additionalProperties": false,
            "required": [
              "name",
              "phone",
              "address",
              "note"
            ]
          },
          "menuItems": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "name": {
                  "type": "string"
                },
                "price": {
                  "type": [
                    "integer",
                    "null"
                  ],
                  "description": "NT dollars, integer. null when unreadable.\n\n{format: int64}"
                },
                "description": {
                  "type": [
                    "string",
                    "null"
                  ]
                },
                "category": {
                  "type": [
                    "string",
                    "null"
                  ]
                },
                "confidence": {
                  "type": "string",
                  "enum": [
                    "high",
                    "medium",
                    "low"
                  ]
                }
              },
              "additionalProperties": false,
              "required": [
                "name",
                "price",
                "description",
                "category",
                "confidence"
              ]
            }
          },
          "warnings": {
            "type": "array",
            "items": {
              "type": "string"
            }
          }
        },
        "additionalProperties": false,
        "required": [
          "vendorId",
          "info",
          "menuItems",
          "warnings"
        ]
      },
      "strict": true,
      "eager_input_streaming": true
    }
  ],
  "thinking": {
    "type": "adaptive",
    "block_binding": {
      "prefix_mismatch_behavior": "drop_block"
    }
  },
  "output_config": {
    "effort": "low"
  }
}
```

Headers（`happy-1.headers.json`）：`anthropic-version: 2023-06-01`、`anthropic-beta: thinking-binding-controls-2026-08-01`（**rig 自動加**，見下）。

要點：
- **圖片**：`{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"…"}}`，與文字同在一個 user message 的 content 陣列（文字在前）。`UserContent::image_raw(bytes, Some(ImageMediaType::JPEG), None)` 由 rig 做 base64。（`providers/anthropic/completion.rs` `image_source`）
- **system**：`preamble` 變成 `system: [{type:"text",text:…}]`（陣列形式）。
- **strict**：預設**不送** `strict`。要開啟：`let mut m = client.completion(..); m.wire = m.wire.with_strict_tools();`（`wire.rs` `Messages::with_strict_tools`）。開啟後每個 tool 帶 `"strict": true`，且 **rig 會自行 sanitize schema**：補 `additionalProperties:false`、補 `required`（全部屬性）、補空 `properties`、把不支援的關鍵字（如 `format: int64`）搬進 `description`（可見上面 JSON 的 `"{format: int64}"`）。因此我們自己的 `strict_schema()` 對 strict 模式是冗餘但無害；**不開 strict 時 rig 原樣送出我們的 schema**（見 `happy-nostrict-1.json`），所以仍建議保留 `strict_schema()` 讓兩種模式一致。Anthropic 端對 strict 支援的 JSON Schema 子集是否接受 `type:["integer","null"]` 需實測（rig 沒改這個寫法）。
- **`eager_input_streaming: true`** 每個 tool 都會帶（Anthropic dialect 預設，`wire.rs` `ToolInputStreaming::Eager`），unary 也一樣。若將來經 gateway 被拒，用 `with_tool_input_streaming(ToolInputStreaming::Off)`。
- **thinking / effort**：`.reasoning(Reasoning::Effort(Effort::Low))`（`AgentBuilder::reasoning`，`rig-agent/src/agent/builder.rs:286`；`Effort` 在 `rig::completion`）→ body 出現 `"output_config":{"effort":"low"}`，**同時 rig 自動加** `"thinking":{"type":"adaptive","block_binding":{"prefix_mismatch_behavior":"drop_block"}}` 與 beta header `thinking-binding-controls-2026-08-01`（`providers/anthropic/completion.rs` `drops_unbound_thinking`、`THINKING_BINDING_BETA`）。也就是說「effort low」在 opus-5-5 上不是「不思考」，而是 adaptive thinking + 低 effort。不設 reasoning 則兩者都不送（`happy` 以外未驗證，請 T4.2 用 recorded body 確認）。
- **max_tokens**：不設時由 `Dialect::default_max_tokens` 依 model 查表（Anthropic dialect：`MaxTokens::ByModel`，`wire.rs:73-80`；`claude-opus-4*` 等無 catalog 時 64000）。本 spike 明確設 `AgentBuilder::max_tokens(4096)`；import agent 建議明設（摘要 + 數十個品項的 tool call，8192 較穩，且避免 `finish_reason=Length`）。
- **tool_choice**：沒設就**完全不送**（= API 的 auto）。符合 design「這兩個模型不支援強制 tool」；rig 另有 `rejects_forced_tool_choice` 對 catalog 內的模型擋掉。`AgentBuilder::tool_choice` 不要用。
- **messages 帶 tool 往返**：turn 2 起，assistant 的 `tool_use` 與 user 的 `tool_result` 會依序追加（`happy-3.json`）。

## Q3 tool 結果與 tool 錯誤如何回給 model

- 成功：`{"type":"tool_result","tool_use_id":…,"content":[{"type":"text","text":"<Output 序列化成 JSON 字串>"}]}`（無 `is_error`）。Output 只要 `Serialize` 即可（`IntoToolOutput` 對所有可序列化型別自動實作，`tool/contextual.rs`）。
- 失敗：`"is_error": true`，content 是 text。範例（`invalid-args-2.json`）：
  `failed to parse tool arguments: invalid type: string "abc", expected i64 at line 1 column 161`。
  - **參數反序列化失敗**（`parse_tool_args`，`tool/contextual.rs`）→ `ToolExecutionError::invalid_args`，原文 serde 錯誤回給 model，**不會中斷 loop**，model 有機會修正（mock 第 2 輪就送了正確參數，成功；用掉 1 個 turn 的 `max_turns` 額度）。注意這種錯誤只有 serde 的訊息、**沒有 JSON path**（只有 line/column），對 model 修正很不友善。若啟用 strict，API 端會先保證型別正確，這類錯誤應該很少。
  - **tool `call()` 回傳 `Err`**：**預設會被遮蔽**。`Tool::map_error` 預設走 `ToolExecutionError::from_error`，model 只看到 kind 級別的固定字串（例如 `"the tool failed"`），原訊息只留給 operator（`tool/result.rs:187-211`、`redact_model_feedback`）。**design 9.1(5)「tool 回傳具體錯誤讓 agent 修正」必須自己 override `map_error`**（或讓 `Error = ToolExecutionError`），例如：
    ```rust
    fn map_error(&self, e: ToolError) -> ToolExecutionError {
        match e { ToolError::Validation(m) => ToolExecutionError::invalid_args(m) } // 訊息對 model 可見
    }
    ```
    實測（`business-invalid-2.json`）：`menuItems[0].price 必須介於 1–100000，收到 -5` 以 `is_error:true` 原文回給 model，下一輪修正成功。
  - 明確建構的 `ToolExecutionError::{invalid_args,not_found,…}(msg)` 其 msg 對 model 可見；用 `.with_model_feedback(text)` 可另外指定給 model 的文字。
- **更好的作法（建議）**：design 的 propose_* 已經設計成「驗證錯誤放在回傳值 `errors: [...]`」（9.3 的 `ProposalDiffSummary.errors`）。這樣不走 error 路徑、不受 redaction 影響、也沒有 `is_error`，且可一次回報多筆錯誤。建議：**業務驗證錯誤用 `Ok(Summary{errors})`；只有真正的內部錯誤才 `Err`（被遮蔽是好事，避免洩漏內部訊息）**。
- 其他：model 呼叫不存在的 tool → `PromptError::UnknownToolCall`（loop 中止，不是回給 model）；`max_invalid_tool_call_retries`、`max_consecutive_malformed_tool_calls`、`AgentHook::on_invalid_tool_call` 可調整無效呼叫的處理（`rig-agent/src/agent/runner.rs:535-545`）。

## Q4 Refusal 如何呈現

- Anthropic unary/stream decoder 把 `stop_reason:"refusal"` 對應成 `FinishReason::ContentFilter`，並把 `stop_details.explanation` 放進 `CompletionResponse.error`（`providers/anthropic/streaming.rs:132-153` `finish_of`）。
- 但 agent loop 對「沒有 answer 且 `truncated_output()`」的 turn 直接失敗，並**只**用固定文字報錯（`rig-core/src/completion/message.rs:127-158` `turn_failure` → `request.rs:160` `no_answer_message`）。實測最終 error：

  ```
  Err(PromptError::Provider(ProviderError::Response(
    "the model produced no answer and stopped with finish_reason=ContentFilter; the provider filtered the response — the content, not the budget, is what it objected to")))
  ```
  **explanation 與 category 在最終 error 中遺失；沒有專屬的 error variant。** `PromptError` 是 `#[non_exhaustive]`（`rig-agent/src/run/response.rs:296`）。
- 可靠的偵測方式（實測可行）：加一個 `AgentHook`，在 `on_model_turn_finished` 讀 `event.finish_reason` 與 `event.raw`（provider 原始 response JSON，含 `stop_reason` 與 `stop_details`）：

  ```rust
  #[derive(Clone, Default)]
  struct TurnSpy(Arc<Mutex<Vec<(Option<FinishReason>, Value /*raw*/, Usage)>>>);
  impl AgentHook for TurnSpy {
      fn on_model_turn_finished(&self, _c: &HookContext, e: ModelTurnFinished<'_>)
          -> impl Future<Output = ModelTurnAction> + Send {
          self.0.lock().unwrap().push((e.finish_reason.cloned(), e.raw.clone(), e.usage));
          async { ModelTurnAction::Continue }
      }
  }
  // builder: .add_hook(spy.clone())     // rig::agent::{AgentHook, HookContext, ModelTurnAction, ModelTurnFinished}
  ```
  實測 hook 看到：`finish_reason=Some(ContentFilter)`、`raw.stop_reason == "refusal"`、`raw.stop_details == {"type":"refusal","category":"cyber","explanation":"…"}`。

  建議映射：`Err(e)` 且 spy 內最後一筆 `raw["stop_reason"] == "refusal"`（或 `finish_reason == ContentFilter`）→ `AiError::Refused { category, explanation }`；其他 `Err` 再依 Q6 分類。**不要**用字串比對最終 error 當唯一依據（僅作為保底：`PromptError::Provider(ProviderError::Response(s))` 且 `s.contains("finish_reason=ContentFilter")`）。
- Refusal 發生在中間 turn（已有 tool call 之後）時，同樣失敗；已收集的 proposals 在我們自己的 context 裡（見 Q7），要決定是否丟棄（建議丟棄，整個 run 標 failed/refused）。
- 另：被 refusal 的 turn 其 usage 在 `Err` 時拿不到（見 Q5），要靠 hook。

## Q5 取得 token 用量

- 成功時：`PromptResponse.usage: Usage`（`rig-agent/src/run/response.rs:83-85`），是整個 run 的**加總**：`usage.input_tokens: Option<u64>`、`output_tokens`、`total_tokens`、`cached_input_tokens`、`cache_creation_input_tokens`、`reasoning_tokens`，另有 `cost: Option<Cost>`（依內建價格表估算的 USD，mock 回 0.015；**價格表是否含 opus-5-5 的正確價格要用真實數字核對，不要直接當帳單**）。實測 3 輪 mock 每輪 1000/50 → `3000/150`。
- `PromptResponse.completion_calls: Vec<CompletionCall>` 有逐輪 usage（實測 3 筆），`messages` 有完整 transcript（可存成 tool 呼叫紀錄）。
- 失敗時：`Err(PromptError)` 不帶 usage。若要記錄失敗 run 的用量，用上面的 `TurnSpy` 累加 `ModelTurnFinished.usage`。
- 注意 Anthropic 的 `input_tokens` 不含 cache read/creation；本專案目前未開 prompt caching（`Messages::with_prompt_caching` 可開，system prompt + tools 固定，每次 run 的 3 輪請求前綴相同，可考慮 T4.4 之後評估）。

## Q6 HTTP 錯誤型別、重試、逾時

- **錯誤型別**：
  - HTTP 非 2xx：`PromptError::Report(ErrorReport)`（不是 `Provider(..)` variant！）。529 實測：`kind: ProviderResponse, retryable: true, http_status: Some(529), code: Some("overloaded_error"), provider_response: Some{status, body, headers, …}`；500 實測：`http_status: Some(500), code: Some("api_error")`。`ErrorReport`（`rig-core/src/error.rs:114-140`）有 `retryable`、`http_status`、`code`、`refusal`、`request_id`，**是 T4.2 做 `AiError::Unavailable`/重試判斷的主要依據**。401/403 → `ProviderError::InvalidAuthentication`（經 `Report` 轉換後 `kind` 不同，請在 T4.2 加一個 mock 401 測試確認）。
  - 傳輸錯誤（連線失敗、reqwest timeout）：實測同樣是 `PromptError::Report`，`kind: Http, retryable: true, http_status: None`。
  - `PromptError` 其他 variant：`Provider(ProviderError)`（如 refusal 的 `Response(String)`）、`Memory`、`MaxTurns{..}`、`Cancelled`、`UnknownToolCall`。`#[non_exhaustive]`，match 要有 `_` 分支。
- **內建重試**：**沒有**。529/500 各只打 1 次就回錯（實測，`HTTP requests to mock = 1`，耗時 ~2ms）。`ErrorReport.retryable` 只是提示。要重試需自己包（建議：只對 `retryable && http_status ∈ {429,500,502,503,529}` 且 run 尚未產生 proposals 時整個 run 重試 1 次，帶 jitter；run 的 tools 有副作用但只寫「待確認變更」，整個重跑要先清掉 context 內的 proposals），或用 rig 的 `reqwest-middleware` feature + `reqwest-retry`（`ReqwestMiddlewareClient`，`rig-reqwest/src/lib.rs:150`；本 spike 未實測）。
- **逾時**：預設 reqwest client 沒有 timeout（`rig-reqwest/src/lib.rs:64` `reqwest::Client::builder().build()`）。兩種做法皆實測可行：
  1. 整個 run 外包 `tokio::time::timeout(Duration::from_secs(90), agent.prompt(..).attach_files(..)?)` → `Elapsed`（design 9.6 的「整個 agent 執行逾時 90 秒」就是這個；future 被 drop 即取消）。
  2. 單次請求 timeout：`AnthropicConfig::…connect(rig::http_client::ReqwestClient::from(reqwest::Client::builder().timeout(60s).connect_timeout(5s).build()?))`（需自己加 `reqwest = "0.13"` 依賴，版本須與 rig 一致）。
  建議兩者並用：整個 run 90 秒（外層 `timeout`）+ 單次請求 ~60 秒（避免單一請求吃光預算卻不報錯）。
- `default_max_turns(6)` 是「**總 model 呼叫數**預算」（含初始呼叫與重試；`runner.rs:165` 文件），超過回 `PromptError::MaxTurns`。design 的流程 list→get→propose→（修正）propose→摘要 = 最少 4～5 次呼叫，加一次修正 6 次剛好用完。**建議 `default_max_turns(8)`**（tool 參數 parse 失敗的重試也計入）。

## Q7 每個 request 建 agent 是否安全 / tool 是否可持有 Arc

- 安全且便宜：`AgentBuilder::...build()` 約 0.4 ms（9 次 run 實測；第一次 12–49 ms 是 process-wide 共用 reqwest client 的 lazy 初始化，只發生一次）。HTTP 連線池由 `rig_reqwest::shared()` 全 process 共用，不會因為每次 build agent 而重建連線。
- Tool 可持有任意 `Send + Sync` 狀態：`ProposeVendor { ctx: Arc<ImportToolContext> }`（`ImportToolContext` 內含 `organizer_id`、`run_id`、`Mutex<Vec<Proposal>>`）。`Tool::call(&self, &mut ToolContext, Args)` 用 `&self`，所以內部可變性要自己用 `Mutex`/repo port。實測 `organizer_id=42 run_id=run-1` 皆從 struct 讀到，不在 JSON schema 中。
- 替代方案：`AgentRunner::tool_context(ToolContext)`（`runner.rs:196`）可為單一 run 注入 typed 值，`call` 內從 `&mut ToolContext` 取；就「每個 request 建一個 agent」的模型而言，直接把 `Arc` 放進 tool struct 更簡單，**建議採用 struct 注入**。
- `Tool` 是 `Sync`，同一個 turn 若 model 平行呼叫多個 tool，可能並行執行（`tool_concurrency`，`runner.rs:506`）；`propose_*` 對同一目標「最新一次取代先前」的邏輯因此要在 `Mutex` 內做。想避免交錯可 `tool_concurrency(1)`（未實測；預設值見 `runner.rs:506` 文件）。
- agent 與 run 之間沒有跨 request 的共享可變狀態（無 memory 設定時），所以 per-request build 沒有串話風險。

## Q8 Server-side fallbacks

**可以，rig 原生支援，不需要自己塞 extra params。** `providers/anthropic/extension.rs`：`SERVER_SIDE_FALLBACK_BETA = "server-side-fallback-2026-07-01"`（:38）、`Fallbacks::Default` 序列化為 `"default"`（:339-347）、`AnthropicOptions::fallbacks()`。需要兩步（beta header 在 provider config，body 欄位在 request option；rig 不會替你加 header）：

```rust
use rig::providers::anthropic::extension::{AnthropicOptions, Fallbacks, SERVER_SIDE_FALLBACK_BETA};

let client = AnthropicConfig::from_env()?
    .with_beta(SERVER_SIDE_FALLBACK_BETA)          // → anthropic-beta header
    .client();
let agent = AgentBuilder::new(client.completion(CLAUDE_OPUS_5_5))
    .provider_option(AnthropicOptions::default().fallbacks(Fallbacks::Default)) // → body "fallbacks":"default"
    …
```

實測（`fallbacks-1.json` / `fallbacks-1.headers.json`）：body 多了 `"fallbacks": "default"`，header `anthropic-beta: server-side-fallback-2026-07-01,thinking-binding-controls-2026-08-01`（多個 beta 以逗號合併）。`Fallbacks::Models(vec!["claude-sonnet-5-5".into()])` 則需 `SERVER_SIDE_FALLBACK_MODELS_BETA`（`2026-06-01`）。`AgentBuilder::additional_params(json!({...}))` 也可手動合併任意欄位作為保底。

只缺一件事：**fallback 後的 response 形狀（哪個 model 回答、usage 怎麼算）沒有 mock 也沒有文件可驗證**，T4.4 有 key 時需確認；若 fallback 後仍 refusal，行為同 Q4。

## Q9 對 design §9 的修正建議與 T4.1／T4.2 建議

**design 需要修改的地方**
1. §9.3 程式示意：`rig::tool::Tool` 的簽章與實際一致（`NAME/Args/Output/Error/description()/parameters()/call(&self,&mut ToolContext,Args)`）；但 `.attach_files([...])?.await?` 的 `?` 位置要注意：`attach_files` 回 `Result<AgentRunner, MessageError>`，之後 `.await` 回 `Result<PromptResponse, PromptError>`。另外 `Agent::prompt` 需 `use rig::prelude::*` 之外還要 `rig::AgentBuilder`。實際 import 見 `spikes/rig-spike/src/main.rs`。
2. §9.3「`parameters()` 用 `strict_schema` 加 `additionalProperties:false`」：改為 **`model.wire = model.wire.with_strict_tools()` 讓 rig 送 `strict:true` 並自動 sanitize**，我們的 `strict_schema()`（inline subschemas + 全 required + additionalProperties:false）當保底；schemars 版本用 1.x，設 `inline_subschemas = true` 避免 `$defs/$ref`。
3. §9.1(5) 與 §9.3：tool 的驗證錯誤**不要靠 `Err`**（預設被 rig 遮蔽成 "the tool failed"）。改成 `Ok(ProposalDiffSummary{ errors })`；若真的要走 `Err`，必須 override `map_error`（Q3）。
4. §9.3「推論強度用 `low`」：實際 body 是 `thinking:{type:adaptive,…}` + `output_config.effort:"low"` + 自動 beta header `thinking-binding-controls-2026-08-01`；不是關閉思考。確認真實 API 接受此組合（mock 無法驗證）。
5. §9.6 refusal：具體化為「`AgentHook` 讀 `ModelTurnFinished.raw`」(Q4)；最終 `PromptError` 沒有 refusal 專屬 variant、也不帶 explanation。
6. §9.6 逾時／錯誤：補上「rig 無內建重試、無預設逾時」，並明訂重試政策（Q6）。`default_max_turns` 的語意是總呼叫數，建議 8。
7. §9.6 token 用量：成功才有 `PromptResponse.usage`；失敗 run 的用量需靠 hook 記錄，`ai_import_runs.usage` 欄位要允許失敗 run 寫入。
8. 新增：要不要用 §9.6 的 server-side fallback——可行（Q8），建議預設 **開啟 `Fallbacks::Default`**（refusal 在 cyber 類誤判時有機會自動換 model），以 `AI_SERVER_FALLBACK=true` 控制。

**T4.1（domain + application）**
- `ImportAgent` port 的錯誤型別：`AiError::{Refused{category,explanation}, Unavailable{retryable}, Timeout, Auth, Internal}`；port 輸出要含 `usage` 與 `tool_calls`（從 `PromptResponse.messages` 取）。`FakeImportAgent` 不需要變。
- `application::ai_import::tools` 的 propose 函式回 `ProposalDiffSummary{created,updated,unchanged,errors}`；驗證錯誤文字包含 JSON path（`menuItems[3].price …`），因為 rig 自己的 parse 錯誤沒有 path。
- 「同一目標重複 propose 取代先前」要在 context 的 `Mutex<HashMap<TargetKey, Proposal>>` 內做；只有 `errors` 為空的 propose 才寫入／取代（避免壞提案蓋掉好提案）。
- Proposal 的 args 型別放在 application 層但 `JsonSchema` derive 需要 `schemars`；若不想讓 application 依賴 schemars，則 args 型別放 infrastructure、application 只收 domain 型別（`ProposeVendorArgs → domain` 轉換在 infra tool wrapper 內）。建議後者（符合 design 的「Tool 是薄包裝」）。

**T4.2（infrastructure + API）**
- 依賴：`rig = { git = …, rev = "36f287be…" }`（預設 features），`schemars = "1"`；rig 會帶入 reqwest 0.13 + rustls（aws-lc-sys，需 cmake/C 編譯器，**Dockerfile 的 build stage 要確認有 cmake**；本機建置已通過）。已記錄在 `spikes/rig-spike/Cargo.lock`。
- `RigImportAgent::run`：每次請求 `AnthropicConfig::from_env()`（啟動時建一次 `Anthropic` client 存進 state，clone 便宜）→ `completion(cfg.model)` → `with_strict_tools()` → `AgentBuilder::new(..).preamble().reasoning(Effort::Low).max_tokens(8192).default_max_turns(8).add_hook(TurnSpy).tool(..)×6.build()`；外層 `tokio::time::timeout(90s, …)`。
- 錯誤映射表：`Elapsed`→`Timeout`；spy 看到 `raw.stop_reason=="refusal"`→`Refused`；`PromptError::Report(r)` 且 `r.http_status ∈ {401,403}`→`Auth`（ops 告警、對外仍 502）；`r.retryable`→`Unavailable{retryable:true}`（重試 1 次）；`MaxTurns`/`UnknownToolCall`/其餘→`Internal`。全部對外 `502 ai_unavailable`，refusal 另帶 code 讓前端提示改手動輸入。
- 測試：repo 內可放一個以 `axum` mock 為基礎的 `RigImportAgent` 整合測試（本 spike 的 `mock.rs` 可直接搬，`ANTHROPIC_BASE_URL`/`with_base_url` 指向它），CI 不需要 key，涵蓋 happy／refusal／invalid-args／529／timeout。
- 有 key 之後必驗的 4 件事（T4.4）：(1) `thinking:adaptive`+`effort:low`+beta header 被接受；(2) `strict:true` 與 `type:["integer","null"]` 被接受；(3) `fallbacks:"default"` + beta 被接受；(4) refusal 的實際 response 形狀與 `stop_details` 欄位名稱。

## 如何重跑

```bash
cd spikes/rig-spike
cargo run                       # 跑 9 個情境，輸出結果，並寫 target/recorded/*.json
cargo clippy -- -D warnings     # 乾淨
```
