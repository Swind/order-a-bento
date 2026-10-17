use std::fmt;
use unicode_normalization::UnicodeNormalization;

use super::{DomainError, text::collapse_whitespace};

/// Normalized name used to decide "same name": NFKC, trim, collapse
/// whitespace, lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NameKey(String);

impl NameKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NameKey {
    fn from(s: &str) -> Self {
        // NFKC again after lowercasing keeps the key a fixed point.
        let n: String = s.nfkc().collect();
        let l: String = n.to_lowercase().nfkc().collect();
        NameKey(collapse_whitespace(&l))
    }
}

impl fmt::Display for NameKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! display_name {
    ($ty:ident, $field:literal) => {
        /// Validated display name: whitespace-collapsed original (no NFKC),
        /// 1..=MAX_LEN chars.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $ty(String);

        impl $ty {
            pub const MAX_LEN: usize = 50;

            pub fn new(s: &str) -> Result<Self, DomainError> {
                let v = collapse_whitespace(s);
                let n = v.chars().count();
                if n == 0 {
                    return Err(DomainError::invalid($field, "不可為空"));
                }
                if n > Self::MAX_LEN {
                    return Err(DomainError::invalid(
                        $field,
                        format!("最多 {} 個字元", Self::MAX_LEN),
                    ));
                }
                Ok(Self(v))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn key(&self) -> NameKey {
                NameKey::from(self.0.as_str())
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

display_name!(PersonName, "name");
display_name!(ItemName, "name");
display_name!(VendorName, "name");
display_name!(RosterName, "name");

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn key_equivalences() {
        let k = |s| NameKey::from(s);
        assert_eq!(k("王小明"), k("王小明 "));
        assert_eq!(k("王小明"), k(" 王小明"));
        assert_eq!(k("ＡＭＹ"), k("amy"));
        assert_eq!(k("Amy  Lin"), k("amy lin"));
        assert_eq!(k("Amy\u{3000}Lin"), k("amy lin"));
        assert_ne!(k("王小明"), k("王小華"));
    }

    #[test]
    fn display_keeps_original() {
        let n = PersonName::new("  Ａｍｙ   Lin ").unwrap();
        assert_eq!(n.as_str(), "Ａｍｙ Lin");
        assert_eq!(n.key(), NameKey::from("amy lin"));
    }

    #[test]
    fn length_rules() {
        assert!(PersonName::new("").is_err());
        assert!(PersonName::new(" \u{3000} ").is_err());
        assert!(ItemName::new(&"字".repeat(50)).is_ok());
        assert!(ItemName::new(&"字".repeat(51)).is_err());
        assert!(VendorName::new(&"a".repeat(51)).is_err());
        assert!(RosterName::new(&"a".repeat(50)).is_ok());
    }

    proptest! {
        #[test]
        fn idempotent(s in ".*") {
            let k = NameKey::from(s.as_str());
            prop_assert_eq!(NameKey::from(k.as_str()), k);
        }

        #[test]
        fn padding_irrelevant(s in ".*", l in "[ \\t\\u{3000}]{0,4}", r in "[ \\t\\u{3000}]{0,4}") {
            let padded = format!("{l}{s}{r}");
            prop_assert_eq!(NameKey::from(padded.as_str()), NameKey::from(s.as_str()));
        }
    }
}
