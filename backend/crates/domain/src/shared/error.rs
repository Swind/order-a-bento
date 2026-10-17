use thiserror::Error;

/// Structured domain error; the API layer maps variants to HTTP problems.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    #[error("欄位「{field}」無效：{reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("欄位「{field}」重複：{value}")]
    Duplicate { field: &'static str, value: String },
    #[error("欄位「{field}」超過上限 {max}")]
    LimitExceeded { field: &'static str, max: usize },
    /// The value collides with an existing record (not within the same request).
    #[error("欄位「{field}」已存在：{value}")]
    Conflict { field: &'static str, value: String },
    #[error("找不到 {entity}")]
    NotFound { entity: &'static str },
    /// The group order is not accepting submissions. `reason` is a stable
    /// machine code: `group_closed`, `deadline_passed` or `group_archived`
    /// (API: 409 with `code = reason`).
    #[error("團購目前不接受送出（{reason}）")]
    Closed { reason: &'static str },
    /// The item is marked sold out (API: 409 `sold_out`).
    #[error("品項已售完：{item}")]
    SoldOut { item: String },
}

impl DomainError {
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}
