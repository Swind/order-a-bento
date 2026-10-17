//! Small in-memory token-bucket rate limiters (keyed by client IP or any key).
//!
//! One process, one SQLite file: an in-memory limiter is enough (no shared
//! store needed). IPv6 clients are bucketed by their /64 prefix.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default burst for the public endpoints.
pub const DEFAULT_BURST: u32 = 30;
/// Default refill rate (requests per second) for the public endpoints.
pub const DEFAULT_PER_SEC: f64 = 2.0;
/// Above this many tracked clients, idle (fully refilled) buckets are dropped.
const PRUNE_THRESHOLD: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimitConfig {
    pub burst: u32,
    pub per_sec: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            burst: DEFAULT_BURST,
            per_sec: DEFAULT_PER_SEC,
        }
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

/// Token buckets keyed by `K` (client IP for public endpoints, organizer id
/// for AI imports). Each key starts with a full bucket of `burst` tokens.
pub struct KeyedLimiter<K> {
    cfg: RateLimitConfig,
    buckets: Mutex<HashMap<K, Bucket>>,
}

impl<K: Eq + Hash> KeyedLimiter<K> {
    pub fn new(cfg: RateLimitConfig) -> Self {
        Self {
            cfg,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> RateLimitConfig {
        self.cfg
    }

    /// Take one token. `Err(retry_after)` when the key is over its limit.
    pub fn check(&self, key: K, now: Instant) -> Result<(), Duration> {
        let burst = f64::from(self.cfg.burst.max(1));
        let rate = self.cfg.per_sec.max(f64::MIN_POSITIVE);
        let mut map = self.buckets.lock().expect("rate limiter poisoned");
        if map.len() > PRUNE_THRESHOLD {
            map.retain(|_, b| {
                b.tokens + now.saturating_duration_since(b.last).as_secs_f64() * rate < burst
            });
        }
        let b = map.entry(key).or_insert(Bucket {
            tokens: burst,
            last: now,
        });
        let elapsed = now.saturating_duration_since(b.last).as_secs_f64();
        b.tokens = (b.tokens + elapsed * rate).min(burst);
        b.last = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - b.tokens) / rate;
            Err(Duration::from_secs_f64(wait.min(3600.0)))
        }
    }
}

/// Per-client-IP limiter for the public endpoints.
pub struct RateLimiter(KeyedLimiter<Option<IpAddr>>);

/// Bucket key: IPv4 as is, IPv6 reduced to its /64 prefix.
fn key_of(ip: Option<IpAddr>) -> Option<IpAddr> {
    match ip {
        Some(IpAddr::V6(v6)) => {
            let mut o = v6.octets();
            o[8..].fill(0);
            Some(IpAddr::V6(o.into()))
        }
        other => other,
    }
}

impl RateLimiter {
    pub fn new(cfg: RateLimitConfig) -> Self {
        Self(KeyedLimiter::new(cfg))
    }

    pub fn config(&self) -> RateLimitConfig {
        self.0.config()
    }

    /// Take one token. `Err(retry_after)` when the client is over its limit.
    /// Requests without a known peer address share one bucket.
    pub fn check(&self, ip: Option<IpAddr>, now: Instant) -> Result<(), Duration> {
        self.0.check(key_of(ip), now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn burst_then_refill() {
        let l = RateLimiter::new(RateLimitConfig {
            burst: 3,
            per_sec: 2.0,
        });
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(l.check(ip("1.1.1.1"), t0).is_ok());
        }
        let wait = l.check(ip("1.1.1.1"), t0).unwrap_err();
        assert!(wait <= Duration::from_millis(500) && wait > Duration::ZERO);
        // Other clients are independent.
        assert!(l.check(ip("2.2.2.2"), t0).is_ok());
        // Half a second refills one token.
        assert!(
            l.check(ip("1.1.1.1"), t0 + Duration::from_millis(500))
                .is_ok()
        );
        assert!(
            l.check(ip("1.1.1.1"), t0 + Duration::from_millis(500))
                .is_err()
        );
        // Never exceeds the burst.
        let later = t0 + Duration::from_secs(60);
        for _ in 0..3 {
            assert!(l.check(ip("1.1.1.1"), later).is_ok());
        }
        assert!(l.check(ip("1.1.1.1"), later).is_err());
    }

    #[test]
    fn ipv6_shares_a_64_bucket_and_unknown_peer_has_one() {
        let l = RateLimiter::new(RateLimitConfig {
            burst: 1,
            per_sec: 0.001,
        });
        let t = Instant::now();
        assert!(l.check(ip("2001:db8::1"), t).is_ok());
        assert!(l.check(ip("2001:db8::ffff"), t).is_err());
        assert!(l.check(ip("2001:db9::1"), t).is_ok());
        assert!(l.check(None, t).is_ok());
        assert!(l.check(None, t).is_err());
    }
}
