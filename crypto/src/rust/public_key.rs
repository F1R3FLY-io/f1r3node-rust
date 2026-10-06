use std::cmp::PartialEq;
use std::hash::{Hash, Hasher};

use eyre::{eyre, Result};
use k256::ecdsa::VerifyingKey;

// See crypto/src/main/scala/coop/rchain/crypto/PublicKey.scala
#[derive(Debug, Clone, Eq, serde::Serialize, serde::Deserialize)]
pub struct PublicKey {
    #[serde(with = "shared::rust::serde_bytes")]
    pub bytes: prost::bytes::Bytes,
}

impl PublicKey {
    pub fn new(bytes: prost::bytes::Bytes) -> Self { PublicKey { bytes } }
    pub fn from_bytes(bs: &[u8]) -> Self { PublicKey::new(bs.to_vec().into()) }

    pub fn validate_secp256k1_hex(pubkey_hex: &str) -> Result<()> {
        let bytes = hex::decode(pubkey_hex).map_err(|e| eyre!("Invalid public key hex: {}", e))?;
        Self::validate_secp256k1_bytes(&bytes)
    }

    pub fn validate_secp256k1_bytes(bytes: &[u8]) -> Result<()> {
        if bytes.len() != 65 || bytes[0] != 0x04 {
            return Err(eyre!(
                "public key must be a 65-byte uncompressed secp256k1 key (0x04-prefixed), got {} bytes",
                bytes.len()
            ));
        }

        VerifyingKey::from_sec1_bytes(bytes)
            .map_err(|e| eyre!("Public key is not a valid secp256k1 point: {}", e))?;

        Ok(())
    }
}

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool { self.bytes == other.bytes }
}

impl Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) { self.bytes.hash(state); }
}

#[cfg(test)]
mod tests {
    use std::hash::DefaultHasher;

    use super::*;

    const VALID_PUBKEY_HEX: &str = "0418a6b57c4aeee6c7e19e3ea25aa5bae270eca8580ee5e59c28921df743e416a316c55ed10c63b99a7c2705de0e0d3c52ad7f06144b7f6ed97d3a63b871ced6ff";

    fn hash_of(key: &PublicKey) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn new_and_from_bytes_produce_equal_keys() {
        let a = PublicKey::new(prost::bytes::Bytes::from_static(&[4, 5, 6]));
        let b = PublicKey::from_bytes(&[4, 5, 6]);
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(a, PublicKey::from_bytes(&[4, 5, 7]));
    }

    #[test]
    fn validate_secp256k1_hex_accepts_valid_uncompressed_key() {
        assert!(PublicKey::validate_secp256k1_hex(VALID_PUBKEY_HEX).is_ok());
    }

    #[test]
    fn validate_secp256k1_hex_rejects_invalid_hex() {
        let err = PublicKey::validate_secp256k1_hex("zznothex").unwrap_err();
        assert!(err.to_string().contains("Invalid public key hex"));
    }

    #[test]
    fn validate_secp256k1_bytes_rejects_wrong_length() {
        let err = PublicKey::validate_secp256k1_bytes(&[0x04; 64]).unwrap_err();
        assert!(err.to_string().contains("65-byte"));
    }

    #[test]
    fn validate_secp256k1_bytes_rejects_wrong_prefix() {
        let mut bytes = hex::decode(VALID_PUBKEY_HEX).unwrap();
        bytes[0] = 0x05;
        assert!(PublicKey::validate_secp256k1_bytes(&bytes).is_err());
    }

    #[test]
    fn validate_secp256k1_bytes_rejects_point_not_on_curve() {
        let mut bytes = vec![0x04];
        bytes.extend_from_slice(&[0xFF; 64]);
        let err = PublicKey::validate_secp256k1_bytes(&bytes).unwrap_err();
        assert!(err.to_string().contains("not a valid secp256k1 point"));
    }
}
