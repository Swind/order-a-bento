//! Text helpers and optional free-text field limits.

use super::DomainError;

pub const NOTE_MAX_LEN: usize = 200;
pub const DESCRIPTION_MAX_LEN: usize = 200;
pub const PHONE_MAX_LEN: usize = 30;
pub const ADDRESS_MAX_LEN: usize = 200;
pub const CATEGORY_MAX_LEN: usize = 30;

/// Trim and collapse whitespace runs to one ASCII space (case/width kept).
pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Trim; blank -> None; reject over `max` chars.
pub fn optional_text(
    field: &'static str,
    v: Option<&str>,
    max: usize,
) -> Result<Option<String>, DomainError> {
    match v.map(str::trim) {
        None | Some("") => Ok(None),
        Some(t) if t.chars().count() > max => {
            Err(DomainError::invalid(field, format!("最多 {max} 個字元")))
        }
        Some(t) => Ok(Some(t.to_string())),
    }
}
