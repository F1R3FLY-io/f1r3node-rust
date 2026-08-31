// See crypto/src/main/scala/coop/rchain/crypto/signatures/SignaturesAlg.scala

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::secp256k1::Secp256k1;
use super::secp256k1_eth::Secp256k1Eth;
#[cfg(feature = "schnorr_secp256k1_experimental")]
use super::{frost_secp256k1::FrostSecp256k1, schnorr_secp256k1::SchnorrSecp256k1};
use crate::rust::private_key::PrivateKey;
use crate::rust::public_key::PublicKey;

pub trait SignaturesAlg: std::fmt::Debug + Send + Sync {
    fn verify(&self, data: &[u8], signature: &[u8], pub_key: &[u8]) -> bool;

    fn sign(&self, data: &[u8], sec: &[u8]) -> Vec<u8>;

    fn to_public(&self, sec: &PrivateKey) -> PublicKey;

    fn new_key_pair(&self) -> (PrivateKey, PublicKey);

    fn name(&self) -> String;

    fn verify_with_public_key(&self, data: &[u8], signature: &[u8], pub_key: &PublicKey) -> bool {
        self.verify(data, signature, &pub_key.bytes)
    }

    fn sign_with_private_key(&self, data: &[u8], sec: &PrivateKey) -> Vec<u8> {
        self.sign(data, &sec.bytes)
    }

    fn sig_length(&self) -> usize;

    fn eq(&self, other: &dyn SignaturesAlg) -> bool;

    fn box_clone(&self) -> Box<dyn SignaturesAlg>;
}

impl Clone for Box<dyn SignaturesAlg> {
    fn clone(&self) -> Self { self.box_clone() }
}

impl PartialEq for Box<dyn SignaturesAlg> {
    fn eq(&self, other: &Self) -> bool { self.name() == other.name() }
}

impl Serialize for Box<dyn SignaturesAlg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_str(&self.name())
    }
}

impl<'de> Deserialize<'de> for Box<dyn SignaturesAlg> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        struct SignaturesAlgVisitor;

        impl<'de> Visitor<'de> for SignaturesAlgVisitor {
            type Value = Box<dyn SignaturesAlg>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a known signature algorithm name")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where E: de::Error {
                match value {
                    "secp256k1" => Ok(Box::new(Secp256k1)),
                    "secp256k1-eth" => Ok(Box::new(Secp256k1Eth)),
                    #[cfg(feature = "schnorr_secp256k1_experimental")]
                    "schnorr-secp256k1" => Ok(Box::new(SchnorrSecp256k1)),
                    #[cfg(feature = "schnorr_secp256k1_experimental")]
                    "frost-secp256k1" => Ok(Box::new(FrostSecp256k1)),
                    // "ed25519" => Ok(Box::new(Ed25519)),
                    _ => Err(de::Error::custom(format!("Unknown algorithm: {}", value))),
                }
            }
        }

        deserializer.deserialize_str(SignaturesAlgVisitor)
    }
}

pub struct SignaturesAlgFactory;

impl SignaturesAlgFactory {
    pub fn apply(name: &str) -> Option<Box<dyn SignaturesAlg>> {
        match name {
            // ed25519 signature algorithm is disabled
            // TODO: quick way to prevent use of ed25519 to sign deploys - OLD
            // https://rchain.atlassian.net/browse/RCHAIN-3560
            // case Ed25519.name => Some(Ed25519)
            "secp256k1" => Some(Box::new(Secp256k1)),
            "secp256k1-eth" => Some(Box::new(Secp256k1Eth)),
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            "schnorr-secp256k1" => Some(Box::new(SchnorrSecp256k1)),
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            "frost-secp256k1" => Some(Box::new(FrostSecp256k1)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_returns_known_algorithms() {
        let alg = SignaturesAlgFactory::apply("secp256k1").unwrap();
        assert_eq!(alg.name(), "secp256k1");

        let eth = SignaturesAlgFactory::apply("secp256k1-eth").unwrap();
        assert_eq!(eth.name(), "secp256k1:eth");
    }

    #[test]
    fn factory_returns_none_for_unknown_or_disabled_algorithms() {
        assert!(SignaturesAlgFactory::apply("ed25519").is_none());
        assert!(SignaturesAlgFactory::apply("").is_none());
        assert!(SignaturesAlgFactory::apply("rsa").is_none());
    }

    #[test]
    fn boxed_alg_clone_and_eq_use_name() {
        let alg: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let cloned = alg.clone();
        let eth: Box<dyn SignaturesAlg> = Box::new(Secp256k1Eth);

        assert!(<Box<dyn SignaturesAlg> as PartialEq>::eq(&alg, &cloned));
        assert!(!<Box<dyn SignaturesAlg> as PartialEq>::eq(&alg, &eth));
        assert!(alg.as_ref().eq(cloned.as_ref()));
        assert!(!alg.as_ref().eq(eth.as_ref()));
    }

    #[test]
    fn default_trait_methods_delegate_to_raw_byte_versions() {
        let alg: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let (sk, pk) = alg.new_key_pair();
        let data = crate::rust::hash::blake2b256::Blake2b256::hash(b"payload".to_vec());

        let sig = alg.sign_with_private_key(&data, &sk);
        assert_eq!(sig, alg.sign(&data, &sk.bytes));
        assert!(alg.verify_with_public_key(&data, &sig, &pk));
        assert!(alg.verify(&data, &sig, &pk.bytes));

        let other_data = crate::rust::hash::blake2b256::Blake2b256::hash(b"other".to_vec());
        assert!(!alg.verify_with_public_key(&other_data, &sig, &pk));
    }

    #[test]
    fn serde_roundtrip_preserves_algorithm() {
        let alg: Box<dyn SignaturesAlg> = Box::new(Secp256k1);
        let encoded = bincode::serialize(&alg).unwrap();
        let decoded: Box<dyn SignaturesAlg> = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded.name(), "secp256k1");
    }

    #[test]
    fn deserialize_rejects_unknown_algorithm_name() {
        let encoded = bincode::serialize("no-such-alg").unwrap();
        let result: Result<Box<dyn SignaturesAlg>, _> = bincode::deserialize(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_accepts_eth_alias() {
        let encoded = bincode::serialize("secp256k1-eth").unwrap();
        let decoded: Box<dyn SignaturesAlg> = bincode::deserialize(&encoded).unwrap();
        assert_eq!(decoded.name(), "secp256k1:eth");
    }
}
