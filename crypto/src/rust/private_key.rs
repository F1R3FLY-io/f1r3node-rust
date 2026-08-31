// See crypto/src/main/scala/coop/rchain/crypto/PrivateKey.scala

use std::cmp::PartialEq;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Eq)]
pub struct PrivateKey {
    pub bytes: prost::bytes::Bytes,
}

impl PrivateKey {
    pub fn new(bytes: prost::bytes::Bytes) -> Self { PrivateKey { bytes } }

    pub fn from_bytes(bs: &[u8]) -> Self { PrivateKey::new(bs.to_vec().into()) }
}

impl PartialEq for PrivateKey {
    fn eq(&self, other: &Self) -> bool { self.bytes == other.bytes }
}

impl Hash for PrivateKey {
    fn hash<H: Hasher>(&self, state: &mut H) { self.bytes.hash(state); }
}

#[cfg(test)]
mod tests {
    use std::hash::DefaultHasher;

    use super::*;

    fn hash_of(key: &PrivateKey) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn new_and_from_bytes_produce_equal_keys() {
        let a = PrivateKey::new(prost::bytes::Bytes::from_static(&[1, 2, 3]));
        let b = PrivateKey::from_bytes(&[1, 2, 3]);
        assert_eq!(a, b);
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn keys_with_different_bytes_are_not_equal() {
        let a = PrivateKey::from_bytes(&[1, 2, 3]);
        let b = PrivateKey::from_bytes(&[1, 2, 4]);
        assert_ne!(a, b);
    }
}
