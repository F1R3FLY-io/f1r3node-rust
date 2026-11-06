// Slash authorization: which slashes a proposer issues and which a receiver
// accepts. Both apply one rule to a slash's own evidence — two blocks one
// sender signed at one sequence number — and the offender's stake, so the
// verdict never depends on the node's own view of the equivocation.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::errors::CasperError;
use casper::rust::slashing_authorization::{
    authorized_slash_candidates, checked_base_seq, checked_next_seq,
    validate_received_slash_deploys, AuthorizedSlashCandidate, SlashAuthError,
};
use casper::rust::util::proto_util;
use casper::rust::validate::Validate;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use dashmap::DashSet;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, EquivocationEvidence, ProcessedSystemDeploy, SystemDeployData,
};
use proptest::prelude::*;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use super::detector_totality_helpers::{block, justification, DetectorFixture};

const CHAIN: [u8; 32] = [0xC1; 32];

fn snapshot(fixture: &DetectorFixture, bonded: &[Bytes]) -> CasperSnapshot {
    CasperSnapshot {
        chain_id: Bytes::from_static(&CHAIN),
        dag: fixture
            .dag_storage
            .get_representation()
            .expect("dag representation"),
        last_finalized_block: Bytes::new(),
        parents: vec![],
        justifications: HashSet::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        rejected_in_scope: Arc::new(DashSet::new()),
        max_block_num: 10_000,
        max_seq_nums: HashMap::new(),
        on_chain_state: OnChainCasperState {
            shard_conf: CasperShardConf::new(),
            bonds_map: bonded
                .iter()
                .map(|validator| (validator.clone(), 100))
                .collect(),
            active_validators: bonded.to_vec(),
        },
    }
}

fn key(byte: u8) -> PrivateKey { PrivateKey::from_bytes(&[byte; 32]) }

fn public(key: &PrivateKey) -> Bytes { Secp256k1.to_public(key).bytes.into() }

/// A block at `seq_num` signed by `key`, told apart from its siblings by `nonce`.
fn signed(key: &PrivateKey, seq_num: i32, nonce: u8) -> BlockMessage {
    let mut b = block(nonce, public(key), seq_num, vec![], vec![]);
    b.header.chain_id = Bytes::from_static(&CHAIN);
    b.sig_algorithm = Secp256k1::name();
    b.block_hash = proto_util::hash_block(&b);
    b.sig = Secp256k1.sign(&b.block_hash, &key.bytes).into();
    b
}

fn evidence(a: &BlockMessage, b: &BlockMessage) -> EquivocationEvidence {
    let (first, second) = if a.block_hash < b.block_hash {
        (a, b)
    } else {
        (b, a)
    };
    EquivocationEvidence {
        first: proto_util::block_commitment(first),
        second: proto_util::block_commitment(second),
    }
}

fn slash(evidence: EquivocationEvidence, issuer: &Bytes) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::create_slash(evidence, PublicKey::from_bytes(issuer)),
    }
}

/// A block `proposer` issues at height 10,000 carrying `slashes`.
fn carrier(proposer: &Bytes, slashes: Vec<ProcessedSystemDeploy>) -> BlockMessage {
    let mut b = block(90, proposer.clone(), 1, vec![], vec![]);
    b.body.state.block_number = 10_000;
    b.body.system_deploys = slashes;
    b
}

/// Holds `second` as an equivocation of `first`, as validation stores it.
fn hold(fixture: &DetectorFixture, first: &BlockMessage, second: &BlockMessage) {
    for b in [first, second] {
        fixture
            .block_store
            .put_block_message(b)
            .expect("store block");
    }
    fixture
        .dag_storage
        .insert(first, InsertMode::Normal)
        .expect("insert first");
    fixture
        .dag_storage
        .insert(second, InsertMode::Equivocation {
            sibling: first.block_hash.clone(),
        })
        .expect("insert equivocation");
}

fn auth_error(result: Result<(), CasperError>) -> SlashAuthError {
    match result {
        Err(CasperError::SlashAuth(error)) => error,
        other => panic!("expected a slash authorization error, got {other:?}"),
    }
}

#[tokio::test]
async fn every_held_equivocation_of_a_bonded_offender_is_a_candidate_whatever_its_age() {
    let fixture = DetectorFixture::new().await;
    let offender = key(0x11);
    let (a, b) = (signed(&offender, 3, 1), signed(&offender, 3, 2));
    hold(&fixture, &a, &b);

    let candidates = authorized_slash_candidates(
        &snapshot(&fixture, &[public(&offender)]),
        &fixture.block_store,
    )
    .expect("candidates");

    assert_eq!(candidates, vec![AuthorizedSlashCandidate {
        offender: public(&offender),
        evidence: evidence(&a, &b),
    }]);
}

#[tokio::test]
async fn an_offender_with_several_equivocations_is_one_candidate() {
    let fixture = DetectorFixture::new().await;
    let offender = key(0x11);
    let (a3, b3) = (signed(&offender, 3, 1), signed(&offender, 3, 2));
    let (a5, b5) = (signed(&offender, 5, 3), signed(&offender, 5, 4));
    hold(&fixture, &a3, &b3);
    hold(&fixture, &a5, &b5);

    let candidates = authorized_slash_candidates(
        &snapshot(&fixture, &[public(&offender)]),
        &fixture.block_store,
    )
    .expect("candidates");

    let lowest = if b3.block_hash < b5.block_hash {
        evidence(&a3, &b3)
    } else {
        evidence(&a5, &b5)
    };
    assert_eq!(candidates, vec![AuthorizedSlashCandidate {
        offender: public(&offender),
        evidence: lowest,
    }]);
}

#[tokio::test]
async fn an_unbonded_offender_is_no_candidate() {
    let fixture = DetectorFixture::new().await;
    let offender = key(0x11);
    hold(&fixture, &signed(&offender, 3, 1), &signed(&offender, 3, 2));

    let candidates = authorized_slash_candidates(&snapshot(&fixture, &[]), &fixture.block_store)
        .expect("candidates");

    assert!(candidates.is_empty());
}

/// Judges `block`'s slashes as a node of shard `root` and chain `CHAIN`
/// whose parents bond `stakes`.
fn received(block: &BlockMessage, stakes: &[(Bytes, i64)]) -> Result<(), CasperError> {
    let bonds: HashMap<Bytes, i64> = stakes.iter().cloned().collect();
    validate_received_slash_deploys(block, &bonds, "root", &CHAIN)
}

#[test]
fn a_received_slash_proving_an_equivocation_of_a_bonded_offender_is_accepted() {
    let (offender, proposer) = (key(0x11), public(&key(0x22)));
    let pair = evidence(&signed(&offender, 3, 1), &signed(&offender, 3, 2));
    let block = carrier(&proposer, vec![slash(pair, &proposer)]);

    received(&block, &[(public(&offender), 100), (proposer, 100)])
        .expect("a slash any node can check is accepted, at any height");
}

#[test]
fn a_received_slash_issued_by_another_validator_is_rejected() {
    let (offender, proposer) = (key(0x11), public(&key(0x22)));
    let pair = evidence(&signed(&offender, 3, 1), &signed(&offender, 3, 2));
    let block = carrier(&proposer, vec![slash(pair, &public(&key(0x33)))]);

    assert!(matches!(
        auth_error(received(&block, &[
            (public(&offender), 100),
            (proposer, 100)
        ])),
        SlashAuthError::IssuerMismatch { .. }
    ));
}

#[test]
fn a_received_slash_whose_evidence_is_not_an_equivocation_is_rejected() {
    let (offender, proposer) = (key(0x11), public(&key(0x22)));
    let pair = evidence(&signed(&offender, 3, 1), &signed(&offender, 4, 2));
    let block = carrier(&proposer, vec![slash(pair, &proposer)]);

    assert_eq!(
        auth_error(received(&block, &[
            (public(&offender), 100),
            (proposer, 100)
        ])),
        SlashAuthError::NotAnEquivocation {
            reason: "the blocks have different sequence numbers"
        }
    );
}

/// A block `key` signed at `seq_num` for shard `shard` of chain `chain`.
fn signed_elsewhere(key: &PrivateKey, seq_num: i32, shard: &str, chain: &[u8]) -> BlockMessage {
    let mut b = block(2, public(key), seq_num, vec![], vec![]);
    b.shard_id = shard.to_string();
    b.header.chain_id = Bytes::copy_from_slice(chain);
    b.sig_algorithm = Secp256k1::name();
    b.block_hash = proto_util::hash_block(&b);
    b.sig = Secp256k1.sign(&b.block_hash, &key.bytes).into();
    b
}

#[test]
fn a_received_slash_citing_a_block_of_another_shard_or_chain_is_rejected() {
    let (offender, proposer) = (key(0x11), public(&key(0x22)));
    let here = signed(&offender, 3, 1);
    for elsewhere in [
        signed_elsewhere(&offender, 3, "root-other", &CHAIN),
        signed_elsewhere(&offender, 3, "root", &[0xC2; 32]),
    ] {
        let block = carrier(&proposer, vec![slash(
            evidence(&here, &elsewhere),
            &proposer,
        )]);
        assert_eq!(
            auth_error(received(&block, &[
                (public(&offender), 100),
                (proposer.clone(), 100)
            ])),
            SlashAuthError::NotAnEquivocation {
                reason: "a block belongs to another shard or chain"
            },
            "shard {}, chain {}",
            elsewhere.shard_id,
            hex::encode(&elsewhere.header.chain_id)
        );
    }
}

#[test]
fn a_received_slash_of_an_unbonded_or_already_slashed_offender_is_rejected() {
    let (offender, proposer) = (key(0x11), public(&key(0x22)));
    let pair = evidence(&signed(&offender, 3, 1), &signed(&offender, 3, 2));
    let block = carrier(&proposer, vec![slash(pair, &proposer)]);

    for stakes in [vec![(proposer.clone(), 100)], vec![
        (public(&offender), 0),
        (proposer.clone(), 100),
    ]] {
        assert!(matches!(
            auth_error(received(&block, &stakes)),
            SlashAuthError::TargetNotBonded { .. }
        ));
    }
}

#[test]
fn a_block_slashing_one_offender_twice_is_rejected() {
    let (offender, proposer) = (key(0x11), public(&key(0x22)));
    let at_3 = evidence(&signed(&offender, 3, 1), &signed(&offender, 3, 2));
    let at_5 = evidence(&signed(&offender, 5, 3), &signed(&offender, 5, 4));
    let block = carrier(&proposer, vec![
        slash(at_3, &proposer),
        slash(at_5, &proposer),
    ]);

    assert!(matches!(
        auth_error(received(&block, &[
            (public(&offender), 100),
            (proposer, 100)
        ])),
        SlashAuthError::DuplicateTarget { .. }
    ));
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

/// A verdict two honest nodes can disagree on — anything relative to the
/// receiver's own view or admission order — must never become slash
/// evidence. Only the equivocation class is stored as an invalid block, so
/// this classification is the single point the slashing pipeline follows.
#[test]
fn view_relative_verdicts_are_not_slash_worthy() {
    assert!(!InvalidBlock::JustificationRegression.is_slashable());
    assert!(!InvalidBlock::UnauthorizedSlashDeploy.is_slashable());
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
}
