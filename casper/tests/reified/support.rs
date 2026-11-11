//! Support of the N4 suite: offered v6 genesis, signed offers with a chosen term, the propose
//! and validate steps of the two- and three-node harness, the labeled signers of the merge row,
//! and the searched signers of the concurrent row, whose cost-cursor buckets are disjoint
//! (DR-119, decision 108).
//!
//! The schedule, the offer and the genesis follow `casper/tests/api/offered_funded_api_test.rs`
//! (`selected_schedule`, `signed_offer_at` and `offered_v6_genesis`, lines 136-256), whose
//! helpers are private to that file. Here the offer takes its term, time stamp, phlo limit and
//! exposure as arguments, because a space deploy needs its own term and phlo budget (P9 design
//! v2, section 8, amendment A2).

use casper::rust::api::block_api::BlockAPI;
use casper::rust::block_status::BlockError;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::util::rholang::costacc::vault_payer::vault_payer;
use crypto::rust::hash::blake2b256::Blake2b256;
// Changed by decision 108: the bucket search returns, because the merged base has the cost-cursor
// buckets of DR-119. Decision 84 had disabled this import.
use crypto::rust::hash::keccak256::Keccak256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Cosigned;
use models::casper::DeployDataProto;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
// Changed by decision 108: the bucket search returns, because the merged base has the cost-cursor
// buckets of DR-119. Decision 84 had disabled this import.
use rholang::rust::interpreter::accounting::monetary_allocation::monetary_scope_for_custodies;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};
// Changed by decision 108: the bucket search returns, because the merged base has the cost-cursor
// buckets of DR-119. Decision 84 had disabled this import.
use rholang::rust::interpreter::rho_type::RhoByteArray;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

/// The phlo price of every offer of the suite.
pub const PRICE: u64 = 2;

/// The shard of the suite.
pub const SHARD: &str = "root";

/// The fixed fee of an offer in atomic REV. Casper requires that the signed total exposure
/// covers the phlo limit at the price plus this fee (`casper/src/rust/util/rholang/acceptance.rs`,
/// `FIXED_FEE_REV` at line 20 and the ceiling at lines 287-292). The constant is private there.
pub const FIXED_FEE_REV: u64 = 1;

/// The selected v6 schedule: the four native dimensions at weight 1 and price 2
/// (`offered_funded_api_test.rs:136-153`).
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
        actual_price: PRICE,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// A signed offered v6 deploy of `term` by the vault owner `owner`, with the time stamp
/// `time_stamp`, the phlo limit `phlo_limit` and the exposure `exposure` of its one source
/// (`offered_funded_api_test.rs:162-241`, with the term, the limit and the exposure as
/// arguments). The exposure must cover the limit at the price plus [`FIXED_FEE_REV`].
pub fn signed_offer(
    owner: &(PrivateKey, PublicKey),
    term: &str,
    time_stamp: i64,
    phlo_limit: u64,
    exposure: u64,
) -> DeployDataProto {
    assert!(
        exposure >= phlo_limit * PRICE + FIXED_FEE_REV,
        "the exposure covers the phlo limit at the price plus the fixed fee"
    );
    let limits = offered_funded_v6_limits().envelope.payload;
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner.1.bytes))),
    };
    let payer = vault_payer(&signature).expect("a principal signature has a vault payer");
    let schedule = selected_schedule();
    let acquisition_terms = schedule
        .encode(PhloGenesisPolicy::LIMITS)
        .expect("the selected schedule encodes");
    let authority = cost_signature_to_sig(&signature).expect("a principal signature has a sig");
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
            .expect("a resource permission has a wire key")
        })
        .collect();
    let source = PhloSourcePolicyV1::new(
        &payer.custody_key,
        exposure,
        exposure,
        true,
        permissions,
        PhloSourceLimits {
            wire: limits.funding.wire,
            resource_permissions: schedule.classes.len(),
            authority_nodes: limits.funding.authority_nodes,
        },
    )
    .expect("the source policy is valid");
    let funding = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: phlo_limit,
                price_ceiling: PRICE,
                required_owner_ceilings: vec![PRICE],
                permitted_schedules: vec![schedule.clone()],
            },
            schedule_commitment: schedule
                .digest(PhloGenesisPolicy::LIMITS)
                .expect("the selected schedule has a digest"),
            total_exposure: u128::from(exposure),
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
        // Added by decision 90: the base's deploy parameters, empty as in
        // `offered_deploy.rs` of the base.
        parameters: Vec::new(),
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: SHARD.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(
        body,
        funding,
        i64::try_from(phlo_limit).expect("the phlo limit fits i64"),
        i64::try_from(PRICE).expect("the price fits i64"),
        limits,
    )
    .expect("the offered deploy is well formed");
    let signed = Cosigned::create_single_envelope(payload, Box::new(Secp256k1), owner.0.clone())
        .expect("the owner signs the offer");
    OfferedFundedDeploy::to_proto(&signed).expect("the signed offer has a proto")
}

/// A genesis with `validators` validators, version 6 and the offered funded v6 policy
/// (`offered_funded_api_test.rs:243-256`).
pub async fn offered_v6_genesis(validators: usize) -> GenesisContext {
    let mut parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators));
    parameters.2.version = 6;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let policy = PhloGenesisPolicy::from_schedule(&selected_schedule())
        .expect("the selected schedule forms a genesis policy")
        .with_offered_funded_v6_active();
    GenesisBuilder::new()
        .with_resource_policy(policy)
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("the offered v6 genesis builds")
}

/// Submits `offer` to `node` and returns its deploy id.
pub async fn submit(node: &TestNode, offer: DeployDataProto) -> Vec<u8> {
    let deploy_id = offer.deploy_id.to_vec();
    BlockAPI::deploy_offered(&node.engine_cell, offer, &None, false, SHARD)
        .await
        .expect("the offer is admitted");
    deploy_id
}

/// Lets `nodes[proposer]` create a block from its deploy pool and add it to its own DAG.
pub async fn propose(nodes: &mut [TestNode], proposer: usize) -> BlockMessage {
    let block = nodes[proposer]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer creates a block");
    accept(&mut nodes[proposer], &block).await;
    block
}

/// Asserts that `node` validates `block` and adds it to its DAG.
pub async fn accept(node: &mut TestNode, block: &BlockMessage) {
    let status = node
        .process_block(block.clone())
        .await
        .expect("the validation completes");
    assert!(
        matches!(status, Either::Right(_)),
        "the node accepts the block: {status:?}"
    );
}

/// The validation status of `block` on `node`, for a block that must be rejected.
pub async fn rejection(node: &mut TestNode, block: &BlockMessage) -> BlockError {
    match node
        .process_block(block.clone())
        .await
        .expect("the validation completes")
    {
        Either::Left(error) => error,
        Either::Right(_) => panic!("the node accepts a block that it must reject"),
    }
}

// Changed by decision 108: the merged base has the cost-cursor buckets of DR-119 in SystemVault.rho
// (`policy_snapshot/tests.rs:353` of the base), so the concurrent row of N4 searches its payers by
// bucket again. Decision 84 had disabled these helpers. They equal the base helpers of
// `offered_funded_api_test.rs:3380-3447`.
/// The cost-cursor bucket of the monetary scope `scope`: the first byte of the Keccak-256 hash of
/// the scope as a Rholang byte array, as `SystemVault.rho` keys its creation locks (DR-119,
/// `CostCursorBuckets.v`).
fn cost_cursor_bucket(scope: &[u8; 32]) -> u8 {
    let encoded = RhoByteArray::create_par(scope.to_vec()).encode_to_vec();
    Keccak256::hash(encoded)[0]
}

/// The two cost-cursor buckets of the vault of `owner`: the bucket of its monetary resource scope
/// and the bucket of its monetary fee scope.
pub fn owner_cursor_buckets(owner: &PublicKey) -> [u8; 2] {
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner.bytes))),
    };
    let custody = vault_payer(&signature)
        .expect("a principal signature has a vault payer")
        .custody_key;
    let context = |label: &[u8]| -> [u8; 32] {
        Blake2b256::hash(label.to_vec())
            .try_into()
            .expect("a Blake2b-256 digest has 32 bytes")
    };
    [
        context(
            b"f1r3node:monetary-resource:canonical-family-lexicographic-minimax:v1:SystemVault:General",
        ),
        context(b"f1r3node:monetary-fee:rotating-capped-max-min:v1:SystemVault:General"),
    ]
    .map(|policy| monetary_scope_for_custodies(&policy, std::iter::once(&custody)))
    .map(|scope| cost_cursor_bucket(&scope))
}

/// The first key of the chain `Blake2b256("{label}-{n}")`, n = 0, 1, ..., whose vault has buckets
/// that `accept` admits. Two offers of payers with disjoint buckets settle without a conflict in
/// the vault (DR-119), so the concurrent row of N4 searches its two payers.
pub fn searched_signer(label: &str, accept: impl Fn(&[u8; 2]) -> bool) -> (PrivateKey, PublicKey) {
    (0u32..65_536)
        .map(|counter| {
            let secret = PrivateKey::from_bytes(&Blake2b256::hash(
                format!("{label}-{counter}").into_bytes(),
            ));
            let public = Secp256k1.to_public(&secret);
            (secret, public)
        })
        .find(|(_, public)| accept(&owner_cursor_buckets(public)))
        .expect("the key chain holds a key with the wanted buckets")
}

// Changed for the N4 contract (a merge across blocks with every collection kind): the space
// branch of the merge row has eight blocks. The text before:
// /// The signer of the key `Blake2b256(label)`. The merge row of N4 gives each of its two space
// /// blocks its own payer (decision 82, fact 3, measured in P9 step 5). On the merged base, a merge
// /// keeps both sequential offers of one payer (probe R10 of decision 108), and the row keeps its
// /// two payers.
/// The signer of the key `Blake2b256(label)`. The merge row of N4 alternates two labeled payers
/// over the space blocks of its branch (decision 82, fact 3, measured in P9 step 5). On the
/// merged base, a merge keeps the sequential offers of one payer (probe R10 of decision 108).
pub fn labeled_signer(label: &str) -> (PrivateKey, PublicKey) {
    let secret = PrivateKey::from_bytes(&Blake2b256::hash(label.as_bytes().to_vec()));
    let public = Secp256k1.to_public(&secret);
    (secret, public)
}

/// The offered v6 genesis of [`offered_v6_genesis`] with a vault of 9,000,000 atomic REV for
/// each signer of `signers`.
pub async fn offered_v6_genesis_with_signers(
    validators: usize,
    signers: &[(PrivateKey, PublicKey)],
) -> GenesisContext {
    let mut parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators));
    parameters.2.version = 6;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    for signer in signers {
        parameters.2.vaults.push(Vault {
            vault_address: VaultAddress::from_public_key(&signer.1)
                .expect("a signer key forms a vault address"),
            initial_balance: 9_000_000,
        });
        parameters.1.push(signer.clone());
    }
    let policy = PhloGenesisPolicy::from_schedule(&selected_schedule())
        .expect("the selected schedule forms a genesis policy")
        .with_offered_funded_v6_active();
    GenesisBuilder::new()
        .with_resource_policy(policy)
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("the offered v6 genesis builds")
}
