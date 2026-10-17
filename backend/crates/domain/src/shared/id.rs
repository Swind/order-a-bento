use std::{cmp::Ordering, fmt, hash::Hash, hash::Hasher, marker::PhantomData, str::FromStr};
use uuid::Uuid;

use super::DomainError;

/// Typed UUIDv7 identifier.
pub struct Id<T>(Uuid, PhantomData<fn() -> T>);

impl<T> Id<T> {
    pub fn new() -> Self {
        Self(Uuid::now_v7(), PhantomData)
    }
    pub fn from_uuid(u: Uuid) -> Self {
        Self(u, PhantomData)
    }
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl<T> Default for Id<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<T> Eq for Id<T> {}
impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.0.hash(h)
    }
}
impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<T> Ord for Id<T> {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0.cmp(&o.0)
    }
}
impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
    }
}
impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl<T> FromStr for Id<T> {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self::from_uuid)
            .map_err(|_| DomainError::invalid("id", "not a valid UUID"))
    }
}

/// Serialized as the canonical UUID string (used by the AI import proposal JSON).
impl<T> serde::Serialize for Id<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}
impl<'de, T> serde::Deserialize<'de> for Id<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str> as serde::Deserialize>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct M;

    #[test]
    fn roundtrip_and_order() {
        let a = Id::<M>::new();
        let b = Id::<M>::new();
        assert!(a < b);
        assert_eq!(a.to_string().parse::<Id<M>>().unwrap(), a);
        assert!("nope".parse::<Id<M>>().is_err());
        assert_eq!(Id::<M>::from_uuid(a.as_uuid()), a);
    }
}
