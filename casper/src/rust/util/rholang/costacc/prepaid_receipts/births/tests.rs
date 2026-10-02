use std::num::NonZeroUsize;
use std::sync::Arc;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostSignatureCompound, CostStack, ListParWithRandom};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use models::rust::phlo_intent::{PhloFundingIntentLimits, PhloFundingIntentV1};
use models::rust::phlo_obligation::PhloObligationKeyLimits;
use models::rust::phlo_source::PhloSourceLimits;
use models::rust::phlo_wire::PhloWireLimits;
use proptest::prelude::*;
use rholang::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloFundingTerms,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_phlo_execution, check_phlo_funding_family, project_phlo_obligations, PhloCaptureLimits,
    PhloConsentLimits, PhloExecutionLimits, PhloExecutionWitness, PhloFundingCase,
    PhloFundingIntentBinding, PhloFundingLimits, PhloFundingSource, PhloOutcome, PhloResource,
    PhloResourceAmount, PhloSourceConsent, RetainedCellBackingLimits,
};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;
use crate::rust::util::rholang::costacc::prepaid_receipts::retained_records::{
    encode_retained_records, prepare_insertions,
};
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    NativePrepaidCell, NativePrepaidCellLimits, NativeRetainedRecordLimits, OrderedPrepaidCells,
    PrepaidCellLimits, PrepaidReceiptBucket, PrepaidReceiptBucketLimits, PrepaidReceiptChange,
    PrepaidReceiptLimits,
};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

async fn verify_live_birth_capture(
    wallets: usize,
    price: u64,
    cells: usize,
    reverse: bool,
    failures: bool,
) {
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (manager, _) = RuntimeManager::create_with_history(
        store,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    );
    let mut runtime = RuntimeOps::new(manager.spawn_runtime().await);
    let empty = runtime.runtime.create_checkpoint().await;
    let owners = (0..wallets)
        .map(|index| CostSignature {
            value: Some(Value::Ground((index as u64).to_be_bytes().to_vec())),
        })
        .collect::<Vec<_>>();
    let signature = if wallets == 1 {
        owners[0].clone()
    } else {
        CostSignature {
            value: Some(Value::Compound(CostSignatureCompound { elements: owners })),
        }
    };
    let authority = cost_signature_to_sig(&signature).unwrap();
    let channel = supply_channel(&authority);
    let datum = |random: u8| ListParWithRandom {
        pars: Vec::new(),
        random_state: vec![random],
        cost_authority: None,
        cost_stack: Some(CostStack {
            cells: vec![signature.clone(); cells],
        }),
    };
    for random in [1, 2, 3] {
        runtime
            .runtime
            .reducer
            .space
            .produce(channel.clone(), datum(random), false)
            .await
            .unwrap();
    }
    let checkpoint = runtime.runtime.create_checkpoint().await;
    let inventory =
        decode_purse_inventory(&runtime.get_data_datums(&channel).await, &signature).unwrap();
    let selected = inventory
        .stacks
        .iter()
        .filter(|stack| stack.random_state != [3])
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 2);
    let mut births = selected
        .iter()
        .map(|stack| AuthorityBornStack {
            stack_id: stack.instance_id,
            produce_hash: stack.source_hash,
            cells: stack.stack.cells.clone(),
        })
        .collect::<Vec<_>>();
    if reverse {
        births.reverse();
    }
    let cap = |n| NonZeroUsize::new(n).unwrap();
    let wire = PhloWireLimits {
        total_bytes: 2_097_152,
        field_bytes: 1_048_576,
    };
    let work = PhloExecutionLimits {
        resource_entries: 2,
        authority_nodes: 65_536,
        key_bytes: 1_048_576,
    };
    let controls_limit = PhloControlsLimits {
        wire,
        owners: 1,
        schedules: 1,
        total_classes: 1,
    };
    let intent_limits = PhloFundingIntentLimits {
        wire,
        controls: controls_limit,
        sources: wallets,
        resource_permissions: 2 * wallets,
        authority_nodes: 65_536,
    };
    let context = crate::rust::util::rholang::costacc::genesis_resource_policy::tests::retained_record_context();
    let mut schedule = context.genesis().record().schedule().unwrap();
    schedule.actual_price = price;
    let terms = schedule.encode(controls_limit.schedule(1)).unwrap();
    let resource = PhloResource {
        location: b"slot",
        class: 0,
        acquisition_terms: &terms,
        authority: &authority,
    };
    let permissions = [resource, PhloResource {
        location: b"another-slot",
        ..resource
    }];
    let retained = permissions.map(|resource| PhloResourceAmount {
        resource,
        quantity: cells as u64,
    });
    let cost = (cells * 2) as u64 * price * wallets as u64;
    let total = cost + 1;
    let keys = (0..wallets)
        .map(|i| {
            let mut key = [0; 32];
            key[24..].copy_from_slice(&(i as u64).to_be_bytes());
            key
        })
        .collect::<Vec<_>>();
    let record = PhloFundingIntentV1 {
        schedule_commitment: schedule.digest(controls_limit.schedule(1)).unwrap(),
        controls: PhloControlsV1 {
            limit: 0,
            price_ceiling: price,
            required_owner_ceilings: vec![price],
            permitted_schedules: vec![schedule],
        },
        total_exposure: u128::from(total),
        sources: keys
            .iter()
            .map(|key| {
                PhloSourceConsent {
                    custody: key,
                    hold_cap: total,
                    debit_cap: total,
                    fee_permitted: true,
                    resources: &permissions,
                }
                .wire_policy(work, PhloSourceLimits {
                    wire,
                    resource_permissions: 2,
                    authority_nodes: 65_536,
                })
                .unwrap()
            })
            .collect(),
    };
    let source_bytes = 12
        + record
            .sources
            .iter()
            .map(|source| {
                source
                    .encode(PhloSourceLimits {
                        wire,
                        resource_permissions: 2,
                        authority_nodes: 65_536,
                    })
                    .unwrap()
                    .len()
                    + 8
            })
            .sum::<usize>();
    assert!(source_bytes <= wire.field_bytes);
    if wallets >= 65 {
        assert!(source_bytes > 524_288);
    }
    let binding = PhloFundingIntentBinding::new(&record, intent_limits).unwrap();
    let view = binding.view().unwrap();
    let schedule = view.controls().permitted_schedules[0];
    let controls = check_phlo_controls(
        schedule.environment,
        0,
        u64::MAX,
        view.controls(),
        schedule,
        0,
    )
    .unwrap();
    let execution = check_phlo_execution(
        controls,
        PhloExecutionWitness {
            available: &[],
            required: &[],
            used: &[],
            unused: &[],
            fresh: &[],
        },
        work,
    )
    .unwrap()
    .with_retained_acquisitions(&retained, &budget())
    .unwrap();
    let obligations =
        project_phlo_obligations(execution, PhloOutcome::Accepted(&[]), cap(3)).unwrap();
    let sources = keys
        .iter()
        .map(|key| PhloFundingSource {
            custody: key,
            capacity: total,
            exposure_limit: total,
            debit_limit: total,
        })
        .collect::<Vec<_>>();
    let eligible = vec![vec![true; 3]; wallets];
    let assignment = (0..wallets)
        .map(|i| {
            obligations
                .amounts()
                .iter()
                .map(|amount| {
                    amount / wallets as u64 + u64::from((i as u64) < amount % wallets as u64)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let cases = [PhloFundingCase {
        obligations: &obligations,
        eligible: &eligible,
        assignment: &assignment,
    }];
    let family =
        check_phlo_funding_family(&sources, &cases, u128::from(total), PhloFundingLimits {
            sources: cap(wallets),
            cases: cap(1),
            obligations: cap(3),
            assignment_cells: 3 * wallets,
            custody_bytes: 32 * wallets,
        })
        .unwrap();
    let checked = view
        .check_family(
            &family,
            PhloFundingTerms {
                required_owner_ceilings: &record.controls.required_owner_ceilings,
                asset: b"REV",
                schedule_commitment: schedule.commitment,
            },
            PhloConsentLimits {
                sources: wallets,
                permission_entries: 2 * wallets,
                case_cells: 3 * wallets,
                authority_nodes: 65_536,
                key_bytes: 1_048_576,
            },
        )
        .unwrap();
    let capture = checked
        .capture_case(
            0,
            PhloCaptureLimits {
                funding: FundingSearchLimits {
                    source_cap: cap(wallets),
                    obligation_cap: cap(3),
                },
                key: PhloObligationKeyLimits {
                    wire,
                    authority_nodes: 65_536,
                },
                aggregate_key_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap();
    let columns = capture
        .obligations()
        .enumerate()
        .filter_map(|(index, column)| {
            matches!(column.key(), PhloObligationKey::RetainedResource(_)).then_some(index)
        })
        .collect::<Vec<_>>();
    let first_stack = births.iter().map(|birth| birth.stack_id).min().unwrap();
    let positions = births
        .iter()
        .map(|birth| {
            let side = usize::from(birth.stack_id != first_stack);
            (0..cells)
                .map(|cell| columns[(cell + side) % 2])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let claims = births
        .iter()
        .zip(&positions)
        .map(|(birth, positions)| RetainedBirthFunding {
            birth,
            obligation_positions: positions,
        })
        .collect::<Vec<_>>();
    let limits = NativeRetainedBirthLimits {
        funding: RetainedBirthFundingLimits {
            births: 2,
            cells: cells * 2,
            obligations: 3,
            authority_bytes: 1_048_576,
        },
        physical_cells: cells * 3,
        physical_bytes: 1_048_576,
    };
    let funding = capture
        .bind_retained_births(&claims, limits.funding, &budget())
        .unwrap();
    let actual = runtime
        .capture_retained_birth_stacks(&funding, limits, &budget())
        .await
        .unwrap();
    let mut expected = selected.into_iter().cloned().collect::<Vec<_>>();
    expected.sort_unstable_by_key(|stack| stack.instance_id);
    assert_eq!(actual, expected);
    let record_limits = NativeRetainedRecordLimits {
        backing: RetainedCellBackingLimits {
            cells: 2 * cells,
            contributions: 2 * wallets + 2 * cells,
        },
        cell: NativePrepaidCellLimits {
            wire,
            authority_nodes: 65_536,
            sources: wallets,
        },
        stack: PrepaidCellLimits { cells, wire },
        aggregate_bytes: 16_777_216,
    };
    let pre_state_root: [u8; 32] = empty.root.bytes().try_into().unwrap();
    let records = encode_retained_records(
        &context,
        &funding,
        pre_state_root,
        [7; 32],
        record_limits,
        &budget(),
    )
    .unwrap();
    assert_eq!(records.len(), 2);
    let mut totals = std::collections::BTreeMap::<([u8; 32], Vec<u8>), u64>::new();
    for (record, stack) in records.iter().zip(&expected) {
        assert_eq!(*record.stack_id(), stack.instance_id);
        assert_eq!(*record.source_hash(), stack.source_hash);
        let ordered =
            OrderedPrepaidCells::decode(record.encoded_cells(), record_limits.stack).unwrap();
        assert_eq!(ordered.cells().len(), cells);
        for (index, bytes) in ordered.cells().iter().enumerate() {
            let cell = NativePrepaidCell::decode(&context, bytes, record_limits.cell).unwrap();
            assert_eq!(
                cell.origin().genesis_root.as_slice(),
                context.genesis().genesis_root().as_ref()
            );
            assert_eq!(cell.origin().pre_state_root, pre_state_root);
            assert_eq!(cell.origin().deploy_id, [7; 32]);
            assert_eq!(cell.origin().birth_source, stack.source_hash);
            assert_eq!(cell.origin().cell_index, index as u64);
            let side = usize::from(stack.instance_id != first_stack);
            let column = capture
                .obligations()
                .nth(columns[(index + side) % 2])
                .unwrap();
            let PhloObligationKey::RetainedResource(expected_resource) = column.key() else {
                panic!("retained resource")
            };
            assert_eq!(cell.resource(), &expected_resource.wire_key(work).unwrap());
            assert_eq!(cell.resource().acquisition_terms, terms);
            assert_eq!(cell.resource().class, 0);
            assert_eq!(cell.acquisition_value(), price * wallets as u64);
            for row in cell.contributions() {
                *totals
                    .entry((row.custody, cell.resource().location.to_vec()))
                    .or_default() += row.amount;
            }
        }
    }
    for column in capture.obligations() {
        if let PhloObligationKey::RetainedResource(resource) = column.key() {
            for (source, amount) in column.contributions() {
                let key: [u8; 32] = source.source().custody.try_into().unwrap();
                assert_eq!(
                    totals
                        .get(&(key, resource.location.to_vec()))
                        .copied()
                        .unwrap_or(0),
                    amount
                );
            }
        }
    }
    let mut reversed = claims.clone();
    reversed.reverse();
    let reordered = capture
        .bind_retained_births(&reversed, limits.funding, &budget())
        .unwrap();
    assert_eq!(
        records,
        encode_retained_records(
            &context,
            &reordered,
            pre_state_root,
            [7; 32],
            record_limits,
            &budget()
        )
        .unwrap()
    );
    assert_eq!(
        runtime.runtime.create_checkpoint().await.root,
        checkpoint.root
    );
    if !failures {
        return;
    }
    let receipt_limits = PrepaidReceiptLimits {
        entries: 2,
        value_bytes: 1_048_576,
        batch_bytes: 4_194_304,
    };
    let bucket = PrepaidReceiptBucketLimits {
        occurrences: 1,
        wire,
    };
    let keys = records
        .iter()
        .map(|record| PrepaidReceiptBucket::key_for_source(record.source_hash()))
        .collect::<Vec<_>>();
    let absent = manager
        .capture_prepaid_receipts(pre_state_root, &keys, receipt_limits, &budget())
        .unwrap();
    let insertions = prepare_insertions(
        &records,
        pre_state_root,
        &absent,
        bucket,
        receipt_limits,
        &budget(),
    )
    .unwrap();
    assert_eq!(insertions.len(), 2);
    assert!(insertions.windows(2).all(|pair| pair[0].0 < pair[1].0));
    for (key, bytes) in &insertions {
        let parsed = PrepaidReceiptBucket::decode(bytes, bucket).unwrap();
        assert_eq!(*key, parsed.storage_key());
        let record = records
            .iter()
            .find(|record| record.source_hash() == parsed.source_hash())
            .unwrap();
        assert_eq!(parsed.receipts(), &[record.encoded_cells()]);
    }
    let unrequested = manager
        .capture_prepaid_receipts(pre_state_root, &[], receipt_limits, &budget())
        .unwrap();
    assert!(prepare_insertions(
        &records,
        pre_state_root,
        &unrequested,
        bucket,
        receipt_limits,
        &budget()
    )
    .is_err());
    let current_root: [u8; 32] = checkpoint.root.bytes().try_into().unwrap();
    let other_root = manager
        .capture_prepaid_receipts(current_root, &keys, receipt_limits, &budget())
        .unwrap();
    assert!(prepare_insertions(
        &records,
        pre_state_root,
        &other_root,
        bucket,
        receipt_limits,
        &budget()
    )
    .is_err());
    for bound in [
        PrepaidReceiptLimits {
            entries: 1,
            ..receipt_limits
        },
        PrepaidReceiptLimits {
            value_bytes: 1,
            ..receipt_limits
        },
        PrepaidReceiptLimits {
            batch_bytes: 1,
            ..receipt_limits
        },
    ] {
        assert!(
            prepare_insertions(&records, pre_state_root, &absent, bucket, bound, &budget())
                .is_err()
        );
    }
    let changes = insertions
        .iter()
        .map(|(key, bytes)| PrepaidReceiptChange {
            receipt_id: *key,
            expected: None,
            replacement: Some(bytes),
        })
        .collect::<Vec<_>>();
    runtime
        .replace_prepaid_receipts(&changes, receipt_limits)
        .await
        .unwrap();
    let populated = runtime.runtime.create_checkpoint().await;
    assert!(runtime
        .replace_prepaid_receipts(&changes, receipt_limits)
        .await
        .is_err());
    assert_eq!(
        runtime.runtime.create_checkpoint().await.root,
        populated.root
    );
    let populated_root: [u8; 32] = populated.root.bytes().try_into().unwrap();
    let present = manager
        .capture_prepaid_receipts(populated_root, &keys, receipt_limits, &budget())
        .unwrap();
    assert!(prepare_insertions(
        &records,
        populated_root,
        &present,
        bucket,
        receipt_limits,
        &budget()
    )
    .is_err());
    runtime.runtime.reset(&checkpoint.root).await.unwrap();
    for limited in [
        NativeRetainedRecordLimits {
            aggregate_bytes: 0,
            ..record_limits
        },
        NativeRetainedRecordLimits {
            backing: RetainedCellBackingLimits {
                cells: cells * 2 - 1,
                ..record_limits.backing
            },
            ..record_limits
        },
        NativeRetainedRecordLimits {
            stack: PrepaidCellLimits {
                cells: cells - 1,
                ..record_limits.stack
            },
            ..record_limits
        },
        NativeRetainedRecordLimits {
            cell: NativePrepaidCellLimits {
                wire: PhloWireLimits {
                    total_bytes: 0,
                    field_bytes: 0,
                },
                ..record_limits.cell
            },
            ..record_limits
        },
    ] {
        assert!(encode_retained_records(
            &context,
            &funding,
            pre_state_root,
            [7; 32],
            limited,
            &budget()
        )
        .is_err());
    }
    assert!(encode_retained_records(
        &context,
        &funding,
        pre_state_root,
        [7; 32],
        record_limits,
        &HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)))
    )
    .is_err());
    for limited in [
        NativeRetainedBirthLimits {
            physical_cells: cells * 3 - 1,
            ..limits
        },
        NativeRetainedBirthLimits {
            physical_bytes: 1,
            ..limits
        },
    ] {
        assert!(runtime
            .capture_retained_birth_stacks(&funding, limited, &budget())
            .await
            .is_err());
    }
    assert!(runtime
        .capture_retained_birth_stacks(
            &funding,
            limits,
            &HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)))
        )
        .await
        .is_err());
    for field in 0..2 {
        let mut altered = births.clone();
        if field == 0 {
            altered[0].stack_id[0] ^= 0xff;
        } else {
            altered[0].produce_hash[0] ^= 0xff;
        }
        let claims = altered
            .iter()
            .zip(&positions)
            .map(|(birth, positions)| RetainedBirthFunding {
                birth,
                obligation_positions: positions,
            })
            .collect::<Vec<_>>();
        let checked = capture
            .bind_retained_births(&claims, limits.funding, &budget())
            .unwrap();
        assert!(runtime
            .capture_retained_birth_stacks(&checked, limits, &budget())
            .await
            .is_err());
    }
    runtime.runtime.reset(&empty.root).await.unwrap();
    assert!(runtime
        .capture_retained_birth_stacks(&funding, limits, &budget())
        .await
        .is_err());
    assert_eq!(runtime.runtime.create_checkpoint().await.root, empty.root);
    runtime.runtime.reset(&checkpoint.root).await.unwrap();
    runtime
        .runtime
        .reducer
        .space
        .produce(channel.clone(), datum(1), false)
        .await
        .unwrap();
    let duplicate = runtime.runtime.create_checkpoint().await;
    assert!(runtime
        .capture_retained_birth_stacks(
            &funding,
            NativeRetainedBirthLimits {
                physical_cells: cells * 4,
                ..limits
            },
            &budget()
        )
        .await
        .is_err());
    assert_eq!(
        runtime.runtime.create_checkpoint().await.root,
        duplicate.root
    );
}

#[tokio::test]
async fn retained_birth_capture_binds_live_cells_and_rejects_missing_or_duplicate_sources() {
    for wallets in [1, 3, 64, 65] {
        for price in [0, 7] {
            verify_live_birth_capture(wallets, price, 3, true, true).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_retained_birth_captures_preserve_each_runtime_state() {
    tokio::try_join!(
        tokio::spawn(verify_live_birth_capture(3, 0, 3, false, true)),
        tokio::spawn(verify_live_birth_capture(65, 7, 5, true, true)),
    )
    .unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn retained_birth_capture_preserves_state_and_physical_identity(
        wallets in 1usize..76, price in 0u64..100, cells in 1usize..17, reverse in any::<bool>(),
    ) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
            .block_on(verify_live_birth_capture(wallets, price, cells, reverse, false));
    }
}
