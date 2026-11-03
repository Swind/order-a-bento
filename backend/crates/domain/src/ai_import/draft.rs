//! Run-owned editing data. IDs address draft rows, never directory aggregates.
use super::{ChangeAction, Proposal, ProposalTarget, TargetRef, VendorInfoSnapshot};
use crate::directory::VendorId;
use crate::shared::text::{
    ADDRESS_MAX_LEN, CATEGORY_MAX_LEN, DESCRIPTION_MAX_LEN, NOTE_MAX_LEN, PHONE_MAX_LEN,
    optional_text,
};
use crate::shared::{DomainError, ItemName, Money, VendorName};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VendorDraft {
    pub id: String,
    pub vendor: TargetRef<VendorId>,
    pub selected: bool,
    pub info_selected: bool,
    pub info: VendorInfoSnapshot,
    pub menu: Vec<DraftItem>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftItem {
    pub id: String,
    pub selected: bool,
    pub name: String,
    #[serde(with = "crate::shared::amount::optional")]
    pub price: Option<i64>,
    pub category: Option<String>,
    pub description: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftTurn {
    pub id: String,
    pub instruction: String,
    pub reply: String,
    pub status: String,
    pub changes: Vec<String>,
    pub undone: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftState {
    pub revision: i64,
    pub vendors: Vec<VendorDraft>,
    pub turns: Vec<DraftTurn>,
    pub undo: Option<Vec<VendorDraft>>,
    pub pending: Option<String>,
    pub pending_until: Option<u64>,
}
impl DraftState {
    pub fn from_proposal(p: &Proposal) -> Self {
        let vendors = p
            .targets
            .iter()
            .enumerate()
            .filter_map(|(vi, t)| {
                let ProposalTarget::Vendor(v) = t else {
                    return None;
                };
                Some(VendorDraft {
                    id: format!("v{vi}"),
                    vendor: v.vendor.clone(),
                    selected: true,
                    info_selected: matches!(v.vendor, TargetRef::New { .. })
                        || v.info.action != ChangeAction::Unchanged,
                    info: v.info.after.clone(),
                    menu: v
                        .menu
                        .iter()
                        .enumerate()
                        .map(|(mi, m)| DraftItem {
                            id: format!("v{vi}-m{mi}"),
                            selected: m.action != ChangeAction::Unchanged,
                            name: m.name.clone(),
                            price: m.price,
                            category: m.category.clone(),
                            description: m.description.clone(),
                        })
                        .collect(),
                })
            })
            .collect();
        Self {
            revision: 0,
            vendors,
            turns: vec![],
            undo: None,
            pending: None,
            pending_until: None,
        }
    }
}
/// Explicit field edits only; unspecified fields and unrelated rows are preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum DraftEdit {
    #[serde(rename_all = "camelCase")]
    RenameVendor { vendor_id: String, name: String },
    #[serde(rename_all = "camelCase")]
    UpdateItem {
        item_id: String,
        name: Option<String>,
        #[serde(default, with = "crate::shared::amount::optional")]
        price: Option<i64>,
        category: Option<String>,
        description: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    DeleteItem { item_id: String },
    #[serde(rename_all = "camelCase")]
    AddItem {
        vendor_id: String,
        name: String,
        #[serde(default, with = "crate::shared::amount::optional")]
        price: Option<i64>,
        category: Option<String>,
        description: Option<String>,
    },
}
pub fn validate_draft(
    vendors: &[VendorDraft],
    baseline: &[VendorDraft],
) -> Result<(), DomainError> {
    let bad = || DomainError::invalid("draft", "草稿格式或識別碼不正確");
    if vendors.len() != baseline.len()
        || vendors.len() > 20
        || vendors.iter().map(|v| v.menu.len()).sum::<usize>() > 500
    {
        return Err(bad());
    }
    let mut ids = std::collections::HashSet::new();
    let chars: usize = vendors
        .iter()
        .map(|v| {
            v.info.name.len()
                + v.info.phone.as_ref().map_or(0, String::len)
                + v.info.address.as_ref().map_or(0, String::len)
                + v.info.note.as_ref().map_or(0, String::len)
                + v.menu
                    .iter()
                    .map(|m| {
                        m.name.len()
                            + m.category.as_ref().map_or(0, String::len)
                            + m.description.as_ref().map_or(0, String::len)
                    })
                    .sum::<usize>()
        })
        .sum();
    if chars > 128_000 {
        return Err(bad());
    }
    for v in vendors {
        let old = baseline.iter().find(|b| b.id == v.id).ok_or_else(bad)?;
        if v.vendor != old.vendor || !ids.insert(&v.id) || v.menu.len() > 500 {
            return Err(bad());
        }
        // Incomplete OCR fields are allowed while editing, but remain bounded.
        if v.info.name.chars().count() > 100 {
            return Err(bad());
        }
        optional_text("phone", v.info.phone.as_deref(), PHONE_MAX_LEN)?;
        optional_text("address", v.info.address.as_deref(), ADDRESS_MAX_LEN)?;
        optional_text("note", v.info.note.as_deref(), NOTE_MAX_LEN)?;
        for m in &v.menu {
            if m.id.is_empty()
                || m.id.len() > 80
                || !ids.insert(&m.id)
                || m.name.chars().count() > 100
            {
                return Err(bad());
            }
            if let Some(p) = m.price {
                // Preserve an OCR error long enough for the next turn to correct it.
                if old.menu.iter().find(|b| b.id == m.id).and_then(|b| b.price) != Some(p) {
                    Money::try_new(p)?;
                }
            }
            optional_text("category", m.category.as_deref(), CATEGORY_MAX_LEN)?;
            optional_text("description", m.description.as_deref(), DESCRIPTION_MAX_LEN)?;
        }
    }
    Ok(())
}
pub fn apply_edits(
    before: &[VendorDraft],
    edits: Vec<DraftEdit>,
) -> Result<(Vec<VendorDraft>, Vec<String>), DomainError> {
    if edits.len() > 100 {
        return Err(DomainError::invalid("edits", "單輪最多 100 筆修改"));
    }
    let mut next = before.to_vec();
    let mut changes = vec![];
    for e in edits {
        match e {
            DraftEdit::RenameVendor { vendor_id, name } => {
                VendorName::new(&name)?;
                let v = next
                    .iter_mut()
                    .find(|v| v.id == vendor_id)
                    .ok_or_else(|| DomainError::invalid("vendorId", "unknown draft vendor"))?;
                if matches!(v.vendor, TargetRef::Existing { .. }) {
                    return Err(DomainError::invalid("name", "既有店家請至店家頁改名"));
                }
                if v.info.name != name {
                    changes.push(format!("店名：{} → {name}", v.info.name));
                }
                v.info.name = name;
            }
            DraftEdit::UpdateItem {
                item_id,
                name,
                price,
                category,
                description,
            } => {
                let m = next
                    .iter_mut()
                    .flat_map(|v| &mut v.menu)
                    .find(|m| m.id == item_id)
                    .ok_or_else(|| DomainError::invalid("itemId", "unknown draft item"))?;
                let old = m.clone();
                if let Some(n) = name {
                    ItemName::new(&n)?;
                    m.name = n;
                }
                if let Some(p) = price {
                    Money::try_new(p)?;
                    m.price = Some(p);
                }
                if let Some(c) = category {
                    m.category = optional_text("category", Some(&c), CATEGORY_MAX_LEN)?;
                }
                if let Some(d) = description {
                    m.description = optional_text("description", Some(&d), DESCRIPTION_MAX_LEN)?;
                }
                let mut fields = vec![];
                if old.name != m.name {
                    fields.push(format!("品名 {} → {}", old.name, m.name));
                }
                if old.price != m.price {
                    fields.push(format!(
                        "價格 {} → {}",
                        old.price.map_or("未填".into(), crate::shared::amount::text),
                        m.price.map_or("未填".into(), crate::shared::amount::text)
                    ));
                }
                if old.category != m.category {
                    fields.push(format!(
                        "分類 {} → {}",
                        old.category.as_deref().unwrap_or("無"),
                        m.category.as_deref().unwrap_or("無")
                    ));
                }
                if old.description != m.description {
                    fields.push(format!(
                        "說明 {} → {}",
                        old.description.as_deref().unwrap_or("無"),
                        m.description.as_deref().unwrap_or("無")
                    ));
                }
                if !fields.is_empty() {
                    changes.push(format!("{}：{}", old.name, fields.join("；")));
                }
            }
            DraftEdit::DeleteItem { item_id } => {
                let v = next
                    .iter_mut()
                    .find(|v| v.menu.iter().any(|m| m.id == item_id))
                    .ok_or_else(|| DomainError::invalid("itemId", "unknown draft item"))?;
                let i = v.menu.iter().position(|m| m.id == item_id).unwrap();
                changes.push(format!("刪除草稿品項：{}", v.menu.remove(i).name));
            }
            DraftEdit::AddItem {
                vendor_id,
                name,
                price,
                category,
                description,
            } => {
                ItemName::new(&name)?;
                let v = next
                    .iter_mut()
                    .find(|v| v.id == vendor_id)
                    .ok_or_else(|| DomainError::invalid("vendorId", "unknown draft vendor"))?;
                changes.push(format!("新增：{name}"));
                v.menu.push(DraftItem {
                    id: crate::shared::Id::<super::ImportRunMarker>::new().to_string(),
                    selected: true,
                    name,
                    price,
                    category,
                    description,
                });
            }
        }
    }
    validate_draft(&next, before)?;
    Ok((next, changes))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft() -> Vec<VendorDraft> {
        serde_json::from_value(serde_json::json!([{
            "id":"v0","vendor":{"kind":"new","name":"原店"},"selected":true,"infoSelected":true,
            "info":{"name":"原店","phone":null,"address":null,"note":"手動備註"},
            "menu":[{"id":"m1","selected":false,"name":"飯","price":80,"category":"飯類","description":"保留說明"},{"id":"m2","selected":true,"name":"湯","price":20,"category":null,"description":null}]
        }])).unwrap()
    }
    #[test]
    fn selective_edits_preserve_ids_selection_and_unmentioned_fields() {
        let before = draft();
        let (next, changes) = apply_edits(
            &before,
            vec![
                DraftEdit::RenameVendor {
                    vendor_id: "v0".into(),
                    name: "新店".into(),
                },
                DraftEdit::UpdateItem {
                    item_id: "m1".into(),
                    name: Some("雞腿飯".into()),
                    price: Some(110),
                    category: Some(String::new()),
                    description: None,
                },
                DraftEdit::DeleteItem {
                    item_id: "m2".into(),
                },
                DraftEdit::AddItem {
                    vendor_id: "v0".into(),
                    name: "紅茶".into(),
                    price: Some(30),
                    category: None,
                    description: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(changes.len(), 4);
        assert_eq!(next[0].menu.len(), 2);
        assert_eq!(next[0].menu[0].id, "m1");
        assert!(!next[0].menu[0].selected);
        assert_eq!(next[0].menu[0].description.as_deref(), Some("保留說明"));
        assert!(next[0].menu[0].category.is_none());
        assert_eq!(next[0].info.note, before[0].info.note);
        assert_ne!(next[0].menu[1].id, "m2");
    }
    #[test]
    fn unknown_ids_bad_prices_and_existing_vendor_rename_reject_whole_batch() {
        let before = draft();
        assert!(
            apply_edits(
                &before,
                vec![
                    DraftEdit::DeleteItem {
                        item_id: "m1".into()
                    },
                    DraftEdit::DeleteItem {
                        item_id: "wrong".into()
                    }
                ]
            )
            .is_err()
        );
        assert_eq!(before[0].menu.len(), 2);
        assert!(
            apply_edits(
                &before,
                vec![DraftEdit::UpdateItem {
                    item_id: "m1".into(),
                    name: None,
                    price: Some(0),
                    category: None,
                    description: None
                }]
            )
            .is_err()
        );
        let mut existing = before;
        existing[0].vendor = TargetRef::Existing {
            existing_id: VendorId::new(),
        };
        assert!(
            apply_edits(
                &existing,
                vec![DraftEdit::RenameVendor {
                    vendor_id: "v0".into(),
                    name: "新店".into()
                }]
            )
            .is_err()
        );
    }

    #[test]
    fn existing_ocr_price_error_can_be_preserved_then_corrected_by_a_turn() {
        let mut before = draft();
        before[0].menu[0].price = Some(0);
        validate_draft(&before, &before).unwrap();
        let (corrected, _) = apply_edits(
            &before,
            vec![DraftEdit::UpdateItem {
                item_id: "m1".into(),
                name: None,
                price: Some(80),
                category: None,
                description: None,
            }],
        )
        .unwrap();
        assert_eq!(corrected[0].menu[0].price, Some(80));
        let mut invalid_manual = corrected.clone();
        invalid_manual[0].menu[0].price = Some(-1);
        assert!(validate_draft(&invalid_manual, &corrected).is_err());
    }
}
