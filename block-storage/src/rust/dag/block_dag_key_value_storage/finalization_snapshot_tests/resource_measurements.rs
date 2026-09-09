use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block_metadata::ValidatedSettledHistoryAdmission;
use models::rust::casper::protocol::casper_message::ValidatorBondGeneration;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;
use crate::allocation_probe::{interval, markers, measure, report};
use crate::rust::key_value_block_store::KeyValueBlockStore;

fn sign(mut block: BlockMessage, seed: u8) -> BlockMessage {
    let algorithm = Secp256k1;
    let key = PrivateKey::from_bytes(&[seed; 32]);
    block
        .body
        .state
        .bonds
        .sort_by(|a, b| a.validator.cmp(&b.validator));
    block
        .body
        .state
        .bond_generations
        .sort_by(|a, b| a.validator.cmp(&b.validator));
    block.sender = algorithm.to_public(&key).bytes;
    block.sig_algorithm = algorithm.name();
    block.block_hash = block.computed_block_hash();
    block.sig = algorithm.sign(&block.block_hash, &key.bytes).into();
    block
}

#[tokio::test]
async fn resource_probe_admission_summaries_do_not_retain_complete_block_bodies() {
    for count in [1u8, 8, 32] {
        let mut retained_summaries = None;
        for body_bytes in [0, 4096, 64 * 1024] {
            let mut manager = InMemoryStoreManager::new();
            let storage = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
            let block_store = KeyValueBlockStore::new(
                manager.store("resource-blocks".to_string()).await.unwrap(),
                manager
                    .store("resource-approved".to_string())
                    .await
                    .unwrap(),
            );
            let target_sender = sign(block(1, 1, vec![hash(201)]), 31).sender;
            let citer_sender = sign(block(2, 101, vec![hash(201)]), 32).sender;
            let mut anchor = block(200, 100, Vec::new());
            anchor.body.state.bonds = vec![
                Bond {
                    validator: target_sender.clone(),
                    stake: 100,
                },
                Bond {
                    validator: citer_sender.clone(),
                    stake: 100,
                },
            ];
            anchor.body.state.bond_generations = vec![
                ValidatorBondGeneration {
                    validator: target_sender,
                    generation: BondGeneration::GENESIS,
                },
                ValidatorBondGeneration {
                    validator: citer_sender,
                    generation: BondGeneration::GENESIS,
                },
            ];
            let anchor = sign(anchor, 31);
            let mut genesis = block(0, 0, Vec::new());
            genesis.body.state.bonds = anchor.body.state.bonds.clone();
            genesis.body.state.bond_generations = anchor.body.state.bond_generations.clone();
            let genesis = sign(genesis, 31);
            storage
                .insert(&genesis, InsertMode::ApprovedGenesis)
                .unwrap();
            block_store
                .put(genesis.block_hash.clone(), &genesis)
                .unwrap();
            block_store.put(anchor.block_hash.clone(), &anchor).unwrap();
            let mut previous_citer = anchor.block_hash.clone();
            for index in 1..=count {
                let mut target = block(index, i64::from(index), vec![hash(201)]);
                target.body.state.bonds = anchor.body.state.bonds.clone();
                target.body.state.bond_generations = anchor.body.state.bond_generations.clone();
                target.body.extra_bytes = Bytes::from(vec![index; body_bytes]);
                let target = sign(target, 31);
                let mut citer = block(index + 64, 100 + i64::from(index), vec![
                    previous_citer,
                    target.block_hash.clone(),
                ]);
                citer.body.state.bonds = anchor.body.state.bonds.clone();
                citer.body.state.bond_generations = anchor.body.state.bond_generations.clone();
                let citer = sign(citer, 32);
                previous_citer = citer.block_hash.clone();
                let proof = ValidatedSettledHistoryAdmission::new(
                    &target,
                    &anchor,
                    &citer,
                    BondGeneration::GENESIS,
                    100,
                )
                .unwrap();
                block_store.put(target.block_hash.clone(), &target).unwrap();
                block_store.put(citer.block_hash.clone(), &citer).unwrap();
                storage
                    .insert_settled_history_certified(&target, &proof)
                    .unwrap();
            }
            storage
                .validate_settled_history_admissions(&block_store, &anchor)
                .unwrap();
            let (validated, validation) = measure(|| {
                storage
                    .validate_settled_history_admissions(&block_store, &anchor)
                    .unwrap()
            });
            assert_eq!(validated, u64::from(count));
            report(
                "admission_validation",
                usize::from(count),
                body_bytes,
                validation,
            );
            storage.clear_settled_recovery_state_for_tests().unwrap();
            let (reconciled, total) = measure(|| {
                storage
                    .reconcile_settled_history_admissions(&block_store, &anchor)
                    .unwrap()
            });
            assert_eq!(reconciled, u64::from(count));
            let cuts = markers();
            let summaries = cuts[2].unwrap();
            let missing = cuts[3].unwrap();
            let prepared = cuts[0].unwrap();
            let published = cuts[1].unwrap();
            report(
                "admission_summaries",
                usize::from(count),
                body_bytes,
                summaries,
            );
            report(
                "admission_recovery_matching",
                usize::from(count),
                body_bytes,
                interval(summaries, missing),
            );
            report(
                "admission_migration_preparation",
                usize::from(count),
                body_bytes,
                interval(missing, prepared),
            );
            report(
                "admission_migration_publication",
                usize::from(count),
                body_bytes,
                interval(prepared, published),
            );
            report(
                "admission_reconciliation_total",
                usize::from(count),
                body_bytes,
                total,
            );
            if let Some(expected) = retained_summaries {
                assert_eq!(summaries.live_delta, expected);
            } else {
                retained_summaries = Some(summaries.live_delta);
            }
        }
    }
}

#[tokio::test]
async fn resource_probe_metadata_projection_is_measured_apart_from_the_ledger_audit() {
    for rising in [false, true] {
        for rounds in [2u8, 8, 64] {
            let (_, storage, genesis, first) = fixture().await;
            let mut parent = genesis;
            for revision in 1..=rounds {
                let next = if revision == 1 {
                    first.clone()
                } else {
                    let next = block(revision, i64::from(revision), vec![parent
                        .block_hash
                        .clone()]);
                    storage.insert(&next, InsertMode::Normal).unwrap();
                    next
                };
                if rising {
                    append_without_projection_at_ft(
                        &storage,
                        &next,
                        i64::from(rounds) + 1 + i64::from(revision),
                        2 * (i64::from(rounds) + 1),
                    );
                } else {
                    append_without_projection(&storage, &next);
                }
                parent = next;
            }
            let (_, projection) = measure(|| storage.reconcile_finalization_projection().unwrap());
            let mode = if rising { "rising_ft" } else { "fixed_ft" };
            report(
                &format!("metadata_projection_{mode}"),
                usize::from(rounds),
                0,
                projection,
            );
            let base = storage.capture_finalization_base().unwrap();
            assert_eq!(base.head.revision, u64::from(rounds));
            assert_eq!(base.dag.last_finalized_block(), parent.block_hash);
            let (_, repeated) = measure(|| storage.reconcile_finalization_projection().unwrap());
            report(
                &format!("metadata_projection_retry_{mode}"),
                usize::from(rounds),
                0,
                repeated,
            );
            assert_eq!(storage.capture_finalization_base().unwrap().head, base.head);
        }
    }
}
