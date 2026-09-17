use std::num::NonZeroUsize;

use acceptance::SupplyReader;
use casper::rust::merging::block_index;
use casper::rust::merging::dag_merger::{self, MergeOccurrenceContext, MergeResult};
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use rholang::rust::interpreter::accounting::monetary_allocation::{
    MonetaryCursor, MonetaryCursorTransition,
};
use rspace_plus_plus::rspace::merger::merging_logic::are_conflicting;

use super::*;

fn charge(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    scope: [u8; 32],
    revision: i64,
    next_position: i64,
    tag: u8,
) -> ApplyCostDeploy {
    let count = NonZeroUsize::new(2).unwrap();
    ApplyCostDeploy::new(
        [tag; 32],
        vec![VaultAllocation::new(payer.to_base58(), 1).unwrap()],
        vec![VaultSettlement::new(payer.to_base58(), 0, 1).unwrap()],
        recipient.to_base58(),
        Blake2b512Random::create_from_bytes(&[tag]),
    )
    .unwrap()
    .with_fee_cursor(
        MonetaryCursorTransition::new(
            scope,
            MonetaryCursor::new(revision, 0, count).unwrap(),
            next_position,
            count,
        )
        .unwrap(),
        count,
    )
    .unwrap()
}

fn branch_index(
    manager: &RuntimeManager,
    base: &StateHash,
    result: SystemDeployResult<()>,
    tag: u8,
) -> (StateHash, DeployChainIndex) {
    let SystemDeployResult::PlaySucceeded {
        state_hash,
        processed_system_deploy,
        mergeable_channels,
        ..
    } = result
    else {
        panic!("each isolated branch must accept its fresh cursor plan");
    };
    let root = Blake2b256Hash::from_bytes_prost(base);
    let diffs = manager
        .convert_number_channels_to_diff(vec![mergeable_channels], &root)
        .unwrap();
    let mut index = block_index::new(
        &vec![tag; 32].into(),
        1,
        &Vec::new(),
        &vec![processed_system_deploy],
        &root,
        &Blake2b256Hash::from_bytes_prost(&state_hash),
        &manager.get_history_repo(),
        &diffs,
    )
    .unwrap();
    assert_eq!(index.deploy_chains.len(), 1);
    let chain = index.deploy_chains.remove(0);
    chain.validate_exact_projection().unwrap();
    (state_hash, chain)
}

pub(super) fn merge_branches(
    manager: &RuntimeManager,
    base: &StateHash,
    chains: &[DeployChainIndex],
) -> MergeResult {
    let base_hash: prost::bytes::Bytes = vec![0xd0; 32].into();
    let mut dag = resources::new_key_value_dag_representation();
    dag.dag_set.insert(base_hash.clone());
    dag.block_number_map.insert(base_hash.clone(), 0);
    for chain in chains {
        let hash = chain.source_block_hash.clone();
        dag.dag_set.insert(hash.clone());
        dag.block_number_map.insert(hash.clone(), 1);
        dag.main_parent_map.insert(hash.clone(), base_hash.clone());
        dag.child_map
            .entry(base_hash.clone())
            .or_default()
            .insert(hash);
    }
    dag_merger::merge(
        &dag,
        &base_hash,
        &Blake2b256Hash::from_bytes_prost(base),
        |hash| {
            Ok(chains
                .iter()
                .filter(|chain| chain.source_block_hash == hash)
                .cloned()
                .collect())
        },
        &manager.get_history_repo(),
        dag_merger::cost_optimal_rejection_alg(),
        Some(
            chains
                .iter()
                .map(|chain| chain.source_block_hash.clone())
                .collect(),
        ),
        true,
        0,
        50,
        &|_| Ok(false),
        &|_| Ok(false),
        &Default::default(),
        &Default::default(),
        &MergeOccurrenceContext {
            require_exact_effects: true,
            ..Default::default()
        },
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monetary_cursor_independent_branches_preserve_scope_conflicts() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let payers = [0, 1]
            .map(|index| VaultAddress::from_public_key(&genesis.genesis_vaults[index].1).unwrap());
        let recipients = [0xc1, 0xc2].map(|tag| {
            VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![tag; 32] })
        });
        assert_ne!(payers[0], payers[1]);
        assert_ne!(recipients[0], recipients[1]);
        for initialized in [false, true] {
            for shared_scope in [false, true] {
                let scopes = [[0xc3; 32], [if shared_scope { 0xc3 } else { 0xc4 }; 32]];
                let mut base = initial.clone();
                if initialized {
                    let mut init_ops = RuntimeOps::new(manager.spawn_runtime().await);
                    for index in 0..if shared_scope { 1 } else { 2 } {
                        base = successful_system_state(
                            init_ops
                                .play_system_deploy(
                                    &base,
                                    &mut charge(
                                        &payers[index],
                                        &recipients[index],
                                        scopes[index],
                                        0,
                                        0,
                                        0xc5 + index as u8,
                                    ),
                                )
                                .await
                                .unwrap(),
                        );
                    }
                }
                let mut left = RuntimeOps::new(manager.spawn_runtime().await);
                let mut right = RuntimeOps::new(manager.spawn_runtime().await);
                let mut left_deploy = charge(
                    &payers[0],
                    &recipients[0],
                    scopes[0],
                    i64::from(initialized),
                    0,
                    0xc7,
                );
                let mut right_deploy = charge(
                    &payers[1],
                    &recipients[1],
                    scopes[1],
                    i64::from(initialized),
                    1,
                    0xc8,
                );
                let (left_result, right_result) = tokio::join!(
                    left.play_system_deploy(&base, &mut left_deploy),
                    right.play_system_deploy(&base, &mut right_deploy),
                );
                let (left_state, left_index) =
                    branch_index(&manager, &base, left_result.unwrap(), 0xd1);
                let (right_state, right_index) =
                    branch_index(&manager, &base, right_result.unwrap(), 0xd2);
                for (index, state) in [&left_state, &right_state].into_iter().enumerate() {
                    assert_eq!(
                        system_vault_balance(&manager, state, &payers[index]).await,
                        system_vault_balance(&manager, &base, &payers[index]).await - 1,
                    );
                    assert_eq!(
                        system_vault_balance(&manager, state, &payers[1 - index]).await,
                        system_vault_balance(&manager, &base, &payers[1 - index]).await,
                    );
                }
                assert_eq!(
                    are_conflicting(&left_index.event_log_index, &right_index.event_log_index),
                    initialized && shared_scope,
                );
                let chains = [left_index, right_index];
                let merged = merge_branches(&manager, &base, &chains);
                let reversed =
                    merge_branches(&manager, &base, &[chains[1].clone(), chains[0].clone()]);
                assert_eq!(merged, reversed, "branch input order must not affect merge");
                assert_eq!(
                    merged.rejected_state_effects.len(),
                    usize::from(shared_scope)
                );
                assert_eq!(
                    merged.applied_state_effects.len(),
                    2 - usize::from(shared_scope)
                );
                let state = merged.post_state.to_bytes_prost();
                let reader = acceptance::RuntimeManagerSupplyReader {
                    runtime_manager: &manager,
                    pre_state_hash: state.clone(),
                };
                for (index, chain) in chains.iter().enumerate() {
                    let rejected = merged.rejected_state_effects.iter().any(|effect| {
                        effect.source_block_hash == chain.source_block_hash
                            && effect.execution_index == 0
                    });
                    assert_eq!(
                        system_vault_balance(&manager, &state, &payers[index]).await,
                        system_vault_balance(&manager, &base, &payers[index]).await
                            - i64::from(!rejected),
                    );
                    assert_eq!(
                        system_vault_balance(&manager, &state, &recipients[index]).await,
                        system_vault_balance(&manager, &base, &recipients[index]).await
                            + i64::from(!rejected),
                    );
                    if !rejected {
                        assert_eq!(
                            reader
                                .read_monetary_cursor(scopes[index], NonZeroUsize::new(2).unwrap())
                                .await
                                .unwrap(),
                            Some(
                                MonetaryCursor::new(
                                    i64::from(initialized) + 1,
                                    index as i64,
                                    NonZeroUsize::new(2).unwrap()
                                )
                                .unwrap()
                            )
                        );
                        if shared_scope {
                            let survivor_only =
                                merge_branches(&manager, &base, std::slice::from_ref(chain));
                            assert_eq!(
                                merged.post_state, survivor_only.post_state,
                                "the rejected branch must leave no merged state effects"
                            );
                            assert_eq!(
                                &state,
                                [&left_state, &right_state][index],
                                "survivor-only merging must preserve the complete executed state"
                            );
                        }
                    }
                }
            }
        }
    })
    .await
    .unwrap();
}
