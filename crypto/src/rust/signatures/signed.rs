use k256::ecdsa::Signature as Secp256k1Signature;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::PublicKey as Secp256k1PublicKey;
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
    fn envelope_intent_v61(&self) -> Result<Vec<u8>, String>;
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
    #[error("envelope commitment has duplicate signer pk: {pk_hex}")]
    DuplicateCommitmentSigner { pk_hex: String },
    #[error("envelope signer order is not canonical at index {index}")]
    NonCanonicalSignerOrder { index: usize },
    #[error("legacy payload signatures do not authenticate a protocol-v6 envelope commitment")]
    LegacyEnvelopeCommitmentUnavailable,
    #[error("unsupported protocol-v6 signature algorithm: {algorithm}")]
    UnsupportedEnvelopeSignatureAlgorithm { algorithm: String },
    #[error("protocol-v6 signer at index {index} has a non-canonical public key")]
    NonCanonicalEnvelopePublicKey { index: usize },
    #[error("protocol-v6 signer at index {index} has a non-canonical signature")]
    NonCanonicalEnvelopeSignature { index: usize },
    #[error("protocol-v6 envelope has duplicate ground authority: {pk_hex}")]
    DuplicateGroundAuthority { pk_hex: String },
    #[error("protocol-v6 envelope intent is invalid: {message}")]
    InvalidEnvelopeIntent { message: String },
    #[error("protocol-v6 presence bitmap is invalid")]
    InvalidEnvelopePresenceBitmap,
}

const ENVELOPE_COMMITMENT_DOMAIN: &[u8] = b"f1r3fly:casper:deploy-envelope:v6.1";
const ENVELOPE_SIGNATURE_DOMAIN: &[u8] = b"f1r3fly:casper:deploy-envelope-signature:v6.1";
const ENVELOPE_PROTOCOL_VERSION: u16 = 6;

fn append_commitment_field(preimage: &mut Vec<u8>, bytes: &[u8]) {
    preimage.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    preimage.extend_from_slice(bytes);
}

fn envelope_scheme_id(algorithm: &str) -> Result<u16, CosignedError> {
    match algorithm {
        "secp256k1" => Ok(1),
        Secp256k1Eth::NAME | Secp256k1Eth::LEGACY_NAME => Ok(2),
        _ => Err(CosignedError::UnsupportedEnvelopeSignatureAlgorithm {
            algorithm: algorithm.to_string(),
        }),
    }
}

fn canonical_envelope_public_key(public_key: &[u8]) -> Option<Vec<u8>> {
    let parsed = Secp256k1PublicKey::from_sec1_bytes(public_key).ok()?;
    let canonical = parsed.to_encoded_point(false).as_bytes().to_vec();
    (canonical.as_slice() == public_key).then_some(canonical)
}

fn canonical_envelope_signature(scheme_id: u16, signature: &[u8]) -> bool {
    let parsed = match scheme_id {
        1 => Secp256k1Signature::from_der(signature)
            .ok()
            .filter(|value| {
                value.to_der().as_bytes() == signature && value.normalize_s().is_none()
            }),
        2 => Secp256k1Signature::from_slice(signature)
            .ok()
            .filter(|value| value.normalize_s().is_none()),
        _ => None,
    };
    parsed.is_some()
}

fn principal_bytes(signer: &Cosigner, index: usize) -> Result<Vec<u8>, CosignedError> {
    let scheme_id = envelope_scheme_id(&signer.sig_algorithm.name())?;
    let public_key = canonical_envelope_public_key(&signer.pk.bytes)
        .ok_or(CosignedError::NonCanonicalEnvelopePublicKey { index })?;
    let mut encoded = Vec::with_capacity(2 + 4 + public_key.len());
    encoded.extend_from_slice(&scheme_id.to_be_bytes());
    encoded.extend_from_slice(&(public_key.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&public_key);
    Ok(encoded)
}

fn presence_bitmap(signers: &[Cosigner]) -> Vec<u8> {
    let mut bitmap = vec![0u8; signers.len().div_ceil(8)];
    for (index, signer) in signers.iter().enumerate() {
        if !signer.sig.is_empty() {
            bitmap[index / 8] |= 1 << (index % 8);
        }
    }
    bitmap
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
        self.pk == other.pk && self.sig == other.sig && self.sig_algorithm.eq(&other.sig_algorithm)
    }
}

impl Eq for Cosigner {}

impl Cosigner {
    pub fn scheme_id_v61(&self) -> Result<u16, CosignedError> {
        envelope_scheme_id(&self.sig_algorithm.name())
    }

    pub fn principal_bytes_v61(&self) -> Result<Vec<u8>, CosignedError> { principal_bytes(self, 0) }
}

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
    #[serde(default)]
    signing_domain: CosignedSigningDomain,
}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize
)]
pub enum CosignedSigningDomain {
    #[default]
    LegacyPayload,
    EnvelopeV6,
}

impl<A: PartialEq> PartialEq for Cosigned<A> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
            && self.signers == other.signers
            && self.cosigner_threshold == other.cosigner_threshold
            && self.signing_domain == other.signing_domain
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
        self.signing_domain.hash(state);
    }
}

impl<A: std::fmt::Debug + serde::Serialize + ToMessage> Cosigned<A> {
    pub fn validate_envelope_signer_order(signers: &[Cosigner]) -> Result<(), CosignedError> {
        if signers.is_empty() {
            return Err(CosignedError::EmptySignerList);
        }
        let principals = signers
            .iter()
            .enumerate()
            .map(|(index, signer)| principal_bytes(signer, index))
            .collect::<Result<Vec<_>, _>>()?;
        for (index, pair) in principals.windows(2).enumerate() {
            match pair[0].cmp(&pair[1]) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => {
                    return Err(CosignedError::DuplicateCommitmentSigner {
                        pk_hex: hex::encode(&signers[index].pk.bytes),
                    });
                }
                std::cmp::Ordering::Greater => {
                    return Err(CosignedError::NonCanonicalSignerOrder { index: index + 1 });
                }
            }
        }
        for pair in signers.windows(2) {
            if pair[0].pk.bytes == pair[1].pk.bytes {
                return Err(CosignedError::DuplicateGroundAuthority {
                    pk_hex: hex::encode(&pair[0].pk.bytes),
                });
            }
        }
        Ok(())
    }

    fn canonical_envelope_signers(signers: &[Cosigner]) -> Result<Vec<Cosigner>, CosignedError> {
        if signers.is_empty() {
            return Err(CosignedError::EmptySignerList);
        }
        let mut canonical = signers
            .iter()
            .enumerate()
            .map(|(index, signer)| Ok((principal_bytes(signer, index)?, signer.clone())))
            .collect::<Result<Vec<_>, CosignedError>>()?;
        canonical.sort_by(|left, right| left.0.cmp(&right.0));
        for pair in canonical.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(CosignedError::DuplicateCommitmentSigner {
                    pk_hex: hex::encode(&pair[0].1.pk.bytes),
                });
            }
        }
        let mut ground_owners = canonical
            .iter()
            .map(|(_, signer)| signer.pk.bytes.clone())
            .collect::<Vec<_>>();
        ground_owners.sort();
        for pair in ground_owners.windows(2) {
            if pair[0] == pair[1] {
                return Err(CosignedError::DuplicateGroundAuthority {
                    pk_hex: hex::encode(&pair[0]),
                });
            }
        }
        Ok(canonical.into_iter().map(|(_, signer)| signer).collect())
    }

    fn envelope_commitment_for_canonical(
        data: &A,
        signers: &[Cosigner],
        cosigner_threshold: u32,
        bitmap: &[u8],
    ) -> Result<prost::bytes::Bytes, CosignedError> {
        let total_signers = signers.len() as u32;
        if cosigner_threshold < 1 || cosigner_threshold > total_signers {
            return Err(CosignedError::InvalidQuorumThreshold {
                threshold: cosigner_threshold,
                total_signers,
            });
        }
        if bitmap.len() != signers.len().div_ceil(8)
            || bitmap.last().is_some_and(|last| {
                let used = signers.len() % 8;
                used != 0 && *last & !((1u8 << used) - 1) != 0
            })
        {
            return Err(CosignedError::InvalidEnvelopePresenceBitmap);
        }
        let selected = bitmap.iter().map(|byte| byte.count_ones()).sum::<u32>();
        if selected < cosigner_threshold
            || (cosigner_threshold == total_signers && selected != total_signers)
        {
            return Err(CosignedError::QuorumNotMet {
                threshold: cosigner_threshold,
                valid_signers: selected,
            });
        }
        let intent = data
            .envelope_intent_v61()
            .map_err(|message| CosignedError::InvalidEnvelopeIntent { message })?;
        let mut policy = Vec::new();
        if cosigner_threshold == total_signers {
            policy.push(1);
            policy.extend_from_slice(&total_signers.to_be_bytes());
        } else {
            policy.push(2);
            policy.extend_from_slice(&cosigner_threshold.to_be_bytes());
            policy.extend_from_slice(&total_signers.to_be_bytes());
        }
        for (index, signer) in signers.iter().enumerate() {
            policy.extend_from_slice(&principal_bytes(signer, index)?);
        }
        let mut preimage = Vec::new();
        append_commitment_field(&mut preimage, ENVELOPE_COMMITMENT_DOMAIN);
        preimage.extend_from_slice(&ENVELOPE_PROTOCOL_VERSION.to_be_bytes());
        append_commitment_field(&mut preimage, &intent);
        append_commitment_field(&mut preimage, &policy);
        preimage.extend_from_slice(&(bitmap.len() as u32).to_be_bytes());
        preimage.extend_from_slice(bitmap);
        Ok(Blake2b256::hash(preimage).into())
    }

    pub fn envelope_commitment_for(
        data: &A,
        signers: &[Cosigner],
        cosigner_threshold: u32,
    ) -> Result<prost::bytes::Bytes, CosignedError> {
        let canonical = Self::canonical_envelope_signers(signers)?;
        let bitmap = presence_bitmap(&canonical);
        Self::envelope_commitment_for_canonical(data, &canonical, cosigner_threshold, &bitmap)
    }

    pub fn envelope_commitment_for_presence(
        data: &A,
        signers: &[Cosigner],
        cosigner_threshold: u32,
        bitmap: &[u8],
    ) -> Result<prost::bytes::Bytes, CosignedError> {
        let canonical = Self::canonical_envelope_signers(signers)?;
        Self::envelope_commitment_for_canonical(data, &canonical, cosigner_threshold, bitmap)
    }

    pub fn envelope_commitment(&self) -> Result<prost::bytes::Bytes, CosignedError> {
        if self.signing_domain != CosignedSigningDomain::EnvelopeV6 {
            return Err(CosignedError::LegacyEnvelopeCommitmentUnavailable);
        }
        Self::envelope_commitment_for(&self.data, &self.signers, self.cosigner_threshold)
    }

    pub fn envelope_signing_hash(
        data: &A,
        signers: &[Cosigner],
        cosigner_threshold: u32,
        signature_algorithm: &str,
    ) -> Result<Vec<u8>, CosignedError> {
        let commitment = Self::envelope_commitment_for(data, signers, cosigner_threshold)?;
        Self::envelope_signature_hash_for_commitment(&commitment, signature_algorithm)
    }

    pub fn envelope_signing_hash_for_presence(
        data: &A,
        signers: &[Cosigner],
        cosigner_threshold: u32,
        bitmap: &[u8],
        signature_algorithm: &str,
    ) -> Result<Vec<u8>, CosignedError> {
        let commitment =
            Self::envelope_commitment_for_presence(data, signers, cosigner_threshold, bitmap)?;
        Self::envelope_signature_hash_for_commitment(&commitment, signature_algorithm)
    }

    fn envelope_signature_hash_for_commitment(
        commitment: &[u8],
        signature_algorithm: &str,
    ) -> Result<Vec<u8>, CosignedError> {
        let scheme_id = envelope_scheme_id(signature_algorithm)?;
        let mut message = Vec::new();
        append_commitment_field(&mut message, ENVELOPE_SIGNATURE_DOMAIN);
        message.extend_from_slice(&ENVELOPE_PROTOCOL_VERSION.to_be_bytes());
        message.extend_from_slice(&scheme_id.to_be_bytes());
        message.extend_from_slice(commitment);
        Ok(Signed::<A>::signature_hash(signature_algorithm, message))
    }

    pub fn from_envelope_signed_data(
        data: A,
        signers: Vec<Cosigner>,
    ) -> Result<Self, CosignedError> {
        let threshold = signers.len() as u32;
        Self::from_envelope_signed_data_threshold_inner(data, signers, threshold)
    }

    pub fn create_single_envelope(
        data: A,
        signature_algorithm: Box<dyn SignaturesAlg>,
        private_key: PrivateKey,
    ) -> Result<Self, CosignedError>
    where
        A: Clone,
    {
        let mut signer = Cosigner {
            pk: signature_algorithm.to_public(&private_key),
            sig: prost::bytes::Bytes::from_static(&[1]),
            sig_algorithm: signature_algorithm,
        };
        let signing_hash = Self::envelope_signing_hash(
            &data,
            std::slice::from_ref(&signer),
            1,
            &signer.sig_algorithm.name(),
        )?;
        signer.sig = signer
            .sig_algorithm
            .sign(&signing_hash, &private_key.bytes)
            .into();
        Self::from_envelope_signed_data_threshold(data, vec![signer], 1)
    }

    pub fn from_envelope_signed_data_threshold(
        data: A,
        signers: Vec<Cosigner>,
        threshold: u32,
    ) -> Result<Self, CosignedError> {
        Self::from_envelope_signed_data_threshold_inner(data, signers, threshold)
    }

    fn from_envelope_signed_data_threshold_inner(
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
        let canonical = Self::canonical_envelope_signers(&signers)?;
        let commitment = Self::envelope_commitment_for(&data, &canonical, threshold)?;
        let mut valid_signers = 0u32;
        for (index, signer) in canonical.iter().enumerate() {
            if signer.sig.is_empty() {
                continue;
            }
            let scheme_id = envelope_scheme_id(&signer.sig_algorithm.name())?;
            if !canonical_envelope_signature(scheme_id, &signer.sig) {
                return Err(CosignedError::NonCanonicalEnvelopeSignature { index });
            }
            let mut message = Vec::new();
            append_commitment_field(&mut message, ENVELOPE_SIGNATURE_DOMAIN);
            message.extend_from_slice(&ENVELOPE_PROTOCOL_VERSION.to_be_bytes());
            message.extend_from_slice(&scheme_id.to_be_bytes());
            message.extend_from_slice(&commitment);
            let hash = Signed::<A>::signature_hash(&signer.sig_algorithm.name(), message);
            if !signer
                .sig_algorithm
                .verify(&hash, &signer.sig, &signer.pk.bytes)
            {
                return Err(CosignedError::SignatureVerifyFailed {
                    index,
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
        Ok(Self {
            data,
            signers: canonical,
            cosigner_threshold: threshold,
            signing_domain: CosignedSigningDomain::EnvelopeV6,
        })
    }

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
        // most; Keccak256 with Ethereum prefix for secp256k1:eth; etc.).
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
            signing_domain: CosignedSigningDomain::LegacyPayload,
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
            signing_domain: CosignedSigningDomain::LegacyPayload,
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
            signing_domain: CosignedSigningDomain::LegacyPayload,
        })
    }

    pub fn is_envelope_bound(&self) -> bool {
        self.signing_domain == CosignedSigningDomain::EnvelopeV6
    }

    pub fn presence_bitmap_v61(&self) -> Result<Vec<u8>, CosignedError> {
        if !self.is_envelope_bound() {
            return Err(CosignedError::LegacyEnvelopeCommitmentUnavailable);
        }
        Ok(presence_bitmap(&self.signers))
    }

    pub fn selected_signers_v61(&self) -> Result<Vec<&Cosigner>, CosignedError> {
        if !self.is_envelope_bound() {
            return Err(CosignedError::LegacyEnvelopeCommitmentUnavailable);
        }
        Ok(self
            .signers
            .iter()
            .filter(|signer| !signer.sig.is_empty())
            .collect())
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
        fn envelope_intent_v61(&self) -> Result<Vec<u8>, String> { Ok(self.encode_to_vec()) }
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
        let cosigned_a = Cosigned::from_signed_data(payload.clone(), vec![s1.clone(), s2.clone()])
            .expect("valid");
        let cosigned_b = Cosigned::from_signed_data(payload.clone(), vec![s2, s1]).expect("valid");
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
        let err = Cosigned::from_signed_data(payload, vec![s1, s1_clone]).expect_err("must reject");
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

    fn fresh_signer_for(payload: &TestPayload) -> Cosigner { fresh_cosigner(payload) }

    fn empty_placeholder_signer() -> Cosigner {
        let secp = Secp256k1;
        let (_, pk) = secp.new_key_pair();
        Cosigner {
            pk,
            sig: prost::bytes::Bytes::new(),
            sig_algorithm: Box::new(secp),
        }
    }

    fn v61_signers(
        payload: &TestPayload,
        member_count: usize,
        selected: &[usize],
        threshold: u32,
    ) -> (Vec<Cosigner>, Vec<PrivateKey>) {
        let secp = Secp256k1;
        let mut members = (0..member_count)
            .map(|_| {
                let (private_key, public_key) = secp.new_key_pair();
                (
                    Cosigner {
                        pk: public_key,
                        sig: prost::bytes::Bytes::new(),
                        sig_algorithm: Box::new(secp.clone()),
                    },
                    private_key,
                )
            })
            .collect::<Vec<_>>();
        members.sort_by_key(|(signer, _)| signer.principal_bytes_v61().unwrap());
        let mut bitmap = vec![0u8; member_count.div_ceil(8)];
        for index in selected {
            bitmap[index / 8] |= 1 << (index % 8);
        }
        let unsigned = members
            .iter()
            .map(|(signer, _)| signer.clone())
            .collect::<Vec<_>>();
        for index in selected {
            let hash = Cosigned::<TestPayload>::envelope_signing_hash_for_presence(
                payload,
                &unsigned,
                threshold,
                &bitmap,
                &members[*index].0.sig_algorithm.name(),
            )
            .unwrap();
            members[*index].0.sig = members[*index]
                .0
                .sig_algorithm
                .sign(&hash, &members[*index].1.bytes)
                .into();
        }
        let (signers, private_keys): (Vec<_>, Vec<_>) = members.into_iter().unzip();
        (signers, private_keys)
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
    fn cosigned_threshold_rejects_empty_signer_list() {
        let payload = TestPayload {
            term: "threshold_empty".to_string(),
            nonce: 100,
        };
        let err = Cosigned::from_signed_data_threshold(payload, vec![], 1)
            .expect_err("empty threshold signer list must reject");
        assert!(matches!(err, CosignedError::EmptySignerList));
    }

    #[test]
    fn cosigned_threshold_rejects_duplicate_signer() {
        let payload = TestPayload {
            term: "threshold_duplicate".to_string(),
            nonce: 100,
        };
        let signer = fresh_signer_for(&payload);
        let err = Cosigned::from_signed_data_threshold(payload, vec![signer.clone(), signer], 1)
            .expect_err("duplicate threshold signer must reject");
        assert!(matches!(err, CosignedError::DuplicateSigner { .. }));
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

    #[test]
    fn envelope_v61_binds_the_selected_subset() {
        let payload = TestPayload {
            term: "selected-subset".to_string(),
            nonce: 1,
        };
        let (left_signers, _) = v61_signers(&payload, 3, &[0, 1], 2);
        let right_bitmap = [0b0000_0110];
        let left = Cosigned::from_envelope_signed_data_threshold(payload, left_signers.clone(), 2)
            .expect("valid v6.1 envelope");
        let right_commitment = Cosigned::<TestPayload>::envelope_commitment_for_presence(
            left.data(),
            &left_signers,
            2,
            &right_bitmap,
        )
        .unwrap();
        assert_ne!(left.envelope_commitment().unwrap(), right_commitment);
    }

    #[test]
    fn envelope_v61_binds_threshold_with_the_same_selected_subset() {
        let payload = TestPayload {
            term: "threshold-policy".to_string(),
            nonce: 2,
        };
        let (signers, _) = v61_signers(&payload, 3, &[0, 1], 2);
        let bitmap = [0b0000_0011];
        let threshold_one = Cosigned::<TestPayload>::envelope_commitment_for_presence(
            &payload, &signers, 1, &bitmap,
        )
        .unwrap();
        let threshold_two = Cosigned::<TestPayload>::envelope_commitment_for_presence(
            &payload, &signers, 2, &bitmap,
        )
        .unwrap();
        assert_ne!(threshold_one, threshold_two);
    }

    #[test]
    fn envelope_v61_selected_signers_exclude_unsigned_policy_members() {
        let payload = TestPayload {
            term: "no-unsigned-authority".to_string(),
            nonce: 3,
        };
        let (signers, _) = v61_signers(&payload, 3, &[1, 2], 2);
        let unsigned = signers[0].pk.clone();
        let envelope = Cosigned::from_envelope_signed_data_threshold(payload, signers, 2)
            .expect("valid v6.1 envelope");
        let selected = envelope.selected_signers_v61().unwrap();
        assert_eq!(selected.len(), 2);
        assert!(selected.iter().all(|signer| signer.pk != unsigned));
    }

    #[test]
    fn envelope_v61_rejects_legacy_payload_signatures() {
        let payload = TestPayload {
            term: "domain-separation".to_string(),
            nonce: 4,
        };
        let signer = fresh_cosigner(&payload);
        let result = Cosigned::from_envelope_signed_data_threshold(payload, vec![signer], 1);
        assert!(matches!(
            result,
            Err(CosignedError::SignatureVerifyFailed { .. })
                | Err(CosignedError::NonCanonicalEnvelopeSignature { .. })
        ));
    }
}
