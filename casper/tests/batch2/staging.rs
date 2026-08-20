//! Shared DAG-geometry staging for the batch2 specs: helpers that force
//! block shapes fork choice would never mint on its own (explicit parent
//! ordering, sibling races, withheld deliveries).

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::Casper;
use models::rust::casper::protocol::casper_message::BlockMessage;

use crate::helper::test_node::TestNode;

/// Mint a block on `node` from explicitly ORDERED parents (parents[0] is
/// the main parent — the spine the frontier walk follows), process it on
/// the same node, and return it. The snapshot's numbering is re-anchored to
/// the overridden parent set (`create` numbers from `max_block_num`, not
/// the parent list).
pub async fn mint_on_parents(
    node: &mut TestNode,
    parents: Vec<BlockMessage>,
    label: &str,
) -> BlockMessage {
    for p in &parents {
        assert!(
            node.casper.dag_contains(&p.block_hash),
            "staging[{label}]: parent {} must be IN THE DAG of the minting \
             node (buffered: {})",
            hex::encode(&p.block_hash[..6]),
            node.casper.buffer_contains(&p.block_hash),
        );
    }
    let mut snapshot = node.casper.get_snapshot().await.expect("snapshot");
    snapshot.max_block_num = parents
        .iter()
        .map(|p| p.body.state.block_number)
        .max()
        .expect("non-empty parent set");
    snapshot.parents = parents;
    let validator_identity = node.validator_id_opt.clone().expect("validator identity");
    let deploy_storage = node.deploy_storage.clone();
    let rejected_buffer = node.rejected_deploy_buffer.clone();
    let runtime_manager = node.runtime_manager.clone();
    let created = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage,
        rejected_buffer,
        &runtime_manager,
        &mut node.block_store,
        true,
    )
    .await
    .unwrap_or_else(|e| panic!("create[{label}] must succeed: {:?}", e));
    let BlockCreatorResult::Created(block, _pre, _post) = created else {
        panic!("create[{label}] must mint on the ordered parent set");
    };
    node.process_block(block.clone())
        .await
        .expect("self-process minted block");
    block
}
