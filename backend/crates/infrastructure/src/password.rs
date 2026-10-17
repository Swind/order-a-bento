//! Argon2id password hashing (`PasswordHasher` port).
//!
//! Default `argon2` parameters (Argon2id v19, m = 19 MiB, t = 2, p = 1); the
//! PHC string stores them, so they can change later without invalidating old
//! hashes. All work runs in `spawn_blocking`.

use std::sync::{Arc, OnceLock};

use application::ports::{PasswordHasher, PortError, PortResult};
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher as _, PasswordVerifier as _};
use async_trait::async_trait;
use rand::TryRngCore;
use rand::rngs::OsRng;

#[derive(Clone, Default)]
pub struct Argon2PasswordHasher {
    /// Hash of a random string, computed on first use, for equalising timing.
    dummy: Arc<OnceLock<String>>,
}

impl Argon2PasswordHasher {
    pub fn new() -> Self {
        Self::default()
    }
}

fn hash_blocking(password: &str) -> PortResult<String> {
    let mut salt = [0u8; 16];
    OsRng.try_fill_bytes(&mut salt).map_err(PortError::new)?;
    let salt = SaltString::encode_b64(&salt).map_err(PortError::new)?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(PortError::new)
}

fn verify_blocking(password: &str, hash: &str) -> PortResult<bool> {
    let parsed = PasswordHash::new(hash).map_err(PortError::new)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(PortError::new(e)),
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> PortResult<T> + Send + 'static,
) -> PortResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(PortError::new)?
}

#[async_trait]
impl PasswordHasher for Argon2PasswordHasher {
    async fn hash(&self, password: &str) -> PortResult<String> {
        let password = password.to_owned();
        blocking(move || hash_blocking(&password)).await
    }

    async fn verify(&self, password: &str, hash: &str) -> PortResult<bool> {
        let (password, hash) = (password.to_owned(), hash.to_owned());
        blocking(move || verify_blocking(&password, &hash)).await
    }

    async fn verify_dummy(&self, password: &str) {
        let password = password.to_owned();
        let dummy = self.dummy.clone();
        let _ = blocking(move || {
            let hash = match dummy.get() {
                Some(h) => h,
                None => {
                    let mut random = [0u8; 24];
                    OsRng.try_fill_bytes(&mut random).map_err(PortError::new)?;
                    let h = hash_blocking(&format!("{random:?}"))?;
                    dummy.get_or_init(|| h)
                }
            };
            verify_blocking(&password, hash)
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_and_verify_roundtrip() {
        let h = Argon2PasswordHasher::new();
        let hash = h.hash("correct horse battery").await.unwrap();
        assert!(
            hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{hash}"
        );
        assert!(h.verify("correct horse battery", &hash).await.unwrap());
        assert!(!h.verify("wrong", &hash).await.unwrap());
        // Fresh salt per hash.
        assert_ne!(hash, h.hash("correct horse battery").await.unwrap());
        // A corrupt stored hash is an error, not "wrong password".
        assert!(h.verify("x", "not-a-phc-string").await.is_err());
        h.verify_dummy("anything").await;
        h.verify_dummy("anything").await;
        assert!(h.dummy.get().is_some());
    }
}
