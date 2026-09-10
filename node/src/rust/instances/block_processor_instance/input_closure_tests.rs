use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use casper::rust::blocks::block_processing_queue::RecoveryControl;
use models::rust::block_implicits::get_random_block_default;
use prost::bytes::Bytes;

use super::*;

struct HeldPayload {
    key: u8,
    bytes: [u8; 8],
    entered: tokio::sync::mpsc::UnboundedSender<u8>,
    resume: std::sync::mpsc::Receiver<()>,
    released: Arc<AtomicUsize>,
}

impl AsRef<[u8]> for HeldPayload {
    fn as_ref(&self) -> &[u8] { &self.bytes }
}

impl Drop for HeldPayload {
    fn drop(&mut self) {
        let _ = self.entered.send(self.key);
        let _ = self.resume.recv_timeout(Duration::from_secs(20));
        self.released.fetch_add(1, Ordering::SeqCst);
    }
}

struct HeldWorkers {
    dispatcher: Option<tokio::task::JoinHandle<Result<(), CasperError>>>,
    sender: Option<BlockProcessingQueueSender>,
    recovery: Arc<RecoveryControl>,
    identities: Arc<BlockProcessingIdentities>,
    released: Arc<AtomicUsize>,
    resumes: Vec<std::sync::mpsc::Sender<()>>,
}

impl HeldWorkers {
    async fn new(buffered: bool) -> Self {
        let (mut instance, sender, casper) = ownership_tests::fixture().await;
        instance.max_parallel_blocks = 2;
        let (entered, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
        let released = Arc::new(AtomicUsize::new(0));
        let mut resumes = Vec::new();
        for key in 1u8..=2 {
            let mut block = get_random_block_default();
            block.block_hash = Bytes::from(vec![key; 32]);
            let (resume, wait) = std::sync::mpsc::channel();
            block.sig = Bytes::from_owner(HeldPayload {
                key,
                bytes: [key; 8],
                entered: entered.clone(),
                resume: wait,
                released: released.clone(),
            });
            instance
                .block_processor
                .note_validation_failure(&block.block_hash)
                .unwrap();
            sender.try_enqueue(casper.clone(), block).unwrap();
            resumes.push(resume);
        }
        if buffered {
            let mut block = get_random_block_default();
            block.block_hash = Bytes::from(vec![3; 32]);
            instance
                .block_processor
                .note_validation_failure(&block.block_hash)
                .unwrap();
            sender.try_enqueue(casper, block).unwrap();
        }
        let held = Self {
            recovery: sender.recovery(),
            identities: sender.identities(),
            sender: Some(sender),
            released,
            resumes,
            dispatcher: Some(tokio::spawn(instance.run())),
        };
        let first = tokio::time::timeout(Duration::from_secs(5), arrivals.recv())
            .await
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(Duration::from_secs(5), arrivals.recv())
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(held.released.load(Ordering::SeqCst), 0);
        held
    }

    async fn wait_for_stop(&self) -> Result<(), tokio::time::error::Elapsed> {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.recovery.signal().is_stopped() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
    }

    async fn finish(mut self) {
        self.sender = None;
        for resume in self.resumes.drain(..) {
            let _ = resume.send(());
        }
        tokio::time::timeout(Duration::from_secs(5), self.dispatcher.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(self.released.load(Ordering::SeqCst), 2);
        assert!(self.identities.is_empty());
    }
}

impl Drop for HeldWorkers {
    fn drop(&mut self) {
        for resume in self.resumes.drain(..) {
            let _ = resume.send(());
        }
        if let Some(dispatcher) = &self.dispatcher {
            dispatcher.abort();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_workers_do_not_hide_closed_empty_input() {
    let mut held = HeldWorkers::new(false).await;
    held.sender = None;
    let observed = held.wait_for_stop().await;
    assert_eq!(held.released.load(Ordering::SeqCst), 0);
    assert_eq!(held.identities.len(), 2);
    held.finish().await;
    assert!(
        observed.is_ok(),
        "closure observation waited for worker completion"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn last_temporary_sender_controls_closure_without_extra_dequeue() {
    let mut held = HeldWorkers::new(false).await;
    let weak = held.sender.as_ref().unwrap().downgrade();
    let temporary = weak.upgrade().unwrap();
    held.sender = None;
    tokio::time::sleep(Duration::from_millis(1250)).await;
    assert!(!held.recovery.signal().is_stopped());
    assert_eq!(held.identities.len(), 2);
    drop(temporary);
    let observed = held.wait_for_stop().await;
    assert!(weak.upgrade().is_none());
    held.finish().await;
    assert!(observed.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn closed_buffered_input_keeps_the_existing_drain_policy() {
    let mut held = HeldWorkers::new(true).await;
    held.sender = None;
    tokio::time::sleep(Duration::from_millis(1250)).await;
    assert!(!held.recovery.signal().is_stopped());
    assert_eq!(held.identities.len(), 3);
    assert_eq!(held.released.load(Ordering::SeqCst), 0);
    held.finish().await;
}
