use application::ports::TokenGenerator;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRngCore;
use rand::rngs::OsRng;

/// 32 bytes from the OS CSPRNG, base64url without padding.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsRngTokenGenerator;

impl TokenGenerator for OsRngTokenGenerator {
    fn generate(&self) -> String {
        let mut buf = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut buf)
            .expect("operating system RNG unavailable");
        URL_SAFE_NO_PAD.encode(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_43_urlsafe_chars_and_unique() {
        let g = OsRngTokenGenerator;
        let a = g.generate();
        assert_eq!(a.len(), 43);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_ne!(a, g.generate());
    }
}
