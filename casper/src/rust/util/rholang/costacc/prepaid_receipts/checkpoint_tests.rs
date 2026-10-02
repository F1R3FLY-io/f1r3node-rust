use std::sync::Arc;

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

const LIMITS: PrepaidReceiptLimits = PrepaidReceiptLimits {
    entries: 4,
    value_bytes: 64,
    batch_bytes: 1024,
};

async fn history(operations: Vec<(usize, u8, u8)>) {
    let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
    let (manager, _) = RuntimeManager::create_with_history(
        store,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    );
    let mut initial = manager.spawn_runtime().await;
    let mut published = initial.create_checkpoint().await.root;
    let mut values = [None, None, None, None];
    for (prefix, outcome, seed) in operations {
        let mut candidate = RuntimeOps::new(manager.spawn_runtime().await);
        candidate.runtime.reset(&published).await.unwrap();
        let fallback = candidate.runtime.create_soft_checkpoint().await;
        let before = values;
        let after = before.map(|value| Some(value.unwrap_or(seed).wrapping_add(1)));
        let (signal, reached) = tokio::sync::oneshot::channel();
        let mut task = tokio::spawn(async move {
            let mut log = Vec::new();
            for index in 0..prefix {
                log.extend(replace(&mut candidate, index, before[index], after[index]).await);
            }
            signal.send(()).unwrap();
            if outcome == 0 {
                std::future::pending::<()>().await;
            }
            if outcome == 1 {
                candidate.runtime.revert_to_soft_checkpoint(fallback).await;
                return (candidate.runtime.create_checkpoint().await.root, log);
            }
            for index in prefix..4 {
                log.extend(replace(&mut candidate, index, before[index], after[index]).await);
            }
            (candidate.runtime.create_checkpoint().await.root, log)
        });
        reached.await.unwrap();
        let mut observer = RuntimeOps::new(manager.spawn_runtime().await);
        observer.runtime.reset(&published).await.unwrap();
        assert_values(&observer, before).await;
        if outcome == 0 {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            let (root, log) = (&mut task).await.unwrap();
            if outcome == 1 {
                assert_eq!(root, published);
            } else {
                let mut replay = RuntimeOps::new(manager.spawn_replay_runtime().await);
                replay.runtime.reset(&published).await.unwrap();
                replay.runtime.rig(log).await.unwrap();
                for index in 0..4 {
                    replace(&mut replay, index, before[index], after[index]).await;
                }
                replay.runtime.check_replay_data().await.unwrap();
                assert_eq!(replay.runtime.create_checkpoint().await.root, root);
                published = root;
                values = after;
            }
        }
        observer.runtime.reset(&published).await.unwrap();
        assert_values(&observer, values).await;
    }
}

async fn replace(
    runtime: &mut RuntimeOps,
    index: usize,
    before: Option<u8>,
    after: Option<u8>,
) -> Log {
    let before = before.map(|value| [value]);
    let after = after.map(|value| [value]);
    runtime
        .replace_prepaid_receipts(
            &[PrepaidReceiptChange {
                receipt_id: [index as u8; 32],
                expected: before.as_ref().map(|value| value.as_slice()),
                replacement: after.as_ref().map(|value| value.as_slice()),
            }],
            LIMITS,
        )
        .await
        .unwrap()
}

async fn assert_values(runtime: &RuntimeOps, expected: [Option<u8>; 4]) {
    for (index, expected) in expected.into_iter().enumerate() {
        assert_eq!(
            runtime
                .read_prepaid_receipt(&[index as u8; 32], LIMITS.value_bytes)
                .await
                .unwrap(),
            expected.map(|value| vec![value])
        );
    }
}

#[test]
fn generated_checkpoint_histories_keep_failed_and_canceled_candidates_private() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let mut runner = TestRunner::new(Config {
        cases: 48,
        source_file: Some(file!()),
        ..Config::default()
    });
    runner
        .run(
            &prop::collection::vec((0usize..5, 0u8..3, any::<u8>()), 1..17),
            |operations| {
                runtime.block_on(history(operations));
                Ok(())
            },
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_checkpoint_histories_allow_all_failure_and_cancellation_prefixes() {
    let operations: Vec<_> = (0..5)
        .flat_map(|prefix| (0..3).map(move |outcome| (prefix, outcome, 17)))
        .collect();
    let reversed = operations.iter().copied().rev().collect();
    tokio::join!(history(operations), history(reversed));
}
