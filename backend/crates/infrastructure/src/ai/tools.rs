//! rig `Tool` wrappers over `application::ai_import::tools`. Each is a thin
//! shell: it records the call, delegates, and returns the output as-is.
//! Business problems are already values in the outputs; only infrastructure
//! failures (`ToolError`) become tool errors, which rig redacts for the model.

use std::sync::Arc;

use application::ai_import::ImportToolContext;
use application::ai_import::tools::{
    self, GetRosterArgs, GetRosterOutput, GetVendorArgs, GetVendorOutput, ProposalDiffSummary,
    ProposeRosterMembersArgs, ProposeVendorArgs, RosterListItem, ToolError, VendorListItem,
};
use rig::tool::{Tool, ToolContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// JSON Schema of `T` in the subset Anthropic's strict tool use accepts:
/// inlined subschemas, every object closed (`additionalProperties: false`) with
/// all properties required. (rig's `with_strict_tools` sanitizes too; this keeps
/// non-strict runs identical.)
pub fn strict_schema<T: JsonSchema>() -> Value {
    let generator = schemars::generate::SchemaSettings::draft2020_12()
        .with(|s| s.inline_subschemas = true)
        .into_generator();
    let mut v = serde_json::to_value(generator.into_root_schema_for::<T>())
        .expect("schema serializes to JSON");
    if let Some(o) = v.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
    }
    make_strict(&mut v);
    v
}

fn make_strict(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("object") {
                if let Some(props) = map.get("properties").and_then(Value::as_object) {
                    let keys: Vec<Value> = props.keys().map(|k| json!(k)).collect();
                    map.insert("required".into(), Value::Array(keys));
                }
                map.insert("additionalProperties".into(), json!(false));
            }
            for child in map.values_mut() {
                make_strict(child);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(make_strict),
        _ => {}
    }
}

/// `(name, parameters)` of the six tools, for tests and diagnostics.
pub fn import_tools_parameters() -> Vec<(&'static str, Value)> {
    vec![
        (ListRosters::NAME, strict_schema::<NoArgs>()),
        (GetRoster::NAME, strict_schema::<GetRosterArgs>()),
        (ListVendors::NAME, strict_schema::<NoArgs>()),
        (GetVendor::NAME, strict_schema::<GetVendorArgs>()),
        (
            ProposeRosterMembers::NAME,
            strict_schema::<ProposeRosterMembersArgs>(),
        ),
        (ProposeVendor::NAME, strict_schema::<ProposeVendorArgs>()),
    ]
}

/// Arguments of the parameterless tools.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct NoArgs {}

macro_rules! tool {
    (
        $(#[$m:meta])* $name:ident, $wire:literal, $args:ty, $out:ty, $desc:literal,
        |$ctx:ident, $a:ident| $body:expr
    ) => {
        $(#[$m])*
        #[derive(Clone)]
        pub struct $name {
            pub ctx: Arc<ImportToolContext>,
        }

        impl Tool for $name {
            const NAME: &'static str = $wire;
            type Args = $args;
            type Output = $out;
            type Error = ToolError;

            fn description(&self) -> String {
                $desc.into()
            }

            fn parameters(&self) -> Value {
                strict_schema::<$args>()
            }

            async fn call(
                &self,
                _tool_ctx: &mut ToolContext,
                $a: Self::Args,
            ) -> Result<Self::Output, Self::Error> {
                self.ctx
                    .record_tool_call(Self::NAME, serde_json::to_value(&$a).unwrap_or(Value::Null));
                let $ctx = &*self.ctx;
                $body.await
            }
        }
    };
}

tool!(
    ListRosters,
    "list_rosters",
    NoArgs,
    Vec<RosterListItem>,
    "列出開團者所有的人員名單（id、名稱、成員數）。",
    |ctx, _a| tools::list_rosters(ctx)
);
tool!(
    GetRoster,
    "get_roster",
    GetRosterArgs,
    GetRosterOutput,
    "取得一份名單的現有成員（名字與備註）。",
    |ctx, a| tools::get_roster(ctx, a)
);
tool!(
    ListVendors,
    "list_vendors",
    NoArgs,
    Vec<VendorListItem>,
    "列出開團者所有的便當店（id、名稱、電話、品項數）。",
    |ctx, _a| tools::list_vendors(ctx)
);
tool!(
    GetVendor,
    "get_vendor",
    GetVendorArgs,
    GetVendorOutput,
    "取得一間便當店的現有資訊與菜單。",
    |ctx, a| tools::get_vendor(ctx, a)
);
tool!(
    ProposeRosterMembers,
    "propose_roster_members",
    ProposeRosterMembersArgs,
    ProposalDiffSummary,
    "提出要新增或覆蓋的名單成員。只會記錄待確認的變更，開團者確認後才會生效。回傳 errors 時請修正後再呼叫一次。",
    |ctx, a| tools::propose_roster_members(ctx, a)
);
tool!(
    ProposeVendor,
    "propose_vendor",
    ProposeVendorArgs,
    ProposalDiffSummary,
    "提出要新增或覆蓋的店家資訊與菜單品項。只會記錄待確認的變更，開團者確認後才會生效。回傳 errors 時請修正後再呼叫一次。",
    |ctx, a| tools::propose_vendor(ctx, a)
);

/// Register all six tools on an agent builder.
macro_rules! add_import_tools {
    ($builder:expr, $ctx:expr) => {
        $builder
            .tool($crate::ai::tools::ListRosters { ctx: $ctx.clone() })
            .tool($crate::ai::tools::GetRoster { ctx: $ctx.clone() })
            .tool($crate::ai::tools::ListVendors { ctx: $ctx.clone() })
            .tool($crate::ai::tools::GetVendor { ctx: $ctx.clone() })
            .tool($crate::ai::tools::ProposeRosterMembers { ctx: $ctx.clone() })
            .tool($crate::ai::tools::ProposeVendor { ctx: $ctx.clone() })
    };
}
pub(crate) use add_import_tools;
