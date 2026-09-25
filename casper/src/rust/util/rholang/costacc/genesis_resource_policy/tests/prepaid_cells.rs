use models::rust::phlo_resource::{PhloAuthorityNode, PhloResourceKeyV1, PhloResourceLimits};
use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};

use super::*;
mod physical_capture;
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    NativePrepaidCell, NativePrepaidCellLimits, NativePrepaidContribution, NativePrepaidOrigin,
    OrderedPrepaidCells, PrepaidCellLimits,
};

const LIMITS: NativePrepaidCellLimits = NativePrepaidCellLimits {
    wire: PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    },
    authority_nodes: 4096,
    sources: 4096,
};

fn origin() -> NativePrepaidOrigin {
    NativePrepaidOrigin {
        genesis_root: [9; 32],
        pre_state_root: [1; 32],
        deploy_id: [2; 32],
        birth_source: [3; 32],
        cell_index: 17,
    }
}

fn resource(context: &AdoptedResourcePolicy, price: u64, leaves: usize, class: u32) -> Vec<u8> {
    let mut schedule = context.genesis().record().schedule().unwrap();
    schedule.actual_price = price;
    let terms = schedule.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let authority = if leaves == 0 {
        vec![PhloAuthorityNode::Unit]
    } else {
        std::iter::repeat_n(PhloAuthorityNode::And, leaves - 1)
            .chain(std::iter::repeat_n(
                PhloAuthorityNode::Ground(b"original-owner"),
                leaves,
            ))
            .collect()
    };
    PhloResourceKeyV1 {
        location: b"original-location",
        class,
        acquisition_terms: &terms,
        authority,
    }
    .encode(PhloResourceLimits {
        wire: LIMITS.wire,
        authority_nodes: LIMITS.authority_nodes,
    })
    .unwrap()
}

fn row(key: u64, amount: u64) -> NativePrepaidContribution {
    let mut custody = [0; 32];
    custody[24..].copy_from_slice(&key.to_be_bytes());
    NativePrepaidContribution { custody, amount }
}

fn raw(original: NativePrepaidOrigin, key: &[u8], rows: &[NativePrepaidContribution]) -> Vec<u8> {
    let mut wire = PhloWireEncoder::new(LIMITS.wire);
    wire.bytes(b"f1r3node:native-prepaid-cell:v1").unwrap();
    for id in [
        original.genesis_root,
        original.pre_state_root,
        original.deploy_id,
        original.birth_source,
    ] {
        wire.bytes(&id).unwrap();
    }
    wire.u64(original.cell_index).unwrap();
    wire.bytes(key).unwrap();
    wire.u64(rows.len() as u64).unwrap();
    for contribution in rows {
        wire.bytes(&contribution.custody).unwrap();
        wire.u64(contribution.amount).unwrap();
    }
    wire.into_bytes()
}

fn equal_shares(value: u64, count: usize) -> Vec<NativePrepaidContribution> {
    (0..count)
        .map(|i| {
            row(
                i as u64,
                value / count as u64 + u64::from((i as u64) < value % count as u64),
            )
        })
        .collect()
}

#[test]
fn native_cell_round_trip_preserves_complete_origin_resource_and_many_wallets() {
    let context = acquisition_context();
    let key = resource(&context, 12_000, 1, 0);
    for count in [1, 2, 3, 65, 1000] {
        let sources = equal_shares(12_000, count);
        let encoded =
            NativePrepaidCell::encode(&context, origin(), &key, &sources, LIMITS).unwrap();
        assert_eq!(encoded, raw(origin(), &key, &sources));
        let cell = NativePrepaidCell::decode(&context, &encoded, LIMITS).unwrap();
        assert_eq!(cell.origin(), origin());
        assert_eq!(cell.resource_bytes(), key);
        assert_eq!(cell.contributions(), sources);
        assert_eq!(cell.acquisition_value(), 12_000);
        assert_eq!(cell.resource().location, b"original-location");
        assert_eq!(cell.resource().class, 0);
        assert_eq!(cell.resource().authority, vec![PhloAuthorityNode::Ground(
            b"original-owner"
        )]);
    }
    let key = resource(&context, 7, 65, 1);
    let rows = equal_shares(65 * 7 * 7, 3);
    let encoded = NativePrepaidCell::encode(&context, origin(), &key, &rows, LIMITS).unwrap();
    assert_eq!(
        NativePrepaidCell::decode(&context, &encoded, LIMITS)
            .unwrap()
            .acquisition_value(),
        65 * 7 * 7
    );
}

#[test]
fn native_cell_rejects_bad_contributions_before_exposing_a_record() {
    let context = acquisition_context();
    let key = resource(&context, 12, 1, 0);
    for sources in [
        vec![],
        vec![row(0, 11)],
        vec![row(0, 13)],
        vec![row(0, 0), row(1, 12)],
        vec![row(1, 6), row(1, 6)],
        vec![row(2, 6), row(1, 6)],
        vec![row(0, u64::MAX), row(1, 13)],
    ] {
        assert!(NativePrepaidCell::encode(&context, origin(), &key, &sources, LIMITS).is_err());
        assert!(
            NativePrepaidCell::decode(&context, &raw(origin(), &key, &sources), LIMITS).is_err()
        );
    }
}

#[test]
fn native_cell_rejects_wrong_genesis_class_policy_and_machine_overflow() {
    let context = acquisition_context();
    let mut another = origin();
    another.genesis_root[0] ^= 1;
    let key = resource(&context, 10, 1, 0);
    let rows = [row(0, 10)];
    assert!(NativePrepaidCell::decode(&context, &raw(another, &key, &rows), LIMITS).is_err());
    let unknown = resource(&context, 10, 1, 2);
    assert!(NativePrepaidCell::decode(&context, &raw(origin(), &unknown, &rows), LIMITS).is_err());
    let other = policy(10, 6, "other")
        .adopt(&CasperShardConf {
            min_phlo_price: 10,
            casper_version: 6,
            shard_name: "other".into(),
            ..CasperShardConf::new()
        })
        .unwrap();
    assert!(NativePrepaidCell::decode(&other, &raw(origin(), &key, &rows), LIMITS).is_err());
    for (price, leaves, class) in [(u64::MAX, 1, 0), (i64::MAX as u64, 2, 0), (u64::MAX, 2, 1)] {
        let key = resource(&context, price, leaves, class);
        assert!(NativePrepaidCell::decode(&context, &raw(origin(), &key, &[]), LIMITS).is_err());
    }
    let key = resource(&context, i64::MAX as u64, 1, 0);
    let encoded = raw(origin(), &key, &[row(0, i64::MAX as u64)]);
    assert_eq!(
        NativePrepaidCell::decode(&context, &encoded, LIMITS)
            .unwrap()
            .acquisition_value(),
        i64::MAX as u64
    );
    let mut wide = policy(0, 6, "root");
    let mut descriptor = wide.record.schedule().unwrap();
    descriptor.classes[0].weight = u64::MAX;
    wide.record = PhloGenesisPolicy::from_schedule(&descriptor).unwrap();
    let wide = wide
        .adopt(&CasperShardConf {
            min_phlo_price: 0,
            casper_version: 6,
            shard_name: "root".into(),
            ..CasperShardConf::new()
        })
        .unwrap();
    let key = resource(&wide, 0, 2, 0);
    assert!(NativePrepaidCell::decode(&wide, &raw(origin(), &key, &[]), LIMITS).is_err());
}

#[test]
fn native_cell_zero_value_requires_no_positive_contributions() {
    let context = acquisition_context();
    for (price, leaves) in [(0, 3), (u64::MAX, 0)] {
        let key = resource(&context, price, leaves, 0);
        let bytes = NativePrepaidCell::encode(&context, origin(), &key, &[], LIMITS).unwrap();
        assert_eq!(
            NativePrepaidCell::decode(&context, &bytes, LIMITS)
                .unwrap()
                .acquisition_value(),
            0
        );
        assert!(
            NativePrepaidCell::decode(&context, &raw(origin(), &key, &[row(0, 1)]), LIMITS)
                .is_err()
        );
    }
}

#[test]
fn native_cell_aggregate_is_not_limited_to_one_native_purse() {
    let context = acquisition_context();
    let key = resource(&context, u64::MAX, 1, 0);
    let rows = [row(0, i64::MAX as u64), row(1, i64::MAX as u64), row(2, 1)];
    let bytes = NativePrepaidCell::encode(&context, origin(), &key, &rows, LIMITS).unwrap();
    let record = NativePrepaidCell::decode(&context, &bytes, LIMITS).unwrap();
    assert_eq!(record.acquisition_value(), u64::MAX);
    assert_eq!(record.contributions(), rows);
    let excessive_purse = [row(0, i64::MAX as u64 + 1), row(1, i64::MAX as u64)];
    assert!(
        NativePrepaidCell::decode(&context, &raw(origin(), &key, &excessive_purse), LIMITS)
            .is_err()
    );
    assert!(NativePrepaidCell::encode(&context, origin(), &key, &excessive_purse, LIMITS).is_err());
}

#[tokio::test]
async fn native_cell_economics_survive_physical_tail_migration_and_replay() {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::{CostSignature, CostStack, ListParWithRandom};
    use rholang::rust::interpreter::accounting::Sig;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rholang::rust::interpreter::test_utils::resources::create_runtimes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use crate::rust::rholang::runtime::RuntimeOps;
    use crate::rust::util::rholang::costacc::prepaid_receipts::{
        PrepaidReceiptBucket, PrepaidReceiptBucketLimits, PrepaidReceiptChange,
        PrepaidReceiptLimits, PrepaidStackPop, PrepaidStackPopLimits,
    };
    use crate::rust::util::rholang::supply::{decode_purse_inventory, supply_channel};

    let context = acquisition_context();
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut play = RuntimeOps::new(play);
    let mut replay = RuntimeOps::new(replay);
    let signature = CostSignature {
        value: Some(Value::Ground(b"original-owner".to_vec())),
    };
    let channel = supply_channel(&Sig::Ground(b"original-owner".to_vec()));
    play.runtime
        .reducer
        .space
        .produce(
            channel.clone(),
            ListParWithRandom {
                pars: Vec::new(),
                random_state: vec![1],
                cost_authority: None,
                cost_stack: Some(CostStack {
                    cells: vec![signature.clone(); 3],
                }),
            },
            false,
        )
        .await
        .unwrap();
    let stacks = decode_purse_inventory(
        &play.runtime.reducer.space.get_data(&channel).await,
        &signature,
    )
    .unwrap()
    .stacks;
    let cell_limits = PrepaidCellLimits {
        cells: 16,
        wire: LIMITS.wire,
    };
    let bucket_limits = PrepaidReceiptBucketLimits {
        occurrences: 16,
        wire: LIMITS.wire,
    };
    let receipt_limits = PrepaidReceiptLimits {
        entries: 16,
        value_bytes: 1_048_576,
        batch_bytes: 4_194_304,
    };
    let records = (0..3)
        .map(|index| {
            let price = [12, 300, 7000][index];
            let key = resource(&context, price, 1, 0);
            NativePrepaidCell::encode(
                &context,
                NativePrepaidOrigin {
                    birth_source: stacks[0].source_hash,
                    cell_index: index as u64,
                    ..origin()
                },
                &key,
                &equal_shares(price, [3, 65, 1000][index]),
                LIMITS,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let refs = records.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let ordered = OrderedPrepaidCells::encode(&refs, cell_limits).unwrap();
    let source =
        PrepaidReceiptBucket::new(stacks[0].source_hash, &[&ordered], bucket_limits).unwrap();
    let source_bytes = source.encode(bucket_limits).unwrap();
    play.replace_prepaid_receipts(
        &[PrepaidReceiptChange {
            receipt_id: source.storage_key(),
            expected: None,
            replacement: Some(&source_bytes),
        }],
        receipt_limits,
    )
    .await
    .unwrap();
    let base = play.runtime.create_checkpoint().await;
    let limits = PrepaidStackPopLimits {
        draws: 16,
        bucket: bucket_limits,
        cells: cell_limits,
        receipts: receipt_limits,
    };
    let draws = [PrepaidStackPop {
        stack_id: stacks[0].instance_id,
        receipt_index: 0,
        count: 1,
    }];
    let log = play
        .apply_prepaid_stack_pops(&stacks, &draws, limits)
        .await
        .unwrap();
    let tail = decode_purse_inventory(
        &play.runtime.reducer.space.get_data(&channel).await,
        &signature,
    )
    .unwrap()
    .stacks;
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].stack.cells.len(), 2);
    assert_ne!(tail[0].source_hash, stacks[0].source_hash);
    let bytes = play
        .read_prepaid_receipt(
            &PrepaidReceiptBucket::key_for_source(&tail[0].source_hash),
            receipt_limits.value_bytes,
        )
        .await
        .unwrap()
        .unwrap();
    let bucket = PrepaidReceiptBucket::decode(&bytes, bucket_limits).unwrap();
    bucket.check_occurrences(&tail[0].source_hash, 1).unwrap();
    let survivor = OrderedPrepaidCells::decode(bucket.receipts()[0], cell_limits).unwrap();
    assert_eq!(survivor.cells(), &refs[1..]);
    let mut sum = 0;
    for (index, bytes) in survivor.cells().iter().enumerate() {
        let cell = NativePrepaidCell::decode(&context, bytes, LIMITS).unwrap();
        assert_eq!(cell.origin().birth_source, stacks[0].source_hash);
        assert_eq!(cell.origin().cell_index, index as u64 + 1);
        assert_eq!(cell.contributions().len(), [65, 1000][index]);
        sum += cell.acquisition_value();
    }
    assert_eq!(sum, 7300);
    let end = play.runtime.create_checkpoint().await;
    replay.runtime.reset(&base.root).await.unwrap();
    replay.runtime.rig(log).await.unwrap();
    replay
        .apply_prepaid_stack_pops(&stacks, &draws, limits)
        .await
        .unwrap();
    replay.runtime.check_replay_data().await.unwrap();
    assert_eq!(replay.runtime.create_checkpoint().await.root, end.root);
}

#[test]
fn native_cell_rejects_malformed_wire_and_all_resource_bounds() {
    let context = acquisition_context();
    let key = resource(&context, 12, 3, 0);
    let rows = [row(0, 12), row(1, 12), row(2, 12)];
    let bytes = raw(origin(), &key, &rows);
    for end in 0..bytes.len() {
        assert!(
            NativePrepaidCell::decode(&context, &bytes[..end], LIMITS).is_err(),
            "truncation {end}"
        );
    }
    let mut input = PhloWireDecoder::new(&bytes, LIMITS.wire).unwrap();
    for _ in 0..5 {
        input.bytes().unwrap();
    }
    input.u64().unwrap();
    input.bytes().unwrap();
    let count_offset = input.position();
    let mut cases = Vec::new();
    let mut wrong_domain = bytes.clone();
    wrong_domain[8] ^= 1;
    cases.push(wrong_domain);
    let mut trailing = bytes.clone();
    trailing.push(0);
    cases.push(trailing);
    let mut enormous = bytes.clone();
    enormous[count_offset..count_offset + 8].fill(255);
    cases.push(enormous);
    let mut bad_width = bytes.clone();
    bad_width[count_offset + 8..count_offset + 16].copy_from_slice(&31u64.to_be_bytes());
    cases.push(bad_width);
    for candidate in cases {
        assert!(NativePrepaidCell::decode(&context, &candidate, LIMITS).is_err());
    }
    for limit in [
        NativePrepaidCellLimits {
            sources: 2,
            ..LIMITS
        },
        NativePrepaidCellLimits {
            authority_nodes: 4,
            ..LIMITS
        },
        NativePrepaidCellLimits {
            wire: PhloWireLimits {
                total_bytes: bytes.len() - 1,
                ..LIMITS.wire
            },
            ..LIMITS
        },
        NativePrepaidCellLimits {
            wire: PhloWireLimits {
                field_bytes: key.len() - 1,
                ..LIMITS.wire
            },
            ..LIMITS
        },
    ] {
        assert!(NativePrepaidCell::encode(&context, origin(), &key, &rows, limit).is_err());
        assert!(NativePrepaidCell::decode(&context, &bytes, limit).is_err());
    }
    let exact = NativePrepaidCellLimits {
        sources: 3,
        authority_nodes: 5,
        wire: PhloWireLimits {
            total_bytes: bytes.len(),
            field_bytes: key.len(),
        },
    };
    assert_eq!(
        NativePrepaidCell::encode(&context, origin(), &key, &rows, exact).unwrap(),
        bytes
    );
    assert!(NativePrepaidCell::decode(&context, &bytes, exact).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn native_cell_arbitrary_contributions_refine_exact_backing(
        price in any::<u64>(), leaves in 0usize..20, class in 0u32..3,
        sources in prop::collection::vec((any::<u8>(), any::<u64>()), 0..12),
        wrong_genesis in any::<bool>(),
    ) {
        let context = acquisition_context();
        let key = resource(&context, price, leaves, class);
        let rows = sources.iter().map(|(id, amount)| row(u64::from(*id), *amount)).collect::<Vec<_>>();
        let mut original = origin();
        if wrong_genesis { original.genesis_root[0] ^= 1; }
        let value = u128::from(price) * leaves as u128 * if class == 1 { 7 } else { 1 };
        let sum = sources.iter().map(|(_, amount)| u128::from(*amount)).sum::<u128>();
        let valid = !wrong_genesis && class < 2 && value <= u64::MAX as u128 && sum == value
            && sources.iter().all(|(_, amount)| *amount > 0 && *amount <= i64::MAX as u64)
            && sources.windows(2).all(|pair| pair[0].0 < pair[1].0);
        let bytes = raw(original, &key, &rows);
        prop_assert_eq!(NativePrepaidCell::decode(&context, &bytes, LIMITS).is_ok(), valid);
        prop_assert_eq!(NativePrepaidCell::encode(&context, original, &key, &rows, LIMITS).is_ok(), valid);
    }

    #[test]
    fn native_cell_cohorts_preserve_original_price_and_all_contributions(
        price in 100u64..100_000, leaves in 1usize..30, count in 1usize..100,
        original_index in any::<u64>(), root in any::<[u8;32]>(),
    ) {
        let context = acquisition_context();
        let key = resource(&context, price, leaves, 1);
        let value = price * leaves as u64 * 7;
        let rows = equal_shares(value, count);
        let original = NativePrepaidOrigin { cell_index: original_index, pre_state_root: root, ..origin() };
        let bytes = NativePrepaidCell::encode(&context, original, &key, &rows, LIMITS).unwrap();
        let cell = NativePrepaidCell::decode(&context, &bytes, LIMITS).unwrap();
        prop_assert_eq!(cell.origin(), original);
        prop_assert_eq!(cell.contributions(), rows.as_slice());
        prop_assert_eq!(cell.acquisition_value(), value);
        prop_assert_eq!(cell.resource_bytes(), key.as_slice());
        prop_assert_eq!(context.check_acquisition_terms(cell.resource().acquisition_terms).unwrap().schedule().actual_price, price);
    }

    #[test]
    fn native_cell_consumption_histories_conserve_original_backing(
        prices in prop::collection::vec(1u64..1000, 1..24),
        pops in prop::collection::vec(0usize..25, 0..30),
    ) {
        let context = acquisition_context();
        let mut bytes = Vec::new();
        for (index, price) in prices.iter().copied().enumerate() {
            let key = resource(&context, price, 1, 0);
            bytes.push(NativePrepaidCell::encode(&context,
                NativePrepaidOrigin { cell_index: index as u64, ..origin() },
                &key, &[row(0, price)], LIMITS).unwrap());
        }
        let limits = PrepaidCellLimits { cells: 24, wire: LIMITS.wire };
        let original = prices.iter().sum::<u64>();
        let mut remaining = bytes.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let mut consumed = 0u64;
        let mut consumed_cells = 0usize;
        for count in pops {
            if remaining.is_empty() { break; }
            let encoded = OrderedPrepaidCells::encode(&remaining, limits).unwrap();
            let stack = OrderedPrepaidCells::decode(&encoded, limits).unwrap();
            if let Ok((used, _)) = stack.split_consumed(count) {
                for record in used {
                    let cell = NativePrepaidCell::decode(&context, record, LIMITS).unwrap();
                    prop_assert_eq!(cell.origin().cell_index, consumed_cells as u64);
                    consumed += cell.acquisition_value();
                    consumed_cells += 1;
                }
                remaining.drain(..count);
            }
            let retained = remaining.iter().map(|record| NativePrepaidCell::decode(&context, record, LIMITS).unwrap().acquisition_value()).sum::<u64>();
            prop_assert_eq!(consumed + retained, original);
            prop_assert_eq!(consumed_cells + remaining.len(), prices.len());
        }
    }
}
