use std::cmp::PartialEq;
use std::hash::{Hash, Hasher};
use k256::ecdsa::{VerifyingKey};
use eyre::{eyre, Result};

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
        let bytes = hex::decode(pubkey_hex)
            .map_err(|e| eyre!("Invalid public key hex: {}", e))?;

        if bytes.len() != 65 || bytes[0] != 0x04 {
            return Err(eyre!("Invalid validator public key"));
        }

        VerifyingKey::from_sec1_bytes(&bytes)
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
