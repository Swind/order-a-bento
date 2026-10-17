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
        assert!(p.contains("1–50 characters"));
        assert!(p.contains("At most 500 people"));
        assert!(p.contains("in the range 1–100000"));
        assert!(p.contains("At most 200 items"));
        assert!(p.contains("1–50 characters, using the original name"));
        // tied to the constants, not just literals
        assert!(p.contains(&format!("At most {} people", Roster::MAX_MEMBERS)));
        assert!(p.contains(&format!("{}–{}", Money::MIN_PRICE, Money::MAX_PRICE)));
    }
}
