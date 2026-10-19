//! Scripted agent for local development without an API key. It runs a fixed
//! script through the *real* tool functions, so the proposal, diffs and issues
//! are exactly what the real agent would produce.

use std::sync::Arc;
use std::time::Duration;

use application::ai_import::tools::{
    self, ConfidenceArg, ExistingRosterRef, ExistingVendorRef, GetVendorArgs, GetVendorOutput,
    MemberArg, MenuItemArg, NewLiteral, NewRosterRef, ProposeRosterMembersArgs, ProposeVendorArgs,
    RosterTargetArg, VendorInfoArg, VendorTargetArg,
};
use application::ai_import::{AgentInput, AgentOutcome, AiError, ImportAgent, ImportToolContext};
use async_trait::async_trait;
use domain::ai_import::{ImportMode, Usage};

pub struct DemoImportAgent {
    /// Pause that makes the UI's loading state visible.
    pub delay: Duration,
}

impl Default for DemoImportAgent {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(800),
        }
    }
}

fn fail(e: tools::ToolError) -> AiError {
    AiError::Unavailable {
        retryable: false,
        detail: e.to_string(),
    }
}

fn member(name: &str, note: Option<&str>, confidence: ConfidenceArg) -> MemberArg {
    MemberArg {
        name: name.into(),
        note: note.map(Into::into),
        confidence,
    }
}

fn item(name: &str, price: Option<i64>, category: &str, confidence: ConfidenceArg) -> MenuItemArg {
    MenuItemArg {
        name: name.into(),
        price,
        description: None,
        category: Some(category.into()),
        confidence,
    }
}

#[async_trait]
impl ImportAgent for DemoImportAgent {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        tokio::time::sleep(self.delay).await;
        let roster = input.mode == ImportMode::Roster;
        let summary = if input.mode == ImportMode::DeliveryAddress {
            let args = tools::ProposeDeliveryAddressArgs {
                name: "示範教室".into(),
                address: "台北市中正區示範路 1 號 3 樓".into(),
                confidence: ConfidenceArg::Medium,
                warnings: vec!["這是示範地址，請勿直接作為實際送餐地址。".into()],
            };
            ctx.record_tool_call(
                "propose_delivery_address",
                serde_json::to_value(&args).unwrap_or_default(),
            );
            tools::propose_delivery_address(&ctx, args)
                .await
                .map_err(fail)?;
            "（示範資料）已提出 1 個送餐地址，請確認後再儲存。"
        } else if roster {
            let target = match input.target_roster_id {
                Some(id) => RosterTargetArg::Existing(ExistingRosterRef {
                    roster_id: id.to_string(),
                }),
                None => RosterTargetArg::New(NewRosterRef {
                    new_roster_name: "示範名單".into(),
                }),
            };
            let args = ProposeRosterMembersArgs {
                target,
                members: vec![
                    member("王小明", None, ConfidenceArg::High),
                    member("林美玲", Some("業務部"), ConfidenceArg::High),
                    member("陳大文", None, ConfidenceArg::Medium),
                    member("Amy 張", None, ConfidenceArg::Low),
                ],
                warnings: vec!["第 4 列字跡模糊，判讀為『Amy 張』".into()],
            };
            ctx.record_tool_call("list_rosters", serde_json::json!({}));
            tools::list_rosters(&ctx).await.map_err(fail)?;
            ctx.record_tool_call(
                "propose_roster_members",
                serde_json::to_value(&args).unwrap_or_default(),
            );
            tools::propose_roster_members(&ctx, args)
                .await
                .map_err(fail)?;
            "（示範資料）已提出 4 位成員，其中『Amy 張』字跡模糊，請確認。"
        } else {
            let mut items = vec![
                item("排骨飯", Some(85), "飯類", ConfidenceArg::High),
                item("雞腿飯", Some(95), "飯類", ConfidenceArg::High),
                item("滷肉飯", Some(60), "飯類", ConfidenceArg::Medium),
                item("今日特餐", None, "飯類", ConfidenceArg::Low),
            ];
            let mut name = "示範便當".to_string();
            let mut target = VendorTargetArg::New(NewLiteral::New);
            if let Some(id) = input.target_vendor_id {
                ctx.record_tool_call(
                    "get_vendor",
                    serde_json::json!({ "vendorId": id.to_string() }),
                );
                let detail = tools::get_vendor(
                    &ctx,
                    GetVendorArgs {
                        vendor_id: id.to_string(),
                    },
                )
                .await
                .map_err(fail)?;
                if let GetVendorOutput::Found(v) = detail {
                    name = v.name;
                    target = VendorTargetArg::Existing(ExistingVendorRef {
                        vendor_id: id.to_string(),
                    });
                    // One price change against the first existing item.
                    if let Some(first) = v.menu.first() {
                        items[0] = item(
                            &first.name,
                            Some(first.price + 5),
                            "飯類",
                            ConfidenceArg::High,
                        );
                    }
                }
            } else {
                ctx.record_tool_call("list_vendors", serde_json::json!({}));
                tools::list_vendors(&ctx).await.map_err(fail)?;
            }
            let args = ProposeVendorArgs {
                target,
                info: VendorInfoArg {
                    name,
                    phone: Some("02-2345-6789".into()),
                    address: None,
                    note: Some("滿 10 個外送".into()),
                },
                menu_items: items,
                warnings: vec!["『今日特餐』沒有標價，請自行補上".into()],
            };
            ctx.record_tool_call(
                "propose_vendor",
                serde_json::to_value(&args).unwrap_or_default(),
            );
            tools::propose_vendor(&ctx, args).await.map_err(fail)?;
            "（示範資料）已提出店家與 4 個品項，『今日特餐』沒有價格，請補上後再套用。"
        };
        let usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
        };
        Ok(AgentOutcome {
            summary: summary.into(),
            usage,
            model: "demo".into(),
        })
    }
}
