use crate::shared::{DomainError, Id};

/// Marker for organizer ids.
pub struct OrganizerMarker;
pub type OrganizerId = Id<OrganizerMarker>;

/// Password policy (local accounts): length in characters.
pub const PASSWORD_MIN_LEN: usize = 10;
pub const PASSWORD_MAX_LEN: usize = 256;

/// Canonical form of a local-account email: trimmed and lowercased.
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Validate and normalise a local-account email (`local@domain`, no spaces).
pub fn parse_local_email(raw: &str) -> Result<String, DomainError> {
    let email = normalize_email(raw);
    match email.split_once('@') {
        Some((local, domain))
            if !local.is_empty()
                && !domain.is_empty()
                && !email.chars().any(|c| c.is_whitespace() || c.is_control()) =>
        {
            Ok(email)
        }
        _ => Err(DomainError::invalid("email", "格式不正確")),
    }
}

/// Check a new password against the policy (length only; no composition rules).
pub fn validate_password(password: &str) -> Result<(), DomainError> {
    let n = password.chars().count();
    if n < PASSWORD_MIN_LEN {
        return Err(DomainError::invalid(
            "password",
            format!("至少需要 {PASSWORD_MIN_LEN} 個字元"),
        ));
    }
    if n > PASSWORD_MAX_LEN {
        return Err(DomainError::invalid(
            "password",
            format!("不可超過 {PASSWORD_MAX_LEN} 個字元"),
        ));
    }
    Ok(())
}

/// A logged-in organizer (group-order owner). Either identified by a Google
/// `sub` (Google and dev logins) or a *local account* (`google_sub` is `None`)
/// that signs in with email and password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organizer {
    pub id: OrganizerId,
    pub google_sub: Option<String>,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

impl Organizer {
    /// Create a new organizer with a fresh id; email and display name must be non-empty.
    pub fn new(
        google_sub: &str,
        email: &str,
        display_name: &str,
        avatar_url: Option<String>,
    ) -> Result<Self, DomainError> {
        let google_sub = google_sub.trim();
        let email = email.trim();
        let display_name = display_name.trim();
        if google_sub.is_empty() {
            return Err(DomainError::invalid("googleSub", "不可為空"));
        }
        if email.is_empty() {
            return Err(DomainError::invalid("email", "不可為空"));
        }
        if display_name.is_empty() {
            return Err(DomainError::invalid("displayName", "不可為空"));
        }
        Ok(Self {
            id: OrganizerId::new(),
            google_sub: Some(google_sub.to_owned()),
            email: email.to_owned(),
            display_name: display_name.to_owned(),
            avatar_url: avatar_url.filter(|u| !u.trim().is_empty()),
        })
    }

    /// Create a local (password) account; the email is normalised.
    pub fn new_local(email: &str, display_name: &str) -> Result<Self, DomainError> {
        let display_name = display_name.trim();
        let email = parse_local_email(email)?;
        if display_name.is_empty() {
            return Err(DomainError::invalid("displayName", "不可為空"));
        }
        Ok(Self {
            id: OrganizerId::new(),
            google_sub: None,
            email,
            display_name: display_name.to_owned(),
            avatar_url: None,
        })
    }

    /// True for accounts without a Google/dev identity.
    pub fn is_local(&self) -> bool {
        self.google_sub.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_fields() {
        assert!(Organizer::new("s", "a@b.c", "A", None).is_ok());
        assert!(Organizer::new("s", " ", "A", None).is_err());
        assert!(Organizer::new("s", "a@b.c", "", None).is_err());
        assert!(Organizer::new("", "a@b.c", "A", None).is_err());
        let o = Organizer::new("s", "a@b.c", "A", Some("".into())).unwrap();
        assert!(o.avatar_url.is_none());
    }

    #[test]
    fn local_accounts_normalise_email() {
        let o = Organizer::new_local("  Admin@Example.COM ", " Root ").unwrap();
        assert_eq!(o.email, "admin@example.com");
        assert_eq!(o.display_name, "Root");
        assert!(o.is_local());
        for bad in ["", "nope", "@x.y", "a@", "a b@x.y"] {
            assert!(Organizer::new_local(bad, "A").is_err(), "{bad}");
        }
        assert!(Organizer::new_local("a@b.c", " ").is_err());
    }

    #[test]
    fn password_policy_counts_characters() {
        assert!(validate_password("123456789").is_err());
        assert!(validate_password("1234567890").is_ok());
        assert!(validate_password("密碼密碼密碼密碼密碼").is_ok());
        assert!(validate_password(&"a".repeat(256)).is_ok());
        assert!(validate_password(&"a".repeat(257)).is_err());
    }
}
