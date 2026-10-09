// DR-116 (gap G6): protocol-6 offered test deploys.
//
// The builders keep the signatures of `casper::rust::util::construct_deploy`,
// so a test moves from legacy deploys to offered envelopes by importing this
// module in place of `construct_deploy`. `TestNode` accepts both formats.
//
// Each offer is owner-direct: the signer's own vault funds the signer's own
// principal resources. The genesis vaults (`DEFAULT_SEC`, `DEFAULT_SEC2` and
// the extra vault keys) are funded, so they can sign offers.

use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::errors::CasperError;
pub use casper::rust::util::construct_deploy::{DEFAULT_SEC, DEFAULT_SEC2};
use casper::rust::util::rholang::costacc::vault_payer::vault_payer;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Cosigned;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};

/// The shard of every test genesis and test node.
pub const TEST_SHARD: &str = "root";

/// The phlo limit that each offer signs. It covers the test terms with a wide
/// margin, and a genesis vault funds it.
const OFFER_PHLO_LIMIT: u64 = 100_000_000;

/// The source cap of each offer, in atomic REV.
const OFFER_SOURCE_CAP: u64 = 400_000_000;

/// The resource schedule of the test genesis policy.
pub fn selected_schedule() -> PhloScheduleV1<'static> {
    let identities: [&[u8]; 4] = [b"compute", b"introduction", b"transfer", b"trace"];
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(identities)
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// The genesis resource policy that activates offered-funded protocol 6.
pub fn offered_v6_policy() -> PhloGenesisPolicy {
    PhloGenesisPolicy::from_schedule(&selected_schedule())
        .expect("the selected schedule forms a genesis policy")
        .with_offered_funded_v6_active()
}

/// An owner-direct offer: the owner's vault funds only the owner's own
/// principal resources.
pub fn owner_offer(
    owner: &(PrivateKey, PublicKey),
    time_stamp: i64,
    term: String,
    valid_after_block_number: i64,
    shard_id: String,
) -> models::casper::DeployDataProto {
    owner_offer_limited(
        owner,
        time_stamp,
        term,
        valid_after_block_number,
        shard_id,
        OFFER_PHLO_LIMIT,
    )
}

/// Added by DR-113: an owner-direct offer with the given signed phlo limit.
pub fn owner_offer_limited(
    owner: &(PrivateKey, PublicKey),
    time_stamp: i64,
    term: String,
    valid_after_block_number: i64,
    shard_id: String,
    phlo_limit: u64,
) -> models::casper::DeployDataProto {
    let (owner_secret, owner_public) = owner;
    let limits = offered_funded_v6_limits().envelope.payload;
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner_public.bytes))),
    };
    let payer = vault_payer(&signature).expect("a principal signature has a vault payer");
    let schedule = selected_schedule();
    let acquisition_terms = schedule
        .encode(PhloGenesisPolicy::LIMITS)
        .expect("the selected schedule encodes");
    let authority = cost_signature_to_sig(&signature).expect("a principal signature converts");
    let location = SignatureChannel::from_sig(&authority).par.encode_to_vec();
    let permissions = (0..schedule.classes.len())
        .map(|class| {
            PhloResource {
                location: &location,
                class,
                acquisition_terms: &acquisition_terms,
                authority: &authority,
            }
            .wire_key(PhloExecutionLimits {
                resource_entries: 1,
                authority_nodes: limits.funding.authority_nodes,
                key_bytes: limits.funding.wire.field_bytes,
            })
            .expect("a resource permission encodes")
        })
        .collect();
    let source = PhloSourcePolicyV1::new(
        &payer.custody_key,
        OFFER_SOURCE_CAP,
        OFFER_SOURCE_CAP,
        true,
        permissions,
        PhloSourceLimits {
            wire: limits.funding.wire,
            resource_permissions: schedule.classes.len(),
            authority_nodes: limits.funding.authority_nodes,
        },
    )
    .expect("the owner's source policy is valid");
    let funding = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: phlo_limit,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![schedule.clone()],
            },
            schedule_commitment: schedule
                .digest(PhloGenesisPolicy::LIMITS)
                .expect("the selected schedule digests"),
            total_exposure: u128::from(OFFER_SOURCE_CAP),
            sources: vec![source],
        },
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(PhloFundingIntentV2Limits {
        wire: limits.funding.wire,
        base: limits.funding,
        grant_uses: limits.funding.wire.total_bytes / 8,
        grant_id_bytes: limits.funding.wire.field_bytes,
        quote_evidence_bytes: limits.funding.wire.field_bytes,
    })
    .expect("the funding intent encodes");
    let body = DeployData {
        term,
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number,
        shard_id,
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(
        body,
        funding,
        i64::try_from(phlo_limit).expect("the offer phlo limit fits an i64"),
        2,
        limits,
    )
    .expect("the offered payload is valid");
    let signed =
        Cosigned::create_single_envelope(payload, Box::new(Secp256k1), owner_secret.clone())
            .expect("the owner signs the offer");
    OfferedFundedDeploy::to_proto(&signed).expect("a signed offer encodes")
}

/// An owner-direct offer at block 0 in the test shard.
pub fn owner_direct_offer(
    owner: &(PrivateKey, PublicKey),
    time_stamp: i64,
    term: String,
) -> models::casper::DeployDataProto {
    owner_offer(owner, time_stamp, term, 0, TEST_SHARD.to_string())
}

/// An offered test deploy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferedDeploy {
    /// The lookup identity of the deploy: the 32-byte protocol-6 deploy id.
    /// Casper keys deploy identities under the name `sig`, as
    /// `RejectedDeploy::sig` does, so a test reads both formats the same way.
    pub sig: Bytes,
    pub envelope: DeployEnvelope,
}

impl Hash for OfferedDeploy {
    fn hash<H: Hasher>(&self, state: &mut H) { self.sig.hash(state) }
}

impl OfferedDeploy {
    pub fn from_proto(proto: models::casper::DeployDataProto) -> Self {
        let envelope = DeployEnvelope::from_proto(proto, offered_funded_v6_limits().envelope)
            .expect("an offered test deploy decodes");
        Self {
            sig: envelope.identity().as_bytes().to_vec().into(),
            envelope,
        }
    }
}

/// The legacy builders take a phlo limit and price for the legacy precharge.
/// An offered deploy is charged its measured usage at the schedule price
/// under the limit that the offer signs, so the offered builders ignore both.
pub fn source_deploy(
    source: String,
    timestamp: i64,
    _phlo_limit: Option<i64>,
    _phlo_price: Option<i64>,
    sec: Option<PrivateKey>,
    valid_after_block_number: Option<i64>,
    shard_id: Option<String>,
) -> Result<OfferedDeploy, CasperError> {
    let secret = sec.unwrap_or_else(|| DEFAULT_SEC.clone());
    let public = Secp256k1.to_public(&secret);
    Ok(OfferedDeploy::from_proto(owner_offer(
        &(secret, public),
        timestamp,
        source,
        valid_after_block_number.unwrap_or(0),
        shard_id
            .filter(|shard| !shard.is_empty())
            .unwrap_or_else(|| TEST_SHARD.to_string()),
    )))
}

pub fn source_deploy_now(
    source: String,
    sec: Option<PrivateKey>,
    valid_after_block_number: Option<i64>,
    shard_id: Option<String>,
) -> Result<OfferedDeploy, CasperError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
    source_deploy(
        source,
        timestamp,
        None,
        None,
        sec,
        valid_after_block_number,
        shard_id,
    )
}

pub fn source_deploy_now_full(
    source: String,
    phlo_limit: Option<i64>,
    phlo_price: Option<i64>,
    sec: Option<PrivateKey>,
    valid_after_block_number: Option<i64>,
    shard_id: Option<String>,
) -> Result<OfferedDeploy, CasperError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
    source_deploy(
        source,
        timestamp,
        phlo_limit,
        phlo_price,
        sec,
        valid_after_block_number,
        shard_id,
    )
}

pub fn basic_deploy_data(
    id: i32,
    sec: Option<PrivateKey>,
    shard_id: Option<String>,
) -> Result<OfferedDeploy, CasperError> {
    source_deploy_now(format!("@{}!({})", id, id), sec, None, shard_id)
}
