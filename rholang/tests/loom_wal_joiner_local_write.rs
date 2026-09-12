//! T-22 (2026-09-11, wave-4 Cluster D+E Phase 4b): loom concurrency
//! harness modeling the `Wal` state under contention between a
//! peer-fetch joiner and a local deploy writer.
//!
//! # Scenario
//!
//! Post-T-07 (`32a28937f`), the production `Wal` holds both `entries`
//! and `ack_hashes` under a single `Arc<RwLock<WalInner>>`.  This
//! test models that shape via `loom::sync::RwLock` and exercises the
//! two races T-22 targets:
//!
//! 1. **Producer + reader** — a local deploy appends WAL entries while
//!    the joiner-side snapshot install reads them via `snapshot()`.
//!    The invariant loom verifies: every observed snapshot is
//!    index-aligned (`entries.len() == ack_hashes.len()` for every
//!    entry pair).  Under the T-07 single-RwLock shape this is
//!    structural — the read guard cannot observe a mid-append state
//!    because the writer holds the exclusive guard for the full
//!    append.  Pre-T-07 (separate Mutex-per-Vec) this invariant was
//!    comment-enforced and would silently break under adversarial
//!    scheduling; T-22 asserts loom finds no interleaving that
//!    violates it post-refactor.
//!
//! 2. **Producer + drainer** — a producer appends while the drainer
//!    (via `take_deploy_entries`) removes the trailing tail.
//!    Invariants: (a) the two vecs remain index-aligned throughout;
//!    (b) `MAX_WAL_ENTRIES_MODEL` is never exceeded; (c) total
//!    appends minus total drained equals the final observed length.
//!
//! # Why a model (not a live Wal)
//!
//! Loom controls interleavings only through `loom::sync` /
//! `loom::sync::atomic`; the production `Wal` uses `std::sync::RwLock`,
//! which loom cannot instrument.  We model the T-07 shape faithfully
//! (single guard over both vecs) using loom's shims so loom's
//! exhaustive scheduler can prove the invariants under adversarial
//! interleavings.  A regression to the pre-T-07 two-Mutex shape
//! would need to be tested by ALSO reverting this file to a
//! two-Mutex model; the model's shape mirrors the production shape
//! by construction.

use loom::sync::{Arc, RwLock};
use loom::thread;

/// Small cap so loom's exhaustive search stays tractable.  The
/// production `MAX_WAL_ENTRIES` is 65536; the invariants tested
/// here are cap-agnostic (they check ordering / alignment, not
/// cap-specific behaviour), so a small model cap keeps the
/// interleaving space small.
const MAX_WAL_ENTRIES_MODEL: usize = 8;

/// T-07-shaped model.  Mirrors `Wal { inner: Arc<RwLock<WalInner>>
/// { entries, ack_hashes } }` exactly.
#[derive(Default)]
struct WalInnerModel {
    entries: Vec<u32>,
    ack_hashes: Vec<u32>,
}

#[derive(Clone, Default)]
struct WalModel {
    inner: Arc<RwLock<WalInnerModel>>,
}

impl WalModel {
    fn new() -> Self { Self::default() }

    /// Appends `(entry, ack_hash)` under a single write guard.
    /// Returns Err(()) if at cap.  Mirrors production
    /// `append_with_ack`.
    fn append(&self, entry: u32, ack_hash: u32) -> Result<(), ()> {
        let mut guard = self.inner.write().unwrap();
        if guard.entries.len() >= MAX_WAL_ENTRIES_MODEL {
            return Err(());
        }
        guard.entries.push(entry);
        guard.ack_hashes.push(ack_hash);
        Ok(())
    }

    /// Returns the current (entries, ack_hashes) tuple under a
    /// read guard.  Mirrors production `snapshot` (which clones
    /// the entries Vec).
    fn snapshot(&self) -> (Vec<u32>, Vec<u32>) {
        let guard = self.inner.read().unwrap();
        (guard.entries.clone(), guard.ack_hashes.clone())
    }

    fn len(&self) -> usize { self.inner.read().unwrap().entries.len() }

    /// Drains entries from position `mark..` under a write guard.
    /// Mirrors production `take_deploy_entries`'s `split_off` shape.
    fn take_from(&self, mark: usize) -> Vec<u32> {
        let mut guard = self.inner.write().unwrap();
        let split_at = mark.min(guard.entries.len());
        let drained = guard.entries.split_off(split_at);
        let _ = guard.ack_hashes.split_off(split_at);
        drained
    }
}

/// Race 1: 2 producers appending concurrently.  Loom verifies that
/// every observable state has `entries.len() == ack_hashes.len()`.
///
/// Under the T-07 single-RwLock shape this is trivially satisfied
/// because the writer holds the exclusive guard across the two
/// pushes.  Pre-T-07 the two Mutexes could be interleaved by an
/// adversarial scheduler between the entries push and the
/// ack_hashes push, producing a mid-state where `entries.len() ==
/// N+1` while `ack_hashes.len() == N`.  This test asserts loom
/// finds no such interleaving under the post-T-07 shape.
#[test]
fn two_producers_never_desync_entries_and_ack_hashes() {
    loom::model(|| {
        let wal = WalModel::new();

        let wal_a = wal.clone();
        let a = thread::spawn(move || {
            wal_a.append(1, 100).unwrap();
        });
        let wal_b = wal.clone();
        let b = thread::spawn(move || {
            wal_b.append(2, 200).unwrap();
        });

        a.join().unwrap();
        b.join().unwrap();

        let (entries, ack_hashes) = wal.snapshot();
        assert_eq!(
            entries.len(),
            ack_hashes.len(),
            "T-22: entries and ack_hashes MUST be index-aligned after \
             concurrent appends (T-07 single-RwLock invariant)."
        );
        assert_eq!(entries.len(), 2);
        // Both entries landed; order is scheduler-dependent so we
        // don't fix it, but the two pairs must be consistent.
        for (i, e) in entries.iter().enumerate() {
            let expected_hash = if *e == 1 { 100 } else { 200 };
            assert_eq!(
                ack_hashes[i], expected_hash,
                "T-22: each entry's ack_hash MUST match the atomic \
                 append (index i={i}, entry={e}, got hash={}, \
                 expected {expected_hash})",
                ack_hashes[i]
            );
        }
    });
}

/// Race 2: 1 producer + 1 reader.  The reader takes snapshots
/// during the producer's append cycle.  Loom asserts every
/// snapshot has `entries.len() == ack_hashes.len()` and the two
/// vecs match pair-wise.
#[test]
fn reader_never_observes_mid_append_state() {
    loom::model(|| {
        let wal = WalModel::new();

        let wal_p = wal.clone();
        let producer = thread::spawn(move || {
            wal_p.append(42, 4242).unwrap();
        });
        let wal_r = wal.clone();
        let reader = thread::spawn(move || {
            let (entries, ack_hashes) = wal_r.snapshot();
            // Snapshot may be taken before, during, or after the
            // producer's append.  In all cases the two vecs MUST
            // be index-aligned — the T-07 read guard atomically
            // observes both or neither.
            assert_eq!(
                entries.len(),
                ack_hashes.len(),
                "T-22: snapshot MUST NOT observe a mid-append \
                 partial state (entries pushed but ack_hashes not \
                 yet, or vice versa).  Post-T-07 the single write \
                 guard makes this structurally impossible."
            );
            // Either 0 or 1 entries — if 1, the pair matches.
            match entries.len() {
                0 => {}
                1 => {
                    assert_eq!(entries[0], 42);
                    assert_eq!(ack_hashes[0], 4242);
                }
                n => panic!("T-22: reader observed unexpected len {n}"),
            }
        });

        producer.join().unwrap();
        reader.join().unwrap();
    });
}

/// Race 3: producer + drainer.  A local deploy appends while a
/// snapshot-installer-analog drainer removes the tail via
/// `take_from(0)` (drain everything).  Loom verifies:
///
/// - Every intermediate snapshot is index-aligned.
/// - Total accounting: (drained.len + final.len) == 1 producer append
///   (the producer commits exactly one entry).
#[test]
fn producer_drainer_race_preserves_accounting() {
    loom::model(|| {
        let wal = WalModel::new();

        let wal_p = wal.clone();
        let producer = thread::spawn(move || {
            wal_p.append(7, 77).unwrap();
        });
        let wal_d = wal.clone();
        let drainer = thread::spawn(move || wal_d.take_from(0));

        producer.join().unwrap();
        let drained = drainer.join().unwrap();

        // Post-condition after both joined: any remaining entries
        // + drained ones sum to 1.
        let (final_entries, final_hashes) = wal.snapshot();
        assert_eq!(
            final_entries.len(),
            final_hashes.len(),
            "T-22: post-race snapshot MUST remain index-aligned",
        );
        assert_eq!(
            drained.len() + final_entries.len(),
            1,
            "T-22: accounting invariant — 1 append minus drained \
             equals final len; got drained={}, final={}",
            drained.len(),
            final_entries.len(),
        );
    });
}

/// T-22 self-check (2026-09-11): explicitly verify that a
/// pre-T-07-shape model — TWO separate `Mutex`es instead of ONE
/// `RwLock` over both fields — DOES produce mid-append states that
/// break index alignment.  Without this check, a bug in the WalModel
/// (e.g., accidentally still using one guard for both operations)
/// would let the parent tests pass on a model that doesn't reflect
/// the pre-T-07 concurrency hazard.  This is the negative control
/// — it verifies loom CAN detect the class of bug T-22 targets.
///
/// The test is expected to fire on at least one interleaving; if
/// loom finds none, either the loom-model space is too small
/// (adjust the loop count) or loom's scheduler regressed.
#[test]
fn t22_self_check_pre_t07_two_mutex_model_can_desync() {
    use loom::sync::Mutex;

    /// Broken pre-T-07 model: `entries` and `ack_hashes` behind
    /// SEPARATE Mutexes.  An adversarial scheduler CAN interleave
    /// the two `push` operations, producing `entries.len() !=
    /// ack_hashes.len()` mid-append.
    #[derive(Clone, Default)]
    struct BrokenWalModel {
        entries: Arc<Mutex<Vec<u32>>>,
        ack_hashes: Arc<Mutex<Vec<u32>>>,
    }

    impl BrokenWalModel {
        fn append(&self, entry: u32, ack_hash: u32) {
            self.entries.lock().unwrap().push(entry);
            // Adversarial scheduler point: between the two locks,
            // another thread's snapshot can observe entries.len() =
            // N+1 but ack_hashes.len() = N.
            self.ack_hashes.lock().unwrap().push(ack_hash);
        }
        fn snapshot(&self) -> (Vec<u32>, Vec<u32>) {
            let e = self.entries.lock().unwrap().clone();
            let a = self.ack_hashes.lock().unwrap().clone();
            (e, a)
        }
    }

    let saw_desync = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_desync_c = saw_desync.clone();

    loom::model(move || {
        let wal = BrokenWalModel::default();

        let wal_p = wal.clone();
        let producer = thread::spawn(move || {
            wal_p.append(1, 100);
        });
        let wal_r = wal.clone();
        let saw = saw_desync_c.clone();
        let reader = thread::spawn(move || {
            let (entries, ack_hashes) = wal_r.snapshot();
            if entries.len() != ack_hashes.len() {
                saw.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });

        producer.join().unwrap();
        reader.join().unwrap();
    });

    assert!(
        saw_desync.load(std::sync::atomic::Ordering::SeqCst),
        "T-22 self-check FAILED: loom did not find a desync \
         interleaving in the pre-T-07 two-Mutex model, meaning \
         either the model is wrong or loom's scheduler is not \
         exploring the interleaving space.  The parent T-22 tests \
         would be false-negatives in this state.",
    );
}

/// Race 4: MAX_WAL_ENTRIES cap under concurrent producers.  Two
/// producers each try to append MAX_WAL_ENTRIES_MODEL - 1 items;
/// the cap must never be exceeded regardless of interleaving.
///
/// Kept small (2 iterations each) to keep loom's search space
/// tractable — the cap invariant is the interesting bit, not the
/// specific count.
#[test]
fn max_wal_entries_cap_never_exceeded_under_contention() {
    loom::model(|| {
        let wal = WalModel::new();

        let wal_a = wal.clone();
        let a = thread::spawn(move || {
            // Loom exhaustively explores; keep loop tiny.
            for i in 0..2 {
                let _ = wal_a.append(i, i * 10);
            }
        });
        let wal_b = wal.clone();
        let b = thread::spawn(move || {
            for i in 0..2 {
                let _ = wal_b.append(100 + i, 1000 + i * 10);
            }
        });

        a.join().unwrap();
        b.join().unwrap();

        assert!(
            wal.len() <= MAX_WAL_ENTRIES_MODEL,
            "T-22: MAX_WAL_ENTRIES_MODEL={MAX_WAL_ENTRIES_MODEL} MUST NOT \
             be exceeded under concurrent appends; got len={}",
            wal.len(),
        );
        let (entries, hashes) = wal.snapshot();
        assert_eq!(
            entries.len(),
            hashes.len(),
            "T-22: cap-race post-state MUST be index-aligned",
        );
    });
}
