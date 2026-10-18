//! The application version: the workspace `version` from `backend/Cargo.toml`, plus the CI
//! build number when `BUILD_NUMBER` was set at compile time ("0.1.0.42", else "0.1.0").

use std::sync::OnceLock;

/// Pure formatting; a build number that is not all ASCII digits is ignored.
pub fn format_version(base: &str, build: Option<&str>) -> String {
    match build {
        Some(b) if !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()) => {
            format!("{base}.{b}")
        }
        _ => base.to_string(),
    }
}

/// The version of this build, computed once.
pub fn version() -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| format_version(env!("CARGO_PKG_VERSION"), option_env!("BUILD_NUMBER")))
}

#[cfg(test)]
mod tests {
    use super::format_version;

    #[test]
    fn formats_with_and_without_build_number() {
        assert_eq!(format_version("0.1.0", None), "0.1.0");
        assert_eq!(format_version("0.1.0", Some("42")), "0.1.0.42");
        assert_eq!(format_version("0.1.0", Some("")), "0.1.0");
        assert_eq!(format_version("0.1.0", Some("4x")), "0.1.0");
        assert_eq!(format_version("0.1.0", Some("-1")), "0.1.0");
    }

    #[test]
    fn version_starts_with_package_version() {
        assert!(super::version().starts_with(env!("CARGO_PKG_VERSION")));
    }
}
