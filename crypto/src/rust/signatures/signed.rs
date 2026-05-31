use prost::Message;

#[cfg(feature = "oqs_pq_experimental")]
use super::oqs_pq::{Falcon512, MlDsa65, SlhDsaSha2_128s};
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
/// uniqueness, per-signer verification against the canonical message hash).
///
/// D3 (DR-9): the singular-phlo escrow/price model is removed, so the
/// share-sum / negative-share / share-overflow variants are gone — funding
/// is the per-signature supply pool Σ⟦s⟧, not an envelope share invariant.
#[derive(Debug, thiserror::Error)]
pub enum CosignedError {
    #[error("signer at index {index} (pk={pk_hex}) failed signature verification")]
    SignatureVerifyFailed { index: usize, pk_hex: String },
    #[error("duplicate signer pk: {pk_hex}")]
    DuplicateSigner { pk_hex: String },
    #[error("empty signer list — a Cosigned envelope requires at least one signer")]
    EmptySignerList,
    #[error("quorum not met: required {threshold}, valid signers {valid_signers}")]
    QuorumNotMet { threshold: u32, valid_signers: u32 },
    #[error("invalid quorum threshold: threshold={threshold}, total_signers={total_signers}; threshold must satisfy 1 ≤ threshold ≤ total_signers")]
    InvalidQuorumThreshold { threshold: u32, total_signers: u32 },
    #[error("LL algebra validation failed at connective {connective}: {message}")]
    SigAlgebraValidationFailed {
        connective: &'static str,
        message: String,
    },
    #[error("Plus.chosen_branch must be 0 (left) or 1 (right), got {got}")]
    PlusInvalidChosenBranch { got: i32 },
    #[error("WhyNot atom verification failed: optional atom presented but signature invalid")]
    WhyNotInvalidSignature,
}

/// One signer in a multi-signature deploy envelope. Sorted ascending by
/// `pk.bytes` inside a [`Cosigned`] (enforced at construction). Each
/// cosigner signs the same canonical message hash as the primary.
///
/// D3 (DR-9): a cosigner carries NO `phlo_share` — fuel for a deploy comes
/// from the per-signature supply pool Σ⟦s⟧, not a per-signer escrow share.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cosigner {
    pub pk: PublicKey,
    #[serde(with = "shared::rust::serde_bytes")]
    pub sig: prost::bytes::Bytes,
    pub sig_algorithm: Box<dyn SignaturesAlg>,
}

impl PartialEq for Cosigner {
    fn eq(&self, other: &Self) -> bool {
        self.pk == other.pk
            && self.sig == other.sig
            && self.sig_algorithm.eq(&other.sig_algorithm)
    }
}

impl Eq for Cosigner {}

impl std::hash::Hash for Cosigner {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pk.hash(state);
        self.sig.hash(state);
        self.sig_algorithm.name().hash(state);
    }
}

/// Multi-signature deploy envelope. Generalizes [`Signed<A>`] to carry an
/// ordered, deduplicated list of cosigners (one or more). A length-1
/// `Cosigned` is observably equivalent to a legacy `Signed`.
///
/// Invariants enforced at construction by [`Cosigned::from_signed_data`]:
/// 1. `signers` is non-empty.
/// 2. `signers` is sorted ascending by `pk.bytes`; no duplicates.
/// 3. Every `signers[i].sig` verifies against the canonical
///    [`Signed::<A>::signature_hash`] of the encoded `data`.
///
/// These invariants are the multi-signature analogue of `Signed<A>`'s
/// single-signature verification, and they realize the operational
/// semantics of the cost-accounted rho-calculus paper's `σ₁ & σ₂`
/// compound-signature operator (`publications/cost-accounting/cost-accounted-rho.tex`,
/// §3.2 Rules 2-5).
///
/// D3 (DR-9): the envelope carries NO phlo escrow — there is no per-signer
/// `phlo_share` and no `Σ shares == phlo_limit` invariant. A deploy's fuel
/// is the per-signature supply pool Σ⟦s⟧, gated at block assembly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cosigned<A> {
    pub data: A,
    signers: Vec<Cosigner>,
    /// Phase 2 M-of-N quorum threshold. Zero for N-of-N (Phase 1)
    /// semantics where every signer must verify; k > 0 indicates at
    /// least `k` of `signers.len()` valid signatures suffice. Carried on
    /// the envelope so it survives ProcessedDeploy round-trip and replay.
    #[serde(default)]
    cosigner_threshold: u32,
}

impl<A: PartialEq> PartialEq for Cosigned<A> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
            && self.signers == other.signers
            && self.cosigner_threshold == other.cosigner_threshold
    }
}

impl<A: Eq> Eq for Cosigned<A> {}

impl<A: std::hash::Hash> std::hash::Hash for Cosigned<A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.hash(state);
        for signer in &self.signers {
            signer.hash(state);
        }
        self.cosigner_threshold.hash(state);
    }
}

impl<A: std::fmt::Debug + serde::Serialize + ToMessage> Cosigned<A> {
    /// Construct and validate a multi-signature envelope.
    ///
    /// The constructor enforces the three invariants listed in the
    /// [`Cosigned`] type documentation (non-empty, canonical-sorted +
    /// deduplicated signers, every signature verifies). Returns:
    /// - `Ok(Cosigned)` if every invariant holds.
    /// - `Err(CosignedError)` if any invariant is violated.
    ///
    /// The constructor canonicalizes the signer order by sorting ascending
    /// on `pk.bytes`; callers do not need to pre-sort. Duplicate `pk`s are
    /// rejected (a deploy must not list the same signer twice).
    ///
    /// D3 (DR-9): no `phlo_limit` parameter and no share-sum invariant —
    /// the envelope carries no escrow.
    pub fn from_signed_data(data: A, signers: Vec<Cosigner>) -> Result<Self, CosignedError> {
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

        // Verify each signer against the canonical message hash. Each
        // signer's algorithm dictates the hash function (Blake2b256 for
        // most; Keccak256 with Ethereum prefix for secp256k1-eth; etc.).
        let serialized_data = data.to_message().encode_to_vec();
        for (i, signer) in canonical.iter().enumerate() {
            let hash =
                Signed::<A>::signature_hash(&signer.sig_algorithm.name(), serialized_data.clone());
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
            cosigner_threshold: 0,
        })
    }

    /// Construct an M-of-N threshold-signature envelope (Phase 2).
    ///
    /// Like [`from_signed_data`] but admits placeholder signers whose `sig`
    /// is empty (those entries do not need to verify; they count toward
    /// the canonical signer list but not toward the quorum tally).
    /// At least `threshold` of the provided signers MUST have valid
    /// signatures verifying against the canonical message hash.
    ///
    /// Invariants (in addition to the Cosigned base invariants):
    /// - `1 ≤ threshold ≤ signers.len()` (returns `InvalidQuorumThreshold` otherwise).
    /// - The number of signers with `sig.is_some_non_empty()` AND a valid
    ///   signature is ≥ `threshold` (returns `QuorumNotMet` otherwise).
    /// - Canonical pk-sort and no-duplicate invariants are unchanged.
    ///
    /// D3 (DR-9): no `phlo_limit` parameter and no per-signer share invariant.
    pub fn from_signed_data_threshold(
        data: A,
        signers: Vec<Cosigner>,
        threshold: u32,
    ) -> Result<Self, CosignedError> {
        if signers.is_empty() {
            return Err(CosignedError::EmptySignerList);
        }
        let total_signers = signers.len() as u32;
        if threshold < 1 || threshold > total_signers {
            return Err(CosignedError::InvalidQuorumThreshold {
                threshold,
                total_signers,
            });
        }

        let mut canonical = signers;
        canonical.sort_by(|a, b| a.pk.bytes.as_ref().cmp(b.pk.bytes.as_ref()));
        for window in canonical.windows(2) {
            if window[0].pk.bytes == window[1].pk.bytes {
                return Err(CosignedError::DuplicateSigner {
                    pk_hex: hex::encode(&window[0].pk.bytes),
                });
            }
        }

        let serialized_data = data.to_message().encode_to_vec();
        let mut valid_signers: u32 = 0;
        for (i, signer) in canonical.iter().enumerate() {
            // Placeholder signers (empty sig) count toward the canonical
            // signer list but not toward the quorum tally.
            if signer.sig.is_empty() {
                continue;
            }
            let hash =
                Signed::<A>::signature_hash(&signer.sig_algorithm.name(), serialized_data.clone());
            if !signer
                .sig_algorithm
                .verify(&hash, &signer.sig, &signer.pk.bytes)
            {
                return Err(CosignedError::SignatureVerifyFailed {
                    index: i,
                    pk_hex: hex::encode(&signer.pk.bytes),
                });
            }
            valid_signers = valid_signers.saturating_add(1);
        }

        if valid_signers < threshold {
            return Err(CosignedError::QuorumNotMet {
                threshold,
                valid_signers,
            });
        }

        Ok(Cosigned {
            data,
            signers: canonical,
            cosigner_threshold: threshold,
        })
    }

    /// Construct a single-signer Cosigned envelope from an already-validated
    /// [`Signed<A>`]. This is the legacy-uplift path: callers decoding a
    /// `cosigners.is_empty()` wire deploy use this to obtain a one-element
    /// `Cosigned<A>`.
    ///
    /// No re-verification occurs (the `Signed<A>` was already verified at
    /// construction); we simply construct the one-element envelope. This is
    /// infallible (D3, DR-9: there is no share invariant to validate), but
    /// the `Result` return is retained for call-site stability.
    pub fn from_single_signer(signed: Signed<A>) -> Result<Self, CosignedError> {
        let signer = Cosigner {
            pk: signed.pk,
            sig: signed.sig,
            sig_algorithm: signed.sig_algorithm,
        };
        Ok(Cosigned {
            data: signed.data,
            signers: vec![signer],
            cosigner_threshold: 0,
        })
    }

    /// Phase 2 M-of-N quorum threshold. 0 = N-of-N (Phase 1) semantics.
    pub fn cosigner_threshold(&self) -> u32 { self.cosigner_threshold }

    /// All signers, in canonical ascending `pk.bytes` order. Always non-empty.
    pub fn signers(&self) -> &[Cosigner] { &self.signers }

    /// The deploy payload. Borrow accessor mirroring [`Self::signers`] so
    /// callers (e.g. `deploy_group_id`) can serialize the canonical payload
    /// without reaching into the public `data` field directly.
    pub fn data(&self) -> &A { &self.data }

    /// The primary signer (`signers[0]`). Equivalent to the legacy
    /// single-signer `Signed<A>`'s sole signer.
    pub fn primary(&self) -> &Cosigner { &self.signers[0] }

    /// `true` if more than one signer is present (i.e., a true multi-sig).
    pub fn is_compound(&self) -> bool { self.signers.len() > 1 }

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
            #[cfg(feature = "oqs_pq_experimental")]
            name if name == MlDsa65::name() => MlDsa65::domain_separated_hash(&serialized_data),
            #[cfg(feature = "oqs_pq_experimental")]
            name if name == Falcon512::name() => {
                Falcon512::domain_separated_hash(&serialized_data)
            }
            #[cfg(feature = "oqs_pq_experimental")]
            name if name == SlhDsaSha2_128s::name() => {
                SlhDsaSha2_128s::domain_separated_hash(&serialized_data)
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
        /// Arbitrary second field so the payload round-trips a non-trivial
        /// message (D3: the deploy carries no phlo escrow; this is just a
        /// nonce to vary the signed bytes across test payloads).
        #[prost(int64, tag = "2")]
        pub nonce: i64,
    }

    impl ToMessage for TestPayload {
        type Type = TestPayload;
        fn to_message(&self) -> Self::Type { self.clone() }
    }

    fn fresh_cosigner(payload: &TestPayload) -> Cosigner {
        let secp = Secp256k1;
        let (sk, pk) = secp.new_key_pair();
        let serialized = payload.encode_to_vec();
        let hash = Signed::<TestPayload>::signature_hash(&Secp256k1::name(), serialized);
        let sig = secp.sign(&hash, &sk.bytes);
        Cosigner {
            pk,
            sig: prost::bytes::Bytes::from(sig),
            sig_algorithm: Box::new(secp),
        }
    }

    #[test]
    fn cosigned_from_signed_data_accepts_canonical_input() {
        let payload = TestPayload {
            term: "test_term".to_string(),
            nonce: 300,
        };
        let s1 = fresh_cosigner(&payload);
        let s2 = fresh_cosigner(&payload);
        let cosigned = Cosigned::from_signed_data(payload.clone(), vec![s1, s2])
            .expect("valid 2-signer cosigned must construct");
        assert!(cosigned.is_compound());
        assert_eq!(cosigned.signers().len(), 2);
        // Canonical order: pk.bytes ascending.
        let pks: Vec<_> = cosigned
            .signers()
            .iter()
            .map(|s| s.pk.bytes.clone())
            .collect();
        assert!(pks[0].as_ref() <= pks[1].as_ref());
    }

    #[test]
    fn cosigned_auto_sorts_input() {
        let payload = TestPayload {
            term: "auto_sort".to_string(),
            nonce: 200,
        };
        let s1 = fresh_cosigner(&payload);
        let s2 = fresh_cosigner(&payload);
        // Submit in arbitrary order; constructor canonicalizes.
        let cosigned_a =
            Cosigned::from_signed_data(payload.clone(), vec![s1.clone(), s2.clone()])
                .expect("valid");
        let cosigned_b =
            Cosigned::from_signed_data(payload.clone(), vec![s2, s1]).expect("valid");
        // Permutation invariant: identical canonical signer list.
        assert_eq!(cosigned_a.signers().len(), cosigned_b.signers().len());
        for (a, b) in cosigned_a.signers().iter().zip(cosigned_b.signers().iter()) {
            assert_eq!(a.pk, b.pk);
            assert_eq!(a.sig, b.sig);
        }
    }

    #[test]
    fn cosigned_rejects_duplicate_signer() {
        let payload = TestPayload {
            term: "dup".to_string(),
            nonce: 200,
        };
        let s1 = fresh_cosigner(&payload);
        let s1_clone = s1.clone();
        let err =
            Cosigned::from_signed_data(payload, vec![s1, s1_clone]).expect_err("must reject");
        match err {
            CosignedError::DuplicateSigner { .. } => {}
            other => panic!("expected DuplicateSigner, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_rejects_empty_signer_list() {
        let payload = TestPayload {
            term: "empty".to_string(),
            nonce: 100,
        };
        let err = Cosigned::from_signed_data(payload, vec![]).expect_err("must reject");
        match err {
            CosignedError::EmptySignerList => {}
            other => panic!("expected EmptySignerList, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_rejects_invalid_signature() {
        let payload_a = TestPayload {
            term: "payload_a".to_string(),
            nonce: 100,
        };
        let payload_b = TestPayload {
            term: "payload_b_different".to_string(),
            nonce: 100,
        };
        // Signer signs payload_a, but envelope claims payload_b.
        let s_for_a = fresh_cosigner(&payload_a);
        let err = Cosigned::from_signed_data(payload_b, vec![s_for_a])
            .expect_err("verification must fail");
        match err {
            CosignedError::SignatureVerifyFailed { index, .. } => {
                assert_eq!(index, 0);
            }
            other => panic!("expected SignatureVerifyFailed, got {:?}", other),
        }
    }

    fn fresh_signer_for(payload: &TestPayload) -> Cosigner {
        fresh_cosigner(payload)
    }

    fn empty_placeholder_signer() -> Cosigner {
        let secp = Secp256k1;
        let (_, pk) = secp.new_key_pair();
        Cosigner {
            pk,
            sig: prost::bytes::Bytes::new(),
            sig_algorithm: Box::new(secp),
        }
    }

    #[test]
    fn cosigned_threshold_accepts_quorum_satisfied_2_of_3() {
        let payload = TestPayload {
            term: "threshold_2_of_3".to_string(),
            nonce: 200,
        };
        let s1 = fresh_signer_for(&payload);
        let s2 = fresh_signer_for(&payload);
        let s3 = empty_placeholder_signer();
        let cosigned = Cosigned::from_signed_data_threshold(payload, vec![s1, s2, s3], 2)
            .expect("2-of-3 with 2 valid sigs must construct");
        assert_eq!(cosigned.signers().len(), 3);
    }

    #[test]
    fn cosigned_threshold_rejects_quorum_not_met() {
        let payload = TestPayload {
            term: "threshold_unmet".to_string(),
            nonce: 100,
        };
        let s1 = fresh_signer_for(&payload);
        let s2 = empty_placeholder_signer();
        let s3 = empty_placeholder_signer();
        let err = Cosigned::from_signed_data_threshold(payload, vec![s1, s2, s3], 2)
            .expect_err("2-of-3 with 1 valid sig must reject");
        match err {
            CosignedError::QuorumNotMet {
                threshold,
                valid_signers,
            } => {
                assert_eq!(threshold, 2);
                assert_eq!(valid_signers, 1);
            }
            other => panic!("expected QuorumNotMet, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_threshold_rejects_threshold_zero() {
        let payload = TestPayload {
            term: "threshold_zero".to_string(),
            nonce: 100,
        };
        let s1 = fresh_signer_for(&payload);
        let err = Cosigned::from_signed_data_threshold(payload, vec![s1], 0)
            .expect_err("threshold=0 must reject");
        match err {
            CosignedError::InvalidQuorumThreshold {
                threshold,
                total_signers,
            } => {
                assert_eq!(threshold, 0);
                assert_eq!(total_signers, 1);
            }
            other => panic!("expected InvalidQuorumThreshold, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_threshold_rejects_threshold_exceeds_total() {
        let payload = TestPayload {
            term: "threshold_too_high".to_string(),
            nonce: 100,
        };
        let s1 = fresh_signer_for(&payload);
        let err = Cosigned::from_signed_data_threshold(payload, vec![s1], 5)
            .expect_err("threshold > total must reject");
        match err {
            CosignedError::InvalidQuorumThreshold {
                threshold,
                total_signers,
            } => {
                assert_eq!(threshold, 5);
                assert_eq!(total_signers, 1);
            }
            other => panic!("expected InvalidQuorumThreshold, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_threshold_rejects_non_empty_invalid_signature_even_when_quorum_met() {
        let payload = TestPayload {
            term: "threshold_invalid_non_empty".to_string(),
            nonce: 300,
        };
        let s1 = fresh_signer_for(&payload);
        let s2 = fresh_signer_for(&payload);
        let other_payload = TestPayload {
            term: "wrong_payload".to_string(),
            nonce: 300,
        };
        let invalid = fresh_signer_for(&other_payload);
        let err = Cosigned::from_signed_data_threshold(payload, vec![s1, s2, invalid], 2)
            .expect_err("non-empty invalid threshold member must reject");
        match err {
            CosignedError::SignatureVerifyFailed { .. } => {}
            other => panic!("expected SignatureVerifyFailed, got {:?}", other),
        }
    }

    #[test]
    fn cosigned_from_single_signer_uplift() {
        let secp = Secp256k1;
        let (sk, _pk) = secp.new_key_pair();
        let payload = TestPayload {
            term: "single".to_string(),
            nonce: 250,
        };
        let signed =
            Signed::<TestPayload>::create(payload, Box::new(secp), sk).expect("signed creation");
        let cosigned =
            Cosigned::from_single_signer(signed).expect("single-signer uplift must work");
        assert!(!cosigned.is_compound());
        assert_eq!(cosigned.signers().len(), 1);
    }

    /// Hybrid (classical + post-quantum) N-of-N multi-signature envelope:
    /// a secp256k1 primary and an ML-DSA-65 cosigner, both signing the SAME
    /// canonical payload but each over its OWN algorithm-specific
    /// `signature_hash` (Blake2b256 for secp256k1, the ML-DSA domain-separated
    /// hash for ML-DSA-65). Reuses the existing `Cosigned::from_signed_data`
    /// path, which re-verifies every signer; construction succeeding proves
    /// BOTH the classical and the post-quantum signature verify under the
    /// generic-over-G signature surface (paper §4.5).
    #[cfg(feature = "oqs_pq_experimental")]
    #[test]
    fn cosigned_hybrid_secp256k1_plus_ml_dsa_65_n_of_n_verifies() {
        use crate::rust::signatures::oqs_pq::MlDsa65;
        use crate::rust::signatures::secp256k1::Secp256k1;

        let payload = TestPayload {
            term: "hybrid_pq_classical".to_string(),
            nonce: 300,
        };
        let serialized = payload.encode_to_vec();

        // Classical secp256k1 cosigner (Blake2b256 signing hash).
        let secp = Secp256k1;
        let (secp_sk, secp_pk) = secp.new_key_pair();
        let secp_hash =
            Signed::<TestPayload>::signature_hash(&Secp256k1::name(), serialized.clone());
        let secp_sig = secp.sign(&secp_hash, &secp_sk.bytes);
        assert!(!secp_sig.is_empty(), "secp256k1 must sign");
        let secp_cosigner = Cosigner {
            pk: secp_pk,
            sig: prost::bytes::Bytes::from(secp_sig),
            sig_algorithm: Box::new(secp),
        };

        // Post-quantum ML-DSA-65 cosigner (domain-separated signing hash).
        let ml = MlDsa65;
        let (ml_sk, ml_pk) = ml.new_key_pair();
        assert!(!ml_sk.bytes.is_empty(), "ML-DSA-65 keygen must succeed");
        let ml_hash =
            Signed::<TestPayload>::signature_hash(&MlDsa65::name(), serialized.clone());
        let ml_sig = ml.sign(&ml_hash, &ml_sk.bytes);
        assert!(!ml_sig.is_empty(), "ML-DSA-65 must sign");
        let ml_cosigner = Cosigner {
            pk: ml_pk,
            sig: prost::bytes::Bytes::from(ml_sig),
            sig_algorithm: Box::new(ml),
        };

        // N-of-N: both signatures must verify for construction to succeed.
        let cosigned = Cosigned::from_signed_data(
            payload.clone(),
            vec![secp_cosigner, ml_cosigner],
        )
        .expect("hybrid secp256k1 + ML-DSA-65 envelope must construct (both verify)");
        assert!(cosigned.is_compound());
        assert_eq!(cosigned.signers().len(), 2);

        // Negative control: corrupt the ML-DSA-65 signature; N-of-N must reject.
        let secp2 = Secp256k1;
        let (secp2_sk, secp2_pk) = secp2.new_key_pair();
        let secp2_hash =
            Signed::<TestPayload>::signature_hash(&Secp256k1::name(), serialized.clone());
        let secp2_sig = secp2.sign(&secp2_hash, &secp2_sk.bytes);
        let secp2_cosigner = Cosigner {
            pk: secp2_pk,
            sig: prost::bytes::Bytes::from(secp2_sig),
            sig_algorithm: Box::new(secp2),
        };
        let ml2 = MlDsa65;
        let (ml2_sk, ml2_pk) = ml2.new_key_pair();
        let ml2_hash =
            Signed::<TestPayload>::signature_hash(&MlDsa65::name(), serialized.clone());
        let mut ml2_sig = ml2.sign(&ml2_hash, &ml2_sk.bytes);
        let mid = ml2_sig.len() / 2;
        ml2_sig[mid] ^= 0x01; // tamper the PQ signature
        let bad_ml_cosigner = Cosigner {
            pk: ml2_pk,
            sig: prost::bytes::Bytes::from(ml2_sig),
            sig_algorithm: Box::new(ml2),
        };
        let err = Cosigned::from_signed_data(
            payload,
            vec![secp2_cosigner, bad_ml_cosigner],
        )
        .expect_err("tampered PQ cosigner must fail N-of-N verification");
        match err {
            CosignedError::SignatureVerifyFailed { .. } => {}
            other => panic!("expected SignatureVerifyFailed, got {:?}", other),
        }
    }
}
