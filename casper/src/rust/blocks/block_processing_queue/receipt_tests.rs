use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use models::rust::block_implicits::get_random_block_default;
use prost::bytes::Bytes;

use super::*;
use crate::rust::casper::test_helpers::TestCasperWithSnapshot;

fn casper() -> Arc<dyn MultiParentCasper + Send + Sync> {
    Arc::new(TestCasperWithSnapshot::new(
        TestCasperWithSnapshot::create_empty_snapshot(),
        get_random_block_default(),
    ))
}

#[test]
fn receipt_precedes_visibility_to_an_independent_worker() {
    let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1024 * 1024).unwrap();
    let received = Arc::new(AtomicBool::new(false));
    let observed = received.clone();
    let (start, started) = std::sync::mpsc::channel();
    let (checked, check) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        checked.send(()).unwrap();
        let item = receiver.blocking_recv().unwrap();
        assert!(observed.load(Ordering::Acquire));
        item
    });
    let block = get_random_block_default();
    sender
        .try_enqueue_with_receipt(casper(), block, |hash| {
            assert!(sender.identities().contains(hash));
            assert!(sender.used_bytes() > 0);
            start.send(()).unwrap();
            check.recv_timeout(Duration::from_secs(5)).unwrap();
            received.store(true, Ordering::Release);
            Ok::<_, &'static str>(())
        })
        .unwrap();
    let item = worker.join().unwrap();
    assert!(sender.used_bytes() > 0);
    drop(item);
    assert!(sender.identities().is_empty());
    assert_eq!(sender.used_bytes(), 0);
}

#[test]
fn receipt_failure_releases_every_reservation_without_publishing() {
    let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1024 * 1024).unwrap();
    let result = sender.try_enqueue_with_receipt(casper(), get_random_block_default(), |_| {
        assert_eq!(sender.capacity(), 0);
        assert!(sender.used_bytes() > 0);
        Err("injected receipt failure")
    });
    assert!(matches!(
        result,
        Err(BlockPublicationError::Receipt("injected receipt failure"))
    ));
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(sender.capacity(), 1);
    assert_eq!(sender.used_bytes(), 0);
    assert!(sender.identities().is_empty());
}

#[test]
fn receipt_is_not_called_for_rejected_or_duplicate_admission() {
    for expected in [
        BlockAdmissionFailure::Duplicate,
        BlockAdmissionFailure::CountCapacity,
        BlockAdmissionFailure::ByteCapacity,
        BlockAdmissionFailure::Oversized,
        BlockAdmissionFailure::Closed,
    ] {
        let block = get_random_block_default();
        let bytes = block.to_proto().encoded_len().max(1);
        let limit = match expected {
            BlockAdmissionFailure::ByteCapacity => bytes,
            BlockAdmissionFailure::Oversized => bytes - 1,
            _ => bytes * 3,
        };
        let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, limit).unwrap();
        let mut held = None;
        if matches!(
            expected,
            BlockAdmissionFailure::Duplicate
                | BlockAdmissionFailure::CountCapacity
                | BlockAdmissionFailure::ByteCapacity
        ) {
            let mut occupying = block.clone();
            if expected != BlockAdmissionFailure::Duplicate {
                occupying.block_hash = Bytes::from(vec![0; 32]);
                if occupying.block_hash == block.block_hash {
                    occupying.block_hash = Bytes::from(vec![1; 32]);
                }
            }
            sender.try_enqueue(casper(), occupying).unwrap();
            if expected == BlockAdmissionFailure::ByteCapacity {
                held = Some(receiver.try_recv().unwrap());
            }
        }
        if expected == BlockAdmissionFailure::Closed {
            receiver.close();
        }
        let before_bytes = sender.used_bytes();
        let before_identities = sender.identities().len();
        let calls = AtomicUsize::new(0);
        let result = sender.try_enqueue_with_receipt(casper(), block, |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok::<_, &'static str>(())
        });
        assert!(
            matches!(result, Err(BlockPublicationError::Admission(error)) if error.failure == expected)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(sender.used_bytes(), before_bytes);
        assert_eq!(sender.identities().len(), before_identities);
        drop(held);
        drop(receiver);
        assert_eq!(sender.used_bytes(), 0);
        assert!(sender.identities().is_empty());
    }
}

#[test]
fn receipt_unwind_does_not_publish_or_leak_reservations() {
    let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1024 * 1024).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sender.try_enqueue_with_receipt(
            casper(),
            get_random_block_default(),
            |_| -> Result<(), &'static str> { panic!("injected receipt unwind") },
        )
    }));
    assert!(result.is_err());
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(sender.capacity(), 1);
    assert_eq!(sender.used_bytes(), 0);
    assert!(sender.identities().is_empty());
}
