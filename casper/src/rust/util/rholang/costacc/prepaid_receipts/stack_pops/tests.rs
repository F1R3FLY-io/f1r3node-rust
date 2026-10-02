use models::rhoapi::cost_signature::Value;
use models::rust::phlo_wire::PhloWireLimits;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;

const LIMITS: PrepaidStackPopLimits = PrepaidStackPopLimits {
    draws: 32,
    bucket: PrepaidReceiptBucketLimits {
        occurrences: 32,
        wire: PhloWireLimits {
            total_bytes: 8192,
            field_bytes: 4096,
        },
    },
    cells: PrepaidCellLimits {
        cells: 128,
        wire: PhloWireLimits {
            total_bytes: 4096,
            field_bytes: 1024,
        },
    },
    receipts: PrepaidReceiptLimits {
        entries: 32,
        value_bytes: 8192,
        batch_bytes: 65_536,
    },
};

fn signature(value: u8) -> CostSignature {
    CostSignature {
        value: Some(Value::Ground(vec![value])),
    }
}

async fn inventory(runtime: &RuntimeOps, head: u8) -> Vec<PurseStack> {
    supply::decode_purse_inventory(
        &runtime
            .runtime
            .reducer
            .space
            .get_data(&supply::supply_channel(&Sig::Ground(vec![head])))
            .await,
        &signature(head),
    )
    .unwrap()
    .stacks
}

async fn fixture() -> (
    RuntimeOps,
    RuntimeOps,
    Vec<PurseStack>,
    Vec<PrepaidStackPop>,
) {
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut native = RuntimeOps::new(play);
    let mut stacks = Vec::new();
    for (head, copies) in [(1u8, 2), (2, 1), (3, 1)] {
        let channel = supply::supply_channel(&Sig::Ground(vec![head]));
        for _ in 0..copies {
            native
                .runtime
                .reducer
                .space
                .produce(
                    channel.clone(),
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: vec![1],
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: (head..=3).map(signature).collect(),
                        }),
                    },
                    false,
                )
                .await
                .unwrap();
        }
        let current = inventory(&native, head).await;
        let originals = match head {
            1 => vec![vec![b"a0".as_slice(), b"a1", b"a2"], vec![
                b"b0".as_slice(),
                b"b1",
                b"b2",
            ]],
            2 => vec![vec![b"c1".as_slice(), b"c2"]],
            _ => vec![vec![b"d2".as_slice()]],
        };
        let encoded = originals
            .iter()
            .map(|cells| OrderedPrepaidCells::encode(cells, LIMITS.cells).unwrap())
            .collect::<Vec<_>>();
        let refs = encoded.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let bucket =
            PrepaidReceiptBucket::new(current[0].source_hash, &refs, LIMITS.bucket).unwrap();
        let bytes = bucket.encode(LIMITS.bucket).unwrap();
        native
            .replace_prepaid_receipts(
                &[PrepaidReceiptChange {
                    receipt_id: bucket.storage_key(),
                    expected: None,
                    replacement: Some(&bytes),
                }],
                LIMITS.receipts,
            )
            .await
            .unwrap();
        stacks.extend(current);
    }
    let draws = vec![
        PrepaidStackPop {
            stack_id: stacks[0].instance_id,
            receipt_index: 0,
            count: 1,
        },
        PrepaidStackPop {
            stack_id: stacks[1].instance_id,
            receipt_index: 1,
            count: 2,
        },
        PrepaidStackPop {
            stack_id: stacks[2].instance_id,
            receipt_index: 0,
            count: 1,
        },
    ];
    (native, RuntimeOps::new(replay), stacks, draws)
}

async fn records(runtime: &RuntimeOps, head: u8) -> Vec<Vec<Vec<u8>>> {
    let stacks = inventory(runtime, head).await;
    let bytes = runtime
        .read_prepaid_receipt(
            &PrepaidReceiptBucket::key_for_source(&stacks[0].source_hash),
            8192,
        )
        .await
        .unwrap()
        .unwrap();
    let bucket = PrepaidReceiptBucket::decode(&bytes, LIMITS.bucket).unwrap();
    bucket
        .check_occurrences(&stacks[0].source_hash, stacks.len())
        .unwrap();
    bucket
        .receipts()
        .iter()
        .map(|record| {
            OrderedPrepaidCells::decode(record, LIMITS.cells)
                .unwrap()
                .cells()
                .iter()
                .map(|cell| cell.to_vec())
                .collect()
        })
        .collect()
}

async fn prior_effect(runtime: &RuntimeOps) {
    runtime
        .runtime
        .reducer
        .space
        .produce(
            RhoByteArray::create_par(b"prior-application-effect".to_vec()),
            ListParWithRandom {
                pars: vec![RhoByteArray::create_par(b"retained".to_vec())],
                random_state: Vec::new(),
                cost_authority: None,
                cost_stack: None,
            },
            false,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn atomic_stack_receipt_migration_preserves_prefixes_destinations_and_replay() {
    let (mut native, mut replay, stacks, draws) = fixture().await;
    let base = native.runtime.create_checkpoint().await;
    let mut roots = Vec::new();
    for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
        2, 1, 0,
    ]] {
        native.runtime.reset(&base.root).await.unwrap();
        prior_effect(&native).await;
        let ordered = order.map(|index| draws[index]);
        let log = native
            .apply_prepaid_stack_pops(&stacks, &ordered, LIMITS)
            .await
            .unwrap();
        assert!(inventory(&native, 1).await.is_empty());
        assert!(native
            .read_prepaid_receipt(
                &PrepaidReceiptBucket::key_for_source(&stacks[0].source_hash),
                8192
            )
            .await
            .unwrap()
            .is_none());
        assert_eq!(records(&native, 2).await, vec![vec![
            b"a1".to_vec(),
            b"a2".to_vec()
        ]]);
        assert_eq!(records(&native, 3).await, vec![
            vec![b"b2".to_vec()],
            vec![b"c2".to_vec()],
            vec![b"d2".to_vec()]
        ]);
        let end = native.runtime.create_checkpoint().await;
        replay.runtime.reset(&base.root).await.unwrap();
        replay.runtime.rig(log).await.unwrap();
        prior_effect(&replay).await;
        replay
            .apply_prepaid_stack_pops(&stacks, &ordered, LIMITS)
            .await
            .unwrap();
        replay.runtime.check_replay_data().await.unwrap();
        assert_eq!(replay.runtime.create_checkpoint().await.root, end.root);
        roots.push(end.root);
    }
    assert!(roots.windows(2).all(|pair| pair[0] == pair[1]));
}

#[tokio::test]
async fn invalid_stack_receipt_selection_publishes_no_partial_transition() {
    let (mut native, _, stacks, draws) = fixture().await;
    let base = native.runtime.create_checkpoint().await;
    for mutation in 0..7 {
        let mut altered = draws.clone();
        let mut limits = LIMITS;
        match mutation {
            0 => altered[2].count = 0,
            1 => altered[2].count = u64::MAX,
            2 => altered[2].receipt_index = usize::MAX,
            3 => altered[1].receipt_index = 0,
            4 => altered.push(draws[0]),
            5 => limits.cells.cells = 3,
            _ => limits.receipts.entries = 1,
        }
        assert!(
            native
                .apply_prepaid_stack_pops(&stacks, &altered, limits)
                .await
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(native.runtime.create_checkpoint().await.root, base.root);
    }
}

#[tokio::test]
async fn receipt_replay_failure_after_physical_pops_restores_both_kinds_of_state() {
    let (mut native, mut replay, stacks, draws) = fixture().await;
    let base = native.runtime.create_checkpoint().await;
    let pops = draws
        .iter()
        .map(|draw| (draw.stack_id, draw.count))
        .collect();
    supply::apply_stack_pops(&mut native, &stacks, &pops)
        .await
        .unwrap();
    let physical_trace = native.runtime.take_event_log().await;
    replay.runtime.reset(&base.root).await.unwrap();
    replay.runtime.rig(physical_trace.clone()).await.unwrap();
    supply::apply_stack_pops(&mut replay, &stacks, &pops)
        .await
        .unwrap();
    replay.runtime.check_replay_data().await.unwrap();
    assert!(inventory(&replay, 1).await.is_empty());
    replay.runtime.reset(&base.root).await.unwrap();
    replay.runtime.rig(physical_trace).await.unwrap();
    let error = replay
        .apply_prepaid_stack_pops(&stacks, &draws, LIMITS)
        .await
        .unwrap_err();
    assert!(matches!(error, CasperError::RuntimeError(_)), "{error}");
    assert_eq!(inventory(&replay, 1).await, stacks[..2]);
    assert_eq!(records(&replay, 1).await, vec![
        vec![b"a0".to_vec(), b"a1".to_vec(), b"a2".to_vec()],
        vec![b"b0".to_vec(), b"b1".to_vec(), b"b2".to_vec()]
    ]);
    assert_eq!(records(&replay, 2).await, vec![vec![
        b"c1".to_vec(),
        b"c2".to_vec()
    ]]);
    assert_eq!(records(&replay, 3).await, vec![vec![b"d2".to_vec()]]);
    assert_eq!(replay.runtime.create_checkpoint().await.root, base.root);
}

#[tokio::test]
async fn complete_consumption_removes_receipts_and_new_tails_create_buckets() {
    let (mut native, _, stacks, draws) = fixture().await;
    let close = [
        PrepaidStackPop {
            stack_id: stacks[2].instance_id,
            receipt_index: 0,
            count: 2,
        },
        PrepaidStackPop {
            stack_id: stacks[3].instance_id,
            receipt_index: 0,
            count: 1,
        },
    ];
    native
        .apply_prepaid_stack_pops(&stacks[2..], &close, LIMITS)
        .await
        .unwrap();
    for stack in &stacks[2..] {
        assert!(native
            .read_prepaid_receipt(
                &PrepaidReceiptBucket::key_for_source(&stack.source_hash),
                8192
            )
            .await
            .unwrap()
            .is_none());
    }
    assert!(inventory(&native, 2).await.is_empty());
    assert!(inventory(&native, 3).await.is_empty());
    native
        .apply_prepaid_stack_pops(&stacks[..2], &draws[..2], LIMITS)
        .await
        .unwrap();
    assert_eq!(records(&native, 2).await, vec![vec![
        b"a1".to_vec(),
        b"a2".to_vec()
    ]]);
    assert_eq!(records(&native, 3).await, vec![vec![b"b2".to_vec()]]);
    assert!(native
        .apply_prepaid_stack_pops(&stacks[..2], &draws[..2], LIMITS)
        .await
        .is_err());
}

#[tokio::test]
async fn missing_or_malformed_provenance_never_becomes_synthetic_credit() {
    let (mut native, _, stacks, draws) = fixture().await;
    let base = native.runtime.create_checkpoint().await;
    let key = PrepaidReceiptBucket::key_for_source(&stacks[0].source_hash);
    let original = native
        .read_prepaid_receipt(&key, 8192)
        .await
        .unwrap()
        .unwrap();
    for mutation in 0..4 {
        native.runtime.reset(&base.root).await.unwrap();
        let bucket = PrepaidReceiptBucket::decode(&original, LIMITS.bucket).unwrap();
        let wrong_cells = OrderedPrepaidCells::encode(&[b"one-cell"], LIMITS.cells).unwrap();
        let replacement = match mutation {
            0 => None,
            1 => Some(
                PrepaidReceiptBucket::new(
                    stacks[0].source_hash,
                    &[&wrong_cells, bucket.receipts()[1]],
                    LIMITS.bucket,
                )
                .unwrap()
                .encode(LIMITS.bucket)
                .unwrap(),
            ),
            2 => Some(
                PrepaidReceiptBucket::new(
                    stacks[0].source_hash,
                    &bucket.receipts()[..1],
                    LIMITS.bucket,
                )
                .unwrap()
                .encode(LIMITS.bucket)
                .unwrap(),
            ),
            _ => Some(
                PrepaidReceiptBucket::new(
                    stacks[0].source_hash,
                    &[b"invalid-cell-record", bucket.receipts()[1]],
                    LIMITS.bucket,
                )
                .unwrap()
                .encode(LIMITS.bucket)
                .unwrap(),
            ),
        };
        native
            .replace_prepaid_receipts(
                &[PrepaidReceiptChange {
                    receipt_id: key,
                    expected: Some(&original),
                    replacement: replacement.as_deref(),
                }],
                LIMITS.receipts,
            )
            .await
            .unwrap();
        let malformed = native.runtime.create_checkpoint().await;
        assert!(native
            .apply_prepaid_stack_pops(&stacks, &draws, LIMITS)
            .await
            .is_err());
        assert_eq!(
            native.runtime.create_checkpoint().await.root,
            malformed.root
        );
    }
}
