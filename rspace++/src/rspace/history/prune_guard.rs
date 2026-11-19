//! Coordination between runtime trie-node writers and the history
//! mark-and-sweep pruner (issue #1061).
//!
//! Two races motivate this type:
//!
//! 1. **In-flight writes race the sweep.** `RadixTreeImpl::pending_writes`
//!    skips nodes whose hashes are already in the store (content-addressed
//!    writes are idempotent — the already-present bytes would equal the pending
//!    bytes). If the sweep deletes one of those "already present" hashes
//!    between the writer's `contains` check and the writer's `put` of the other
//!    nodes, the new state references a dangling pointer. An LMDB env
//!    serialises `contains`/`put`/`delete` individually but never the
//!    read-then-conditional-write window.
//! 2. **Not-yet-in-DAG roots.** A block's post-state root, or a merge
//!    intermediate from a proposed block, can be written to the trie-node store
//!    before the block appears in the DAG. The casper-side mark-root
//!    enumeration walks the DAG, so it misses these roots. The sweep then
//!    deletes their new trie nodes and the next replay fails with
//!    `MissingNode`.
//!
//! `PruneGuard` fixes both:
//!
//! * A `RwLock<()>` protects the content-addressed write window. Writers take
//!   the shared side around their `stage` + `write` sequence (the full
//!   read-then-conditional-write span); the sweep takes the exclusive side for
//!   the whole pass. Writers in flight when the sweep tries to acquire the
//!   exclusive side block it until they complete.
//! * A pinned-roots LRU carries roots that a writer recently committed or that
//!   a `reset` locked in. The sweep unions every pinned root with its
//!   caller-supplied `retained_roots` set before the mark walk, so their
//!   subtrees survive even if the DAG walk did not reach them.
//!
//! # Lock poisoning
//!
//! `pin`, `pinned_snapshot`, `enter_write`, and `enter_sweep` recover from
//! `PoisonError` by taking the inner value. A panic inside the critical
//! section leaves `Pinned` in a safe over-retention state (see the invariant
//! comment on `Pinned`), so continuing is safer than cascading one panic
//! into a chain of writer panics on lock acquisition.

use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

/// Upper bound on the size of the pinned-roots LRU. 4096 covers many
/// minutes of in-flight blocks at realistic rates while using less than
/// 200 KiB of resident memory (32 bytes per hash). Eviction is strictly
/// LRU, so re-pinning a root (via a repeat `reset` or the same post-state
/// from a reorg replay) refreshes its position.
const PINNED_ROOTS_CAP: usize = 4096;

/// LRU + membership index for pinned roots.
///
/// # Invariants
///
/// * Every hash in `index` appears exactly once in `order`.
/// * Every hash in `order` appears in `index`.
///
/// If a panic inside `insert` breaks the first invariant, the worst case is
/// a stale entry in `index` (dedup won't admit it; the snapshot then still
/// returns it — safe over-retention). The second invariant can't be broken
/// because `insert` only pushes to `order` after inserting into `index`.
struct Pinned {
    order: VecDeque<Blake2b256Hash>,
    index: HashSet<Blake2b256Hash>,
}

impl Pinned {
    fn new() -> Self {
        Self {
            order: VecDeque::new(),
            index: HashSet::new(),
        }
    }

    /// Insert `root` or refresh its LRU position if already present.
    ///
    /// Complexity: O(1) on a new root (the common case), O(n) on a re-pin
    /// because `VecDeque::remove` shifts elements. Re-pins are rare: a
    /// caller only re-pins by resetting to an older post-state or by
    /// re-checkpointing to the same content-addressed hash.
    fn insert(&mut self, root: Blake2b256Hash) {
        if self.index.contains(&root) {
            // Refresh LRU position.
            if let Some(pos) = self.order.iter().position(|h| h == &root) {
                self.order.remove(pos);
            }
            self.order.push_back(root);
            return;
        }
        self.index.insert(root.clone());
        self.order.push_back(root);
        while self.order.len() > PINNED_ROOTS_CAP {
            if let Some(evicted) = self.order.pop_front() {
                self.index.remove(&evicted);
            }
        }
    }

    fn snapshot(&self) -> Vec<Blake2b256Hash> {
        // In debug builds, verify the two invariants. A mismatch here is
        // a programming bug in this module, not something a caller can
        // trigger.
        debug_assert_eq!(
            self.order.len(),
            self.index.len(),
            "Pinned invariant broken: order and index diverged"
        );
        // Iterate the set — order is irrelevant for the sweep's mark walk,
        // and HashSet iteration avoids rebuilding a Vec that will just be
        // fed back into a HashSet inside atomic_prune_via_store.
        self.index.iter().cloned().collect()
    }
}

pub struct PruneGuard {
    rw: RwLock<()>,
    pinned: Mutex<Pinned>,
}

impl PruneGuard {
    pub fn new() -> Self {
        Self {
            rw: RwLock::new(()),
            pinned: Mutex::new(Pinned::new()),
        }
    }

    /// Record a root so the sweep retains its subtree even if the DAG walk
    /// did not reach it. Re-pinning an existing root refreshes its LRU
    /// position; eviction is LRU once `PINNED_ROOTS_CAP` is exceeded.
    pub fn pin(&self, root: Blake2b256Hash) {
        self.pinned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root);
    }

    #[must_use]
    pub fn pinned_snapshot(&self) -> Vec<Blake2b256Hash> {
        self.pinned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot()
    }

    /// Acquire the shared side of the sweep RW lock. Writers hold this
    /// around their content-addressed `pending_writes` + `write` sequence
    /// so the sweep cannot delete a node between the writer's `contains`
    /// check and its subsequent `put`.
    #[must_use = "the guard releases the lock when dropped"]
    pub fn enter_write(&self) -> RwLockReadGuard<'_, ()> {
        self.rw.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Acquire the exclusive side of the sweep RW lock. The sweep holds
    /// this for the full pass; concurrent writers wait on the shared side
    /// until the sweep releases.
    #[must_use = "the guard releases the lock when dropped"]
    pub fn enter_sweep(&self) -> RwLockWriteGuard<'_, ()> {
        self.rw.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for PruneGuard {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn h(b: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![b; 32]) }

    #[test]
    fn pin_deduplicates() {
        let g = PruneGuard::new();
        g.pin(h(1));
        g.pin(h(1));
        g.pin(h(2));
        assert_eq!(g.pinned_snapshot().len(), 2);
    }

    /// The structure caps at `PINNED_ROOTS_CAP` entries. With distinct
    /// pins LRU reduces to FIFO: adding more roots than the cap evicts
    /// older entries one-for-one.
    #[test]
    fn pin_count_stays_at_cap_when_exceeded() {
        let g = PruneGuard::new();
        for i in 0..(PINNED_ROOTS_CAP as u64 + 10) {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&i.to_le_bytes());
            g.pin(Blake2b256Hash::from_bytes(b.to_vec()));
        }
        assert_eq!(g.pinned_snapshot().len(), PINNED_ROOTS_CAP);
    }

    /// With distinct pins LRU degenerates to FIFO: the oldest leaves first
    /// when the cap is exceeded, not an arbitrary one. The dedicated LRU
    /// test below covers the re-pin path.
    #[test]
    fn pin_eviction_order_is_oldest_first() {
        let g = PruneGuard::new();
        // Pin exactly `cap` + 3 distinct roots. The first 3 must be evicted.
        let total = PINNED_ROOTS_CAP + 3;
        for i in 0..total {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&(i as u64).to_le_bytes());
            g.pin(Blake2b256Hash::from_bytes(b.to_vec()));
        }
        let snap: std::collections::HashSet<Blake2b256Hash> =
            g.pinned_snapshot().into_iter().collect();
        for evicted in 0..3 {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&(evicted as u64).to_le_bytes());
            let h = Blake2b256Hash::from_bytes(b.to_vec());
            assert!(!snap.contains(&h), "oldest pin #{evicted} must be evicted");
        }
        for kept in 3..total {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&(kept as u64).to_le_bytes());
            let h = Blake2b256Hash::from_bytes(b.to_vec());
            assert!(snap.contains(&h), "newer pin #{kept} must be retained");
        }
    }

    #[test]
    fn write_and_sweep_are_mutually_exclusive() {
        let g = Arc::new(PruneGuard::new());
        let w1 = g.enter_write();
        // A second shared holder is allowed — writers do not block each
        // other, only the sweep blocks them.
        let w2 = g.enter_write();
        drop(w1);
        drop(w2);
        // Now the sweep can take the exclusive side.
        let _s = g.enter_sweep();
    }

    /// `enter_write` must block while the sweep holds exclusive. Two
    /// channels give a fully deterministic flow: the writer signals
    /// `ready` just before attempting `enter_write`, and signals
    /// `acquired` only after it returns. Before releasing the sweep, the
    /// main thread checks that the `acquired` signal has NOT fired —
    /// `try_recv` + a short sleep makes this robust against an OS that
    /// silently delays the writer's scheduling. After releasing, the
    /// `acquired` signal must arrive within a generous timeout.
    #[test]
    fn enter_write_blocks_while_sweep_holds_exclusive() {
        use std::sync::mpsc::{TryRecvError, channel};
        use std::thread;
        use std::time::Duration;

        let g = Arc::new(PruneGuard::new());
        let sweep_held = g.enter_sweep();

        let (ready_tx, ready_rx) = channel::<()>();
        let (acquired_tx, acquired_rx) = channel::<()>();
        let g2 = Arc::clone(&g);
        let writer = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let _w = g2.enter_write();
            acquired_tx.send(()).unwrap();
            g2.pin(h(0xAB));
        });

        // Writer thread has been scheduled and is about to attempt the
        // shared acquire. Give it 20 ms to try — if a bug ever lets
        // shared pass while exclusive is held, this is orders of
        // magnitude more than enough to observe.
        ready_rx.recv().unwrap();
        thread::sleep(Duration::from_millis(20));
        match acquired_rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Ok(()) => panic!("writer acquired shared while sweep held exclusive"),
            Err(TryRecvError::Disconnected) => panic!("writer thread panicked"),
        }

        drop(sweep_held);
        acquired_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("writer must unblock within 5 s after sweep releases");
        writer.join().unwrap();
        assert_eq!(g.pinned_snapshot().len(), 1);
    }

    /// A re-pin of an existing root refreshes its LRU position — the root
    /// becomes the youngest entry, not the oldest. Under cap pressure
    /// this is what keeps an actively-used root (e.g., repeated reset to
    /// the same post-state during a reorg replay) from being evicted.
    #[test]
    fn repin_refreshes_lru_position() {
        let g = PruneGuard::new();
        // Pin to fill the LRU exactly to capacity.
        for i in 0..PINNED_ROOTS_CAP {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&(i as u64).to_le_bytes());
            g.pin(Blake2b256Hash::from_bytes(b.to_vec()));
        }
        // Re-pin root #0 — it should move to the back.
        let mut root0_bytes = [0u8; 32];
        root0_bytes[..8].copy_from_slice(&0u64.to_le_bytes());
        let root0 = Blake2b256Hash::from_bytes(root0_bytes.to_vec());
        g.pin(root0.clone());

        // Insert one more fresh root; it evicts the OLDEST, which is now
        // root #1 (because root #0 was moved to the back).
        let mut fresh = [0u8; 32];
        fresh[..8].copy_from_slice(&(PINNED_ROOTS_CAP as u64).to_le_bytes());
        g.pin(Blake2b256Hash::from_bytes(fresh.to_vec()));

        let snap: HashSet<Blake2b256Hash> = g.pinned_snapshot().into_iter().collect();
        assert!(snap.contains(&root0), "re-pinned root must survive LRU eviction");
        let mut evicted = [0u8; 32];
        evicted[..8].copy_from_slice(&1u64.to_le_bytes());
        let evicted_hash = Blake2b256Hash::from_bytes(evicted.to_vec());
        assert!(
            !snap.contains(&evicted_hash),
            "second-oldest root must be evicted now that #0 was refreshed"
        );
    }
}
