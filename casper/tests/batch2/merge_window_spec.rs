// Merge-time validity-window enforcement, end to end.
//
// A silent validator's tip stays its latest message indefinitely, and as a
// below-floor sibling it remains mergeable — so a carrier block holding a
// within-window deploy can arrive and merge arbitrarily late. Without a
// merge-time window rule the late merge EXECUTES the deploy past its
// validity window (violating the deploy expiration contract) and can flip a
// settled `Expired` verdict back to `Finalized`. The rule — keyed on the
// merging block's FLOOR, so routinely re-applied above-floor history is
// arithmetically unreachable — rejects such chains WITH a record: the
// deploy's disposition becomes a terminal rejection (the block-expired
// selection filter guarantees recovery never re-proposes it) and its
// effects never enter canonical state.
//
// Topology notes. THREE bonded validators; the default test genesis bonds
// stakes 2i+1 = {1, 3, 5}. The SILENT carrier author must be the
// 1-stake validator (nodes[0]): the progressing pair then holds 8/9 —
// enough for the clique oracle's strict >1/2 witnessing — so the floor
// advances past the deploy's window while the author is dark. (Silencing a
// larger staker pins the floor and the window can never close — witnessed
// weight needs a quorum WITHOUT the silent party.) The short
// deploy_lifespan (5) closes the window within a handful of blocks.

use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use prost::bytes::Bytes;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const DEPLOY_LIFESPAN: i64 = 5;

fn gstring_channel(name: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }],
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn late_carrier_past_window_is_rejected_with_record_and_without_effect() {
    let n_validators = 3usize;
    let genesis_parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(n_validators));
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(genesis_parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network_with_deploy_lifespan(
        genesis,
        n_validators,
        None,
        None,
        None,
        None,
        Some(DEPLOY_LIFESPAN),
    )
    .await
    .expect("create_network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    // Validator 1 (stake 1) authors the carrier: a within-window deploy
    // (valid_after = 0, window edge = 0 + lifespan = 5) executed near
    // genesis. The author then goes silent; the carrier is not delivered to
    // the others until after the floor has passed the window edge.
    let late_deploy = construct_deploy::source_deploy_now_full(
        r#"@"late"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build late deploy");
    let late_sig: Bytes = late_deploy.sig.clone();
    let carrier = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&late_deploy))
        .await
        .expect("validator 1 proposes carrier");
    assert!(
        carrier
            .body
            .deploys
            .iter()
            .any(|pd| pd.deploy.sig == late_sig && !pd.is_failed),
        "carrier must execute the deploy cleanly at height {}",
        carrier.body.state.block_number
    );

    // An early IN-window write on the progressing chain: the no-erasure pin.
    // Its effect must still be present after the late merge — the window
    // rule must never re-litigate validly-landed history.
    let keep_deploy = construct_deploy::source_deploy_now_full(
        r#"@"keep"!(1)"#.to_string(),
        None,
        None,
        Some(construct_deploy::DEFAULT_SEC2.clone()),
        Some(0),
        Some(shard_id.clone()),
    )
    .expect("build keep deploy");
    let keep_sig: Bytes = keep_deploy.sig.clone();
    nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&keep_deploy))
        .await
        .expect("validator 2 proposes keep block");
    {
        let (b, c) = nodes.split_at_mut(2);
        c[0].sync_with_one(&mut b[1])
            .await
            .expect("sync keep block 1 -> 2");
    }

    // Honest progression by validators 2 and 3 (stakes 3 + 5 = 8/9):
    // alternating proposals with both-ways syncs advance the floor past the
    // window edge while validator 1 stays silent. Markers past the short
    // window are silently filtered (block-expired) — heartbeat mode keeps
    // the chain advancing regardless.
    let progression_keys: [&PrivateKey; 2] = [
        &construct_deploy::DEFAULT_SEC2,
        &crate::util::genesis_builder::EXTRA_GENESIS_VAULT_KEY_PAIRS[0].0,
    ];
    for round in 0..8i32 {
        let proposer = 1 + (round % 2) as usize;
        let marker = {
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
            construct_deploy::basic_deploy_data(
                round,
                Some(progression_keys[proposer - 1].clone()),
                Some(shard_id.clone()),
            )
            .expect("build progression marker")
        };
        nodes[proposer]
            .add_block_from_deploys(std::slice::from_ref(&marker))
            .await
            .expect("progression proposal");
        {
            let (b, c) = nodes.split_at_mut(2);
            if proposer == 1 {
                c[0].sync_with_one(&mut b[1])
                    .await
                    .expect("sync progression 1 -> 2");
            } else {
                b[1].sync_with_one(&mut c[0])
                    .await
                    .expect("sync progression 2 -> 1");
            }
        }
    }

    // Late delivery: validator 2 now sees the silent validator's carrier
    // and merges it with the progressed tips.
    {
        let (a, b) = nodes.split_at_mut(1);
        b[0].sync_with_one(&mut a[0])
            .await
            .expect("late-sync carrier 0 -> 1");
    }
    assert!(
        nodes[1].contains(&carrier.block_hash),
        "validator 2 must observe the carrier after the late sync"
    );
    let merge_marker = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::basic_deploy_data(
            100,
            Some(construct_deploy::DEFAULT_SEC2.clone()),
            Some(shard_id.clone()),
        )
        .expect("build merge marker")
    };
    let merge_block = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&merge_marker))
        .await
        .expect("validator 2 proposes the late merge");
    assert!(
        merge_block
            .header
            .parents_hash_list
            .contains(&carrier.block_hash),
        "the late merge must take the carrier as a parent (merge at height \
         {}, parents {:?})",
        merge_block.body.state.block_number,
        merge_block
            .header
            .parents_hash_list
            .iter()
            .map(|h| hex::encode(&h[..4.min(h.len())]))
            .collect::<Vec<_>>()
    );

    // The record: the late deploy is rejected, not merged.
    let rejected_sigs: Vec<Bytes> = merge_block
        .body
        .rejected_deploys
        .iter()
        .map(|rd| Bytes::copy_from_slice(rd.deploy_id()))
        .collect();
    assert!(
        rejected_sigs.contains(&late_sig),
        "a carrier chain merged after the floor closed its deploy's \
         validity window must be rejected WITH a record; got rejected sigs \
         {:?}",
        rejected_sigs
            .iter()
            .map(|s| hex::encode(&s[..8.min(s.len())]))
            .collect::<Vec<_>>()
    );

    // No-erasure pin: validly-landed history is untouched — the early
    // in-window write's effect survives and it is not spuriously rejected.
    assert!(
        !rejected_sigs.contains(&keep_sig),
        "the window rule must never reject validly-landed in-window history"
    );
    let keep_datums = nodes[1]
        .runtime_manager
        .get_data(
            merge_block.body.state.post_state_hash.clone(),
            &gstring_channel("keep"),
        )
        .await
        .expect("read @\"keep\"");
    assert!(
        !keep_datums.is_empty(),
        "the early in-window write's effect must survive the late merge"
    );

    // The effect: nothing on @"late" in the merged state.
    let late_datums = nodes[1]
        .runtime_manager
        .get_data(
            merge_block.body.state.post_state_hash.clone(),
            &gstring_channel("late"),
        )
        .await
        .expect("read @\"late\"");
    assert!(
        late_datums.is_empty(),
        "a past-window deploy's effects must not enter canonical state; \
         @\"late\" holds {} datum(s) at the merge post-state",
        late_datums.len()
    );
}
