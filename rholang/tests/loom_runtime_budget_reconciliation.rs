//! Loom model of the Option-E `RuntimeBudget` reconciliation contract:
//! workers race lock-free attempts, post-hoc canonical walk is the
//! consensus answer. The production `RuntimeBudget` uses `std::sync`
//! primitives (not loom-aware), so this test exercises a structurally
//! equivalent toy budget that follows the same invariants — pushing
//! every attempt to an append log, racing a CAS on `consumed`, and
//! deriving the canonical commit set + OOP boundary by sorting and
//! walking attempts at finalization.
//!
//! The headline invariant verified across every loom-explored schedule
//! is: **for the same input multiset of attempts and the same initial
//! budget, the canonical reconciliation produces the same
//! `(committed_set, oop_event, consumed_units)` triple regardless of
//! which CAS race winners occurred at runtime.** This mirrors the
//! TLA+ `RuntimeRaceDoesNotChangeReconciledDigest` invariant in
//! `formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla` and
//! the Rocq `rb_reconcile_permutation_invariant` theorem in
//! `formal/rocq/cost_accounted_rho/theories/RuntimeBudgetRefinement.v`.

use loom::sync::atomic::{AtomicI64, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ToyEvent {
    /// Canonical sort key (mirrors `BillableTokenEvent`'s
    /// `local_index`/`source_path` ordering). Set by the test's
    /// program structure, never by Tokio scheduling.
    rank: u64,
    weight: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToyReconciliation {
    committed: Vec<ToyEvent>,
    oop: Option<ToyEvent>,
    consumed_units: i64,
}

struct ToyBudget {
    initial: i64,
    consumed: AtomicI64,
    attempt_log: Mutex<Vec<ToyEvent>>,
}

enum AttemptOutcome {
    Granted,
    Oop,
}

impl ToyBudget {
    fn new(initial: i64) -> Self {
        Self {
            initial,
            consumed: AtomicI64::new(0),
            attempt_log: Mutex::new(Vec::new()),
        }
    }

    /// Lock-free attempt mirroring the production `attempt_one`:
    /// push to the attempt log first (no matter what), then CAS on
    /// `consumed` for the runtime grant/oop decision. The CAS counter
    /// is a liveness gate; the consensus answer comes from `reconcile()`.
    fn attempt(&self, event: ToyEvent) -> AttemptOutcome {
        {
            let mut log = self.attempt_log.lock().unwrap();
            log.push(event.clone());
        }

        let weight = event.weight;
        let mut current = self.consumed.load(Ordering::Acquire);
        loop {
            if current < 0 || self.initial < 0 || current >= self.initial {
                return AttemptOutcome::Oop;
            }
            let next = current.saturating_add(weight);
            if next > self.initial {
                return AttemptOutcome::Oop;
            }
            match self.consumed.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return AttemptOutcome::Granted,
                Err(actual) => current = actual,
            }
        }
    }

    /// Pure canonical reconciliation. Sorts the attempt log by
    /// program-derived rank and walks until the budget is exhausted.
    /// Idempotent and independent of which CAS races succeeded.
    fn reconcile(&self) -> ToyReconciliation {
        let mut attempts: Vec<ToyEvent> = self.attempt_log.lock().unwrap().clone();
        attempts.sort();

        let mut committed = Vec::new();
        let mut consumed_units: i64 = 0;
        let mut oop = None;
        for event in attempts {
            let next = consumed_units.saturating_add(event.weight);
            if next > self.initial {
                oop = Some(event);
                consumed_units = self.initial;
                break;
            }
            consumed_units = next;
            committed.push(event);
        }
        ToyReconciliation {
            committed,
            oop,
            consumed_units,
        }
    }
}

/// Two-attempt race over a budget that fits both → reconciliation must
/// commit both regardless of schedule. Catches a regression where the
/// CAS path drops an attempt from the log.
#[test]
fn reconcile_commits_both_when_budget_admits_both() {
    loom::model(|| {
        let budget = Arc::new(ToyBudget::new(10));
        let event_a = ToyEvent { rank: 1, weight: 3 };
        let event_b = ToyEvent { rank: 2, weight: 4 };

        let a = {
            let budget = budget.clone();
            let event = event_a.clone();
            thread::spawn(move || {
                let _ = budget.attempt(event);
            })
        };
        let b = {
            let budget = budget.clone();
            let event = event_b.clone();
            thread::spawn(move || {
                let _ = budget.attempt(event);
            })
        };

        a.join().unwrap();
        b.join().unwrap();

        let rec = budget.reconcile();
        assert_eq!(
            rec,
            ToyReconciliation {
                committed: vec![event_a.clone(), event_b.clone()],
                oop: None,
                consumed_units: 7,
            }
        );
    });
}

/// Two-attempt race over a budget that fits only one → canonical OOP
/// must be the higher-rank event, NOT the runtime CAS loser. This is
/// the load-bearing schedule-invariance assertion: regardless of which
/// thread won the CAS race, the canonical answer is the same.
#[test]
fn reconcile_canonical_oop_is_higher_rank_event_under_any_schedule() {
    loom::model(|| {
        let budget = Arc::new(ToyBudget::new(5));
        let event_a = ToyEvent { rank: 1, weight: 3 };
        let event_b = ToyEvent { rank: 2, weight: 3 };

        let a = {
            let budget = budget.clone();
            let event = event_a.clone();
            thread::spawn(move || {
                let _ = budget.attempt(event);
            })
        };
        let b = {
            let budget = budget.clone();
            let event = event_b.clone();
            thread::spawn(move || {
                let _ = budget.attempt(event);
            })
        };

        a.join().unwrap();
        b.join().unwrap();

        let rec = budget.reconcile();
        // Canonical walk: rank=1 commits (consumed=3), rank=2 would push
        // to 6 > 5 → OOP. This is true under every loom-explored
        // schedule, even when the CAS race had event_b succeed first
        // and event_a fail at runtime.
        assert_eq!(
            rec,
            ToyReconciliation {
                committed: vec![event_a.clone()],
                oop: Some(event_b.clone()),
                consumed_units: 5,
            }
        );
    });
}

/// Three threads, budget admits two → canonical commit set is the two
/// lex-smallest events, canonical OOP is the third. Verifies the
/// canonical walk's first-overflow semantics under racing CAS.
#[test]
fn reconcile_commits_lex_smallest_prefix_under_any_schedule() {
    loom::model(|| {
        let budget = Arc::new(ToyBudget::new(6));
        let event_low = ToyEvent { rank: 1, weight: 2 };
        let event_mid = ToyEvent { rank: 2, weight: 3 };
        let event_high = ToyEvent { rank: 3, weight: 4 };

        let a = {
            let budget = budget.clone();
            let event = event_low.clone();
            thread::spawn(move || {
                let _ = budget.attempt(event);
            })
        };
        let b = {
            let budget = budget.clone();
            let event = event_mid.clone();
            thread::spawn(move || {
                let _ = budget.attempt(event);
            })
        };
        let c = {
            let budget = budget.clone();
            let event = event_high.clone();
            thread::spawn(move || {
                let _ = budget.attempt(event);
            })
        };

        a.join().unwrap();
        b.join().unwrap();
        c.join().unwrap();

        let rec = budget.reconcile();
        // Canonical walk over sorted ranks: 1+2=2, 2+3=5, 3+4=9>6.
        // event_low commits (2), event_mid commits (5), event_high OOP.
        assert_eq!(
            rec,
            ToyReconciliation {
                committed: vec![event_low.clone(), event_mid.clone()],
                oop: Some(event_high.clone()),
                consumed_units: 6,
            }
        );
    });
}
