// Regression test for the parents-post-state cache shadowing the
// rejected-deploy-buffer populate: an exploratory-deploy query computes the
// merged tip state with NO buffer attached; if that computation seeds the
// cache under the same key the validate/create path later looks up, the
// buffer populate — a side effect of the merge, not part of the cached
// value — is silently skipped, and a merge-rejected deploy never becomes
// re-proposable. The cache key must therefore distinguish bufferless from
// buffered computations.
//
// The collision is real under defaults: `disable_late_block_filtering`
// defaults to true (casper.rs `CasperShardConf::new`) and the exploratory
// path overrides it to Some(true), so the flag cannot separate the keys.

use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use serial_test::serial;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

const SEED_RHO: &str = r#"@"shadowcell"!(0)"#;

fn rmw_rho(add: i64) -> String {
    format!(
        r#"for (@v <- @"shadowcell") {{ @"shadowcell"!(v + {}) }}"#,
        add
    )
}

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .expect("build genesis");

        Self { genesis }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn bufferless_cache_seed_must_not_shadow_buffer_populate() {
    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 2, None, None, None, None)
        .await
        .expect("create_network(2)");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let seed_deploy = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            SEED_RHO.to_string(),
            None,
            None,
            None,
            None,
            Some(shard_id.clone()),
        )
        .expect("build seed deploy")
    };
    let rmw_v0 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            rmw_rho(1),
            None,
            None,
            None,
            None,
            Some(shard_id.clone()),
        )
        .expect("build rmw for V0")
    };
    let rmw_v1 = {
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        construct_deploy::source_deploy_now_full(
            rmw_rho(2),
            None,
            None,
            None,
            None,
            Some(shard_id.clone()),
        )
        .expect("build rmw for V1")
    };

    // Seed the cell everywhere, then conflicting sibling RMWs.
    let b0 = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&seed_deploy))
        .await
        .expect("V0 proposes b0 (seed)");
    let outcome = nodes[1]
        .process_block(b0.clone())
        .await
        .expect("process b0");
    assert!(
        matches!(outcome, Either::Right(_)),
        "V1 must validate b0, got {:?}",
        outcome
    );
    let b1 = nodes[0]
        .add_block_from_deploys(std::slice::from_ref(&rmw_v0))
        .await
        .expect("V0 proposes b1");
    let b2 = nodes[1]
        .add_block_from_deploys(std::slice::from_ref(&rmw_v1))
        .await
        .expect("V1 proposes b2");
    let outcome = nodes[1]
        .process_block(b1.clone())
        .await
        .expect("process b1");
    assert!(
        matches!(outcome, Either::Right(_)),
        "V1 must validate b1, got {:?}",
        outcome
    );

    // V1 now sees both siblings. Compute the merged tip state twice over
    // the same snapshot: first the exploratory way (no buffer, filtering
    // override Some(true)), then the validate/create way (buffer attached,
    // shard-config filtering).
    let snapshot = nodes[1].casper.get_snapshot().await.expect("get_snapshot");
    assert!(
        snapshot
            .parents
            .iter()
            .any(|p| p.block_hash == b1.block_hash)
            && snapshot
                .parents
                .iter()
                .any(|p| p.block_hash == b2.block_hash),
        "snapshot parents must contain both conflicting siblings; got {:?}",
        snapshot
            .parents
            .iter()
            .map(|p| hex::encode(&p.block_hash))
            .collect::<Vec<_>>()
    );
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();

    let exploratory_merged =
        casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
            &nodes[1].block_store,
            snapshot.parents.clone(),
            &snapshot,
            &nodes[1].runtime_manager,
            &latest_messages,
            Some(true),
            None,
            None,
            None,
        )
        .await
        .expect("exploratory-style merge (no buffer)");
    let rejected_record = exploratory_merged
        .rejected_user
        .first()
        .cloned()
        .expect("the conflicting RMWs must produce a merge rejection");
    let rejected_sig: Bytes = rejected_record.sig.clone();
    assert!(
        !nodes[1]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .contains_sig(&rejected_sig)
            .expect("buffer.contains_sig"),
        "the bufferless computation must not have populated the buffer"
    );

    // The populate is owner-scoped: only the carrier sender's node buffers
    // the pair. The buffered call therefore runs as the carrier's sender —
    // custody itself is pinned by the dedup-orphan spec; this spec pins the
    // cache not shadowing the populate.
    let owner: Bytes = nodes[1]
        .block_store
        .get(&rejected_record.carrier)
        .expect("carrier read")
        .expect("carrier block present")
        .sender
        .clone();

    let validate_merged =
        casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
            &nodes[1].block_store,
            snapshot.parents.clone(),
            &snapshot,
            &nodes[1].runtime_manager,
            &latest_messages,
            None,
            Some(&nodes[1].rejected_deploy_buffer),
            None,
            Some(&owner),
        )
        .await
        .expect("validate-style merge (buffer attached)");
    let sigs = |records: &[models::rust::casper::protocol::casper_message::RejectedDeploy]| {
        records.iter().map(|r| r.sig.clone()).collect::<Vec<_>>()
    };
    assert_eq!(
        sigs(&validate_merged.rejected_user),
        sigs(&exploratory_merged.rejected_user),
        "both computations must agree on the rejected set"
    );
    assert!(
        nodes[1]
            .rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .contains_sig(&rejected_sig)
            .expect("buffer.contains_sig"),
        "the buffered computation must populate the rejected-deploy buffer \
         even when a bufferless computation already seeded the cache"
    );
}
