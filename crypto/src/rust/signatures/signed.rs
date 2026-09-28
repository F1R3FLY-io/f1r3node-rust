use prost::Message;

use super::secp256k1_eth::Secp256k1Eth;
use super::signatures_alg::SignaturesAlg;
#[cfg(feature = "schnorr_secp256k1_experimental")]
use super::{frost_secp256k1::FrostSecp256k1, schnorr_secp256k1::SchnorrSecp256k1};
use crate::rust::hash::blake2b256::Blake2b256;
use crate::rust::hash::keccak256::Keccak256;
use crate::rust::private_key::PrivateKey;
use crate::rust::public_key::PublicKey;

pub trait ToMessage {
    type Type: Message;
    fn to_message(&self) -> Self::Type;
}

// See crypto/src/main/scala/coop/rchain/crypto/signatures/Signed.scala
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Signed<A> {
    pub data: A,
    pub pk: PublicKey,
    #[serde(with = "shared::rust::serde_bytes")]
    pub sig: prost::bytes::Bytes,
    pub sig_algorithm: Box<dyn SignaturesAlg>,
}

impl<A: std::fmt::Debug + serde::Serialize + ToMessage> Signed<A> {
    pub fn create(
        data: A,
        sig_algorithm: Box<dyn SignaturesAlg>,
        sk: PrivateKey,
    ) -> Result<Self, String> {
        let serialized_data = data.to_message().encode_to_vec();
        let hash = Signed::<A>::signature_hash(&sig_algorithm.name(), serialized_data);
        let sig = sig_algorithm.sign(&hash, &sk.bytes);

        Ok(Self {
            data,
            pk: sig_algorithm.to_public(&sk),
            sig: prost::bytes::Bytes::from(sig),
            sig_algorithm,
        })
    }

    /// Construct a `Signed` whose signature is deliberately *not* bound to `pk`.
    ///
    /// Unlike [`Signed::create`], which derives `pk` from the signing key, this
    /// Signs `data` with `signing_sk` while carrying a caller-supplied `pk`.
    ///
    /// Standard verification will fail.
    ///
    /// Used for exploratory (read-only) deploys where only a public key is
    /// available. The signature is kept — not omitted — because Rholang exposes
    /// it via `rho:system:deployId` / `rho:rchain:deployId`, and an empty value
    /// would cause cost estimates to diverge from real deploys. Folding `pk`
    /// into the preimage ensures distinct deployers get distinct deployIds.
    ///
    /// **Do not use on any path where signature verification matters.**
    ///
    /// Returns `Result` for API parity with [`Signed::create`]; this path cannot fail.
    pub fn create_unbound(
        data: A,
        pk: PublicKey,
        signing_sk: PrivateKey,
        sig_algorithm: Box<dyn SignaturesAlg>,
    ) -> Result<Self, String> {
        let mut preimage = data.to_message().encode_to_vec();
        preimage.extend_from_slice(&pk.bytes);
        let hash = Signed::<A>::signature_hash(&sig_algorithm.name(), preimage);
        let sig = sig_algorithm.sign(&hash, &signing_sk.bytes);

        Ok(Self {
            data,
            pk,
            sig: prost::bytes::Bytes::from(sig),
            sig_algorithm,
        })
    }

    pub fn from_signed_data(
        data: A,
        pk: PublicKey,
        sig: prost::bytes::Bytes,
        sig_algorithm: Box<dyn SignaturesAlg>,
    ) -> Result<Option<Self>, String> {
        let serialized_data = data.to_message().encode_to_vec();
        let hash = Signed::<A>::signature_hash(&sig_algorithm.name(), serialized_data);

        if sig_algorithm.verify(&hash, &sig, &pk.bytes) {
            Ok(Some(Self {
                data,
                pk,
                sig,
                sig_algorithm,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn signature_hash(sig_alg_name: &str, serialized_data: Vec<u8>) -> Vec<u8> {
        match sig_alg_name {
            name if name == Secp256k1Eth::name() => {
                let prefix = Signed::<A>::eth_prefix(serialized_data.len());
                let mut combined = prefix;
                combined.extend(serialized_data);
                Keccak256::hash(combined)
            }
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            name if name == SchnorrSecp256k1::name() => {
                SchnorrSecp256k1::domain_separated_hash(&serialized_data)
            }
            #[cfg(feature = "schnorr_secp256k1_experimental")]
            name if name == FrostSecp256k1::name() => {
                FrostSecp256k1::domain_separated_hash(&serialized_data)
            }

            _ => Blake2b256::hash(serialized_data),
        }
    }

    fn eth_prefix(msg_length: usize) -> Vec<u8> {
        format!("\u{0019}Ethereum Signed Message:\n{}", msg_length)
            .as_bytes()
            .to_vec()
    }
}

impl<A: PartialEq> PartialEq for Signed<A> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
            && self.pk == other.pk
            && self.sig == other.sig
            && self.sig_algorithm.eq(&other.sig_algorithm)
    }
}

impl<A: Eq> Eq for Signed<A> {}

impl<A: std::hash::Hash> std::hash::Hash for Signed<A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.hash(state);
        self.pk.hash(state);
        self.sig.hash(state);
        self.sig_algorithm.name().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use super::*;
    use crate::rust::signatures::secp256k1::Secp256k1;
    use crate::rust::signatures::signatures_alg::SignaturesAlg;

    #[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, prost::Message)]
    struct TestData {
        #[prost(bytes = "vec", tag = "1")]
        value: Vec<u8>,
    }

    impl ToMessage for TestData {
        type Type = TestData;
        fn to_message(&self) -> Self::Type { self.clone() }
    }

    fn test_data(value: &[u8]) -> TestData {
        TestData {
            value: value.to_vec(),
        }
    }

    fn alg() -> Box<dyn SignaturesAlg> { Box::new(Secp256k1) }

    #[test]
    fn create_derives_pk_from_signing_key_and_verifies() {
        let (sk, pk) = Secp256k1.new_key_pair();
        let signed = Signed::create(test_data(b"hello"), alg(), sk).unwrap();

        assert_eq!(signed.pk, pk);
        assert!(!signed.sig.is_empty());

        let verified = Signed::from_signed_data(
            test_data(b"hello"),
            signed.pk.clone(),
            signed.sig.clone(),
            alg(),
        )
        .unwrap();
        assert_eq!(verified, Some(signed));
    }

    #[test]
    fn from_signed_data_rejects_tampered_signature_and_data() {
        let (sk, _) = Secp256k1.new_key_pair();
        let signed = Signed::create(test_data(b"hello"), alg(), sk).unwrap();

        let mut tampered_sig = signed.sig.to_vec();
        tampered_sig[10] ^= 0xFF;
        let result = Signed::from_signed_data(
            test_data(b"hello"),
            signed.pk.clone(),
            prost::bytes::Bytes::from(tampered_sig),
            alg(),
        )
        .unwrap();
        assert_eq!(result, None);

        let result = Signed::from_signed_data(
            test_data(b"tampered"),
            signed.pk.clone(),
            signed.sig.clone(),
            alg(),
        )
        .unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn create_unbound_carries_foreign_pk_and_fails_standard_verification() {
        let (signing_sk, _) = Secp256k1.new_key_pair();
        let (_, foreign_pk) = Secp256k1.new_key_pair();

        let unbound = Signed::create_unbound(
            test_data(b"exploratory"),
            foreign_pk.clone(),
            signing_sk,
            alg(),
        )
        .unwrap();

        assert_eq!(unbound.pk, foreign_pk);
        assert!(!unbound.sig.is_empty());

        let verified = Signed::from_signed_data(
            test_data(b"exploratory"),
            unbound.pk.clone(),
            unbound.sig.clone(),
            alg(),
        )
        .unwrap();
        assert_eq!(verified, None);
    }

    #[test]
    fn create_unbound_gives_distinct_signatures_for_distinct_pks() {
        let (signing_sk, _) = Secp256k1.new_key_pair();
        let (_, pk_a) = Secp256k1.new_key_pair();
        let (_, pk_b) = Secp256k1.new_key_pair();

        let sig_a = Signed::create_unbound(
            test_data(b"same data"),
            pk_a.clone(),
            signing_sk.clone(),
            alg(),
        )
        .unwrap()
        .sig;
        let sig_a_again =
            Signed::create_unbound(test_data(b"same data"), pk_a, signing_sk.clone(), alg())
                .unwrap()
                .sig;
        let sig_b = Signed::create_unbound(test_data(b"same data"), pk_b, signing_sk, alg())
            .unwrap()
            .sig;

        assert_eq!(sig_a, sig_a_again);
        assert_ne!(sig_a, sig_b);
    }

    #[test]
    fn signature_hash_uses_blake2b256_by_default() {
        let data = b"some serialized data".to_vec();
        let hash = Signed::<TestData>::signature_hash("secp256k1", data.clone());
        assert_eq!(hash, crate::rust::hash::blake2b256::Blake2b256::hash(data));
    }

    #[test]
    fn signature_hash_uses_eth_prefixed_keccak_for_eth_alg() {
        let data = b"eth data".to_vec();
        let hash = Signed::<TestData>::signature_hash(&super::Secp256k1Eth::name(), data.clone());

        let mut prefixed = format!("\u{0019}Ethereum Signed Message:\n{}", data.len())
            .as_bytes()
            .to_vec();
        prefixed.extend(data);
        assert_eq!(
            hash,
            crate::rust::hash::keccak256::Keccak256::hash(prefixed)
        );
    }

    #[test]
    fn equal_signed_values_have_equal_hashes() {
        let (sk, _) = Secp256k1.new_key_pair();
        let a = Signed::create(test_data(b"hash me"), alg(), sk.clone()).unwrap();
        let b = Signed::create(test_data(b"hash me"), alg(), sk.clone()).unwrap();
        let c = Signed::create(test_data(b"different"), alg(), sk).unwrap();

        assert_eq!(a, b);
        assert_ne!(a, c);

        fn hash_of(signed: &Signed<TestData>) -> u64 {
            let mut hasher = DefaultHasher::new();
            signed.hash(&mut hasher);
            hasher.finish()
        }
        assert_eq!(hash_of(&a), hash_of(&b));

        let cloned = a.clone();
        assert_eq!(a, cloned);
    }
}
