//! `SystemTime` <-> RFC 3339 UTC text (second precision, fixed width so that
//! SQL string comparison matches chronological order).

use std::time::SystemTime;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub fn to_text(t: SystemTime) -> String {
    let dt = OffsetDateTime::from(t)
        .replace_nanosecond(0)
        .expect("0 is a valid nanosecond");
    dt.format(&Rfc3339)
        .expect("RFC 3339 formatting cannot fail")
}

pub fn from_text(s: &str) -> Result<SystemTime, time::error::Parse> {
    OffsetDateTime::parse(s, &Rfc3339).map(SystemTime::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn roundtrip_truncates_to_seconds() {
        let t = SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 123_000_000);
        let s = to_text(t);
        assert_eq!(s, "2023-11-14T22:13:20Z");
        assert_eq!(
            from_text(&s).unwrap(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
        assert!(from_text("garbage").is_err());
    }
}
