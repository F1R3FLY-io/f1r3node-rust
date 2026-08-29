// On an LFS-restored node a validator's latest-message slot can name a block
// below the restore horizon — held as a hash only, never indexed. Read paths
// that materialize latest messages must abstain that validator, not error:
// one stale slot otherwise fails every fault-tolerance read (the #306
// `lookup_unsafe` storm) and every fork-choice run on the node.

use std::collections::HashMap;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
use casper::rust::estimator::Estimator;
use casper::rust::safety::clique_oracle::CliqueOracle;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block_default;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;
use parking_lot::RwLock as PlRwLock;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use crate::helper::block_util::generate_validator;

fn bonded_block(
    number: i64,
    parents: Vec<BlockHash>,
    sender: Validator,
    bonds: &[(Validator, i64)],
) -> BlockMessage {
    let mut block = get_random_block_default();
    block.body.state.block_number = number;
    block.header.parents_hash_list = parents;
    block.sender = sender;
    block.body.state.bonds = bonds
        .iter()
        .map(|(validator, stake)| Bond {
            validator: validator.clone(),
            stake: *stake,
        })
        .collect();
    block
}

/// Two bonded validators; a two-block held chain (genesis -> target); one
/// validator's latest message poisoned to an unheld hash — the restored
/// node's shape.
fn restored_dag_with_stale_lm() -> (KeyValueDagRepresentation, BlockMessage, BlockMessage) {
    let v_live = generate_validator(Some("live"));
    let v_stale = generate_validator(Some("stale"));
    let bonds = [(v_live.clone(), 100i64), (v_stale.clone(), 100i64)];

    let genesis = bonded_block(0, vec![], Validator::new(), &bonds);
    let target = bonded_block(1, vec![genesis.block_hash.clone()], v_live.clone(), &bonds);
    let below_horizon = BlockHash::from(b"below-restore-horizon-lm".to_vec());

    let metadata_store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
    let mut metadata_index = BlockMetadataStore::new(metadata_store);
    metadata_index
        .add(BlockMetadata::from_block(&genesis, false, None, None))
        .expect("seed genesis metadata");
    metadata_index
        .add(BlockMetadata::from_block(&target, false, None, None))
        .expect("seed target metadata");

    let mut dag_set = imbl::HashSet::new();
    let mut block_number_map = imbl::HashMap::new();
    let mut height_map = imbl::OrdMap::new();
    for (hash, number) in [(&genesis.block_hash, 0i64), (&target.block_hash, 1)] {
        dag_set.insert(hash.clone());
        block_number_map.insert(hash.clone(), number);
        height_map
            .entry(number)
            .or_insert_with(imbl::HashSet::new)
            .insert(hash.clone());
    }
    let mut main_parent_map = imbl::HashMap::new();
    main_parent_map.insert(target.block_hash.clone(), genesis.block_hash.clone());
    let mut child_map = imbl::HashMap::new();
    child_map.insert(
        genesis.block_hash.clone(),
        imbl::HashSet::unit(target.block_hash.clone()),
    );

    let mut latest_messages_map = imbl::HashMap::new();
    latest_messages_map.insert(v_live, target.block_hash.clone());
    latest_messages_map.insert(v_stale, below_horizon);

    let mut finalized_blocks_set = imbl::HashSet::new();
    finalized_blocks_set.insert(genesis.block_hash.clone());

    let dag = KeyValueDagRepresentation {
        dag_set,
        latest_messages_map,
        child_map,
        height_map,
        block_number_map,
        main_parent_map,
        self_justification_map: imbl::HashMap::new(),
        invalid_blocks_set: imbl::HashSet::new(),
        last_finalized_block_hash: genesis.block_hash.clone(),
        finalized_blocks_set,
        block_metadata_index: Arc::new(PlRwLock::new(metadata_index)),
        floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        lifecycle: Arc::new(PlRwLock::new(
            block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(),
        )),
    };
    (dag, genesis, target)
}

#[tokio::test]
async fn a_stale_latest_message_does_not_fail_fault_tolerance() {
    let (dag, _genesis, target) = restored_dag_with_stale_lm();

    let result = CliqueOracle::normalized_fault_tolerance(&target.block_hash, &dag).await;

    assert!(
        result.is_ok(),
        "one unheld latest-message slot must abstain, not fail every \
         fault-tolerance read on the node: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn a_stale_latest_message_does_not_fail_fork_choice() {
    let (mut dag, genesis, target) = restored_dag_with_stale_lm();
    let latest_messages: HashMap<Validator, BlockHash> =
        dag.latest_messages_map.clone().into_iter().collect();

    let estimator = Estimator::apply(1, None);
    let result = estimator
        .tips_with_latest_messages(&mut dag, &genesis, latest_messages)
        .await;

    match result {
        Ok(fork_choice) => {
            assert!(
                fork_choice.tips.contains(&target.block_hash),
                "fork choice must still rank the held chain, got {:?}",
                fork_choice.tips
            );
        }
        Err(err) => panic!(
            "one unheld latest-message slot must abstain, not fail every \
             fork-choice run on the node: {err:?}"
        ),
    }
}
