use std::collections::{HashMap, HashSet};

use super::{UpsertOutcome, UpsertReport};
use crate::identity::OrganizerId;
use crate::shared::{
    DomainError, Id, NameKey, Patch, PersonName, RosterName,
    text::{NOTE_MAX_LEN, optional_text},
};

pub struct RosterMarker;
pub struct RosterMemberMarker;
pub type RosterId = Id<RosterMarker>;
pub type RosterMemberId = Id<RosterMemberMarker>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterMember {
    pub id: RosterMemberId,
    pub name: PersonName,
    pub note: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInput {
    pub name: String,
    pub note: Option<String>,
}

/// Manual (tri-state) edit of one member: `name` None = keep; `note` per [`Patch`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberEdit {
    pub name: Option<String>,
    pub note: Patch<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    id: RosterId,
    organizer_id: OrganizerId,
    name: RosterName,
    members: Vec<RosterMember>,
}

impl Roster {
    pub const MAX_MEMBERS: usize = 500;

    pub fn new(organizer_id: OrganizerId, name: &str) -> Result<Self, DomainError> {
        Ok(Self {
            id: RosterId::new(),
            organizer_id,
            name: RosterName::new(name)?,
            members: Vec::new(),
        })
    }

    /// Reconstitute from storage. Data is trusted but uniqueness and the
    /// member limit are still checked.
    pub fn from_parts(
        id: RosterId,
        organizer_id: OrganizerId,
        name: RosterName,
        members: Vec<RosterMember>,
    ) -> Result<Self, DomainError> {
        if members.len() > Self::MAX_MEMBERS {
            return Err(DomainError::LimitExceeded {
                field: "members",
                max: Self::MAX_MEMBERS,
            });
        }
        let mut seen = HashSet::new();
        for m in &members {
            if !seen.insert(m.name.key()) {
                return Err(DomainError::Duplicate {
                    field: "name",
                    value: m.name.to_string(),
                });
            }
        }
        Ok(Self {
            id,
            organizer_id,
            name,
            members,
        })
    }

    pub fn id(&self) -> RosterId {
        self.id
    }
    pub fn organizer_id(&self) -> OrganizerId {
        self.organizer_id
    }
    pub fn name(&self) -> &RosterName {
        &self.name
    }
    pub fn members(&self) -> &[RosterMember] {
        &self.members
    }

    pub fn rename(&mut self, name: &str) -> Result<(), DomainError> {
        self.name = RosterName::new(name)?;
        Ok(())
    }

    pub fn remove_member(&mut self, id: RosterMemberId) -> Result<(), DomainError> {
        let before = self.members.len();
        self.members.retain(|m| m.id != id);
        if self.members.len() == before {
            return Err(DomainError::NotFound { entity: "member" });
        }
        Ok(())
    }

    /// Manual edit of a single member. Unlike [`Roster::upsert_members`] a `null`
    /// note clears it. Renaming onto another member's `NameKey` is a
    /// [`DomainError::Conflict`]. Validates everything before applying.
    pub fn edit_member(&mut self, id: RosterMemberId, edit: MemberEdit) -> Result<(), DomainError> {
        let idx = self
            .members
            .iter()
            .position(|m| m.id == id)
            .ok_or(DomainError::NotFound { entity: "member" })?;
        let name = edit.name.as_deref().map(PersonName::new).transpose()?;
        let note = edit.note.resolve("note", NOTE_MAX_LEN)?;
        if let Some(n) = &name
            && self
                .members
                .iter()
                .any(|m| m.id != id && m.name.key() == n.key())
        {
            return Err(DomainError::Conflict {
                field: "name",
                value: n.to_string(),
            });
        }
        let m = &mut self.members[idx];
        if let Some(n) = name {
            m.name = n;
        }
        note.apply(&mut m.note);
        Ok(())
    }

    /// All-or-nothing upsert by `NameKey`.
    ///
    /// Existing members keep their display name; `note` is overwritten only
    /// by a non-blank input note. New members are appended.
    pub fn upsert_members(
        &mut self,
        inputs: Vec<MemberInput>,
    ) -> Result<UpsertReport<RosterMemberId>, DomainError> {
        let mut parsed = Vec::with_capacity(inputs.len());
        let mut seen: HashSet<NameKey> = HashSet::new();
        for i in &inputs {
            let name = PersonName::new(&i.name)?;
            let note = optional_text("note", i.note.as_deref(), NOTE_MAX_LEN)?;
            if !seen.insert(name.key()) {
                return Err(DomainError::Duplicate {
                    field: "name",
                    value: name.to_string(),
                });
            }
            parsed.push((name, note));
        }
        let index: HashMap<NameKey, usize> = self
            .members
            .iter()
            .enumerate()
            .map(|(i, m)| (m.name.key(), i))
            .collect();
        let new_count = parsed
            .iter()
            .filter(|(n, _)| !index.contains_key(&n.key()))
            .count();
        if self.members.len() + new_count > Self::MAX_MEMBERS {
            return Err(DomainError::LimitExceeded {
                field: "members",
                max: Self::MAX_MEMBERS,
            });
        }

        let mut next_order = self
            .members
            .iter()
            .map(|m| m.sort_order)
            .max()
            .map_or(0, |m| m + 1);
        let mut outcomes = Vec::with_capacity(parsed.len());
        for (name, note) in parsed {
            match index.get(&name.key()) {
                Some(&i) => {
                    let m = &mut self.members[i];
                    match note {
                        Some(n) if m.note.as_deref() != Some(n.as_str()) => {
                            m.note = Some(n);
                            outcomes.push(UpsertOutcome::Updated(m.id));
                        }
                        _ => outcomes.push(UpsertOutcome::Unchanged(m.id)),
                    }
                }
                None => {
                    let id = RosterMemberId::new();
                    self.members.push(RosterMember {
                        id,
                        name,
                        note,
                        sort_order: next_order,
                    });
                    next_order += 1;
                    outcomes.push(UpsertOutcome::Created(id));
                }
            }
        }
        Ok(UpsertReport { outcomes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster {
        Roster::new(OrganizerId::new(), "午餐組").unwrap()
    }
    fn inp(n: &str, note: Option<&str>) -> MemberInput {
        MemberInput {
            name: n.into(),
            note: note.map(Into::into),
        }
    }

    #[test]
    fn create_update_unchanged() {
        let mut r = roster();
        let rep = r
            .upsert_members(vec![inp("王小明", None), inp("Amy", Some("素食"))])
            .unwrap();
        assert_eq!((rep.created(), rep.updated(), rep.unchanged()), (2, 0, 0));
        assert_eq!(r.members()[1].sort_order, 1);

        let rep = r
            .upsert_members(vec![
                inp("ＡＭＹ", Some("不吃辣")),
                inp("王小明 ", Some("  ")),
                inp("新人", None),
            ])
            .unwrap();
        assert_eq!((rep.created(), rep.updated(), rep.unchanged()), (1, 1, 1));
        assert!(matches!(rep.outcomes[0], UpsertOutcome::Updated(_)));
        assert!(matches!(rep.outcomes[1], UpsertOutcome::Unchanged(_)));
        assert!(matches!(rep.outcomes[2], UpsertOutcome::Created(_)));
        assert_eq!(r.members()[1].name.as_str(), "Amy");
        assert_eq!(r.members()[1].note.as_deref(), Some("不吃辣"));
        assert_eq!(r.members()[2].sort_order, 2);

        // same note again -> unchanged
        let rep = r.upsert_members(vec![inp("amy", Some("不吃辣"))]).unwrap();
        assert_eq!(rep.unchanged(), 1);
    }

    #[test]
    fn duplicate_in_input_rejected_atomically() {
        let mut r = roster();
        let err = r
            .upsert_members(vec![inp("Amy", None), inp("ａｍｙ ", None)])
            .unwrap_err();
        assert!(matches!(err, DomainError::Duplicate { .. }));
        assert!(r.members().is_empty());
    }

    #[test]
    fn invalid_input_rejected_atomically() {
        let mut r = roster();
        assert!(
            r.upsert_members(vec![inp("ok", None), inp(" ", None)])
                .is_err()
        );
        assert!(r.members().is_empty());
    }

    #[test]
    fn limit_no_partial_mutation() {
        let mut r = roster();
        let many: Vec<_> = (0..500).map(|i| inp(&format!("p{i}"), None)).collect();
        r.upsert_members(many).unwrap();
        let err = r
            .upsert_members(vec![inp("p0", Some("x")), inp("extra", None)])
            .unwrap_err();
        assert!(matches!(err, DomainError::LimitExceeded { max: 500, .. }));
        assert_eq!(r.members().len(), 500);
        assert_eq!(r.members()[0].note, None);
        // updating existing at the limit is fine
        assert!(r.upsert_members(vec![inp("p0", Some("x"))]).is_ok());
    }

    #[test]
    fn rename_remove_from_parts() {
        let mut r = roster();
        r.rename("新名單").unwrap();
        assert!(r.rename(" ").is_err());
        let rep = r.upsert_members(vec![inp("a", None)]).unwrap();
        let UpsertOutcome::Created(id) = rep.outcomes[0] else {
            panic!()
        };
        r.remove_member(id).unwrap();
        assert!(matches!(
            r.remove_member(id),
            Err(DomainError::NotFound { .. })
        ));
        let m = |n: &str| RosterMember {
            id: RosterMemberId::new(),
            name: PersonName::new(n).unwrap(),
            note: None,
            sort_order: 0,
        };
        assert!(
            Roster::from_parts(
                RosterId::new(),
                OrganizerId::new(),
                RosterName::new("x").unwrap(),
                vec![m("A"), m("a")]
            )
            .is_err()
        );
    }

    #[test]
    fn edit_member_tri_state_and_conflict() {
        let mut r = roster();
        let rep = r
            .upsert_members(vec![inp("Amy", Some("素食")), inp("Bob", None)])
            .unwrap();
        let (UpsertOutcome::Created(amy), UpsertOutcome::Created(bob)) =
            (rep.outcomes[0], rep.outcomes[1])
        else {
            panic!()
        };
        // Keep: absent note stays.
        r.edit_member(
            amy,
            MemberEdit {
                name: Some("Ａｍｙ Lin".into()),
                note: Patch::Keep,
            },
        )
        .unwrap();
        assert_eq!(r.members()[0].name.as_str(), "Ａｍｙ Lin");
        assert_eq!(r.members()[0].note.as_deref(), Some("素食"));
        // Clear.
        r.edit_member(
            amy,
            MemberEdit {
                name: None,
                note: Patch::Clear,
            },
        )
        .unwrap();
        assert_eq!(r.members()[0].note, None);
        // Case-only rename of itself is fine; collision with Bob is not.
        r.edit_member(
            bob,
            MemberEdit {
                name: Some("BOB".into()),
                note: Patch::Set(" x ".into()),
            },
        )
        .unwrap();
        assert_eq!(r.members()[1].note.as_deref(), Some("x"));
        let before = r.clone();
        let err = r
            .edit_member(
                bob,
                MemberEdit {
                    name: Some("ａｍｙ  lin".into()),
                    note: Patch::Clear,
                },
            )
            .unwrap_err();
        assert!(matches!(err, DomainError::Conflict { .. }));
        assert_eq!(r, before);
        // Invalid input applies nothing; unknown id is NotFound.
        assert!(
            r.edit_member(
                bob,
                MemberEdit {
                    name: Some(" ".into()),
                    note: Patch::Clear
                }
            )
            .is_err()
        );
        assert_eq!(r, before);
        assert!(matches!(
            r.edit_member(RosterMemberId::new(), MemberEdit::default()),
            Err(DomainError::NotFound { .. })
        ));
    }
}
