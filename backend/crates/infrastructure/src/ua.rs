//! `User-Agent` summaries (`UaParser` adapter) built on `woothee`.
//!
//! Examples: `iPhone · iOS 18.1 · LINE 14.17.0`, `Android · Chrome 129`,
//! `Mac · Safari 18`. The LINE in-app browser (` Line/<version>` in the UA) is
//! preferred over the underlying browser name.

use application::ports::UaParser;
use woothee::parser::Parser;

const UNKNOWN: &str = "UNKNOWN";

#[derive(Debug, Default, Clone, Copy)]
pub struct WootheeUaParser;

/// Version of the LINE in-app browser, e.g. `Some("14.17.0")`; `Some("")` when
/// the marker is present without a usable version.
fn line_version(ua: &str) -> Option<String> {
    let lower = ua.to_ascii_lowercase();
    let at = lower.find(" line/")? + " line/".len();
    let v: String = lower[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    Some(v.trim_matches('.').to_owned())
}

fn known(s: &str) -> Option<&str> {
    (!s.is_empty() && s != UNKNOWN).then_some(s)
}

fn device_label(os: &str) -> Option<&'static str> {
    Some(match os {
        "iPhone" => "iPhone",
        "iPad" => "iPad",
        "Android" => "Android",
        "Mac OSX" => "Mac",
        "Linux" => "Linux",
        "ChromeOS" => "ChromeOS",
        s if s.starts_with("Windows") => "Windows",
        _ => return None,
    })
}

fn major(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

impl UaParser for WootheeUaParser {
    fn summarize(&self, ua: &str) -> Option<String> {
        let ua = ua.trim();
        if ua.is_empty() {
            return None;
        }
        let parsed = Parser::new().parse(ua);
        let mut parts: Vec<String> = Vec::new();
        if let Some(r) = &parsed
            && let Some(os) = known(r.os)
        {
            match device_label(os) {
                Some(label) => parts.push(label.to_owned()),
                None => parts.push(os.to_owned()),
            }
            if matches!(os, "iPhone" | "iPad")
                && let Some(v) = known(&r.os_version)
            {
                parts.push(format!("iOS {v}"));
            }
        }
        if let Some(v) = line_version(ua) {
            parts.push(if v.is_empty() {
                "LINE".to_owned()
            } else {
                format!("LINE {v}")
            });
        } else if let Some(r) = &parsed
            && let Some(name) = known(r.name)
        {
            parts.push(match known(r.version) {
                Some(v) => format!("{name} {}", major(v)),
                None => name.to_owned(),
            });
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(ua: &str) -> Option<String> {
        WootheeUaParser.summarize(ua)
    }

    #[test]
    fn iphone_line() {
        assert_eq!(
            s("Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari Line/14.17.0").as_deref(),
            Some("iPhone · iOS 18.1 · LINE 14.17.0")
        );
    }

    #[test]
    fn android_line_beats_webview() {
        assert_eq!(
            s("Mozilla/5.0 (Linux; Android 14; SM-S918B Build/UP1A.231005.007; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/129.0.6668.81 Mobile Safari/537.36 Line/14.17.0").as_deref(),
            Some("Android · LINE 14.17.0")
        );
        // Case-insensitive marker, version followed by more tokens.
        assert_eq!(
            s("Mozilla/5.0 (Linux; Android 13) Chrome/120.0.0.0 Mobile Safari/537.36 LINE/13.1.0/LIFF").as_deref(),
            Some("Android · LINE 13.1.0")
        );
    }

    #[test]
    fn android_chrome() {
        assert_eq!(
            s("Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.6668.81 Mobile Safari/537.36").as_deref(),
            Some("Android · Chrome 129")
        );
    }

    #[test]
    fn desktop() {
        assert_eq!(
            s("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36").as_deref(),
            Some("Windows · Chrome 129")
        );
        assert_eq!(
            s("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15").as_deref(),
            Some("Mac · Safari 18")
        );
    }

    #[test]
    fn iphone_safari() {
        assert_eq!(
            s("Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1").as_deref(),
            Some("iPhone · iOS 18.1 · Safari 18")
        );
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(s("garbage!!"), None);
        assert_eq!(s(""), None);
        assert_eq!(s("   "), None);
        assert_eq!(s("Airline/9.9"), None);
    }
}
