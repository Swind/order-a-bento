/// Device / connection metadata attached to a submission. Everything here is
/// advisory (client-supplied values can be forged). Pure value object; UA
/// parsing happens outside the domain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientInfo {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub ua_summary: Option<String>,
    pub device_id: Option<String>,
    pub device_model: Option<String>,
    pub screen: Option<String>,
    pub language: Option<String>,
    pub timezone: Option<String>,
}

fn clean(v: Option<String>, max: usize) -> Option<String> {
    let t = v?;
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    // `take` on chars never splits a multibyte character.
    Some(
        t.chars()
            .take(max)
            .collect::<String>()
            .trim_end()
            .to_owned(),
    )
    .filter(|s| !s.is_empty())
}

impl ClientInfo {
    pub const IP_MAX: usize = 64;
    pub const USER_AGENT_MAX: usize = 512;
    pub const UA_SUMMARY_MAX: usize = 100;
    pub const DEVICE_ID_MAX: usize = 64;
    pub const DEVICE_MODEL_MAX: usize = 100;
    pub const SCREEN_MAX: usize = 32;
    pub const LANGUAGE_MAX: usize = 35;
    pub const TIMEZONE_MAX: usize = 64;

    /// Trim, drop blanks, truncate each field (by chars); `device_id` keeps
    /// only `[A-Za-z0-9-]`.
    pub fn sanitize(self) -> Self {
        let device_id = self.device_id.map(|d| {
            d.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect::<String>()
        });
        Self {
            ip: clean(self.ip, Self::IP_MAX),
            user_agent: clean(self.user_agent, Self::USER_AGENT_MAX),
            ua_summary: clean(self.ua_summary, Self::UA_SUMMARY_MAX),
            device_id: clean(device_id, Self::DEVICE_ID_MAX),
            device_model: clean(self.device_model, Self::DEVICE_MODEL_MAX),
            screen: clean(self.screen, Self::SCREEN_MAX),
            language: clean(self.language, Self::LANGUAGE_MAX),
            timezone: clean(self.timezone, Self::TIMEZONE_MAX),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_drops_blank_and_truncates_on_char_boundary() {
        let ci = ClientInfo {
            ip: Some("  1.2.3.4 ".into()),
            user_agent: Some("字".repeat(600)),
            ua_summary: Some("   ".into()),
            device_model: Some("😀".repeat(150)),
            ..Default::default()
        }
        .sanitize();
        assert_eq!(ci.ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(ci.user_agent.unwrap().chars().count(), 512);
        assert_eq!(ci.ua_summary, None);
        assert_eq!(ci.device_model.unwrap().chars().count(), 100);
    }

    #[test]
    fn device_id_filtered() {
        let ci = ClientInfo {
            device_id: Some(" ab-12_<script>字 ".into()),
            ..Default::default()
        }
        .sanitize();
        assert_eq!(ci.device_id.as_deref(), Some("ab-12script"));
        let ci = ClientInfo {
            device_id: Some("!!!".into()),
            ..Default::default()
        }
        .sanitize();
        assert!(ci.is_empty());
        let long = ClientInfo {
            device_id: Some("a".repeat(100)),
            ..Default::default()
        }
        .sanitize();
        assert_eq!(long.device_id.unwrap().len(), 64);
    }
}
