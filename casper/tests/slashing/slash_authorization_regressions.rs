// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Slash-authorization regression suite.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §9 + §10.
// Theorems: T-9.8 (authorization predicate), T-9.7 (seq-num density).
// Rocq: formal/rocq/slashing/theories/BugFixSlashAuthorization.v,
// BugFixSeqArithmetic.v, BugFixSeqNumDensity.v.
//
// This is the production-path companion to the predicate-level tests in
// `slashing_authorization.rs::kani_proofs`. Every rejection rule in
// `validate_received_slash_deploys` has at least one regression here:
//   - issuer ≠ block.sender,
//   - target_activation_epoch ≠ current_epoch,
//   - invalid_block_hash unknown to the DAG,
//   - referenced block not flagged invalid,
//   - offender currently unbonded,
//   - duplicate (offender, target_epoch) in same block.
// Boundary helpers (`checked_base_seq`, `checked_next_seq`,
// `epoch_for_block_number`) are exercised against hostile inputs at the
// same time so a single failure points at the specific rule.

use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::errors::CasperError;
use casper::rust::slashing_authorization::{
    authorized_slash_candidates, checked_base_seq, checked_next_seq, epoch_for_block_number,
    has_slash_evidence, validate_received_slash_deploys, CanonicalSlashAuthority, SlashAuthError,
};
use casper::rust::validate::Validate;
use crypto::rust::public_key::PublicKey;
use dashmap::DashSet;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, SystemDeployData};
use proptest::prelude::*;
use rspace_plus_plus::rspace::history::Either;

use super::detector_totality_helpers::{block, justification, DetectorFixture};

fn put_block(
    fixture: &DetectorFixture,
    block: &models::rust::casper::protocol::casper_message::BlockMessage,
    invalid: bool,
) {
    fixture
        .block_store
        .put_block_message(block)
        .expect("store block");
    fixture
        .dag_storage
        .insert(
            block,
            if invalid {
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Invalid
            } else {
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal
            },
        )
        .expect("insert block");
}

fn snapshot_from_fixture(
    fixture: &DetectorFixture,
    max_block_num: i64,
    epoch_length: i32,
    bonded: Vec<prost::bytes::Bytes>,
) -> CasperSnapshot {
    let bonds_map = bonded
        .iter()
        .map(|validator| (validator.clone(), 100))
        .collect::<HashMap<_, _>>();
    let bond_generations = fixture
        .validators
        .iter()
        .map(|validator| (validator.clone(), BondGeneration::GENESIS))
        .collect::<HashMap<_, _>>();

    CasperSnapshot {
        dag: fixture
            .dag_storage
            .get_representation()
            .expect("dag representation"),
        last_finalized_block: prost::bytes::Bytes::new(),
        lca: prost::bytes::Bytes::new(),
        tips: vec![],
        parents: vec![],
        justifications: Vec::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        rejected_in_scope: Arc::new(DashSet::new()),
        max_block_num,
        max_seq_nums: HashMap::new(),
        finalized_floor_bonds: Vec::new(),
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf {
                epoch_length,
                ..CasperShardConf::new()
            },
            bonds_map,
            bond_generations,
            active_validators: bonded,
        },
        consensus_context:
            casper::rust::causal_equivocation::CertifiedConsensusContext::pre_genesis(),
        finalized_floor_certificate: None,
    }
}

fn authority(
    snapshot: &CasperSnapshot,
    bonds: &HashMap<prost::bytes::Bytes, i64>,
) -> CanonicalSlashAuthority {
    CanonicalSlashAuthority::from_parts(
        prost::bytes::Bytes::new(),
        bonds.clone(),
        snapshot.on_chain_state.bond_generations.clone(),
    )
    .expect("canonical authority")
}

fn candidates(
    snapshot: &CasperSnapshot,
    bonds: &HashMap<prost::bytes::Bytes, i64>,
) -> Result<Vec<casper::rust::slashing_authorization::AuthorizedSlashCandidate>, CasperError> {
    authorized_slash_candidates(
        snapshot,
        snapshot
            .max_block_num
            .checked_add(1)
            .expect("next block number"),
        &authority(snapshot, bonds),
    )
}

fn validate_slashes(
    block: &models::rust::casper::protocol::casper_message::BlockMessage,
    snapshot: &CasperSnapshot,
    bonds: &HashMap<prost::bytes::Bytes, i64>,
) -> Result<(), CasperError> {
    validate_received_slash_deploys(block, snapshot, &authority(snapshot, bonds))
}

fn slash_deploy(
    invalid_block_hash: prost::bytes::Bytes,
    issuer: prost::bytes::Bytes,
    target_activation_epoch: i64,
) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash,
            equivocation_block_hash: None,
            issuer_public_key: PublicKey::from_bytes(&issuer),
            target_activation_epoch,
            target_bond_generation: BondGeneration::GENESIS,
        },
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    }
}

fn equivocation_slash_deploy(
    first_block_hash: prost::bytes::Bytes,
    second_block_hash: prost::bytes::Bytes,
    issuer: prost::bytes::Bytes,
    target_activation_epoch: i64,
) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::create_equivocation_slash(
            first_block_hash,
            second_block_hash,
            PublicKey::from_bytes(&issuer),
            target_activation_epoch,
            BondGeneration::GENESIS,
        ),
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    }
}

fn slash_block(
    hash_byte: u8,
    proposer: prost::bytes::Bytes,
    block_number: i64,
    invalid_block_hash: prost::bytes::Bytes,
    issuer: prost::bytes::Bytes,
    target_activation_epoch: i64,
    validators: Vec<prost::bytes::Bytes>,
) -> models::rust::casper::protocol::casper_message::BlockMessage {
    let mut block = block(
        hash_byte,
        proposer,
        i32::try_from(block_number).unwrap_or(0),
        vec![],
        validators,
    );
    block.body.state.block_number = block_number;
    block.body.system_deploys = vec![slash_deploy(
        invalid_block_hash,
        issuer,
        target_activation_epoch,
    )];
    block
}

#[tokio::test]
async fn stale_invalid_evidence_is_not_an_authorized_slash_candidate() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let invalid = block(30, offender.clone(), 5, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot = snapshot_from_fixture(&fixture, 10, 10, vec![offender]);
    let candidates = candidates(&snapshot, &snapshot.on_chain_state.bonds_map).expect("candidates");

    assert!(
        candidates.is_empty(),
        "epoch-scoped authorization must not propose slash deploys from stale evidence"
    );
}

#[tokio::test]
async fn current_epoch_invalid_evidence_is_authorized_once_per_offender() {
    for reverse in [false, true] {
        let fixture = DetectorFixture::new().await;
        let offender = fixture.validators[0].clone();
        let invalid_a = block(31, offender.clone(), 11, vec![], fixture.validators.clone());
        let invalid_b = block(32, offender.clone(), 12, vec![], fixture.validators.clone());
        let evidence = if reverse {
            [&invalid_b, &invalid_a]
        } else {
            [&invalid_a, &invalid_b]
        };
        for invalid in evidence {
            put_block(&fixture, invalid, true);
        }

        let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![offender.clone()]);
        let candidates =
            candidates(&snapshot, &snapshot.on_chain_state.bonds_map).expect("candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].offender, offender);
        assert_eq!(candidates[0].target_activation_epoch.get(), 1);
        assert_eq!(
            candidates[0].invalid_block_hash,
            invalid_a
                .block_hash
                .clone()
                .min(invalid_b.block_hash.clone())
        );
    }
}

#[tokio::test]
async fn neglected_equivocation_uses_pre_state_bonds_across_local_invalid_views() {
    for reverse_invalid in [false, true] {
        let fixture = DetectorFixture::new().await;
        let offender = fixture.validators[0].clone();
        let child_a = block(41, offender.clone(), 1, vec![], fixture.validators.clone());
        let child_b = block(42, offender.clone(), 1, vec![], fixture.validators.clone());
        put_block(&fixture, &child_a, reverse_invalid);
        put_block(&fixture, &child_b, !reverse_invalid);
        fixture.add_record(0, 0, &[]);

        let candidate = block(
            43,
            fixture.validators[4].clone(),
            2,
            vec![
                justification(fixture.validators[2].clone(), child_a.block_hash.clone()),
                justification(fixture.validators[3].clone(), child_b.block_hash.clone()),
            ],
            fixture.validators[1..].to_vec(),
        );
        let bonded_pre_state = fixture
            .validators
            .iter()
            .cloned()
            .map(|validator| (validator, 100))
            .collect();
        let unbonded_pre_state = fixture
            .validators
            .iter()
            .skip(1)
            .cloned()
            .map(|validator| (validator, 100))
            .collect();

        assert_eq!(
            fixture
                .check_with_pre_state_bonds(&candidate, &bonded_pre_state)
                .await,
            Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation))
        );
        assert_eq!(
            fixture
                .check_with_pre_state_bonds(&candidate, &unbonded_pre_state)
                .await,
            Either::Right(casper::rust::block_status::ValidBlock::Valid)
        );
    }
}

#[tokio::test]
async fn no_candidates_from_a_view_lagging_the_finalized_frontier() {
    // joiner7's shape from CI run 32588262605 (amd64-docker 35b31728): a
    // restored joiner mid-catch-up whose finalizer had already advanced its
    // own LFB into a later epoch while the proposal snapshot's
    // justification-derived view still sat in an earlier one. The slashes it
    // issued matched its lagging view's epoch and were rejected as
    // non-current by every live peer — pure DOA noise that then minted
    // UnauthorizedSlashDeploy verdicts on the carriers.
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let finalized_sender = fixture.validators[1].clone();

    // Evidence in the VIEW's current epoch (view max #3 → proposing #4 →
    // epoch 0 at length 10; evidence #2 → epoch 0): mintable today.
    let invalid = block(33, offender.clone(), 2, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    // The node's own finalized frontier is a full epoch ahead of that view.
    let lfb = block(34, finalized_sender, 13, vec![], fixture.validators.clone());
    put_block(&fixture, &lfb, false);

    let mut snapshot = snapshot_from_fixture(&fixture, 3, 10, vec![offender]);
    snapshot.last_finalized_block = lfb.block_hash.clone();

    let candidates = candidates(&snapshot, &snapshot.on_chain_state.bonds_map).expect("candidates");
    assert!(
        candidates.is_empty(),
        "a view trailing this node's own finalized frontier is mid-catch-up \
         and must not issue slash deploys"
    );
}

#[tokio::test]
async fn candidates_flow_when_the_view_matches_its_finalized_frontier() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let finalized_sender = fixture.validators[1].clone();

    let invalid = block(35, offender.clone(), 2, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    // LFB in the SAME epoch as the view: not lagging, issuance proceeds.
    let lfb = block(36, finalized_sender, 3, vec![], fixture.validators.clone());
    put_block(&fixture, &lfb, false);

    let mut snapshot = snapshot_from_fixture(&fixture, 3, 10, vec![offender.clone()]);
    snapshot.last_finalized_block = lfb.block_hash.clone();

    let candidates = candidates(&snapshot, &snapshot.on_chain_state.bonds_map).expect("candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].offender, offender);
}

#[tokio::test]
async fn received_stale_slash_deploy_is_rejected_before_replay() {
    // Doubles as the JSON-back-compat negative-path test: a legacy JSON
    // payload that omits the `target_activation_epoch` field deserializes
    // with default 0 (see
    // `node/src/rust/api/serde_types/system_deploy_info.rs::tests::
    //  slash_system_deploy_json_defaults_missing_target_activation_epoch`).
    // The contract pinned here is that the default value must NOT widen
    // the slashable surface — when `current_epoch > 0` the receive-side
    // predicate rejects the slash as EpochMismatch, propagating up as
    // `InvalidBlock::UnauthorizedSlashDeploy` (slashing the proposer, not
    // the target).
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(33, offender.clone(), 5, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot = snapshot_from_fixture(&fixture, 10, 10, vec![offender, proposer.clone()]);
    let mut slash_block = block(34, proposer.clone(), 11, vec![], fixture.validators.clone());
    slash_block.body.state.block_number = 11;
    slash_block.body.system_deploys = vec![slash_deploy(invalid.block_hash.clone(), proposer, 0)];

    let err = validate_slashes(&slash_block, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("reject stale");
    // Per-variant pattern match (regression hardening: prior `.contains()`
    // assertion would silently pass any error whose Display includes
    // "non-current epoch", masking a wrong-variant rerouting).
    assert!(
        matches!(
            err,
            CasperError::SlashAuth(SlashAuthError::EpochMismatch { .. })
        ),
        "expected SlashAuthError::EpochMismatch, got {err:?}"
    );
    // Operator-diagnostic-text stability check kept as a paired assertion.
    assert!(
        err.to_string().contains("non-current epoch"),
        "expected stale epoch rejection, got {err}"
    );
}

#[tokio::test]
async fn current_epoch_received_slash_deploy_is_accepted() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(36, offender.clone(), 11, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot =
        snapshot_from_fixture(&fixture, 11, 10, vec![offender.clone(), proposer.clone()]);
    let slash_block = slash_block(
        37,
        proposer.clone(),
        11,
        invalid.block_hash.clone(),
        proposer,
        1,
        fixture.validators.clone(),
    );

    validate_slashes(&slash_block, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect("current slash deploy");
}

#[tokio::test]
async fn objective_equivocation_slash_is_arrival_order_independent() {
    let forward = DetectorFixture::new().await;
    let reverse = DetectorFixture::new().await;
    let offender = forward.validators[0].clone();
    let proposer = forward.validators[1].clone();
    assert_eq!(reverse.validators[0], offender);
    assert_eq!(reverse.validators[1], proposer);

    let first = block(53, offender.clone(), 11, vec![], forward.validators.clone());
    let second = block(54, offender.clone(), 11, vec![], forward.validators.clone());
    put_block(&forward, &first, false);
    put_block(&forward, &second, true);
    put_block(&reverse, &second, false);
    put_block(&reverse, &first, true);

    let forward_snapshot =
        snapshot_from_fixture(&forward, 11, 10, vec![offender.clone(), proposer.clone()]);
    let reverse_snapshot =
        snapshot_from_fixture(&reverse, 11, 10, vec![offender.clone(), proposer.clone()]);
    let forward_candidates = candidates(
        &forward_snapshot,
        &forward_snapshot.on_chain_state.bonds_map,
    )
    .expect("forward candidates");
    let reverse_candidates = candidates(
        &reverse_snapshot,
        &reverse_snapshot.on_chain_state.bonds_map,
    )
    .expect("reverse candidates");
    assert_eq!(forward_candidates, reverse_candidates);
    assert_eq!(forward_candidates.len(), 1);
    assert_eq!(forward_candidates[0].offender, offender);
    assert_eq!(
        (
            forward_candidates[0].invalid_block_hash.clone(),
            forward_candidates[0]
                .equivocation_block_hash
                .clone()
                .expect("paired evidence"),
        ),
        (first.block_hash.clone(), second.block_hash.clone())
    );

    let mut slash = block(55, proposer.clone(), 11, vec![], forward.validators.clone());
    slash.body.state.block_number = 11;
    slash.body.system_deploys = vec![equivocation_slash_deploy(
        second.block_hash,
        first.block_hash,
        proposer,
        1,
    )];
    validate_slashes(
        &slash,
        &forward_snapshot,
        &forward_snapshot.on_chain_state.bonds_map,
    )
    .expect("forward accepts objective pair");
    validate_slashes(
        &slash,
        &reverse_snapshot,
        &reverse_snapshot.on_chain_state.bonds_map,
    )
    .expect("reverse accepts objective pair");
}

#[tokio::test]
async fn objective_equivocation_slash_rejects_noncanonical_or_mismatched_pairs() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let other = fixture.validators[2].clone();
    let first = block(56, offender.clone(), 11, vec![], fixture.validators.clone());
    let second = block(57, offender.clone(), 11, vec![], fixture.validators.clone());
    let wrong_sender = block(58, other.clone(), 11, vec![], fixture.validators.clone());
    let wrong_sequence = block(59, offender.clone(), 12, vec![], fixture.validators.clone());
    for evidence in [&first, &second, &wrong_sender, &wrong_sequence] {
        put_block(&fixture, evidence, evidence.block_hash != first.block_hash);
    }
    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![offender, proposer.clone(), other]);

    let mut noncanonical = block(60, proposer.clone(), 11, vec![], fixture.validators.clone());
    noncanonical.body.state.block_number = 11;
    noncanonical.body.system_deploys = vec![ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash: second.block_hash.clone(),
            equivocation_block_hash: Some(first.block_hash.clone()),
            issuer_public_key: PublicKey::from_bytes(&proposer),
            target_activation_epoch: 1,
            target_bond_generation: BondGeneration::GENESIS,
        },
        pre_state_hash: Vec::<u8>::new().into(),
        post_state_hash: Vec::<u8>::new().into(),
    }];
    let err = validate_slashes(&noncanonical, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("noncanonical pair");
    assert!(matches!(
        err,
        CasperError::SlashAuth(SlashAuthError::NonCanonicalEquivocationPair { .. })
    ));

    for conflicting_hash in [wrong_sender.block_hash, wrong_sequence.block_hash] {
        let mut mismatched = noncanonical.clone();
        mismatched.body.system_deploys = vec![equivocation_slash_deploy(
            first.block_hash.clone(),
            conflicting_hash,
            proposer.clone(),
            1,
        )];
        let err = validate_slashes(&mismatched, &snapshot, &snapshot.on_chain_state.bonds_map)
            .expect_err("mismatched pair");
        assert!(matches!(
            err,
            CasperError::SlashAuth(SlashAuthError::EquivocationEvidenceMismatch)
        ));
    }
}

#[tokio::test]
async fn cross_epoch_objective_pair_never_falls_back_to_local_unary_evidence() {
    let forward = DetectorFixture::new().await;
    let reverse = DetectorFixture::new().await;
    let offender = forward.validators[0].clone();
    let mut prior_epoch = block(61, offender.clone(), 11, vec![], forward.validators.clone());
    prior_epoch.body.state.block_number = 9;
    let mut current_epoch = block(62, offender.clone(), 11, vec![], forward.validators.clone());
    current_epoch.body.state.block_number = 10;
    put_block(&forward, &prior_epoch, false);
    put_block(&forward, &current_epoch, true);
    put_block(&reverse, &current_epoch, false);
    put_block(&reverse, &prior_epoch, true);
    let forward_snapshot = snapshot_from_fixture(&forward, 10, 10, vec![offender.clone()]);
    let reverse_snapshot = snapshot_from_fixture(&reverse, 10, 10, vec![offender]);

    assert!(candidates(
        &forward_snapshot,
        &forward_snapshot.on_chain_state.bonds_map,
    )
    .expect("forward candidates")
    .is_empty());
    assert!(candidates(
        &reverse_snapshot,
        &reverse_snapshot.on_chain_state.bonds_map,
    )
    .expect("reverse candidates")
    .is_empty());
}

#[tokio::test]
async fn candidate_pair_is_canonicalized_within_the_target_lifetime() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let mut old = block(63, offender.clone(), 11, vec![], fixture.validators.clone());
    old.body.state.block_number = 9;
    let mut current_first = block(64, offender.clone(), 11, vec![], fixture.validators.clone());
    current_first.body.state.block_number = 10;
    let mut current_second = block(65, offender.clone(), 11, vec![], fixture.validators.clone());
    current_second.body.state.block_number = 11;
    for evidence in [&old, &current_first, &current_second] {
        put_block(&fixture, evidence, false);
    }
    let snapshot = snapshot_from_fixture(&fixture, 10, 10, vec![offender.clone()]);

    let candidates = candidates(&snapshot, &snapshot.on_chain_state.bonds_map)
        .expect("current-lifetime candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].offender, offender);
    assert_eq!(candidates[0].invalid_block_hash, current_first.block_hash);
    assert_eq!(
        candidates[0].equivocation_block_hash,
        Some(current_second.block_hash)
    );
}

#[tokio::test]
async fn objective_pair_activates_authority_without_invalid_index() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let first = block(69, offender.clone(), 11, vec![], fixture.validators.clone());
    let second = block(70, offender.clone(), 11, vec![], fixture.validators.clone());
    put_block(&fixture, &first, false);
    put_block(&fixture, &second, false);
    let snapshot = snapshot_from_fixture(&fixture, 10, 10, vec![offender.clone()]);

    assert!(snapshot.dag.invalid_blocks().is_empty());
    assert!(has_slash_evidence(&snapshot));
    let candidates =
        candidates(&snapshot, &snapshot.on_chain_state.bonds_map).expect("pair-only candidate");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].offender, offender);
    assert_eq!(candidates[0].invalid_block_hash, first.block_hash);
    assert_eq!(
        candidates[0].equivocation_block_hash,
        Some(second.block_hash)
    );
}

#[tokio::test]
async fn objective_pair_selection_is_permutation_invariant_after_epoch_filtering() {
    let permutations = [
        [0usize, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for order in permutations {
        let fixture = DetectorFixture::new().await;
        let offender = fixture.validators[0].clone();
        let mut old = block(71, offender.clone(), 11, vec![], fixture.validators.clone());
        old.body.state.block_number = 9;
        let mut current_first = block(72, offender.clone(), 11, vec![], fixture.validators.clone());
        current_first.body.state.block_number = 10;
        let mut current_second =
            block(73, offender.clone(), 11, vec![], fixture.validators.clone());
        current_second.body.state.block_number = 11;
        let evidence = [&old, &current_first, &current_second];
        for index in order {
            put_block(&fixture, evidence[index], false);
        }
        let snapshot = snapshot_from_fixture(&fixture, 10, 10, vec![offender]);
        let selected = candidates(&snapshot, &snapshot.on_chain_state.bonds_map)
            .expect("permutation candidate");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].invalid_block_hash, current_first.block_hash);
        assert_eq!(
            selected[0].equivocation_block_hash,
            Some(current_second.block_hash.clone())
        );
    }
}

#[tokio::test]
async fn received_cross_epoch_objective_pair_is_rejected() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let mut old = block(74, offender.clone(), 11, vec![], fixture.validators.clone());
    old.body.state.block_number = 9;
    let mut current = block(75, offender.clone(), 11, vec![], fixture.validators.clone());
    current.body.state.block_number = 10;
    put_block(&fixture, &old, false);
    put_block(&fixture, &current, false);
    let snapshot =
        snapshot_from_fixture(&fixture, 10, 10, vec![offender.clone(), proposer.clone()]);
    let mut slash = block(76, proposer.clone(), 11, vec![], fixture.validators.clone());
    slash.body.state.block_number = 11;
    slash.body.system_deploys = vec![equivocation_slash_deploy(
        old.block_hash,
        current.block_hash,
        proposer,
        1,
    )];

    let error = validate_slashes(&slash, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("cross-epoch pair");
    assert!(matches!(
        error,
        CasperError::SlashAuth(SlashAuthError::EvidenceEpochMismatch { .. })
    ));
}

#[tokio::test]
async fn structural_collision_suppresses_only_its_own_unary_fallback() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let mut old = block(66, offender.clone(), 11, vec![], fixture.validators.clone());
    old.body.state.block_number = 9;
    let mut current = block(67, offender.clone(), 11, vec![], fixture.validators.clone());
    current.body.state.block_number = 10;
    let mut independent = block(68, offender.clone(), 12, vec![], fixture.validators.clone());
    independent.body.state.block_number = 10;
    put_block(&fixture, &old, false);
    put_block(&fixture, &current, true);
    put_block(&fixture, &independent, true);
    let snapshot = snapshot_from_fixture(&fixture, 10, 10, vec![offender.clone()]);

    let candidates = candidates(&snapshot, &snapshot.on_chain_state.bonds_map)
        .expect("independent unary candidate");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].offender, offender);
    assert_eq!(candidates[0].invalid_block_hash, independent.block_hash);
    assert_eq!(candidates[0].equivocation_block_hash, None);
}

#[tokio::test]
async fn canonical_pre_state_zero_overrides_positive_snapshot_bond() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(47, offender.clone(), 11, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot =
        snapshot_from_fixture(&fixture, 11, 10, vec![offender.clone(), proposer.clone()]);
    let canonical_bonds = HashMap::from([(proposer.clone(), 100)]);
    let candidates = candidates(&snapshot, &canonical_bonds).expect("candidates");
    assert!(candidates.is_empty());

    let slash_block = slash_block(
        48,
        proposer.clone(),
        11,
        invalid.block_hash,
        proposer,
        1,
        fixture.validators.clone(),
    );
    let err = validate_slashes(&slash_block, &snapshot, &canonical_bonds)
        .expect_err("canonical zero bond must reject slash");
    assert!(matches!(
        err,
        CasperError::SlashAuth(SlashAuthError::TargetNotBonded { .. })
    ));
}

#[tokio::test]
async fn canonical_pre_state_positive_overrides_zero_snapshot_bond() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(49, offender.clone(), 11, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![proposer.clone()]);
    let canonical_bonds = HashMap::from([(offender.clone(), 100), (proposer.clone(), 100)]);
    let candidates = candidates(&snapshot, &canonical_bonds).expect("candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].offender, offender);

    let slash_block = slash_block(
        50,
        proposer.clone(),
        11,
        invalid.block_hash,
        proposer,
        1,
        fixture.validators.clone(),
    );
    validate_slashes(&slash_block, &snapshot, &canonical_bonds)
        .expect("canonical positive bond must authorize slash");
}

#[tokio::test]
async fn canonical_pre_state_generation_overrides_stale_snapshot_generation() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(77, offender.clone(), 11, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let mut snapshot =
        snapshot_from_fixture(&fixture, 11, 10, vec![offender.clone(), proposer.clone()]);
    snapshot.on_chain_state.bond_generations.insert(
        offender.clone(),
        BondGeneration::GENESIS.next().expect("stale generation"),
    );
    let canonical = CanonicalSlashAuthority::from_parts(
        prost::bytes::Bytes::from_static(b"canonical-pre-state"),
        HashMap::from([(offender.clone(), 100), (proposer.clone(), 100)]),
        HashMap::from([
            (offender.clone(), BondGeneration::GENESIS),
            (proposer.clone(), BondGeneration::GENESIS),
        ]),
    )
    .expect("canonical authority");

    let selected = authorized_slash_candidates(&snapshot, 12, &canonical).expect("candidate");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].offender, offender);

    let slash_block = slash_block(
        78,
        proposer.clone(),
        12,
        invalid.block_hash,
        proposer,
        1,
        fixture.validators.clone(),
    );
    validate_received_slash_deploys(&slash_block, &snapshot, &canonical)
        .expect("canonical generation authorizes slash");
}

#[tokio::test]
async fn received_slash_deploy_rejects_issuer_mismatch() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let wrong_issuer = fixture.validators[2].clone();
    let invalid = block(38, offender.clone(), 11, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![offender, proposer.clone()]);
    let slash_block = slash_block(
        39,
        proposer,
        11,
        invalid.block_hash.clone(),
        wrong_issuer,
        1,
        fixture.validators.clone(),
    );

    let err = validate_slashes(&slash_block, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("reject issuer");
    assert!(
        matches!(
            err,
            CasperError::SlashAuth(SlashAuthError::IssuerMismatch { .. })
        ),
        "expected SlashAuthError::IssuerMismatch, got {err:?}"
    );
    assert!(err.to_string().contains("issuer does not match"));
}

#[tokio::test]
async fn received_slash_deploy_rejects_unknown_invalid_hash() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![offender, proposer.clone()]);
    let slash_block = slash_block(
        40,
        proposer.clone(),
        11,
        prost::bytes::Bytes::from(vec![222; 32]),
        proposer,
        1,
        fixture.validators.clone(),
    );

    let err = validate_slashes(&slash_block, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("reject unknown");
    assert!(
        matches!(
            err,
            CasperError::SlashAuth(SlashAuthError::ReferencesUnknownBlock { .. })
        ),
        "expected SlashAuthError::ReferencesUnknownBlock, got {err:?}"
    );
    assert!(err.to_string().contains("unknown invalid block"));
}

#[tokio::test]
async fn received_slash_deploy_rejects_valid_block_reference() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let valid = block(41, offender.clone(), 1, vec![], fixture.validators.clone());
    put_block(&fixture, &valid, false);

    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![offender, proposer.clone()]);
    let slash_block = slash_block(
        42,
        proposer.clone(),
        11,
        valid.block_hash.clone(),
        proposer,
        1,
        fixture.validators.clone(),
    );

    let err = validate_slashes(&slash_block, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("reject valid");
    assert!(
        matches!(
            err,
            CasperError::SlashAuth(SlashAuthError::ReferencesValidBlock { .. })
        ),
        "expected SlashAuthError::ReferencesValidBlock, got {err:?}"
    );
    assert!(err.to_string().contains("valid block"));
}

#[tokio::test]
async fn received_slash_deploy_rejects_unbonded_target() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(43, offender, 11, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![proposer.clone()]);
    let slash_block = slash_block(
        44,
        proposer.clone(),
        11,
        invalid.block_hash.clone(),
        proposer,
        1,
        fixture.validators.clone(),
    );

    let err = validate_slashes(&slash_block, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("reject unbonded");
    assert!(
        matches!(
            err,
            CasperError::SlashAuth(SlashAuthError::TargetNotBonded { .. })
        ),
        "expected SlashAuthError::TargetNotBonded, got {err:?}"
    );
    assert!(err.to_string().contains("not currently bonded"));
}

#[tokio::test]
async fn received_slash_deploy_rejects_duplicate_target_in_one_block() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(45, offender.clone(), 11, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![offender, proposer.clone()]);
    let mut slash_block = slash_block(
        46,
        proposer.clone(),
        11,
        invalid.block_hash.clone(),
        proposer.clone(),
        1,
        fixture.validators.clone(),
    );
    slash_block
        .body
        .system_deploys
        .push(slash_deploy(invalid.block_hash.clone(), proposer, 1));

    let err = validate_slashes(&slash_block, &snapshot, &snapshot.on_chain_state.bonds_map)
        .expect_err("reject duplicate");
    assert!(
        matches!(
            err,
            CasperError::SlashAuth(SlashAuthError::DuplicateTarget { .. })
        ),
        "expected SlashAuthError::DuplicateTarget, got {err:?}"
    );
    assert!(err.to_string().contains("duplicate slash deploy target"));
}

#[tokio::test]
async fn duplicate_justification_validators_are_invalid() {
    let fixture = DetectorFixture::new().await;
    let mut js = fixture
        .validators
        .iter()
        .cloned()
        .map(|validator| justification(validator, fixture.genesis.block_hash.clone()))
        .collect::<Vec<_>>();
    js.push(justification(
        fixture.validators[0].clone(),
        fixture.genesis.block_hash.clone(),
    ));
    let mut candidate = block(
        35,
        fixture.validators[0].clone(),
        1,
        js,
        fixture.validators.clone(),
    );
    candidate.header.parents_hash_list = vec![fixture.genesis.block_hash.clone()];

    let result = Validate::justifications_well_formed(&candidate);

    assert_eq!(
        result,
        Either::Left(BlockError::Invalid(InvalidBlock::InvalidFollows))
    );
}

#[test]
fn checked_sequence_arithmetic_rejects_boundaries() {
    assert_eq!(checked_base_seq(i32::MIN), None);
    assert_eq!(checked_base_seq(-1), None);
    assert_eq!(checked_base_seq(0), None);
    assert_eq!(checked_base_seq(1), Some(0));
    assert_eq!(checked_next_seq(i32::MAX as u64), None);
    assert_eq!(checked_next_seq(41), Some(42));
}

/// CI run 32588262605 shard16: joiner7 judged five foreign bonding-era
/// blocks JustificationRegression — verdicts no other node issued — then
/// peers judged the resulting DOA slash carriers UnauthorizedSlashDeploy,
/// the catch-all minted evidence for every one, and the recursive slashes
/// burned honest stake to FT −18.55. A verdict two honest nodes can
/// disagree on — anything relative to the receiver's own records or
/// admission order — must never mint slash evidence. The dispatcher's
/// invalid-record and evidence minting both key off `is_slashable`, so
/// this classification is the single point the whole slashing pipeline
/// (candidates, neglect obligation, receive-side authorization) follows.
#[test]
fn view_relative_verdicts_are_not_slash_worthy() {
    assert!(!InvalidBlock::JustificationRegression.is_slashable());
    assert!(!InvalidBlock::UnauthorizedSlashDeploy.is_slashable());
    assert!(!InvalidBlock::NeglectedInvalidBlock.is_slashable());
    assert!(!InvalidBlock::NeglectedEquivocation.is_slashable());
    assert!(!InvalidBlock::InvalidTransaction.is_slashable());
}

#[test]
fn equivocation_class_remains_slash_worthy() {
    assert!(InvalidBlock::AdmissibleEquivocation.is_slashable());
    assert!(InvalidBlock::IgnorableEquivocation.is_slashable());
}

proptest! {
    #[test]
    fn checked_next_seq_matches_i32_successor(n in 0_u64..=((i32::MAX as u64) + 1)) {
        let expected = n
            .checked_add(1)
            .and_then(|next| i32::try_from(next).ok());
        prop_assert_eq!(checked_next_seq(n), expected);
    }

    #[test]
    fn checked_base_seq_rejects_nonpositive(n in i32::MIN..=0) {
        prop_assert_eq!(checked_base_seq(n), None);
    }

    #[test]
    fn checked_base_seq_matches_positive_i32_predecessor(n in 1_i32..=i32::MAX) {
        prop_assert_eq!(checked_base_seq(n), Some(n - 1));
    }

    #[test]
    fn epoch_for_block_number_matches_floor_division(
        block_number in 0_i64..1_000_000_i64,
        epoch_length in 1_i32..10_000_i32,
    ) {
        // Phase 9 (C-6) + Phase 10 (C-5): `epoch_for_block_number` returns
        // `Result<Epoch, DomainError>` — the happy path is `Ok(Epoch::new(...))`.
        prop_assert_eq!(
            epoch_for_block_number(block_number, epoch_length),
            Ok(casper::rust::epoch::Epoch::new(block_number / i64::from(epoch_length)))
        );
    }

    #[test]
    fn epoch_for_block_number_rejects_invalid_domains(
        negative_block_number in i64::MIN..0_i64,
        epoch_length in 1_i32..10_000_i32,
    ) {
        // Phase 9 (C-6): negative block numbers and non-positive
        // epoch lengths are now distinguishable typed errors.
        prop_assert_eq!(
            epoch_for_block_number(negative_block_number, epoch_length),
            Err(casper::rust::slashing_authorization::DomainError::NegativeBlockNumber(
                negative_block_number
            ))
        );
        prop_assert_eq!(
            epoch_for_block_number(0, 0),
            Err(casper::rust::slashing_authorization::DomainError::InvalidEpochLength(0))
        );
        prop_assert_eq!(
            epoch_for_block_number(0, -1),
            Err(casper::rust::slashing_authorization::DomainError::InvalidEpochLength(-1))
        );
    }
}
