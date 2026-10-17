use std::fmt;

use crate::shared::DomainError;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Unguessable share-link token: 32 random bytes as unpadded base64url
/// (43 chars). Randomness is supplied by the caller; the domain does no IO.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ShareToken(String);

impl ShareToken {
    pub const LEN: usize = 43;

    pub fn from_random_bytes(bytes: [u8; 32]) -> Self {
        let mut out = String::with_capacity(Self::LEN);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            let chars = chunk.len() + 1; // 1->2, 2->3, 3->4
            for i in 0..chars {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
        }
        Self(out)
    }

    pub fn parse(s: &str) -> Result<Self, DomainError> {
        if s.len() != Self::LEN || !s.bytes().all(|b| ALPHABET.contains(&b)) {
            return Err(DomainError::invalid("shareToken", "格式不正確"));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Tokens are secrets: never print them in debug output.
impl fmt::Debug for ShareToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ShareToken(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_43_chars_and_roundtrips() {
        let t = ShareToken::from_random_bytes([0xff; 32]);
        assert_eq!(t.as_str().len(), 43);
        assert!(ShareToken::parse(t.as_str()).is_ok());
        let t0 = ShareToken::from_random_bytes([0; 32]);
        assert_eq!(t0.as_str(), "A".repeat(43));
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as u8 * 7;
        }
        let t = ShareToken::from_random_bytes(b);
        assert_eq!(ShareToken::parse(t.as_str()).unwrap(), t);
    }

    #[test]
    fn parse_rules() {
        let ok = "A".repeat(43);
        assert!(ShareToken::parse(&ok).is_ok());
        assert!(ShareToken::parse(&"A".repeat(42)).is_err());
        assert!(ShareToken::parse(&"A".repeat(44)).is_err());
        for bad in ['+', '/', '=', ' ', '字'] {
            let s = format!("{}{bad}", "A".repeat(42));
            assert!(ShareToken::parse(&s).is_err(), "{bad}");
            let s = format!("{bad}{}", "A".repeat(42));
            assert!(ShareToken::parse(&s).is_err(), "{bad}");
        }
        assert!(ShareToken::parse(&format!("{}-", "A".repeat(41) + "_")).is_ok());
    }
}
