use casper::rust::merging::block_index;
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use models::rhoapi::CostAuthority;
use models::rust::utils::new_etuple_par;
use rholang::rust::interpreter::accounting::authority::merge_authorities;
use rholang::rust::interpreter::merging::mergeable_tags::{
    bitmask_or_mergeable_tag_name, non_negative_mergeable_tag_name, BITMASK_OR_MERGEABLE_TAG_URI,
    INTEGER_ADD_MERGEABLE_TAG_URI,
};
use rspace_plus_plus::rspace::hashing::stable_hash_provider;

use super::*;

fn numeric_authority(manager: &RuntimeManager, state: &StateHash, channel: &Par) -> CostAuthority {
    let reader = manager
        .get_history_repo()
        .get_history_reader(&Blake2b256Hash::from_bytes_prost(state))
        .unwrap();
    let data = reader
        .get_data(&stable_hash_provider::hash(channel))
        .unwrap();
    assert_eq!(data.len(), 1);
    data[0]
        .a
        .cost_authority
        .clone()
        .expect("signed numeric output authority")
}

async fn signed_writer(
    manager: &RuntimeManager,
    genesis: &BlockMessage,
    base: &StateHash,
    deploy: Cosigned<DeployData>,
    context: BlockData,
    tag: u8,
) -> (StateHash, Vec<DeployChainIndex>) {
    let admission = manager
        .certify_state_bound_admission(base, vec![deploy], &context, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(admission.outcome().admitted.len(), 1);
    assert!(admission.outcome().rejected.is_empty());
    assert!(admission.outcome().deferred.is_empty());
    let (post, processed, system, _) = manager
        .compute_state_with_bonds_cosigned_admitted(admission, Vec::new())
        .await
        .unwrap();
    assert_eq!(processed.len(), 1);
    assert!(!processed[0].is_failed);
    let mut cold = manager.clone();
    cold.replay_cache = None;
    assert_eq!(
        cold.replay_compute_state(
            base,
            processed.clone(),
            system.clone(),
            &context,
            None,
            false
        )
        .await
        .unwrap(),
        post,
    );
    let user_post = processed[0].post_state_hash.clone();
    let mut carrier = genesis.clone();
    carrier.block_hash = vec![tag; 32].into();
    carrier.header.parents_hash_list = vec![genesis.block_hash.clone()];
    carrier.header.timestamp = context.time_stamp;
    carrier.body.state.block_number = context.block_number;
    carrier.body.state.pre_state_hash = base.clone();
    carrier.body.state.post_state_hash = post;
    carrier.body.deploys = processed.clone();
    carrier.body.system_deploys = system;
    carrier.sender = context.sender.bytes;
    carrier.seq_num = context.seq_num;
    let maps = manager.load_mergeable_channels(&carrier).unwrap();
    let index = block_index::new(
        &carrier.block_hash,
        context.block_number,
        &processed,
        &Vec::new(),
        &Blake2b256Hash::from_bytes_prost(base),
        &Blake2b256Hash::from_bytes_prost(&user_post),
        &manager.get_history_repo(),
        &maps[..processed.len()].to_vec(),
    )
    .unwrap();
    assert_eq!(index.deploy_chains.len(), 1);
    for chain in &index.deploy_chains {
        chain.validate_exact_projection().unwrap();
    }
    (user_post, index.deploy_chains)
}

async fn assert_consumer_settlement(
    manager: &RuntimeManager,
    base: &StateHash,
    deploy: Cosigned<DeployData>,
    context: &BlockData,
    payers: &[PrivateKey],
    inherited: &CostAuthority,
) -> StateHash {
    let admission = manager
        .certify_state_bound_admission(base, vec![deploy], context, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(admission.outcome().admitted.len(), 1);
    assert!(admission.outcome().rejected.is_empty());
    assert!(admission.outcome().deferred.is_empty());
    let (post, processed, system, _) = manager
        .compute_state_with_bonds_cosigned_admitted(admission, Vec::new())
        .await
        .unwrap();
    assert_eq!(processed.len(), 1);
    assert!(!processed[0].is_failed);
    let certificate = processed[0].authority_funding_certificate.as_ref().unwrap();
    let witness = processed[0].authority_cost_witness.as_ref().unwrap();
    assert!(!witness.events.is_empty());
    assert!(!witness.byte_events.is_empty());
    let contains_inherited = |authority: &Option<CostAuthority>| {
        authority.as_ref().is_some_and(|authority| {
            inherited
                .regions
                .iter()
                .all(|region| authority.regions.contains(region))
        })
    };
    assert!(witness
        .events
        .iter()
        .any(|event| contains_inherited(&event.authority)));
    assert!(witness
        .byte_events
        .iter()
        .any(|event| { event.kind == 2 && contains_inherited(&event.authority) }));
    for key in payers {
        let payer = vault_payer(
            &sig_to_cost_signature(&Sig::Ground(accounting::principal_ground_v61(
                &Secp256k1.to_public(key).bytes,
            )))
            .unwrap(),
        )
        .unwrap();
        let draw = |resources: &[models::casper::CostAuthorityResourceProto]| {
            resources
                .iter()
                .filter(|resource| resource.key.as_ref() == payer.lane_key.as_slice())
                .map(|resource| resource.amount)
                .sum::<u64>()
        };
        let computation = draw(&witness.settlement);
        let bytes = draw(&witness.byte_settlement);
        let fee = draw(&certificate.fee_allocation);
        assert!(
            computation > 0,
            "each writer and the consumer must fund the COMM"
        );
        assert!(
            bytes > 0,
            "each writer and the consumer must fund transferred bytes"
        );
        assert_eq!(
            system_vault_balance(manager, base, &payer.address).await
                - system_vault_balance(manager, &post, &payer.address).await,
            i64::try_from(computation + bytes + fee).unwrap(),
        );
    }
    assert_eq!(
        authority_demand(inherited).unwrap().0.values().sum::<u64>(),
        2
    );
    let mut cold = manager.clone();
    cold.replay_cache = None;
    assert_eq!(
        cold.replay_compute_state(base, processed, system, context, None, false)
            .await
            .unwrap(),
        post,
    );
    post
}

async fn assert_insufficient_inherited_funding_and_top_up(
    manager: &RuntimeManager,
    block: &BlockMessage,
    base: &StateHash,
    consumer: Cosigned<DeployData>,
    context: &BlockData,
    keys: &[PrivateKey],
    channel: &Par,
) {
    let authority = &numeric_authority(manager, base, channel);
    let exhausted = VaultAddress::from_public_key(&Secp256k1.to_public(&keys[0])).unwrap();
    let donor = VaultAddress::from_public_key(&Secp256k1.to_public(&keys[2])).unwrap();
    let balance = system_vault_balance(manager, base, &exhausted).await;
    let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
    let drained = successful_system_state(
        ops.play_system_deploy(
            base,
            &mut ProtocolBurnDeploy::new(
                exhausted.to_base58(),
                balance,
                Blake2b512Random::create_from_bytes(&[0xe3]),
            )
            .unwrap(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(system_vault_balance(manager, &drained, &exhausted).await, 0);
    assert_eq!(numeric_authority(manager, &drained, channel), *authority);
    let rejected = manager
        .certify_state_bound_admission(&drained, vec![consumer.clone()], context, &HashMap::new())
        .await
        .unwrap();
    assert!(rejected.outcome().admitted.is_empty());
    assert_eq!(rejected.outcome().rejected.len(), 1);
    assert!(rejected.outcome().deferred.is_empty());
    let (rejected_post, rejected_deploys, rejected_system, _) = manager
        .compute_state_with_bonds_cosigned_admitted(rejected, Vec::new())
        .await
        .unwrap();
    assert!(rejected_deploys.is_empty());
    let empty = manager
        .certify_state_bound_admission(&drained, Vec::new(), context, &HashMap::new())
        .await
        .unwrap();
    let (empty_post, empty_deploys, empty_system, _) = manager
        .compute_state_with_bonds_cosigned_admitted(empty, Vec::new())
        .await
        .unwrap();
    assert_eq!(
        rejected_post, empty_post,
        "rejection must preserve the complete empty-block state"
    );
    assert_eq!(rejected_system, empty_system);
    assert_eq!(rejected_deploys, empty_deploys);
    assert_eq!(
        numeric_authority(manager, &rejected_post, channel),
        *authority
    );
    let donor_before = system_vault_balance(manager, &rejected_post, &donor).await;
    let amount = 100_000;
    let deposit = protocol_v6_source(
        format!(
            r#"
            new lookup(`rho:registry:lookup`), vaults, payerCh, keyCh, done,
                deployerId(`rho:system:deployerId`) in {{
              lookup!(`rho:vault:system`, *vaults) |
              for (@(_, systemVault) <- vaults) {{
                @systemVault!("find", "{}", *payerCh) |
                @systemVault!("deployerAuthKey", *deployerId, *keyCh) |
                for (@(true, payer) <- payerCh & key <- keyCh) {{
                  @payer!("transfer", "{}", {amount}, *key, *done) |
                  for (@result <- done) {{ @"numeric-top-up-result"!(result) }}
                }}
              }}
            }}
        "#,
            donor.to_base58(),
            exhausted.to_base58()
        ),
        203,
        keys[2].clone(),
    );
    let mut deposit_context = context.clone();
    deposit_context.time_stamp = 204;
    deposit_context.seq_num += 1;
    deposit_context.block_number += 1;
    let mut retry_context = deposit_context.clone();
    retry_context.time_stamp += 1;
    retry_context.seq_num += 1;
    retry_context.block_number += 1;
    let (funded, _) = signed_writer(
        manager,
        block,
        &rejected_post,
        deposit,
        deposit_context,
        0xe4,
    )
    .await;
    assert_eq!(
        system_vault_balance(manager, &funded, &exhausted).await,
        amount
    );
    assert!(system_vault_balance(manager, &funded, &donor).await < donor_before - amount);
    assert_eq!(numeric_authority(manager, &funded, channel), *authority);
    let post =
        assert_consumer_settlement(manager, &funded, consumer, &retry_context, keys, authority)
            .await;
    assert!(manager.get_data(post, channel).await.unwrap().is_empty());
}

async fn assert_sibling_consumers_keep_one_and_preserve_unrelated_effect(
    manager: &RuntimeManager,
    genesis: &GenesisContext,
    block: &BlockMessage,
    base: &StateHash,
    uri: &str,
    channel: &Par,
) {
    let consumer = |index: usize| {
        protocol_v6_source(
            format!(
                r#"new tag(`{uri}`) in {{ for (@value <- @(*tag, "numeric-funded")) {{ @"numeric-sibling-{index}"!(value) }} }}"#
            ),
            300 + index as i64,
            genesis.genesis_vaults[2 + index].0.clone(),
        )
    };
    let context = |index: usize| BlockData {
        time_stamp: 310,
        block_number: 2,
        sender: genesis.validator_pks()[index].clone(),
        seq_num: 2,
    };
    let unrelated = protocol_v6_source(
        r#"@"numeric-unrelated"!(17)"#.to_string(),
        302,
        genesis.genesis_vaults[0].0.clone(),
    );
    let (left, right, independent) = tokio::join!(
        signed_writer(manager, block, base, consumer(0), context(2), 0xe5),
        signed_writer(manager, block, base, consumer(1), context(3), 0xe6),
        signed_writer(manager, block, base, unrelated, context(0), 0xe7),
    );
    let contested = rspace_plus_plus::rspace::merger::merging_logic::conflicts(
        &left.1[0].event_log_index,
        &right.1[0].event_log_index,
    );
    assert!(contested.0.contains(&stable_hash_provider::hash(channel)));
    let executed_states = [left.0, right.0, independent.0];
    let chains = [left.1, right.1, independent.1].concat();
    let merged = monetary_cursor_branches::merge_branches(manager, base, &chains);
    assert_eq!(merged.rejected_state_effects.len(), 1);
    assert_eq!(merged.applied_state_effects.len(), 2);
    assert_ne!(
        merged.rejected_state_effects[0].source_block_hash,
        chains[2].source_block_hash
    );
    let retained = chains
        .iter()
        .filter(|chain| {
            chain.source_block_hash != merged.rejected_state_effects[0].source_block_hash
        })
        .cloned()
        .collect::<Vec<_>>();
    let retained_only = monetary_cursor_branches::merge_branches(manager, base, &retained);
    assert_eq!(merged.post_state, retained_only.post_state);
    let reversed = monetary_cursor_branches::merge_branches(
        manager,
        base,
        &chains.iter().rev().cloned().collect::<Vec<_>>(),
    );
    assert_eq!(merged, reversed);
    let post = merged.post_state.to_bytes_prost();
    for public_key in genesis
        .genesis_vaults
        .iter()
        .map(|(_, public_key)| public_key.clone())
        .chain(genesis.validator_pks())
    {
        let address = VaultAddress::from_public_key(&public_key).unwrap();
        let initial = system_vault_balance(manager, base, &address).await;
        let mut expected = initial;
        for (chain, executed) in chains.iter().zip(&executed_states) {
            if chain.source_block_hash != merged.rejected_state_effects[0].source_block_hash {
                expected += system_vault_balance(manager, executed, &address).await - initial;
            }
        }
        assert_eq!(
            system_vault_balance(manager, &post, &address).await,
            expected
        );
    }
    assert!(manager
        .get_data(post.clone(), channel)
        .await
        .unwrap()
        .is_empty());
    let unrelated = manager
        .get_data(
            post.clone(),
            &RhoString::create_par("numeric-unrelated".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(unrelated.len(), 1);
    assert_eq!(RhoNumber::unapply(&unrelated[0]), Some(17));
    let mut surviving_consumers = 0;
    for (index, chain) in chains.iter().take(2).enumerate() {
        let results = manager
            .get_data(
                post.clone(),
                &RhoString::create_par(format!("numeric-sibling-{index}")),
            )
            .await
            .unwrap();
        let rejected =
            merged.rejected_state_effects[0].source_block_hash == chain.source_block_hash;
        assert_eq!(results.len(), usize::from(!rejected));
        if !rejected {
            assert_eq!(RhoNumber::unapply(&results[0]), Some(3));
            surviving_consumers += 1;
        }
    }
    assert_eq!(surviving_consumers, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_numeric_zero_and_removal_effects_preserve_state_on_merge() {
    with_runtime_manager(|manager, genesis, block| async move {
        let base = block.body.state.post_state_hash.clone();
        let keys = genesis.genesis_vaults[..3]
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for (uri, tag) in [
            (
                INTEGER_ADD_MERGEABLE_TAG_URI,
                non_negative_mergeable_tag_name(),
            ),
            (
                BITMASK_OR_MERGEABLE_TAG_URI,
                bitmask_or_mergeable_tag_name(),
            ),
        ] {
            let channel = |name: &str| {
                new_etuple_par(vec![tag.clone(), RhoString::create_par(name.to_string())])
            };
            let seed_context = BlockData {
                time_stamp: 101,
                block_number: 1,
                sender: genesis.validator_pks()[0].clone(),
                seq_num: 1,
            };
            let seed = protocol_v6_source(
                format!(
                    r#"new tag(`{uri}`) in {{
                    @(*tag, "numeric-domain-removed")!(8) |
                    @(*tag, "numeric-domain-replaced")!(8) |
                    @(*tag, "numeric-domain-cancelled")!(8)
                }}"#
                ),
                100,
                keys[0].clone(),
            );
            let (seed_post, _) =
                signed_writer(&manager, &block, &base, seed, seed_context, 0xc0).await;
            let mut left_context = BlockData {
                time_stamp: 201,
                block_number: 2,
                sender: genesis.validator_pks()[1].clone(),
                seq_num: 1,
            };
            let left = protocol_v6_source(
                format!(
                    r#"new tag(`{uri}`) in {{
                    for (@removed <- @(*tag, "numeric-domain-removed")) {{
                        @"numeric-domain-removal-result"!(removed)
                    }} |
                    for (@value <- @(*tag, "numeric-domain-replaced")) {{
                        @(*tag, "numeric-domain-replaced")!(value)
                    }} |
                    for (@value <- @(*tag, "numeric-domain-cancelled")) {{
                        @(*tag, "numeric-domain-cancelled")!(value + 1)
                    }}
                }}"#
                ),
                200,
                keys[1].clone(),
            );
            let right_value = if uri == INTEGER_ADD_MERGEABLE_TAG_URI {
                "value - 1"
            } else {
                "value"
            };
            let right = protocol_v6_source(
                format!(
                    r#"new tag(`{uri}`) in {{
                    for (@value <- @(*tag, "numeric-domain-cancelled")) {{
                        @(*tag, "numeric-domain-cancelled")!({right_value})
                    }}
                }}"#
                ),
                202,
                keys[2].clone(),
            );
            let mut right_context = left_context.clone();
            right_context.sender = genesis.validator_pks()[2].clone();
            left_context.time_stamp = 203;
            right_context.time_stamp = 203;
            let (left, right) = tokio::join!(
                signed_writer(&manager, &block, &seed_post, left, left_context, 0xc1),
                signed_writer(&manager, &block, &seed_post, right, right_context, 0xc2),
            );
            let replaced = channel("numeric-domain-replaced");
            let cancelled = channel("numeric-domain-cancelled");
            let removed_hash = stable_hash_provider::hash(&channel("numeric-domain-removed"));
            let replaced_hash = stable_hash_provider::hash(&replaced);
            let cancelled_hash = stable_hash_provider::hash(&cancelled);
            let left_diffs = &left.1[0].event_log_index.number_channels_data;
            let right_diffs = &right.1[0].event_log_index.number_channels_data;
            assert!(!left_diffs.contains_key(&removed_hash));
            assert_eq!(
                left_diffs.get(&replaced_hash).map(|(diff, _)| *diff),
                Some(0)
            );
            assert_eq!(
                left_diffs.get(&cancelled_hash).map(|(diff, _)| *diff),
                Some(1)
            );
            assert_eq!(
                right_diffs.get(&cancelled_hash).map(|(diff, _)| *diff),
                Some(if uri == INTEGER_ADD_MERGEABLE_TAG_URI {
                    -1
                } else {
                    0
                }),
            );
            let removed_change = left.1[0]
                .state_changes
                .datums_changes
                .get(&removed_hash)
                .unwrap();
            assert!(removed_change.added.is_empty());
            assert_eq!(removed_change.removed.len(), 1);
            let expected_replaced = numeric_authority(&manager, &left.0, &replaced);
            let expected_cancelled = merge_authorities(&[
                numeric_authority(&manager, &left.0, &cancelled),
                numeric_authority(&manager, &right.0, &cancelled),
            ])
            .unwrap();
            let chains = [left.1, right.1].concat();
            let merged = monetary_cursor_branches::merge_branches(&manager, &seed_post, &chains);
            assert!(
                merged.rejected_state_effects.is_empty(),
                "{:?}",
                merged.rejected_state_effects
            );
            assert_eq!(merged.applied_state_effects.len(), 2);
            let reversed = monetary_cursor_branches::merge_branches(
                &manager,
                &seed_post,
                &chains.iter().rev().cloned().collect::<Vec<_>>(),
            );
            assert_eq!(merged, reversed);
            let merged_post = merged.post_state.to_bytes_prost();
            assert!(manager
                .get_data(merged_post.clone(), &channel("numeric-domain-removed"))
                .await
                .unwrap()
                .is_empty());
            for (location, value) in [
                (replaced.clone(), 8),
                (
                    cancelled.clone(),
                    if uri == INTEGER_ADD_MERGEABLE_TAG_URI {
                        8
                    } else {
                        9
                    },
                ),
                (
                    RhoString::create_par("numeric-domain-removal-result".to_string()),
                    8,
                ),
            ] {
                let data = manager
                    .get_data(merged_post.clone(), &location)
                    .await
                    .unwrap();
                assert_eq!(data.len(), 1);
                assert_eq!(RhoNumber::unapply(&data[0]), Some(value));
            }
            assert_eq!(
                numeric_authority(&manager, &merged_post, &replaced),
                expected_replaced
            );
            assert_eq!(
                numeric_authority(&manager, &merged_post, &cancelled),
                expected_cancelled
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_numeric_writers_merge_and_charge_inherited_purses_on_replay() {
    with_runtime_manager(|manager, genesis, block| async move {
        let base = block.body.state.post_state_hash.clone();
        let keys = genesis.genesis_vaults[..3]
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let context = BlockData {
            time_stamp: 201,
            block_number: 2,
            sender: genesis.validator_pks()[0].clone(),
            seq_num: 2,
        };
        for (uri, tag) in [
            (INTEGER_ADD_MERGEABLE_TAG_URI, non_negative_mergeable_tag_name()),
            (BITMASK_OR_MERGEABLE_TAG_URI, bitmask_or_mergeable_tag_name()),
        ] {
            let channel = new_etuple_par(vec![tag, RhoString::create_par("numeric-funded".to_string())]);
            let writer = |index: usize| {
                protocol_v6_source(
                    format!(r#"new tag(`{uri}`) in {{ @(*tag, "numeric-funded")!({}) }}"#, index + 1),
                    100 + index as i64,
                    keys[index].clone(),
                )
            };
            let mut writer_context = context.clone();
            writer_context.time_stamp = 101;
            writer_context.block_number = 1;
            writer_context.seq_num = 1;
            let mut right_context = writer_context.clone();
            right_context.sender = genesis.validator_pks()[1].clone();
            let (left, right) = tokio::join!(
                signed_writer(&manager, &block, &base, writer(0), writer_context, 0xe1),
                signed_writer(&manager, &block, &base, writer(1), right_context, 0xe2),
            );
            let authorities = [
                numeric_authority(&manager, &left.0, &channel),
                numeric_authority(&manager, &right.0, &channel),
            ];
            let expected = merge_authorities(&authorities).unwrap();
            let chains = [left.1, right.1].concat();
            let merged = monetary_cursor_branches::merge_branches(&manager, &base, &chains);
            assert!(
                merged.rejected_state_effects.is_empty(),
                "numeric writers were rejected: {:?}; event conflict: {:?}",
                merged.rejected_state_effects,
                rspace_plus_plus::rspace::merger::merging_logic::conflict_reason(
                    &chains[0].event_log_index, &chains[1].event_log_index,
                ),
            );
            assert_eq!(merged.applied_state_effects.len(), 2);
            let reversed = monetary_cursor_branches::merge_branches(
                &manager,
                &base,
                &chains.iter().rev().cloned().collect::<Vec<_>>(),
            );
            assert_eq!(merged, reversed);
            let merged_state = merged.post_state.to_bytes_prost();
            assert_eq!(numeric_authority(&manager, &merged_state, &channel), expected);
            let values = manager.get_data(merged_state.clone(), &channel).await.unwrap();
            assert_eq!(values.len(), 1);
            assert_eq!(RhoNumber::unapply(&values[0]), Some(3));
            let consumer = protocol_v6_source(
                format!(r#"new tag(`{uri}`) in {{ for (@value <- @(*tag, "numeric-funded")) {{ @"numeric-funded-result"!(value) }} }}"#),
                200,
                keys[2].clone(),
            );
            let post = assert_consumer_settlement(
                &manager, &merged_state, consumer.clone(), &context, &keys, &expected,
            )
            .await;
            assert!(manager.get_data(post.clone(), &channel).await.unwrap().is_empty());
            let result = manager
                .get_data(post, &RhoString::create_par("numeric-funded-result".to_string()))
                .await
                .unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(RhoNumber::unapply(&result[0]), Some(3));
            assert_insufficient_inherited_funding_and_top_up(
                &manager, &block, &merged_state, consumer, &context, &keys, &channel,
            )
            .await;
            if uri == INTEGER_ADD_MERGEABLE_TAG_URI {
                assert_sibling_consumers_keep_one_and_preserve_unrelated_effect(
                    &manager, &genesis, &block, &merged_state, uri, &channel,
                )
                .await;
            }
        }
    })
    .await
    .unwrap();
}
