use crate::shared::{DomainError, Id};

/// Marker for organizer ids.
pub struct OrganizerMarker;
pub type OrganizerId = Id<OrganizerMarker>;

/// A logged-in organizer (group-order owner), identified by a Google `sub`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organizer {
    pub id: OrganizerId,
    pub google_sub: String,
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
            google_sub: google_sub.to_owned(),
            email: email.to_owned(),
            display_name: display_name.to_owned(),
            avatar_url: avatar_url.filter(|u| !u.trim().is_empty()),
        })
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
}
