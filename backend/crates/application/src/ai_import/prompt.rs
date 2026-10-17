//! System prompt of the import agent (design 9.4).

use domain::directory::{Roster, Vendor};
use domain::shared::{ItemName, Money, PersonName};

const TEMPLATE: &str = include_str!("import_agent.md");

/// The prompt with `{{...}}` placeholders replaced by the domain limits, so the
/// rules the model sees always match the validation.
pub fn import_agent_system_prompt() -> String {
    [
        ("{{MAX_NAME_LEN}}", PersonName::MAX_LEN.to_string()),
        ("{{MAX_MEMBERS}}", Roster::MAX_MEMBERS.to_string()),
        ("{{MAX_ITEM_NAME_LEN}}", ItemName::MAX_LEN.to_string()),
        ("{{MIN_PRICE}}", Money::MIN_PRICE.to_string()),
        ("{{MAX_PRICE}}", Money::MAX_PRICE.to_string()),
        ("{{MAX_MENU_ITEMS}}", Vendor::MAX_MENU_ITEMS.to_string()),
    ]
    .into_iter()
    .fold(TEMPLATE.to_string(), |p, (k, v)| p.replace(k, &v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_substituted() {
        let p = import_agent_system_prompt();
        assert!(!p.contains("{{"), "unsubstituted placeholder");
        assert!(p.contains("1–50 字元"));
        assert!(p.contains("最多 500 人"));
        assert!(p.contains("範圍 1–100000"));
        assert!(p.contains("最多 200 個品項"));
        assert!(p.contains("1–50 字元，使用傳單上的原名"));
        // tied to the constants, not just literals
        assert!(p.contains(&format!("最多 {} 人", Roster::MAX_MEMBERS)));
        assert!(p.contains(&format!("{}–{}", Money::MIN_PRICE, Money::MAX_PRICE)));
    }
}
