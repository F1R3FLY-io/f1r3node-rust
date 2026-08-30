use std::sync::Arc;

use casper::rust::casper::CasperShardConf;
use casper::rust::util::mergeable_channels_gc::{collect_garbage, GcSweep};

use crate::helper::block_dag_storage_fixture::with_genesis;
use crate::helper::block_generator::{build_block_at_height, step};
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn collection_deletes_only_the_safe_authenticated_execution() {
    let context = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1)),
        ))
        .await
        .unwrap();

    with_genesis(
        context.clone(),
        |mut block_store, mut block_dag_storage, mut runtime_manager| async move {
            let genesis = context.genesis_block.clone();
            let creator = context.validator_pks()[0].bytes.clone();
            let mut parent = genesis.clone();
            let mut chain = Vec::new();

            for height in 1..=4 {
                let candidate = build_block_at_height(
                    height,
                    vec![parent.block_hash.clone()],
                    Some(creator.clone()),
                    height * 100,
                    None,
                    None,
                    None,
                    None,
                    Some(genesis.shard_id.clone()),
                    None,
                    Some(height as i32),
                );
                step(
                    &mut block_dag_storage,
                    &mut block_store,
                    &mut runtime_manager,
                    &candidate,
                )
                .await
                .unwrap();
                parent = block_store.get(&candidate.block_hash).unwrap().unwrap();
                chain.push(parent.clone());
            }

            block_dag_storage
                .record_directly_finalized(chain[0].block_hash.clone(), 1.0, |_| async { Ok(()) })
                .await
                .unwrap();

            let mut dag = block_dag_storage.get_representation().unwrap();
            dag.child_map
                .insert(genesis.block_hash.clone(), Default::default());

            let mut conf = CasperShardConf::new();
            conf.max_parent_depth = 1;
            conf.mergeable_channels_gc_depth_buffer = 1;

            assert!(runtime_manager.has_mergeable_entry(&chain[0]).unwrap());
            assert!(runtime_manager.has_mergeable_entry(&chain[1]).unwrap());

            let runtime_manager = Arc::new(runtime_manager);
            let mut sweep = GcSweep::new();
            let deleted = collect_garbage(&mut sweep, &dag, &block_store, &runtime_manager, &conf)
                .await
                .unwrap();

            assert_eq!(deleted, 1);
            assert!(!runtime_manager.has_mergeable_entry(&chain[0]).unwrap());
            assert!(runtime_manager.has_mergeable_entry(&chain[1]).unwrap());

            let repeated = collect_garbage(&mut sweep, &dag, &block_store, &runtime_manager, &conf)
                .await
                .unwrap();
            assert_eq!(repeated, 0);
            assert!(runtime_manager.has_mergeable_entry(&chain[1]).unwrap());
        },
    )
    .await;
}
