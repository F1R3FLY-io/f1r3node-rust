// Finality-gated recovery, end to end: a rejected deploy returns ONLY by
// fresh execution in a later block, gated on its rejection being settled in
// the block's frozen floor closure, with custody scoped to the owner.
//
// Ungated re-proposal regenerated same-sig sibling copies faster than merges
// could adjudicate them, pinning recovery below the first carrier and
// livelocking the shard under sustained contention. The gate
// (`FloorContext::retry_gate_open`) is a pure function of the block, so the
// proposer's deferral and every validator's `PrematureDeployRetry` verdict
// are the same computation on the same frozen inputs.
//
// Staging (three validators, default stakes {1, 3, 5}):
//   seed cell on nodes[1] -> contenders on nodes[0]/nodes[1] -> adjudicating
//   merge M on nodes[1] rejects one contender WITH a record. The loser's
//   carrier sender is the retry owner; the record is live (above every
//   floor) immediately after M.

use std::collections::HashMap;

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::finality::floor::floor_of_block;
use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::util::rholang::interpreter_util;
use casper::rust::util::{construct_deploy, proto_util};
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, F1r3flyState, Header, Justification,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn rejected_sigs(block: &BlockMessage) -> Vec<Bytes> {
    block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| rd.sig.clone())
        .collect()
}

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

/// Contest two deploys on a seeded single-value cell; the adjudicating
/// merge M (nodes[1]) rejects exactly one with a record. Returns
/// (nodes, shard_id, loser_sig, loser_owner_index, [carrier A, carrier B,
/// M] in causal order for delivery).
async fn stage_live_rejection() -> (Vec<TestNode>, String, Bytes, usize, Vec<BlockMessage>) {
    let (mut nodes, shard_id) = three_node_network().await;

    let seed = construct_deploy::source_deploy_now_full(
        r#"@"gate_cell"!("s")"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build seed");
    let s_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&seed))
        .await
        .expect("seed block");
    nodes[0]
        .process_block(s_block.clone())
        .await
        .expect("nodes[0] processes seed");
    nodes[2]
        .process_block(s_block.clone())
        .await
        .expect("nodes[2] processes seed");

    let contender_a = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"gate_cell") { @"gate_cell"!("a") }"#.to_string(),
            None,
            None,
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(0),
            Some(shard_id.clone()),
        )
        .expect("build contender a")
    };
    let contender_b = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            r#"for (@v <- @"gate_cell") { @"gate_cell"!("b") }"#.to_string(),
            None,
            None,
            Some(
                crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0]
                    .0
                    .clone(),
            ),
            None,
            Some(shard_id.clone()),
        )
        .expect("build contender b")
    };
    let a_sig: Bytes = contender_a.sig.clone();
    let b_sig: Bytes = contender_b.sig.clone();
    let a_block = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&contender_a))
        .await
        .expect("carrier A on nodes[0]");
    let b_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&contender_b))
        .await
        .expect("carrier B on nodes[1]");
    nodes[1]
        .process_block(a_block.clone())
        .await
        .expect("nodes[1] processes A");
    let m_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(
            &construct_deploy::basic_deploy_data(
                10,
                Some(construct_deploy::DEFAULT_SEC2.clone()),
                Some(shard_id.clone()),
            )
            .expect("m marker"),
        ))
        .await
        .expect("adjudicating merge M");
    let m_rejected = rejected_sigs(&m_block);
    let a_lost = m_rejected.contains(&a_sig);
    let b_lost = m_rejected.contains(&b_sig);
    assert!(
        a_lost ^ b_lost,
        "exactly one contender rejected at M (rejected: {:?})",
        m_rejected.iter().map(short).collect::<Vec<_>>(),
    );
    let (loser_sig, loser_owner) = if a_lost {
        (a_sig, 0usize)
    } else {
        (b_sig, 1usize)
    };

    (nodes, shard_id, loser_sig, loser_owner, vec![
        a_block, b_block, m_block,
    ])
}

/// Deliver `blocks` (causal order) to every node that lacks them, so M is
/// VALIDATED everywhere — a block parked on pending dependencies would
/// leave latest messages and the owner-scoped populate stale.
async fn deliver_everywhere(nodes: &mut [TestNode], blocks: &[BlockMessage]) {
    for node in nodes.iter_mut() {
        for b in blocks {
            if !node.contains(&b.block_hash) {
                node.process_block(b.clone())
                    .await
                    .expect("causal delivery");
            }
        }
    }
}

/// THE GATE, validity side: re-including a rejected sig while its kept
/// rejection is still live (above every floor) is `PrematureDeployRetry` on
/// every validator — and the proposer never mints such a block, so the
/// invalid shape is staged through the production checkpoint directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn premature_retry_is_rejected_by_every_validator() {
    let (mut nodes, shard_id, loser_sig, loser_owner, staged_blocks) = stage_live_rejection().await;

    // Everyone validates the contest and M (populate arms the owner's
    // buffer), then the owner attempts the retry IMMEDIATELY — the
    // rejection record sits in M, far above any floor. The proposer's own
    // create must DEFER: no block carrying the loser may be minted.
    deliver_everywhere(&mut nodes, &staged_blocks).await;
    let owner_attempt = nodes[loser_owner]
        .create_block(&[])
        .await
        .expect("owner attempts a proposal");
    if let casper::rust::blocks::proposer::propose_result::BlockCreatorResult::Created(
        block,
        _,
        _,
    ) = &owner_attempt
    {
        assert!(
            !block
                .body
                .deploys
                .iter()
                .any(|pd| pd.deploy.sig == loser_sig),
            "the proposer must defer a retry whose rejection is not settled \
             in its floor (gate closed); it minted the loser {} instead",
            short(&loser_sig),
        );
    }

    // Validity side: a block that DOES carry the premature retry is
    // rejected by every validator with the non-slashable verdict. No honest
    // proposer mints it (the deferral above), so it is assembled through
    // the production checkpoint directly — the same technique the
    // carrier-record spec uses for proposer-unreachable shapes.
    let retry_holder = nodes[loser_owner]
        .rejected_deploy_buffer
        .lock()
        .expect("buffer lock")
        .read_all()
        .expect("buffer read");
    let retry_deploy = retry_holder
        .into_iter()
        .find(|d| d.sig == loser_sig)
        .expect("owner's buffer must hold the loser (owner-scoped populate)");
    let snapshot = nodes[loser_owner]
        .casper
        .get_snapshot()
        .await
        .expect("owner snapshot");
    let validator_identity = nodes[loser_owner]
        .validator_id_opt
        .clone()
        .expect("owner validator identity");
    let runtime_manager = nodes[loser_owner].runtime_manager.clone();
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
        &mut nodes[loser_owner].block_store,
        snapshot.parents.clone(),
        vec![retry_deploy],
        Vec::new(),
        &snapshot,
        &runtime_manager,
        block_data,
        HashMap::new(),
        None,
        None,
        None,
    )
    .await
    .expect("checkpoint carrying the premature retry");
    let body = Body {
        state: F1r3flyState {
            pre_state_hash: checkpoint.pre_state_hash,
            post_state_hash: checkpoint.post_state_hash,
            bonds: checkpoint.bonds,
            block_number: next_block_num,
        },
        deploys: checkpoint.deploys,
        rejected_deploys: checkpoint.rejected_deploys,
        system_deploys: checkpoint.system_deploys,
        extra_bytes: Bytes::new(),
        applied_from_scope: checkpoint.applied_from_scope,
        merge_base: checkpoint.merge_base.unwrap_or_default(),
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
    let premature_block = validator_identity.sign_block(&unsigned);

    let verdict = nodes[2]
        .process_block(premature_block)
        .await
        .expect("nodes[2] processes the premature retry");
    assert_eq!(
        verdict,
        Either::Left(BlockError::Invalid(InvalidBlock::PrematureDeployRetry)),
        "re-inclusion against a live contest must be PrematureDeployRetry \
         (non-slashable) on every validator",
    );
}

/// THE GATE, green side: once the floor covers the rejection record, the
/// owner's own create re-proposes the loser and the block validates
/// everywhere — recovery is one finalization latency behind adjudication,
/// owner-scoped.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn settled_rejection_opens_the_gate_and_the_owner_retries() {
    let (mut nodes, shard_id, loser_sig, loser_owner, staged_blocks) = stage_live_rejection().await;
    let m_block = staged_blocks.last().expect("staged M").clone();
    let m_height = m_block.body.state.block_number;

    // Owner-scoped custody: the owner's buffer holds the loser after
    // validating M; the non-owner live validator's does not.
    deliver_everywhere(&mut nodes, &staged_blocks).await;
    let non_owner = if loser_owner == 0 { 1 } else { 0 };
    assert!(
        nodes[loser_owner]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .contains_sig(&loser_sig)
            .expect("contains_sig"),
        "the owner (sender of the rejected copy's carrier) buffers the retry",
    );
    assert!(
        !nodes[non_owner]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .contains_sig(&loser_sig)
            .expect("contains_sig"),
        "a non-owner validator must not buffer a foreign deploy's retry",
    );

    // Settle: nodes[2] (5/9 stake, self-witnessing majority) advances the
    // floor over M, delivering every round to all.
    let mut gate_open_tip: Option<BlockMessage> = None;
    for round in 0..30i32 {
        let b = nodes[2]
            .add_block_from_deploys(std::slice::from_ref(
                &construct_deploy::basic_deploy_data(
                    100 + round,
                    Some(construct_deploy::DEFAULT_SEC2.clone()),
                    Some(shard_id.clone()),
                )
                .expect("settle marker"),
            ))
            .await
            .expect("settle round");
        for (other, node) in nodes.iter_mut().enumerate() {
            if other != 2 {
                node.process_block(b.clone())
                    .await
                    .expect("settle delivery");
            }
        }
        let dag = nodes[loser_owner].casper.block_dag().await.expect("dag");
        let floor = floor_of_block(&dag, &b.block_hash, FtThreshold::from_f32_lossy(0.0))
            .await
            .expect("floor_of_block");
        let covered = floor.hash == m_block.block_hash
            || (floor.block_number >= m_height
                && dag
                    .is_dag_ancestor(&m_block.block_hash, &floor.hash)
                    .expect("ancestor query"));
        if covered {
            gate_open_tip = Some(b);
            break;
        }
    }
    assert!(
        gate_open_tip.is_some(),
        "staging precondition: the floor must come to cover the record \
         within the settle rounds",
    );

    // The owner's own create now re-proposes the loser, and the block
    // validates on the other nodes.
    let retried = nodes[loser_owner]
        .create_block_unsafe(&[])
        .await
        .expect("owner retries through its own create");
    assert!(
        retried
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == loser_sig),
        "with the rejection settled in the floor, the owner's create must \
         re-propose the loser (body sigs: {:?})",
        retried
            .body
            .deploys
            .iter()
            .map(|pd| short(&pd.deploy.sig))
            .collect::<Vec<_>>(),
    );
    for (i, node) in nodes.iter_mut().enumerate() {
        if i != loser_owner {
            let verdict = node
                .process_block(retried.clone())
                .await
                .expect("validator processes the gated retry");
            assert!(
                matches!(
                    verdict,
                    Either::Right(casper::rust::block_status::ValidBlock::Valid)
                ),
                "a gate-open retry must validate everywhere; nodes[{}] said {:?}",
                i,
                verdict,
            );
        }
    }
}
