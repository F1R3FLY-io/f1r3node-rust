use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Bonded validators that have never proposed carry the genesis hash in their
/// latest-message slot. Once this node has a block of its own, the snapshot's
/// parent must be that block — a placeholder slot must not put genesis beside
/// it. Genesis is height 0, and a height-0 candidate bounds every ancestry
/// walk at zero, which on a restored node reaches below the restore horizon.
#[tokio::test]
async fn a_snapshot_never_cites_a_genesis_placeholder_as_a_parent() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("build genesis");
    let genesis_hash = genesis.genesis_block.block_hash.clone();
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut node = TestNode::standalone(genesis)
        .await
        .expect("standalone node");

    let dag = node.casper.block_dag().await.expect("dag");
    let placeholder_slots = dag
        .latest_message_hashes()
        .iter()
        .filter(|(_, hash)| **hash == genesis_hash)
        .count();
    assert!(
        placeholder_slots > 0,
        "fixture must seed a placeholder for a bonded validator with no blocks"
    );

    let deploy = construct_deploy::basic_deploy_data(0, None, Some(shard_id)).expect("deploy");
    let own = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("propose one block");

    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let parents: Vec<_> = snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect();

    assert!(
        parents.contains(&own.block_hash),
        "this node's own block is a parent; got {parents:?}"
    );
    assert!(
        !parents.contains(&genesis_hash),
        "the placeholder slots must be abstained: genesis is not a parent once \
         a real one exists; got {parents:?}"
    );
}
