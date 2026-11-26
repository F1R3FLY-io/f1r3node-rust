//! Coordination between runtime trie-node writers and the history
//! mark-and-sweep pruner (issues #1061, #1098).
//!
//! Three races motivate this type:
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
//! 3. **Stale pins keep the mark set too large (issue #1098).** A pure
//!    count-bounded LRU of up to `PINNED_ROOTS_CAP` roots keeps load-period
//!    roots pinned long after their blocks reached the DAG. At idle the DAG
//!    walk alone would retain a few roots, but every pinned root also drags its
//!    full subtree into the mark set. A two-hour soak showed ~1M nodes still
//!    marked at idle, so pruning barely reduced growth.
//!
//! `PruneGuard` fixes all three:
//!
//! * A `RwLock<()>` protects the content-addressed write window. Writers take
//!   the shared side around their `stage` + `write` sequence (the full
//!   read-then-conditional-write span); the sweep takes the exclusive side in
//!   short bursts (once at pass start to quiesce in-flight writers, and once
//!   per delete batch) rather than for the whole pass.
//! * A pinned-roots LRU carries roots that a writer recently committed or that
//!   a `reset` locked in. Each pin also records the sweep generation when it
//!   landed. `begin_pass` bumps the generation and drops pins older than
//!   `retention_passes` passes, so a pin from a block that has since reached
//!   the DAG does not keep its subtree pinned forever.
//! * The sweep unions every pinned root with its caller-supplied
//!   `retained_roots` set before the mark walk, so their subtrees survive even
//!   if the DAG walk did not reach them.
//!
//! # Lock poisoning
//!
//! `pin`, `pinned_snapshot`, `enter_write`, `enter_sweep`, and `begin_pass`
//! recover from `PoisonError` by taking the inner value. A panic inside the
//! critical section leaves `Pinned` in a safe over-retention state (see the
//! invariant comment on `Pinned`), so continuing is safer than cascading one
//! panic into a chain of writer panics on lock acquisition.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

/// Upper bound on the size of the pinned-roots LRU. 4096 covers many
/// minutes of in-flight blocks at realistic rates while using less than
/// 200 KiB of resident memory (40 bytes per entry — a hash plus a u64
/// generation). Eviction is LRU on count, and generation-based on age;
/// re-pinning a root refreshes both.
const PINNED_ROOTS_CAP: usize = 4096;

/// Sentinel used for the "pass start" generation before the first pass.
/// Pins recorded before any pass are stamped with `INITIAL_GENERATION` and
/// are evicted by the first `begin_pass` whose `retention_passes` argument
/// is finite and smaller than the current generation.
const INITIAL_GENERATION: u64 = 0;

/// LRU + membership index for pinned roots.
///
/// # Invariants
///
/// * Every hash in `generations` appears exactly once in `order`.
/// * Every hash in `order` appears in `generations`.
///
/// If a panic inside `insert` breaks the first invariant, the worst case is
/// a stale entry in `generations` (dedup won't admit it; the snapshot then
/// still returns it — safe over-retention). The second invariant can't be
/// broken because `insert` only pushes to `order` after inserting into
/// `generations`.
struct Pinned {
    order: VecDeque<Blake2b256Hash>,
    generations: HashMap<Blake2b256Hash, u64>,
}

impl Pinned {
    fn new() -> Self {
        Self {
            order: VecDeque::new(),
            generations: HashMap::new(),
        }
    }

    /// Insert `root` at the given generation, or refresh its LRU position
    /// and generation if already present.
    ///
    /// Complexity: O(1) on a new root (the common case), O(n) on a re-pin
    /// because `VecDeque::remove` shifts elements. Re-pins are rare: a
    /// caller only re-pins by resetting to an older post-state or by
    /// re-checkpointing to the same content-addressed hash.
    fn insert(&mut self, root: Blake2b256Hash, generation: u64) {
        if self.generations.contains_key(&root) {
            if let Some(pos) = self.order.iter().position(|h| h == &root) {
                self.order.remove(pos);
            }
            self.order.push_back(root.clone());
            self.generations.insert(root, generation);
            return;
        }
        self.generations.insert(root.clone(), generation);
        self.order.push_back(root);
        while self.order.len() > PINNED_ROOTS_CAP {
            if let Some(evicted) = self.order.pop_front() {
                self.generations.remove(&evicted);
            }
        }
    }

    /// Remove pins strictly older than `min_generation`. Returns the count
    /// evicted so the caller can record it in metrics.
    fn evict_older_than(&mut self, min_generation: u64) -> usize {
        let mut evicted = 0usize;
        // Build the retained-order deque by walking the current order and
        // keeping the entries whose generation is current enough. Doing it
        // this way (O(n)) rather than by repeatedly calling `remove` keeps
        // eviction linear even when many pins are stale at once.
        let kept: VecDeque<Blake2b256Hash> = std::mem::take(&mut self.order)
            .into_iter()
            .filter(|h| match self.generations.get(h) {
                Some(&g) if g >= min_generation => true,
                _ => {
                    self.generations.remove(h);
                    evicted += 1;
                    false
                }
            })
            .collect();
        self.order = kept;
        evicted
    }

    fn snapshot(&self) -> Vec<Blake2b256Hash> {
        debug_assert_eq!(
            self.order.len(),
            self.generations.len(),
            "Pinned invariant broken: order and generations diverged"
        );
        // Iterate the map — order is irrelevant for the sweep's mark walk,
        // and HashMap iteration avoids rebuilding a Vec that will just be
        // fed back into a HashSet inside atomic_prune_via_store_batched.
        self.generations.keys().cloned().collect()
    }

    fn len(&self) -> usize { self.generations.len() }
}

pub struct PruneGuard {
    rw: RwLock<()>,
    pinned: Mutex<Pinned>,
    /// Monotonic pass counter. `begin_pass` increments it and returns the
    /// new value; `pin` stamps the current value onto new pins. Writes to
    /// this counter happen under `pinned`'s Mutex so a `pin` and a
    /// concurrent `begin_pass` cannot stamp the new pin with a stale
    /// generation.
    generation: AtomicU64,
}

/// Counters a `begin_pass` call hands back so the caller can log how much
/// age-based eviction trimmed from the pinned FIFO (issue #1098
/// observability).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassStart {
    pub generation: u64,
    pub pins_before: usize,
    pub pins_evicted: usize,
    pub pins_after: usize,
}

impl PruneGuard {
    pub fn new() -> Self {
        Self {
            rw: RwLock::new(()),
            pinned: Mutex::new(Pinned::new()),
            generation: AtomicU64::new(INITIAL_GENERATION),
        }
    }

    /// Record a root so the sweep retains its subtree even if the DAG walk
    /// did not reach it. Re-pinning an existing root refreshes its LRU
    /// position AND its generation; eviction is LRU once `PINNED_ROOTS_CAP`
    /// is exceeded, and generation-based on `begin_pass`.
    pub fn pin(&self, root: Blake2b256Hash) {
        let gen = self.generation.load(Ordering::Acquire);
        self.pinned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root, gen);
    }

    #[must_use]
    pub fn pinned_snapshot(&self) -> Vec<Blake2b256Hash> {
        self.pinned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot()
    }

    /// Begin a new sweep pass: bump the generation counter and evict any
    /// pin older than `retention_passes` passes. Returns a struct with
    /// before/after counts so the caller can log and emit metrics.
    ///
    /// `retention_passes = 0` would evict every pin from before this pass;
    /// use a value of at least 1 in production so a pin from a writer that
    /// committed between passes still survives into the next sweep.
    pub fn begin_pass(&self, retention_passes: u64) -> PassStart {
        // Hold the pinned mutex across the generation bump AND the
        // eviction so no concurrent `pin` can race in between and get the
        // wrong generation. (Writers hold `enter_write` but not this
        // Mutex.) Minimum hold time: one atomic add + one HashMap walk.
        let mut pinned = self.pinned.lock().unwrap_or_else(PoisonError::into_inner);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let pins_before = pinned.len();
        // `min_generation` guards against underflow when a brand-new guard
        // ticks its first pass before any pin has been recorded.
        let min_generation = generation.saturating_sub(retention_passes);
        let pins_evicted = pinned.evict_older_than(min_generation);
        PassStart {
            generation,
            pins_before,
            pins_evicted,
            pins_after: pinned.len(),
        }
    }

    /// Current generation. Call `begin_pass` to bump it; this accessor is
    /// a read-only observation used by tests.
    pub fn generation(&self) -> u64 { self.generation.load(Ordering::Acquire) }

    /// Acquire the shared side of the sweep RW lock. Writers hold this
    /// around their content-addressed `pending_writes` + `write` sequence
    /// so the sweep cannot delete a node between the writer's `contains`
    /// check and its subsequent `put`.
    #[must_use = "the guard releases the lock when dropped"]
    pub fn enter_write(&self) -> RwLockReadGuard<'_, ()> {
        self.rw.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Acquire the exclusive side of the sweep RW lock. The sweep takes
    /// this briefly — once at pass start to quiesce in-flight writers for
    /// a consistent pin snapshot, and once per delete batch — rather than
    /// for the whole pass. Concurrent writers wait on the shared side
    /// until the current holder releases.
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

        let snap: std::collections::HashSet<Blake2b256Hash> =
            g.pinned_snapshot().into_iter().collect();
        assert!(snap.contains(&root0), "re-pinned root must survive LRU eviction");
        let mut evicted = [0u8; 32];
        evicted[..8].copy_from_slice(&1u64.to_le_bytes());
        let evicted_hash = Blake2b256Hash::from_bytes(evicted.to_vec());
        assert!(
            !snap.contains(&evicted_hash),
            "second-oldest root must be evicted now that #0 was refreshed"
        );
    }

    /// Age-based eviction (issue #1098). Pins recorded before the previous
    /// pass are dropped at the start of the next pass with retention = 1.
    /// This is the lever that keeps the mark set from growing without
    /// bound under long-running load.
    #[test]
    fn begin_pass_evicts_pins_older_than_retention() {
        let g = PruneGuard::new();
        // Pass 1: pin three roots.
        g.begin_pass(1);
        g.pin(h(1));
        g.pin(h(2));
        g.pin(h(3));
        // Pass 2: pin one more. Previous-pass pins must survive (retention = 1).
        g.begin_pass(1);
        g.pin(h(4));
        let snap: std::collections::HashSet<Blake2b256Hash> =
            g.pinned_snapshot().into_iter().collect();
        for i in 1..=4 {
            assert!(snap.contains(&h(i)), "pin h({i}) must survive retention = 1 across one pass");
        }
        // Pass 3: evict pins older than this pass by 1 — i.e., pass-1 pins
        // (h(1), h(2), h(3)) are now too old and must be dropped. The pass-2
        // pin h(4) survives one more pass.
        let start = g.begin_pass(1);
        assert_eq!(start.pins_evicted, 3);
        let snap: std::collections::HashSet<Blake2b256Hash> =
            g.pinned_snapshot().into_iter().collect();
        for i in 1..=3 {
            assert!(!snap.contains(&h(i)), "pin h({i}) must be evicted after 2 passes");
        }
        assert!(snap.contains(&h(4)));
    }

    /// Re-pinning refreshes generation too, so an active root survives
    /// age-based eviction across many passes.
    #[test]
    fn repin_refreshes_generation() {
        let g = PruneGuard::new();
        g.begin_pass(1);
        g.pin(h(1));
        // Several passes go by. Each pass re-pins h(1) (e.g., a reset that
        // keeps hitting the same post-state during replay).
        for _ in 0..5 {
            g.begin_pass(1);
            g.pin(h(1));
        }
        // One more pass with retention = 1 would evict pins older than
        // that pass. h(1) was re-pinned in the previous pass, so it stays.
        let start = g.begin_pass(1);
        assert_eq!(start.pins_evicted, 0);
        assert!(g.pinned_snapshot().contains(&h(1)));
    }

    /// `begin_pass` counts the pin population before and after eviction so
    /// the caller can log it (issue #1098 observability action).
    #[test]
    fn begin_pass_reports_counts() {
        let g = PruneGuard::new();
        g.begin_pass(1);
        g.pin(h(1));
        g.pin(h(2));
        g.begin_pass(1);
        g.pin(h(3));
        let start = g.begin_pass(1);
        assert_eq!(start.pins_before, 3);
        assert_eq!(start.pins_evicted, 2, "h(1) and h(2) are two passes old");
        assert_eq!(start.pins_after, 1);
        assert_eq!(start.generation, 3);
    }
}
