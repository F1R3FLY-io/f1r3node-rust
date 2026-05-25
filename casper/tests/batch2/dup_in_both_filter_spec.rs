// Regression test for Bug C: `compute_deploys_checkpoint` must not
// execute a deploy whose signature was rejected by the parent merge
// in the same round.
//
// Root cause of residual `test_bonding_validators` multi-Datum:
// `compute_deploys_checkpoint` runs `compute_parents_post_state`
// (which produces `pre_state_hash` + `rejected_deploys`) and then
// `compute_state_with_bonds` (which executes the prepared deploys),
// with NO filter between them. So a sig that appears in the prepared
// `deploys` AND in the merge's `rejected_deploys` ends up in BOTH
// `body.deploys` AND `body.rejected_deploys` of the resulting block —
// the `dup_in_both` pattern. Replay then re-executes the sig on top of
// pre-state that already contains its effects via the accepting
// ancestor → multi-Datum on tagged channels.
//
// This test verifies the filter at the function-contract level: given
// inputs that trigger the dup, the function must drop the rejected sig
// from `processed_deploys`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::interpreter_util::compute_deploys_checkpoint;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use crypto::rust::public_key::PublicKey;
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();

        Self { genesis }
    }
}

/// Same conflict pattern as `recovery_cycle_spec`: two same-key deploys
/// whose combined precharge would drive the source vault below zero.
/// `conflict_set_merger::fold_rejection` rejects whichever branch it
/// processes second.
const CONFLICT_RHO: &str = r#"
Nil
"#;
const PHLO_LIMIT: i64 = 8;
const PHLO_PRICE: i64 = 1_000_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn compute_deploys_checkpoint_filters_sigs_rejected_by_same_merge() {
    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 2, None, None, None, None)
        .await
        .expect("create_network(2)");

    let deploy_x = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            CONFLICT_RHO.to_string(),
            Some(PHLO_LIMIT),
            Some(PHLO_PRICE),
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy_x")
    };
    let deploy_y = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            CONFLICT_RHO.to_string(),
            Some(PHLO_LIMIT),
            Some(PHLO_PRICE),
            Some(construct_deploy::DEFAULT_SEC.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("build deploy_y")
    };

    // Lex-larger sig goes to validator 0's block — same routing as
    // recovery_cycle_spec — so we know which sig the merge rejects.
    let (deploy_a, deploy_b) = if deploy_x.sig >= deploy_y.sig {
        (deploy_x, deploy_y)
    } else {
        (deploy_y, deploy_x)
    };
    let sig_a: Bytes = deploy_a.sig.clone();
    assert!(sig_a > deploy_b.sig);

    let block_a = nodes[0]
        .add_block_from_deploys(&[deploy_a.clone()])
        .await
        .expect("validator 0 proposes block_a");
    let block_b = nodes[1]
        .add_block_from_deploys(&[deploy_b.clone()])
        .await
        .expect("validator 1 proposes block_b");
    assert_ne!(block_a.block_hash, block_b.block_hash);

    // Sync so val 0 sees block_b — required for the multi-parent merge.
    {
        let (a, b) = nodes.split_at_mut(1);
        a[0].sync_with_one(&mut b[0]).await.expect("sync 0 -> 1");
    }
    {
        let (a, b) = nodes.split_at_mut(1);
        b[0].sync_with_one(&mut a[0]).await.expect("sync 1 -> 0");
    }
    assert!(nodes[0].contains(&block_b.block_hash));

    // Directly invoke compute_deploys_checkpoint on val 0 with:
    //   * parents = [block_a, block_b]  — triggers conflict in the merge
    //   * deploys = [deploy_a]          — sig that the merge will reject
    //
    // Bypasses the proposer's parent-selection + prepare_user_deploys so
    // the trigger condition is unambiguous: the function is given a sig
    // its parent-merge is about to reject in the same round.
    let snapshot = nodes[0]
        .casper
        .get_snapshot()
        .await
        .expect("val 0 snapshot");

    let parent_block_numbers: Vec<i64> = [&block_a, &block_b]
        .iter()
        .map(|b| b.body.state.block_number)
        .collect();
    let new_block_number = parent_block_numbers.iter().max().copied().unwrap_or(0) + 1;
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let block_data = BlockData {
        time_stamp: now_millis,
        block_number: new_block_number,
        sender: PublicKey::from_bytes(&block_a.sender),
        seq_num: block_a.seq_num + 1,
    };

    // Split borrows: runtime_manager and rejected_deploy_buffer are
    // accessed via shared references; block_store needs &mut. Hold them
    // separately so the borrow checker is happy with the call.
    let node = &mut nodes[0];
    let runtime_manager = node.runtime_manager.clone();
    let rejected_buffer = node.rejected_deploy_buffer.clone();
    let result = compute_deploys_checkpoint(
        &mut node.block_store,
        vec![block_a.clone(), block_b.clone()],
        vec![deploy_a.clone()],
        Vec::<SystemDeployEnum>::new(),
        &snapshot,
        &runtime_manager,
        block_data,
        HashMap::new(),
        Some(&rejected_buffer),
    )
    .await
    .expect("compute_deploys_checkpoint should succeed");

    let (_pre_state_hash, _post_state_hash, processed_deploys, rejected_deploys, _, _) = result;

    let processed_sigs: Vec<Bytes> = processed_deploys
        .iter()
        .map(|pd| pd.deploy.sig.clone())
        .collect();

    // Precondition: the merge must reject sig_a. If this fails the test
    // setup itself is broken — the conflict didn't trigger as expected.
    assert!(
        rejected_deploys.contains(&sig_a),
        "test precondition: merge of [block_a, block_b] must reject deploy_a's sig {} \
         (the lex-larger of the same-key vault-conflict pair). Got rejected sigs: {:?}",
        hex::encode(&sig_a),
        rejected_deploys
            .iter()
            .map(|s| hex::encode(s.as_ref()))
            .collect::<Vec<_>>()
    );

    // Bug C assertion: the same sig must NOT appear in processed_deploys.
    // If it does, body.deploys ∩ body.rejected_deploys ≠ ∅ — replay
    // would re-execute the sig on top of canonical pre-state that
    // already contains its effects → multi-Datum on tagged channels.
    assert!(
        !processed_sigs.contains(&sig_a),
        "Bug C: compute_deploys_checkpoint executed deploy_a even though the \
         parent merge rejected its sig in the same round. The block would ship \
         with body.deploys ∩ body.rejected_deploys ≠ ∅, and the next proposer's \
         pre-state derivation would multi-Datum on the conflicting tagged channel.\n\
         processed sigs: {:?}\nrejected sigs:  {:?}\nsig_a:          {}",
        processed_sigs
            .iter()
            .map(|s| hex::encode(s.as_ref()))
            .collect::<Vec<_>>(),
        rejected_deploys
            .iter()
            .map(|s| hex::encode(s.as_ref()))
            .collect::<Vec<_>>(),
        hex::encode(&sig_a),
    );
}
