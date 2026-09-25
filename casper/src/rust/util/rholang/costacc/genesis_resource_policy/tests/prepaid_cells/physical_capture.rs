use std::sync::Arc;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostStack, ListParWithRandom};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    PrepaidReceiptBucket, PrepaidReceiptBucketLimits, PrepaidReceiptChange, PrepaidReceiptLimits,
    PrepaidStackCaptureLimits,
};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::supply::{decode_purse_inventory, supply_channel, PurseStack};

mod demand;

fn limits() -> PrepaidStackCaptureLimits {
    PrepaidStackCaptureLimits {
        stacks: 128,
        physical_cells: 1024,
        physical_bytes: 1_048_576,
        bucket: PrepaidReceiptBucketLimits {
            occurrences: 128,
            wire: LIMITS.wire,
        },
        cells: PrepaidCellLimits {
            cells: 1024,
            wire: LIMITS.wire,
        },
        native_cell: LIMITS,
        receipts: PrepaidReceiptLimits {
            entries: 128,
            value_bytes: 1_048_576,
            batch_bytes: 2_097_152,
        },
    }
}

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

#[tokio::test]
async fn generated_prepaid_consumption_binds_exact_rooted_prefixes() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};
    use rholang::rust::interpreter::accounting::phlo_execution::{
        PhloExecutionLimits, PhloResource, PhloResourceAmount,
    };

    use crate::rust::util::rholang::costacc::prepaid_receipts::{
        NativePrepaidConsumption, NativePrepaidInventoryLimits, PrepaidStackPop,
        PrepaidStackPopLimits,
    };

    let context = acquisition_context();
    let (manager, mut runtime, stacks) = fixture(3, 2).await;
    let values = records(stacks[0].source_hash, 3, 2, 65);
    let root = store(
        &mut runtime,
        stacks[0].source_hash,
        stacks[0].source_hash,
        &values,
    )
    .await;
    let captured = manager
        .capture_prepaid_stacks(root, &stacks, &context, limits(), &budget())
        .unwrap();
    let inventory_limits = NativePrepaidInventoryLimits {
        cells: limits().cells,
        cell: LIMITS,
        execution: PhloExecutionLimits {
            resource_entries: 8,
            authority_nodes: 4096,
            key_bytes: 1_048_576,
        },
    };
    let inventory = captured
        .resource_inventory(inventory_limits, &budget())
        .unwrap();
    assert_eq!(inventory.root(), root);
    assert!(inventory.records_for_stack(&[99; 32]).is_none());
    for case in 0..5 {
        let mut bad = inventory_limits;
        match case {
            0 => bad.cells.cells = 5,
            1 => bad.execution.resource_entries = 5,
            2 => bad.execution.authority_nodes = 5,
            3 => bad.execution.key_bytes = 1,
            _ => bad.cells.wire.total_bytes = 1,
        }
        assert!(
            captured.resource_inventory(bad, &budget()).is_err(),
            "case {case}"
        );
    }
    for dimension in [
        models::rust::host_work::HostWorkDimension::SearchStateBytes,
        models::rust::host_work::HostWorkDimension::VerificationOperations,
        models::rust::host_work::HostWorkDimension::VerificationBytes,
    ] {
        let mut caps = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
        caps.set(dimension, HostWorkLimit::new(0));
        let work = HostWorkBudget::new(caps);
        assert!(captured
            .resource_inventory(inventory_limits, &work)
            .is_err());
        assert!(work.is_rejected());
    }
    let encoded: Vec<_> = (0..2)
        .map(|i| resource(&context, 10_000 + i, 1, 0))
        .collect();
    let keys: Vec<_> = encoded
        .iter()
        .map(|bytes| {
            PhloResourceKeyV1::decode(bytes, PhloResourceLimits {
                wire: LIMITS.wire,
                authority_nodes: LIMITS.authority_nodes,
            })
            .unwrap()
        })
        .collect();
    let authority = Sig::Ground(b"original-owner".to_vec());
    let resources: Vec<_> = keys
        .iter()
        .map(|key| PhloResource {
            location: key.location,
            class: key.class as usize,
            acquisition_terms: key.acquisition_terms,
            authority: &authority,
        })
        .collect();
    for stack in &stacks {
        let records = inventory.records_for_stack(&stack.instance_id).unwrap();
        assert_eq!(records.len(), 3);
        for record in records {
            assert_eq!(record.len(), 2);
            for (index, cell) in record.iter().enumerate() {
                assert_eq!(cell.resource(), resources[index]);
                assert_eq!(cell.receipt().contributions().len(), 65);
                assert_eq!(
                    cell.receipt().origin().genesis_root.as_slice(),
                    context.genesis().genesis_root().as_ref()
                );
                assert_ne!(
                    cell.resource().authority,
                    &rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(
                        &stack.stack.cells[index]
                    )
                    .unwrap()
                );
            }
        }
    }
    let first = PrepaidStackPop {
        stack_id: stacks[0].instance_id,
        receipt_index: 0,
        count: 1,
    };
    for draws in [
        vec![PrepaidStackPop {
            stack_id: [99; 32],
            ..first
        }],
        vec![PrepaidStackPop {
            receipt_index: 3,
            ..first
        }],
        vec![PrepaidStackPop { count: 0, ..first }],
        vec![PrepaidStackPop { count: 3, ..first }],
        vec![first, first],
        vec![first, PrepaidStackPop {
            stack_id: stacks[1].instance_id,
            ..first
        }],
    ] {
        assert!(inventory.prefixes(&draws, 3, &budget()).is_err());
    }
    assert!(inventory.prefixes(&[first], 0, &budget()).is_err());
    assert!(inventory.prefixes(&[], 0, &budget()).unwrap().is_empty());
    let mut runner = TestRunner::new(Config {
        cases: 128,
        source_file: Some(file!()),
        ..Config::default()
    });
    runner
        .run(
            &(
                proptest::collection::vec(0_u64..3, 3),
                any::<bool>(),
                0_u8..7,
            ),
            |(counts, reverse, mutation)| {
                let mut draws: Vec<_> = stacks
                    .iter()
                    .zip(&counts)
                    .enumerate()
                    .filter_map(|(index, (stack, count))| {
                        (*count > 0).then_some(PrepaidStackPop {
                            stack_id: stack.instance_id,
                            receipt_index: index,
                            count: *count,
                        })
                    })
                    .collect();
                let mut used: Vec<_> = resources
                    .iter()
                    .enumerate()
                    .filter_map(|(index, resource)| {
                        let quantity =
                            counts.iter().filter(|count| **count > index as u64).count() as u64;
                        (quantity > 0).then_some(PhloResourceAmount {
                            resource: *resource,
                            quantity,
                        })
                    })
                    .collect();
                if reverse {
                    draws.reverse();
                    used.reverse();
                }
                let prefixes = inventory.prefixes(&draws, 3, &budget()).unwrap();
                assert_eq!(prefixes.len(), draws.len());
                for (prefix, draw) in prefixes.iter().zip(&draws) {
                    prop_assert_eq!(prefix.stack().instance_id, draw.stack_id);
                    prop_assert_eq!(prefix.receipt_index(), draw.receipt_index);
                    prop_assert_eq!(prefix.resources().len(), draw.count as usize);
                    prop_assert_eq!(
                        prefix.current_authorities(),
                        &prefix.stack().stack.cells[..draw.count as usize]
                    );
                    for (index, cell) in prefix.resources().iter().enumerate() {
                        prop_assert_eq!(cell.resource(), resources[index]);
                    }
                }
                let mut valid = true;
                match mutation {
                    1 if !used.is_empty() => {
                        used[0].quantity += 1;
                        valid = false;
                    }
                    2 if !used.is_empty() => {
                        used[0].resource.location = b"changed-location";
                        valid = false;
                    }
                    3 if !used.is_empty() => {
                        used[0].resource.class = 1;
                        valid = false;
                    }
                    4 if !draws.is_empty() => {
                        draws.push(draws[0]);
                        valid = false;
                    }
                    5 if !draws.is_empty() => {
                        draws[0].receipt_index = 3;
                        valid = false;
                    }
                    6 if !used.is_empty() => {
                        used[0].resource.acquisition_terms = resources[usize::from(
                            used[0].resource.acquisition_terms == resources[0].acquisition_terms,
                        )]
                        .acquisition_terms;
                        valid = false;
                    }
                    _ => {}
                }
                let consumption = NativePrepaidConsumption {
                    captured: &captured,
                    draws: &draws,
                    limits: PrepaidStackPopLimits {
                        draws: 4,
                        bucket: limits().bucket,
                        cells: limits().cells,
                        receipts: limits().receipts,
                    },
                    execution: PhloExecutionLimits {
                        resource_entries: 8,
                        authority_nodes: 4096,
                        key_bytes: 1_048_576,
                    },
                    cell: LIMITS,
                    physical_cells: limits().physical_cells,
                    physical_bytes: limits().physical_bytes,
                };
                prop_assert_eq!(consumption.check_resources(used, &budget()).is_ok(), valid);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(runtime.runtime.create_checkpoint().await.root.bytes(), root);
}

async fn fixture(
    occurrences: usize,
    cells: usize,
) -> (Arc<RuntimeManager>, RuntimeOps, Vec<PurseStack>) {
    fixture_with_authority(occurrences, cells, CostSignature {
        value: Some(Value::Ground(b"current-owner".to_vec())),
    })
    .await
}

async fn fixture_with_authority(
    occurrences: usize,
    cells: usize,
    head: CostSignature,
) -> (Arc<RuntimeManager>, RuntimeOps, Vec<PurseStack>) {
    let stores = RSpaceStore {
        history: Arc::new(InMemoryKeyValueStore::new()),
        roots: Arc::new(InMemoryKeyValueStore::new()),
        cold: Arc::new(InMemoryKeyValueStore::new()),
    };
    let (manager, _) = RuntimeManager::create_with_history(
        stores,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    );
    let runtime = RuntimeOps::new(manager.spawn_runtime().await);
    let channel = supply_channel(
        &rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(&head).unwrap(),
    );
    for _ in 0..occurrences {
        runtime
            .runtime
            .reducer
            .space
            .produce(
                channel.clone(),
                ListParWithRandom {
                    pars: Vec::new(),
                    random_state: vec![7],
                    cost_authority: None,
                    cost_stack: Some(CostStack {
                        cells: vec![head.clone(); cells],
                    }),
                },
                false,
            )
            .await
            .unwrap();
    }
    let stacks = decode_purse_inventory(
        &runtime.runtime.reducer.space.get_data(&channel).await,
        &head,
    )
    .unwrap()
    .stacks;
    (Arc::new(manager), runtime, stacks)
}

fn records(source: [u8; 32], occurrences: usize, cells: usize, wallets: usize) -> Vec<Vec<u8>> {
    let context = acquisition_context();
    (0..occurrences)
        .map(|occurrence| {
            let values = (0..cells)
                .map(|index| {
                    let price = 10_000 + index as u64;
                    let key = resource(&context, price, 1, 0);
                    NativePrepaidCell::encode(
                        &context,
                        NativePrepaidOrigin {
                            birth_source: source,
                            cell_index: index as u64,
                            deploy_id: [occurrence as u8; 32],
                            ..origin()
                        },
                        &key,
                        &equal_shares(price, wallets),
                        LIMITS,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            OrderedPrepaidCells::encode(
                &values.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                limits().cells,
            )
            .unwrap()
        })
        .collect()
}

async fn store(
    runtime: &mut RuntimeOps,
    source: [u8; 32],
    claimed_source: [u8; 32],
    values: &[Vec<u8>],
) -> [u8; 32] {
    let bucket = PrepaidReceiptBucket::new(
        claimed_source,
        &values.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        limits().bucket,
    )
    .unwrap();
    let bytes = bucket.encode(limits().bucket).unwrap();
    runtime
        .replace_prepaid_receipts(
            &[PrepaidReceiptChange {
                receipt_id: PrepaidReceiptBucket::key_for_source(&source),
                expected: None,
                replacement: Some(&bytes),
            }],
            limits().receipts,
        )
        .await
        .unwrap();
    runtime
        .runtime
        .create_checkpoint()
        .await
        .root
        .bytes()
        .try_into()
        .unwrap()
}

#[tokio::test]
async fn physical_capture_preserves_all_occurrences_original_terms_and_many_wallets() {
    let context = acquisition_context();
    let (manager, mut runtime, stacks) = fixture(3, 2).await;
    let values = records(stacks[0].source_hash, 3, 2, 65);
    let root = store(
        &mut runtime,
        stacks[0].source_hash,
        stacks[0].source_hash,
        &values,
    )
    .await;
    let captured = manager
        .capture_prepaid_stacks(root, &stacks[..1], &context, limits(), &budget())
        .unwrap();
    assert_eq!(captured.root(), root);
    assert!(std::ptr::eq(captured.policy(), &context));
    assert_eq!(captured.stacks(), &[&stacks[0]]);
    let bucket = captured
        .records_for_stack(&root, &stacks[0].instance_id)
        .unwrap()
        .unwrap();
    assert_eq!(bucket.receipts().len(), 3);
    for record in bucket.receipts() {
        let cells = OrderedPrepaidCells::decode(record, limits().cells).unwrap();
        for (index, bytes) in cells.cells().iter().enumerate() {
            let cell = NativePrepaidCell::decode(&context, bytes, LIMITS).unwrap();
            assert_eq!(cell.contributions().len(), 65);
            assert_eq!(cell.acquisition_value(), 10_000 + index as u64);
            assert_eq!(cell.resource().location, b"original-location");
            assert_eq!(cell.resource().authority, vec![PhloAuthorityNode::Ground(
                b"original-owner"
            )]);
        }
    }
    assert!(captured
        .records_for_stack(&root, &stacks[1].instance_id)
        .unwrap()
        .is_none());
    assert!(captured.records_for_stack(&[0; 32], &[0; 32]).is_err());
    let mut reverse = stacks.clone();
    reverse.reverse();
    let first = manager
        .capture_prepaid_stacks(root, &stacks, &context, limits(), &budget())
        .unwrap();
    let second = manager
        .capture_prepaid_stacks(root, &reverse, &context, limits(), &budget())
        .unwrap();
    assert_eq!(first.stacks(), second.stacks());
    assert_eq!(runtime.runtime.create_checkpoint().await.root.bytes(), root);
}

#[tokio::test]
async fn physical_capture_rejects_missing_and_stale_physical_evidence() {
    let context = acquisition_context();
    let (manager, mut runtime, stacks) = fixture(2, 2).await;
    let before = runtime.runtime.create_checkpoint().await;
    let old_root = before.root.bytes().try_into().unwrap();
    assert!(manager
        .capture_prepaid_stacks(old_root, &stacks, &context, limits(), &budget())
        .is_err());
    let root = store(
        &mut runtime,
        stacks[0].source_hash,
        stacks[0].source_hash,
        &records(stacks[0].source_hash, 2, 2, 3),
    )
    .await;
    for mutation in 0..7 {
        let mut candidate = stacks.clone();
        let last = candidate.last_mut().unwrap();
        match mutation {
            0 => last.source_hash[0] ^= 1,
            1 => last.instance_id[0] ^= 1,
            2 => last.datum_index += 1,
            3 => last.random_state.push(9),
            4 => last.persistent = !last.persistent,
            5 => last.stack.cells.pop().map(|_| ()).unwrap(),
            _ => candidate[1] = candidate[0].clone(),
        }
        assert!(
            manager
                .capture_prepaid_stacks(root, &candidate, &context, limits(), &budget())
                .is_err(),
            "mutation {mutation}"
        );
    }
    assert!(manager
        .capture_prepaid_stacks(old_root, &stacks, &context, limits(), &budget())
        .is_err());
    assert!(manager
        .capture_prepaid_stacks([0; 32], &[], &context, limits(), &budget())
        .is_err());
    assert!(manager
        .capture_prepaid_stacks(root, &[], &context, limits(), &budget())
        .unwrap()
        .stacks()
        .is_empty());
    assert_eq!(runtime.runtime.create_checkpoint().await.root.bytes(), root);
}

#[tokio::test]
async fn physical_capture_checks_complete_bucket_not_only_selected_occurrences() {
    let context = acquisition_context();
    let (manager, mut runtime, stacks) = fixture(3, 2).await;
    let base = runtime.runtime.create_checkpoint().await;
    let source = stacks[0].source_hash;
    for mutation in 0..6 {
        runtime.runtime.reset(&base.root).await.unwrap();
        let mut values = records(source, 3, 2, 1);
        let mut claimed = source;
        match mutation {
            0 => {
                values.pop();
            }
            1 => values.push(values[0].clone()),
            2 => values[2] = records(source, 1, 1, 1).remove(0),
            3 => claimed[0] ^= 1,
            4 => {
                let cells = OrderedPrepaidCells::decode(&values[2], limits().cells).unwrap();
                let mut last = cells.cells()[1].to_vec();
                *last.last_mut().unwrap() ^= 1;
                values[2] = OrderedPrepaidCells::encode(&[cells.cells()[0], &last], limits().cells)
                    .unwrap();
            }
            _ => values[2] = b"not ordered cell records".to_vec(),
        }
        let root = store(&mut runtime, source, claimed, &values).await;
        assert!(
            manager
                .capture_prepaid_stacks(root, &stacks[..1], &context, limits(), &budget())
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(runtime.runtime.create_checkpoint().await.root.bytes(), root);
    }
}

#[tokio::test]
async fn physical_capture_enforces_inventory_receipt_and_work_limits() {
    let context = acquisition_context();
    let (manager, mut runtime, stacks) = fixture(3, 2).await;
    let source = stacks[0].source_hash;
    let root = store(&mut runtime, source, source, &records(source, 3, 2, 3)).await;
    let defaults = limits();
    let mut cases = vec![
        PrepaidStackCaptureLimits {
            stacks: 2,
            ..defaults
        },
        PrepaidStackCaptureLimits {
            physical_cells: 5,
            ..defaults
        },
        PrepaidStackCaptureLimits {
            physical_bytes: 0,
            ..defaults
        },
        PrepaidStackCaptureLimits {
            bucket: PrepaidReceiptBucketLimits {
                occurrences: 2,
                ..defaults.bucket
            },
            ..defaults
        },
        PrepaidStackCaptureLimits {
            cells: PrepaidCellLimits {
                cells: 1,
                ..defaults.cells
            },
            ..defaults
        },
        PrepaidStackCaptureLimits {
            native_cell: NativePrepaidCellLimits {
                sources: 2,
                ..LIMITS
            },
            ..defaults
        },
    ];
    let mut no_receipts = defaults;
    no_receipts.receipts.entries = 0;
    cases.push(no_receipts);
    for limited in cases {
        assert!(
            manager
                .capture_prepaid_stacks(root, &stacks[..1], &context, limited, &budget())
                .is_err(),
            "{limited:?}"
        );
    }
    let zero = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(manager
        .capture_prepaid_stacks(root, &stacks, &context, defaults, &zero)
        .is_err());
    assert!(manager
        .capture_prepaid_stacks(
            root,
            &stacks[..1],
            &context,
            PrepaidStackCaptureLimits {
                stacks: 3,
                physical_cells: 6,
                ..defaults
            },
            &budget()
        )
        .is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn physical_capture_refines_complete_rooted_bucket_model(
        occurrences in 1usize..6, cells in 1usize..5, wallets in 1usize..10,
        missing in any::<bool>(), truncated in any::<bool>(), wrong_source in any::<bool>(),
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let context = acquisition_context();
            let (manager, mut runtime, stacks) = fixture(occurrences, cells).await;
            let source = stacks[0].source_hash;
            let count = occurrences - usize::from(missing);
            let mut values = records(source, count, cells, wallets);
            if truncated && !values.is_empty() {
                values[0] = b"invalid-cell-record".to_vec();
            }
            let mut claimed = source;
            if wrong_source { claimed[0] ^= 1; }
            let root = store(&mut runtime, source, claimed, &values).await;
            let result = manager.capture_prepaid_stacks(root, &stacks[..1], &context, limits(), &budget());
            assert_eq!(result.is_ok(), !missing && !truncated && !wrong_source);
            assert_eq!(runtime.runtime.create_checkpoint().await.root.bytes(), root);
        });
    }
}
