use std::collections::HashMap;
use std::time::Duration;

use casper::rust::merging::block_index;
use casper::rust::merging::conflict_set_merger::{self, ResolvedConflicts};
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use casper::rust::util::rholang::system_deploy_util;
use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hot_store_trie_action::HotStoreTrieAction;
use shared::rust::hashable_set::HashableSet;

use crate::util::rholang::resources::with_runtime_manager;

const ORDER_GUARD_METRIC: &str = "dag.merge.order-dependent-survivor-pair";

fn pos_call(validator_pk_hex: &str, amount: i64) -> String {
    format!(
        r#"
new rl(`rho:registry:lookup`), poSCh, retCh, deployerId(`rho:system:deployerId`) in {{
  rl!(`rho:system:pos`, *poSCh) |
  for(@(_, PoS) <- poSCh) {{
    @PoS!("delegate", *deployerId, "{validator_pk_hex}".hexToBytes(), {amount}, *retCh)
  }}
}}
"#
    )
}

fn sorted_added(chain: &DeployChainIndex, channel: &Blake2b256Hash) -> Option<Vec<Vec<u8>>> {
    chain
        .state_changes
        .datums_changes
        .get(channel)
        .map(|change| {
            let mut added = change.added.clone();
            added.sort();
            added
        })
}

/// Issue #746: concurrent PoS calls in one block are independent deploy chains,
/// and each chain carries the block's whole diff on the shared PoS channel. The
/// merge applies that one diff once, so the Finding-A guard must stay silent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_pos_calls_in_one_block_do_not_trip_order_guard() {
    with_runtime_manager(|runtime_manager, genesis_context, _| async move {
        let base_state = genesis_context
            .genesis_block
            .body
            .state
            .post_state_hash
            .clone();
        let creator = &genesis_context.validator_key_pairs[0].1;
        let target_hex = hex::encode(&genesis_context.validator_key_pairs[1].1.bytes);
        let seq_num = 1;
        let block_number = 1;

        let deploys = (0..2)
            .map(|i| {
                construct_deploy::source_deploy_now_full(
                    pos_call(&target_hex, 100),
                    None,
                    None,
                    Some(genesis_context.genesis_vaults[i].0.clone()),
                    None,
                    None,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        let block_data = BlockData {
            time_stamp: deploys[0].data.time_stamp,
            seq_num,
            block_number,
            sender: creator.clone(),
        };
        let system_deploys = vec![SystemDeployEnum::Close(CloseBlockDeploy {
            initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
                creator.clone(),
                seq_num,
            ),
        })];

        let (post_state, processed_deploys, processed_system_deploys) = runtime_manager
            .compute_state(
                &base_state,
                deploys,
                system_deploys,
                block_data,
                Some(HashMap::new()),
            )
            .await
            .unwrap();
        assert_eq!(processed_deploys.len(), 2);
        assert!(processed_deploys.iter().all(|d| !d.is_failed));

        let mergeable_channels = runtime_manager
            .load_mergeable_channels(&post_state, creator.bytes.clone(), seq_num)
            .unwrap();
        let block_hash = Bytes::from(vec![0x46; 32]);
        let index = block_index::new(
            &block_hash,
            block_number,
            &processed_deploys,
            &processed_system_deploys,
            &Blake2b256Hash::from_bytes_prost(&base_state),
            &Blake2b256Hash::from_bytes_prost(&post_state),
            &runtime_manager.get_history_repo(),
            &mergeable_channels,
        )
        .unwrap();

        let user_chains: Vec<&DeployChainIndex> = processed_deploys
            .iter()
            .map(|d| {
                index
                    .deploy_chains
                    .iter()
                    .find(|chain| {
                        chain
                            .deploys_with_cost
                            .0
                            .iter()
                            .any(|c| c.deploy_id == d.deploy.sig)
                    })
                    .unwrap()
            })
            .collect();
        assert!(
            !std::ptr::eq(user_chains[0], user_chains[1]),
            "the two PoS calls must be independent deploy chains"
        );

        let shared_channel = user_chains[0]
            .state_changes
            .datums_changes
            .iter()
            .map(|entry| entry.key().clone())
            .find(|channel| {
                let first = sorted_added(user_chains[0], channel);
                first.as_ref().is_some_and(|added| added.len() == 2)
                    && first == sorted_added(user_chains[1], channel)
            })
            .expect("both sibling chains must carry the same two-datum diff on the PoS channel");

        let resolved = ResolvedConflicts {
            to_merge: index
                .deploy_chains
                .iter()
                .map(|chain| HashableSet(std::iter::once(chain.clone()).collect()))
                .collect(),
            rejected: HashableSet(Default::default()),
            late_set_size: 0,
            actual_set_size: index.deploy_chains.len(),
            branches_count: index.deploy_chains.len(),
            rejected_as_dependents_count: 0,
            optimal_rejection_count: 0,
            conflict_map_conflicts_count: 0,
            rejection_options_count: 0,
            branches_time: Duration::ZERO,
            conflicts_map_time: Duration::ZERO,
            rejection_options_time: Duration::ZERO,
        };

        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            conflict_set_merger::compute_merged_state(
                &resolved,
                &|chain: &DeployChainIndex| Ok(chain.state_changes.clone()),
                &|chain: &DeployChainIndex| chain.source_block_hash.clone(),
                &|chain: &DeployChainIndex| chain.event_log_index.number_channels_data.clone(),
                &|_, _| Ok(Vec::<HotStoreTrieAction<(), (), (), ()>>::new()),
                &|_| Ok(shared_channel.clone()),
            )
            .unwrap();
        });

        let trips: u64 = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, _, _, _)| key.key().name() == ORDER_GUARD_METRIC)
            .map(|(_, _, _, value)| match value {
                DebugValue::Counter(v) => v,
                _ => 0,
            })
            .sum();
        assert_eq!(
            trips, 0,
            "Finding-A guard tripped on sibling chains of one block"
        );
    })
    .await
    .unwrap()
}
