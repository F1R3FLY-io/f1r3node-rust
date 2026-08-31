//! Exhaustive Loom schedules for `Arc<AnchorRoot<PathMap<V>>>` ownership.
//!
//! This is a structural refinement of `PathMapAnchorRootTeardown.tla`. Each
//! logical immutable PathMap version owns one `AnchorRoot`, represented here by
//! an `AnchorRoot` with one raw-header token. Wrapper cloning clones only its
//! `Arc`. The final Arc transition invokes `AnchorRoot::drop`, which consumes
//! the raw-header token and elects exactly one reverse-prune PDA. A shared
//! semantic mutation creates and registers a fresh anchor before it becomes
//! visible. The actual zipper traversal is verified separately by Rocq, Verus,
//! and production PathMap tests; duplicating topology in this model would blur
//! the concurrency boundary rather than strengthen it.

use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

const OLD: usize = 0;
const FRESH_A: usize = 1;
const FRESH_B: usize = 2;
const SLOT_COUNT: usize = 3;

struct SlotLedger {
    registered: AtomicUsize,
    live_roots: AtomicUsize,
    live_raw_headers: AtomicUsize,
    pda_teardowns: AtomicUsize,
    raw_header_teardowns: AtomicUsize,
}

impl SlotLedger {
    fn new() -> Self {
        Self {
            registered: AtomicUsize::new(0),
            live_roots: AtomicUsize::new(0),
            live_raw_headers: AtomicUsize::new(0),
            pda_teardowns: AtomicUsize::new(0),
            raw_header_teardowns: AtomicUsize::new(0),
        }
    }
}

struct Ledger {
    slots: [SlotLedger; SLOT_COUNT],
}

impl Ledger {
    fn new() -> Self {
        Self {
            slots: [SlotLedger::new(), SlotLedger::new(), SlotLedger::new()],
        }
    }

    fn register(&self, slot: usize) {
        let slot = &self.slots[slot];
        assert_eq!(
            slot.registered.fetch_add(1, Ordering::SeqCst),
            0,
            "a logical version is registered exactly once"
        );
        assert_eq!(
            slot.live_roots.fetch_add(1, Ordering::SeqCst),
            0,
            "one AnchorRoot per logical version"
        );
        assert_eq!(
            slot.live_raw_headers.fetch_add(1, Ordering::SeqCst),
            0,
            "one raw PathMap header per active anchor"
        );
    }

    fn teardown(&self, slot: usize) {
        let slot = &self.slots[slot];
        assert_eq!(
            slot.live_roots.fetch_sub(1, Ordering::SeqCst),
            1,
            "only the final Arc may destroy the registered AnchorRoot"
        );
        assert_eq!(
            slot.live_raw_headers.fetch_sub(1, Ordering::SeqCst),
            1,
            "the final anchor owns exactly one raw header"
        );
        assert_eq!(
            slot.pda_teardowns.fetch_add(1, Ordering::SeqCst),
            0,
            "exactly one reverse-prune PDA per dead anchor"
        );
        assert_eq!(
            slot.raw_header_teardowns.fetch_add(1, Ordering::SeqCst),
            0,
            "the raw header is consumed exactly once"
        );
    }

    fn assert_closed(&self, expected_slots: &[usize]) {
        for (index, slot) in self.slots.iter().enumerate() {
            let expected = usize::from(expected_slots.contains(&index));
            assert_eq!(slot.registered.load(Ordering::SeqCst), expected);
            assert_eq!(slot.live_roots.load(Ordering::SeqCst), 0);
            assert_eq!(slot.live_raw_headers.load(Ordering::SeqCst), 0);
            assert_eq!(slot.pda_teardowns.load(Ordering::SeqCst), expected);
            assert_eq!(slot.raw_header_teardowns.load(Ordering::SeqCst), expected);
        }
    }
}

struct AnchorRoot {
    ledger: Arc<Ledger>,
    slot: usize,
}

impl Drop for AnchorRoot {
    fn drop(&mut self) { self.ledger.teardown(self.slot); }
}

struct Anchor {
    root: Arc<AnchorRoot>,
}

impl Anchor {
    fn registered(ledger: Arc<Ledger>, slot: usize) -> Self {
        // Registration precedes publication of the owning Arc. This is the
        // runtime obligation that kills the unregistered-make_mut mutant.
        ledger.register(slot);
        Self {
            root: Arc::new(AnchorRoot { ledger, slot }),
        }
    }

    fn fork_shared_version(&mut self, slot: usize) {
        let ledger = Arc::clone(&self.root.ledger);
        let fresh = Self::registered(ledger, slot);
        // Assignment drops this wrapper's old Arc only after the fresh anchor
        // is completely registered. Other aliases keep the old version alive.
        *self = fresh;
    }
}

impl Clone for Anchor {
    fn clone(&self) -> Self {
        Self {
            root: Arc::clone(&self.root),
        }
    }
}

#[test]
fn two_and_three_alias_final_drops_elect_one_teardown() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        let anchor = Anchor::registered(Arc::clone(&ledger), OLD);
        let second = anchor.clone();
        let third = anchor.clone();

        let a = thread::spawn(move || drop(second));
        let b = thread::spawn(move || drop(third));
        drop(anchor);
        a.join().unwrap();
        b.join().unwrap();

        ledger.assert_closed(&[OLD]);
    });
}

#[test]
fn clone_and_drop_schedules_never_create_a_second_raw_header() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        let anchor = Anchor::registered(Arc::clone(&ledger), OLD);
        let drop_alias = anchor.clone();
        let clone_alias = anchor.clone();

        let dropper = thread::spawn(move || drop(drop_alias));
        let cloner = thread::spawn(move || {
            let transient = clone_alias.clone();
            drop(clone_alias);
            drop(transient);
        });
        drop(anchor);
        dropper.join().unwrap();
        cloner.join().unwrap();

        ledger.assert_closed(&[OLD]);
    });
}

#[test]
fn final_old_drop_racing_a_fresh_mutation_closes_each_anchor_once() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        let anchor = Anchor::registered(Arc::clone(&ledger), OLD);
        let mut mutator_alias = anchor.clone();

        let mutator = thread::spawn(move || {
            mutator_alias.fork_shared_version(FRESH_A);
            drop(mutator_alias);
        });
        let old_dropper = thread::spawn(move || drop(anchor));
        mutator.join().unwrap();
        old_dropper.join().unwrap();

        ledger.assert_closed(&[OLD, FRESH_A]);
    });
}

#[test]
fn two_forked_mutations_preserve_three_independent_authorities() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        let anchor = Anchor::registered(Arc::clone(&ledger), OLD);
        let mut first = anchor.clone();
        let mut second = anchor.clone();

        let a = thread::spawn(move || {
            first.fork_shared_version(FRESH_A);
            drop(first);
        });
        let b = thread::spawn(move || {
            second.fork_shared_version(FRESH_B);
            drop(second);
        });
        drop(anchor);
        a.join().unwrap();
        b.join().unwrap();

        ledger.assert_closed(&[OLD, FRESH_A, FRESH_B]);
    });
}

#[test]
fn abort_before_registration_creates_no_fresh_authority() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        let anchor = Anchor::registered(Arc::clone(&ledger), OLD);
        let alias = anchor.clone();

        let aborted = thread::spawn(move || {
            // Validation rejects before make_mut allocates/registers a root.
            drop(alias);
        });
        drop(anchor);
        aborted.join().unwrap();

        ledger.assert_closed(&[OLD]);
    });
}

#[test]
fn abort_after_registration_drains_the_private_fresh_anchor() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new());
        let anchor = Anchor::registered(Arc::clone(&ledger), OLD);
        let mut alias = anchor.clone();

        let aborted = thread::spawn(move || {
            alias.fork_shared_version(FRESH_A);
            // Validation rejects after registration but before publication.
            // Ordinary ownership cleanup must still elect the fresh PDA.
            drop(alias);
        });
        drop(anchor);
        aborted.join().unwrap();

        ledger.assert_closed(&[OLD, FRESH_A]);
    });
}
