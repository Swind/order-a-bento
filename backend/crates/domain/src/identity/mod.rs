//! Identity context: organizer accounts.

mod organizer;

pub use organizer::{
    Organizer, OrganizerId, OrganizerMarker, PASSWORD_MAX_LEN, PASSWORD_MIN_LEN, normalize_email,
    parse_local_email, validate_password,
};
