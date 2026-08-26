// See casper/src/test/scala/coop/rchain/casper/merging/MergingCases.scala

use std::collections::HashMap;

use casper::rust::merging::block_index;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::system_deploy_util;
use models::rust::casper::protocol::casper_message::{BlockMessage, Body, F1r3flyState, Header};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic;

use crate::util::rholang::resources::with_runtime_manager;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_deploys_executed_inside_single_state_transition_should_be_dependent() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let base_state = genesis_context
                .genesis_block
                .body
                .state
                .post_state_hash
                .clone();
            let payer1_key = &genesis_context.genesis_vaults[0].0;
            let payer2_key = &genesis_context.genesis_vaults[1].0;
            let state_transition_creator = &genesis_context.validator_key_pairs[0].1;
            let seq_num = 1;
            let block_num = 1;

            let d1 = construct_deploy::source_deploy_now_full(
                "@\"causal-dependency\"!(0)".to_string(),
                None,
                None,
                Some(payer1_key.clone()),
                None,
                None,
            )
            .unwrap();

            let d2 = construct_deploy::source_deploy_now_full(
                "for (_ <- @\"causal-dependency\") { Nil }".to_string(),
                None,
                None,
                Some(payer2_key.clone()),
                None,
                None,
            )
            .unwrap();
            let producer_sig = d1.sig.clone();
            let consumer_sig = d2.sig.clone();

            let block_timestamp = d1.data.time_stamp;
            let block_data = BlockData {
                time_stamp: block_timestamp,
                seq_num,
                block_number: block_num,
                sender: state_transition_creator.clone(),
            };

            let invalid_blocks = HashMap::new();
            let user_deploys = vec![d1, d2];
            let system_deploys = vec![
                casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                    CloseBlockDeploy::new(
                        system_deploy_util::generate_close_deploy_random_seed_from_pk(
                            state_transition_creator.clone(),
                            seq_num,
                        ),
                    ),
                ),
            ];

            let (post_state_hash, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &base_state,
                    user_deploys,
                    system_deploys,
                    block_data,
                    Some(invalid_blocks),
                )
                .await
                .unwrap();

            assert_eq!(processed_deploys.len(), 2);

            let block = BlockMessage {
                block_hash: vec![0x41; 32].into(),
                header: Header {
                    parents_hash_list: vec![genesis_block.block_hash],
                    timestamp: block_timestamp,
                    version: genesis_block.header.version,
                    extra_bytes: Vec::<u8>::new().into(),
                    sender_bond_generation: genesis_block.header.sender_bond_generation,
                    objective_equivocation_evidence_delta: Vec::new(),
                },
                body: Body {
                    state: F1r3flyState {
                        pre_state_hash: base_state.clone(),
                        post_state_hash: post_state_hash.clone(),
                        bonds: Vec::new(),
                        bond_generations: Vec::new(),
                        active_validators: Vec::new(),
                        block_number: block_num,
                    },
                    deploys: processed_deploys.clone(),
                    rejected_deploys: Vec::new(),
                    rejected_state_effects: Vec::new(),
                    system_deploys: processed_system_deploys,
                    extra_bytes: Vec::<u8>::new().into(),
                },
                justifications: Vec::new(),
                sender: state_transition_creator.bytes.clone(),
                seq_num,
                sig: Vec::<u8>::new().into(),
                sig_algorithm: String::new(),
                shard_id: genesis_block.shard_id,
                extra_bytes: Vec::<u8>::new().into(),
            };

            let mergeable_channels = runtime_manager.load_mergeable_channels(&block).unwrap();

            let processed_deploys_with_mergeable = processed_deploys
                .iter()
                .cloned()
                .zip(mergeable_channels)
                .collect::<Vec<_>>();

            let idxs = processed_deploys_with_mergeable
                .into_iter()
                .map(|(d, merge_chs)| {
                    (
                        d.deploy.sig,
                        block_index::create_event_log_index(
                            &d.deploy_log,
                            runtime_manager.get_history_repo(),
                            &Blake2b256Hash::from_bytes_prost(&base_state),
                            merge_chs,
                        ),
                    )
                })
                .collect::<Vec<_>>();

            let producer_idx = &idxs
                .iter()
                .find(|(sig, _)| sig == &producer_sig)
                .expect("producer deploy index")
                .1;
            let consumer_idx = &idxs
                .iter()
                .find(|(sig, _)| sig == &consumer_sig)
                .expect("consumer deploy index")
                .1;

            let producer_depends = merging_logic::depends(producer_idx, consumer_idx);
            let consumer_depends = merging_logic::depends(consumer_idx, producer_idx);
            let conflicts = merging_logic::are_conflicting(producer_idx, consumer_idx);

            let producer_surviving =
                merging_logic::produces_created_and_not_destroyed(producer_idx);
            let producer_non_mergeable = producer_surviving
                .0
                .difference(&producer_idx.produces_mergeable.0)
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            let consumer_consumed_non_mergeable = consumer_idx
                .produces_consumed
                .0
                .difference(&producer_idx.produces_mergeable.0)
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            let causal_produces = producer_non_mergeable
                .intersection(&consumer_consumed_non_mergeable)
                .collect::<Vec<_>>();

            let consumer_surviving =
                merging_logic::consumes_created_and_not_destroyed(consumer_idx);
            let causal_consumes = consumer_surviving
                .0
                .intersection(&producer_idx.consumes_produced.0)
                .collect::<Vec<_>>();

            let deploy_chains = merging_logic::compute_related_sets(
                &idxs.iter().map(|(_, idx)| idx.clone()).collect(),
                merging_logic::depends,
            );

            assert!(!conflicts);
            assert_eq!(causal_produces.len() + causal_consumes.len(), 1);
            assert_eq!(producer_depends, causal_consumes.len() == 1);
            assert_eq!(consumer_depends, causal_produces.len() == 1);
            assert_ne!(producer_idx, consumer_idx);
            assert_eq!(deploy_chains.0.len(), 1);
        },
    )
    .await
    .unwrap()
}
