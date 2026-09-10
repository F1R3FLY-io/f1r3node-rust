use std::sync::Arc;

use casper::rust::blocks::block_processing_queue::{
    BlockAdmissionFailure, BlockProcessingIdentities, BlockProcessingQueueSender, RecoveryWake,
};
use casper::rust::casper::MultiParentCasper;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block_default;
use prost::bytes::Bytes;
use prost::Message;

use super::setup::TestFixture;

#[test]
fn generated_admission_histories_preserve_physical_leases() {
    use std::collections::{BTreeMap, VecDeque};

    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let fixture = runtime.block_on(TestFixture::new());
    let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(fixture.casper.clone());
    let base = get_random_block_default();
    let strategy = (
        1usize..5,
        1usize..8,
        prop::collection::vec((0u8..5, 0u8..8, 0usize..1024), 0..128),
    );
    let mut runner = TestRunner::new(Config {
        cases: 64,
        ..Config::default()
    });
    runner
        .run(&strategy, |(count_limit, byte_multiplier, operations)| {
            let byte_limit = base.to_proto().encoded_len().max(1) * byte_multiplier;
            let (sender, mut receiver) =
                BlockProcessingQueueSender::channel(count_limit, byte_limit).unwrap();
            let signal = sender.recovery().signal();
            let mut queued = VecDeque::new();
            let mut workers = BTreeMap::new();
            let mut expected = BTreeMap::<u8, usize>::new();
            let mut closed = false;
            for (operation, key, body_size) in operations {
                let mut expected_wake = RecoveryWake::Idle;
                match operation {
                    0 => {
                        let mut block = base.clone();
                        block.block_hash = Bytes::from(vec![key; 32]);
                        block.extra_bytes = Bytes::from(vec![0; body_size]);
                        let bytes = block.to_proto().encoded_len().max(1);
                        let used: usize = expected.values().sum();
                        let failure = if expected.contains_key(&key) {
                            Some(BlockAdmissionFailure::Duplicate)
                        } else if bytes > byte_limit {
                            Some(BlockAdmissionFailure::Oversized)
                        } else if bytes > byte_limit - used {
                            Some(BlockAdmissionFailure::ByteCapacity)
                        } else if closed {
                            Some(BlockAdmissionFailure::Closed)
                        } else if queued.len() == count_limit {
                            Some(BlockAdmissionFailure::CountCapacity)
                        } else {
                            None
                        };
                        let result = sender.try_enqueue(casper.clone(), block);
                        prop_assert_eq!(result.as_ref().err().map(|e| e.failure), failure);
                        if failure.is_none() {
                            expected.insert(key, bytes);
                            queued.push_back(key);
                        }
                    }
                    1 => {
                        if let Some(key) = queued.pop_front() {
                            let item = receiver.try_recv().unwrap();
                            prop_assert_eq!(item.block.block_hash.as_ref(), &[key; 32]);
                            prop_assert_eq!(item.reservation.bytes(), expected[&key]);
                            prop_assert!(workers.insert(key, item).is_none());
                            sender.record_dequeue();
                            expected_wake = RecoveryWake::Work { proposal: false };
                        } else {
                            prop_assert!(receiver.try_recv().is_err());
                        }
                    }
                    2 => {
                        if let Some(item) = workers.remove(&key) {
                            drop(item);
                            expected.remove(&key);
                            expected_wake = RecoveryWake::Work { proposal: false };
                        }
                    }
                    3 => {
                        receiver.close();
                        closed = true;
                    }
                    _ => {
                        signal.request(true);
                        expected_wake = RecoveryWake::Work { proposal: true };
                    }
                }
                prop_assert_eq!(sender.used_bytes(), expected.values().sum::<usize>());
                prop_assert_eq!(sender.identities().len(), expected.len());
                prop_assert_eq!(receiver.len(), queued.len());
                prop_assert!(sender.used_bytes() <= byte_limit);
                for candidate in 0u8..8 {
                    let hash = Bytes::from(vec![candidate; 32]);
                    prop_assert_eq!(
                        sender.identities().contains(&hash),
                        expected.contains_key(&candidate)
                    );
                }
                prop_assert_eq!(signal.take(), expected_wake);
                prop_assert_eq!(signal.take(), RecoveryWake::Idle);
            }
            drop(workers);
            drop(receiver);
            prop_assert_eq!(sender.used_bytes(), 0);
            prop_assert!(sender.identities().is_empty());
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn overlapping_temporary_senders_release_the_last_strong_endpoint() {
    let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1024).unwrap();
    let weak = sender.downgrade();
    let recovery_borrow = weak.upgrade().unwrap();
    drop(sender);
    let metric_borrow = weak.upgrade().unwrap();
    drop(recovery_borrow);
    assert!(weak.upgrade().is_some());
    drop(metric_borrow);
    assert!(weak.upgrade().is_none());
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_keeps_admission_charged_until_payload_destruction_returns() {
    struct DelayedBody {
        bytes: Vec<u8>,
        started: Option<tokio::sync::oneshot::Sender<()>>,
        release: std::sync::mpsc::Receiver<()>,
    }
    impl AsRef<[u8]> for DelayedBody {
        fn as_ref(&self) -> &[u8] { &self.bytes }
    }
    impl Drop for DelayedBody {
        fn drop(&mut self) {
            self.started.take().unwrap().send(()).unwrap();
            self.release
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
    }

    let fixture = TestFixture::new().await;
    let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(fixture.casper.clone());
    let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1_000_000).unwrap();
    let signal = sender.recovery().signal();
    let (dropping_tx, dropping_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut block = get_random_block_default();
    let key = block.block_hash.clone();
    block.extra_bytes = Bytes::from_owner(DelayedBody {
        bytes: vec![0; 1024],
        started: Some(dropping_tx),
        release: release_rx,
    });
    let bytes = block.to_proto().encoded_len();
    sender.try_enqueue(casper, block).unwrap();
    let item = receiver.recv().await.unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(async move {
        let (_lease, _casper, _body) = item.into_parts();
        started_tx.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    started_rx.await.unwrap();
    workers.abort_all();
    dropping_rx.await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(sender.used_bytes(), bytes);
    assert!(sender.identities().contains(&key));
    assert_eq!(signal.take(), RecoveryWake::Idle);
    release_tx.send(()).unwrap();
    let result = workers.join_next().await.unwrap();
    assert!(result.unwrap_err().is_cancelled());
    assert!(workers.is_empty());
    assert_eq!(sender.used_bytes(), 0);
    assert!(!sender.identities().contains(&key));
    assert_eq!(signal.take(), RecoveryWake::Work { proposal: false });
    assert_eq!(signal.take(), RecoveryWake::Idle);
}

struct ObservedBody {
    bytes: Vec<u8>,
    identities: Arc<BlockProcessingIdentities>,
    key: BlockHash,
}

#[tokio::test]
async fn running_publication_announces_only_committed_engines() {
    use casper::rust::engine::engine::{noop, transition_to_running};
    use casper::rust::engine::engine_cell::EngineCell;
    use models::rust::casper::protocol::casper_message::ApprovedBlock;
    use shared::rust::shared::f1r3fly_event::F1r3flyEvent;
    use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

    let fixture = TestFixture::new().await;
    let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(fixture.casper.clone());
    for failure in 0..3 {
        let cell = EngineCell::init();
        let (queue, _receiver) = BlockProcessingQueueSender::channel(2, 1_000_000).unwrap();
        let recovery = queue.recovery();
        if failure == 1 {
            recovery.stop();
        } else if failure == 2 {
            let other = fixture.block_processing_queue_tx.recovery();
            cell.set_running(
                Arc::new(noop()),
                other.clone(),
                other.startup().prepare(&casper),
            )
            .await
            .unwrap();
        }
        let before = cell.get().await;
        let events = F1r3flyEvents::new();
        let result = transition_to_running(
            queue.clone(),
            queue.identities(),
            casper.clone(),
            ApprovedBlock {
                candidate: fixture.approved_block_candidate.clone(),
                sigs: Vec::new(),
                floor_seed: None,
            },
            Arc::new(|_| Box::pin(async { Ok(()) })),
            false,
            fixture.transport_layer.clone(),
            fixture.rp_conf_ask.clone(),
            fixture.block_retriever.clone(),
            None,
            None,
            None,
            &cell,
            &events,
            None,
        )
        .await;
        let buffer = events.startup_buffer();
        let announcements = buffer
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, F1r3flyEvent::EnteredRunningState(_)))
            .count();
        if failure == 0 {
            result.unwrap();
            assert_eq!(announcements, 1);
            assert!(Arc::ptr_eq(
                &cell.get().await.with_casper().unwrap(),
                &casper
            ));
            assert!(Arc::ptr_eq(&recovery.context().unwrap().unwrap(), &casper));
        } else {
            assert!(result.is_err());
            assert_eq!(announcements, 0);
            assert!(Arc::ptr_eq(&before, &cell.get().await));
            assert!(recovery.context().unwrap().is_none());
        }
    }
}

impl AsRef<[u8]> for ObservedBody {
    fn as_ref(&self) -> &[u8] { &self.bytes }
}

impl Drop for ObservedBody {
    fn drop(&mut self) {
        assert!(
            self.identities.contains(&self.key),
            "body outlived its admission identity"
        );
    }
}

#[tokio::test]
async fn admission_identity_covers_queue_rejection_and_worker_cancellation() {
    let fixture = TestFixture::new().await;
    let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(fixture.casper.clone());
    let (sender, receiver) = BlockProcessingQueueSender::channel(1, 1_000_000).unwrap();
    let signal = sender.recovery().signal();
    let identities = sender.identities();
    let block = get_random_block_default();
    let key = block.block_hash.clone();
    let bytes = block.to_proto().encoded_len();
    sender.try_enqueue(casper.clone(), block.clone()).unwrap();
    assert_eq!(sender.used_bytes(), bytes);
    assert_eq!(
        signal.take(),
        RecoveryWake::Idle,
        "admission must not create a retry request"
    );
    let duplicate = sender
        .try_enqueue(casper.clone(), block.clone())
        .unwrap_err();
    assert_eq!(duplicate.failure, BlockAdmissionFailure::Duplicate);
    assert_eq!(sender.used_bytes(), bytes);
    assert_eq!(identities.len(), 1);

    let other = get_random_block_default();
    let other_key = other.block_hash.clone();
    let rejected = sender.try_enqueue(casper.clone(), other).unwrap_err();
    assert_eq!(rejected.failure, BlockAdmissionFailure::CountCapacity);
    assert!(!identities.contains(&other_key));
    assert!(identities.contains(&key));
    assert_eq!(sender.used_bytes(), bytes);
    assert_eq!(
        signal.take(),
        RecoveryWake::Idle,
        "count rejection must not wake the pump"
    );
    drop(receiver);
    assert!(identities.is_empty());
    assert_eq!(sender.used_bytes(), 0);
    assert_eq!(signal.take(), RecoveryWake::Work { proposal: false });
    assert_eq!(signal.take(), RecoveryWake::Idle);
    let closed = sender
        .try_enqueue(casper.clone(), block.clone())
        .unwrap_err();
    assert_eq!(closed.failure, BlockAdmissionFailure::Closed);
    assert_eq!(
        signal.take(),
        RecoveryWake::Idle,
        "closed admission must not create a retry request"
    );
    assert!(identities.is_empty());
    assert_eq!(sender.used_bytes(), 0);

    let (sender, receiver) = BlockProcessingQueueSender::channel(2, bytes).unwrap();
    let signal = sender.recovery().signal();
    sender.try_enqueue(casper.clone(), block.clone()).unwrap();
    let mut other = block.clone();
    other.block_hash = Bytes::from(vec![91; block.block_hash.len()]);
    assert_eq!(
        sender
            .try_enqueue(casper.clone(), other)
            .unwrap_err()
            .failure,
        BlockAdmissionFailure::ByteCapacity
    );
    let mut oversized = block;
    oversized.block_hash = Bytes::from(vec![92; key.len()]);
    oversized.extra_bytes = Bytes::from(vec![1; bytes + 1]);
    assert_eq!(
        sender
            .try_enqueue(casper.clone(), oversized)
            .unwrap_err()
            .failure,
        BlockAdmissionFailure::Oversized
    );
    assert_eq!(sender.identities().len(), 1);
    assert_eq!(
        signal.take(),
        RecoveryWake::Idle,
        "failed reservations must not wake the pump"
    );
    drop(receiver);
    assert!(sender.identities().is_empty());
    assert_eq!(sender.used_bytes(), 0);

    let mut rejected = get_random_block_default();
    rejected.extra_bytes = Bytes::from(vec![1; 512]);
    let rejected_bytes = rejected.to_proto().encoded_len();
    for oversized in [false, true] {
        let capacity = if oversized {
            rejected_bytes - 1
        } else {
            rejected_bytes
        };
        let (sender, receiver) = BlockProcessingQueueSender::channel(2, capacity).unwrap();
        let mut candidate = rejected.clone();
        candidate.extra_bytes = Bytes::from_owner(ObservedBody {
            bytes: vec![1; 512],
            identities: sender.identities(),
            key: candidate.block_hash.clone(),
        });
        if !oversized {
            let mut existing = rejected.clone();
            let mut existing_hash = existing.block_hash.to_vec();
            existing_hash[0] ^= 1;
            existing.block_hash = Bytes::from(existing_hash);
            sender.try_enqueue(casper.clone(), existing).unwrap();
        }
        let error = sender.try_enqueue(casper.clone(), candidate).unwrap_err();
        assert_eq!(
            error.failure,
            if oversized {
                BlockAdmissionFailure::Oversized
            } else {
                BlockAdmissionFailure::ByteCapacity
            }
        );
        drop(receiver);
        assert_eq!(sender.used_bytes(), 0);
        assert!(sender.identities().is_empty());
    }

    let (sender, receiver) = BlockProcessingQueueSender::channel(4, 1_000_000).unwrap();
    let shared_block = get_random_block_default();
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let outcomes = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let sender = sender.clone();
                let casper = casper.clone();
                let block = shared_block.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    sender.try_enqueue(casper, block)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    for error in outcomes.into_iter().filter_map(Result::err) {
        assert_eq!(error.failure, BlockAdmissionFailure::Duplicate);
    }
    assert_eq!(sender.identities().len(), 1);
    assert_eq!(receiver.len(), 1);
    drop(receiver);
    assert_eq!(sender.used_bytes(), 0);
    assert!(sender.identities().is_empty());

    let inspection_block = get_random_block_default();
    let inspection_key = inspection_block.block_hash.clone();
    fixture
        .block_processing_queue_tx
        .try_enqueue(casper.clone(), inspection_block)
        .unwrap();
    let admitted_bytes = fixture.block_processing_queue_tx.used_bytes();
    let queue_capacity = fixture.block_processing_queue_tx.capacity();
    for _ in 0..3 {
        assert!(fixture.is_block_admitted(&inspection_key));
        assert_eq!(
            fixture.block_processing_queue_tx.used_bytes(),
            admitted_bytes
        );
        assert_eq!(fixture.block_processing_queue_tx.capacity(), queue_capacity);
    }
    let item = fixture
        .block_processing_queue_rx
        .lock()
        .await
        .try_recv()
        .unwrap();
    assert_eq!(item.block.block_hash, inspection_key);
    drop(item);
    assert!(!fixture.is_block_admitted(&inspection_key));

    for outcome in 0..7 {
        let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1_000_000).unwrap();
        let mut block = get_random_block_default();
        block.extra_bytes = Bytes::from_owner(ObservedBody {
            bytes: vec![1; 512],
            identities: sender.identities(),
            key: block.block_hash.clone(),
        });
        sender.try_enqueue(casper.clone(), block).unwrap();
        if outcome == 0 {
            drop(receiver);
        } else {
            let item = receiver.recv().await.unwrap();
            let (ready, started) = tokio::sync::oneshot::channel();
            let worker = tokio::spawn(async move {
                let (_lease, _casper, block) = item.into_parts();
                if outcome == 6 {
                    return async move {
                        let _body = block;
                        ready.send(()).unwrap();
                        std::future::pending::<Result<(), &str>>().await
                    }
                    .await;
                }
                let _body = block;
                ready.send(()).unwrap();
                if outcome == 3 {
                    panic!("injected processing panic");
                }
                if outcome == 4 {
                    return Err("injected processing error");
                }
                if outcome == 5 {
                    return Ok(());
                }
                std::future::pending::<()>().await;
                Ok(())
            });
            if outcome == 1 {
                worker.abort();
            } else {
                started.await.unwrap();
                if outcome == 2 || outcome == 6 {
                    worker.abort();
                }
            }
            let result = worker.await;
            match outcome {
                1 | 2 | 6 => assert!(result.unwrap_err().is_cancelled()),
                3 => assert!(result.unwrap_err().is_panic()),
                4 => assert_eq!(result.unwrap(), Err("injected processing error")),
                5 => assert_eq!(result.unwrap(), Ok(())),
                _ => unreachable!(),
            }
        }
        assert_eq!(sender.used_bytes(), 0);
        assert!(sender.identities().is_empty());
    }
}
