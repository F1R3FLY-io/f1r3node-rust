//! B2(b) (CA-P-171) — concurrent DISJOINT-POOL admission, loom model.
//!
//! The production acceptance gate keys each deploy's funding to its signer's
//! supply pool `Σ⟦s⟧` (a per-signature `Lane` in the runtime's
//! `Arc<DashMap<[u8;32], Lane>>`, `accounting/mod.rs`). Disjoint signatures key
//! DISJOINT lane entries, so two deploys on different pools settle without
//! contending a shared lock — the `lane_pool_disjoint` corollary
//! (`formal/rocq/cost_accounted_rho/theories/ChannelSeparation.v`) and the
//! TM-CA-165 cross-group ledger's "disjoint pools never interfere" property.
//!
//! `DashMap`/`std` primitives are not loom-aware, so this is a STRUCTURAL shadow
//! model (the `loom_multi_sig_fanout.rs` pattern): two independent lanes, each a
//! loom atomic balance, with two threads each admitting a deploy against its OWN
//! lane via a CAS-decrement. The invariant verified across EVERY loom-explored
//! interleaving: BOTH admissions succeed (a disjoint peer never blocks the
//! other), each lane is debited EXACTLY its own deploy's demand (no cross-lane
//! corruption / no double-debit), and a shared liveness counter that BOTH threads
//! touch is updated exactly twice (the only legitimately-shared state stays
//! consistent — no lost update).
//!
//! Companion to:
//!   * `casper/.../acceptance.rs::tests::disjoint_pools_*` (the example side),
//!   * `MultiSignerProtocol.tla` / `loom_multi_sig_fanout.rs` (the fan-out
//!     atomic-commit analogue).

use loom::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

/// One signer's supply pool `Σ⟦s⟧` — an atomic balance plus the record of how
/// much THIS pool was debited (so we can assert it was debited by exactly its
/// own deploy, never a foreign lane).
struct Lane {
    balance: AtomicI64,
    debited: AtomicI64,
}

impl Lane {
    fn new(initial: i64) -> Self {
        Self {
            balance: AtomicI64::new(initial),
            debited: AtomicI64::new(0),
        }
    }

    /// Admit a deploy of `demand` against this lane: CAS-decrement the balance
    /// iff it covers the demand (Def 19 `Σ ≥ Δ`, all-or-nothing). Returns true
    /// on admit. The check-and-debit is a CAS loop on THIS lane's atomic only —
    /// no shared lock — so a concurrent admission on a DISJOINT lane never
    /// contends it.
    fn admit(&self, demand: i64) -> bool {
        let mut current = self.balance.load(Ordering::Acquire);
        loop {
            if current < demand {
                return false; // unfunded ⇒ reject, no debit (no side effect).
            }
            match self.balance.compare_exchange(
                current,
                current - demand,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.debited.fetch_add(demand, Ordering::AcqRel);
                    return true;
                }
                Err(observed) => current = observed,
            }
        }
    }
}

/// Two DISJOINT lanes admitted concurrently from two threads: both succeed,
/// each lane is debited exactly its own demand, and neither lane's balance is
/// perturbed by the other (no cross-lane corruption under any interleaving).
#[test]
fn disjoint_lanes_admit_concurrently_without_interference() {
    loom::model(|| {
        // Lane A funded for demand 3; lane B funded for demand 5. Disjoint.
        let lane_a = Arc::new(Lane::new(10));
        let lane_b = Arc::new(Lane::new(10));
        // The only legitimately-shared state: a liveness counter both admission
        // threads bump on success (models the runtime's shared admitted-count).
        let admitted = Arc::new(AtomicUsize::new(0));

        let a = {
            let lane_a = lane_a.clone();
            let admitted = admitted.clone();
            thread::spawn(move || {
                if lane_a.admit(3) {
                    admitted.fetch_add(1, Ordering::AcqRel);
                }
            })
        };
        let b = {
            let lane_b = lane_b.clone();
            let admitted = admitted.clone();
            thread::spawn(move || {
                if lane_b.admit(5) {
                    admitted.fetch_add(1, Ordering::AcqRel);
                }
            })
        };
        a.join().unwrap();
        b.join().unwrap();

        // BOTH admitted (a disjoint peer never blocks the other).
        assert_eq!(
            admitted.load(Ordering::Acquire),
            2,
            "both disjoint-pool deploys must be admitted"
        );
        // Each lane debited EXACTLY its own demand — no cross-lane corruption.
        assert_eq!(
            lane_a.debited.load(Ordering::Acquire),
            3,
            "lane A debited its own 3"
        );
        assert_eq!(
            lane_b.debited.load(Ordering::Acquire),
            5,
            "lane B debited its own 5"
        );
        // Balances reflect exactly the own-lane debit (conservation per pool).
        assert_eq!(
            lane_a.balance.load(Ordering::Acquire),
            7,
            "lane A: 10 − 3 = 7"
        );
        assert_eq!(
            lane_b.balance.load(Ordering::Acquire),
            5,
            "lane B: 10 − 5 = 5"
        );
    });
}

/// One funded + one UNDERFUNDED disjoint lane admitted concurrently: the funded
/// one is admitted and debited; the underfunded one is rejected with NO side
/// effect (its balance untouched) — and the rejection does not block or perturb
/// the funded lane. Holds under every interleaving (the disjoint lanes share no
/// lock, so the reject path cannot starve the admit path).
#[test]
fn disjoint_funded_and_underfunded_lanes_settle_independently() {
    loom::model(|| {
        let funded = Arc::new(Lane::new(8)); // covers demand 5
        let underfunded = Arc::new(Lane::new(2)); // below demand 5 ⇒ reject
        let admitted = Arc::new(AtomicUsize::new(0));

        let a = {
            let funded = funded.clone();
            let admitted = admitted.clone();
            thread::spawn(move || {
                if funded.admit(5) {
                    admitted.fetch_add(1, Ordering::AcqRel);
                }
            })
        };
        let b = {
            let underfunded = underfunded.clone();
            let admitted = admitted.clone();
            thread::spawn(move || {
                if underfunded.admit(5) {
                    admitted.fetch_add(1, Ordering::AcqRel);
                }
            })
        };
        a.join().unwrap();
        b.join().unwrap();

        // Exactly the funded lane admitted.
        assert_eq!(
            admitted.load(Ordering::Acquire),
            1,
            "only the funded lane admits"
        );
        assert_eq!(
            funded.debited.load(Ordering::Acquire),
            5,
            "funded lane debited 5"
        );
        assert_eq!(
            funded.balance.load(Ordering::Acquire),
            3,
            "funded: 8 − 5 = 3"
        );
        // NO SIDE EFFECT on the rejected lane.
        assert_eq!(
            underfunded.debited.load(Ordering::Acquire),
            0,
            "the rejected lane must NOT be debited"
        );
        assert_eq!(
            underfunded.balance.load(Ordering::Acquire),
            2,
            "the rejected lane's balance is untouched"
        );
    });
}

/// A liveness check that the SHARED admitted-counter is the ONLY shared state and
/// it never loses an update: even though two threads race to bump it, every
/// successful admit is reflected (no lost increment under any interleaving). Both
/// lanes funded ⇒ the counter MUST reach exactly 2.
#[test]
fn shared_admitted_counter_has_no_lost_update() {
    loom::model(|| {
        let lane_a = Arc::new(Mutex::new(0i64)); // a Mutex-guarded lane balance
        let lane_b = Arc::new(Lane::new(10));
        let admitted = Arc::new(AtomicUsize::new(0));

        // Seed lane_a's balance.
        *lane_a.lock().unwrap() = 10;

        let a = {
            let lane_a = lane_a.clone();
            let admitted = admitted.clone();
            thread::spawn(move || {
                // Mutex-guarded check-and-debit (a different lane representation,
                // still disjoint from lane_b).
                let mut bal = lane_a.lock().unwrap();
                if *bal >= 4 {
                    *bal -= 4;
                    drop(bal);
                    admitted.fetch_add(1, Ordering::AcqRel);
                }
            })
        };
        let b = {
            let lane_b = lane_b.clone();
            let admitted = admitted.clone();
            thread::spawn(move || {
                if lane_b.admit(6) {
                    admitted.fetch_add(1, Ordering::AcqRel);
                }
            })
        };
        a.join().unwrap();
        b.join().unwrap();

        assert_eq!(
            admitted.load(Ordering::Acquire),
            2,
            "both admits reflected — no lost update on the shared counter"
        );
        assert_eq!(*lane_a.lock().unwrap(), 6, "lane A (mutex): 10 − 4 = 6");
        assert_eq!(
            lane_b.balance.load(Ordering::Acquire),
            4,
            "lane B: 10 − 6 = 4"
        );
    });
}
