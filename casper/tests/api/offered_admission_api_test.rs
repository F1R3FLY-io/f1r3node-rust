//! G1-6 (DR-124): the check before execution is live in play and in every
//! replay. The check runs before any runtime entry. A rejection leaves no
//! state change and consumes no tokens (P1:2191, P1:2210-2212). A complete
//! estimate binds `Certificate` with the bound B and holds B·phloPrice + 1. An
//! incomplete estimate binds `SignedLimit` and holds phloLimit·phloPrice + 1.
//!
//! The module is a child of `offered_funded_api_test`, so it shares that
//! module's fixtures. The per-node records and hooks of the test-utils
//! recorder observe each check, each runtime entry and each correctness
//! failure.

use casper::rust::util::rholang::acceptance::{prepare_offered_candidate, OfferedCandidateLimits};
use casper::rust::util::rholang::costacc::direct_wallet_funding::{
    gate_offered_candidate, OfferedEstimateKind, OfferedGate, OfferedGateInput,
};
use casper::rust::util::rholang::costacc::offered_acceptance::{
    OfferedCheckRecord, OfferedCheckRole, OfferedUsageKind,
};
use casper::rust::util::rholang::costacc::offered_grants::offered_grant_issue_call_limits;
use casper::rust::util::rholang::costacc::production_limits::offered_funded_v6_production_limits;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Cosigner;
use models::rust::block::state_hash::StateHash;
use models::rust::deploy_envelope::DeployEnvelopeRef;
use models::rust::native_cost_evidence::NativeFundingCaseV1;
use rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource;
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;

use super::*;
use crate::helper::offered_deploy::{owner_offer_limited, OfferedDeploy};
use crate::util::genesis_builder::GenesisContext;

/// A term whose estimate is complete: a fresh channel, closed data, a pattern
/// without outer levels and an empty body (design §10.1).
const COMPLETE_TERM: &str = "new x in { x!(0) | for (@n <- x) { Nil } }";

/// The phlo limit and the source cap of `owner_direct_offer`
/// (`helper/offered_deploy.rs`).
const OWNER_OFFER_LIMIT: u64 = 100_000_000;

/// An owner-direct offer whose only source has the hold cap and the debit
/// cap `source_cap`. Its total exposure is phloLimit·phloPrice + 1, the
/// smallest exposure that admission allows.
fn capped_offer(
    owner: &(PrivateKey, PublicKey),
    time_stamp: i64,
    term: &str,
    phlo_limit: u64,
    source_cap: u64,
) -> models::casper::DeployDataProto {
    permissioned_offer(
        owner,
        time_stamp,
        term,
        phlo_limit,
        source_cap,
        &[0, 1, 2, 3],
        &[],
    )
}

/// An offer whose only source is the owner's purse. The source permits the
/// classes `owner_classes` of the owner's lane, and each extra lane with the
/// classes listed for it. Its total exposure is phloLimit·phloPrice + 1.
fn permissioned_offer(
    owner: &(PrivateKey, PublicKey),
    time_stamp: i64,
    term: &str,
    phlo_limit: u64,
    source_cap: u64,
    owner_classes: &[usize],
    extra: &[(CostSignature, &[usize])],
) -> models::casper::DeployDataProto {
    let limits = offered_funded_v6_limits().envelope.payload;
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner.1.bytes))),
    };
    let payer = vault_payer(&signature).expect("a principal signature has a vault payer");
    let schedule = selected_schedule();
    let acquisition_terms = schedule
        .encode(PhloGenesisPolicy::LIMITS)
        .expect("the selected schedule encodes");
    let lanes = std::iter::once((signature.clone(), owner_classes))
        .chain(
            extra
                .iter()
                .map(|(signature, classes)| (signature.clone(), *classes)),
        )
        .map(|(signature, classes)| {
            let authority = cost_signature_to_sig(&signature).expect("a lane signature converts");
            let location = SignatureChannel::from_sig(&authority).par.encode_to_vec();
            (authority, location, classes)
        })
        .collect::<Vec<_>>();
    let mut permissions = Vec::new();
    for (authority, location, classes) in &lanes {
        for class in classes.iter() {
            permissions.push(
                PhloResource {
                    location,
                    class: *class,
                    acquisition_terms: &acquisition_terms,
                    authority,
                }
                .wire_key(PhloExecutionLimits {
                    resource_entries: 1,
                    authority_nodes: limits.funding.authority_nodes,
                    key_bytes: limits.funding.wire.field_bytes,
                })
                .expect("a resource permission encodes"),
            );
        }
    }
    let permission_count = permissions.len();
    let source = PhloSourcePolicyV1::new(
        &payer.custody_key,
        source_cap,
        source_cap,
        true,
        permissions,
        PhloSourceLimits {
            wire: limits.funding.wire,
            resource_permissions: permission_count,
            authority_nodes: limits.funding.authority_nodes,
        },
    )
    .expect("the permissioned source policy is valid");
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
            total_exposure: u128::from(phlo_limit) * 2 + 1,
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
        parameters: Vec::new(),
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(
        body,
        funding,
        i64::try_from(phlo_limit).expect("the phlo limit fits an i64"),
        2,
        limits,
    )
    .expect("the capped payload is valid");
    let signed = Cosigned::create_single_envelope(payload, Box::new(Secp256k1), owner.0.clone())
        .expect("the owner signs the offer");
    OfferedFundedDeploy::to_proto(&signed).expect("a signed offer encodes")
}

async fn admit(node: &TestNode, offer: &models::casper::DeployDataProto) {
    BlockAPI::deploy_offered(&node.engine_cell, offer.clone(), &None, false, "root")
        .await
        .expect("a well-formed offer is admitted");
}

/// The reason that the proposer recorded when it quarantined `offer`.
fn recorded_rejection(node: &TestNode, offer: &models::casper::DeployDataProto) -> String {
    let id = models::rust::deploy_id::DeployIdV6::try_from(offer.deploy_id.to_vec())
        .expect("an offered deploy id has 32 bytes");
    node.deploy_storage
        .lock()
        .envelope_rejection(&id)
        .map(|rejection| rejection.reason)
        .expect("the rejected offer has a status entry")
}

/// The decisions of the checks that `role` ran on `node` for `offer`.
fn checks(
    node: &TestNode,
    offer: &models::casper::DeployDataProto,
    role: OfferedCheckRole,
) -> Vec<OfferedCheckRecord> {
    node.runtime_manager
        .offered_budget_usages()
        .into_iter()
        .filter_map(|record| match record.kind {
            OfferedUsageKind::Check(found, decision)
                if found == role && record.deploy_id == offer.deploy_id.as_ref() =>
            {
                Some(decision)
            }
            _ => None,
        })
        .collect()
}

/// The number of records of `kind` on `node` for `offer`.
fn records(
    node: &TestNode,
    offer: &models::casper::DeployDataProto,
    kind: OfferedUsageKind,
) -> usize {
    node.runtime_manager
        .offered_budget_usages()
        .iter()
        .filter(|record| record.kind == kind && record.deploy_id == offer.deploy_id.as_ref())
        .count()
}

fn decision_of(gate: &OfferedGate) -> OfferedCheckRecord {
    OfferedCheckRecord::Accepted {
        complete: matches!(gate.kind(), OfferedEstimateKind::Complete),
        estimate: gate.estimate_phlo(),
        resource_bound: gate.resource_bound(),
        bound_source: gate.bound_source(),
        total_hold: gate.total_hold(),
    }
}

fn payer_address(owner: &(PrivateKey, PublicKey)) -> VaultAddress {
    VaultAddress::from_public_key(&owner.1).expect("a public key has a vault address")
}

/// The decision of the check, computed again outside play and replay (see
/// `direct_gate`).
async fn direct_decision(
    node: &TestNode,
    genesis: &GenesisContext,
    offer: &models::casper::DeployDataProto,
    root: &StateHash,
) -> OfferedCheckRecord {
    match direct_gate(node, genesis, offer, root).await {
        Ok(gate) => decision_of(&gate),
        Err(_) => OfferedCheckRecord::Rejected,
    }
}

/// The check again, outside play and replay, on `node` at `root`, with a
/// fresh offered runtime (DR-102, design §6.4).
async fn direct_gate(
    node: &TestNode,
    genesis: &GenesisContext,
    offer: &models::casper::DeployDataProto,
    root: &StateHash,
) -> Result<
    OfferedGate,
    casper::rust::util::rholang::costacc::direct_wallet_funding::OfferedGateRejection,
> {
    let adopted = AdoptedResourcePolicy::load(
        &node.runtime_manager,
        &genesis.genesis_block,
        node.casper.casper_shard_conf(),
    )
    .await
    .expect("the adopted policy loads");
    let envelope = OfferedDeploy::from_proto(offer.clone()).envelope;
    let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
        panic!("the offer has an offered envelope");
    };
    let original: [u8; 32] = root.as_ref().try_into().expect("a root has 32 bytes");
    let funding = offered_funded_v6_limits().envelope.payload.funding;
    let prepared = prepare_offered_candidate(
        signed,
        &adopted,
        original,
        original,
        OfferedCandidateLimits {
            funding: PhloFundingIntentV2Limits {
                wire: funding.wire,
                base: funding,
                grant_uses: funding.wire.total_bytes / 8,
                grant_id_bytes: funding.wire.field_bytes,
                quote_evidence_bytes: funding.wire.field_bytes,
            },
            max_phlo_limit: i64::MAX as u64,
        },
    )
    .expect("the candidate prepares");
    let snapshot = node
        .runtime_manager
        .read_direct_offered_wallet_snapshot(&envelope, root, None)
        .await
        .expect("the wallet snapshot reads");
    let selected = prepared
        .selected_terms(&adopted)
        .expect("the signed schedule is selected");
    let schedule = PhloScheduleBinding::new(selected.schedule(), PhloGenesisPolicy::LIMITS)
        .expect("the schedule binds");
    let runtime = node
        .runtime_manager
        .spawn_offered_runtime(offered_grant_issue_call_limits())
        .await
        .expect("an offered runtime spawns");
    gate_offered_candidate(OfferedGateInput {
        envelope: &envelope,
        prepared: &prepared,
        selected_terms: selected.bytes(),
        schedule: &schedule,
        minimum_price: adopted.genesis().minimum_price(),
        snapshot: &snapshot,
        evaluator: &runtime.reducer,
        limits: offered_funded_v6_production_limits().family,
    })
}

/// The lanes with known demand in the estimate of `offer`, before the check
/// restricts it to the client lanes, and the deploy lane.
async fn estimated_lanes(
    node: &TestNode,
    offer: &models::casper::DeployDataProto,
) -> (Vec<[u8; 32]>, [u8; 32]) {
    // Changed by the G1-6 fix (DR-124): the estimate moved into
    // `estimated_counts`, which also returns the counts.
    let (counts, deploy_lane) = estimated_counts(node, offer).await;
    let mut lanes = counts.keys().map(|(lane, _)| *lane).collect::<Vec<_>>();
    lanes.dedup();
    (lanes, deploy_lane)
}

/// The counts V(lane, class) of the estimate of `offer`, before the check
/// restricts it to the client lanes, and the deploy lane.
async fn estimated_counts(
    node: &TestNode,
    offer: &models::casper::DeployDataProto,
) -> (std::collections::BTreeMap<([u8; 32], usize), u64>, [u8; 32]) {
    use rholang::rust::interpreter::accounting::delta_sigma::estimate::estimate_offered_term;
    use rholang::rust::interpreter::accounting::funding_sig;

    let envelope = OfferedDeploy::from_proto(offer.clone()).envelope;
    let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
        panic!("the offer has an offered envelope");
    };
    let funding = funding_sig(signed);
    let runtime = node
        .runtime_manager
        .spawn_offered_runtime(offered_grant_issue_call_limits())
        .await
        .expect("an offered runtime spawns");
    let estimate = estimate_offered_term(
        &envelope.body().term,
        models::rust::normalizer_env::normalizer_env_from_envelope(&envelope).unwrap(),
        casper::rust::util::rholang::tools::Tools::user_envelope_rng(&envelope),
        &funding,
        &selected_schedule(),
        &runtime.reducer,
    )
    .expect("the estimate runs");
    // Changed by the G1-6 fix (DR-124): the caller derives the lanes from the
    // counts.
    // let mut lanes = estimate
    //     .quantities
    //     .keys()
    //     .map(|(lane, _)| *lane)
    //     .collect::<Vec<_>>();
    // lanes.dedup();
    // (lanes, funding.lane_hash())
    (estimate.quantities, funding.lane_hash())
}

/// Every node accepts `block`, every node holds one receipt for `offer`, and
/// every check of the producer's play, the producer's replay and each
/// validator's replay reaches one decision. Returns the decision and the
/// receipt.
async fn accepted_with_one_decision(
    nodes: &mut [TestNode],
    block: &models::rust::casper::protocol::casper_message::BlockMessage,
    offer: &models::casper::DeployDataProto,
) -> (
    OfferedCheckRecord,
    casper::rust::api::block_api::OfferedSettlementReceipt,
) {
    let receipt = super::accepted_everywhere_with_one_receipt(nodes, block, &offer.deploy_id).await;
    let play = checks(&nodes[0], offer, OfferedCheckRole::Play);
    assert_eq!(play.len(), 1, "the producer checks the offer once in play");
    let decision = play[0];
    for (index, node) in nodes.iter().enumerate() {
        let replays = checks(node, offer, OfferedCheckRole::Replay);
        assert!(
            !replays.is_empty(),
            "node {index} checks the offer in replay"
        );
        assert!(
            replays.iter().all(|replayed| *replayed == decision),
            "node {index} reaches the producer's decision: {replays:?} against {decision:?}"
        );
    }
    (decision, receipt)
}

/// The funding evidence of `block`: every source holds exactly its debit and
/// refunds 0 (design §8.1, `sections.rs`), and the payer's balance after the
/// block is its balance before minus its debit.
async fn evidence_holds_equal_debits(
    nodes: &[TestNode],
    block: &models::rust::casper::protocol::casper_message::BlockMessage,
    payer: &(PrivateKey, PublicKey),
) {
    let protocol = offered_funded_v6_limits();
    let offered = block.body.deploys[0]
        .as_offered()
        .expect("the block carries an offered deploy");
    let evidence = offered
        .evidence(protocol.evidence)
        .expect("the evidence decodes");
    let case = NativeFundingCaseV1::decode(evidence.funding_case, protocol.funding_case)
        .expect("the funding case decodes");
    assert_eq!(
        case.sources.len(),
        1,
        "an owner-direct offer has one source"
    );
    for source in &case.sources {
        assert_eq!(source.hold, source.debit, "the hold equals the debit");
        assert_eq!(source.refund, 0, "the refund is implicit");
    }
    let address = payer_address(payer);
    let before = super::vault_balance(nodes, &block.body.state.pre_state_hash, &address).await;
    let after = super::vault_balance(nodes, &block.body.state.post_state_hash, &address).await;
    assert_eq!(
        u128::from(after) + u128::from(case.sources[0].debit),
        u128::from(before),
        "the post-state balance is the pre-state balance minus the debit"
    );
}

/// An offer whose holds do not fit never enters the runtime. The proposer
/// quarantines it with "insufficient phlogiston" (P1:2210) and includes the
/// next offer. The rejected payer pays no fee.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_underfunded_offer_never_enters_the_runtime_and_pays_no_fee() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a single-node network starts");
    let underfunded_payer = genesis.genesis_vaults[0].clone();
    let funded_payer = genesis.genesis_vaults[2].clone();
    // The term uses a public channel, so its estimate is incomplete and it
    // holds 2,000,000·2 + 1. Its only source can hold 1,000,000.
    let underfunded = capped_offer(
        &underfunded_payer,
        1,
        "@\"admission-underfunded\"!(1)",
        2_000_000,
        1_000_000,
    );
    let funded = owner_direct_offer(&funded_payer, 2, "@\"admission-funded\"!(1)".to_string());
    admit(&nodes[0], &underfunded).await;
    admit(&nodes[0], &funded).await;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer skips the underfunded offer");
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        funded.deploy_id.as_ref()
    );
    let reason = recorded_rejection(&nodes[0], &underfunded);
    assert!(reason.contains("insufficient phlogiston"), "{reason}");
    assert_eq!(
        records(&nodes[0], &underfunded, OfferedUsageKind::PlayRuntimeEntry),
        0,
        "the underfunded offer never enters the runtime"
    );
    assert_eq!(
        checks(&nodes[0], &underfunded, OfferedCheckRole::Play),
        vec![OfferedCheckRecord::Rejected]
    );
    assert!(
        records(&nodes[0], &funded, OfferedUsageKind::PlayRuntimeEntry) >= 1,
        "the counter sees the runtime entry of the funded offer"
    );
    let address = payer_address(&underfunded_payer);
    assert_eq!(
        super::vault_balance(&nodes, &block.body.state.post_state_hash, &address).await,
        super::vault_balance(&nodes, &block.body.state.pre_state_hash, &address).await,
        "the rejected payer pays no fee"
    );
}

/// A complete estimate holds B·phloPrice + 1 under `Certificate` with the
/// bound B. An incomplete estimate holds phloLimit·phloPrice + 1 under
/// `SignedLimit`. The producer and every validator reach the same decision
/// and the same receipt. The check on a fresh runtime gives the same decision,
/// and the evidence shows a hold equal to the debit with refund 0.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_complete_offer_holds_its_estimate_and_an_incomplete_offer_holds_its_limit() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("a three-node network starts");
    let payer = genesis.genesis_vaults[0].clone();

    let incomplete = owner_direct_offer(&payer, 1, "@\"admission-incomplete\"!(1)".to_string());
    let (block, _) = super::offered_block(&mut nodes, 0, incomplete.clone()).await;
    let (decision, receipt) = accepted_with_one_decision(&mut nodes, &block, &incomplete).await;
    let OfferedCheckRecord::Accepted {
        complete,
        resource_bound,
        bound_source,
        total_hold,
        ..
    } = decision
    else {
        panic!("the incomplete offer is accepted: {decision:?}");
    };
    assert!(!complete);
    assert_eq!(bound_source, NativeBoundSource::SignedLimit);
    assert_eq!(resource_bound, OWNER_OFFER_LIMIT);
    assert_eq!(total_hold, u128::from(OWNER_OFFER_LIMIT) * 2 + 1);
    assert!(receipt.phlo_used <= resource_bound);
    assert_eq!(
        direct_decision(
            &nodes[1],
            &genesis,
            &incomplete,
            &block.body.state.pre_state_hash
        )
        .await,
        decision,
        "the check gives the same decision on a fresh runtime"
    );
    evidence_holds_equal_debits(&nodes, &block, &payer).await;

    let complete = owner_direct_offer(&payer, 2, COMPLETE_TERM.to_string());
    let (block, _) = super::offered_block(&mut nodes, 0, complete.clone()).await;
    let (decision, receipt) = accepted_with_one_decision(&mut nodes, &block, &complete).await;
    let OfferedCheckRecord::Accepted {
        complete: is_complete,
        estimate,
        resource_bound,
        bound_source,
        total_hold,
    } = decision
    else {
        panic!("the complete offer is accepted: {decision:?}");
    };
    assert!(is_complete);
    assert_eq!(bound_source, NativeBoundSource::Certificate);
    assert!(estimate > 0 && estimate <= OWNER_OFFER_LIMIT);
    assert_eq!(resource_bound, estimate);
    assert_eq!(total_hold, u128::from(estimate) * 2 + 1);
    assert!(receipt.phlo_used <= estimate, "the run stays within B");
    assert!(receipt.rev_spent <= total_hold, "the charge fits the hold");
    assert_eq!(
        direct_decision(
            &nodes[2],
            &genesis,
            &complete,
            &block.body.state.pre_state_hash
        )
        .await,
        decision,
        "the check gives the same decision on a fresh runtime"
    );
    evidence_holds_equal_debits(&nodes, &block, &payer).await;
}

/// An estimate above the signed limit rejects before execution (design §6.3
/// item 1). A limit equal to the estimate is accepted under `Certificate`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_estimate_above_the_signed_limit_rejects_before_execution() {
    let genesis = super::offered_v6_genesis(1).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a single-node network starts");
    let payer = genesis.genesis_vaults[0].clone();
    let measured = owner_direct_offer(&payer, 1, COMPLETE_TERM.to_string());
    let (block, _) = super::offered_block(&mut nodes, 0, measured.clone()).await;
    assert!(matches!(
        nodes[0]
            .process_block(block.clone())
            .await
            .expect("the producer processes its block"),
        Either::Right(_)
    ));
    let measured_checks = checks(&nodes[0], &measured, OfferedCheckRole::Play);
    let [OfferedCheckRecord::Accepted { estimate, .. }] = measured_checks[..] else {
        panic!("the measured offer is accepted once: {measured_checks:?}");
    };
    assert!(estimate > 1);

    let above = owner_offer_limited(
        &payer,
        2,
        COMPLETE_TERM.to_string(),
        0,
        "root".to_string(),
        estimate - 1,
    );
    let exact = owner_offer_limited(
        &payer,
        3,
        COMPLETE_TERM.to_string(),
        0,
        "root".to_string(),
        estimate,
    );
    admit(&nodes[0], &above).await;
    admit(&nodes[0], &exact).await;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer skips the offer above its limit");
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        exact.deploy_id.as_ref()
    );
    let reason = recorded_rejection(&nodes[0], &above);
    assert!(reason.contains("exceeds the signed phlo limit"), "{reason}");
    assert_eq!(
        records(&nodes[0], &above, OfferedUsageKind::PlayRuntimeEntry),
        0,
        "the offer above its limit never enters the runtime"
    );
    assert_eq!(checks(&nodes[0], &exact, OfferedCheckRole::Play), vec![
        OfferedCheckRecord::Accepted {
            complete: true,
            estimate,
            resource_bound: estimate,
            bound_source: NativeBoundSource::Certificate,
            total_hold: u128::from(estimate) * 2 + 1,
        }
    ]);
}

/// A forced overrun of a certified bound (design §7, second row): the hook
/// removes the counts of a complete estimate, so the run passes the bound 0.
/// The producer logs a correctness failure and rejects the offer with no
/// charge.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forced_certificate_overrun_rejects_without_a_charge_and_logs_a_correctness_failure() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a single-node network starts");
    let overrun_payer = genesis.genesis_vaults[0].clone();
    let funded_payer = genesis.genesis_vaults[2].clone();
    let overrun = owner_direct_offer(&overrun_payer, 1, COMPLETE_TERM.to_string());
    let funded = owner_direct_offer(&funded_payer, 2, "@\"admission-funded\"!(1)".to_string());
    nodes[0]
        .runtime_manager
        .remove_offered_estimate_counts_for_tests(&overrun.deploy_id);
    admit(&nodes[0], &overrun).await;
    admit(&nodes[0], &funded).await;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer skips the overrun offer");
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        funded.deploy_id.as_ref()
    );
    assert_eq!(checks(&nodes[0], &overrun, OfferedCheckRole::Play), vec![
        OfferedCheckRecord::Accepted {
            complete: true,
            estimate: 0,
            resource_bound: 0,
            bound_source: NativeBoundSource::Certificate,
            total_hold: 1,
        }
    ]);
    assert_eq!(
        records(&nodes[0], &overrun, OfferedUsageKind::PlayRuntimeEntry),
        1
    );
    assert_eq!(
        records(
            &nodes[0],
            &overrun,
            OfferedUsageKind::CertificateCorrectnessFailure
        ),
        1,
        "the producer logs one correctness failure"
    );
    let reason = recorded_rejection(&nodes[0], &overrun);
    assert!(reason.contains("non-user failure"), "{reason}");
    let address = payer_address(&overrun_payer);
    assert_eq!(
        super::vault_balance(&nodes, &block.body.state.post_state_hash, &address).await,
        super::vault_balance(&nodes, &block.body.state.pre_state_hash, &address).await,
        "the overrun is not charged"
    );
}

/// A producer that skips the check (a hook of that node only) includes an
/// offer whose holds do not fit. Every validator runs the check, never
/// enters the runtime, and rejects the block (design §6.4, check 1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validators_reject_the_block_of_a_producer_that_skips_the_check() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("a three-node network starts");
    let payer = genesis.genesis_vaults[0].clone();
    let offer = capped_offer(
        &payer,
        1,
        "@\"admission-skipped\"!(1)",
        2_000_000,
        1_000_000,
    );
    nodes[0]
        .runtime_manager
        .skip_offered_check_for_tests(&offer.deploy_id);
    let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
    for (index, node) in nodes.iter_mut().enumerate().skip(1) {
        let status = node
            .process_block(block.clone())
            .await
            .expect("validation completes");
        assert!(
            matches!(
                &status,
                Either::Left(BlockError::BlockException(error))
                    if error.to_string().contains("insufficient phlogiston")
            ),
            "node {index} rejects the block: {status:?}"
        );
        assert!(!node.casper.dag_contains(&block.block_hash));
    }
    for (index, node) in nodes.iter().enumerate().skip(1) {
        let replays = checks(node, &offer, OfferedCheckRole::Replay);
        assert!(!replays.is_empty(), "node {index} checks the offer");
        assert!(replays
            .iter()
            .all(|decision| *decision == OfferedCheckRecord::Rejected));
        assert_eq!(
            records(node, &offer, OfferedUsageKind::ReplayRuntimeEntry),
            0,
            "node {index} never enters the runtime"
        );
    }
}

/// The check in play reads the URN map of the play runtime, and the check in
/// replay reads the URN map of the spawned offered runtime. Both equal the
/// URN map of the native replay environment that evaluates the offer
/// (design §6.4, arbitration LOW 9).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_play_replay_and_native_replay_environments_share_one_urn_map() {
    let genesis = super::offered_v6_genesis(2).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let payer = genesis.genesis_vaults[0].clone();
    let offer = owner_direct_offer(&payer, 1, COMPLETE_TERM.to_string());
    let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
    super::accepted_everywhere_with_one_receipt(&mut nodes, &block, &offer.deploy_id).await;
    for (index, node) in nodes.iter().enumerate() {
        let replays = records(node, &offer, OfferedUsageKind::ReplayUrnMap {
            matches_check: true,
        });
        assert!(replays >= 1, "node {index} compares the URN maps");
        assert_eq!(
            records(node, &offer, OfferedUsageKind::ReplayUrnMap {
                matches_check: false,
            },),
            0,
            "node {index} replays with the URN map of its check"
        );
    }
    let play = nodes[0]
        .runtime_manager
        .spawn_offered_play_runtime(offered_grant_issue_call_limits())
        .await
        .expect("the offered play runtime spawns");
    let replay = nodes[1]
        .runtime_manager
        .spawn_offered_runtime(offered_grant_issue_call_limits())
        .await
        .expect("the offered replay runtime spawns");
    assert!(!play.reducer.urn_map.is_empty());
    assert_eq!(play.reducer.urn_map, replay.reducer.urn_map);
}

// Changed by G3 (DR-135, S7): a reader now consumes a user-signed datum, and
// the writer pays its share.
// /// Existing outcomes stay under the live check. Consuming a user-signed
// /// datum still needs the writer's token, and an endless loop still exhausts
// /// its signed limit as a charged user failure (DR-113). Both estimates are
// /// incomplete, so both offers hold their whole limit under `SignedLimit`.
/// Existing outcomes stay under the live check. A reader consumes a
/// user-signed datum, and the writer pays its share (DR-135). An endless loop
/// still exhausts its signed limit as a charged user failure (DR-113). Both
/// estimates are incomplete, so both offers hold their whole limit under
/// `SignedLimit`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn existing_outcomes_keep_their_class_with_incomplete_estimates() {
    const ENDLESS_LOOP: &str = "new loop in { contract loop(@n) = { loop!(n + 1) } | loop!(0) }";
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let writer = genesis.genesis_vaults[2].clone();
    let reader = genesis.genesis_vaults[0].clone();
    let incomplete_at = |limit: u64| OfferedCheckRecord::Accepted {
        complete: false,
        estimate: 0,
        resource_bound: limit,
        bound_source: NativeBoundSource::SignedLimit,
        total_hold: u128::from(limit) * 2 + 1,
    };
    let same_class =
        |found: OfferedCheckRecord, expected: OfferedCheckRecord| match (found, expected) {
            (
                OfferedCheckRecord::Accepted {
                    complete,
                    resource_bound,
                    bound_source,
                    total_hold,
                    ..
                },
                OfferedCheckRecord::Accepted {
                    complete: expected_complete,
                    resource_bound: expected_bound,
                    bound_source: expected_source,
                    total_hold: expected_hold,
                    ..
                },
            ) => {
                complete == expected_complete
                    && resource_bound == expected_bound
                    && bound_source == expected_source
                    && total_hold == expected_hold
            }
            _ => false,
        };

    let write = owner_direct_offer(&writer, 1, "@\"admission-user-channel\"!(1)".to_string());
    super::propose_offer(&mut nodes, write.clone())
        .await
        .expect("the writer's datum is funded");
    let read = owner_direct_offer(
        &reader,
        2,
        "for(_ <- @\"admission-user-channel\") { Nil }".to_string(),
    );
    // Changed by G3 (DR-135, S7): the read settles with the reader's row and
    // the writer's row.
    // let rejection = super::propose_offer(&mut nodes, read.clone())
    //     .await
    //     .expect_err("consuming a user-signed datum needs the writer's token");
    // assert!(
    //     rejection.contains("no feasible signed assignment"),
    //     "{rejection}"
    // );
    super::propose_offer_rows(&mut nodes, read.clone(), 2)
        .await
        .expect("the reader consumes the writer's datum, and the writer pays its share");
    for offer in [&write, &read] {
        let play = checks(&nodes[0], offer, OfferedCheckRole::Play);
        assert_eq!(play.len(), 1);
        assert!(
            same_class(play[0], incomplete_at(OWNER_OFFER_LIMIT)),
            "{play:?}"
        );
    }

    let limit = 20_000;
    let looping = owner_offer_limited(
        &writer,
        3,
        ENDLESS_LOOP.to_string(),
        0,
        "root".to_string(),
        limit,
    );
    let (block, _) = super::offered_block(&mut nodes, 0, looping.clone()).await;
    assert!(
        block.body.deploys[0].is_failed(),
        "the exhausted deploy is published as failed"
    );
    let receipt =
        super::accepted_everywhere_with_one_receipt(&mut nodes, &block, &looping.deploy_id).await;
    assert!(receipt.rev_spent > 0, "the exhaustion is charged");
    let play = checks(&nodes[0], &looping, OfferedCheckRole::Play);
    assert_eq!(play.len(), 1);
    assert!(same_class(play[0], incomplete_at(limit)), "{play:?}");
}

/// The signature that the normalizer gives the unbound name `name` in a cost
/// signature.
fn spelled_signature(name: &str) -> CostSignature {
    rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt(&format!(
        "{{% Nil %}}[ {name} ]"
    ))
    .expect("a signed term compiles")
    .cost_signed_terms[0]
        .signature
        .clone()
        .expect("a signed term has a signature")
}

fn lane_of(signature: &CostSignature) -> [u8; 32] {
    cost_signature_to_sig(signature)
        .expect("a static signature converts")
        .lane_hash()
}

/// A funding-slot installer (P1:1534-1630). Its persistent receive is
/// unsigned, so its introduction is the installer's own demand. The body
/// signs `[entry -o slot]`, which desugars to `{for(..){{P}_slot}}_entry`
/// (P1 `def:sugar-lollipop`). `entry` and `slot` are fresh names of the
/// installer, so no source of the envelope is located at their purses or
/// permits their lanes: the envelope commits to its sources and their
/// permissions, and the names come from its identity.
const FUNDING_SLOT_INSTALLER: &str = r#"new entry, slot, entryAddressCh, slotAddressCh,
      VaultAddress(`rho:vault:address`), DeployerIdOps(`rho:system:deployerId:ops`) in {
      for (@request, deployerId <= @"agent-trigger") {
        new publicKeyCh in {
          DeployerIdOps!("pubKeyBytes", *deployerId, *publicKeyCh) |
          for (@publicKey <- publicKeyCh) {
            if (publicKey == "GATEWAY_PUBLIC_KEY".hexToBytes()) {
              {% for(@accepted <- entry) {
                new x in { x!(0) | for(@0 <- x) { @"agent-ran"!(true) } }
              } %}[entry -o slot] |
              entry!(request)
            }
          }
        }
      } |
      VaultAddress!("fromUnforgeable", *entry, *entryAddressCh) |
      for (@entryAddress <- entryAddressCh) { @"agent-entry-address"!!(entryAddress) } |
      VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
      for (@slotAddress <- slotAddressCh) { @"agent-slot-address"!!(slotAddress) }
    }"#;

/// DR-124: the check holds only the client's demand (P1:2191-2200). The
/// demand of the slot lanes leaves the holds, so a funding-slot installer
/// passes the check and holds its signed limit under `SignedLimit`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_funding_slot_installer_passes_the_check_and_holds_its_signed_limit() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let installer = genesis.genesis_vaults[0].clone();
    let term = FUNDING_SLOT_INSTALLER.replace(
        "GATEWAY_PUBLIC_KEY",
        &hex::encode(&genesis.genesis_vaults[2].1.bytes),
    );
    let offer = owner_direct_offer(&installer, 1, term);
    let (lanes, deploy_lane) = estimated_lanes(&nodes[0], &offer).await;
    assert_eq!(
        lanes.len(),
        3,
        "the deploy lane, entry and slot have known demand"
    );
    assert!(lanes.contains(&deploy_lane));
    let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
    let (decision, _) = accepted_with_one_decision(&mut nodes, &block, &offer).await;
    let OfferedCheckRecord::Accepted {
        complete,
        estimate,
        resource_bound,
        bound_source,
        total_hold,
    } = decision
    else {
        panic!("the installer is accepted: {decision:?}");
    };
    assert!(!complete);
    assert!(estimate > 0, "the outer unsigned receive is client demand");
    assert_eq!(bound_source, NativeBoundSource::SignedLimit);
    assert_eq!(resource_bound, OWNER_OFFER_LIMIT);
    assert_eq!(total_hold, u128::from(OWNER_OFFER_LIMIT) * 2 + 1);
}

/// Review finding M-3 under DR-124: the plan counts the layer `{% Nil %}[alice]`
/// inside sent data (P1 `def:token-demand`), but no source is located at the
/// purse of `alice` or permits it. The layer adds nothing to the client's
/// demand, so the offer needs no source for `alice` and holds its signed
/// limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_send_of_a_foreign_signed_process_needs_no_source_for_that_lane() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let payer = genesis.genesis_vaults[0].clone();
    let offer = owner_direct_offer(&payer, 1, "new x in { x!({% Nil %}[alice]) }".to_string());
    let (lanes, deploy_lane) = estimated_lanes(&nodes[0], &offer).await;
    assert!(lanes.contains(&deploy_lane));
    assert!(
        lanes.contains(&lane_of(&spelled_signature("alice"))),
        "the plan counts the signed layer inside the data"
    );
    let gate = direct_gate(
        &nodes[0],
        &genesis,
        &offer,
        &genesis.genesis_block.body.state.post_state_hash,
    )
    .await
    .expect("the check accepts the offer");
    assert_eq!(
        gate.kind(),
        &OfferedEstimateKind::Incomplete(
            rholang::rust::interpreter::accounting::delta_sigma::estimate::IncompleteReason::ForeignLane
        )
    );
    let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
    assert!(!block.body.deploys[0].is_failed());
    let (decision, _) = accepted_with_one_decision(&mut nodes, &block, &offer).await;
    assert_eq!(decision, decision_of(&gate));
    assert!(matches!(decision, OfferedCheckRecord::Accepted {
        complete: false,
        bound_source: NativeBoundSource::SignedLimit,
        ..
    }));
}

/// A client lane whose class has no eligible source still rejects the offer
/// before execution with "insufficient phlogiston" (design §6.3 item 2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_lane_class_without_an_eligible_source_rejects_before_execution() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a single-node network starts");
    let restricted_payer = genesis.genesis_vaults[0].clone();
    let funded_payer = genesis.genesis_vaults[2].clone();
    let restricted = permissioned_offer(
        &restricted_payer,
        1,
        COMPLETE_TERM,
        2_000_000,
        5_000_000,
        &[0],
        &[],
    );
    let funded = owner_direct_offer(&funded_payer, 2, "@\"admission-funded\"!(1)".to_string());
    admit(&nodes[0], &restricted).await;
    admit(&nodes[0], &funded).await;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer skips the restricted offer");
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        funded.deploy_id.as_ref()
    );
    let reason = recorded_rejection(&nodes[0], &restricted);
    assert!(reason.contains("insufficient phlogiston"), "{reason}");
    assert_eq!(
        records(&nodes[0], &restricted, OfferedUsageKind::PlayRuntimeEntry),
        0
    );
    assert_eq!(
        checks(&nodes[0], &restricted, OfferedCheckRole::Play),
        vec![OfferedCheckRecord::Rejected]
    );
}

/// The loophole control of DR-124: restricting the class permissions of a
/// source does not make a client lane foreign. If it did, the classes without
/// a permission would leave the holds, the incomplete offer would hold its
/// limit and run, and settlement would then find no eligible source. The
/// deploy lane stays a client lane, so the offer is rejected before
/// execution. The same term with every class permitted is accepted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restricting_class_permissions_does_not_exclude_a_client_lane() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a single-node network starts");
    let term = "@\"admission-public\"!(1)";
    let restricted = permissioned_offer(
        &genesis.genesis_vaults[0],
        1,
        term,
        2_000_000,
        5_000_000,
        &[0],
        &[],
    );
    let permitted = permissioned_offer(
        &genesis.genesis_vaults[2],
        2,
        term,
        2_000_000,
        5_000_000,
        &[0, 1, 2, 3],
        &[],
    );
    admit(&nodes[0], &restricted).await;
    admit(&nodes[0], &permitted).await;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer includes the permitted offer");
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        permitted.deploy_id.as_ref()
    );
    let reason = recorded_rejection(&nodes[0], &restricted);
    assert!(reason.contains("insufficient phlogiston"), "{reason}");
    assert_eq!(
        records(&nodes[0], &restricted, OfferedUsageKind::PlayRuntimeEntry),
        0,
        "the restricted offer never runs"
    );
    assert!(matches!(
        checks(&nodes[0], &permitted, OfferedCheckRole::Play)[..],
        [OfferedCheckRecord::Accepted {
            complete: false,
            bound_source: NativeBoundSource::SignedLimit,
            ..
        }]
    ));
}

// Disabled by the G1-6 fix (DR-124 correction): the corrected rule makes a
// lane that a source permits for at least one class a client lane, so the
// expectation of this test (a permitted lane is foreign) no longer holds.
// The inverted test `a_lane_that_sources_permit_is_a_client_lane_with_exact_holds`
// replaces it.
// /// The rule of DR-124 locates a source at the purse of its payer. A source
// /// that only permits a lane is not located there, so that lane is foreign.
// /// This is the case of the `payer` lane in the 64-source test. The offer holds
// /// its signed limit, runs under `SignedLimit`, and settlement charges the
// /// work of the lane to the permitting purse, as it did before G1-6.
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// async fn a_lane_that_a_source_only_permits_is_a_foreign_lane() {
//     let genesis = super::offered_v6_genesis(3).await;
//     let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
//         .await
//         .expect("a two-node network starts");
//     let payer = genesis.genesis_vaults[0].clone();
//     let payer_lane = spelled_signature("payer");
//     let offer = permissioned_offer(
//         &payer,
//         1,
//         "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]",
//         2_000_000,
//         5_000_000,
//         &[0, 1, 2, 3],
//         &[(payer_lane.clone(), &[0, 1, 2, 3])],
//     );
//     let gate = direct_gate(
//         &nodes[0],
//         &genesis,
//         &offer,
//         &genesis.genesis_block.body.state.post_state_hash,
//     )
//     .await
//     .expect("the check accepts the offer");
//     assert_eq!(
//         gate.kind(),
//         &OfferedEstimateKind::Incomplete(
//             rholang::rust::interpreter::accounting::delta_sigma::estimate::IncompleteReason::ForeignLane
//         )
//     );
//     assert_eq!(gate.estimate_phlo(), 0, "the deploy lane has no demand");
//     assert_eq!(gate.total_hold(), 2_000_000 * 2 + 1);
//     let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
//     assert!(!block.body.deploys[0].is_failed());
//     let (decision, receipt) = accepted_with_one_decision(&mut nodes, &block, &offer).await;
//     assert_eq!(decision, decision_of(&gate));
//     assert!(
//         receipt.fresh_phlo > 0,
//         "the work of the payer lane is charged"
//     );
//     assert_eq!(
//         receipt.rev_spent,
//         u128::from(receipt.fresh_phlo) * u128::from(receipt.phlo_price) + receipt.fee_rev
//     );
//     assert!(
//         lane_of(&payer_lane)
//             != lane_of(&CostSignature {
//                 value: Some(Value::Ground(principal_ground_v61(&payer.1.bytes))),
//             })
//     );
// }

// ── The lanes that the sources permit (G1-6 fix, DR-124) ───────────────────

/// Signer keys for the offers with several sources. The keys follow the
/// default genesis keys and avoid the system keys, which may not sign a
/// deploy.
fn sponsor_keys(count: usize) -> Vec<(PrivateKey, PublicKey)> {
    let forbidden = casper::rust::genesis::contracts::standard_deploys::system_public_keys();
    let mut keys = Vec::with_capacity(count);
    let mut index = 20_000u32;
    while keys.len() < count {
        let mut private_bytes = [0u8; 32];
        private_bytes[28..].copy_from_slice(&index.to_be_bytes());
        index += 1;
        let private = PrivateKey::from_bytes(&private_bytes);
        let public = Secp256k1.to_public(&private);
        if forbidden.iter().any(|key| **key == public) {
            continue;
        }
        keys.push((private, public));
    }
    keys
}

/// An offer funded by the principal purse of each signer. Source `i` permits
/// the lanes and classes of `permissions[i]`, and only the first source
/// permits the fee. Every signer cosigns the envelope. The total exposure is
/// phloLimit·phloPrice + 1.
fn sponsored_offer(
    signers: &[(PrivateKey, PublicKey)],
    time_stamp: i64,
    term: &str,
    phlo_limit: u64,
    source_cap: u64,
    permissions: &[Vec<(CostSignature, Vec<usize>)>],
) -> models::casper::DeployDataProto {
    let limits = offered_funded_v6_limits().envelope.payload;
    let schedule = selected_schedule();
    let acquisition_terms = schedule
        .encode(PhloGenesisPolicy::LIMITS)
        .expect("the selected schedule encodes");
    let payers = signers
        .iter()
        .map(|(_, public)| {
            vault_payer(&CostSignature {
                value: Some(Value::Ground(principal_ground_v61(&public.bytes))),
            })
            .expect("a principal signature has a vault payer")
        })
        .collect::<Vec<_>>();
    let lanes = permissions
        .iter()
        .map(|lanes| {
            lanes
                .iter()
                .map(|(signature, classes)| {
                    let authority =
                        cost_signature_to_sig(signature).expect("a lane signature converts");
                    let location = SignatureChannel::from_sig(&authority).par.encode_to_vec();
                    (authority, location, classes.clone())
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let sources = payers
        .iter()
        .zip(&lanes)
        .enumerate()
        .map(|(index, (payer, lanes))| {
            let mut keys = Vec::new();
            for (authority, location, classes) in lanes {
                for class in classes {
                    keys.push(
                        PhloResource {
                            location,
                            class: *class,
                            acquisition_terms: &acquisition_terms,
                            authority,
                        }
                        .wire_key(PhloExecutionLimits {
                            resource_entries: 1,
                            authority_nodes: limits.funding.authority_nodes,
                            key_bytes: limits.funding.wire.field_bytes,
                        })
                        .expect("a resource permission encodes"),
                    );
                }
            }
            let permission_count = keys.len();
            PhloSourcePolicyV1::new(
                &payer.custody_key,
                source_cap,
                source_cap,
                index == 0,
                keys,
                PhloSourceLimits {
                    wire: limits.funding.wire,
                    resource_permissions: permission_count,
                    authority_nodes: limits.funding.authority_nodes,
                },
            )
            .expect("the sponsored source policy is valid")
        })
        .collect::<Vec<_>>();
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
            total_exposure: u128::from(phlo_limit) * 2 + 1,
            sources,
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
        parameters: Vec::new(),
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(
        body,
        funding,
        i64::try_from(phlo_limit).expect("the phlo limit fits an i64"),
        2,
        limits,
    )
    .expect("the sponsored payload is valid");
    let mut cosigners = signers
        .iter()
        .map(|(_, public)| Cosigner {
            pk: public.clone(),
            sig: prost::bytes::Bytes::from_static(&[1]),
            sig_algorithm: Box::new(Secp256k1),
        })
        .collect::<Vec<_>>();
    let hash = Cosigned::envelope_signing_hash(
        &payload,
        &cosigners,
        u32::try_from(cosigners.len()).expect("a small signer count"),
        &Secp256k1::name(),
    )
    .expect("the envelope hash computes");
    for (cosigner, (private, _)) in cosigners.iter_mut().zip(signers) {
        cosigner.sig = Secp256k1.sign(&hash, &private.bytes).into();
    }
    let signed = Cosigned::from_envelope_signed_data(payload, cosigners)
        .expect("the signers cosign the offer");
    OfferedFundedDeploy::to_proto(&signed).expect("a signed offer encodes")
}

/// The inverted test of the 64-source case. A lane that the sources permit
/// is a client lane, because the client's purses may pay it (P2
/// `prop:local-suff`, P2:1520-1527). Four sources each permit one class of
/// the lane `payer`, at whose purse no source is located. The estimate stays
/// complete. The offer runs under `Certificate` with the bound B and holds
/// B·phloPrice + 1. Each class lands on the source that permits it, and the
/// fee lands on the first source.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lane_that_sources_permit_is_a_client_lane_with_exact_holds() {
    let sponsors = sponsor_keys(4);
    let genesis = super::offered_v6_genesis_with_signers(2, &sponsors).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let payer_lane = spelled_signature("payer");
    let permissions = (0..4)
        .map(|class| vec![(payer_lane.clone(), vec![class])])
        .collect::<Vec<_>>();
    let offer = sponsored_offer(
        &sponsors,
        1,
        "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]",
        2_000_000,
        5_000_000,
        &permissions,
    );
    let (counts, deploy_lane) = estimated_counts(&nodes[0], &offer).await;
    let payer_key = lane_of(&payer_lane);
    assert!(
        counts.keys().all(|(lane, _)| *lane == payer_key),
        "only the lane payer has demand: {counts:?}"
    );
    assert_ne!(payer_key, deploy_lane);
    let class_counts = (0..4)
        .map(|class| counts.get(&(payer_key, class)).copied().unwrap_or(0))
        .collect::<Vec<_>>();
    assert!(
        class_counts.iter().filter(|count| **count > 0).count() >= 2,
        "the lane payer has demand in at least two classes: {class_counts:?}"
    );
    let estimate = class_counts.iter().sum::<u64>();
    let gate = direct_gate(
        &nodes[0],
        &genesis,
        &offer,
        &genesis.genesis_block.body.state.post_state_hash,
    )
    .await
    .expect("the check accepts the offer");
    assert_eq!(gate.kind(), &OfferedEstimateKind::Complete);
    assert_eq!(gate.bound_source(), NativeBoundSource::Certificate);
    assert_eq!(gate.estimate_phlo(), estimate);
    assert_eq!(gate.resource_bound(), estimate);
    assert_eq!(gate.total_hold(), u128::from(estimate) * 2 + 1);
    let expected = sponsors
        .iter()
        .zip(&class_counts)
        .enumerate()
        .map(|(index, ((_, public), count))| {
            let custody = vault_payer(&CostSignature {
                value: Some(Value::Ground(principal_ground_v61(&public.bytes))),
            })
            .expect("a principal signature has a vault payer")
            .custody_key;
            (custody, u128::from(*count) * 2 + u128::from(index == 0))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        gate.holds()
            .iter()
            .copied()
            .collect::<std::collections::BTreeMap<_, _>>(),
        expected,
        "each class lands on the source that permits it, and the fee on the first source"
    );
    let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
    assert!(!block.body.deploys[0].is_failed());
    let (decision, receipt) = accepted_with_one_decision(&mut nodes, &block, &offer).await;
    assert_eq!(decision, decision_of(&gate));
    assert!(
        receipt.fresh_phlo <= estimate,
        "the run stays within its certified bound"
    );
}

/// The loophole control for a permitted lane. The owner's source permits only
/// the class 0 of the lane `payer`, which has demand in more classes. The lane
/// is a client lane, so the classes without a permission reject the offer
/// before execution with "insufficient phlogiston". The same term with every
/// class of `payer` permitted is accepted under `Certificate`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_permitted_lane_without_a_source_for_a_class_with_demand_rejects_before_execution() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("a single-node network starts");
    let payer_lane = spelled_signature("payer");
    let term = "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]";
    let restricted = permissioned_offer(
        &genesis.genesis_vaults[0],
        1,
        term,
        2_000_000,
        5_000_000,
        &[0, 1, 2, 3],
        &[(payer_lane.clone(), &[0])],
    );
    let permitted = permissioned_offer(
        &genesis.genesis_vaults[2],
        2,
        term,
        2_000_000,
        5_000_000,
        &[0, 1, 2, 3],
        &[(payer_lane.clone(), &[0, 1, 2, 3])],
    );
    let (counts, _) = estimated_counts(&nodes[0], &restricted).await;
    assert!(
        (1..4).any(|class| counts
            .get(&(lane_of(&payer_lane), class))
            .copied()
            .unwrap_or(0)
            > 0),
        "the lane payer has demand outside the class 0: {counts:?}"
    );
    admit(&nodes[0], &restricted).await;
    admit(&nodes[0], &permitted).await;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer includes the permitted offer");
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        permitted.deploy_id.as_ref()
    );
    let reason = recorded_rejection(&nodes[0], &restricted);
    assert!(reason.contains("insufficient phlogiston"), "{reason}");
    assert_eq!(
        records(&nodes[0], &restricted, OfferedUsageKind::PlayRuntimeEntry),
        0,
        "the restricted offer never runs"
    );
    assert_eq!(
        checks(&nodes[0], &restricted, OfferedCheckRole::Play),
        vec![OfferedCheckRecord::Rejected]
    );
    assert!(matches!(
        checks(&nodes[0], &permitted, OfferedCheckRole::Play)[..],
        [OfferedCheckRecord::Accepted {
            complete: true,
            bound_source: NativeBoundSource::Certificate,
            ..
        }]
    ));
}

/// Review finding M-3 when a source permits the lane `alice`. The layer
/// `{% Nil %}[alice]` inside sent data is then the client's sponsored demand.
/// The lane is a client lane, the estimate stays complete, and B counts the
/// demand of `alice`. The offer holds B·phloPrice + 1 under `Certificate`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_send_of_a_signed_process_holds_that_lane_when_a_source_permits_it() {
    let genesis = super::offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("a two-node network starts");
    let payer = genesis.genesis_vaults[0].clone();
    let alice = spelled_signature("alice");
    let offer = permissioned_offer(
        &payer,
        1,
        "new x in { x!({% Nil %}[alice]) }",
        2_000_000,
        5_000_000,
        &[0, 1, 2, 3],
        &[(alice.clone(), &[0, 1, 2, 3])],
    );
    let (counts, deploy_lane) = estimated_counts(&nodes[0], &offer).await;
    let alice_key = lane_of(&alice);
    assert!(
        counts
            .iter()
            .any(|((lane, _), count)| *lane == alice_key && *count > 0),
        "the plan counts the signed layer inside the data"
    );
    assert!(counts
        .keys()
        .all(|(lane, _)| *lane == alice_key || *lane == deploy_lane));
    let estimate = counts.values().sum::<u64>();
    let gate = direct_gate(
        &nodes[0],
        &genesis,
        &offer,
        &genesis.genesis_block.body.state.post_state_hash,
    )
    .await
    .expect("the check accepts the offer");
    assert_eq!(gate.kind(), &OfferedEstimateKind::Complete);
    assert_eq!(gate.bound_source(), NativeBoundSource::Certificate);
    assert_eq!(
        gate.estimate_phlo(),
        estimate,
        "B counts the demand of alice"
    );
    assert_eq!(gate.total_hold(), u128::from(estimate) * 2 + 1);
    let (block, _) = super::offered_block(&mut nodes, 0, offer.clone()).await;
    assert!(!block.body.deploys[0].is_failed());
    let (decision, _) = accepted_with_one_decision(&mut nodes, &block, &offer).await;
    assert_eq!(decision, decision_of(&gate));
}
