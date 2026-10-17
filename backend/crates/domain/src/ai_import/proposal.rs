use serde::{Deserialize, Serialize};

use crate::directory::{RosterId, VendorId};
use crate::shared::NameKey;

/// What applying a proposed change would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeAction {
    Create,
    Update,
    Unchanged,
}

impl ChangeAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Unchanged => "unchanged",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// Problem with one proposed entry. Serialized as a plain snake_case string so
/// the frontend can switch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Issue {
    PriceMissing,
    PriceOutOfRange,
    NameEmpty,
    NameTooLong,
    NoteTooLong,
    DescriptionTooLong,
    CategoryTooLong,
    DuplicateInInput,
}

/// Which aggregate a target refers to.
///
/// For a `New` vendor `name` is the vendor name (same as `info.after.name`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetRef<I> {
    #[serde(rename_all = "camelCase")]
    Existing {
        existing_id: I,
    },
    New {
        name: String,
    },
}

impl<I: PartialEq> TargetRef<I> {
    /// Same existing id, or same `NameKey` of the new name.
    pub fn same_as(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Existing { existing_id: a }, Self::Existing { existing_id: b }) => a == b,
            (Self::New { name: a }, Self::New { name: b }) => {
                NameKey::from(a.as_str()) == NameKey::from(b.as_str())
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedMember {
    pub action: ChangeAction,
    pub name: String,
    /// Existing note (None for new members or when there was none).
    pub before_note: Option<String>,
    /// Resulting note (existing note is kept when the input note is blank).
    pub note: Option<String>,
    pub confidence: Confidence,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterTarget {
    pub roster: TargetRef<RosterId>,
    pub members: Vec<ProposedMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorInfoSnapshot {
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedVendorInfo {
    pub action: ChangeAction,
    pub before: Option<VendorInfoSnapshot>,
    pub after: VendorInfoSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemSnapshot {
    pub price: i64,
    pub description: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedMenuItem {
    pub action: ChangeAction,
    pub name: String,
    pub before: Option<MenuItemSnapshot>,
    /// `None` = price unknown (issue `price_missing`); the user must fill it in.
    pub price: Option<i64>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub confidence: Confidence,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorTarget {
    pub vendor: TargetRef<VendorId>,
    pub info: ProposedVendorInfo,
    pub menu: Vec<ProposedMenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProposalTarget {
    Roster(RosterTarget),
    Vendor(VendorTarget),
}

impl ProposalTarget {
    /// Whether both proposals address the same roster / vendor.
    pub fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Roster(a), Self::Roster(b)) => a.roster.same_as(&b.roster),
            (Self::Vendor(a), Self::Vendor(b)) => a.vendor.same_as(&b.vendor),
            _ => false,
        }
    }
}

/// Pending changes accumulated by the agent's propose tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub targets: Vec<ProposalTarget>,
    pub warnings: Vec<String>,
}

impl Proposal {
    pub const MAX_WARNINGS: usize = 20;
    pub const MAX_WARNING_CHARS: usize = 200;

    /// Insert `target`; an earlier proposal for the same target is replaced
    /// in place (the agent fixing a previous attempt).
    pub fn replace_target(&mut self, target: ProposalTarget) {
        match self.targets.iter().position(|t| t.same_target(&target)) {
            Some(i) => self.targets[i] = target,
            None => self.targets.push(target),
        }
    }

    /// Append warnings (trimmed, truncated, de-duplicated, capped).
    pub fn add_warnings<I: IntoIterator<Item = String>>(&mut self, warnings: I) {
        for w in warnings {
            let w: String = w.trim().chars().take(Self::MAX_WARNING_CHARS).collect();
            if !w.is_empty()
                && self.warnings.len() < Self::MAX_WARNINGS
                && !self.warnings.contains(&w)
            {
                self.warnings.push(w);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str) -> ProposedMember {
        ProposedMember {
            action: ChangeAction::Create,
            name: name.into(),
            before_note: None,
            note: Some("x".into()),
            confidence: Confidence::High,
            issues: vec![],
        }
    }
    fn roster_new(name: &str, members: Vec<ProposedMember>) -> ProposalTarget {
        ProposalTarget::Roster(RosterTarget {
            roster: TargetRef::New { name: name.into() },
            members,
        })
    }
    fn vendor(r: TargetRef<VendorId>, item: &str) -> ProposalTarget {
        ProposalTarget::Vendor(VendorTarget {
            vendor: r.clone(),
            info: ProposedVendorInfo {
                action: ChangeAction::Create,
                before: None,
                after: VendorInfoSnapshot {
                    name: "店".into(),
                    phone: None,
                    address: None,
                    note: None,
                },
            },
            menu: vec![ProposedMenuItem {
                action: ChangeAction::Create,
                name: item.into(),
                before: Some(MenuItemSnapshot {
                    price: 1,
                    description: None,
                    category: None,
                }),
                price: None,
                description: None,
                category: None,
                confidence: Confidence::Low,
                issues: vec![Issue::PriceMissing],
            }],
        })
    }

    #[test]
    fn json_round_trip_and_shape() {
        let rid = RosterId::new();
        let vid = VendorId::new();
        let mut p = Proposal::default();
        p.replace_target(roster_new("新名單", vec![member("Amy")]));
        p.replace_target(ProposalTarget::Roster(RosterTarget {
            roster: TargetRef::Existing { existing_id: rid },
            members: vec![],
        }));
        p.replace_target(vendor(TargetRef::Existing { existing_id: vid }, "雞腿飯"));
        p.add_warnings(["請確認".to_string()]);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Proposal>(&json).unwrap(), p);

        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["targets"][0]["type"], "roster");
        assert_eq!(v["targets"][0]["roster"]["kind"], "new");
        assert_eq!(v["targets"][1]["roster"]["existingId"], rid.to_string());
        assert_eq!(v["targets"][2]["type"], "vendor");
        assert_eq!(v["targets"][2]["menu"][0]["issues"][0], "price_missing");
        assert_eq!(v["targets"][2]["menu"][0]["confidence"], "low");
        assert_eq!(
            v["targets"][0]["members"][0]["beforeNote"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn replace_target_semantics() {
        let mut p = Proposal::default();
        p.replace_target(roster_new("業務部", vec![member("A")]));
        p.replace_target(roster_new("其他", vec![member("B")]));
        // same New name by NameKey replaces in place
        p.replace_target(roster_new(" ＡBC ", vec![]));
        p.replace_target(roster_new("業務部 ", vec![member("A2")]));
        assert_eq!(p.targets.len(), 3);
        let ProposalTarget::Roster(r) = &p.targets[0] else {
            panic!()
        };
        assert_eq!(r.members[0].name, "A2");

        let id = RosterId::new();
        let ex = |m| {
            ProposalTarget::Roster(RosterTarget {
                roster: TargetRef::Existing { existing_id: id },
                members: m,
            })
        };
        p.replace_target(ex(vec![member("1")]));
        p.replace_target(ex(vec![member("2")]));
        assert_eq!(p.targets.len(), 4);

        // vendors: New by name key, Existing by id; roster and vendor never collide
        let n = |s: &str| TargetRef::<VendorId>::New { name: s.into() };
        p.replace_target(vendor(n("店"), "a"));
        p.replace_target(vendor(n("店 "), "b"));
        assert_eq!(p.targets.len(), 5);
        let vid = VendorId::new();
        p.replace_target(vendor(TargetRef::Existing { existing_id: vid }, "c"));
        p.replace_target(vendor(TargetRef::Existing { existing_id: vid }, "d"));
        assert_eq!(p.targets.len(), 6);
        let ProposalTarget::Vendor(v) = &p.targets[5] else {
            panic!()
        };
        assert_eq!(v.menu[0].name, "d");
    }

    #[test]
    fn warnings_are_deduped_and_capped() {
        let mut p = Proposal::default();
        p.add_warnings(["a".into(), " a ".into(), "".into()]);
        assert_eq!(p.warnings, ["a"]);
        p.add_warnings((0..40).map(|i| format!("w{i}")));
        assert_eq!(p.warnings.len(), Proposal::MAX_WARNINGS);
    }
}
