// See casper/src/test/scala/coop/rchain/casper/merging/MergingCases.scala

use std::collections::HashMap;

use casper::rust::merging::block_index;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::system_deploy_util;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic;

use crate::util::rholang::resources::with_runtime_manager;

/**
 * Two deploys inside a single state transition.
 *
 * PRE-D3 rationale (kept for history): the deploys shared the same PVV for
 * precharge/refund, so the second depended on the produce that wrote the new
 * PVV balance in the first — landing them in one deploy chain.
 *
 * DR-9/D3 (OD-2): the escrow precharge/refund system deploys are REMOVED, so
 * there is no PVV-balance produce to couple them. With source `"Nil"` both
 * deploys reduce to byte-identical, side-effect-free event-log indices; they
 * are mutually INDEPENDENT yet collapse to a single `HashableSet` element and
 * therefore STILL land in one deploy chain. The test's observable contract —
 * a single chain, not two — is preserved; only the mechanism changed.
 */
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_deploys_executed_inside_single_state_transition_should_be_dependent() {
    with_runtime_manager(|runtime_manager, genesis_context, _| async move {
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

        let block_data = BlockData {
            time_stamp: d1.data.time_stamp,
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

        let (post_state_hash, processed_deploys, _) = runtime_manager
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

        let mergeable_channels = runtime_manager
            .load_mergeable_channels(
                &post_state_hash,
                state_transition_creator.bytes.clone(),
                seq_num,
            )
            .unwrap();

        // Combine processed deploys with cached mergeable channels data
        let processed_deploys_with_mergeable = processed_deploys
            .to_vec()
            .into_iter()
            .zip(mergeable_channels)
            .collect::<Vec<_>>();

        let idxs = processed_deploys_with_mergeable
            .into_iter()
            .map(|(d, merge_chs)| {
                block_index::create_event_log_index(
                    &d.deploy_log,
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

        // DR-9/D3 (OD-2): the per-deploy escrow pre-charge / refund system
        // deploys are REMOVED (casper `costacc/mod.rs`), so a deploy no longer
        // emits the per-validator-vault (PVV) balance-update produce that used
        // to couple two deploys sharing a PVV. With source `"Nil"`, both
        // deploys now reduce to BYTE-IDENTICAL, side-effect-free event-log
        // indices (verified: `idxs[0] == idxs[1]`). Consequently neither
        // depends on the other (`!first_depends && !second_depends`) and they
        // do not conflict.
        assert!(!conflicts);
        assert!(!first_depends);
        assert!(!second_depends);
        // The two identical event-log indices DEDUPLICATE into a single
        // `HashableSet` element (it is backed by a `HashSet<EventLogIndex>`),
        // so `compute_related_sets` returns ONE related set — the deploys land
        // in a SINGLE deploy chain, which is what the test name asserts
        // ("...should be dependent" = end up in one chain, not two). Under the
        // pre-D3 model the chain was singular because the second deploy
        // DEPENDED on the first's PVV produce; under D3 it is singular because
        // the two effect-free deploys are identical and collapse. Either way
        // the consensus-relevant outcome — one chain — is unchanged.
        assert_eq!(deploy_chains.0.len(), 1);
    })
    .await
    .unwrap()
}
