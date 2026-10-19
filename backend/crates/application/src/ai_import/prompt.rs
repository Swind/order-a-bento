//! System prompts of the import agent (design 9.4): one for roster uploads and
//! one for vendor flyer / menu uploads. The user always chooses the kind up
//! front, so each prompt carries only its own rules and workflow.

use domain::ai_import::ImportMode;
use domain::directory::{Roster, Vendor};
use domain::shared::{ItemName, Money, PersonName};

const ROSTER: &str = include_str!("prompts/roster.md");
const VENDOR: &str = include_str!("prompts/vendor.md");

// Fragments shared by both prompts, spliced in at `{{NAME}}` lines.
const CAUTION: &str = include_str!("prompts/caution.md");
const LANGUAGE: &str = include_str!("prompts/language.md");
const WRITES: &str = include_str!("prompts/writes.md");
const STRUCK_OUT: &str = include_str!("prompts/struck_out.md");
const OUTPUT: &str = include_str!("prompts/output.md");
const IMPORTANT: &str = include_str!("prompts/important.md");

/// The system prompt for `mode`: the mode's template with the shared fragments
/// spliced in, the mode's wording filled into the fragments and the `{{...}}`
/// limits replaced by the domain constants, so the rules the model sees always
/// match the validation.
pub fn import_agent_system_prompt(mode: ImportMode) -> String {
    if mode == ImportMode::DeliveryAddress {
        return include_str!("prompts/delivery_address.md").into();
    }
    let (template, wording): (&str, [(&str, &str); 5]) = match mode {
        ImportMode::DeliveryAddress => unreachable!(),
        ImportMode::Roster => (
            ROSTER,
            [
                ("{{DATA_EXAMPLES}}", "names, notes"),
                ("{{ENTRIES}}", "Members"),
                ("{{CHECK_WHAT}}", "name"),
                (
                    "{{WARNING_EXAMPLES}}",
                    "「第 3 列字跡模糊，判讀為『陳美麗』」「『王小明』被劃掉，未列入」",
                ),
                ("{{TARGET_CHOICE}}", "which roster"),
            ],
        ),
        ImportMode::Vendor => (
            VENDOR,
            [
                (
                    "{{DATA_EXAMPLES}}",
                    "names, prices, categories, descriptions, phone numbers",
                ),
                ("{{ENTRIES}}", "Items"),
                ("{{CHECK_WHAT}}", "item"),
                (
                    "{{WARNING_EXAMPLES}}",
                    "「『雞腿飯』價格被摺痕遮住」「『今日特餐』沒有標價」",
                ),
                ("{{TARGET_CHOICE}}", "which shop"),
            ],
        ),
    };
    let mut p = template.to_string();
    for (key, fragment) in [
        ("{{CAUTION}}", CAUTION),
        ("{{LANGUAGE}}", LANGUAGE),
        ("{{WRITES}}", WRITES),
        ("{{STRUCK_OUT}}", STRUCK_OUT),
        ("{{OUTPUT}}", OUTPUT),
        ("{{IMPORTANT}}", IMPORTANT),
    ] {
        p = p.replace(key, fragment.trim_end());
    }
    for (k, v) in wording {
        p = p.replace(k, v);
    }
    for (k, v) in [
        ("{{MAX_NAME_LEN}}", PersonName::MAX_LEN.to_string()),
        ("{{MAX_MEMBERS}}", Roster::MAX_MEMBERS.to_string()),
        ("{{MAX_ITEM_NAME_LEN}}", ItemName::MAX_LEN.to_string()),
        ("{{MIN_PRICE}}", Money::MIN_PRICE.to_string()),
        ("{{MAX_PRICE}}", Money::MAX_PRICE.to_string()),
        ("{{MAX_MENU_ITEMS}}", Vendor::MAX_MENU_ITEMS.to_string()),
    ] {
        p = p.replace(k, &v);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both() -> [(ImportMode, String); 2] {
        [ImportMode::Roster, ImportMode::Vendor].map(|m| (m, import_agent_system_prompt(m)))
    }

    #[test]
    fn placeholders_are_substituted() {
        for (_, p) in both() {
            assert!(!p.contains("{{"), "unsubstituted placeholder");
            assert!(!p.contains("}}"));
        }
        let r = import_agent_system_prompt(ImportMode::Roster);
        assert!(r.contains("1–50 characters"));
        assert!(r.contains(&format!("At most {} people", Roster::MAX_MEMBERS)));
        let v = import_agent_system_prompt(ImportMode::Vendor);
        assert!(v.contains("in the range 1–100000"));
        assert!(v.contains(&format!("{}–{}", Money::MIN_PRICE, Money::MAX_PRICE)));
        assert!(v.contains("At most 200 items"));
        assert!(v.contains("1–50 characters, using the original name"));
    }

    #[test]
    fn each_prompt_has_only_its_own_rules() {
        let r = import_agent_system_prompt(ImportMode::Roster);
        let v = import_agent_system_prompt(ImportMode::Vendor);
        assert!(r.contains("## Roster rules") && !r.contains("## Vendor rules"));
        assert!(v.contains("## Vendor rules") && !v.contains("## Roster rules"));
        assert!(r.contains("propose_roster_members") && !r.contains("propose_vendor"));
        assert!(v.contains("propose_vendor") && !v.contains("propose_roster_members"));
        assert!(!r.contains("list_vendors") && !v.contains("list_rosters"));
        for p in [&r, &v] {
            assert!(!p.contains("Decide the photo type"));
            assert!(!p.contains("Mode hint"));
            assert!(p.contains("do not call a propose tool"));
        }
    }

    #[test]
    fn shared_sections_are_in_both() {
        for (_, p) in both() {
            for s in [
                "## Language",
                "## Workflow",
                "## How the system writes data",
                "## Struck-out entries",
                "## confidence",
                "## warnings",
                "## Important",
            ] {
                assert!(p.contains(s), "missing {s}");
            }
        }
    }

    /// docs/design.md 9.4 shows both final prompts verbatim.
    #[test]
    fn design_doc_shows_the_final_prompts() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/design.md");
        let Ok(doc) = std::fs::read_to_string(path) else {
            return; // built outside the repository
        };
        for (mode, p) in both() {
            assert!(
                doc.contains(p.trim_end()),
                "docs/design.md 9.4 is out of date for {mode:?}"
            );
        }
    }
}
