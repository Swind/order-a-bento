use std::collections::{HashMap, HashSet};

use super::{UpsertOutcome, UpsertReport};
use crate::identity::OrganizerId;
use crate::shared::{
    DomainError, Id, ItemName, Money, NameKey, Patch, VendorName,
    text::{
        ADDRESS_MAX_LEN, CATEGORY_MAX_LEN, DESCRIPTION_MAX_LEN, NOTE_MAX_LEN, PHONE_MAX_LEN,
        optional_text,
    },
};

pub struct VendorMarker;
pub struct MenuItemMarker;
pub type VendorId = Id<VendorMarker>;
pub type MenuItemId = Id<MenuItemMarker>;

/// Private reference image metadata; bytes are loaded only by the image port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorImage {
    pub revision: String,
    pub size_bytes: u32,
    pub width: u32,
    pub height: u32,
    pub updated_at: std::time::SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: MenuItemId,
    pub name: ItemName,
    pub price: Money,
    pub description: Option<String>,
    pub category: Option<String>,
    pub sort_order: i32,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItemInput {
    pub name: String,
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
}

/// Blank / `None` fields never overwrite existing values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VendorInfoPatch {
    pub name: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
}

/// Manual (tri-state) edit of vendor info: `name` None = keep, others per [`Patch`].
/// Unlike [`VendorInfoPatch`], `Clear` (and a blank `Set`) empties the field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VendorInfoEdit {
    pub name: Option<String>,
    pub phone: Patch<String>,
    pub address: Patch<String>,
    pub note: Patch<String>,
}

/// Manual edit of one menu item (`None` / `Keep` = unchanged).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuItemEdit {
    pub name: Option<String>,
    pub price: Option<i64>,
    pub description: Patch<String>,
    pub category: Patch<String>,
    pub archived: Option<bool>,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vendor {
    pub image: Option<VendorImage>,
    pub sharing_group_id: Option<crate::sharing::SharingGroupId>,
    pub can_manage: bool,
    id: VendorId,
    organizer_id: OrganizerId,
    name: VendorName,
    phone: Option<String>,
    address: Option<String>,
    note: Option<String>,
    menu: Vec<MenuItem>,
}

/// Overwrite `slot` with `new` if `new` is Some and different. Returns changed.
fn overwrite(slot: &mut Option<String>, new: Option<String>) -> bool {
    match new {
        Some(n) if slot.as_deref() != Some(n.as_str()) => {
            *slot = Some(n);
            true
        }
        _ => false,
    }
}

impl Vendor {
    pub const MAX_MENU_ITEMS: usize = 200;

    pub fn new(organizer_id: OrganizerId, name: &str) -> Result<Self, DomainError> {
        Ok(Self {
            id: VendorId::new(),
            image: None,
            sharing_group_id: None,
            can_manage: true,
            organizer_id,
            name: VendorName::new(name)?,
            phone: None,
            address: None,
            note: None,
            menu: Vec::new(),
        })
    }

    /// Reconstitute from storage (checks menu uniqueness and limit).
    pub fn from_parts(
        id: VendorId,
        organizer_id: OrganizerId,
        name: VendorName,
        phone: Option<String>,
        address: Option<String>,
        note: Option<String>,
        menu: Vec<MenuItem>,
    ) -> Result<Self, DomainError> {
        if menu.len() > Self::MAX_MENU_ITEMS {
            return Err(DomainError::LimitExceeded {
                field: "menu",
                max: Self::MAX_MENU_ITEMS,
            });
        }
        let mut seen = HashSet::new();
        for m in &menu {
            if !seen.insert(m.name.key()) {
                return Err(DomainError::Duplicate {
                    field: "name",
                    value: m.name.to_string(),
                });
            }
        }
        Ok(Self {
            id,
            image: None,
            sharing_group_id: None,
            can_manage: true,
            organizer_id,
            name,
            phone,
            address,
            note,
            menu,
        })
    }

    pub fn id(&self) -> VendorId {
        self.id
    }
    pub fn organizer_id(&self) -> OrganizerId {
        self.organizer_id
    }
    pub fn name(&self) -> &VendorName {
        &self.name
    }
    pub fn phone(&self) -> Option<&str> {
        self.phone.as_deref()
    }
    pub fn address(&self) -> Option<&str> {
        self.address.as_deref()
    }
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
    pub fn menu(&self) -> &[MenuItem] {
        &self.menu
    }

    /// Returns whether anything changed. Validates everything before applying.
    pub fn update_info(&mut self, patch: VendorInfoPatch) -> Result<bool, DomainError> {
        let name = match patch.name.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(n) => Some(VendorName::new(n)?),
        };
        let phone = optional_text("phone", patch.phone.as_deref(), PHONE_MAX_LEN)?;
        let address = optional_text("address", patch.address.as_deref(), ADDRESS_MAX_LEN)?;
        let note = optional_text("note", patch.note.as_deref(), NOTE_MAX_LEN)?;
        let mut changed = false;
        if let Some(n) = name
            && n != self.name
        {
            self.name = n;
            changed = true;
        }
        changed |= overwrite(&mut self.phone, phone);
        changed |= overwrite(&mut self.address, address);
        changed |= overwrite(&mut self.note, note);
        Ok(changed)
    }

    /// Manual edit of the vendor info with merge-patch semantics (clearing allowed).
    /// Validates everything before applying; returns whether anything changed.
    pub fn edit_info(&mut self, edit: VendorInfoEdit) -> Result<bool, DomainError> {
        let name = edit.name.as_deref().map(VendorName::new).transpose()?;
        let phone = edit.phone.resolve("phone", PHONE_MAX_LEN)?;
        let address = edit.address.resolve("address", ADDRESS_MAX_LEN)?;
        let note = edit.note.resolve("note", NOTE_MAX_LEN)?;
        let mut changed = false;
        if let Some(n) = name
            && n != self.name
        {
            self.name = n;
            changed = true;
        }
        changed |= phone.apply(&mut self.phone);
        changed |= address.apply(&mut self.address);
        changed |= note.apply(&mut self.note);
        Ok(changed)
    }

    /// Manual edit of one menu item. Renaming onto another item's `NameKey` is a
    /// [`DomainError::Conflict`]. Validates everything before applying.
    pub fn edit_item(&mut self, id: MenuItemId, edit: MenuItemEdit) -> Result<(), DomainError> {
        let idx = self
            .menu
            .iter()
            .position(|m| m.id == id)
            .ok_or(DomainError::NotFound {
                entity: "menu_item",
            })?;
        let name = edit.name.as_deref().map(ItemName::new).transpose()?;
        let price = edit.price.map(Money::try_new).transpose()?;
        let description = edit
            .description
            .resolve("description", DESCRIPTION_MAX_LEN)?;
        let category = edit.category.resolve("category", CATEGORY_MAX_LEN)?;
        if let Some(n) = &name
            && self
                .menu
                .iter()
                .any(|m| m.id != id && m.name.key() == n.key())
        {
            return Err(DomainError::Conflict {
                field: "name",
                value: n.to_string(),
            });
        }
        let m = &mut self.menu[idx];
        if let Some(n) = name {
            m.name = n;
        }
        if let Some(p) = price {
            m.price = p;
        }
        description.apply(&mut m.description);
        category.apply(&mut m.category);
        if let Some(a) = edit.archived {
            m.archived = a;
        }
        if let Some(o) = edit.sort_order {
            m.sort_order = o;
        }
        Ok(())
    }

    /// All-or-nothing upsert by `NameKey`.
    ///
    /// Existing items keep their display name; price always overwrites;
    /// description/category only when non-blank; an archived item is
    /// un-archived (counts as an update).
    pub fn upsert_menu_items(
        &mut self,
        inputs: Vec<MenuItemInput>,
    ) -> Result<UpsertReport<MenuItemId>, DomainError> {
        struct P {
            name: ItemName,
            price: Money,
            description: Option<String>,
            category: Option<String>,
        }
        let mut parsed = Vec::with_capacity(inputs.len());
        let mut seen: HashSet<NameKey> = HashSet::new();
        for i in &inputs {
            let name = ItemName::new(&i.name)?;
            let price = Money::try_new(i.price)?;
            let description =
                optional_text("description", i.description.as_deref(), DESCRIPTION_MAX_LEN)?;
            let category = optional_text("category", i.category.as_deref(), CATEGORY_MAX_LEN)?;
            if !seen.insert(name.key()) {
                return Err(DomainError::Duplicate {
                    field: "name",
                    value: name.to_string(),
                });
            }
            parsed.push(P {
                name,
                price,
                description,
                category,
            });
        }
        let index: HashMap<NameKey, usize> = self
            .menu
            .iter()
            .enumerate()
            .map(|(i, m)| (m.name.key(), i))
            .collect();
        let new_count = parsed
            .iter()
            .filter(|p| !index.contains_key(&p.name.key()))
            .count();
        if self.menu.len() + new_count > Self::MAX_MENU_ITEMS {
            return Err(DomainError::LimitExceeded {
                field: "menu",
                max: Self::MAX_MENU_ITEMS,
            });
        }

        let mut next_order = self
            .menu
            .iter()
            .map(|m| m.sort_order)
            .max()
            .map_or(0, |m| m + 1);
        let mut outcomes = Vec::with_capacity(parsed.len());
        for p in parsed {
            match index.get(&p.name.key()) {
                Some(&i) => {
                    let m = &mut self.menu[i];
                    let mut changed = false;
                    if m.price != p.price {
                        m.price = p.price;
                        changed = true;
                    }
                    changed |= overwrite(&mut m.description, p.description);
                    changed |= overwrite(&mut m.category, p.category);
                    if m.archived {
                        m.archived = false;
                        changed = true;
                    }
                    outcomes.push(if changed {
                        UpsertOutcome::Updated(m.id)
                    } else {
                        UpsertOutcome::Unchanged(m.id)
                    });
                }
                None => {
                    let id = MenuItemId::new();
                    self.menu.push(MenuItem {
                        id,
                        name: p.name,
                        price: p.price,
                        description: p.description,
                        category: p.category,
                        sort_order: next_order,
                        archived: false,
                    });
                    next_order += 1;
                    outcomes.push(UpsertOutcome::Created(id));
                }
            }
        }
        Ok(UpsertReport { outcomes })
    }

    pub fn archive_item(&mut self, id: MenuItemId) -> Result<(), DomainError> {
        self.menu
            .iter_mut()
            .find(|m| m.id == id)
            .map(|m| m.archived = true)
            .ok_or(DomainError::NotFound {
                entity: "menu_item",
            })
    }

    pub fn remove_item(&mut self, id: MenuItemId) -> Result<(), DomainError> {
        let before = self.menu.len();
        self.menu.retain(|m| m.id != id);
        if self.menu.len() == before {
            return Err(DomainError::NotFound {
                entity: "menu_item",
            });
        }
        Ok(())
    }

    /// Usable for opening a group order: at least one non-archived item.
    pub fn is_orderable(&self) -> bool {
        self.menu.iter().any(|m| !m.archived)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vendor() -> Vendor {
        Vendor::new(OrganizerId::new(), "池上便當").unwrap()
    }
    fn item(n: &str, price: i64) -> MenuItemInput {
        MenuItemInput {
            name: n.into(),
            price,
            description: None,
            category: None,
        }
    }

    #[test]
    fn update_info_does_not_overwrite_with_blank() {
        let mut v = vendor();
        let p = VendorInfoPatch {
            phone: Some(" 02-1234 ".into()),
            address: Some("台北".into()),
            ..Default::default()
        };
        assert!(v.update_info(p.clone()).unwrap());
        assert!(!v.update_info(p).unwrap());
        let changed = v
            .update_info(VendorInfoPatch {
                name: Some("  ".into()),
                phone: Some("".into()),
                address: None,
                note: Some(" ".into()),
            })
            .unwrap();
        assert!(!changed);
        assert_eq!(v.phone(), Some("02-1234"));
        assert_eq!(v.address(), Some("台北"));
        assert_eq!(v.name().as_str(), "池上便當");
        assert!(
            v.update_info(VendorInfoPatch {
                name: Some("新店名".into()),
                ..Default::default()
            })
            .unwrap()
        );
        assert!(
            v.update_info(VendorInfoPatch {
                phone: Some("0".repeat(31)),
                name: Some("不應套用".into()),
                ..Default::default()
            })
            .is_err()
        );
        assert_eq!(v.name().as_str(), "新店名");
    }

    #[test]
    fn menu_upsert_and_price_overwrite() {
        let mut v = vendor();
        let rep = v
            .upsert_menu_items(vec![item("排骨飯", 100), item("雞腿飯", 120)])
            .unwrap();
        assert_eq!(rep.created(), 2);
        let mut i = item("排骨飯 ", 110);
        i.category = Some("飯類".into());
        let rep = v.upsert_menu_items(vec![i, item("雞腿飯", 120)]).unwrap();
        assert_eq!((rep.updated(), rep.unchanged()), (1, 1));
        assert_eq!(v.menu()[0].price.get(), 110);
        assert_eq!(v.menu()[0].category.as_deref(), Some("飯類"));
        // blank category does not clear
        v.upsert_menu_items(vec![item("排骨飯", 110)]).unwrap();
        assert_eq!(v.menu()[0].category.as_deref(), Some("飯類"));
    }

    #[test]
    fn invalid_price_and_duplicates_atomic() {
        let mut v = vendor();
        assert!(
            v.upsert_menu_items(vec![item("a", 10), item("b", 0)])
                .is_err()
        );
        assert!(
            v.upsert_menu_items(vec![item("A", 10), item("ａ", 10)])
                .is_err()
        );
        assert!(v.menu().is_empty());
    }

    #[test]
    fn archived_revived_and_orderable() {
        let mut v = vendor();
        assert!(!v.is_orderable());
        let rep = v.upsert_menu_items(vec![item("a", 10)]).unwrap();
        let UpsertOutcome::Created(id) = rep.outcomes[0] else {
            panic!()
        };
        assert!(v.is_orderable());
        v.archive_item(id).unwrap();
        assert!(!v.is_orderable());
        let rep = v.upsert_menu_items(vec![item("a", 10)]).unwrap();
        assert!(matches!(rep.outcomes[0], UpsertOutcome::Updated(_)));
        assert!(v.is_orderable());
        v.remove_item(id).unwrap();
        assert!(v.remove_item(id).is_err());
        assert!(v.archive_item(id).is_err());
    }

    #[test]
    fn menu_limit() {
        let mut v = vendor();
        let many: Vec<_> = (0..200).map(|i| item(&format!("i{i}"), 10)).collect();
        v.upsert_menu_items(many).unwrap();
        let err = v
            .upsert_menu_items(vec![item("i0", 99), item("extra", 10)])
            .unwrap_err();
        assert!(matches!(err, DomainError::LimitExceeded { max: 200, .. }));
        assert_eq!(v.menu().len(), 200);
        assert_eq!(v.menu()[0].price.get(), 10);
    }

    #[test]
    fn edit_info_clears_and_sets() {
        let mut v = vendor();
        v.update_info(VendorInfoPatch {
            phone: Some("02-1".into()),
            note: Some("n".into()),
            ..Default::default()
        })
        .unwrap();
        // Keep everything: no change.
        assert!(!v.edit_info(VendorInfoEdit::default()).unwrap());
        assert!(
            v.edit_info(VendorInfoEdit {
                phone: Patch::Clear,
                address: Patch::Set(" 台北 ".into()),
                ..Default::default()
            })
            .unwrap()
        );
        assert_eq!(v.phone(), None);
        assert_eq!(v.address(), Some("台北"));
        assert_eq!(v.note(), Some("n"));
        // Blank Set clears; blank name is invalid and applies nothing.
        assert!(
            v.edit_info(VendorInfoEdit {
                note: Patch::Set("".into()),
                ..Default::default()
            })
            .unwrap()
        );
        assert_eq!(v.note(), None);
        assert!(
            v.edit_info(VendorInfoEdit {
                name: Some(" ".into()),
                address: Patch::Clear,
                ..Default::default()
            })
            .is_err()
        );
        assert_eq!(v.address(), Some("台北"));
        assert!(
            v.edit_info(VendorInfoEdit {
                name: Some("新店".into()),
                ..Default::default()
            })
            .unwrap()
        );
        assert_eq!(v.name().as_str(), "新店");
    }

    #[test]
    fn edit_item_fields_and_conflict() {
        let mut v = vendor();
        let rep = v
            .upsert_menu_items(vec![item("排骨飯", 100), item("雞腿飯", 120)])
            .unwrap();
        let (UpsertOutcome::Created(a), UpsertOutcome::Created(b)) =
            (rep.outcomes[0], rep.outcomes[1])
        else {
            panic!()
        };
        v.edit_item(
            a,
            MenuItemEdit {
                price: Some(110),
                description: Patch::Set("好吃".into()),
                archived: Some(true),
                sort_order: Some(9),
                ..Default::default()
            },
        )
        .unwrap();
        let m = &v.menu()[0];
        assert_eq!(m.price.get(), 110);
        assert_eq!(m.description.as_deref(), Some("好吃"));
        assert!(m.archived);
        assert_eq!(m.sort_order, 9);
        v.edit_item(
            a,
            MenuItemEdit {
                description: Patch::Clear,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(v.menu()[0].description, None);
        let before = v.clone();
        assert!(matches!(
            v.edit_item(
                b,
                MenuItemEdit {
                    name: Some(" 排骨飯 ".into()),
                    price: Some(5),
                    ..Default::default()
                }
            ),
            Err(DomainError::Conflict { .. })
        ));
        assert!(
            v.edit_item(
                b,
                MenuItemEdit {
                    price: Some(0),
                    name: Some("x".into()),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert_eq!(v, before);
        assert!(matches!(
            v.edit_item(MenuItemId::new(), MenuItemEdit::default()),
            Err(DomainError::NotFound { .. })
        ));
    }
}
