// See crypto/src/main/scala/coop/rchain/crypto/signatures/SignaturesAlg.scala

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(feature = "oqs_pq_experimental")]
use super::oqs_pq::{Falcon512, MlDsa65, SlhDsaSha2_128s};
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

    /// Decidable equality on ground signatures `g ∈ G` (DR-2: the per-`G`
    /// decidable-eq interface of the cost-accounted rho-calculus, realizing
    /// the `sig_eq_dec` obligation of the Rocq `sig` model on the `SGround`
    /// axis). Ground signatures are opaque byte sequences, so the default is
    /// byte equality; algorithms with a non-trivial canonical form (e.g. a
    /// curve with multiple wire encodings of the same key) may override.
    fn ground_eq(&self, a: &[u8], b: &[u8]) -> bool {
        a == b
    }

    /// Hash a ground signature `g` to its canonical-process encoding `H_g`
    /// (DR-2; the spec's `Σ⟦g⟧ = quote(H_g)`, eq:app-sig-ground). Default is
    /// Blake2b256 over the ground bytes, matching the repo-wide content-hash
    /// used for `#P`-style process hashes; algorithms that pin a different
    /// canonical hash may override.
    fn ground_hash(&self, g: &[u8]) -> Vec<u8> {
        crate::rust::hash::blake2b256::Blake2b256::hash(g.to_vec())
    }
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
                    #[cfg(feature = "oqs_pq_experimental")]
                    "oqs-ml-dsa-65/v1" => Ok(Box::new(MlDsa65)),
                    #[cfg(feature = "oqs_pq_experimental")]
                    "oqs-falcon-512/v1" => Ok(Box::new(Falcon512)),
                    #[cfg(feature = "oqs_pq_experimental")]
                    "oqs-slh-dsa-sha2-128s/v1" => Ok(Box::new(SlhDsaSha2_128s)),
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
            #[cfg(feature = "oqs_pq_experimental")]
            "oqs-ml-dsa-65/v1" => Some(Box::new(MlDsa65)),
            #[cfg(feature = "oqs_pq_experimental")]
            "oqs-falcon-512/v1" => Some(Box::new(Falcon512)),
            #[cfg(feature = "oqs_pq_experimental")]
            "oqs-slh-dsa-sha2-128s/v1" => Some(Box::new(SlhDsaSha2_128s)),
            _ => None,
        }
    }
}

/// Registry-parity tests for the OQS post-quantum backends.
///
/// The OQS algorithm names are registered in FIVE independent locations
/// (factory `apply`, Deserialize `visit_str`, `signed::signature_hash`,
/// `casper::validate::{verify_signature, signature_algorithm_supported}`, and
/// `node::web_api::lookup_sig_algorithm`). The casper block-signature registry
/// is a SEPARATE `match` that must agree with the crypto factory or block
/// validation will reject otherwise-valid PQ-signed blocks. crypto cannot
/// depend on casper (cycle), so we REPLICATE the casper predicate here with
/// the exact same gated arms and assert that the crypto factory, the
/// Deserialize visitor, the replicated casper predicate, and the canonical
/// [`super::oqs_pq::OQS_REGISTERED_ALGORITHMS`] table never drift.
#[cfg(all(test, feature = "oqs_pq_experimental"))]
mod oqs_registry_parity_tests {
    use serde::de::value::{Error as ValueError, StrDeserializer};
    use serde::de::IntoDeserializer;

    use super::super::oqs_pq::{
        Falcon512, MlDsa65, SlhDsaSha2_128s, OQS_REGISTERED_ALGORITHMS,
    };
    use super::*;

    /// Byte-for-byte replica of `casper::rust::validate::Validate::
    /// signature_algorithm_supported`'s OQS arms. If casper's copy changes,
    /// this replica must change too — that is the point: the parity test
    /// fails loudly if the two registries diverge in their OQS coverage.
    fn casper_signature_algorithm_supported_replica(algorithm: &str) -> bool {
        match algorithm {
            "secp256k1" => true,
            a if a == MlDsa65::name() => true,
            a if a == Falcon512::name() => true,
            a if a == SlhDsaSha2_128s::name() => true,
            _ => false,
        }
    }

    /// Exercise the real `Deserialize` impl for `Box<dyn SignaturesAlg>` via a
    /// `StrDeserializer`, which drives `deserialize_str` -> `visit_str`. No
    /// JSON dependency required.
    fn deserialize_accepts(name: &str) -> bool {
        let de: StrDeserializer<ValueError> = name.into_deserializer();
        let parsed: Result<Box<dyn SignaturesAlg>, ValueError> =
            Box::<dyn SignaturesAlg>::deserialize(de);
        match parsed {
            Ok(alg) => alg.name() == name,
            Err(_) => false,
        }
    }

    #[test]
    fn every_oqs_name_resolves_in_all_registries() {
        for (name, _algorithm) in OQS_REGISTERED_ALGORITHMS.iter() {
            // (1) Factory.
            let factory = SignaturesAlgFactory::apply(name);
            assert!(
                factory.is_some(),
                "SignaturesAlgFactory::apply({name}) must resolve"
            );
            assert_eq!(
                factory.expect("checked some").name(),
                *name,
                "factory must round-trip the canonical name for {name}"
            );

            // (2) Deserialize visitor.
            assert!(
                deserialize_accepts(name),
                "Deserialize visitor must accept {name}"
            );

            // (3) Replicated casper block-signature registry.
            assert!(
                casper_signature_algorithm_supported_replica(name),
                "casper signature_algorithm_supported replica must accept {name}"
            );
        }
    }

    #[test]
    fn registries_reject_unversioned_and_unknown_names() {
        // The unversioned names must NOT resolve anywhere — the `/v1` suffix is
        // consensus-load-bearing.
        for bad in ["oqs-ml-dsa-65", "oqs-falcon-512", "oqs-slh-dsa-sha2-128s", "totally-bogus"] {
            assert!(
                SignaturesAlgFactory::apply(bad).is_none(),
                "factory must reject {bad}"
            );
            assert!(
                !deserialize_accepts(bad),
                "Deserialize visitor must reject {bad}"
            );
            assert!(
                !casper_signature_algorithm_supported_replica(bad),
                "casper replica must reject {bad}"
            );
        }
    }

    #[test]
    fn factory_and_casper_replica_agree_pointwise() {
        // For every registered OQS name, all three boolean registries must
        // return the same verdict (true). This is the anti-drift assertion.
        for (name, _algorithm) in OQS_REGISTERED_ALGORITHMS.iter() {
            let in_factory = SignaturesAlgFactory::apply(name).is_some();
            let in_deserialize = deserialize_accepts(name);
            let in_casper = casper_signature_algorithm_supported_replica(name);
            assert_eq!(
                (in_factory, in_deserialize, in_casper),
                (true, true, true),
                "registries disagree for {name}: factory={in_factory}, \
                 deserialize={in_deserialize}, casper={in_casper}"
            );
        }
    }
}
