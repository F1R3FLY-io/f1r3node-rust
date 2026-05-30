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
            match self
                .consumed
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
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

/// OOP-truncation under truncated recording. In the OOP case a fork may unwind
/// and stop recording its remaining attempts, so the recorded multiset — and
/// therefore the per-operation committed set / digest — becomes schedule-
/// dependent. This is precisely WHY the per-operation digest is NOT a consensus
/// quantity (it was removed from consensus; see threat-model TM-CA-151). The
/// quantity consensus DOES compare, `consumed` (= `total_cost`), still clamps
/// to `initial` deterministically under every loom-explored schedule, because
/// any run that OOPs has recorded cumulative weight exceeding `initial`.
#[test]
fn oop_truncation_keeps_consumed_clamped_under_every_schedule() {
    loom::model(|| {
        let budget = Arc::new(ToyBudget::new(5));
        // Each fork attempts a first event; only if that was granted does it go
        // on to attempt its second event — modeling a fork that unwinds (stops
        // recording) once it hits OOP. The two first events (weight 3 each) are
        // always recorded and already exceed the budget of 5, so every schedule
        // reaches the OOP boundary.
        let a = {
            let budget = budget.clone();
            thread::spawn(move || {
                if matches!(
                    budget.attempt(ToyEvent { rank: 1, weight: 3 }),
                    AttemptOutcome::Granted
                ) {
                    let _ = budget.attempt(ToyEvent { rank: 3, weight: 3 });
                }
            })
        };
        let b = {
            let budget = budget.clone();
            thread::spawn(move || {
                if matches!(
                    budget.attempt(ToyEvent { rank: 2, weight: 3 }),
                    AttemptOutcome::Granted
                ) {
                    let _ = budget.attempt(ToyEvent { rank: 4, weight: 3 });
                }
            })
        };
        a.join().unwrap();
        b.join().unwrap();

        // `consumed_units` (the consensus `total_cost`) clamps to `initial` on
        // OOP regardless of which fork's later events were recorded. The
        // committed set itself is intentionally NOT asserted here: it is
        // schedule-dependent under truncation and is not a consensus quantity.
        let rec = budget.reconcile();
        assert_eq!(rec.consumed_units, 5);
        assert!(rec.oop.is_some());
    });
}

/// D0 — per-signature TOKEN POOL: two DISJOINT signatures each get their own
/// `ToyBudget` lane (mirroring the production `RuntimeBudget::lanes`
/// `DashMap<[u8;32], Lane>`, where disjoint signatures key disjoint entries
/// per the `lane_pool_disjoint` corollary in `ChannelSeparation.v`). Two
/// threads race reservations into the two lanes concurrently; the pool's
/// `total_cost` is the SUM over per-lane canonical reconciliations.
///
/// The headline invariant, verified across every loom-explored schedule: the
/// per-lane reconciliations are INDEPENDENT (a CAS race within one lane never
/// touches the other lane's counter or log), and the pool total — the sum of
/// the two per-lane `consumed_units` — is the SAME under every interleaving of
/// the two threads. This is the concurrency face of spec §7.6 ("no
/// interleaving" is PER-SIGNATURE, not global) and of
/// `rb_pool_total_cost = Σ rb_total_cost` in `RuntimeBudgetRefinement.v`:
/// disjoint signatures contend on nothing, so the order in which lanes are
/// driven and summed is irrelevant.
#[test]
fn reconcile_two_disjoint_lanes_sum_is_schedule_independent() {
    loom::model(|| {
        // Lane A: initial 10, two events 3+4 → consumed 7, no OOP.
        // Lane B: initial 5,  two events 3+4 → commits 3, OOPs on 4 → clamps 5.
        // Pool total = 7 + 5 = 12 under EVERY schedule.
        let lane_a = Arc::new(ToyBudget::new(10));
        let lane_b = Arc::new(ToyBudget::new(5));

        let a_lo = ToyEvent { rank: 1, weight: 3 };
        let a_hi = ToyEvent { rank: 2, weight: 4 };
        let b_lo = ToyEvent { rank: 1, weight: 3 };
        let b_hi = ToyEvent { rank: 2, weight: 4 };

        // Thread 1 drives lane A; thread 2 drives lane B. The two lanes share
        // NOTHING, so their reservations proceed fully concurrently.
        let t_a = {
            let lane = lane_a.clone();
            let (lo, hi) = (a_lo.clone(), a_hi.clone());
            thread::spawn(move || {
                let _ = lane.attempt(lo);
                let _ = lane.attempt(hi);
            })
        };
        let t_b = {
            let lane = lane_b.clone();
            let (lo, hi) = (b_lo.clone(), b_hi.clone());
            thread::spawn(move || {
                let _ = lane.attempt(lo);
                let _ = lane.attempt(hi);
            })
        };

        t_a.join().unwrap();
        t_b.join().unwrap();

        // Each lane reconciles independently via the canonical walk.
        let rec_a = lane_a.reconcile();
        let rec_b = lane_b.reconcile();

        // Lane A commits both (no OOP, consumed 7); lane B OOPs and clamps 5.
        assert_eq!(rec_a.consumed_units, 7);
        assert!(rec_a.oop.is_none());
        assert_eq!(rec_b.consumed_units, 5);
        assert!(rec_b.oop.is_some());

        // The pool total is the order-independent SUM over lanes — identical
        // under every loom-explored interleaving (and summed in either order).
        let total_ab = rec_a
            .consumed_units
            .saturating_add(rec_b.consumed_units);
        let total_ba = rec_b
            .consumed_units
            .saturating_add(rec_a.consumed_units);
        assert_eq!(total_ab, 12);
        assert_eq!(total_ab, total_ba);
    });
}
