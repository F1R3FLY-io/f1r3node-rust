// See casper/src/test/scala/coop/rchain/casper/merging/MergingCases.scala

use std::collections::HashMap;

// Added by DR-134 (DR-65, gap G2): the offered play takes pending candidates.
use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
use casper::rust::merging::block_index;
// Changed by DR-134 (DR-65, gap G2): offered deploys.
// use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::system_deploy_util;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic;

// Added by DR-134 (DR-65, gap G2): offered deploys.
use crate::helper::offered_deploy as construct_deploy;
// Changed by DR-134 (DR-65, gap G2): the adopted policy of the test genesis.
// use crate::util::rholang::resources::with_runtime_manager;
use crate::util::rholang::resources::{adopted_policy, with_runtime_manager};

/**
 * Two deploys inside single state transition are using the same PVV for precharge and refund.
 * So this should be dependent over produce that puts new value into PVV balance in the first deploy.
 * TODO adjust this once/if there is a solution to make deploys touching the same PVV non dependent
 */
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_deploys_executed_inside_single_state_transition_should_be_dependent() {
    with_runtime_manager(|runtime_manager, genesis_context, _| async move {
        // Added by DR-134 (DR-65, gap G2): the offered play takes the adopted
        // policy of the test genesis. It reads the genesis block before the
        // line below moves the post-state out of it.
        let adopted = adopted_policy(&runtime_manager, &genesis_context.genesis_block).await;
        let base_state = genesis_context.genesis_block.body.state.post_state_hash;
        let payer1_key = &genesis_context.genesis_vaults[0].0;
        let payer2_key = &genesis_context.genesis_vaults[1].0;
        let state_transition_creator = &genesis_context.validator_key_pairs[0].1;
        let seq_num = 1;
        let block_num = 1;

        let d1 = construct_deploy::source_deploy_now_full(
            "Nil".to_string(),
            None,
            None,
            Some(payer1_key.clone()),
            None,
            None,
        )
        .unwrap();

        let d2 = construct_deploy::source_deploy_now_full(
            "Nil".to_string(),
            None,
            None,
            Some(payer2_key.clone()),
            None,
            None,
        )
        .unwrap();

        // Changed by DR-134 (DR-65, gap G2): the block time of the offers. The
        // two offers have different signers, so they are distinct envelopes.
        // let block_data = BlockData {
        //     time_stamp: d1.data.time_stamp,
        let block_data = BlockData {
            time_stamp: d1.envelope.body().time_stamp,
            seq_num,
            block_number: block_num,
            sender: state_transition_creator.clone(),
        };

        let invalid_blocks = HashMap::new();
        // Changed by DR-134 (DR-65, gap G2): one block carries the batch of both
        // offers.
        // let user_deploys = vec![d1, d2];
        let user_deploys = vec![
            PendingDeployCandidate::Envelope(d1.envelope),
            PendingDeployCandidate::Envelope(d2.envelope),
        ];
        let system_deploys = vec![
            casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                CloseBlockDeploy {
                    initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
                        state_transition_creator.clone(),
                        seq_num,
                    ),
                },
            ),
        ];

        // Changed by DR-134 (DR-65, gap G2): the offered play of the batch.
        // let (post_state_hash, processed_deploys, _) = runtime_manager
        //     .compute_state(
        //         &base_state,
        //         user_deploys,
        //         system_deploys,
        //         block_data,
        //         Some(invalid_blocks),
        //     )
        //     .await
        //     .unwrap();
        let (post_state_hash, processed_deploys, _, _) = runtime_manager
            .compute_state_with_bonds_envelopes(
                &base_state,
                user_deploys,
                system_deploys,
                block_data,
                Some(invalid_blocks),
                None,
                Some(&adopted),
            )
            .await
            .expect("the batch of two offers plays");

        assert_eq!(processed_deploys.len(), 2);

        let mergeable_channels = runtime_manager
            .load_mergeable_channels(
                &post_state_hash,
                state_transition_creator.bytes.clone(),
                seq_num,
            )
            .unwrap();

        // Combine processed deploys with cached mergeable channels data
        let processed_deploys_with_mergeable = processed_deploys
            .iter()
            .cloned()
            .zip(mergeable_channels)
            .collect::<Vec<_>>();

        let idxs = processed_deploys_with_mergeable
            .into_iter()
            .map(|(d, merge_chs)| {
                block_index::create_event_log_index(
                    // Changed by DR-134 (DR-65, gap G2): the log of a processed
                    // offer.
                    // &d.deploy_log,
                    d.deploy_log(),
                    runtime_manager.get_history_repo(),
                    &Blake2b256Hash::from_bytes_prost(&base_state),
                    merge_chs,
                )
            })
            .collect::<Vec<_>>();

        let first_depends = merging_logic::depends(&idxs[0], &idxs[1]);
        let second_depends = merging_logic::depends(&idxs[1], &idxs[0]);
        let conflicts = merging_logic::are_conflicting(&idxs[0], &idxs[1]);

        let deploy_chains = merging_logic::compute_related_sets(
            &idxs.iter().cloned().collect(),
            merging_logic::depends,
        );

        // deploys inside one state transition never conflict, as executed in a sequence (for now)
        assert!(!conflicts);
        // first deploy does not depend on the second
        assert!(!first_depends);
        // second deploy depends on the first, as it consumes produce put by first one when updating per validator vault balance
        assert!(!second_depends);
        // deploys should be be put in separate deploy chains
        assert_eq!(deploy_chains.0.len(), 2);
    })
    .await
    .unwrap()
}
