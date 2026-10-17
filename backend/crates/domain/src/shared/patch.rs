//! Tri-state field edit used by manual (PATCH-style) edits.

use super::DomainError;
use super::text::optional_text;

/// JSON merge-patch semantics: absent = keep, `null` = clear, value = set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Patch<T> {
    #[default]
    Keep,
    Clear,
    Set(T),
}

impl<T> Patch<T> {
    pub fn from_option(v: Option<Option<T>>) -> Self {
        match v {
            None => Self::Keep,
            Some(None) => Self::Clear,
            Some(Some(t)) => Self::Set(t),
        }
    }
}

impl Patch<String> {
    /// Apply to an optional text slot. A blank `Set` clears. Returns whether the
    /// slot changed. Validation happens in [`Patch::resolve`] so callers can
    /// validate everything before mutating anything.
    pub fn resolve(
        &self,
        field: &'static str,
        max: usize,
    ) -> Result<Resolved<String>, DomainError> {
        Ok(match self {
            Patch::Keep => Resolved::Keep,
            Patch::Clear => Resolved::Value(None),
            Patch::Set(s) => Resolved::Value(optional_text(field, Some(s), max)?),
        })
    }
}

/// A validated [`Patch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved<T> {
    Keep,
    Value(Option<T>),
}

impl<T: PartialEq> Resolved<T> {
    /// Write into `slot`; returns whether it changed.
    pub fn apply(self, slot: &mut Option<T>) -> bool {
        match self {
            Resolved::Keep => false,
            Resolved::Value(v) if *slot != v => {
                *slot = v;
                true
            }
            Resolved::Value(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_option_maps_three_states() {
        assert_eq!(Patch::<i32>::from_option(None), Patch::Keep);
        assert_eq!(Patch::<i32>::from_option(Some(None)), Patch::Clear);
        assert_eq!(Patch::from_option(Some(Some(1))), Patch::Set(1));
    }

    #[test]
    fn resolve_and_apply() {
        let mut slot = Some("a".to_string());
        assert!(!Patch::Keep.resolve("note", 5).unwrap().apply(&mut slot));
        assert!(
            Patch::Set(" b ".to_string())
                .resolve("note", 5)
                .unwrap()
                .apply(&mut slot)
        );
        assert_eq!(slot.as_deref(), Some("b"));
        assert!(
            !Patch::Set("b".to_string())
                .resolve("note", 5)
                .unwrap()
                .apply(&mut slot)
        );
        assert!(
            Patch::Set("  ".to_string())
                .resolve("note", 5)
                .unwrap()
                .apply(&mut slot)
        );
        assert_eq!(slot, None);
        assert!(!Patch::Clear.resolve("note", 5).unwrap().apply(&mut slot));
        assert!(
            Patch::Set("toolong".to_string())
                .resolve("note", 5)
                .is_err()
        );
    }
}
