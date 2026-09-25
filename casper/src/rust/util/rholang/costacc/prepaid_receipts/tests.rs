use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use proptest::prelude::*;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;

const LIMITS: PrepaidReceiptLimits = PrepaidReceiptLimits {
    entries: 32,
    value_bytes: 1024,
    batch_bytes: 8192,
};

async fn runtime() -> RuntimeOps {
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    RuntimeOps::new(
        create_runtime_from_kv_store(
            store,
            Arc::new(HashMap::new()),
            false,
            &mut Vec::new(),
            Arc::new(Box::new(Matcher)),
            ExternalServices::noop(),
        )
        .await,
    )
}

fn change<'a>(
    id: u8,
    expected: Option<&'a [u8]>,
    replacement: Option<&'a [u8]>,
) -> PrepaidReceiptChange<'a> {
    PrepaidReceiptChange {
        receipt_id: [id; 32],
        expected,
        replacement,
    }
}

#[tokio::test]
async fn duplicate_stack_consumption_preserves_the_surviving_prepaid_provenance() {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::{CostSignature, CostStack};
    use rholang::rust::interpreter::accounting::Sig;

    use crate::rust::util::rholang::supply::{decode_purse_inventory, supply_channel};

    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut native = RuntimeOps::new(play);
    let mut replay = RuntimeOps::new(replay);
    let head = CostSignature {
        value: Some(Value::Ground(b"slot".to_vec())),
    };
    let channel = supply_channel(&Sig::Ground(b"slot".to_vec()));
    let datum = ListParWithRandom {
        pars: Vec::new(),
        random_state: vec![1],
        cost_authority: None,
        cost_stack: Some(CostStack {
            cells: vec![head.clone()],
        }),
    };
    for _ in 0..2 {
        native
            .runtime
            .reducer
            .space
            .produce(channel.clone(), datum.clone(), false)
            .await
            .unwrap();
    }
    let before = decode_purse_inventory(
        &native.runtime.reducer.space.get_data(&channel).await,
        &head,
    )
    .unwrap();
    assert_eq!(before.stacks.len(), 2);
    assert_eq!(before.stacks[0].source_hash, before.stacks[1].source_hash);
    let bucket_limits = PrepaidReceiptBucketLimits {
        occurrences: 16,
        wire: models::rust::phlo_wire::PhloWireLimits {
            total_bytes: 1024,
            field_bytes: 512,
        },
    };
    let mut bucket = PrepaidReceiptBucket::new(
        before.stacks[0].source_hash,
        &[b"first acquisition", b"second acquisition"],
        bucket_limits,
    )
    .unwrap();
    bucket
        .check_occurrences(&before.stacks[0].source_hash, before.stacks.len())
        .unwrap();
    let original = bucket.encode(bucket_limits).unwrap();
    let edits = [PrepaidReceiptChange {
        receipt_id: bucket.storage_key(),
        expected: None,
        replacement: Some(&original),
    }];
    native
        .replace_prepaid_receipts(&edits, LIMITS)
        .await
        .unwrap();
    let initial = native.runtime.create_checkpoint().await;
    native
        .runtime
        .reducer
        .space
        .remove_data_at_recorded(
            &channel,
            before.stacks[0].datum_index,
            &before.stacks[0].instance_id,
        )
        .await
        .unwrap();
    assert_eq!(bucket.remove(0).unwrap(), b"first acquisition");
    let replacement = bucket.encode(bucket_limits).unwrap();
    let update = [PrepaidReceiptChange {
        receipt_id: bucket.storage_key(),
        expected: Some(&original),
        replacement: Some(&replacement),
    }];
    let log = native
        .replace_prepaid_receipts(&update, LIMITS)
        .await
        .unwrap();
    let after = decode_purse_inventory(
        &native.runtime.reducer.space.get_data(&channel).await,
        &head,
    )
    .unwrap();
    assert_eq!(after.stacks.len(), 1);
    assert_eq!(after.stacks[0].source_hash, before.stacks[1].source_hash);
    assert_ne!(after.stacks[0].instance_id, before.stacks[1].instance_id);
    let stored = native
        .read_prepaid_receipt(
            &PrepaidReceiptBucket::key_for_source(&after.stacks[0].source_hash),
            LIMITS.value_bytes,
        )
        .await
        .unwrap()
        .unwrap();
    let surviving = PrepaidReceiptBucket::decode(&stored, bucket_limits).unwrap();
    surviving
        .check_occurrences(&after.stacks[0].source_hash, after.stacks.len())
        .unwrap();
    assert_eq!(surviving.receipts(), &[b"second acquisition".as_slice()]);
    let retained = native.runtime.create_checkpoint().await;
    replay.runtime.reset(&initial.root).await.unwrap();
    replay.runtime.rig(log).await.unwrap();
    let replay_inventory = decode_purse_inventory(
        &replay.runtime.reducer.space.get_data(&channel).await,
        &head,
    )
    .unwrap();
    replay
        .runtime
        .reducer
        .space
        .remove_data_at_recorded(
            &channel,
            replay_inventory.stacks[0].datum_index,
            &replay_inventory.stacks[0].instance_id,
        )
        .await
        .unwrap();
    replay
        .replace_prepaid_receipts(&update, LIMITS)
        .await
        .unwrap();
    replay.runtime.check_replay_data().await.unwrap();
    assert_eq!(replay.runtime.create_checkpoint().await.root, retained.root);
}

#[tokio::test]
async fn receipt_updates_preserve_exact_bytes_and_follow_checkpoint_reset() {
    let mut runtime = runtime().await;
    let original = [0, 1, 0, 255];
    assert_eq!(
        runtime.read_prepaid_receipt(&[1; 32], 1024).await.unwrap(),
        None
    );
    runtime
        .replace_prepaid_receipts(&[change(1, None, Some(&original))], LIMITS)
        .await
        .unwrap();
    let before = runtime.runtime.create_checkpoint().await;
    runtime
        .replace_prepaid_receipts(&[change(1, Some(&original), Some(b"new terms"))], LIMITS)
        .await
        .unwrap();
    assert_eq!(
        runtime.read_prepaid_receipt(&[1; 32], 1024).await.unwrap(),
        Some(b"new terms".to_vec())
    );
    assert!(runtime
        .replace_prepaid_receipts(&[change(1, Some(&original), None)], LIMITS)
        .await
        .is_err());
    runtime
        .replace_prepaid_receipts(&[change(1, Some(b"new terms"), None)], LIMITS)
        .await
        .unwrap();
    assert_eq!(
        runtime.read_prepaid_receipt(&[1; 32], 1024).await.unwrap(),
        None
    );
    runtime.runtime.reset(&before.root).await.unwrap();
    assert_eq!(
        runtime.read_prepaid_receipt(&[1; 32], 1024).await.unwrap(),
        Some(original.to_vec())
    );
}

#[tokio::test]
async fn stale_late_entry_and_invalid_batches_leave_all_receipts_unchanged() {
    let mut runtime = runtime().await;
    runtime
        .replace_prepaid_receipts(
            &[change(1, None, Some(b"one")), change(2, None, Some(b"two"))],
            LIMITS,
        )
        .await
        .unwrap();
    let before = runtime.runtime.create_checkpoint().await;
    let invalid_batches = [
        vec![
            change(1, Some(b"one"), None),
            change(2, Some(b"stale"), None),
        ],
        vec![
            change(1, Some(b"one"), None),
            change(1, Some(b"one"), Some(b"other")),
        ],
        vec![change(1, Some(b"one"), Some(&[]))],
        vec![change(1, Some(b"one"), Some(b"one"))],
        vec![change(3, None, None)],
    ];
    for batch in invalid_batches {
        assert!(runtime
            .replace_prepaid_receipts(&batch, LIMITS)
            .await
            .is_err());
        assert_eq!(runtime.runtime.create_checkpoint().await.root, before.root);
    }
    for limits in [
        PrepaidReceiptLimits {
            entries: 0,
            ..LIMITS
        },
        PrepaidReceiptLimits {
            value_bytes: 2,
            ..LIMITS
        },
        PrepaidReceiptLimits {
            batch_bytes: 1,
            ..LIMITS
        },
    ] {
        assert!(runtime
            .replace_prepaid_receipts(&[change(1, Some(b"one"), None)], limits)
            .await
            .is_err());
        assert_eq!(runtime.runtime.create_checkpoint().await.root, before.root);
    }
}

#[tokio::test]
async fn noncanonical_or_duplicate_stored_values_cannot_supply_receipts() {
    let mut runtime = runtime().await;
    let empty = runtime.runtime.create_checkpoint().await;
    for mutation in 0..7 {
        runtime.runtime.reset(&empty.root).await.unwrap();
        let mut value = receipt_datum(b"original");
        match mutation {
            0 => value.pars.clear(),
            1 => value.pars.push(Par::default()),
            2 => value.random_state.push(1),
            3 => value.cost_authority = Some(Default::default()),
            4 => {
                value.pars[0].unforgeables = new_gsys_auth_token_par(Vec::new(), false).unforgeables
            }
            _ => (),
        }
        runtime
            .runtime
            .reducer
            .space
            .produce(receipt_channel(&[1; 32]), value.clone(), mutation == 5)
            .await
            .unwrap();
        if mutation == 6 {
            runtime
                .runtime
                .reducer
                .space
                .produce(receipt_channel(&[1; 32]), value, false)
                .await
                .unwrap();
        }
        assert!(
            runtime.read_prepaid_receipt(&[1; 32], 1024).await.is_err(),
            "mutation {mutation}"
        );
    }
}

#[tokio::test]
async fn receipt_replay_matches_full_root_and_partial_replay_failure_restores_state() {
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut play = RuntimeOps::new(play);
    let mut replay = RuntimeOps::new(replay);
    let initial = [change(1, None, Some(b"one")), change(2, None, Some(b"two"))];
    play.replace_prepaid_receipts(&initial, LIMITS)
        .await
        .unwrap();
    let before = play.runtime.create_checkpoint().await;
    let edits = [
        change(1, Some(b"one"), None),
        change(2, Some(b"two"), Some(b"revised")),
    ];
    let log = play.replace_prepaid_receipts(&edits, LIMITS).await.unwrap();
    let after = play.runtime.create_checkpoint().await;
    replay.runtime.reset(&before.root).await.unwrap();
    let first_removal = log.iter().take(2).cloned().collect::<Vec<_>>();
    replay.runtime.rig(first_removal.clone()).await.unwrap();
    replay
        .replace_prepaid_receipts(&edits[..1], LIMITS)
        .await
        .unwrap();
    replay.runtime.check_replay_data().await.unwrap();
    assert_eq!(
        replay.read_prepaid_receipt(&[1; 32], 1024).await.unwrap(),
        None
    );
    replay.runtime.reset(&before.root).await.unwrap();
    replay.runtime.rig(first_removal).await.unwrap();
    let error = replay
        .replace_prepaid_receipts(&edits, LIMITS)
        .await
        .unwrap_err();
    assert!(matches!(error, CasperError::RuntimeError(_)), "{error}");
    assert_eq!(
        replay.read_prepaid_receipt(&[1; 32], 1024).await.unwrap(),
        Some(b"one".to_vec())
    );
    assert_eq!(
        replay.read_prepaid_receipt(&[2; 32], 1024).await.unwrap(),
        Some(b"two".to_vec())
    );
    replay.runtime.reset(&before.root).await.unwrap();
    replay.runtime.rig(log.clone()).await.unwrap();
    let replay_log = replay
        .replace_prepaid_receipts(&edits, LIMITS)
        .await
        .unwrap();
    assert_eq!(log.len(), 5);
    assert_eq!(replay_log, log[..4]);
    replay.runtime.check_replay_data().await.unwrap();
    assert_eq!(replay.runtime.create_checkpoint().await.root, after.root);
}

#[tokio::test]
async fn receipt_success_returns_prior_events_and_complete_trace_replays() {
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
    let mut play = RuntimeOps::new(play);
    let mut replay = RuntimeOps::new(replay);
    let before = play.runtime.create_checkpoint().await;
    let channel = RhoByteArray::create_par(b"ordinary channel".to_vec());
    let value = receipt_datum(b"ordinary data");
    play.runtime
        .reducer
        .space
        .produce(channel.clone(), value.clone(), false)
        .await
        .unwrap();
    assert!(play
        .replace_prepaid_receipts(&[], LIMITS)
        .await
        .unwrap()
        .is_empty());
    let log = play
        .replace_prepaid_receipts(&[change(1, None, Some(b"receipt"))], LIMITS)
        .await
        .unwrap();
    assert_eq!(log.len(), 2);
    let after = play.runtime.create_checkpoint().await;
    assert!(after.log.is_empty());
    replay.runtime.reset(&before.root).await.unwrap();
    replay.runtime.rig(log.clone()).await.unwrap();
    replay
        .runtime
        .reducer
        .space
        .produce(channel, value, false)
        .await
        .unwrap();
    let replay_log = replay
        .replace_prepaid_receipts(&[change(1, None, Some(b"receipt"))], LIMITS)
        .await
        .unwrap();
    assert!(replay_log.is_empty());
    replay.runtime.check_replay_data().await.unwrap();
    assert_eq!(replay.runtime.create_checkpoint().await.root, after.root);
}

#[tokio::test]
async fn disjoint_receipts_converge_in_opposite_orders_on_independent_runtimes() {
    let mut left = runtime().await;
    let mut right = runtime().await;
    let first = [change(1, None, Some(b"one"))];
    let second = [change(2, None, Some(b"two"))];
    let (a, b) = tokio::join!(
        async {
            left.replace_prepaid_receipts(&first, LIMITS).await.unwrap();
            left.replace_prepaid_receipts(&second, LIMITS)
                .await
                .unwrap();
            left.runtime.create_checkpoint().await.root
        },
        async {
            right
                .replace_prepaid_receipts(&second, LIMITS)
                .await
                .unwrap();
            right
                .replace_prepaid_receipts(&first, LIMITS)
                .await
                .unwrap();
            right.runtime.create_checkpoint().await.root
        },
    );
    assert_eq!(a, b);
}

#[tokio::test]
async fn receipt_channels_require_system_authority_and_transition_hashes_bind_each_field() {
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::errors::InterpreterError;
    let mut native = runtime().await;
    for uri in ["sys:casper:authToken", "sys:test:authToken:make"] {
        let result = native
            .runtime
            .evaluate_with_phlo(
                &format!("new token(`{uri}`) in {{ token!(Nil) }}"),
                Cost::create(100_000, "test"),
            )
            .await
            .unwrap();
        assert!(result.errors.iter().any(|error| matches!(error,
            InterpreterError::BugFoundError(message) if message.contains(&format!("No value set for {uri}."))
        )), "{:?}", result.errors);
    }
    let channel = receipt_channel(&[1; 32]);
    let plain = RhoList::create_par(vec![
        RhoByteArray::create_par(b"sys:casper:authToken".to_vec()),
        RhoByteArray::create_par(RECEIPT_DOMAIN.to_vec()),
        RhoByteArray::create_par(vec![1; 32]),
    ]);
    assert_ne!(channel, plain);
    let base = replacement_id(change(1, Some(b"old"), Some(b"new")));
    for other in [
        change(2, Some(b"old"), Some(b"new")),
        change(1, Some(b"other"), Some(b"new")),
        change(1, Some(b"old"), Some(b"other")),
        change(1, None, Some(b"new")),
        change(1, Some(b"old"), None),
    ] {
        assert_ne!(base, replacement_id(other));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn batch_preflight_and_permutations_match_independent_reference(
        replacements in prop::collection::btree_map(0_u8..24, prop::option::of(2_u8..9), 0..16),
        stale in any::<bool>(),
    ) {
        let executor = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        executor.block_on(async {
            let mut left = runtime().await;
            let mut right = runtime().await;
            let initial: Vec<_> = replacements.keys().map(|id| change(*id, None, Some(b"original"))).collect();
            left.replace_prepaid_receipts(&initial, LIMITS).await.unwrap();
            right.replace_prepaid_receipts(&initial, LIMITS).await.unwrap();
            let before = left.runtime.create_checkpoint().await.root;
            prop_assert_eq!(right.runtime.create_checkpoint().await.root, before.clone());
            let values: Vec<_> = replacements.iter().map(|(id, value)| (*id, value.map(|byte| [byte]))).collect();
            let changes: Vec<_> = values.iter().enumerate().map(|(index, (id, value))| change(
                *id,
                Some(if stale && index == values.len() - 1 { b"stale" } else { b"original" }),
                value.as_ref().map(|bytes| bytes.as_slice()),
            )).collect();
            let mut reversed = changes.clone();
            reversed.reverse();
            let first = left.replace_prepaid_receipts(&changes, LIMITS).await;
            let second = right.replace_prepaid_receipts(&reversed, LIMITS).await;
            let accepted = !stale || changes.is_empty();
            prop_assert_eq!(first.is_ok(), accepted);
            prop_assert_eq!(second.is_ok(), accepted);
            if accepted {
                prop_assert_eq!(first.unwrap(), second.unwrap());
            }
            let after = left.runtime.create_checkpoint().await.root;
            prop_assert_eq!(right.runtime.create_checkpoint().await.root, after.clone());
            if !accepted {
                prop_assert_eq!(after, before);
            }
            for (id, replacement) in replacements {
                let expected = if accepted { replacement.map(|byte| vec![byte]) } else { Some(b"original".to_vec()) };
                prop_assert_eq!(left.read_prepaid_receipt(&[id; 32], 1024).await.unwrap(), expected);
            }
            Ok(())
        })?;
    }

    #[test]
    fn arbitrary_receipt_histories_match_independent_compare_and_replace_model(
        operations in prop::collection::vec((0_u8..4, prop::option::of(1_u8..8), prop::option::of(1_u8..8)), 0..24),
    ) {
        let executor = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        executor.block_on(async {
            let mut native = runtime().await;
            let mut reference = BTreeMap::new();
            for (id, expected, replacement) in operations {
                let old = expected.map(|value| [value]);
                let new = replacement.map(|value| [value]);
                let row = change(id, old.as_ref().map(|bytes| bytes.as_slice()), new.as_ref().map(|bytes| bytes.as_slice()));
                let accepted = expected != replacement && reference.get(&id).copied() == expected;
                let result = native.replace_prepaid_receipts(&[row], LIMITS).await;
                prop_assert_eq!(result.is_ok(), accepted);
                if accepted {
                    match replacement {
                        Some(value) => { reference.insert(id, value); },
                        None => { reference.remove(&id); },
                    }
                }
                for key in 0..4 {
                    prop_assert_eq!(native.read_prepaid_receipt(&[key; 32], 1024).await.unwrap(), reference.get(&key).map(|value| vec![*value]));
                }
            }
            Ok(())
        })?;
    }
}
