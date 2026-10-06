use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlgFactory;
use crypto::rust::signatures::signed::{Signed, ToMessage};
use prost::Message;
use shared::rust::ByteVector;

use crate::casper::*;
use crate::rust::deploy_id::DeployIdV6;

type ByteString = prost::bytes::Bytes;

#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    Eq,
    Hash,
    utoipa::ToSchema
)]
pub struct DeployData {
    pub term: String,
    pub language: String,
    #[serde(rename = "timestamp")]
    pub time_stamp: i64,
    #[serde(rename = "validAfterBlockNumber")]
    pub valid_after_block_number: i64,
    #[serde(rename = "shardId")]
    pub shard_id: String,
    /// Optional millisecond timestamp after which deploy is invalid (None = no expiration)
    pub expiration_timestamp: Option<i64>,
    #[serde(default, rename = "authorityPresentations")]
    pub authority_presentations: Vec<crate::rhoapi::CostSignature>,
}

impl ToMessage for DeployData {
    type Type = DeployDataProto;
    fn to_message(&self) -> Self::Type { DeployData::_to_proto(self.clone()) }
    fn envelope_intent_v61(&self) -> Result<Vec<u8>, String> {
        self.validate_authority_presentations()?;
        if self.language != "rholang" {
            return Err("protocol-v6 deploy language must be rholang".to_string());
        }
        let timestamp = u64::try_from(self.time_stamp)
            .map_err(|_| "protocol-v6 deploy timestamp must be nonnegative".to_string())?;
        let valid_after = u64::try_from(self.valid_after_block_number)
            .map_err(|_| "protocol-v6 valid-after block must be nonnegative".to_string())?;
        if self.shard_id.is_empty() {
            return Err("protocol-v6 shard ID must be nonempty".to_string());
        }
        let mut intent = Vec::new();
        intent.extend_from_slice(&1u16.to_be_bytes());
        intent.push(1);
        append_deploy_intent_field(&mut intent, self.term.as_bytes());
        intent.extend_from_slice(&timestamp.to_be_bytes());
        intent.extend_from_slice(&valid_after.to_be_bytes());
        append_deploy_intent_field(&mut intent, self.shard_id.as_bytes());
        match self.expiration_timestamp {
            None => intent.push(0),
            Some(expiration) if expiration > 0 => {
                intent.push(1);
                intent.extend_from_slice(&(expiration as u64).to_be_bytes());
            }
            Some(_) => {
                return Err("protocol-v6 expiration timestamp must be positive".to_string());
            }
        }
        intent.extend_from_slice(&(self.authority_presentations.len() as u32).to_be_bytes());
        for presentation in &self.authority_presentations {
            append_deploy_intent_field(&mut intent, &canonical_cost_signature_bytes(presentation)?);
        }
        Ok(intent)
    }
}

fn append_deploy_intent_field(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    output.extend_from_slice(bytes);
}

fn canonical_cost_signature_bytes(
    signature: &crate::rhoapi::CostSignature,
) -> Result<Vec<u8>, String> {
    use crate::rhoapi::cost_signature::Value;
    use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
    use crate::rust::rholang::sorter::sortable::Sortable;

    let mut encoded = Vec::new();
    match signature.value.as_ref() {
        Some(Value::Unit(true)) => encoded.push(0),
        Some(Value::Ground(bytes)) => {
            encoded.push(1);
            append_deploy_intent_field(&mut encoded, bytes);
        }
        Some(Value::Quote(par)) => {
            let canonical = ParSortMatcher::sort_match(par).term;
            if canonical != *par {
                return Err("authority quote must contain a canonical process".to_string());
            }
            encoded.push(2);
            append_deploy_intent_field(&mut encoded, &canonical.encode_to_vec());
        }
        Some(Value::Name(par)) => {
            let canonical = ParSortMatcher::sort_match(par).term;
            if canonical != *par {
                return Err("authority name must contain a canonical process".to_string());
            }
            encoded.push(3);
            append_deploy_intent_field(&mut encoded, &canonical.encode_to_vec());
        }
        Some(Value::Compound(compound)) => {
            let mut children = Vec::new();
            for child in &compound.elements {
                match child.value.as_ref() {
                    Some(Value::Compound(_)) | Some(Value::Unit(_)) => {
                        return Err(
                            "authority compound must be flat and contain no unit".to_string()
                        );
                    }
                    _ => children.push(canonical_cost_signature_bytes(child)?),
                }
            }
            if children.len() < 2 {
                return Err("authority compound must contain at least two elements".to_string());
            }
            let supplied = children.clone();
            children.sort();
            if children != supplied {
                return Err("authority compound elements must be canonically ordered".to_string());
            }
            encoded.push(4);
            encoded.extend_from_slice(&(children.len() as u32).to_be_bytes());
            for child in children {
                append_deploy_intent_field(&mut encoded, &child);
            }
        }
        Some(Value::BoundLevel(_)) => {
            return Err(
                "authority presentation contains an unresolved bound signature".to_string(),
            );
        }
        Some(Value::Unit(false)) => {
            return Err("authority presentation contains a false unit".to_string());
        }
        None => return Err("authority presentation is missing its signature".to_string()),
    }
    Ok(encoded)
}

/// Internal helper for walking a `SigCompound` expression and collecting
/// atomic signer leaves. Used exclusively by
/// [`DeployData::from_proto_cosigned_with_sig_algebra`] and its callees.
#[derive(Clone, Debug)]
struct AlgebraAtom {
    pk: Vec<u8>,
    sig: prost::bytes::Bytes,
    sig_algorithm: String,
}

struct FundingAlgebraAnalysis {
    min_required: u32,
    all_required: bool,
}

impl AlgebraAtom {
    fn from_proto(atom: &crate::casper::SigAtom) -> Self {
        Self {
            pk: atom.pk.to_vec(),
            sig: atom.sig.clone(),
            sig_algorithm: atom.sig_algorithm.clone(),
        }
    }
}

impl DeployData {
    // D3 (DR-9): the singular-phlo escrow/price arithmetic
    // (`checked_total_phlo_charge[_value]`, `total_phlo_charge`,
    // `refund_amount_for_token_cost[_value]`, `validate_phlo`) is REMOVED. A
    // deploy's cost is the per-COMM token count (computed by the runtime); it is
    // funded by canonical SystemVault custody and prepaid located stacks and is
    // gated at block assembly (`casper/.../util/rholang/acceptance.rs`). There is
    // no client-supplied phlo limit or price and no legacy escrow refund.

    /// Returns true if this deploy has a time-based expiration set
    pub fn has_expiration(&self) -> bool {
        self.expiration_timestamp
            .map(|exp| exp > 0)
            .unwrap_or(false)
    }

    /// Returns true if this deploy has expired at the given time
    pub fn is_expired_at(&self, current_time_millis: i64) -> bool {
        self.expiration_timestamp
            .map(|exp| current_time_millis > exp)
            .unwrap_or(false)
    }

    pub fn encode(a: DeployData) -> ByteVector { DeployData::_to_proto(a).encode_to_vec() }

    pub fn decode(a: ByteVector) -> Result<DeployData, String> {
        let proto = DeployDataProto::decode(&a[..])
            .map_err(|e| format!("Failed to decode DeployData: {}", e))?;
        Self::reject_funding_intent(&proto)?;
        let data = DeployData::_from_proto(proto);
        data.validate_authority_presentations()?;
        Ok(data)
    }

    pub(crate) fn _from_proto(proto: DeployDataProto) -> Self {
        Self {
            term: proto.term,
            language: proto.language,
            time_stamp: proto.timestamp,
            valid_after_block_number: proto.valid_after_block_number,
            shard_id: proto.shard_id,
            // 0 in protobuf means not set, convert to None
            expiration_timestamp: if proto.expiration_timestamp == 0 {
                None
            } else {
                Some(proto.expiration_timestamp)
            },
            authority_presentations: proto.authority_presentations,
        }
    }

    fn validate_authority_presentations(&self) -> Result<(), String> {
        use crate::rhoapi::cost_signature::Value;
        use crate::rhoapi::CostSignature;
        use crate::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
        use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
        use crate::rust::rholang::sorter::sortable::Sortable;

        fn validate(signature: &CostSignature) -> Result<(), String> {
            match signature.value.as_ref() {
                Some(Value::Ground(_)) | Some(Value::Unit(true)) => Ok(()),
                Some(Value::Quote(par)) | Some(Value::Name(par))
                    if ParSortMatcher::sort_match(par).term == *par =>
                {
                    Ok(())
                }
                Some(Value::Compound(compound)) if compound.elements.len() >= 2 => {
                    compound.elements.iter().try_for_each(validate)
                }
                Some(Value::BoundLevel(_)) => {
                    Err("authority presentation contains an unresolved bound signature".to_string())
                }
                Some(Value::Compound(_)) => Err(
                    "authority presentation contains a malformed compound signature".to_string(),
                ),
                Some(Value::Unit(false)) | Some(Value::Quote(_)) | Some(Value::Name(_)) => {
                    Err("authority presentation contains a non-canonical signature".to_string())
                }
                None => Err("authority presentation is missing its signature".to_string()),
            }
        }

        let mut previous: Option<Vec<u8>> = None;
        for signature in &self.authority_presentations {
            let canonical = sort_signature(signature).term;
            validate(&canonical)?;
            if &canonical != signature {
                return Err("authority presentations must contain canonical signatures".to_string());
            }
            let encoded = canonical_cost_signature_bytes(&canonical)?;
            if previous.as_ref().is_some_and(|prior| prior >= &encoded) {
                return Err(
                    "authority presentations must be strictly ordered and unique".to_string(),
                );
            }
            previous = Some(encoded);
        }
        Ok(())
    }

    /// Primary-signer-only decode. Returns `Signed<DeployData>` constructed
    /// from the primary signer's fields (`deployer`, `sig`, `sig_algorithm`)
    /// regardless of whether the wire deploy carries cosigners. Callers that
    /// need the full multi-signature envelope MUST use
    /// [`Self::from_proto_cosigned`].
    pub fn from_proto(proto: DeployDataProto) -> Result<Signed<DeployData>, String> {
        Self::reject_funding_intent(&proto)?;
        let algorithm = SignaturesAlgFactory::apply(&proto.sig_algorithm)
            .ok_or_else(|| format!("Unknown signature algorithm: {}", proto.sig_algorithm))?;

        let sig = proto.sig.clone();
        let pk = PublicKey::from_bytes(&proto.deployer);
        let data = DeployData::_from_proto(proto);
        data.validate_authority_presentations()?;
        let signed = Signed::from_signed_data(data, pk, sig, algorithm)?;

        match signed {
            Some(signed) => Ok(signed),
            None => Err("Invalid signature".to_string()),
        }
    }

    /// Multi-signature aware decode. Returns a [`Cosigned<DeployData>`]
    /// envelope covering both legacy single-sig wire deploys (`cosigners`
    /// empty → one-element envelope) and multi-sig wire deploys (`cosigners`
    /// non-empty → N-element envelope).
    ///
    /// Invariants enforced by `Cosigned::from_signed_data` at construction:
    /// 1. All signers' signatures verify against the canonical message hash.
    /// 2. Canonical pk-ascending sort; no duplicates.
    ///
    /// D3 (DR-9): there is no per-signer `phlo_share` and no share-sum
    /// invariant — authority is resolved from canonical vault custody and
    /// prepaid located stacks.
    pub fn from_proto_cosigned_legacy(
        proto: DeployDataProto,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        Self::reject_funding_intent(&proto)?;
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        if !proto.deploy_id.is_empty() || proto.authorization_v61.is_some() {
            return Err(
                "legacy deploy cannot contain protocol-v6 authorization fields".to_string(),
            );
        }

        if let Some(sig_algebra) = proto.sig_algebra.clone() {
            let data = DeployData::_from_proto(proto);
            data.validate_authority_presentations()?;
            return Self::from_proto_cosigned_with_sig_algebra(data, &sig_algebra);
        }

        let is_multi_sig = !proto.cosigners.is_empty();
        let cosigner_threshold = proto.cosigner_threshold;
        let total_signers = 1 + proto.cosigners.len();
        if cosigner_threshold < 0 || cosigner_threshold as usize > total_signers {
            return Err(format!(
                "Invalid cosigner_threshold {}: must satisfy 0 ≤ threshold ≤ {} (total signers)",
                cosigner_threshold, total_signers
            ));
        }

        let primary_alg = SignaturesAlgFactory::apply(&proto.sig_algorithm).ok_or_else(|| {
            format!(
                "Unknown primary signature algorithm: {}",
                proto.sig_algorithm
            )
        })?;

        // Build the canonical signer list. Primary first (will be sorted
        // canonically by Cosigned::from_signed_data). D3 (DR-9): no per-signer
        // phlo_share — funding follows the verified authority algebra.
        let mut signers = Vec::with_capacity(1 + proto.cosigners.len());
        signers.push(Cosigner {
            pk: PublicKey::from_bytes(&proto.deployer),
            sig: proto.sig.clone(),
            sig_algorithm: primary_alg,
        });
        for cs in &proto.cosigners {
            let alg = SignaturesAlgFactory::apply(&cs.sig_algorithm).ok_or_else(|| {
                format!(
                    "Unknown cosigner signature algorithm: {} for cosigner pk={}",
                    cs.sig_algorithm,
                    hex::encode(&cs.pk)
                )
            })?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&cs.pk),
                sig: cs.sig.clone(),
                sig_algorithm: alg,
            });
        }

        let data = DeployData::_from_proto(proto);
        data.validate_authority_presentations()?;
        if cosigner_threshold == 0 {
            Cosigned::from_signed_data(data, signers).map_err(|e| {
                format!(
                    "Cosigned envelope validation failed (is_multi_sig={}): {}",
                    is_multi_sig, e
                )
            })
        } else {
            Cosigned::from_signed_data_threshold(data, signers, cosigner_threshold as u32)
                .map_err(|e| {
                    format!(
                        "Cosigned threshold envelope validation failed (threshold={}, total_signers={}): {}",
                        cosigner_threshold, total_signers, e
                    )
                })
        }
    }

    pub fn from_proto_cosigned(
        proto: DeployDataProto,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        Self::reject_funding_intent(&proto)?;
        Self::from_proto_envelope(proto, 0x0006_0001, |proto| {
            let data = Self::_from_proto(proto);
            data.validate_authority_presentations()?;
            Ok(data)
        })
    }

    fn reject_funding_intent(proto: &DeployDataProto) -> Result<(), String> {
        Self::reject_phlo_offer(proto)?;
        if proto.funding_intent.is_some()
            || proto
                .authorization_v61
                .as_ref()
                .is_some_and(|auth| auth.format_version != 0x0006_0001)
        {
            return Err(
                "funded deploy requires its explicit authorization decoder and execution policy"
                    .to_string(),
            );
        }
        Ok(())
    }

    pub(crate) fn reject_phlo_offer(proto: &DeployDataProto) -> Result<(), String> {
        if proto.phlo_price != 0 || proto.phlo_limit != 0 {
            return Err("phlo offer requires its explicit authorization decoder".to_string());
        }
        Ok(())
    }

    pub(crate) fn from_proto_envelope<A: std::fmt::Debug + serde::Serialize + ToMessage>(
        proto: DeployDataProto,
        format_version: u32,
        decode: impl FnOnce(DeployDataProto) -> Result<A, String>,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<A>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        use crate::casper::authorization_policy_v61::Policy;

        if proto.deploy_id.len() != DeployIdV6::LENGTH {
            return Err("protocol-v6 deploy requires a 32-byte DeployId".to_string());
        }
        if !proto.deployer.is_empty()
            || !proto.sig.is_empty()
            || !proto.sig_algorithm.is_empty()
            || !proto.cosigners.is_empty()
            || proto.cosigner_threshold != 0
            || proto.sig_algebra.is_some()
        {
            return Err(
                "protocol-v6 deploy cannot contain legacy authorization fields".to_string(),
            );
        }
        let authorization = proto
            .authorization_v61
            .as_ref()
            .ok_or_else(|| "protocol-v6 deploy authorization is missing".to_string())?;
        if authorization.format_version != format_version {
            return Err(
                "protocol-v6 deploy authorization format does not match its decoder".to_string(),
            );
        }
        let policy = authorization
            .policy
            .as_ref()
            .and_then(|policy| policy.policy.as_ref())
            .ok_or_else(|| "protocol-v6 deploy authorization policy is missing".to_string())?;
        let (members, threshold) = match policy {
            Policy::AllOf(policy) => {
                let count = u32::try_from(policy.members.len())
                    .map_err(|_| "protocol-v6 signer count exceeds u32".to_string())?;
                if count == 0 {
                    return Err("protocol-v6 AllOf policy must contain members".to_string());
                }
                (&policy.members, count)
            }
            Policy::Threshold(policy) => {
                let count = u32::try_from(policy.members.len())
                    .map_err(|_| "protocol-v6 signer count exceeds u32".to_string())?;
                if policy.minimum == 0 || policy.minimum >= count {
                    return Err("protocol-v6 threshold must satisfy 1 <= k < N".to_string());
                }
                (&policy.members, policy.minimum)
            }
        };
        let expected_bitmap_len = members.len().div_ceil(8);
        if authorization.presence_bitmap.len() != expected_bitmap_len
            || authorization.presence_bitmap.last().is_some_and(|last| {
                let used = members.len() % 8;
                used != 0 && *last & !((1u8 << used) - 1) != 0
            })
        {
            return Err("protocol-v6 presence bitmap is not canonical".to_string());
        }
        let selected_indices = authorization
            .presence_bitmap
            .iter()
            .enumerate()
            .flat_map(|(byte_index, byte)| {
                (0..8).filter_map(move |bit| {
                    ((*byte & (1 << bit)) != 0).then_some(byte_index * 8 + bit)
                })
            })
            .filter(|index| *index < members.len())
            .collect::<Vec<_>>();
        if authorization.witnesses.len() != selected_indices.len()
            || authorization
                .witnesses
                .iter()
                .zip(&selected_indices)
                .any(|(witness, expected)| {
                    witness.signature.is_empty() || witness.member_index as usize != *expected
                })
        {
            return Err(
                "protocol-v6 witnesses do not exactly match the presence bitmap".to_string(),
            );
        }
        let mut witness_iter = authorization.witnesses.iter().peekable();
        let signers = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                let algorithm_name = match SignatureSchemeV61::try_from(member.scheme)
                    .unwrap_or(SignatureSchemeV61::Unspecified)
                {
                    SignatureSchemeV61::Secp256k1 => "secp256k1",
                    SignatureSchemeV61::Secp256k1Eth => "secp256k1:eth",
                    _ => return Err("protocol-v6 signature scheme is not active".to_string()),
                };
                let signature = if witness_iter
                    .peek()
                    .is_some_and(|witness| witness.member_index as usize == index)
                {
                    witness_iter
                        .next()
                        .expect("peeked witness")
                        .signature
                        .clone()
                } else {
                    ByteString::new()
                };
                Ok(Cosigner {
                    pk: PublicKey::from_bytes(&member.public_key),
                    sig: signature,
                    sig_algorithm: SignaturesAlgFactory::apply(algorithm_name)
                        .expect("active protocol-v6 signature scheme"),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Cosigned::<A>::validate_envelope_signer_order(&signers)
            .map_err(|error| format!("protocol-v6 envelope validation failed: {error}"))?;
        let expected_commitment = proto.deploy_id.clone();
        let data = decode(proto)?;
        let envelope = Cosigned::from_envelope_signed_data_threshold(data, signers, threshold)
            .map_err(|error| format!("protocol-v6 envelope validation failed: {error}"))?;
        if envelope
            .envelope_commitment()
            .map_err(|error| format!("protocol-v6 envelope validation failed: {error}"))?
            != expected_commitment
        {
            return Err("protocol-v6 DeployId mismatch".to_string());
        }
        Ok(envelope)
    }

    /// Validates the admission algebra accepted at the deploy boundary. `Atom`
    /// and `Tensor` realize the funding-signature grammar. A top-level
    /// `Threshold` realizes a k-of-N admission quorum whose members must be
    /// atomic candidate signers. Thresholds nested under `Tensor` are rejected
    /// because the flat `Cosigned` threshold cannot preserve that formula.
    /// Capability connectives are rejected before the algebra is lowered to a
    /// canonical `Cosigned` envelope.
    pub fn from_proto_cosigned_with_sig_algebra(
        data: DeployData,
        sig_algebra: &crate::casper::SigCompound,
    ) -> Result<crypto::rust::signatures::signed::Cosigned<DeployData>, String> {
        use crypto::rust::signatures::signed::{Cosigned, Cosigner};

        data.validate_authority_presentations()?;
        let mut atoms: Vec<AlgebraAtom> = Vec::new();
        let analysis = Self::analyze_funding_algebra(sig_algebra, &mut atoms)?;

        let mut signers: Vec<Cosigner> = Vec::with_capacity(atoms.len());
        for atom in atoms.into_iter() {
            let alg = SignaturesAlgFactory::apply(&atom.sig_algorithm)
                .ok_or_else(|| format!("Unknown signature algorithm: {}", atom.sig_algorithm))?;
            signers.push(Cosigner {
                pk: PublicKey::from_bytes(&atom.pk),
                sig: atom.sig,
                sig_algorithm: alg,
            });
        }

        let total = signers.len() as u32;
        if analysis.all_required {
            debug_assert_eq!(analysis.min_required, total);
            Cosigned::from_signed_data(data, signers)
                .map_err(|e| format!("Cosigned sig_algebra validation failed: {}", e))
        } else {
            Cosigned::from_signed_data_threshold(data, signers, analysis.min_required).map_err(
                |e| {
                    format!(
                        "Cosigned sig_algebra threshold validation failed (min_required={}): {}",
                        analysis.min_required, e
                    )
                },
            )
        }
    }

    fn analyze_funding_algebra(
        sig: &crate::casper::SigCompound,
        atoms: &mut Vec<AlgebraAtom>,
    ) -> Result<FundingAlgebraAnalysis, String> {
        use crate::casper::sig_compound::Connective;
        let connective = sig
            .connective
            .as_ref()
            .ok_or_else(|| "SigCompound.connective missing".to_string())?;
        match connective {
            Connective::Atom(atom) => {
                atoms.push(AlgebraAtom::from_proto(atom));
                Ok(FundingAlgebraAnalysis {
                    min_required: 1,
                    all_required: true,
                })
            }
            Connective::Tensor(pair) => {
                let left = pair
                    .left
                    .as_deref()
                    .ok_or_else(|| "SigPair.left missing".to_string())?;
                let right = pair
                    .right
                    .as_deref()
                    .ok_or_else(|| "SigPair.right missing".to_string())?;
                let left = Self::analyze_funding_algebra(left, atoms)?;
                let right = Self::analyze_funding_algebra(right, atoms)?;
                if !left.all_required || !right.all_required {
                    return Err(
                        "SigThreshold must be the top-level admission connective; a scalar signer threshold cannot preserve Tensor composition"
                            .to_string(),
                    );
                }
                Ok(FundingAlgebraAnalysis {
                    min_required: left
                        .min_required
                        .checked_add(right.min_required)
                        .ok_or_else(|| "Funding algebra signer count overflow".to_string())?,
                    all_required: left.all_required && right.all_required,
                })
            }
            Connective::Threshold(threshold) => {
                if threshold.threshold < 1
                    || (threshold.threshold as usize) > threshold.members.len()
                {
                    return Err(format!(
                        "SigThreshold.threshold must satisfy 1 ≤ threshold ≤ members.len() ({}), got {}",
                        threshold.members.len(), threshold.threshold
                    ));
                }
                for member in &threshold.members {
                    match member.connective.as_ref() {
                        Some(Connective::Atom(atom)) => atoms.push(AlgebraAtom::from_proto(atom)),
                        Some(Connective::Plus(_)) => {
                            return Err(Self::capability_connective_error("⊕", "Plus"));
                        }
                        Some(Connective::With(_)) => {
                            return Err(Self::capability_connective_error("&", "With"));
                        }
                        Some(Connective::Bang(_)) => {
                            return Err(Self::capability_connective_error("!", "Bang"));
                        }
                        Some(Connective::Whynot(_)) => {
                            return Err(Self::capability_connective_error("?", "WhyNot"));
                        }
                        Some(Connective::Lolly(_)) => {
                            return Err(Self::capability_connective_error("⊸", "Lolly"));
                        }
                        Some(Connective::Tensor(_)) | Some(Connective::Threshold(_)) => {
                            return Err(
                                "SigThreshold members must be atomic candidate signers".to_string()
                            );
                        }
                        None => return Err("SigThreshold member connective missing".to_string()),
                    }
                }
                Ok(FundingAlgebraAnalysis {
                    min_required: threshold.threshold as u32,
                    all_required: false,
                })
            }
            Connective::Plus(_) => Err(Self::capability_connective_error("⊕", "Plus")),
            Connective::With(_) => Err(Self::capability_connective_error("&", "With")),
            Connective::Bang(_) => Err(Self::capability_connective_error("!", "Bang")),
            Connective::Whynot(_) => Err(Self::capability_connective_error("?", "WhyNot")),
            Connective::Lolly(_) => Err(Self::capability_connective_error("⊸", "Lolly")),
        }
    }

    fn capability_connective_error(symbol: &str, name: &str) -> String {
        format!(
            "value/capability connective {symbol} ({name}) is not a funding-signature former (cost-accounted-rho §App-A: funding signatures are g | #P | s∘s — ground/quote atoms folded by the tensor ∘; value/capability connectives ⊕/&/!/?/⊸ are capability-layer only)"
        )
    }

    fn _to_proto(dd: DeployData) -> DeployDataProto {
        DeployDataProto {
            term: dd.term,
            language: String::new(),
            timestamp: dd.time_stamp,
            valid_after_block_number: dd.valid_after_block_number,
            shard_id: dd.shard_id,
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.expiration_timestamp.unwrap_or(0),
            authority_presentations: dd.authority_presentations,
            ..Default::default()
        }
    }

    pub fn to_proto(dd: Signed<DeployData>) -> DeployDataProto { Self::to_proto_ref(&dd) }

    pub fn to_proto_ref(dd: &Signed<DeployData>) -> DeployDataProto {
        DeployDataProto {
            term: dd.data.term.clone(),
            language: dd.data.language.clone(),
            timestamp: dd.data.time_stamp,
            valid_after_block_number: dd.data.valid_after_block_number,
            shard_id: dd.data.shard_id.clone(),
            deployer: dd.pk.bytes.clone().into(),
            sig: dd.sig.clone().into(),
            sig_algorithm: dd.sig_algorithm.name(),
            // Only include expirationTimestamp if set to maintain backward compatibility
            expiration_timestamp: dd.data.expiration_timestamp.unwrap_or(0),
            authority_presentations: dd.data.authority_presentations.clone(),
            ..Default::default()
        }
    }

    /// Serialize a [`Cosigned<DeployData>`] back to [`DeployDataProto`] wire
    /// format. For single-signer cosigned envelopes the output is
    /// byte-identical to `to_proto(signed)` (cosigners empty). For
    /// multi-signer envelopes the additional cosigners populate the
    /// `cosigners[]` field. D3 (DR-9): no per-signer phlo_share.
    pub fn to_proto_cosigned(
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> DeployDataProto {
        if !cosigned.is_envelope_bound() {
            let primary = cosigned.primary();
            return DeployDataProto {
                term: cosigned.data.term.clone(),
                language: cosigned.data.language.clone(),
                timestamp: cosigned.data.time_stamp,
                valid_after_block_number: cosigned.data.valid_after_block_number,
                shard_id: cosigned.data.shard_id.clone(),
                deployer: primary.pk.bytes.clone().into(),
                sig: primary.sig.clone(),
                sig_algorithm: primary.sig_algorithm.name(),
                expiration_timestamp: cosigned.data.expiration_timestamp.unwrap_or(0),
                authority_presentations: cosigned.data.authority_presentations.clone(),
                cosigners: cosigned
                    .signers()
                    .iter()
                    .skip(1)
                    .map(|signer| crate::casper::CompoundSigner {
                        pk: signer.pk.bytes.clone().into(),
                        sig: signer.sig.clone(),
                        sig_algorithm: signer.sig_algorithm.name(),
                    })
                    .collect(),
                cosigner_threshold: i32::try_from(cosigned.cosigner_threshold())
                    .unwrap_or(i32::MAX),
                ..Default::default()
            };
        }

        let proto = DeployDataProto {
            term: cosigned.data.term.clone(),
            language: cosigned.data.language.clone(),
            timestamp: cosigned.data.time_stamp,
            valid_after_block_number: cosigned.data.valid_after_block_number,
            shard_id: cosigned.data.shard_id.clone(),
            expiration_timestamp: cosigned.data.expiration_timestamp.unwrap_or(0),
            authority_presentations: cosigned.data.authority_presentations.clone(),
            ..Default::default()
        };
        Self::to_proto_envelope(cosigned, proto, 0x0006_0001)
            .expect("envelope-bound Cosigned invariant")
    }

    pub(crate) fn to_proto_envelope<A: std::fmt::Debug + serde::Serialize + ToMessage>(
        cosigned: &crypto::rust::signatures::signed::Cosigned<A>,
        mut proto: DeployDataProto,
        format_version: u32,
    ) -> Result<DeployDataProto, String> {
        let commitment = cosigned
            .envelope_commitment()
            .map_err(|error| error.to_string())?;
        use crate::casper::authorization_policy_v61::Policy;
        let members = cosigned
            .signers()
            .iter()
            .map(|signer| crate::casper::PrincipalV61 {
                scheme: i32::from(signer.scheme_id_v61().expect("validated v6.1 scheme")),
                public_key: signer.pk.bytes.clone().into(),
            })
            .collect::<Vec<_>>();
        let threshold = cosigned.cosigner_threshold();
        let policy = if threshold == members.len() as u32 {
            Policy::AllOf(crate::casper::AllOfPolicyV61 { members })
        } else {
            Policy::Threshold(crate::casper::ThresholdPolicyV61 {
                minimum: threshold,
                members,
            })
        };
        let witnesses = cosigned
            .signers()
            .iter()
            .enumerate()
            .filter(|(_, signer)| !signer.sig.is_empty())
            .map(|(index, signer)| crate::casper::SignatureWitnessV61 {
                member_index: index as u32,
                signature: signer.sig.clone(),
            })
            .collect();
        proto.deploy_id = commitment;
        proto.authorization_v61 = Some(crate::casper::DeployAuthorizationV61 {
            format_version,
            policy: Some(crate::casper::AuthorizationPolicyV61 {
                policy: Some(policy),
            }),
            presence_bitmap: cosigned
                .presence_bitmap_v61()
                .expect("envelope-bound Cosigned invariant")
                .into(),
            witnesses,
        });
        Ok(proto)
    }
}
