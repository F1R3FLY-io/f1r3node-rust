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

/// Error returned by [`Cosigned::from_signed_data`] when the input fails
/// any of the multi-signature envelope invariants (canonical pk ordering,
/// uniqueness, non-negative shares summing to phlo_limit, per-signer
/// verification against the canonical message hash).
#[derive(Debug, thiserror::Error)]
pub enum CosignedError {
    #[error("signer at index {index} (pk={pk_hex}) failed signature verification")]
    SignatureVerifyFailed { index: usize, pk_hex: String },
    #[error("duplicate signer pk: {pk_hex}")]
    DuplicateSigner { pk_hex: String },
    #[error(
        "Σ phlo_share ({sum}) does not equal phlo_limit ({expected})"
    )]
    PhloShareMismatch { sum: i64, expected: i64 },
    #[error("empty signer list — a Cosigned envelope requires at least one signer")]
    EmptySignerList,
    #[error("negative phlo_share at index {index}: {share}")]
    NegativePhloShare { index: usize, share: i64 },
    #[error("phlo share arithmetic overflow when summing signer contributions")]
    PhloShareOverflow,
}

/// One signer in a multi-signature deploy envelope. Sorted ascending by
/// `pk.bytes` inside a [`Cosigned`] (enforced at construction). Each
/// cosigner signs the same canonical message hash as the primary.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cosigner {
    pub pk: PublicKey,
    #[serde(with = "shared::rust::serde_bytes")]
    pub sig: prost::bytes::Bytes,
    pub sig_algorithm: Box<dyn SignaturesAlg>,
    pub phlo_share: i64,
}

impl PartialEq for Cosigner {
    fn eq(&self, other: &Self) -> bool {
        self.pk == other.pk
            && self.sig == other.sig
            && self.sig_algorithm.eq(&other.sig_algorithm)
            && self.phlo_share == other.phlo_share
    }
}

impl Eq for Cosigner {}

impl std::hash::Hash for Cosigner {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pk.hash(state);
        self.sig.hash(state);
        self.sig_algorithm.name().hash(state);
        self.phlo_share.hash(state);
    }
}

/// Multi-signature deploy envelope. Generalizes [`Signed<A>`] to carry an
/// ordered, deduplicated list of cosigners (one or more), each contributing
/// a share of the total `phlo_limit`. A length-1 `Cosigned` is observably
/// equivalent to a legacy `Signed`.
///
/// Invariants enforced at construction by [`Cosigned::from_signed_data`]:
/// 1. `signers` is non-empty.
/// 2. `signers` is sorted ascending by `pk.bytes`; no duplicates.
/// 3. Every `signers[i].sig` verifies against the canonical
///    [`Signed::<A>::signature_hash`] of the encoded `data`.
/// 4. Every `signers[i].phlo_share` is non-negative.
/// 5. The sum of `signers[i].phlo_share` equals `phlo_limit` (the caller
///    supplies `phlo_limit` because `Cosigned` is generic over `A` and
///    cannot extract the field directly).
///
/// These invariants are the multi-signature analogue of `Signed<A>`'s
/// single-signature verification, and they realize the operational
/// semantics of the cost-accounted rho-calculus paper's `σ₁ & σ₂`
/// compound-signature operator (`publications/cost-accounting/cost-accounted-rho.tex`,
/// §3.2 Rules 2-5).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cosigned<A> {
    pub data: A,
    signers: Vec<Cosigner>,
}

impl<A: PartialEq> PartialEq for Cosigned<A> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data && self.signers == other.signers
    }
}

impl<A: Eq> Eq for Cosigned<A> {}

impl<A: std::hash::Hash> std::hash::Hash for Cosigned<A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.hash(state);
        for signer in &self.signers {
            signer.hash(state);
        }
    }
}

impl<A: std::fmt::Debug + serde::Serialize + ToMessage> Cosigned<A> {
    /// Construct and validate a multi-signature envelope.
    ///
    /// `phlo_limit` MUST equal the deploy's `phlo_limit` field (this method
    /// cannot extract it from `A` directly since `A` is generic). The
    /// constructor enforces all five invariants listed in the [`Cosigned`]
    /// type documentation. Returns:
    /// - `Ok(Some(Cosigned))` if every invariant holds.
    /// - `Err(CosignedError)` if any invariant is violated.
    ///
    /// The constructor canonicalizes the signer order by sorting ascending
    /// on `pk.bytes`; callers do not need to pre-sort. Duplicate `pk`s are
    /// rejected (a deploy must not list the same signer twice).
    pub fn from_signed_data(
        data: A,
        signers: Vec<Cosigner>,
        phlo_limit: i64,
    ) -> Result<Self, CosignedError> {
        if signers.is_empty() {
            return Err(CosignedError::EmptySignerList);
        }

        // Canonicalize order by pk.bytes ascending. Stable sort preserves
        // input order within equal-key groups, which `dedup` then catches.
        let mut canonical = signers;
        canonical.sort_by(|a, b| a.pk.bytes.as_ref().cmp(b.pk.bytes.as_ref()));

        // Reject duplicate signers.
        for window in canonical.windows(2) {
            if window[0].pk.bytes == window[1].pk.bytes {
                return Err(CosignedError::DuplicateSigner {
                    pk_hex: hex::encode(&window[0].pk.bytes),
                });
            }
        }

        // Validate non-negative shares and compute sum with overflow check.
        let mut share_sum: i64 = 0;
        for (i, signer) in canonical.iter().enumerate() {
            if signer.phlo_share < 0 {
                return Err(CosignedError::NegativePhloShare {
                    index: i,
                    share: signer.phlo_share,
                });
            }
            share_sum = share_sum
                .checked_add(signer.phlo_share)
                .ok_or(CosignedError::PhloShareOverflow)?;
        }
        if share_sum != phlo_limit {
            return Err(CosignedError::PhloShareMismatch {
                sum: share_sum,
                expected: phlo_limit,
            });
        }

        // Verify each signer against the canonical message hash. Each
        // signer's algorithm dictates the hash function (Blake2b256 for
        // most; Keccak256 with Ethereum prefix for secp256k1-eth; etc.).
        let serialized_data = data.to_message().encode_to_vec();
        for (i, signer) in canonical.iter().enumerate() {
            let hash = Signed::<A>::signature_hash(
                &signer.sig_algorithm.name(),
                serialized_data.clone(),
            );
            if !signer
                .sig_algorithm
                .verify(&hash, &signer.sig, &signer.pk.bytes)
            {
                return Err(CosignedError::SignatureVerifyFailed {
                    index: i,
                    pk_hex: hex::encode(&signer.pk.bytes),
                });
            }
        }

        Ok(Cosigned {
            data,
            signers: canonical,
        })
    }

    /// Construct a single-signer Cosigned envelope from an already-validated
    /// [`Signed<A>`]. This is the legacy-uplift path: callers decoding a
    /// `cosigners.is_empty()` wire deploy can use this to obtain a
    /// `Cosigned<A>` whose lone signer pays the entire `phlo_limit`.
    ///
    /// `phlo_limit` MUST equal the deploy's `phlo_limit` field. No
    /// re-verification occurs (the `Signed<A>` was already verified at
    /// construction); we simply construct the one-element envelope and
    /// validate the share-sum invariant.
    pub fn from_single_signer(signed: Signed<A>, phlo_limit: i64) -> Result<Self, CosignedError> {
        if phlo_limit < 0 {
            return Err(CosignedError::NegativePhloShare {
                index: 0,
                share: phlo_limit,
            });
        }
        let signer = Cosigner {
            pk: signed.pk,
            sig: signed.sig,
            sig_algorithm: signed.sig_algorithm,
            phlo_share: phlo_limit,
        };
        Ok(Cosigned {
            data: signed.data,
            signers: vec![signer],
        })
    }

    /// All signers, in canonical ascending `pk.bytes` order. Always non-empty.
    pub fn signers(&self) -> &[Cosigner] { &self.signers }

    /// The primary signer (`signers[0]`). Equivalent to the legacy
    /// single-signer `Signed<A>`'s sole signer.
    pub fn primary(&self) -> &Cosigner { &self.signers[0] }

    /// `true` if more than one signer is present (i.e., a true multi-sig).
    pub fn is_compound(&self) -> bool { self.signers.len() > 1 }

    /// Sum of all signers' `phlo_share`s. Equals the deploy's `phlo_limit`
    /// by construction (enforced at [`Cosigned::from_signed_data`]).
    pub fn total_phlo_share(&self) -> i64 {
        self.signers
            .iter()
            .map(|s| s.phlo_share)
            .fold(0_i64, i64::saturating_add)
    }

    /// Reconstitute the primary signer as a legacy [`Signed<A>`] value,
    /// consuming the envelope. Used at storage / API boundaries where
    /// `Signed<A>` is the shape (e.g. `ProcessedDeploy.deploy: Signed<DeployData>`).
    ///
    /// "Unchecked" because no re-verification occurs — the per-signer
    /// signature was already verified at [`Cosigned::from_signed_data`]
    /// construction. The returned `Signed<A>` carries the primary signer's
    /// pk, sig, and sig_algorithm (matching the legacy single-sig wire shape).
    /// For multi-sig envelopes, additional cosigners ARE LOST by this
    /// conversion — callers needing them must use the original `Cosigned<A>`.
    pub fn into_legacy_signed_unchecked(self) -> Signed<A> {
        let primary = self
            .signers
            .into_iter()
            .next()
            .expect("Cosigned invariant: signers is non-empty");
        Signed {
            data: self.data,
            pk: primary.pk,
            sig: primary.sig,
            sig_algorithm: primary.sig_algorithm,
        }
    }
}

impl<A: Clone> Cosigned<A> {
    /// Borrow-only legacy view of this envelope's primary signer, producing
    /// a [`Signed<A>`] by cloning. Used by code paths (e.g., legacy seed
    /// derivation in `generate_pre_charge_deploy_random_seed`) that take
    /// `&Signed<A>` and need the legacy single-sig wire shape WITHOUT
    /// consuming the `Cosigned<A>` envelope.
    ///
    /// For single-signer cosigned (the legacy uplift case from
    /// `from_single_signer`), this borrow-then-clone is the right back-compat
    /// path. For multi-signer cosigned this still returns the primary's
    /// view; additional cosigners are not visible through the returned
    /// `Signed<A>`. Callers needing the full set must use the source
    /// `Cosigned<A>` directly.
    pub fn as_legacy_signed_ref(&self) -> Signed<A> {
        let primary = &self.signers[0];
        Signed {
            data: self.data.clone(),
            pk: primary.pk.clone(),
            sig: primary.sig.clone(),
            sig_algorithm: primary.sig_algorithm.clone(),
        }
    }
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
mod cosigned_tests {
    use super::*;
    use crate::rust::signatures::secp256k1::Secp256k1;

    #[derive(Clone, PartialEq, prost::Message, serde::Serialize, serde::Deserialize)]
    struct TestPayload {
        #[prost(string, tag = "1")]
        pub term: String,
        #[prost(int64, tag = "2")]
        pub phlo_limit: i64,
    }

    impl ToMessage for TestPayload {
        type Type = TestPayload;
        fn to_message(&self) -> Self::Type { self.clone() }
    }

    fn fresh_cosigner(payload: &TestPayload, phlo_share: i64) -> Cosigner {
        let secp = Secp256k1;
        let (sk, pk) = secp.new_key_pair();
        let serialized = payload.encode_to_vec();
        let hash =
            Signed::<TestPayload>::signature_hash(&Secp256k1::name(), serialized);
        let sig = secp.sign(&hash, &sk.bytes);
        Cosigner {
            pk,
            sig: prost::bytes::Bytes::from(sig),
            sig_algorithm: Box::new(secp),
            phlo_share,
        }
    }

    #[test]
    fn cosigned_from_signed_data_accepts_canonical_input() {
        let payload = TestPayload {
            term: "test_term".to_string(),
            phlo_limit: 300,
        };
        let s1 = fresh_cosigner(&payload, 100);
        let s2 = fresh_cosigner(&payload, 200);
        let cosigned = Cosigned::from_signed_data(payload.clone(), vec![s1, s2], 300)
            .expect("valid 2-signer cosigned must construct");
        assert!(cosigned.is_compound());
        assert_eq!(cosigned.signers().len(), 2);
        assert_eq!(cosigned.total_phlo_share(), 300);
        // Canonical order: pk.bytes ascending.
        let pks: Vec<_> = cosigned.signers().iter().map(|s| s.pk.bytes.clone()).collect();
        assert!(pks[0].as_ref() <= pks[1].as_ref());
    }

    #[test]
    fn cosigned_auto_sorts_input() {
        let payload = TestPayload {
            term: "auto_sort".to_string(),
            phlo_limit: 200,
        };
        let s1 = fresh_cosigner(&payload, 100);
        let s2 = fresh_cosigner(&payload, 100);
        // Submit in arbitrary order; constructor canonicalizes.
        let cosigned_a =
            Cosigned::from_signed_data(payload.clone(), vec![s1.clone(), s2.clone()], 200)
                .expect("valid");
        let cosigned_b =
            Cosigned::from_signed_data(payload.clone(), vec![s2, s1], 200).expect("valid");
        // Permutation invariant: identical canonical signer list.
        assert_eq!(cosigned_a.signers().len(), cosigned_b.signers().len());
        for (a, b) in cosigned_a.signers().iter().zip(cosigned_b.signers().iter()) {
            assert_eq!(a.pk, b.pk);
            assert_eq!(a.sig, b.sig);
            assert_eq!(a.phlo_share, b.phlo_share);
        }
    }

    #[test]
    fn cosigned_rejects_duplicate_signer() {
        let payload = TestPayload {
            term: "dup".to_string(),
            phlo_limit: 200,
        };
        let s1 = fresh_cosigner(&payload, 100);
        let s1_clone = s1.clone();
        let err =
            Cosigned::from_signed_data(payload, vec![s1, s1_clone], 200).expect_err("must reject");
        match err {
            CosignedError::DuplicateSigner { .. } => {}
            other => panic!("expected DuplicateSigner, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_rejects_share_sum_mismatch() {
        let payload = TestPayload {
            term: "mismatch".to_string(),
            phlo_limit: 500,
        };
        let s1 = fresh_cosigner(&payload, 100);
        let s2 = fresh_cosigner(&payload, 200);
        // Σ shares = 300, but phlo_limit = 500.
        let err = Cosigned::from_signed_data(payload, vec![s1, s2], 500).expect_err("must reject");
        match err {
            CosignedError::PhloShareMismatch { sum, expected } => {
                assert_eq!(sum, 300);
                assert_eq!(expected, 500);
            }
            other => panic!("expected PhloShareMismatch, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_rejects_negative_share() {
        let payload = TestPayload {
            term: "neg".to_string(),
            phlo_limit: 100,
        };
        let s1 = fresh_cosigner(&payload, -50);
        let err = Cosigned::from_signed_data(payload, vec![s1], 100).expect_err("must reject");
        match err {
            CosignedError::NegativePhloShare { share, .. } => {
                assert_eq!(share, -50);
            }
            other => panic!("expected NegativePhloShare, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_rejects_empty_signer_list() {
        let payload = TestPayload {
            term: "empty".to_string(),
            phlo_limit: 100,
        };
        let err = Cosigned::from_signed_data(payload, vec![], 100).expect_err("must reject");
        match err {
            CosignedError::EmptySignerList => {}
            other => panic!("expected EmptySignerList, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_rejects_invalid_signature() {
        let payload_a = TestPayload {
            term: "payload_a".to_string(),
            phlo_limit: 100,
        };
        let payload_b = TestPayload {
            term: "payload_b_different".to_string(),
            phlo_limit: 100,
        };
        // Signer signs payload_a, but envelope claims payload_b.
        let s_for_a = fresh_cosigner(&payload_a, 100);
        let err = Cosigned::from_signed_data(payload_b, vec![s_for_a], 100)
            .expect_err("verification must fail");
        match err {
            CosignedError::SignatureVerifyFailed { index, .. } => {
                assert_eq!(index, 0);
            }
            other => panic!("expected SignatureVerifyFailed, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_from_single_signer_uplift() {
        let secp = Secp256k1;
        let (sk, _pk) = secp.new_key_pair();
        let payload = TestPayload {
            term: "single".to_string(),
            phlo_limit: 250,
        };
        let signed =
            Signed::<TestPayload>::create(payload, Box::new(secp), sk).expect("signed creation");
        let cosigned =
            Cosigned::from_single_signer(signed, 250).expect("single-signer uplift must work");
        assert!(!cosigned.is_compound());
        assert_eq!(cosigned.signers().len(), 1);
        assert_eq!(cosigned.signers()[0].phlo_share, 250);
        assert_eq!(cosigned.total_phlo_share(), 250);
    }
}
