use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::slashing_authorization::{
    authorized_slash_candidates, checked_base_seq, checked_next_seq, epoch_for_block_number,
    validate_received_slash_deploys,
};
use casper::rust::validate::Validate;
use crypto::rust::public_key::PublicKey;
use dashmap::{DashMap, DashSet};
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
        .insert(block, invalid, false)
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

    CasperSnapshot {
        dag: fixture.dag_storage.get_representation(),
        last_finalized_block: prost::bytes::Bytes::new(),
        lca: prost::bytes::Bytes::new(),
        tips: vec![],
        parents: vec![],
        justifications: DashSet::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        max_block_num,
        max_seq_nums: DashMap::new(),
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf {
                epoch_length,
                ..CasperShardConf::new()
            },
            bonds_map,
            active_validators: bonded,
        },
    }
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
            issuer_public_key: PublicKey::from_bytes(&issuer),
            target_activation_epoch,
        },
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
    let candidates = authorized_slash_candidates(&snapshot).expect("candidates");

    assert!(
        candidates.is_empty(),
        "epoch-scoped authorization must not propose slash deploys from stale evidence"
    );
}

#[tokio::test]
async fn current_epoch_invalid_evidence_is_authorized_once_per_offender() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let invalid_a = block(31, offender.clone(), 11, vec![], fixture.validators.clone());
    let invalid_b = block(32, offender.clone(), 12, vec![], fixture.validators.clone());
    for invalid in [&invalid_a, &invalid_b] {
        put_block(&fixture, invalid, true);
    }

    let snapshot = snapshot_from_fixture(&fixture, 11, 10, vec![offender.clone()]);
    let candidates = authorized_slash_candidates(&snapshot).expect("candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].offender, offender);
    assert_eq!(candidates[0].target_activation_epoch, 1);
    assert_eq!(
        candidates[0].invalid_block_hash,
        invalid_a
            .block_hash
            .clone()
            .min(invalid_b.block_hash.clone())
    );
}

#[tokio::test]
async fn received_stale_slash_deploy_is_rejected_before_replay() {
    let fixture = DetectorFixture::new().await;
    let offender = fixture.validators[0].clone();
    let proposer = fixture.validators[1].clone();
    let invalid = block(33, offender.clone(), 5, vec![], fixture.validators.clone());
    put_block(&fixture, &invalid, true);

    let snapshot = snapshot_from_fixture(&fixture, 10, 10, vec![offender, proposer.clone()]);
    let mut slash_block = block(34, proposer.clone(), 11, vec![], fixture.validators.clone());
    slash_block.body.state.block_number = 11;
    slash_block.body.system_deploys = vec![slash_deploy(invalid.block_hash.clone(), proposer, 0)];

    let err = validate_received_slash_deploys(&slash_block, &snapshot).expect_err("reject stale");
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

    validate_received_slash_deploys(&slash_block, &snapshot).expect("current slash deploy");
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

    let err = validate_received_slash_deploys(&slash_block, &snapshot).expect_err("reject issuer");
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

    let err = validate_received_slash_deploys(&slash_block, &snapshot).expect_err("reject unknown");
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

    let err = validate_received_slash_deploys(&slash_block, &snapshot).expect_err("reject valid");
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

    let err =
        validate_received_slash_deploys(&slash_block, &snapshot).expect_err("reject unbonded");
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

    let err =
        validate_received_slash_deploys(&slash_block, &snapshot).expect_err("reject duplicate");
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

    let result = Validate::justification_follows(&candidate, &fixture.block_store);

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

#[test]
fn unauthorized_slash_status_is_slashable() {
    assert!(InvalidBlock::UnauthorizedSlashDeploy.is_slashable());
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
        prop_assert_eq!(
            epoch_for_block_number(block_number, epoch_length),
            Some(block_number / i64::from(epoch_length))
        );
    }

    #[test]
    fn epoch_for_block_number_rejects_invalid_domains(
        negative_block_number in i64::MIN..0_i64,
        epoch_length in any::<i32>(),
    ) {
        prop_assert_eq!(epoch_for_block_number(negative_block_number, epoch_length), None);
        prop_assert_eq!(epoch_for_block_number(0, 0), None);
        prop_assert_eq!(epoch_for_block_number(0, -1), None);
    }
}
