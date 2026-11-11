use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore};

use super::checked_block::{CheckedBlock, IntegrityFailure};

/// Integrity checks that run at once, node-wide.
pub const MAX_PARALLEL_RECEIPT_VERIFICATIONS: usize = 2;
/// Integrity checks admitted and not yet finished, node-wide.
pub const MAX_PENDING_RECEIPT_VERIFICATIONS: usize = 512;
/// Integrity checks admitted and not yet finished for one peer. One of them
/// runs at a time.
pub const MAX_PENDING_RECEIPT_VERIFICATIONS_PER_PEER: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptRefusal {
    Invalid(IntegrityFailure),
    Overloaded,
    /// Another copy of the block passed the check, or the node already knows
    /// the block.
    AlreadyKnown,
}

/// The lock for one hash under check. The flag records that a copy passed.
struct HashSlot {
    users: usize,
    lock: Arc<Mutex<bool>>,
}

type HashSlots = DashMap<BlockHash, HashSlot>;

struct HashTicket {
    slots: Arc<HashSlots>,
    hash: BlockHash,
    lock: Arc<Mutex<bool>>,
    guard: Option<OwnedMutexGuard<bool>>,
}

impl Drop for HashTicket {
    fn drop(&mut self) {
        self.guard.take();
        if let Entry::Occupied(mut entry) = self.slots.entry(self.hash.clone()) {
            if Arc::ptr_eq(&entry.get().lock, &self.lock) {
                entry.get_mut().users -= 1;
                if entry.get().users == 0 {
                    entry.remove();
                }
            }
        }
    }
}

struct PeerSlots {
    generation: u64,
    users: usize,
    pending: Arc<Semaphore>,
    running: Arc<Semaphore>,
}

type PeerMap = DashMap<Bytes, PeerSlots>;

struct PeerTicket {
    per_peer: Arc<PeerMap>,
    peer: Bytes,
    generation: u64,
    permits: Vec<OwnedSemaphorePermit>,
}

impl Drop for PeerTicket {
    fn drop(&mut self) {
        self.permits.clear();
        if let Entry::Occupied(mut entry) = self.per_peer.entry(self.peer.clone()) {
            if entry.get().generation == self.generation {
                let slots = entry.get_mut();
                slots.users = slots.users.saturating_sub(1);
                if slots.users == 0 {
                    entry.remove();
                }
            }
        }
    }
}

struct ActiveVerification(Arc<AtomicUsize>);

impl ActiveVerification {
    fn start(active: Arc<AtomicUsize>, peak_active: &AtomicUsize) -> Self {
        let now = active.fetch_add(1, Ordering::AcqRel) + 1;
        peak_active.fetch_max(now, Ordering::AcqRel);
        Self(active)
    }
}

impl Drop for ActiveVerification {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::AcqRel); }
}

/// Runs the integrity check of received block messages, bounded node-wide and
/// per peer, off the async runtime. Copies of one hash are checked one at a
/// time: once a copy passes, the others are skipped, and a forged copy that
/// fails does not keep the honest one out.
pub struct ReceiptVerifier {
    pending: Arc<Semaphore>,
    running: Arc<Semaphore>,
    max_pending_per_peer: usize,
    per_peer: Arc<PeerMap>,
    next_generation: AtomicU64,
    active: Arc<AtomicUsize>,
    peak_active: Arc<AtomicUsize>,
    by_hash: Arc<HashSlots>,
}

impl ReceiptVerifier {
    pub fn new(max_parallel: usize, max_pending: usize, max_pending_per_peer: usize) -> Self {
        Self {
            pending: Arc::new(Semaphore::new(max_pending)),
            running: Arc::new(Semaphore::new(max_parallel)),
            max_pending_per_peer,
            per_peer: Arc::new(DashMap::new()),
            next_generation: AtomicU64::new(0),
            active: Arc::new(AtomicUsize::new(0)),
            peak_active: Arc::new(AtomicUsize::new(0)),
            by_hash: Arc::new(DashMap::new()),
        }
    }

    pub fn shared() -> &'static ReceiptVerifier {
        static VERIFIER: std::sync::LazyLock<ReceiptVerifier> = std::sync::LazyLock::new(|| {
            ReceiptVerifier::new(
                MAX_PARALLEL_RECEIPT_VERIFICATIONS,
                MAX_PENDING_RECEIPT_VERIFICATIONS,
                MAX_PENDING_RECEIPT_VERIFICATIONS_PER_PEER,
            )
        });
        &VERIFIER
    }

    /// Check `block`, sent by `peer`. A block whose hash `known` reports is
    /// skipped.
    pub async fn verify(
        &self,
        peer: &Bytes,
        block: BlockMessage,
        source: &'static str,
        known: impl Fn(&BlockHash) -> bool,
    ) -> Result<CheckedBlock, ReceiptRefusal> {
        self.verify_with(peer, block, known, move |block| {
            CheckedBlock::check(block, source).map_err(|(_, failure)| failure)
        })
        .await
    }

    fn admit(
        &self,
        peer: &Bytes,
        pending: OwnedSemaphorePermit,
    ) -> (PeerTicket, Arc<Semaphore>, Arc<Semaphore>) {
        let (generation, peer_pending, peer_running) = {
            let mut slots = self
                .per_peer
                .entry(peer.clone())
                .or_insert_with(|| PeerSlots {
                    generation: self.next_generation.fetch_add(1, Ordering::Relaxed),
                    users: 0,
                    pending: Arc::new(Semaphore::new(self.max_pending_per_peer)),
                    running: Arc::new(Semaphore::new(1)),
                });
            slots.users += 1;
            (
                slots.generation,
                slots.pending.clone(),
                slots.running.clone(),
            )
        };
        let ticket = PeerTicket {
            per_peer: self.per_peer.clone(),
            peer: peer.clone(),
            generation,
            permits: vec![pending],
        };
        (ticket, peer_pending, peer_running)
    }

    async fn verify_with<F>(
        &self,
        peer: &Bytes,
        block: BlockMessage,
        known: impl Fn(&BlockHash) -> bool,
        check: F,
    ) -> Result<CheckedBlock, ReceiptRefusal>
    where
        F: FnOnce(BlockMessage) -> Result<CheckedBlock, IntegrityFailure> + Send + 'static,
    {
        let Ok(pending) = self.pending.clone().try_acquire_owned() else {
            return Err(ReceiptRefusal::Overloaded);
        };
        let (mut ticket, peer_pending, peer_running) = self.admit(peer, pending);
        let peer_pending = peer_pending
            .try_acquire_owned()
            .map_err(|_| ReceiptRefusal::Overloaded)?;
        ticket.permits.push(peer_pending);

        let hash = block.block_hash.clone();
        let lock = {
            let mut slot = self
                .by_hash
                .entry(hash.clone())
                .or_insert_with(|| HashSlot {
                    users: 0,
                    lock: Arc::new(Mutex::new(false)),
                });
            slot.users += 1;
            slot.lock.clone()
        };
        let mut hash_ticket = HashTicket {
            slots: self.by_hash.clone(),
            hash: hash.clone(),
            lock: lock.clone(),
            guard: None,
        };
        let guard = lock.lock_owned().await;
        if *guard || known(&hash) {
            return Err(ReceiptRefusal::AlreadyKnown);
        }
        hash_ticket.guard = Some(guard);

        let peer_running = peer_running
            .acquire_owned()
            .await
            .map_err(|_| ReceiptRefusal::Overloaded)?;
        ticket.permits.push(peer_running);
        let running = self
            .running
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ReceiptRefusal::Overloaded)?;
        ticket.permits.push(running);

        let active = self.active.clone();
        let peak_active = self.peak_active.clone();
        tokio::task::spawn_blocking(move || {
            let _ticket = ticket;
            let mut hash_ticket = hash_ticket;
            let _active = ActiveVerification::start(active, &peak_active);
            let checked = check(block).map_err(ReceiptRefusal::Invalid)?;
            if let Some(passed) = hash_ticket.guard.as_mut() {
                **passed = true;
            }
            Ok(checked)
        })
        .await
        .unwrap_or_else(|join_error| {
            tracing::error!(error = %join_error, "block integrity check failed to run");
            Err(ReceiptRefusal::Invalid(IntegrityFailure::Malformed))
        })
    }

    pub fn peak_active(&self) -> usize { self.peak_active.load(Ordering::Acquire) }

    pub fn tracked_peers(&self) -> usize { self.per_peer.len() }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::block_implicits::get_random_block_default;

    use super::*;
    use crate::rust::validator_identity::ValidatorIdentity;

    const SOURCE: &str = "test";

    fn check(block: BlockMessage) -> Result<CheckedBlock, IntegrityFailure> {
        CheckedBlock::check(block, SOURCE).map_err(|(_, failure)| failure)
    }

    fn signed_block() -> BlockMessage {
        let (secret_key, _) = Secp256k1.new_key_pair();
        ValidatorIdentity::new(&secret_key).sign_block(&get_random_block_default())
    }

    fn large_unsigned_block() -> BlockMessage {
        let mut large = get_random_block_default();
        large.extra_bytes = vec![0u8; 4 * 1024 * 1024].into();
        large
    }

    async fn flood(
        verifier: Arc<ReceiptVerifier>,
        peers: Vec<(Bytes, BlockMessage)>,
    ) -> Vec<Result<CheckedBlock, ReceiptRefusal>> {
        let tasks: Vec<_> = peers
            .into_iter()
            .map(|(peer, block)| {
                let verifier = verifier.clone();
                tokio::spawn(async move { verifier.verify(&peer, block, SOURCE, |_| false).await })
            })
            .collect();
        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await.expect("verification task"));
        }
        results
    }

    fn count(results: &[Result<CheckedBlock, ReceiptRefusal>], refusal: ReceiptRefusal) -> usize {
        results
            .iter()
            .filter(|result| result.as_ref().err() == Some(&refusal))
            .count()
    }

    async fn hold_verification(
        verifier: Arc<ReceiptVerifier>,
        peer: Bytes,
        block: BlockMessage,
    ) -> (
        tokio::task::JoinHandle<Result<CheckedBlock, ReceiptRefusal>>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            verifier
                .verify_with(
                    &peer,
                    block,
                    |_| false,
                    move |block| {
                        started_tx.send(()).expect("verification started");
                        release_rx
                            .recv_timeout(Duration::from_secs(30))
                            .expect("release verification");
                        check(block)
                    },
                )
                .await
        });
        started_rx.await.expect("verification started");
        (task, release_tx)
    }

    async fn wait_for_pending_permits(verifier: &ReceiptVerifier, available: usize) {
        tokio::time::timeout(Duration::from_secs(30), async {
            while verifier.pending.available_permits() != available {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pending verification state");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn receipt_verification_stays_bounded_under_a_flood_of_invalid_blocks() {
        let attacker = Bytes::from_static(b"attacker");
        let invalid = ReceiptRefusal::Invalid(
            check(large_unsigned_block()).expect_err("an unsigned block fails its check"),
        );

        let single_peer = Arc::new(ReceiptVerifier::new(2, 512, 16));
        let (first, release) = hold_verification(
            single_peer.clone(),
            attacker.clone(),
            large_unsigned_block(),
        )
        .await;
        let waiters: Vec<_> = (0..15)
            .map(|_| {
                let verifier = single_peer.clone();
                let peer = attacker.clone();
                tokio::spawn(async move {
                    verifier
                        .verify(&peer, large_unsigned_block(), SOURCE, |_| false)
                        .await
                })
            })
            .collect();
        wait_for_pending_permits(&single_peer, 496).await;
        let mut results = flood(
            single_peer.clone(),
            (0..184)
                .map(|_| (attacker.clone(), large_unsigned_block()))
                .collect(),
        )
        .await;
        release.send(()).expect("release first check");
        results.push(first.await.expect("first check"));
        for waiter in waiters {
            results.push(waiter.await.expect("pending check"));
        }
        let overloaded = count(&results, ReceiptRefusal::Overloaded);
        assert_eq!(overloaded, 184);
        assert_eq!(overloaded + count(&results, invalid), 200);
        assert_eq!(single_peer.peak_active(), 1);
        assert_eq!(single_peer.tracked_peers(), 0);

        let mixed = Arc::new(ReceiptVerifier::new(2, 512, 16));
        let (first, release) =
            hold_verification(mixed.clone(), attacker.clone(), large_unsigned_block()).await;
        let waiters: Vec<_> = (0..15)
            .map(|_| {
                let verifier = mixed.clone();
                let peer = attacker.clone();
                tokio::spawn(async move {
                    verifier
                        .verify(&peer, large_unsigned_block(), SOURCE, |_| false)
                        .await
                })
            })
            .collect();
        wait_for_pending_permits(&mixed, 496).await;
        let honest = flood(
            mixed.clone(),
            (0..8u8)
                .map(|i| (Bytes::from(vec![i; 20]), signed_block()))
                .collect(),
        )
        .await;
        let flooded = flood(
            mixed.clone(),
            (0..184)
                .map(|_| (attacker.clone(), large_unsigned_block()))
                .collect(),
        )
        .await;
        release.send(()).expect("release first check");
        first.await.expect("first check").expect_err("unsigned");
        for waiter in waiters {
            waiter.await.expect("pending check").expect_err("unsigned");
        }
        assert_eq!(
            honest.iter().filter(|result| result.is_ok()).count(),
            8,
            "honest peers are checked while one peer floods: {honest:?}"
        );
        assert_eq!(count(&flooded, ReceiptRefusal::Overloaded), 184);
        assert!(mixed.peak_active() <= 2);
        assert_eq!(mixed.tracked_peers(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_copies_share_verification_but_a_forged_first_copy_does_not_claim_the_hash()
    {
        let original = signed_block();
        for forged_first in [false, true] {
            let verifier = Arc::new(ReceiptVerifier::new(2, 8, 4));
            let checks = Arc::new(AtomicUsize::new(0));
            let mut first_block = original.clone();
            if forged_first {
                first_block.extra_bytes = Bytes::from_static(b"changed after signing");
            }
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let first = {
                let verifier = verifier.clone();
                let checks = checks.clone();
                tokio::spawn(async move {
                    verifier
                        .verify_with(
                            &Bytes::from_static(b"first"),
                            first_block,
                            |_| false,
                            move |block| {
                                checks.fetch_add(1, Ordering::SeqCst);
                                started_tx.send(()).expect("verification started");
                                release_rx
                                    .recv_timeout(Duration::from_secs(30))
                                    .expect("release");
                                check(block)
                            },
                        )
                        .await
                })
            };
            started_rx.await.expect("first verification started");
            let second = {
                let verifier = verifier.clone();
                let checks = checks.clone();
                let original = original.clone();
                tokio::spawn(async move {
                    verifier
                        .verify_with(
                            &Bytes::from_static(b"second"),
                            original,
                            |_| false,
                            move |block| {
                                checks.fetch_add(1, Ordering::SeqCst);
                                check(block)
                            },
                        )
                        .await
                })
            };
            wait_for_pending_permits(&verifier, 6).await;
            let cancelled = {
                let verifier = verifier.clone();
                let original = original.clone();
                tokio::spawn(async move {
                    verifier
                        .verify(&Bytes::from_static(b"cancelled"), original, SOURCE, |_| {
                            false
                        })
                        .await
                })
            };
            wait_for_pending_permits(&verifier, 5).await;
            cancelled.abort();
            assert!(cancelled.await.expect_err("cancelled task").is_cancelled());
            assert_eq!(verifier.pending.available_permits(), 6);
            release_tx.send(()).expect("release first verification");
            let first = first.await.expect("first task");
            let second = second.await.expect("second task");
            if forged_first {
                assert!(matches!(first, Err(ReceiptRefusal::Invalid(_))));
                assert!(second.is_ok());
                assert_eq!(checks.load(Ordering::SeqCst), 2);
            } else {
                assert!(first.is_ok());
                assert_eq!(second, Err(ReceiptRefusal::AlreadyKnown));
                assert_eq!(checks.load(Ordering::SeqCst), 1);
            }
            assert!(verifier.by_hash.is_empty());
            assert_eq!(verifier.tracked_peers(), 0);
            assert_eq!(verifier.pending.available_permits(), 8);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_known_block_is_not_checked() {
        let verifier = ReceiptVerifier::new(2, 8, 8);
        let block = signed_block();
        let result = verifier
            .verify(&Bytes::from_static(b"peer"), block, SOURCE, |_| true)
            .await;
        assert_eq!(result, Err(ReceiptRefusal::AlreadyKnown));
        assert_eq!(verifier.peak_active(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancelled_caller_keeps_its_verification_slot_until_the_check_ends() {
        let verifier = Arc::new(ReceiptVerifier::new(1, 8, 8));
        let block = get_random_block_default();
        let (first, release_tx) = hold_verification(
            verifier.clone(),
            Bytes::from_static(b"first"),
            block.clone(),
        )
        .await;
        first.abort();
        assert!(first
            .await
            .expect_err("first caller cancelled")
            .is_cancelled());

        let mut other = get_random_block_default();
        other.extra_bytes = Bytes::from_static(b"another block");
        let second = {
            let verifier = verifier.clone();
            tokio::spawn(async move {
                verifier
                    .verify(&Bytes::from_static(b"second"), other, SOURCE, |_| false)
                    .await
            })
        };
        wait_for_pending_permits(&verifier, 6).await;
        assert_eq!(verifier.running.available_permits(), 0);
        assert!(!second.is_finished());

        release_tx.send(()).expect("release first check");
        assert!(second.await.expect("second caller").is_err());
        assert_eq!(verifier.peak_active(), 1);
        assert_eq!(verifier.tracked_peers(), 0);
    }

    #[test]
    fn the_last_verification_ticket_removes_its_peer_entry() {
        let verifier = ReceiptVerifier::new(2, 8, 8);
        let peer = Bytes::from_static(b"peer");
        let pending = || {
            verifier
                .pending
                .clone()
                .try_acquire_owned()
                .expect("pending permit")
        };
        for _ in 0..500 {
            let (first, ..) = verifier.admit(&peer, pending());
            let (second, ..) = verifier.admit(&peer, pending());
            let barrier = std::sync::Barrier::new(3);
            let third = std::thread::scope(|scope| {
                for ticket in [first, second] {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        drop(ticket);
                    });
                }
                scope
                    .spawn(|| {
                        barrier.wait();
                        verifier.admit(&peer, pending()).0
                    })
                    .join()
                    .expect("admission thread")
            });
            drop(third);
            assert_eq!(verifier.tracked_peers(), 0);
        }
    }
}
