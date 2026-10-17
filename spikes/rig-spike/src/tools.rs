//! Tools mirroring design 9.3 (propose_vendor + list_vendors).
use std::sync::{Arc, Mutex};

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Injected by the backend at construction time (NOT a tool parameter).
#[derive(Debug, Default)]
pub struct ImportToolContext {
    pub organizer_id: i64,
    pub run_id: String,
    pub proposals: Mutex<Vec<ProposeVendorArgs>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VendorInfo {
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    pub name: String,
    /// NT dollars, integer. null when unreadable.
    pub price: Option<i64>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProposeVendorArgs {
    /// Existing vendor id, or null to create a new vendor.
    pub vendor_id: Option<i64>,
    pub info: VendorInfo,
    pub menu_items: Vec<MenuItem>,
    pub warnings: Vec<String>,
}

/// Make a schemars schema Anthropic-strict compatible:
/// every object: additionalProperties=false, required = all properties.
pub fn strict_schema<T: JsonSchema>() -> Value {
    let generator = schemars::generate::SchemaSettings::draft2020_12()
        .with(|s| s.inline_subschemas = true)
        .into_generator();
    let mut v = serde_json::to_value(generator.into_root_schema_for::<T>()).unwrap();
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
            for (_, child) in map.iter_mut() {
                make_strict(child);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(make_strict),
        _ => {}
    }
}

#[derive(Debug)]
pub enum ToolError {
    Validation(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(m) => write!(f, "{m}"),
        }
    }
}
impl std::error::Error for ToolError {}

#[derive(Clone)]
pub struct ProposeVendor {
    pub ctx: Arc<ImportToolContext>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalDiffSummary {
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub errors: Vec<String>,
}

impl Tool for ProposeVendor {
    const NAME: &'static str = "propose_vendor";
    type Args = ProposeVendorArgs;
    type Output = ProposalDiffSummary;
    type Error = ToolError;

    fn description(&self) -> String {
        "提出要新增或覆蓋的店家資訊與菜單品項。只會寫入待確認變更，使用者確認後才會生效。".into()
    }

    fn parameters(&self) -> Value {
        strict_schema::<ProposeVendorArgs>()
    }

    /// Without this override the model only sees "the tool failed" (from_error redacts).
    fn map_error(&self, error: ToolError) -> ToolExecutionError {
        match error {
            ToolError::Validation(m) => ToolExecutionError::invalid_args(m),
        }
    }

    async fn call(&self, _ctx: &mut ToolContext, args: Self::Args) -> Result<Self::Output, ToolError> {
        let mut errors = vec![];
        for (i, it) in args.menu_items.iter().enumerate() {
            if let Some(p) = it.price {
                if !(1..=100_000).contains(&p) {
                    errors.push(format!("menuItems[{i}].price 必須介於 1–100000，收到 {p}"));
                }
            }
        }
        if !errors.is_empty() {
            return Err(ToolError::Validation(errors.join("; ")));
        }
        println!(
            "    [tool propose_vendor] organizer_id={} run_id={} items={}",
            self.ctx.organizer_id,
            self.ctx.run_id,
            args.menu_items.len()
        );
        let n = args.menu_items.len();
        self.ctx.proposals.lock().unwrap().push(args);
        Ok(ProposalDiffSummary { created: n, updated: 0, unchanged: 0, errors: vec![] })
    }
}

#[derive(Clone)]
pub struct ListVendors {
    pub ctx: Arc<ImportToolContext>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoArgs {}

impl Tool for ListVendors {
    const NAME: &'static str = "list_vendors";
    type Args = NoArgs;
    type Output = Value;
    type Error = ToolError;

    fn description(&self) -> String {
        "列出目前登入者的所有店家。".into()
    }
    fn parameters(&self) -> Value {
        strict_schema::<NoArgs>()
    }
    async fn call(&self, _ctx: &mut ToolContext, _args: NoArgs) -> Result<Value, ToolError> {
        println!("    [tool list_vendors] organizer_id={}", self.ctx.organizer_id);
        Ok(json!([{ "id": 7, "name": "阿財排骨便當", "phone": "02-2345-6789", "itemCount": 3 }]))
    }
}
