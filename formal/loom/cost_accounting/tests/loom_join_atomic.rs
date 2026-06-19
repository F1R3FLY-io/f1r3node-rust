//! B3 (CA-P-052/108) — atomic N-ary join, loom model.
//!
//! A token-gated N-ary join `for( {% y1<-x1 %}[s1] & … & {% yk<-xk %}[sk] ){ P }`
//! fires only when ALL k clause SURFACES are present, and consumes the combined
//! join token EXACTLY ONCE. The W1 red-team's hazard (`TokenGatedJoin.tla`,
//! MAJOR-5) is a PARTIAL multi-party debit: a strict PREFIX of the join's pools
//! gets debited while the join never fires (observable to a concurrent group as
//! a griefing drain). The NATIVE acceptance-time model (M1) forbids this: a
//! group's pools are EITHER all debited (fire) OR all untouched (reject) — never
//! a strict prefix (`Inv_M1_AtomicNoPartialPrefix`).
//!
//! This is the loom-level image of that invariant. `std`/RSpace join-matching is
//! not loom-aware, so this is a STRUCTURAL shadow model: the k join surfaces
//! arrive on separate threads (racing partial arrivals); the join's check-all-
//! surfaces-then-debit-once transition is held inside ONE critical section so it
//! is atomic. Across EVERY loom-explored interleaving the invariant holds:
//!   * the combined token is debited 0 times (join un-fired) or EXACTLY 1 time
//!     (join fired) — never a partial/multi debit;
//!   * if the join did NOT fire, the pool is UNTOUCHED (no prefix spent);
//!   * if it DID fire, ALL k surfaces were present at the firing instant.
//!
//! Companion to `formal/tlaplus/cost_accounted_rho/TokenGatedJoin.tla`
//! (`Inv_M1_AtomicNoPartialPrefix`, `Inv_M1_NoVictimDrainWithoutFire`).

use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

/// Shadow N-ary join state. `surfaces_present` is the set of arrived clause
/// surfaces (a bitset over k clauses); `combined_token_debits` counts how many
/// times the single combined join token was consumed (MUST end at 0 or 1).
struct Join {
    /// Number of clauses (arity) the join requires.
    arity: usize,
    /// Bitset of arrived surfaces + the fired flag, behind ONE lock so the
    /// "all present? then debit once + mark fired" transition is atomic.
    inner: Mutex<JoinInner>,
    /// How many times the combined token was debited (observable; 0 or 1).
    combined_token_debits: AtomicUsize,
}

struct JoinInner {
    present_mask: u32,
    fired: bool,
}

impl Join {
    fn new(arity: usize) -> Self {
        Self {
            arity,
            inner: Mutex::new(JoinInner {
                present_mask: 0,
                fired: false,
            }),
            combined_token_debits: AtomicUsize::new(0),
        }
    }

    /// Deliver clause `i`'s surface. Inside the lock: record the arrival, and if
    /// ALL `arity` surfaces are now present AND the join has not yet fired, fire
    /// it — debit the combined token EXACTLY once and mark fired. The whole
    /// check-then-debit is atomic (one critical section), so no interleaving can
    /// observe a partial multi-party debit.
    fn deliver(&self, i: usize) {
        let mut guard = self.inner.lock().unwrap();
        guard.present_mask |= 1u32 << i;
        let all_present = guard.present_mask.count_ones() as usize == self.arity;
        if all_present && !guard.fired {
            guard.fired = true;
            // The single combined-token debit — performed once, under the lock.
            self.combined_token_debits.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn fired(&self) -> bool {
        self.inner.lock().unwrap().fired
    }

    fn present_count(&self) -> usize {
        self.inner.lock().unwrap().present_mask.count_ones() as usize
    }
}

/// ALL surfaces arrive (one per thread): the join fires and the combined token
/// is consumed EXACTLY ONCE — never twice (no double-debit) and never zero (it
/// must fire once complete). Atomic-no-partial-prefix in the fully-arriving case.
#[test]
fn full_arrival_fires_join_and_debits_combined_token_exactly_once() {
    loom::model(|| {
        let join = Arc::new(Join::new(2));
        let j0 = join.clone();
        let j1 = join.clone();

        let t0 = thread::spawn(move || j0.deliver(0));
        let t1 = thread::spawn(move || j1.deliver(1));
        t0.join().unwrap();
        t1.join().unwrap();

        // All surfaces present ⇒ the join fired.
        assert!(join.fired(), "all surfaces present ⇒ join fires");
        assert_eq!(join.present_count(), 2, "both surfaces recorded");
        // The combined token is consumed EXACTLY ONCE regardless of which thread
        // observed completion (the atomic fire-once guard).
        assert_eq!(
            join.combined_token_debits.load(Ordering::Acquire),
            1,
            "the combined join token is consumed exactly once (no double-debit)"
        );
    });
}

/// A PARTIAL arrival (only one of two surfaces ever delivered): the join does
/// NOT fire, and the combined token is debited ZERO times — NO partial prefix
/// spent. This is `Inv_M1_AtomicNoPartialPrefix` in the under-funded/partial
/// case: an incomplete join leaves the pool UNTOUCHED (the griefing-drain
/// refutation).
#[test]
fn partial_arrival_never_debits_the_combined_token() {
    loom::model(|| {
        let join = Arc::new(Join::new(2));
        let j0 = join.clone();

        // Only surface 0 ever arrives; surface 1 is withheld (a partial join).
        let t0 = thread::spawn(move || j0.deliver(0));
        t0.join().unwrap();

        // The join did NOT fire (a surface is missing).
        assert!(!join.fired(), "an incomplete join must not fire");
        assert_eq!(join.present_count(), 1, "only one surface arrived");
        // NO partial debit: the combined token is untouched.
        assert_eq!(
            join.combined_token_debits.load(Ordering::Acquire),
            0,
            "a partial join must NOT debit the combined token (no prefix spent)"
        );
    });
}

/// A 3-ary join with one surface WITHHELD races the two delivered surfaces in
/// arbitrary order: the join still never fires and never debits — the
/// no-partial-prefix invariant holds for higher arity and across all
/// interleavings of the present surfaces. Critically, even though the two
/// present surfaces race the shared mask, no interleaving produces a partial
/// debit.
#[test]
fn ternary_join_with_one_missing_surface_never_partially_debits() {
    loom::model(|| {
        let join = Arc::new(Join::new(3));
        let j0 = join.clone();
        let j1 = join.clone();

        // Surfaces 0 and 1 arrive concurrently; surface 2 is missing.
        let t0 = thread::spawn(move || j0.deliver(0));
        let t1 = thread::spawn(move || j1.deliver(1));
        t0.join().unwrap();
        t1.join().unwrap();

        assert!(!join.fired(), "missing the 3rd surface ⇒ never fires");
        assert!(join.present_count() <= 2, "at most the two delivered surfaces");
        assert_eq!(
            join.combined_token_debits.load(Ordering::Acquire),
            0,
            "no fire ⇒ no debit, even with two surfaces racing (no partial prefix)"
        );
    });
}

/// ALL THREE surfaces arrive concurrently (full 3-ary race): the join fires and
/// debits the combined token EXACTLY ONCE — atomicity holds under the full
/// 3-thread interleaving (the fire-once guard admits no double-debit even when
/// the completing surface is ambiguous across schedules).
#[test]
fn ternary_full_arrival_debits_exactly_once_under_full_race() {
    loom::model(|| {
        let join = Arc::new(Join::new(3));
        let j0 = join.clone();
        let j1 = join.clone();
        let j2 = join.clone();

        let t0 = thread::spawn(move || j0.deliver(0));
        let t1 = thread::spawn(move || j1.deliver(1));
        let t2 = thread::spawn(move || j2.deliver(2));
        t0.join().unwrap();
        t1.join().unwrap();
        t2.join().unwrap();

        assert!(join.fired(), "all 3 surfaces ⇒ join fires");
        assert_eq!(join.present_count(), 3, "all 3 surfaces recorded");
        assert_eq!(
            join.combined_token_debits.load(Ordering::Acquire),
            1,
            "the combined token is consumed exactly once under the full 3-way race"
        );
    });
}
