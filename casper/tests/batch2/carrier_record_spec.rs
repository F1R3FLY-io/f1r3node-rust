// A rejection record names the copy it adjudicated, and the record list is
// consensus down to that naming.
//
// Two reds, one per half of the contract:
//
// 1. THE SAME-BLOCK EXEMPTION: the checkpoint's rejected-list equality
//    excuses any computed rejection whose sig the block itself carries in
//    `body.deploys`. A block can therefore execute a deploy fresh, drop the
//    record its own merge computed for that sig's scope copy, and validate
//    clean — the adjudication vanishes from the chain while its subject
//    rides in the same block. The equality must hold with no carve-out: a
//    computed record is part of the block's consensus content whether or
//    not the sig also appears in the body.
//
// 2. THE CARRIER IS CONSENSUS: the record's carrier field states which
//    copy was adjudicated. A reader trusting that statement needs it
//    validator-checked exactly like the sig — a block naming a carrier its
//    own recomputed merge did not reject must be invalid, else the field
//    is advisory metadata a proposer can forge.

use std::collections::{BTreeMap, HashMap};

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::rholang::interpreter_util;
use casper::rust::util::{construct_deploy, proto_util};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, Header, Justification,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn short(sig: &Bytes) -> String { hex::encode(&sig[..8.min(sig.len())]) }

async fn three_node_network() -> (Vec<TestNode>, String) {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, n_validators, None, None, None, None)
        .await
        .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    (nodes, shard_id)
}

/// Stage the contest: a seed cell, two contenders that both consume it
/// (a genuine conflict), carried on divergent branches. Returns the two
/// carrier blocks and the contender deploys keyed by sig.
async fn stage_contest(
    nodes: &mut [TestNode],
    shard_id: &str,
) -> (
    BlockMessage,
    BlockMessage,
    Vec<(
        Bytes,
        crypto::rust::signatures::signed::Signed<
            models::rust::casper::protocol::casper_message::DeployData,
        >,
    )>,
) {
    let seed = construct_deploy::source_deploy_now_full(
        r#"@"race"!("s")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.to_string()),
    )
    .expect("build seed");
    let s_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&seed))
        .await
        .expect("seed block S");
    for i in [0usize, 2usize] {
        nodes[i]
            .process_block(s_block.clone())
            .await
            .expect("process S");
    }

    let contender_d = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("d") | @"XD"!(v) }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.to_string()),
        )
        .expect("build contender d")
    };
    let contender_f = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"race") { @"race"!("f") | @"XF"!(v) }"#.to_string(),
            None,
            None,
            Some(
                crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0]
                    .0
                    .clone(),
            ),
            None,
            Some(shard_id.to_string()),
        )
        .expect("build contender f")
    };
    let contenders = vec![
        (contender_d.sig.clone(), contender_d.clone()),
        (contender_f.sig.clone(), contender_f.clone()),
    ];
    let c_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&contender_d))
        .await
        .expect("block C with d");
    let a_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&contender_f))
        .await
        .expect("block A with f");
    (c_block, a_block, contenders)
}

/// RED 1: a block that executes a deploy fresh while OMITTING the record
/// its own merge computed for that sig must be `InvalidRejectedDeploy`.
///
/// The block is assembled through the production checkpoint on the full
/// frontier — the merge adjudicates the contest and rejects one contender;
/// that loser is then executed FRESH in the same block (its effect is not
/// in the merged pre-state, so execution is clean and replay reproduces
/// it), and the record list is packaged EMPTY. Every state check passes:
/// the pre-state hash matches the recomputed merge, and replay matches the
/// recorded post-state. Only the rejected-list equality can catch the
/// dropped adjudication — and its same-sig carve-out excuses exactly this
/// shape, so the block validates clean end to end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn fresh_carry_must_not_excuse_a_dropped_record() {
    let (mut nodes, shard_id) = three_node_network().await;
    let (c_block, a_block, contenders) = stage_contest(&mut nodes, &shard_id).await;

    // nodes[2] sees both carriers and no adjudication: its merge computes
    // the contest's rejection fresh, with no visible record covering it.
    for b in [&c_block, &a_block] {
        nodes[2]
            .process_block((*b).clone())
            .await
            .expect("nodes[2] processes a carrier");
    }

    let snapshot = nodes[2]
        .casper
        .get_snapshot()
        .await
        .expect("nodes[2] snapshot");
    assert!(
        snapshot
            .parents
            .iter()
            .any(|p| p.block_hash == c_block.block_hash)
            && snapshot
                .parents
                .iter()
                .any(|p| p.block_hash == a_block.block_hash),
        "staging precondition: both carriers must be in nodes[2]'s frontier"
    );

    let latest_messages: BTreeMap<Validator, BlockHash> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let runtime_manager = nodes[2].runtime_manager.clone();
    let merged = interpreter_util::compute_parents_post_state(
        &nodes[2].block_store,
        snapshot.parents.clone(),
        &snapshot,
        &runtime_manager,
        &latest_messages,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("merge over the full frontier");
    assert_eq!(
        merged.rejected_user.len(),
        1,
        "staging precondition: the contest must reject exactly one contender \
         (rejected: {:?})",
        merged
            .rejected_user
            .iter()
            .map(|record| short(&record.sig))
            .collect::<Vec<_>>(),
    );
    let loser_sig = merged.rejected_user[0].sig.clone();
    let loser_deploy = contenders
        .iter()
        .find(|(sig, _)| *sig == loser_sig)
        .map(|(_, d)| d.clone())
        .expect("loser is one of the contenders");

    // Build the block through the production checkpoint: the merge rejects
    // the loser's scope copy, then the same deploy executes fresh on the
    // merged pre-state.
    let validator_identity = nodes[2]
        .validator_id_opt
        .clone()
        .expect("nodes[2] validator identity");
    let next_block_num = snapshot.max_block_num + 1;
    let next_seq_num = snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| *seq as i32 + 1)
        .unwrap_or(1);
    let now_millis = snapshot
        .parents
        .iter()
        .map(|p| p.header.timestamp)
        .max()
        .expect("parents have timestamps")
        + 1;
    let block_data = BlockData {
        time_stamp: now_millis,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };
    let checkpoint = interpreter_util::compute_deploys_checkpoint(
        &mut nodes[2].block_store,
        snapshot.parents.clone(),
        vec![loser_deploy],
        Vec::new(),
        &snapshot,
        &runtime_manager,
        block_data.clone(),
        HashMap::new(),
        None,
        None,
        None,
    )
    .await
    .expect("checkpoint on the full frontier");
    assert!(
        checkpoint
            .rejected_deploys
            .iter()
            .any(|record| record.sig == loser_sig),
        "staging precondition: the checkpoint's merge must reject the loser's \
         scope copy (rejected: {:?})",
        checkpoint
            .rejected_deploys
            .iter()
            .map(|record| short(&record.sig))
            .collect::<Vec<_>>(),
    );
    let fresh_execution = checkpoint
        .deploys
        .iter()
        .find(|pd| pd.deploy.sig == loser_sig)
        .expect("the loser must execute fresh in this block");
    assert!(
        !fresh_execution.is_failed,
        "staging precondition: the fresh execution must succeed"
    );

    // Package with the record list EMPTY: the adjudication is dropped while
    // its subject rides in the body.
    let body = Body {
        state: F1r3flyState {
            pre_state_hash: checkpoint.pre_state_hash,
            post_state_hash: checkpoint.post_state_hash,
            bonds: checkpoint.bonds,
            block_number: next_block_num,
        },
        deploys: checkpoint.deploys,
        rejected_deploys: Vec::new(),
        system_deploys: checkpoint.system_deploys,
        extra_bytes: Bytes::new(),
    };
    let header = Header {
        parents_hash_list: snapshot
            .parents
            .iter()
            .map(|p| p.block_hash.clone())
            .collect(),
        timestamp: now_millis,
        version: 1,
        extra_bytes: Bytes::new(),
    };
    let justifications: Vec<Justification> = snapshot.justifications.iter().cloned().collect();
    let unsigned = proto_util::unsigned_block_proto(
        body,
        header,
        justifications,
        shard_id.clone(),
        Some(next_seq_num),
    );
    let block = validator_identity.sign_block(&unsigned);

    // THE RED: the rejected-list equality must catch the dropped record.
    let mut validation_snapshot = nodes[2]
        .casper
        .get_snapshot()
        .await
        .expect("validation snapshot");
    let result = interpreter_util::validate_block_checkpoint(
        &block,
        &nodes[2].block_store,
        &mut validation_snapshot,
        &runtime_manager,
        None,
        None,
        None,
    )
    .await
    .expect("validation completes");
    assert!(
        matches!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy))
        ),
        "DROPPED ADJUDICATION: a block carrying deploy {} fresh while omitting \
         the record its own merge computed for that sig must be \
         InvalidRejectedDeploy; validation returned {:?}",
        short(&loser_sig),
        match &result {
            Either::Left(err) => format!("Left({:?})", err),
            Either::Right(hash) =>
                format!("Right({:?} — the block validated clean)", hash.is_some()),
        },
    );
}

/// RED 2: the carrier a record names is consensus content. A block whose
/// record carries a carrier the validator's own recomputed merge did not
/// reject must be `InvalidRejectedDeploy` — otherwise the field is
/// forgeable metadata and no reader may trust it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn record_carrier_is_consensus_checked() {
    let (mut nodes, shard_id) = three_node_network().await;
    let (c_block, _a_block, _contenders) = stage_contest(&mut nodes, &shard_id).await;

    // The adjudicating merge M on nodes[1] rejects one contender with a
    // record.
    nodes[1]
        .process_block(c_block.clone())
        .await
        .expect("nodes[1] processes C");
    let marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            7,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("merge marker")
    };
    let m_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&marker))
        .await
        .expect("adjudicating merge M");
    assert!(
        !m_block.body.rejected_deploys.is_empty(),
        "staging precondition: M must adjudicate the contest with a record"
    );

    // Tamper the record's carrier: the sig is untouched, so any sig-level
    // equality still holds.
    let mut tampered = m_block.clone();
    let forged_carrier = Bytes::from(vec![0xAB; 32]);
    assert_ne!(
        tampered.body.rejected_deploys[0].carrier, forged_carrier,
        "the forgery must differ from the recorded carrier"
    );
    tampered.body.rejected_deploys[0].carrier = forged_carrier;

    let runtime_manager = nodes[1].runtime_manager.clone();
    let mut validation_snapshot = nodes[1]
        .casper
        .get_snapshot()
        .await
        .expect("validation snapshot");
    let result = interpreter_util::validate_block_checkpoint(
        &tampered,
        &nodes[1].block_store,
        &mut validation_snapshot,
        &runtime_manager,
        None,
        None,
        None,
    )
    .await
    .expect("validation completes");
    assert!(
        matches!(
            result,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy))
        ),
        "FORGED CARRIER: a record naming a carrier the recomputed merge did \
         not reject must be InvalidRejectedDeploy; validation returned {:?}",
        match &result {
            Either::Left(err) => format!("Left({:?})", err),
            Either::Right(hash) =>
                format!("Right({:?} — the block validated clean)", hash.is_some()),
        },
    );
}
