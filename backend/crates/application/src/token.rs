//! Session token helpers. The raw token only ever lives in the cookie;
//! the database stores its SHA-256 hash.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// SHA-256 of a raw session token (the value stored in `sessions.token_hash`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenHash(pub [u8; 32]);

impl TokenHash {
    pub fn of(raw: &str) -> Self {
        Self(Sha256::digest(raw.as_bytes()).into())
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenHash(..)")
    }
}

/// PKCE S256 challenge for a verifier: `base64url(sha256(verifier))`.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_not_raw() {
        let h = TokenHash::of("abc");
        assert_eq!(h, TokenHash::of("abc"));
        assert_ne!(h, TokenHash::of("abd"));
        assert_eq!(h.as_bytes().len(), 32);
    }

    #[test]
    fn rfc7636_vector() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
