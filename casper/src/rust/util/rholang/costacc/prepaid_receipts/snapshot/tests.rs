use std::collections::BTreeMap;
use std::sync::Arc;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::costacc::prepaid_receipts::{receipt_datum, PrepaidReceiptChange};

const LIMITS: PrepaidReceiptLimits = PrepaidReceiptLimits {
    entries: 32,
    value_bytes: 4096,
    batch_bytes: 65_536,
};

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)))
}

async fn fixture() -> (Arc<RuntimeManager>, RuntimeOps) {
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
    let native = RuntimeOps::new(manager.spawn_runtime().await);
    (Arc::new(manager), native)
}

async fn checkpoint(native: &mut RuntimeOps) -> [u8; 32] {
    native
        .runtime
        .create_checkpoint()
        .await
        .root
        .bytes()
        .try_into()
        .unwrap()
}

#[tokio::test]
async fn snapshot_distinguishes_absent_unrequested_and_wrong_root() {
    let (manager, mut native) = fixture().await;
    let before = checkpoint(&mut native).await;
    let absent = manager
        .capture_prepaid_receipts(before, &[[1; 32]], LIMITS, &budget())
        .unwrap();
    assert_eq!(absent.root(), before);
    assert_eq!(absent.receipt(&before, &[1; 32]).unwrap(), Some(None));
    assert_eq!(absent.receipt(&before, &[2; 32]).unwrap(), None);
    native
        .replace_prepaid_receipts(
            &[PrepaidReceiptChange {
                receipt_id: [1; 32],
                expected: None,
                replacement: Some(b"original"),
            }],
            LIMITS,
        )
        .await
        .unwrap();
    let after = checkpoint(&mut native).await;
    assert_ne!(before, after);
    assert!(absent.receipt(&after, &[1; 32]).is_err());
    assert!(absent.receipt(&after, &[2; 32]).is_err());
    let present = manager
        .capture_prepaid_receipts(after, &[[2; 32], [1; 32]], LIMITS, &budget())
        .unwrap();
    assert_eq!(
        present.receipt(&after, &[1; 32]).unwrap(),
        Some(Some(b"original".as_slice()))
    );
    assert_eq!(present.receipt(&after, &[2; 32]).unwrap(), Some(None));
    let historical = manager
        .capture_prepaid_receipts(before, &[[1; 32]], LIMITS, &budget())
        .unwrap();
    assert_eq!(historical, absent);
    let mut forged_root = after;
    forged_root[0] ^= 1;
    assert!(matches!(
        manager.capture_prepaid_receipts(forged_root, &[[1; 32]], LIMITS, &budget()),
        Err(CasperError::RuntimeError(_))
    ));
    assert!(manager
        .capture_prepaid_receipts(forged_root, &[], LIMITS, &budget())
        .is_err());
    let empty = manager
        .capture_prepaid_receipts(after, &[], LIMITS, &budget())
        .unwrap();
    assert_eq!(empty.root(), after);
    assert_eq!(empty.receipt(&after, &[1; 32]).unwrap(), None);
}

#[tokio::test]
async fn snapshot_rejects_duplicates_late_malformed_values_and_bounds() {
    let (manager, mut native) = fixture().await;
    native
        .replace_prepaid_receipts(
            &[
                PrepaidReceiptChange {
                    receipt_id: [1; 32],
                    expected: None,
                    replacement: Some(b"one"),
                },
                PrepaidReceiptChange {
                    receipt_id: [2; 32],
                    expected: None,
                    replacement: Some(b"second"),
                },
            ],
            LIMITS,
        )
        .await
        .unwrap();
    let root = checkpoint(&mut native).await;
    assert!(manager
        .capture_prepaid_receipts(root, &[[1; 32], [1; 32]], LIMITS, &budget())
        .is_err());
    let overhead = 2 * (size_of::<[u8; 32]>() + size_of::<SnapshotEntry>());
    for bounds in [
        PrepaidReceiptLimits {
            entries: 1,
            ..LIMITS
        },
        PrepaidReceiptLimits {
            value_bytes: 5,
            ..LIMITS
        },
        PrepaidReceiptLimits {
            batch_bytes: overhead + 8,
            ..LIMITS
        },
    ] {
        assert!(manager
            .capture_prepaid_receipts(root, &[[1; 32], [2; 32]], bounds, &budget())
            .is_err());
    }
    let exact = PrepaidReceiptLimits {
        entries: 2,
        value_bytes: 6,
        batch_bytes: overhead + 9,
    };
    assert!(manager
        .capture_prepaid_receipts(root, &[[1; 32], [2; 32]], exact, &budget())
        .is_ok());
    let tiny = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(manager
        .capture_prepaid_receipts(root, &[[1; 32]], LIMITS, &tiny)
        .is_err());
    assert!(tiny.is_rejected());
    for mutation in 0..3 {
        native
            .runtime
            .reset(&Blake2b256Hash::from_bytes(root.to_vec()))
            .await
            .unwrap();
        let mut datum = receipt_datum(b"invalid");
        if mutation == 1 {
            datum.random_state.push(0);
        }
        native
            .runtime
            .reducer
            .space
            .produce(receipt_channel(&[3; 32]), datum, mutation == 2)
            .await
            .unwrap();
        if mutation == 0 {
            native
                .runtime
                .reducer
                .space
                .produce(
                    receipt_channel(&[3; 32]),
                    receipt_datum(b"duplicate"),
                    false,
                )
                .await
                .unwrap();
        }
        let bad_root = checkpoint(&mut native).await;
        assert!(manager
            .capture_prepaid_receipts(bad_root, &[[1; 32], [3; 32]], LIMITS, &budget())
            .is_err());
        assert_eq!(checkpoint(&mut native).await, bad_root);
    }
    assert!(manager
        .capture_prepaid_receipts(root, &[[1; 32], [2; 32]], exact, &budget())
        .is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_snapshots_keep_independent_roots_during_publication() {
    let (manager, mut native) = fixture().await;
    native
        .replace_prepaid_receipts(
            &[PrepaidReceiptChange {
                receipt_id: [1; 32],
                expected: None,
                replacement: Some(b"old"),
            }],
            LIMITS,
        )
        .await
        .unwrap();
    let first = checkpoint(&mut native).await;
    let mut workers = Vec::new();
    let barrier = Arc::new(std::sync::Barrier::new(5));
    for _ in 0..4 {
        let manager = Arc::clone(&manager);
        let barrier = Arc::clone(&barrier);
        workers.push(tokio::task::spawn_blocking(move || {
            barrier.wait();
            for _ in 0..32 {
                let snapshot = manager
                    .capture_prepaid_receipts(first, &[[2; 32], [1; 32]], LIMITS, &budget())
                    .unwrap();
                assert_eq!(
                    snapshot.receipt(&first, &[1; 32]).unwrap(),
                    Some(Some(b"old".as_slice()))
                );
                assert_eq!(snapshot.receipt(&first, &[2; 32]).unwrap(), Some(None));
            }
        }));
    }
    let barrier_wait = tokio::task::spawn_blocking(move || barrier.wait());
    barrier_wait.await.unwrap();
    for index in 0..8u8 {
        let prior = if index == 0 {
            b"old".as_slice()
        } else {
            &[index - 1]
        };
        native
            .replace_prepaid_receipts(
                &[PrepaidReceiptChange {
                    receipt_id: [1; 32],
                    expected: Some(prior),
                    replacement: Some(&[index]),
                }],
                LIMITS,
            )
            .await
            .unwrap();
        let later = checkpoint(&mut native).await;
        let fresh = manager
            .capture_prepaid_receipts(later, &[[1; 32]], LIMITS, &budget())
            .unwrap();
        assert_eq!(
            fresh.receipt(&later, &[1; 32]).unwrap(),
            Some(Some([index].as_slice()))
        );
        assert!(fresh.receipt(&first, &[1; 32]).is_err());
    }
    for worker in workers {
        worker.await.unwrap();
    }
    let first_view = manager
        .capture_prepaid_receipts(first, &[[1; 32]], LIMITS, &budget())
        .unwrap();
    assert_eq!(
        first_view.receipt(&first, &[1; 32]).unwrap(),
        Some(Some(b"old".as_slice()))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn snapshot_histories_refine_rooted_lookup_and_key_permutations(
        changes in prop::collection::vec((0u8..8, prop::option::of(1u8..255)), 0..24),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (manager, mut native) = fixture().await;
            let mut reference = BTreeMap::new();
            let mut histories = Vec::new();
            let keys = (0u8..8).map(|id| [id;32]).collect::<Vec<_>>();
            let reverse = keys.iter().rev().copied().collect::<Vec<_>>();
            histories.push((checkpoint(&mut native).await, reference.clone()));
            for (id, value) in changes {
                let old = reference.get(&id).copied();
                if old == value { continue; }
                let prior = old.map(|n| [n]);
                let next = value.map(|n| [n]);
                native.replace_prepaid_receipts(&[PrepaidReceiptChange {
                    receipt_id: [id; 32], expected: prior.as_ref().map(|x| x.as_slice()), replacement: next.as_ref().map(|x| x.as_slice()),
                }], LIMITS).await.unwrap();
                match value { Some(v) => { reference.insert(id, v); }, None => { reference.remove(&id); } }
                histories.push((checkpoint(&mut native).await, reference.clone()));
            }
            for (root, values) in histories {
                let normal = manager.capture_prepaid_receipts(root, &keys, LIMITS, &budget()).unwrap();
                let reversed = manager.capture_prepaid_receipts(root, &reverse, LIMITS, &budget()).unwrap();
                prop_assert_eq!(&normal, &reversed);
                for id in 0u8..8 {
                    let expected = values.get(&id).copied().map(|n| [n]);
                    prop_assert_eq!(normal.receipt(&root, &[id;32]).unwrap(), Some(expected.as_ref().map(|x| x.as_slice())));
                }
                prop_assert_eq!(normal.receipt(&root, &[9;32]).unwrap(), None);
            }
            Ok(())
        })?;
    }
}
