//! # Cost accounting — formal correspondence (continued-gslt-cost-v2)
//!
//! This runtime is the operational image of the Rocq Cost endofunctor/monad
//! (`formal/rocq/cost_accounted_rho/`). The mapping is additive witnessing only —
//! no behavioral change (see `docs/theory/cost-accounting-as-monad-correspondence.md`):
//!  - η (unmetered embedding)   ↔ the system/unmetered budget mode
//!    (`CostMonad.cost_eta`, `CAAdjunctions.cost_install`).
//!  - μ (grade accumulation)    ↔ per-COMM charge accumulation; non-idempotent
//!    (`CostMonad.cost_mu` / `cost_mu_modulus_accumulates`).
//!  - located capabilities      ↔ the per-signature `DashMap` lanes, disjoint
//!    (`CALocatedPurses.draw_disjoint` / `ChannelSeparation.lane_pool_disjoint`).
//!  - graded transition ⟨a⟩_s   ↔ the signature key on billable events
//!    (`CAGradedTransition.graded_step`).
//!  - linear no-double-spend    ↔ the resource-logic / Δσ discipline
//!    (`CATypeDiscipline.ca_linear_no_contraction`).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use costs::Cost;
use crossbeam_queue::SegQueue;
use crypto::rust::hash::blake2b256::Blake2b256;
use dashmap::DashMap;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

use super::errors::InterpreterError;

pub mod cost_accounting;
pub mod costs;
pub mod delta_sigma;
pub mod has_cost;
pub mod resource_logic;

const DEPLOY_SIGNATURE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:deploy-signature:v1";
/// Domain separator for compound (multi-signer) deploy signatures. Distinct
/// from the legacy single-sig `DEPLOY_SIGNATURE_DOMAIN` so legacy deploys on
/// chain retain their existing `deploy_id`s, while multi-sig deploys get a
/// distinguishable id derived from the canonically-ordered set of signatures.
const COMPOUND_DEPLOY_SIGNATURE_DOMAIN: &[u8] =
    b"f1r3node:cost-accounted-rho:compound-deploy-signature:v1";
const COST_TRACE_DIGEST_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:cost-trace:v1";
/// Domain separator for the per-signature lane key (`Sig::lane_hash`). The
/// lane key digests the SAME canonical signature serialization that
/// `SignatureChannel::from_sig` uses to derive the supply channel `Σ⟦s⟧`
/// (`sig_canonical_bytes`), so a deploy's lane key for signature `s` and its
/// supply channel are anchored to one canonical basis (no drift — see
/// `docs/theory/cost-accounting-impl/supply-realization-c-d-handoff.md`,
/// "Integration invariant"). Distinct from the channel domain only by this
/// separator: `lane_hash` is an internal map key (`[u8;32]`), while the
/// channel is a `GPrivate`-keyed `Par`; both are pure functions of the same
/// canonical bytes.
const SIGNATURE_LANE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:signature-lane:v1";
pub const MAX_COST_TRACE_EVENTS: u64 = 1_048_576;
pub const MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES: usize = 512;
pub const MAX_COST_TRACE_SOURCE_PATH_COMPONENTS: usize = 1024;

#[derive(Clone)]
pub struct RuntimeBudget {
    initial_tokens: Arc<AtomicI64>,
    // Liveness counter — tracks weights successfully claimed by parallel
    // workers via CAS. Strictly an internal runtime check used to short
    // out branches once the budget is exhausted. The consensus-relevant
    // consumed value comes from `reconcile()`, NOT this counter, because
    // it may differ from the canonical reconciliation when workers race.
    consumed_tokens: Arc<AtomicI64>,
    signature: Arc<Mutex<Sig>>,
    deploy_id: Arc<Mutex<[u8; 32]>>,
    log: Arc<Mutex<VecDeque<Cost>>>,
    event_log: Arc<Mutex<VecDeque<BillableTokenEvent>>>,
    // Lock-free append queue of every reservation ATTEMPT (whether or
    // not the runtime CAS race granted it). `attempt_one` pushes here
    // with no lock — `crossbeam_queue::SegQueue` is a wait-free MPMC
    // queue, so concurrent reducer forks never contend a Mutex on the
    // hot path. `reconcile()` drains it into `attempt_accumulator`
    // (drain-append-recompute), so mid-deploy reads do not lose later
    // attempts. Cost-accounted-rho paper §3 Rule 1 (single shared
    // signature/token within a deploy) — the canonical reduction order
    // is structurally determined by the program, not by Tokio
    // scheduling. See `formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla`
    // and `formal/rocq/cost_accounted_rho/theories/RuntimeBudgetRefinement.v`.
    attempt_queue: Arc<SegQueue<AttemptRecord>>,
    // Internal reconciliation accumulator. Drained-into from
    // `attempt_queue` by `reconcile()` and re-walked to compute the
    // canonical reconciliation. Touched ONLY inside `reconcile`/`reset`
    // — NEVER per-event — so the hot path stays lock-free. The same
    // Mutex also guards `canonical_reconciliation` repopulation and the
    // diagnostic `event_log`/`log` mirrors during finalization.
    attempt_accumulator: Arc<Mutex<Vec<AttemptRecord>>>,
    // Cached canonical reconciliation. Populated by `reconcile()` when
    // the attempt queue is drained at deploy finalization; reset by
    // `reset_from_token` when the budget is reused for a new deploy.
    // Read by `total_cost`, `cost_trace_digest`, `cost_trace_event_count`,
    // `last_oop_event`. Recomputed whenever `reconcile()` observes newly
    // drained attempts; the hot path never invalidates it per-event.
    canonical_reconciliation: Arc<Mutex<Option<CanonicalReconciliation>>>,
    max_log_entries: usize,
    unmetered: Arc<AtomicU64>,
    // Per-signature token pool (spec §4.6 spectral decomposition into
    // per-signature pools; §7.6 "no interleaving" is PER-SIGNATURE, not
    // global). The N=1 (single-signature) FAST PATH keeps this map EMPTY:
    // every legacy single-signature deploy leaves `lanes` empty and runs the
    // EXISTING scalar `attempt_one`/`reconcile`/`total_cost` path
    // byte-identically — the lane pool is only consulted once a deploy has
    // routed attempts into one or more lanes. Lock-free reads/inserts mirror
    // `rspace_plus_plus/src/rspace/rspace.rs`'s `phase_a_locks`
    // (`Arc<DashMap<…>>`): disjoint signatures key disjoint `Lane` entries, so
    // concurrent per-lane reconciliation across distinct signatures never
    // contends (the `lane_pool_disjoint` corollary in
    // `formal/rocq/cost_accounted_rho/theories/ChannelSeparation.v`). Keyed by
    // `Sig::lane_hash` — the same canonical basis as the supply channel
    // `SignatureChannel::from_sig` (integration invariant).
    lanes: Arc<DashMap<[u8; 32], Lane>>,
}

/// One per-signature token pool entry (spec §4.6). Structurally mirrors the
/// `RuntimeBudget` scalar fields so `reconcile_lane` — the SAME canonical
/// reconciliation walk used by the scalar path — applies unchanged per lane:
/// atomics where the scalar path uses atomics (`initial_tokens`,
/// `consumed_tokens`), a lock-free `attempt_queue` (wait-free MPMC
/// `SegQueue`), and a `Mutex`-guarded `accumulator` + cached `reconciliation`
/// touched ONLY at lane finalization (never on the per-event hot path), again
/// matching the scalar budget. Each lane is an independent instance of the
/// proven scalar budget (`rb_pool` in
/// `formal/rocq/cost_accounted_rho/theories/RuntimeBudgetRefinement.v`).
///
/// `#[allow(dead_code)]`: D0 establishes the per-signature pool SUBSTRATE
/// (struct + lock-free routing + per-lane reconciliation) and exercises it
/// from the in-crate tests; the PRODUCTION write path that routes a deploy's
/// charges into lanes lands at the D2 block-assembly funding gate
/// (`block_creator.rs::admit_by_funding`, per
/// `docs/theory/cost-accounting-impl/workstream-d-acceptance.md`). The
/// `sig`/`consumed_tokens` fields are part of the spec'd `Lane` shape (§4.6)
/// and are read on that future path; they are retained (not removed) so the
/// substrate matches the design exactly. The N=1 scalar fast path never
/// constructs a `Lane`.
#[allow(dead_code)]
struct Lane {
    /// The whole-signature value σ this lane pools fuel for (Def 7.4 — no
    /// per-component split; one compound lane per deploy in D-scope).
    sig: Sig,
    /// Initial token budget for this signature's pool. `AtomicI64` matching
    /// the scalar `RuntimeBudget::initial_tokens`.
    initial_tokens: AtomicI64,
    /// Liveness counter for this lane (CAS-claimed weights). Strictly an
    /// internal runtime gate, identical in role to the scalar
    /// `consumed_tokens`; the consensus-relevant consumed value for the lane
    /// comes from `reconcile_lane`, NOT this counter.
    consumed_tokens: AtomicI64,
    /// Lock-free append queue of every reservation ATTEMPT routed to this
    /// lane (wait-free MPMC `SegQueue`), drained into `accumulator` by the
    /// lane reconciliation. Mirrors the scalar `attempt_queue`.
    attempt_queue: SegQueue<AttemptRecord>,
    /// Reconciliation accumulator for this lane. Drained-into from
    /// `attempt_queue` and re-walked by `reconcile_lane`. Touched only at lane
    /// finalization, so the per-event path stays lock-free. Mirrors the scalar
    /// `attempt_accumulator`.
    accumulator: Mutex<Vec<AttemptRecord>>,
    /// Cached canonical reconciliation for this lane (drain-append-recompute).
    /// Mirrors the scalar `canonical_reconciliation`.
    reconciliation: Mutex<Option<CanonicalReconciliation>>,
}

impl Lane {
    // See the `Lane` doc comment: lane construction is on the staged D2
    // production routing path and is exercised by the in-crate D0 tests.
    #[allow(dead_code)]
    fn new(sig: Sig, initial: i64) -> Self {
        Self {
            sig,
            initial_tokens: AtomicI64::new(initial),
            consumed_tokens: AtomicI64::new(0),
            attempt_queue: SegQueue::new(),
            accumulator: Mutex::new(Vec::new()),
            reconciliation: Mutex::new(None),
        }
    }
}

/// One reservation attempt recorded during evaluation. Pushed to the
/// lock-free `attempt_log` whether or not the runtime CAS race granted
/// the reservation. `amount` is `Some` for reservations driven via
/// `reserve_canonical_with_cost` (so the canonical reconciliation can
/// reconstruct the cost-log entries deterministically), `None` for
/// `reserve_canonical` (event-only reservations).
#[derive(Clone, Debug)]
struct AttemptRecord {
    event: BillableTokenEvent,
    amount: Option<Cost>,
}

/// Pure-function output of `reconcile()`: the canonical, schedule-
/// independent answer to "given this multiset of reservation attempts
/// and an initial budget, which events would have committed and which
/// would have been the OOP boundary, in the canonical reduction order
/// derivable from the program's source structure?"
///
/// The canonical order is the derived `Ord` on `BillableTokenEvent`:
/// `(deploy_id, source_path, redex_id, local_index, kind, weight)` — all
/// program-structure-derived components, never schedule-dependent.
///
/// Corresponds to the `Merge` action in
/// `formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla` and to
/// `rb_reconcile` in
/// `formal/rocq/cost_accounted_rho/theories/RuntimeBudgetRefinement.v`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CanonicalReconciliation {
    /// Events that fit within the budget in canonical order.
    committed: Vec<BillableTokenEvent>,
    /// First event whose cumulative weight would exceed the initial
    /// budget, if any. None means the deploy completed without OOP.
    oop: Option<BillableTokenEvent>,
    /// Final consumed cost: `Σ committed.weight` if no OOP, `initial`
    /// (clamped UP) if OOP — preserves the `deploy.cost == phlo_limit`
    /// invariant the integration tests assert.
    consumed_units: i64,
    /// Per-committed-event Cost values reconstructed from the attempt
    /// log's `amount` field. Used to repopulate the diagnostic
    /// `log: VecDeque<Cost>` deterministically.
    cost_amounts: Vec<Cost>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostTraceDigest {
    // Canonical hash of successful reservations plus the optional OOP
    // boundary. The digest is order-insensitive for successful parallel
    // reservations but still sensitive to event descriptors and OOP boundary.
    pub digest: Vec<u8>,
    pub event_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostReservationBatch {
    pub events: Vec<BillableTokenEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionPermit {
    pub event: BillableTokenEvent,
    pub weight: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostCommit {
    pub permits: Vec<ExecutionPermit>,
    pub consumed_weight: u64,
    pub oop: Option<BillableTokenEvent>,
}

/// Runtime liveness outcome for a single reservation attempt. The
/// attempt is always recorded in `attempt_log` regardless of outcome;
/// this enum only tells the caller whether to let the branch proceed.
enum AttemptOutcome {
    Granted,
    Oop,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePath(pub Vec<u32>);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RedexId(pub u64);

/// The kind of a billable token event. D3 (DR-9, OD-3) splits the former
/// single `SourceStep` into two CONSENSUS-relevant-vs-diagnostic kinds:
///
///   * [`BillableKind::Comm`] — a token-consuming COMM reduction (send /
///     receive). THIS is the consensus cost unit: the spec's "one token per
///     COMM" (cost-accounted-rho §3.6 Rules 1-5, §7.2). `reconcile_lane`
///     counts each committed `Comm` as exactly 1 toward `consumed_units`.
///   * [`BillableKind::Reduction`] — a non-COMM structural reduction
///     (`new` / `match` / `if`). Metered for DIAGNOSTIC fidelity (it walks
///     into the event log + digest with its per-op weight) but contributes
///     ZERO to the consensus consumed cost.
///
/// `Primitive` / `Substitution` are likewise DIAGNOSTIC-only (per-op gas):
/// they appear in the event log/digest but never gate consensus. The split
/// is INTERNAL (never on the wire) — it only affects how `reconcile_lane`
/// tallies the per-COMM consensus cost vs. the diagnostic stream.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BillableKind {
    /// Token-consuming COMM reduction (send / receive). Consensus cost = 1.
    Comm,
    /// Non-COMM structural reduction (new / match / if). Diagnostic; cost = 0.
    Reduction,
    Primitive(String),
    Substitution,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BillableTokenEvent {
    pub deploy_id: [u8; 32],
    /// Per-signature lane key (`Sig::lane_hash`) of the deploy's signature.
    /// Placed immediately after `deploy_id` so the derived `Ord` on
    /// `BillableTokenEvent` orders by `(deploy_id, sig_hash, source_path,
    /// redex_id, local_index, kind, weight)`. Both `deploy_id` and `sig_hash`
    /// are constant within a single deploy (the signature is installed before
    /// evaluation begins), so the per-lane order — the projection of events
    /// onto a fixed `sig_hash` — is a strict REFINEMENT of the global order:
    /// the global walk over all events, restricted to one lane, visits that
    /// lane's events in exactly the lane's own canonical order. This is the
    /// `sig_hash`-second-key invariant the spectral decomposition (spec §4.6,
    /// §7.6 "no interleaving is PER-SIGNATURE") relies on. In D-scope every
    /// deploy carries ONE compound lane, so `sig_hash` is identical across a
    /// deploy's events and the scalar fast path is unaffected.
    pub sig_hash: [u8; 32],
    pub source_path: SourcePath,
    pub redex_id: RedexId,
    pub local_index: u64,
    pub kind: BillableKind,
    pub weight: u64,
}

impl RuntimeBudget {
    fn resolve_max_log_entries() -> usize { 1024 }

    pub fn new(initial_value: Cost) -> Self {
        let max_log_entries = Self::resolve_max_log_entries();
        let initial_capacity = if max_log_entries == 0 {
            0
        } else if max_log_entries == usize::MAX {
            1024
        } else {
            max_log_entries.min(1024)
        };

        Self {
            initial_tokens: Arc::new(AtomicI64::new(initial_value.value)),
            consumed_tokens: Arc::new(AtomicI64::new(0)),
            signature: Arc::new(Mutex::new(Sig::Unit)),
            deploy_id: Arc::new(Mutex::new([0; 32])),
            log: Arc::new(Mutex::new(VecDeque::with_capacity(initial_capacity))),
            event_log: Arc::new(Mutex::new(VecDeque::with_capacity(initial_capacity))),
            attempt_queue: Arc::new(SegQueue::new()),
            attempt_accumulator: Arc::new(Mutex::new(Vec::new())),
            canonical_reconciliation: Arc::new(Mutex::new(None)),
            max_log_entries,
            unmetered: Arc::new(AtomicU64::new(0)),
            // N=1 fast path: the lane pool starts empty and stays empty for
            // every legacy single-signature deploy (mirrors `rspace.rs`
            // `phase_a_locks` construction).
            lanes: Arc::new(DashMap::new()),
        }
    }

    pub fn unmetered() -> Self {
        let budget = Self::new(Cost::unsafe_max());
        budget.unmetered.store(1, Ordering::Release);
        budget
    }

    pub fn reserve_canonical_with_cost(
        &self,
        event: BillableTokenEvent,
        amount: Cost,
    ) -> Result<(), InterpreterError> {
        // Unmetered mode bypasses validation AND billing, matching the
        // pre-Option-E commit_canonical_batch contract (system deploys
        // can charge arbitrary weights). Mirrored in reserve_canonical
        // and commit_canonical_batch.
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(());
        }
        Self::validate_billable_event(&event)?;
        // SAFETY: per-deploy finalization is single-threaded by contract
        // (reset happens strictly between deploys, never concurrently
        // with in-flight attempts), so no reset-vs-attempt serializer is
        // needed. Recording is lock-free via `attempt_one`.
        let outcome = self.attempt_one(event, Some(amount));
        match outcome {
            AttemptOutcome::Granted => Ok(()),
            AttemptOutcome::Oop => Err(InterpreterError::OutOfPhlogistonsError),
        }
    }

    // The per-event `append_cost_log` / `append_event_log` helpers were
    // removed from the hot path in Milestone 2: the diagnostic `log` /
    // `event_log` ring buffers are now repopulated from the canonical
    // committed set at finalization (see `repopulate_diagnostic_logs`,
    // called from `reconcile`), so nothing appends to them per-grant
    // anymore. Their bounded ring-buffer push logic is inlined into
    // `repopulate_diagnostic_logs`.
    //
    // pub(crate) fn append_cost_log(&self, amount: Cost) {
    //     if self.max_log_entries > 0 {
    //         let mut log = self.log.lock().unwrap();
    //         if log.len() >= self.max_log_entries {
    //             let _ = log.pop_front();
    //         }
    //         log.push_back(amount);
    //     }
    // }
    //
    // fn append_event_log(&self, event: BillableTokenEvent) {
    //     if self.max_log_entries > 0 {
    //         let mut log = self.event_log.lock().unwrap();
    //         if log.len() >= self.max_log_entries {
    //             let _ = log.pop_front();
    //         }
    //         log.push_back(event);
    //     }
    // }

    fn validate_billable_event(event: &BillableTokenEvent) -> Result<(), InterpreterError> {
        if event.weight == 0 || event.weight > i64::MAX as u64 {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        if event.source_path.0.len() > MAX_COST_TRACE_SOURCE_PATH_COMPONENTS {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        if let BillableKind::Primitive(name) = &event.kind {
            if name.len() > MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES {
                return Err(InterpreterError::OutOfPhlogistonsError);
            }
        }

        Ok(())
    }

    pub fn reserve_canonical(&self, event: BillableTokenEvent) -> Result<(), InterpreterError> {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(());
        }
        Self::validate_billable_event(&event)?;
        // SAFETY: per-deploy finalization is single-threaded by contract
        // (reset happens strictly between deploys), so no reset-vs-attempt
        // serializer is needed. Recording is lock-free via `attempt_one`.
        let outcome = self.attempt_one(event, None);
        match outcome {
            AttemptOutcome::Granted => Ok(()),
            AttemptOutcome::Oop => Err(InterpreterError::OutOfPhlogistonsError),
        }
    }

    /// Record one reservation attempt and try to claim its weight from
    /// `consumed_tokens` via lock-free CAS. The attempt is ALWAYS pushed
    /// to `attempt_queue` (so the canonical reconciliation sees it even if
    /// the CAS race grants nothing). Returns whether the runtime should
    /// let the caller's branch proceed (`Granted`) or abort it (`Oop`).
    ///
    /// The runtime's grant/oop decision is for liveness only; the
    /// consensus-relevant commit set is computed post-hoc by `reconcile()`.
    ///
    /// API contract: callers must NOT call `reconcile()` (or any reader
    /// that triggers it — `total_cost`, `cost_trace_digest`, etc.) before
    /// all `attempt_one` calls for a given deploy have completed. A
    /// mid-deploy read drains the then-current queue and caches a partial
    /// reconciliation; the cache is recomputed on the next read once more
    /// attempts have been drained (drain-append-recompute). Per-deploy
    /// finalization is single-threaded by contract at the call sites in
    /// `runtime.rs::process_deploy` and `replay_runtime.rs::replay`.
    fn attempt_one(&self, event: BillableTokenEvent, amount: Option<Cost>) -> AttemptOutcome {
        // Unmetered fast path: system deploys + scoped unmetered scopes
        // bypass billing entirely. Don't touch the attempt queue so the
        // reconciliation stays empty (unmetered budgets are not subject
        // to consensus authentication of cost trace).
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return AttemptOutcome::Granted;
        }

        // Record the attempt for the canonical reconciliation. Pushed
        // lock-free before the CAS so the reconciliation sees every
        // attempt even if the CAS race grants nothing. The reconciliation
        // cache is NOT invalidated here — `reconcile()` recomputes
        // whenever it drains newly-enqueued attempts, keeping the hot
        // path free of the cache Mutex.
        self.attempt_queue.push(AttemptRecord {
            event: event.clone(),
            amount,
        });

        let initial = self.initial_tokens.load(Ordering::Acquire);
        // D3 (DR-9, OD-3): the liveness gate is PER-COMM, coherent with the
        // consensus tally in `reconcile_lane`. A COMM costs ONE token; a
        // diagnostic event (Reduction / Primitive / Substitution) costs ZERO
        // and is ALWAYS granted (it can never exhaust the budget). The per-op
        // `weight` is diagnostic only and no longer gates liveness.
        let cost_unit = Self::consensus_cost_unit(&event.kind);

        // A zero-cost event always proceeds (it does not touch the budget).
        if cost_unit == 0 {
            return AttemptOutcome::Granted;
        }

        // Lock-free CAS loop for a COMM (cost_unit == 1). On overflow, return
        // Oop without writing the clamp — the canonical reconciliation
        // establishes the consensus consumed/OOP values; this counter is just a
        // liveness gate.
        let mut current = self.consumed_tokens.load(Ordering::Acquire);
        loop {
            if current < 0 || initial < 0 {
                return AttemptOutcome::Oop;
            }
            if current >= initial {
                return AttemptOutcome::Oop;
            }
            let next = current.saturating_add(cost_unit);
            if next > initial {
                return AttemptOutcome::Oop;
            }
            match self.consumed_tokens.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                // Runtime grant is liveness only; the diagnostic
                // `event_log`/`log` mirrors and the consensus-relevant
                // commit set are derived from `reconcile()` at
                // finalization, NOT mirrored per-grant on the hot path.
                Ok(_) => return AttemptOutcome::Granted,
                Err(actual) => current = actual,
            }
        }
    }

    /// Batch entry point retained for callers that issue multi-event
    /// reservations as a single canonical-ordered unit. Each event is
    /// processed via the same lock-free `attempt_one` path; permits/oop
    /// are aggregated.
    ///
    /// SAFETY: per-deploy finalization is single-threaded by contract
    /// (reset happens strictly between deploys, never concurrently with
    /// in-flight batch commits), so no reset-vs-commit serializer is
    /// needed.
    pub fn commit_canonical_batch(
        &self,
        batch: CostReservationBatch,
    ) -> Result<CostCommit, InterpreterError> {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(CostCommit {
                permits: batch
                    .events
                    .into_iter()
                    .map(|event| ExecutionPermit {
                        weight: event.weight,
                        event,
                    })
                    .collect(),
                consumed_weight: 0,
                oop: None,
            });
        }

        for event in &batch.events {
            Self::validate_billable_event(event)?;
        }

        // Canonical intra-batch order: sort before walking so the
        // CostCommit returned is invariant under input permutation.
        // This is the existing per-batch contract (verified by
        // `canonical_batch_commit_is_permutation_invariant`). The
        // post-hoc cross-batch canonical reconciliation in `reconcile()`
        // applies the same sort over the union of all attempts.
        let mut events = batch.events;
        events.sort();

        let mut permits = Vec::new();
        let mut consumed_weight = 0u64;
        let mut oop = None;
        for event in events {
            let weight = event.weight;
            match self.attempt_one(event.clone(), None) {
                AttemptOutcome::Granted => {
                    consumed_weight = consumed_weight.saturating_add(weight);
                    permits.push(ExecutionPermit { weight, event });
                }
                AttemptOutcome::Oop => {
                    oop = Some(event);
                    break;
                }
            }
        }

        Ok(CostCommit {
            permits,
            consumed_weight,
            oop,
        })
    }

    /// Drain the attempt queue into the reconciliation accumulator and
    /// compute the canonical reconciliation. Pure function of (all
    /// recorded attempts, initial_tokens). Idempotent: subsequent calls
    /// with no newly-enqueued attempts return the cached value without
    /// re-walking (drain-append-recompute).
    ///
    /// Called at deploy finalization (single-threaded by contract — the
    /// caller is `runtime.rs::process_deploy` / `replay_runtime.rs::replay`
    /// after `evaluate()` joins the parallel reducer). Calls before
    /// finalization drain the then-current queue and cache a partial
    /// reconciliation; once more attempts have been enqueued, the next
    /// call drains them, appends to the accumulator, and recomputes.
    fn reconcile(&self) -> CanonicalReconciliation {
        // The cache Mutex serializes finalization and also guards the
        // accumulator + diagnostic mirrors during recompute. The hot
        // path (`attempt_one`) never touches it — it only pushes to the
        // lock-free `attempt_queue`.
        let mut cache = self
            .canonical_reconciliation
            .lock()
            .expect("reconciliation cache lock");

        // Drain the lock-free attempt queue into the accumulator. This is
        // the ONLY place (besides `reset`) that touches the accumulator,
        // so the per-event path stays lock-free.
        let mut drained_any = false;
        {
            let mut accumulator = self
                .attempt_accumulator
                .lock()
                .expect("attempt accumulator poisoned");
            while let Some(record) = self.attempt_queue.pop() {
                accumulator.push(record);
                drained_any = true;
            }
        }

        // Idempotent fast path: nothing new drained and a prior result
        // is cached → return it without re-walking.
        if !drained_any {
            if let Some(rec) = cache.as_ref() {
                return rec.clone();
            }
        }

        let initial = self.initial_tokens.load(Ordering::Acquire);

        // The canonical commit walk is shared with the per-signature lanes
        // (`reconcile_lane`): scalar and per-lane reconciliation run the SAME
        // pure walk over (initial, attempts), so the N=1 scalar path stays
        // byte-identical to the pre-D0 implementation.
        let attempts: Vec<AttemptRecord> = {
            let accumulator = self
                .attempt_accumulator
                .lock()
                .expect("attempt accumulator poisoned");
            accumulator.clone()
        };
        let rec = Self::reconcile_lane(initial, &attempts);

        // Repopulate the diagnostic `event_log` / `log` mirrors from the
        // canonical committed set. This moves their population OFF the
        // hot path (they were previously appended per-grant inside
        // `attempt_one`) and onto finalization, so `get_event_log` /
        // `get_log` now reflect the canonical committed set rather than a
        // schedule-dependent record of CAS-race winners.
        self.repopulate_diagnostic_logs(&rec.committed, &rec.cost_amounts);

        *cache = Some(rec.clone());
        rec
    }

    /// Pure canonical reconciliation over one signature's pool: given an
    /// `initial` budget and the multiset of reservation `attempts` recorded
    /// for that signature, return the canonical `CanonicalReconciliation`
    /// (committed set, OOP boundary, clamped `consumed_units`, reconstructed
    /// `cost_amounts`) in the schedule-INDEPENDENT canonical reduction order.
    ///
    /// This is the EXACT walk previously inlined in `reconcile()`; extracting
    /// it lets BOTH the scalar fast path (`reconcile`, one signature, lanes
    /// empty) and each per-signature lane (`reconcile_lane_pool`) call it. The
    /// scalar path is therefore byte-identical to the pre-D0 implementation
    /// (pinned by `legacy_single_sig_byte_identical`), and `total_cost` over
    /// the lane pool is a commutative sum of independent applications of this
    /// same function (spec §4.6 spectral decomposition; `rb_pool_total_cost =
    /// Σ rb_total_cost` in `RuntimeBudgetRefinement.v`).
    ///
    /// Pure: no `self` access, no interior mutation — output depends only on
    /// `(initial, attempts)`, never on Tokio scheduling.
    fn reconcile_lane(initial: i64, attempts: &[AttemptRecord]) -> CanonicalReconciliation {
        // D3 (DR-9, OD-3): the consensus cost unit is the COMM count, not the
        // per-op weight. `consumed_units` tallies ONE per committed
        // [`BillableKind::Comm`] event and ZERO for every other kind
        // (Reduction / Primitive / Substitution are DIAGNOSTIC). The liveness
        // OOP boundary likewise fires when the budget would admit one COMM too
        // many: a metered budget of `initial` tokens commits at most `initial`
        // COMMs, then OOPs on the next COMM. Zero-cost (non-COMM) events never
        // trigger OOP — they commit freely for diagnostic fidelity.
        //
        // The pre-D3 `initial`-rank K-window truncation assumed every event
        // cost >= 1; that no longer holds (only COMMs cost 1), so the walk is
        // bounded only by the hard `MAX_COST_TRACE_EVENTS` cap (the attempt
        // count is bounded upstream by `reduce.rs::eval_inner`'s term-count
        // limit). Accepted user deploys run with `initial == i64::MAX`
        // (unmetered-for-liveness, OD-1), so the boundary never fires and
        // `consumed_units` is the exact total COMM count.
        let k_bound = MAX_COST_TRACE_EVENTS as usize;

        // Canonical sort key is the derived Ord on BillableTokenEvent.
        // Multiplicity is preserved: a deploy that re-attempts the same
        // logical event (e.g. through a loop) MUST see the repeated
        // attempt contribute, just as it did under the pre-Option-E
        // commit_lock contract.
        let mut attempts: Vec<AttemptRecord> = attempts.to_vec();
        attempts.sort_by(|a, b| a.event.cmp(&b.event));
        if attempts.len() > k_bound {
            attempts.truncate(k_bound);
        }

        // Simulate the canonical commit walk, counting COMMs as the consensus
        // cost. `cost_unit_of` is 1 for a COMM, 0 otherwise.
        let mut committed = Vec::with_capacity(attempts.len());
        let mut cost_amounts: Vec<Cost> = Vec::new();
        let mut consumed_units: i64 = 0;
        let mut oop: Option<BillableTokenEvent> = None;

        for rec in attempts.into_iter() {
            let cost_unit = Self::consensus_cost_unit(&rec.event.kind);
            let next = consumed_units.saturating_add(cost_unit);
            // Only a COMM that would exceed the budget is an OOP boundary.
            // A zero-cost event (cost_unit == 0) can never exceed `initial`
            // (it leaves `consumed_units` unchanged), so it always commits.
            if cost_unit > 0 && next > initial {
                oop = Some(rec.event);
                consumed_units = initial;
                break;
            }
            consumed_units = next;
            if let Some(amount) = rec.amount {
                cost_amounts.push(amount);
            }
            committed.push(rec.event);
        }

        CanonicalReconciliation {
            committed,
            oop,
            consumed_units,
            cost_amounts,
        }
    }

    /// The per-event CONSENSUS cost contribution (DR-9 one-token-per-COMM):
    /// `1` for a token-consuming COMM (send / receive), `0` for every
    /// diagnostic kind (new / match / if Reductions, Primitives,
    /// Substitutions). This is the single source of truth for the per-COMM
    /// consensus tally used by [`Self::reconcile_lane`].
    #[inline]
    fn consensus_cost_unit(kind: &BillableKind) -> i64 {
        match kind {
            BillableKind::Comm => 1,
            BillableKind::Reduction | BillableKind::Primitive(_) | BillableKind::Substitution => 0,
        }
    }

    /// Record one reservation ATTEMPT into the per-signature lane keyed by
    /// `sig.lane_hash()`, creating the lane (seeded with `initial` tokens) on
    /// first touch. Lock-free: the lane is found-or-inserted via
    /// `DashMap::entry` (mirroring `rspace.rs` `phase_a_locks`), then the
    /// attempt is pushed onto the lane's wait-free `SegQueue`. Returns the
    /// runtime liveness outcome for the lane (`Granted`/`Oop`) via the lane's
    /// own CAS counter — exactly the scalar `attempt_one` contract, applied
    /// per lane.
    ///
    /// Disjoint signatures key disjoint `DashMap` entries (the
    /// `lane_pool_disjoint` corollary), so concurrent reservations against
    /// distinct signatures never contend (spec §7.6 per-signature
    /// no-interleaving). The scalar fields and the legacy `attempt_one` path
    /// are untouched, so leaving every deploy single-lane (`lanes` empty)
    /// preserves the N=1 byte-identical fast path.
    ///
    /// `#[allow(dead_code)]`: this is the lane WRITE path. D0 lands it as
    /// substrate (exercised by the in-crate tests); the production caller that
    /// routes a deploy's charges into lanes lands at the D2 funding gate.
    #[allow(dead_code)]
    fn attempt_in_lane(
        &self,
        sig: &Sig,
        initial: i64,
        event: BillableTokenEvent,
        amount: Option<Cost>,
    ) -> AttemptOutcome {
        let key = sig.lane_hash();
        let lane = self
            .lanes
            .entry(key)
            .or_insert_with(|| Lane::new(sig.clone(), initial));

        // Record the attempt for the lane's canonical reconciliation, pushed
        // lock-free before the CAS so the reconciliation sees every attempt
        // even if the CAS race grants nothing (scalar `attempt_one` contract).
        lane.attempt_queue.push(AttemptRecord {
            event: event.clone(),
            amount,
        });

        let lane_initial = lane.initial_tokens.load(Ordering::Acquire);
        // D3 (DR-9, OD-3): per-COMM liveness gate (identical to the scalar
        // `attempt_one`). A diagnostic (cost 0) event always proceeds; only a
        // COMM (cost 1) draws from the lane budget.
        let cost_unit = Self::consensus_cost_unit(&event.kind);
        if cost_unit == 0 {
            return AttemptOutcome::Granted;
        }

        // Lock-free CAS loop for a COMM, identical to the scalar `attempt_one`.
        let mut current = lane.consumed_tokens.load(Ordering::Acquire);
        loop {
            if current < 0 || lane_initial < 0 {
                return AttemptOutcome::Oop;
            }
            if current >= lane_initial {
                return AttemptOutcome::Oop;
            }
            let next = current.saturating_add(cost_unit);
            if next > lane_initial {
                return AttemptOutcome::Oop;
            }
            match lane.consumed_tokens.compare_exchange(
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

    /// Reconcile ONE lane: drain its lock-free `attempt_queue` into the lane
    /// accumulator and recompute its canonical reconciliation via the shared
    /// `reconcile_lane` walk (drain-append-recompute, idempotent). Mirrors the
    /// scalar `reconcile()` minus the diagnostic `event_log`/`log` mirrors
    /// (lanes carry no diagnostic ring buffers; the scalar budget owns those).
    fn reconcile_one_lane(lane: &Lane) -> CanonicalReconciliation {
        let mut cache = lane.reconciliation.lock().expect("lane reconciliation");

        let mut drained_any = false;
        {
            let mut accumulator = lane.accumulator.lock().expect("lane accumulator");
            while let Some(record) = lane.attempt_queue.pop() {
                accumulator.push(record);
                drained_any = true;
            }
        }

        if !drained_any {
            if let Some(rec) = cache.as_ref() {
                return rec.clone();
            }
        }

        let initial = lane.initial_tokens.load(Ordering::Acquire);
        let attempts: Vec<AttemptRecord> = {
            let accumulator = lane.accumulator.lock().expect("lane accumulator");
            accumulator.clone()
        };
        let rec = Self::reconcile_lane(initial, &attempts);
        *cache = Some(rec.clone());
        rec
    }

    /// Sum of consumed cost over ALL per-signature lanes (spec §4.6 spectral
    /// decomposition: the deploy's cost is `Σ_σ` over the per-signature pools).
    /// Commutative / order-independent: each lane reconciles independently via
    /// the pure `reconcile_lane` walk and the result is summed with saturating
    /// addition, so the total is invariant under the order in which lanes are
    /// visited (`rb_pool_total_cost = Σ rb_total_cost` in
    /// `RuntimeBudgetRefinement.v`). Returns `None` when the lane pool is empty
    /// — the signal that the deploy is on the N=1 scalar fast path.
    fn lane_pool_total_cost(&self) -> Option<i64> {
        if self.lanes.is_empty() {
            return None;
        }
        let mut total: i64 = 0;
        for lane in self.lanes.iter() {
            let consumed = Self::reconcile_one_lane(lane.value()).consumed_units;
            total = total.saturating_add(consumed);
        }
        Some(total)
    }

    /// Repopulate the bounded diagnostic `event_log` / `log` ring buffers
    /// from the canonical committed set. Called only from `reconcile()`
    /// (under the cache lock) at finalization. The ring buffers retain at
    /// most `max_log_entries` of the lowest-rank committed events/costs.
    fn repopulate_diagnostic_logs(&self, committed: &[BillableTokenEvent], cost_amounts: &[Cost]) {
        if self.max_log_entries == 0 {
            return;
        }
        {
            let mut event_log = self.event_log.lock().expect("event log");
            event_log.clear();
            let skip = committed.len().saturating_sub(self.max_log_entries);
            for event in committed.iter().skip(skip) {
                event_log.push_back(event.clone());
            }
        }
        {
            let mut log = self.log.lock().expect("cost log");
            log.clear();
            let skip = cost_amounts.len().saturating_sub(self.max_log_entries);
            for amount in cost_amounts.iter().skip(skip) {
                log.push_back(amount.clone());
            }
        }
    }

    pub fn get(&self) -> Cost {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Cost::unsafe_max();
        }
        let initial = self.initial_tokens.load(Ordering::Acquire);
        let consumed = self.reconcile().consumed_units;
        Cost::create(initial.saturating_sub(consumed), "token budget remaining")
    }

    pub fn set(&self, new_value: Cost) {
        let token = Token::coalesced(self.signature(), cost_value_to_token_count(new_value.value));
        self.reset_from_token(&token);
    }

    pub fn reset_from_signed_process(&self, signed: &SignedProcess) {
        if let Some(token) = signed.token() {
            self.reset_from_token(token);
        }
    }

    pub fn reset_from_token(&self, token: &Token) {
        // SAFETY: reset is strictly between deploys — per-deploy
        // finalization is single-threaded by contract, so reset never
        // races in-flight `attempt_one`/`commit_canonical_batch` calls.
        // No reset-vs-attempt serializer is needed.
        //
        // The cache Mutex is the single guard shared with `reconcile`;
        // holding it here orders this reset against any finalization
        // recompute on the same budget.
        let mut cache = self
            .canonical_reconciliation
            .lock()
            .expect("reconciliation cache lock");
        self.initial_tokens
            .store(token.remaining_units_i64(), Ordering::Release);
        self.consumed_tokens.store(0, Ordering::Release);
        *self.signature.lock().expect("signature lock") = token.signature();
        self.event_log.lock().expect("event log").clear();
        self.log.lock().expect("cost log").clear();
        // Drain and discard any residual lock-free attempts, then clear
        // the reconciliation accumulator.
        while self.attempt_queue.pop().is_some() {}
        self.attempt_accumulator
            .lock()
            .expect("attempt accumulator poisoned")
            .clear();
        // Clear the per-signature lane pool so the reused budget starts on the
        // N=1 scalar fast path again (mirrors `rspace.rs` `phase_a_locks.clear`).
        self.lanes.clear();
        *cache = None;
    }

    pub fn set_deploy_signature(&self, signature: &[u8]) {
        // The envelope `Sig` is derived by the ONE shared function
        // [`envelope_sig_single`] so the runtime install, the D2 acceptance
        // gate, and replay never drift (cost-accounting WD-D2 §D2.2 — one
        // extracted derivation). The deploy-signature digest is a `#P`-style
        // process-hash (the Blake2b256 of the domain-separated wire signature),
        // NOT a ground key `g`, so it is a `Sig::Quote` atom (eq:app-sig-hash).
        let sig = envelope_sig_single(signature);
        // The deploy_id reuses the same digest: a `Sig::Quote(hash)` carries
        // exactly the domain-separated Blake2b256, so this is byte-identical to
        // the pre-extraction inline derivation.
        let mut deploy_id = [0; 32];
        match &sig {
            Sig::Quote(hash) => deploy_id.copy_from_slice(&hash[..32]),
            // `envelope_sig_single` is total to `Sig::Quote`; this arm is
            // unreachable and exists only to keep the match exhaustive.
            _ => unreachable!("envelope_sig_single always yields Sig::Quote"),
        }
        *self.deploy_id.lock().expect("deploy id lock") = deploy_id;
        // Cost-accounting channels are internal capabilities derived from,
        // but not equal to, the wire signature. Domain separation prevents
        // accidental reuse of raw signature bytes as another protocol hash.
        *self.signature.lock().expect("signature lock") = sig;
    }

    /// Install a compound (multi-signer) deploy signature into the budget.
    ///
    /// `signatures` MUST be non-empty and in canonical (ascending pk.bytes)
    /// order — the caller (the deploy-decoder boundary at
    /// [`Cosigned::from_signed_data`](crypto::rust::signatures::signed::Cosigned))
    /// enforces this. Each entry is the raw wire signature bytes of one cosigner.
    ///
    /// Folds the hashes into a **left-associated** `Sig::And` tree, matching
    /// the operational semantics of the cost-accounted rho-calculus paper's
    /// `σ₁ & σ₂` compound-signature operator (§3.2 Rules 2-5): fuel must come
    /// from BOTH (all) component signature channels. The signature commutativity
    /// at the `SignatureChannel::from_sig` reflection layer (via
    /// `ParSortMatcher::sort_match`) means the choice of left-associativity is
    /// observable only in the wire-level `Sig` value, never in the reflected
    /// signature channel.
    ///
    /// The `deploy_id` is derived as
    /// `Blake2b256(COMPOUND_DEPLOY_SIGNATURE_DOMAIN || concat(domain_separated_hash(sig_i) for i))`,
    /// using a distinct domain separator from the legacy single-sig path so
    /// existing on-chain deploys keep their `deploy_id`s while multi-sig
    /// deploys obtain distinguishable ones.
    ///
    /// For `signatures.len() == 1` this is observably distinct from
    /// [`set_deploy_signature`] (different `deploy_id` due to different domain
    /// separator), but operationally equivalent in terms of the resulting
    /// `Sig::Quote` value and `SignatureChannel` reflection.
    pub fn set_deploy_signatures(&self, signatures: &[&[u8]]) {
        assert!(
            !signatures.is_empty(),
            "set_deploy_signatures requires at least one signature"
        );

        // Domain-separated hash of each individual wire signature. Per-signature
        // domain separation uses the COMPOUND domain so single-element calls
        // remain distinguishable from legacy single-sig deploys. Shared with
        // [`envelope_sig_compound`] so the runtime install and the D2 gate /
        // replay derive ONE identical compound `Sig` (no drift).
        let sig_hashes = compound_sig_hashes(signatures);

        // Fold into the left-associated `Sig::And` tree via the ONE shared
        // function (WD-D2 §D2.2 — single extracted derivation):
        //   [h0]          => Sig::Quote(h0)
        //   [h0, h1]      => Sig::And(Sig::Quote(h0), Sig::Quote(h1))
        //   [h0, h1, h2]  => Sig::And(Sig::And(Sig::Quote(h0), Sig::Quote(h1)), Sig::Quote(h2))
        let folded_sig: Sig = fold_compound_sig(&sig_hashes);

        // deploy_id derives from the full ordered concatenation of per-sig
        // hashes under the COMPOUND domain. Canonical-order input means
        // permutation-equal multi-sig deploys produce identical deploy_ids.
        let mut id_buf =
            Vec::with_capacity(COMPOUND_DEPLOY_SIGNATURE_DOMAIN.len() + 32 * sig_hashes.len());
        id_buf.extend_from_slice(COMPOUND_DEPLOY_SIGNATURE_DOMAIN);
        for h in &sig_hashes {
            id_buf.extend_from_slice(h);
        }
        let deploy_id_hash = Blake2b256::hash(id_buf);
        let mut deploy_id = [0_u8; 32];
        deploy_id.copy_from_slice(&deploy_id_hash[..32]);

        *self.deploy_id.lock().expect("deploy id lock") = deploy_id;
        *self.signature.lock().expect("signature lock") = folded_sig;
    }

    pub fn signature(&self) -> Sig { self.signature.lock().expect("signature lock").clone() }

    pub fn deploy_id(&self) -> [u8; 32] { *self.deploy_id.lock().expect("deploy id lock") }

    pub fn set_unmetered(&self, unmetered: bool) {
        // System deploys use unmetered mode only around post-evaluation
        // settlement work. The flag intentionally bypasses runtime fuel
        // reservation instead of crediting tokens back to the user budget;
        // turning it off restores the same consumed/remaining counters. New
        // consensus paths should prefer `enter_unmetered_scope`, which
        // restores this flag on every return path.
        self.unmetered
            .store(if unmetered { 1 } else { 0 }, Ordering::Release);
    }

    pub fn enter_unmetered_scope(&self) -> UnmeteredBudgetScope {
        let previous = self.unmetered.swap(1, Ordering::AcqRel);
        UnmeteredBudgetScope {
            budget: self.clone(),
            previous,
        }
    }

    /// Consensus-relevant consumed cost. Reads the canonical reconciliation
    /// (schedule-independent) rather than the runtime CAS counter — the
    /// counter is a liveness gate and may not match the canonical commit
    /// when workers race. On OOP the reconciliation clamps to `initial`,
    /// preserving the `deploy.cost == phlo_limit` integration-test invariant.
    ///
    /// N=1 fast path: when the per-signature lane pool is empty (every legacy
    /// single-signature deploy), this runs the EXISTING scalar reconciliation
    /// byte-identically. When lanes are present, the deploy's cost is the
    /// order-independent SUM over the per-signature pools (spec §4.6 spectral
    /// decomposition; `lane_pool_total_cost`), each pool reconciled via the
    /// SAME `reconcile_lane` walk.
    pub fn total_cost(&self) -> Cost {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Cost::create(0, "unmetered token budget");
        }
        match self.lane_pool_total_cost() {
            Some(total) => Cost::create(total, "consumed source-token units (per-signature pool)"),
            None => Cost::create(
                self.reconcile().consumed_units,
                "consumed source-token units",
            ),
        }
    }

    pub fn remaining(&self) -> Cost { self.get() }

    /// Diagnostic running cost log. As of Milestone 2 this is the bounded
    /// ring-buffer of per-committed-event `Cost` values from the canonical
    /// reconciliation (NOT a schedule-dependent record of CAS-race
    /// winners): `reconcile()` repopulates it at finalization. Triggering
    /// `reconcile()` here ensures the mirror reflects all recorded
    /// attempts. For the consensus-relevant aggregate cost see
    /// `total_cost()`. `clear_log` empties it without affecting any
    /// consensus observable; a later `reconcile()` recompute repopulates.
    pub fn get_log(&self) -> Vec<Cost> {
        // Ensure the diagnostic mirror reflects the canonical committed
        // set (populated by `reconcile` at finalization).
        let _ = self.reconcile();
        self.log.lock().unwrap().iter().cloned().collect()
    }

    /// Diagnostic event log. As of Milestone 2 this returns the canonical
    /// committed set (bounded to the diagnostic ring-buffer capacity),
    /// repopulated by `reconcile()` at finalization rather than appended
    /// per CAS-grant. It is therefore schedule-independent and equal to
    /// `get_canonical_event_log` up to the ring-buffer bound.
    pub fn get_event_log(&self) -> Vec<BillableTokenEvent> {
        // Ensure the diagnostic mirror reflects the canonical committed
        // set (populated by `reconcile` at finalization).
        let _ = self.reconcile();
        self.event_log.lock().unwrap().iter().cloned().collect()
    }

    pub fn get_canonical_event_log(&self) -> Vec<BillableTokenEvent> {
        let mut events = self.get_event_log();
        events.sort();
        events
    }

    /// Consensus-relevant OOP boundary. Reads the canonical
    /// reconciliation, which includes any prior metered attempts even
    /// when the budget is currently in unmetered mode (only NEW unmetered
    /// attempts are skipped — see `attempt_one`'s unmetered fast path).
    pub fn last_oop_event(&self) -> Option<BillableTokenEvent> { self.reconcile().oop }

    pub fn clear_log(&self) { self.log.lock().unwrap().clear(); }

    pub fn clear_event_log(&self) { self.event_log.lock().unwrap().clear(); }

    /// Returns the finalized consensus-trace event count.
    ///
    /// This is the count of canonical-committed events plus 1 if a
    /// canonical OOP boundary exists. Driven by `reconcile()`; idempotent.
    /// Reflects prior metered attempts even in unmetered mode (only NEW
    /// unmetered attempts are skipped — see `attempt_one`).
    pub fn cost_trace_event_count(&self) -> u64 {
        let rec = self.reconcile();
        rec.committed.len() as u64 + u64::from(rec.oop.is_some())
    }

    /// Builds the finalized consensus cost-trace digest.
    ///
    /// The digest is computed over the canonical reconciliation — a pure
    /// function of (program + initial budget), independent of Tokio
    /// scheduling. This is strictly stronger than the previous "trace of
    /// one runtime schedule" contract: any property provable about the
    /// previous digest restricted to a canonical schedule is provable here,
    /// and schedule-invariance is now an additional theorem.
    ///
    /// See paper §3 Rule 1 (single shared signature/token within a deploy)
    /// and `formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla`'s
    /// `RuntimeRaceDoesNotChangeReconciledDigest` invariant.
    pub fn cost_trace_digest(&self) -> CostTraceDigest {
        fn feed_len_prefixed(update: &mut dyn FnMut(&[u8]), data: &[u8]) {
            update(&(data.len() as u64).to_le_bytes());
            update(data);
        }

        fn feed_event(update: &mut dyn FnMut(&[u8]), tag: u8, event: &BillableTokenEvent) {
            update(&[tag]);
            update(&event.deploy_id);
            update(&(event.source_path.0.len() as u64).to_le_bytes());
            for component in &event.source_path.0 {
                update(&component.to_le_bytes());
            }
            update(&event.redex_id.0.to_le_bytes());
            update(&event.local_index.to_le_bytes());
            // D3 (OD-3): kind tag — `Primitive` and `Substitution` keep their
            // legacy tags (1, 2); the former `SourceStep` tag (0) is RETIRED
            // and split into `Comm` (3) and `Reduction` (4). All tags remain
            // distinct so the diagnostic digest stays collision-free.
            match &event.kind {
                BillableKind::Primitive(name) => {
                    update(&[1]);
                    feed_len_prefixed(update, name.as_bytes());
                }
                BillableKind::Substitution => update(&[2]),
                BillableKind::Comm => update(&[3]),
                BillableKind::Reduction => update(&[4]),
            }
            update(&event.weight.to_le_bytes());
        }

        // Unmetered mode doesn't blank the digest — prior metered
        // attempts (from before set_unmetered was toggled) remain in
        // the attempt log and continue to participate in the canonical
        // reconciliation. Only NEW attempts under unmetered mode are
        // skipped (`attempt_one`'s unmetered fast path).
        let rec = self.reconcile();

        let mut tagged_events: Vec<(u8, BillableTokenEvent)> = rec
            .committed
            .into_iter()
            .map(|event| (0u8, event))
            .collect();
        if let Some(event) = rec.oop {
            tagged_events.push((1u8, event));
        }
        // Canonical input is already sorted by event order; this sort
        // call is defensive (preserves the prior contract that the
        // digest is over a (tag, event)-sorted list).
        tagged_events.sort_by(|(left_tag, left), (right_tag, right)| {
            left_tag.cmp(right_tag).then_with(|| left.cmp(right))
        });

        let digest = Blake2b256::hash_stream(|update| {
            update(COST_TRACE_DIGEST_DOMAIN);
            update(&(tagged_events.len() as u64).to_le_bytes());
            for (tag, event) in &tagged_events {
                feed_event(update, *tag, event);
            }
        });

        CostTraceDigest {
            digest,
            event_count: tagged_events.len() as u64,
        }
    }
}

pub struct UnmeteredBudgetScope {
    budget: RuntimeBudget,
    previous: u64,
}

impl Drop for UnmeteredBudgetScope {
    fn drop(&mut self) {
        self.budget
            .unmetered
            .store(self.previous, Ordering::Release);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Sig {
    /// `1` — multiplicative unit. Identity for `And` / `Tensor`: σ ⊗ 1 ≡ σ.
    Unit,
    /// Atomic GROUND signature `g ∈ G` (cost-accounted rho-calculus §App-A,
    /// eq:app-sig-ground): a ground signature key whose translation is
    /// `Σ⟦g⟧ = quote(H_g)`. Carries the opaque ground bytes. Distinct from
    /// `Quote` only in its wire `AtomKind` tag and its source-level
    /// translation (`H_g` vs `H(𝒫⟦P⟧)`); the cost behavior is identical (each
    /// atom gates exactly one token) and `SignatureChannel::from_sig` derives
    /// the SAME channel from equal bytes. `Ground` is the default atom axis
    /// (proto3 `AtomKind::GROUND = 0`), so a `SigAtom` decoded without an
    /// `atom_kind` field is a `Ground` atom — preserving backward compat.
    Ground(Vec<u8>),
    /// Atomic QUOTE signature `#P` (cost-accounted rho-calculus §App-A,
    /// eq:app-sig-hash): a cryptographic process-hash whose translation is
    /// `Σ⟦#P⟧ = quote(H(𝒫⟦P⟧))`. Carries the Blake2b256 of the
    /// domain-separated wire signature — a `#P`-style process hash, NOT a
    /// ground key. Produced by `set_deploy_signature` /
    /// `set_deploy_signatures`. Reflects to the SAME channel as a `Ground`
    /// atom of equal bytes (DR-1: the axis does not affect `Δ_s`).
    Quote(Vec<u8>),
    /// Compound conjunction — both signature channels must contribute fuel.
    /// Corresponds to the cost-accounted-rho paper's `σ₁ & σ₂` operator
    /// (`publications/cost-accounting/cost-accounted-rho.tex` line 288).
    /// In linear-logic terms, this is the multiplicative tensor `⊗`. The
    /// variant name `And` is preserved for backward compatibility with the
    /// existing Phase 1 substrate; Phase 3's full LL-rich rename to
    /// `Tensor` is deferred to a coordinated rename PR per plan §3.1.
    And(Box<Sig>, Box<Sig>),
    /// Phase 2: M-of-N quorum threshold. The deploy is authorized when
    /// at least `threshold` of the `members` signatures verify. Canonical
    /// ordering on `members` is enforced at Cosigned envelope construction
    /// (sort by hash bytes). `threshold` must satisfy `1 <= threshold <= members.len()`.
    ///
    /// Quorum is NOT cheaply derivable from `Plus`/`And` without `O(C(n,k))`
    /// blow-up, so `Threshold` is a primitive even in LL-rich designs.
    Threshold { threshold: u32, members: Vec<Sig> },
    /// Phase 3 LL-rich algebra — additive disjunction `⊕`.
    /// Signer's choice: at construction time, the signer commits to one
    /// branch (left = 0, right = 1) and only that branch's signature is
    /// required. The verifier reads the branch witness from the wire
    /// envelope. Inspired by `publications/TypedCurrency/typed_value.tex`
    /// §"Linearity: why the calculus must reject contraction" (line 307+).
    Plus(Box<Sig>, Box<Sig>),
    /// Phase 3 LL-rich algebra — additive conjunction (LL's *with*) `&`.
    /// Verifier's choice: both branches' signatures must be present; the
    /// verifier (block proposer) picks which branch's fuel actually flows
    /// at evaluation time. Dual to `Plus`.
    With(Box<Sig>, Box<Sig>),
    /// Phase 3 LL-rich algebra — exponential `!` (of-course / bang).
    /// Replicable signature: same authorization witnesses many reductions.
    /// LL-canonical: unbounded uses. Bounded variant available via the
    /// `rho:system:capabilities` registry (Phase 3 §3.5).
    Bang(Box<Sig>),
    /// Phase 3 LL-rich algebra — exponential `?` (why-not).
    /// Optional / zero-or-more uses. Dual to `Bang`. Allows deploys whose
    /// authorization is "may be present" — verifier accepts whether or
    /// not the wrapped signature is presented.
    WhyNot(Box<Sig>),
    /// Phase 3 LL-rich algebra — linear implication `⊸` (lolly).
    /// Capability delegation: presenting a `from` signature produces a
    /// `to` signature via the registered transformer process. Stored
    /// on-chain in the `rho:system:capabilities` registry contract per
    /// Phase 3 §3.5 design.
    Lolly(Box<Sig>, Box<Sig>),
}

/// Derive the envelope `Sig` of a SINGLE-signer deploy from its raw wire
/// signature bytes — the spec's `#P`-style process-hash atom
/// `Sig::Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ sig))` (eq:app-sig-hash).
///
/// This is the ONE extracted derivation shared by the runtime install
/// ([`RuntimeBudget::set_deploy_signature`]), the D2 acceptance gate
/// (`casper/.../util/rholang/acceptance.rs`), and replay
/// (`casper/.../rholang/replay_runtime.rs`) so the three can never drift on the
/// envelope signature that keys the supply pool `Σ⟦s⟧` (cost-accounting WD-D2
/// §D2.2 — getting the envelope wrong mis-keys the pool, so it MUST match the
/// install). Legacy `DEPLOY_SIGNATURE_DOMAIN` so on-chain single-sig deploys
/// keep their identity bit-for-bit.
pub fn envelope_sig_single(signature: &[u8]) -> Sig {
    let mut domain_separated_signature =
        Vec::with_capacity(DEPLOY_SIGNATURE_DOMAIN.len() + signature.len());
    domain_separated_signature.extend_from_slice(DEPLOY_SIGNATURE_DOMAIN);
    domain_separated_signature.extend_from_slice(signature);
    Sig::Quote(Blake2b256::hash(domain_separated_signature))
}

/// Per-signer domain-separated Blake2b256 hashes under the COMPOUND domain, in
/// the (canonical, pk-ascending) order the caller supplies. Shared by
/// [`RuntimeBudget::set_deploy_signatures`] (which additionally folds the
/// concatenation into the `deploy_id`) and [`fold_compound_sig`].
fn compound_sig_hashes(signatures: &[&[u8]]) -> Vec<Vec<u8>> {
    let mut sig_hashes: Vec<Vec<u8>> = Vec::with_capacity(signatures.len());
    for sig_bytes in signatures.iter() {
        let mut domain_separated =
            Vec::with_capacity(COMPOUND_DEPLOY_SIGNATURE_DOMAIN.len() + sig_bytes.len());
        domain_separated.extend_from_slice(COMPOUND_DEPLOY_SIGNATURE_DOMAIN);
        domain_separated.extend_from_slice(sig_bytes);
        sig_hashes.push(Blake2b256::hash(domain_separated));
    }
    sig_hashes
}

/// Fold per-signer hashes into the left-associated `Sig::And` tree (each leaf a
/// `Sig::Quote` `#P`-atom), matching the cost-accounted rho-calculus `σ₁ & σ₂`
/// compound operator (§3.2 Rules 2-5): fuel must come from ALL component
/// channels. `hashes` MUST be non-empty (the caller guarantees ≥1 signer).
fn fold_compound_sig(hashes: &[Vec<u8>]) -> Sig {
    let mut iter = hashes.iter().cloned();
    let first = iter
        .next()
        .expect("fold_compound_sig requires at least one signature hash");
    iter.fold(Sig::Quote(first), |acc, hash| {
        Sig::And(Box::new(acc), Box::new(Sig::Quote(hash)))
    })
}

/// Derive the envelope `Sig` of a COMPOUND (multi-signer) deploy from the
/// canonically-ordered per-signer wire signatures — the left-associated
/// `Sig::And` fold of `Sig::Quote(Blake2b256(COMPOUND_DEPLOY_SIGNATURE_DOMAIN ‖
/// sig_i))`. The same extracted derivation the runtime install uses (no drift,
/// WD-D2 §D2.2). `signatures` MUST be non-empty and in canonical pk-ascending
/// order (the `Cosigned` constructor enforces this).
pub fn envelope_sig_compound(signatures: &[&[u8]]) -> Sig {
    fold_compound_sig(&compound_sig_hashes(signatures))
}

/// The ONE function that derives a deploy's envelope `Sig` from its
/// [`Cosigned`](crypto::rust::signatures::signed::Cosigned) envelope, used
/// IDENTICALLY by the runtime install, the D2 acceptance gate, and replay
/// (cost-accounting WD-D2 §D2.2). Dispatches on arity EXACTLY as
/// [`crate::rust::interpreter::rho_runtime`]'s install site does
/// (`casper/.../rholang/runtime.rs::evaluate_cosigned`): a single signer is the
/// legacy `Sig::Quote` over `DEPLOY_SIGNATURE_DOMAIN`; a compound is the
/// left-associated `Sig::And` fold over `COMPOUND_DEPLOY_SIGNATURE_DOMAIN`.
///
/// Under the s₀ collapse the envelope `Sig` drives ONLY the deploy's `sig_key`
/// (= [`Sig::lane_hash`]) and hence its supply channel `Σ⟦s⟧`; getting it wrong
/// mis-keys the pool. Anchoring gate + install + replay to this single function
/// is the no-drift guarantee.
pub fn envelope_sig<A>(cosigned: &crypto::rust::signatures::signed::Cosigned<A>) -> Sig
where A: std::fmt::Debug + serde::Serialize + crypto::rust::signatures::signed::ToMessage {
    if cosigned.is_compound() {
        let sigs: Vec<&[u8]> = cosigned.signers().iter().map(|s| s.sig.as_ref()).collect();
        envelope_sig_compound(&sigs)
    } else {
        envelope_sig_single(&cosigned.primary().sig)
    }
}

impl Sig {
    /// Serialize the runtime `Sig` algebra into the `SigCompound`
    /// wire-format proto message (Phase 2+3 `CasperMessage.proto`).
    /// `Sig::Ground`/`Sig::Quote` become a `SigAtom` whose `atom_kind`
    /// records the axis (`GROUND` vs `QUOTE`); pk + sig + sigAlgorithm are
    /// unavailable at this layer — they live on `Cosigner`); for the
    /// substrate-only serialization, atomic signatures are encoded as
    /// `pk = hash_bytes` placeholder. Downstream Cosigned-shape encoders
    /// (`models/src/rust/casper/protocol/casper_message.rs`) populate the
    /// full SigAtom from the matching Cosigner.
    pub fn to_proto(&self) -> models::casper::SigCompound {
        use models::casper::{
            sig_compound, AtomKind, SigAtom, SigBang, SigCompound, SigLolly, SigPair, SigPlus,
            SigThreshold,
        };
        let connective = match self {
            Sig::Unit => sig_compound::Connective::Atom(SigAtom {
                pk: Default::default(),
                sig: Default::default(),
                sig_algorithm: String::new(),
                atom_kind: AtomKind::Ground as i32,
            }),
            Sig::Ground(bytes) => sig_compound::Connective::Atom(SigAtom {
                pk: bytes.clone().into(),
                sig: Default::default(),
                sig_algorithm: String::new(),
                atom_kind: AtomKind::Ground as i32,
            }),
            Sig::Quote(bytes) => sig_compound::Connective::Atom(SigAtom {
                pk: bytes.clone().into(),
                sig: Default::default(),
                sig_algorithm: String::new(),
                atom_kind: AtomKind::Quote as i32,
            }),
            Sig::And(left, right) => sig_compound::Connective::Tensor(Box::new(SigPair {
                left: Some(Box::new(left.to_proto())),
                right: Some(Box::new(right.to_proto())),
            })),
            Sig::Threshold { threshold, members } => {
                sig_compound::Connective::Threshold(SigThreshold {
                    threshold: *threshold as i32,
                    members: members.iter().map(|m| m.to_proto()).collect(),
                })
            }
            Sig::Plus(left, right) => sig_compound::Connective::Plus(Box::new(SigPlus {
                left: Some(Box::new(left.to_proto())),
                right: Some(Box::new(right.to_proto())),
                chosen_branch: 0,
            })),
            Sig::With(left, right) => sig_compound::Connective::With(Box::new(SigPair {
                left: Some(Box::new(left.to_proto())),
                right: Some(Box::new(right.to_proto())),
            })),
            Sig::Bang(inner) => sig_compound::Connective::Bang(Box::new(SigBang {
                inner: Some(Box::new(inner.to_proto())),
                uses_bound: 0,
                capability_handle: Default::default(),
            })),
            Sig::WhyNot(inner) => sig_compound::Connective::Whynot(Box::new(inner.to_proto())),
            Sig::Lolly(from, to) => sig_compound::Connective::Lolly(Box::new(SigLolly {
                from: Some(Box::new(from.to_proto())),
                to: Some(Box::new(to.to_proto())),
                capability_handle: Default::default(),
            })),
        };
        SigCompound {
            connective: Some(connective),
        }
    }

    /// Deserialize a `SigCompound` wire-format proto into the runtime `Sig`
    /// algebra. The reverse of `Sig::to_proto`.
    pub fn from_proto(proto: &models::casper::SigCompound) -> Result<Sig, String> {
        use models::casper::sig_compound;
        let connective = proto
            .connective
            .as_ref()
            .ok_or_else(|| "SigCompound.connective missing".to_string())?;
        match connective {
            sig_compound::Connective::Atom(atom) => {
                use models::casper::AtomKind;
                if atom.pk.is_empty() {
                    Ok(Sig::Unit)
                } else {
                    // proto3 default `GROUND = 0` ⇒ a legacy atom decoded
                    // without an `atom_kind` field is a ground atom. Only an
                    // explicit `QUOTE` tag produces `Sig::Quote`; any unknown
                    // tag falls back to `Ground` (the conservative default).
                    match AtomKind::try_from(atom.atom_kind) {
                        Ok(AtomKind::Quote) => Ok(Sig::Quote(atom.pk.to_vec())),
                        Ok(AtomKind::Ground) | Err(_) => Ok(Sig::Ground(atom.pk.to_vec())),
                    }
                }
            }
            sig_compound::Connective::Tensor(pair) => {
                let left = Sig::from_proto(
                    pair.left
                        .as_ref()
                        .ok_or_else(|| "tensor.left missing".to_string())?,
                )?;
                let right = Sig::from_proto(
                    pair.right
                        .as_ref()
                        .ok_or_else(|| "tensor.right missing".to_string())?,
                )?;
                Ok(Sig::And(Box::new(left), Box::new(right)))
            }
            sig_compound::Connective::Plus(plus) => {
                let left = Sig::from_proto(
                    plus.left
                        .as_ref()
                        .ok_or_else(|| "plus.left missing".to_string())?,
                )?;
                let right = Sig::from_proto(
                    plus.right
                        .as_ref()
                        .ok_or_else(|| "plus.right missing".to_string())?,
                )?;
                Ok(Sig::Plus(Box::new(left), Box::new(right)))
            }
            sig_compound::Connective::With(pair) => {
                let left = Sig::from_proto(
                    pair.left
                        .as_ref()
                        .ok_or_else(|| "with.left missing".to_string())?,
                )?;
                let right = Sig::from_proto(
                    pair.right
                        .as_ref()
                        .ok_or_else(|| "with.right missing".to_string())?,
                )?;
                Ok(Sig::With(Box::new(left), Box::new(right)))
            }
            sig_compound::Connective::Bang(bang) => {
                let inner = Sig::from_proto(
                    bang.inner
                        .as_ref()
                        .ok_or_else(|| "bang.inner missing".to_string())?,
                )?;
                Ok(Sig::Bang(Box::new(inner)))
            }
            sig_compound::Connective::Whynot(inner_proto) => {
                let inner = Sig::from_proto(inner_proto)?;
                Ok(Sig::WhyNot(Box::new(inner)))
            }
            sig_compound::Connective::Lolly(lolly) => {
                let from = Sig::from_proto(
                    lolly
                        .from
                        .as_ref()
                        .ok_or_else(|| "lolly.from missing".to_string())?,
                )?;
                let to = Sig::from_proto(
                    lolly
                        .to
                        .as_ref()
                        .ok_or_else(|| "lolly.to missing".to_string())?,
                )?;
                Ok(Sig::Lolly(Box::new(from), Box::new(to)))
            }
            sig_compound::Connective::Threshold(thresh) => {
                let members: Result<Vec<Sig>, String> =
                    thresh.members.iter().map(Sig::from_proto).collect();
                Ok(Sig::Threshold {
                    threshold: thresh.threshold as u32,
                    members: members?,
                })
            }
        }
    }

    /// Canonical, collision-resistant, shape-agnostic per-signature lane key.
    ///
    /// THE INTEGRATION INVARIANT (supply-realization-c-d-handoff.md): the lane
    /// key and the supply channel `Σ⟦s⟧` MUST share one canonical basis so a
    /// deploy's lane key for signature `s` and its supply channel are derived
    /// from the same canonical signature serialization (no drift). We realize
    /// that by deriving the lane key DIRECTLY from the supply channel: the lane
    /// key is the Blake2b256 of the canonical wire encoding of the very `Par`
    /// that [`SignatureChannel::from_sig`] produces. Both the lane pool
    /// (`RuntimeBudget::lanes`) and the supply channel are therefore anchored
    /// to the single function `from_sig`, so two signatures share a lane iff
    /// they share a supply channel — exactly the no-drift property the C↔D
    /// handoff requires.
    ///
    /// Shape-agnostic over ALL `Sig` variants because `from_sig` is total over
    /// the algebra (`Unit`, `Ground`, `Quote`, `And`, `Threshold`, `Plus`,
    /// `With`, `Bang`, `WhyNot`, `Lolly`): the atom axis collapses at the
    /// channel (DR-1: equal atom bytes ⇒ equal channel) and compounds are made
    /// permutation-invariant by `ParSortMatcher::sort_match`, so `lane_hash`
    /// inherits the same canonical, axis-independent, permutation-invariant
    /// identity. Domain-separated (`SIGNATURE_LANE_DOMAIN`) so the lane-key
    /// digest can never collide with another protocol hash over the same Par
    /// bytes.
    pub fn lane_hash(&self) -> [u8; 32] {
        use prost::Message;
        let channel = SignatureChannel::from_sig(self).par;
        let encoded = channel.encode_to_vec();
        let mut domain_separated = Vec::with_capacity(SIGNATURE_LANE_DOMAIN.len() + encoded.len());
        domain_separated.extend_from_slice(SIGNATURE_LANE_DOMAIN);
        domain_separated.extend_from_slice(&encoded);
        let hash = Blake2b256::hash(domain_separated);
        let mut lane_key = [0_u8; 32];
        lane_key.copy_from_slice(&hash[..32]);
        lane_key
    }

    /// `true` iff this `Sig` is a member of the FUNDING-signature grammar of the
    /// cost-accounted rho-calculus (§App-A, `eq:app-sig-ground`/`eq:app-sig-hash`):
    ///
    /// ```text
    /// s(G) ::= g | #P | s ∘ s
    /// ```
    ///
    /// i.e. the ground/quote ATOMS (`Sig::Unit` — the `1` identity for `∘` —,
    /// `Sig::Ground` = `g`, `Sig::Quote` = `#P`) folded by the multiplicative
    /// tensor `∘` (`Sig::And`). This is EXACTLY what the Rocq `sig` inductive
    /// admits (`SUnit | SGround | SQuote | SAnd`, `CostAccountedSyntax.v`) and the
    /// only shape `accounting::envelope_sig*` ever constructs.
    ///
    /// Returns `false` for the VALUE/CAPABILITY type-logic connectives
    /// (`Sig::Plus` ⊕, `Sig::With` &, `Sig::Bang` !, `Sig::WhyNot` ?,
    /// `Sig::Lolly` ⊸) — these belong to the capability/type layer
    /// (`typed_value.tex`, `rho:system:capabilities` + W2), NOT to funding — and
    /// for `Sig::Threshold`: a `k`-of-`N` quorum is an admission-boundary
    /// predicate (F-A Threshold=(A)), lowered to a flat `Cosigned` + scalar
    /// `cosigner_threshold` at ingress and NEVER kept as a funding-`Sig` former,
    /// so the funding grammar stays exactly `g|#P|s∘s` (paper- + Rocq-faithful).
    ///
    /// F-A separation guard (`docs/theory/cost-accounting-impl/
    /// f-a-funding-vs-capability-separation.md` §3/§6): the funding chokepoint
    /// (`casper/.../acceptance.rs::build_candidate_with_logic`) asserts this on
    /// the envelope `Sig`, and the supply-channel keying
    /// (`casper/.../supply.rs::supply_channel`) + `SignatureChannel::from_sig`
    /// `debug_assert!` it as a precondition — so a value/capability connective can
    /// never key a funding supply pool `Σ⟦s⟧`.
    pub fn is_funding_former(&self) -> bool {
        match self {
            Sig::Unit | Sig::Ground(_) | Sig::Quote(_) => true,
            Sig::And(left, right) => left.is_funding_former() && right.is_funding_former(),
            Sig::Threshold { .. }
            | Sig::Plus(_, _)
            | Sig::With(_, _)
            | Sig::Bang(_)
            | Sig::WhyNot(_)
            | Sig::Lolly(_, _) => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Unit,
    Count { sig: Sig, remaining: u64 },
    Gate { sig: Sig, rest: Box<Token> },
}

impl Token {
    pub fn coalesced(sig: Sig, remaining: u64) -> Self { Token::Count { sig, remaining } }

    pub fn gate(sig: Sig, rest: Token) -> Self {
        Token::Gate {
            sig,
            rest: Box::new(rest),
        }
    }

    pub fn signature(&self) -> Sig {
        match self {
            Token::Unit => Sig::Unit,
            Token::Count { sig, .. } | Token::Gate { sig, .. } => sig.clone(),
        }
    }

    pub fn remaining_units(&self) -> u64 {
        match self {
            Token::Unit => 0,
            Token::Count { remaining, .. } => *remaining,
            Token::Gate { rest, .. } => 1u64.saturating_add(rest.remaining_units()),
        }
    }

    fn remaining_units_i64(&self) -> i64 { token_units_to_i64(self.remaining_units()) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignedProcess {
    Signed { process: Par, sig: Sig },
    Token(Token),
    Par(Box<SignedProcess>, Box<SignedProcess>),
}

impl SignedProcess {
    pub fn metered(process: Par, sig: Sig, token_count: u64) -> Self {
        SignedProcess::Par(
            Box::new(SignedProcess::Signed {
                process,
                sig: sig.clone(),
            }),
            Box::new(SignedProcess::Token(Token::coalesced(sig, token_count))),
        )
    }

    pub fn source_process(&self) -> Option<&Par> {
        match self {
            SignedProcess::Signed { process, .. } => Some(process),
            SignedProcess::Token(_) => None,
            SignedProcess::Par(left, right) => {
                left.source_process().or_else(|| right.source_process())
            }
        }
    }

    pub fn token(&self) -> Option<&Token> {
        match self {
            SignedProcess::Signed { .. } => None,
            SignedProcess::Token(token) => Some(token),
            SignedProcess::Par(left, right) => left.token().or_else(|| right.token()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureChannel {
    pub par: Par,
}

impl SignatureChannel {
    /// Reflect a `Sig` onto its content-addressed substrate channel.
    ///
    /// FUNDING PRECONDITION (F-A separation, `docs/theory/cost-accounting-impl/
    /// f-a-funding-vs-capability-separation.md` §3/§6/red-team M3): on every
    /// FUNDING path the argument is a funding-grammar `Sig`
    /// (`Sig::is_funding_former` — `g|#P|s∘s`). The six value/capability
    /// connective arms below (`Threshold`/`Plus`/`With`/`Bang`/`WhyNot`/`Lolly`)
    /// are CAPABILITY-LAYER ONLY (`typed_value.tex`, `rho:system:capabilities`)
    /// and are UNREACHABLE on the funding path — the envelope `Sig` is built
    /// solely by `accounting::envelope_sig*` (total to `Quote`/`And`), the
    /// acceptance gate rejects any non-funding envelope, and ingress
    /// (`from_proto_cosigned_with_sig_algebra`) rejects the five type-logic
    /// connectives before they reach a `Cosigned`.
    ///
    /// The `debug_assert!` that enforces this precondition lives on the FUNDING
    /// entry point [`crate::rust::interpreter::accounting`]'s
    /// `supply::supply_channel` (`casper/.../util/rholang/supply.rs`), NOT here:
    /// `from_sig` is deliberately TOTAL over the WHOLE algebra so the capability
    /// layer + the LL reflection round-trip tests (`ll_algebra_spec.rs`,
    /// `ll_rejection_spec.rs`, which call `from_sig` on `Plus`/`With`/`Bang`/
    /// `WhyNot`/`Lolly`/`Threshold` and assert reflection is non-panicking) keep
    /// working. Asserting inside this shared reflection primitive would
    /// (incorrectly) make those non-funding capability callers panic. See the
    /// red-team M3 deviation note in the F-A design doc.
    pub fn from_sig(sig: &Sig) -> Self {
        match sig {
            Sig::Unit => SignatureChannel {
                par: Par::default(),
            },
            // DR-1: the ground/quote axis does NOT affect the channel
            // derivation — both `Σ⟦g⟧` and `Σ⟦#P⟧` reflect to a quoted name,
            // and at the substrate the channel is the `GPrivate` keyed by the
            // content-hash of the atom bytes. Equal bytes ⇒ equal channel,
            // regardless of axis. Both arms are therefore byte-identical; the
            // distinction lives only in the wire `AtomKind` and the
            // source-level translation (`H_g` vs `H(𝒫⟦P⟧)`).
            Sig::Ground(bytes) | Sig::Quote(bytes) => SignatureChannel {
                par: Par::default().with_unforgeables(vec![GUnforgeable {
                    unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                        id: Blake2b256::hash(bytes.clone()),
                    })),
                }]),
            },
            Sig::And(left, right) => {
                let left_channel = Self::from_sig(left).par;
                let right_channel = Self::from_sig(right).par;
                let combined = concatenate_pars(left_channel, right_channel);
                SignatureChannel {
                    par: ParSortMatcher::sort_match(&combined).term,
                }
            }
            Sig::Threshold {
                threshold: _,
                members,
            } => {
                // Quorum reflection: concatenate ALL member channels under
                // ParSortMatcher::sort_match. The k-of-N quorum semantic is
                // enforced by the verifier layer (`Cosigned::from_signed_data`
                // for threshold envelopes — Phase 2 will extend that) which
                // accepts the deploy when at least `threshold` of `members`
                // signatures verify. The reflected channel is permutation-
                // invariant in `members` thanks to ParSortMatcher::sort_match,
                // matching the Sig::And case.
                let mut combined = Par::default();
                for member in members {
                    let member_channel = Self::from_sig(member).par;
                    combined = concatenate_pars(combined, member_channel);
                }
                SignatureChannel {
                    par: ParSortMatcher::sort_match(&combined).term,
                }
            }
            Sig::Plus(left, right) => {
                // Additive disjunction: signer's choice. The wire envelope
                // carries an explicit branch witness; at the substrate level
                // the reflected channel is the canonical-sorted union of
                // both branch channels (verifier reads the witness from the
                // envelope to know which branch's signature to validate).
                let left_channel = Self::from_sig(left).par;
                let right_channel = Self::from_sig(right).par;
                let combined = concatenate_pars(left_channel, right_channel);
                SignatureChannel {
                    par: ParSortMatcher::sort_match(&combined).term,
                }
            }
            Sig::With(left, right) => {
                // Additive conjunction (LL "with"): verifier's choice. Both
                // branches' channels are exposed; verifier picks at
                // evaluation time which branch's fuel flows. Reflection is
                // identical-shape to Plus at the substrate (channel
                // composition), with the distinction enforced by the
                // verifier's branch-selection logic.
                let left_channel = Self::from_sig(left).par;
                let right_channel = Self::from_sig(right).par;
                let combined = concatenate_pars(left_channel, right_channel);
                SignatureChannel {
                    par: ParSortMatcher::sort_match(&combined).term,
                }
            }
            Sig::Bang(inner) => {
                // Exponential bang `!σ`: replicable. The reflected channel
                // is the inner signature's channel; the replication semantic
                // is enforced by the registry contract layer (capability
                // store yields fresh fuel on each invocation). Phase 3 §3.5
                // capability registry implements the replication state.
                Self::from_sig(inner)
            }
            Sig::WhyNot(inner) => {
                // Exponential why-not `?σ`: optional. Reflected channel is
                // the inner signature's channel; the verifier accepts the
                // deploy whether or not this channel actually carries fuel.
                Self::from_sig(inner)
            }
            Sig::Lolly(from, to) => {
                // Linear implication `σ_from ⊸ σ_to`: capability. The
                // reflected channel is the union of `from` and `to`
                // channels (substrate composition); the capability-store
                // transformer (rho:system:capabilities) operationally
                // consumes σ_from to produce σ_to at invocation time.
                let from_channel = Self::from_sig(from).par;
                let to_channel = Self::from_sig(to).par;
                let combined = concatenate_pars(from_channel, to_channel);
                SignatureChannel {
                    par: ParSortMatcher::sort_match(&combined).term,
                }
            }
        }
    }
}

fn cost_value_to_token_count(value: i64) -> u64 {
    if value < 0 {
        0
    } else {
        value as u64
    }
}

fn token_units_to_i64(value: u64) -> i64 {
    if value > i64::MAX as u64 {
        i64::MAX
    } else {
        value as i64
    }
}

#[cfg(test)]
mod d0_lane_pool_tests {
    use super::*;

    // Build a deterministic COMM attempt record with a fixed deploy/sig
    // context; `local_index` drives the canonical `Ord` rank within a lane.
    // D3 (DR-9, OD-3): the consensus cost of a COMM is ONE, regardless of its
    // diagnostic `weight`, so `consumed_units` tallies the COMM COUNT.
    fn attempt(local_index: u64, weight: u64) -> AttemptRecord {
        attempt_kind(local_index, weight, BillableKind::Comm)
    }

    // Like [`attempt`] but with an explicit kind, so tests can exercise the
    // per-COMM vs. diagnostic (`Reduction`/`Primitive`/`Substitution`) split.
    fn attempt_kind(local_index: u64, weight: u64, kind: BillableKind) -> AttemptRecord {
        AttemptRecord {
            event: BillableTokenEvent {
                deploy_id: [9; 32],
                sig_hash: [0; 32],
                source_path: SourcePath(vec![local_index as u32]),
                redex_id: RedexId(local_index),
                local_index,
                kind,
                weight,
            },
            amount: Some(Cost::create(weight as i64, "test")),
        }
    }

    // A distinct ground signature per lane index. `from_sig` content-hashes
    // the atom bytes, so distinct bytes ⇒ distinct supply channels ⇒ distinct
    // `lane_hash` ⇒ disjoint lanes (the `lane_pool_disjoint` corollary).
    fn lane_sig(tag: u8) -> Sig { Sig::Ground(vec![tag, tag, tag, tag]) }

    /// The N=1 scalar fast path is BYTE-IDENTICAL to the pre-D0
    /// implementation: a single-signature deploy never touches the lane pool,
    /// and its `reconcile()` output equals the extracted `reconcile_lane`
    /// walk over the same attempt multiset, field-for-field
    /// (`committed`, `oop`, `consumed_units`, `cost_amounts`). This pins the
    /// fast-path invariant — `total_cost()` provably takes the scalar branch
    /// because `lanes` is empty and `lane_pool_total_cost()` is `None`.
    #[test]
    fn legacy_single_sig_scalar_equals_lane_per_comm() {
        // D3 (DR-9, OD-3): the consensus cost is the COMM COUNT. With an
        // `initial` budget of 2 COMM tokens and three COMM attempts, the
        // canonical walk commits the first two COMMs (running count 2) and OOPs
        // on the third; on OOP `consumed_units` clamps UP to `initial` (= 2).
        // The per-op `weight` is now DIAGNOSTIC and does NOT affect the count.
        let initial = 2_i64;
        let budget = RuntimeBudget::new(Cost::create(initial, "scalar fast path"));

        // Drive attempts through the PUBLIC scalar entry point — exactly what
        // every legacy single-signature deploy does. Weights are arbitrary
        // (diagnostic); each event is a COMM, so each costs ONE token.
        let attempts = vec![attempt(0, 11), attempt(1, 11), attempt(2, 11)];
        for record in &attempts {
            let _ = budget.reserve_canonical_with_cost(
                record.event.clone(),
                record.amount.clone().expect("test amount"),
            );
        }

        // The lane pool MUST stay empty on the scalar path.
        assert!(
            budget.lanes.is_empty(),
            "scalar fast path must not populate the lane pool"
        );
        assert_eq!(
            budget.lane_pool_total_cost(),
            None,
            "empty lane pool signals the N=1 scalar fast path"
        );

        // The budget's scalar reconciliation must equal the extracted canonical
        // walk over the same attempt multiset, field-for-field.
        let scalar = budget.reconcile();
        let reference = RuntimeBudget::reconcile_lane(initial, &attempts);
        assert_eq!(
            scalar, reference,
            "scalar reconcile() must equal reconcile_lane() field-for-field"
        );

        // Per-COMM canonical answer: two COMMs commit, the third OOPs, and
        // consumed clamps to `initial` (= the COMM budget).
        assert_eq!(scalar.consumed_units, initial);
        assert_eq!(scalar.committed.len(), 2);
        assert!(scalar.oop.is_some(), "the third COMM is the OOP boundary");

        // `total_cost()` takes the scalar branch and reports the COMM count.
        assert_eq!(budget.total_cost().value, initial);
    }

    /// D3 (DR-9, OD-3): a diagnostic `Reduction` event COMMITS (it appears in
    /// the committed set / event log) but contributes ZERO to the per-COMM
    /// consensus consumed cost, and never triggers an OOP boundary. A budget of
    /// 1 COMM token admits ONE COMM plus arbitrarily many Reductions.
    #[test]
    fn reduction_events_are_diagnostic_and_cost_zero() {
        let budget = RuntimeBudget::new(Cost::create(1, "one-comm budget"));
        // One COMM (cost 1) interleaved with three Reductions (cost 0 each).
        let attempts = vec![
            attempt_kind(0, 11, BillableKind::Comm),
            attempt_kind(1, 128, BillableKind::Reduction),
            attempt_kind(2, 256, BillableKind::Reduction),
            attempt_kind(3, 64, BillableKind::Reduction),
        ];
        for record in &attempts {
            let _ = budget.reserve_canonical_with_cost(
                record.event.clone(),
                record.amount.clone().expect("test amount"),
            );
        }
        let rec = budget.reconcile();
        // All four events commit (the single COMM fits the 1-token budget; the
        // Reductions cost 0), and none is an OOP boundary.
        assert_eq!(rec.committed.len(), 4, "COMM + 3 Reductions all commit");
        assert!(
            rec.oop.is_none(),
            "no OOP — only the COMM costs, and it fits"
        );
        // Consensus consumed cost is the COMM count = 1, NOT the weight sum.
        assert_eq!(rec.consumed_units, 1);
        assert_eq!(budget.total_cost().value, 1);
    }

    /// A second COMM on a 1-token budget IS the OOP boundary even when
    /// diagnostic Reductions precede it (the Reductions commit for free; the
    /// over-budget COMM clamps consumed to the COMM budget).
    #[test]
    fn second_comm_over_budget_is_oop_despite_reductions() {
        let budget = RuntimeBudget::new(Cost::create(1, "one-comm budget"));
        let attempts = vec![
            attempt_kind(0, 11, BillableKind::Comm),
            attempt_kind(1, 100, BillableKind::Reduction),
            attempt_kind(2, 11, BillableKind::Comm),
        ];
        for record in &attempts {
            let _ = budget.reserve_canonical_with_cost(
                record.event.clone(),
                record.amount.clone().expect("test amount"),
            );
        }
        let rec = budget.reconcile();
        // The first COMM and the Reduction commit; the second COMM OOPs.
        assert_eq!(rec.committed.len(), 2);
        assert!(
            rec.oop.is_some(),
            "the second COMM exceeds the 1-token budget"
        );
        assert_eq!(rec.consumed_units, 1, "consumed clamps to the COMM budget");
    }

    /// The per-signature pool's `total_cost` is the order-independent SUM of
    /// the per-lane canonical reconciliations, and each lane's reconciliation
    /// equals the scalar reconciliation a standalone single-signature budget
    /// would produce for that signature's events (spec §4.6 spectral
    /// decomposition; `rb_pool_total_cost = Σ rb_total_cost`).
    #[test]
    fn per_lane_reconcile_is_sum_of_scalar() {
        // Three disjoint signatures, each with its own budget and COMM events.
        // D3 (DR-9, OD-3): each COMM costs ONE token (weights are diagnostic).
        // Lane A (initial 10): 2 COMMs → consumed 2, no OOP.
        // Lane B (initial 1):  2 COMMs → 1 commits, OOPs on the 2nd → clamps to 1.
        // Lane C (initial 6):  2 COMMs → consumed 2, no OOP.
        let lanes = [
            (lane_sig(1), 10_i64, vec![attempt(0, 11), attempt(1, 11)]),
            (lane_sig(2), 1_i64, vec![attempt(0, 11), attempt(1, 11)]),
            (lane_sig(3), 6_i64, vec![attempt(0, 11), attempt(1, 11)]),
        ];

        let budget = RuntimeBudget::new(Cost::unsafe_max());

        // Route every attempt into its signature's lane via the lock-free
        // per-lane entry point. (Intentionally interleave lanes to exercise
        // order-independence of the eventual sum.)
        let max_len = lanes.iter().map(|(_, _, a)| a.len()).max().unwrap_or(0);
        for i in 0..max_len {
            for (sig, initial, attempts) in &lanes {
                if let Some(record) = attempts.get(i) {
                    let _ = budget.attempt_in_lane(
                        sig,
                        *initial,
                        record.event.clone(),
                        record.amount.clone(),
                    );
                }
            }
        }

        // Expected per-lane consumed via the pure scalar walk, summed.
        let expected_sum: i64 = lanes
            .iter()
            .map(|(_, initial, attempts)| {
                RuntimeBudget::reconcile_lane(*initial, attempts).consumed_units
            })
            .sum();
        // 2 (A) + 1 (B clamped) + 2 (C) = 5 COMMs.
        assert_eq!(expected_sum, 5);

        // The pool total must equal that order-independent sum.
        assert_eq!(
            budget.lane_pool_total_cost(),
            Some(expected_sum),
            "lane_pool_total_cost must be the sum over per-signature lanes"
        );

        // And each lane must match a standalone scalar budget for the SAME
        // signature's events (a lane is an independent instance of the scalar
        // budget — `rb_pool`).
        for (sig, initial, attempts) in &lanes {
            let standalone = RuntimeBudget::new(Cost::create(*initial, "standalone"));
            for record in attempts {
                let _ = standalone.reserve_canonical_with_cost(
                    record.event.clone(),
                    record.amount.clone().expect("test amount"),
                );
            }
            let scalar_consumed = standalone.reconcile().consumed_units;

            let lane_consumed = {
                let key = sig.lane_hash();
                let lane_ref = budget.lanes.get(&key).expect("lane present after routing");
                RuntimeBudget::reconcile_one_lane(lane_ref.value()).consumed_units
            };
            assert_eq!(
                lane_consumed, scalar_consumed,
                "each lane reconciliation must equal the scalar budget for that signature"
            );
        }

        // `total_cost()` takes the pool branch (lanes non-empty).
        assert_eq!(budget.total_cost().value, expected_sum);
    }

    /// The integration invariant: `Sig::lane_hash` shares ONE canonical basis
    /// with `SignatureChannel::from_sig`. Signatures that reflect to the same
    /// supply channel MUST share a lane key, and signatures with distinct
    /// channels MUST get distinct lane keys.
    #[test]
    fn lane_hash_shares_from_sig_canonical_basis() {
        // DR-1: the ground/quote axis collapses at the channel (equal bytes ⇒
        // equal channel), so a Ground and a Quote atom over the SAME bytes
        // share both the supply channel AND the lane key.
        let g = Sig::Ground(vec![1, 2, 3, 4]);
        let q = Sig::Quote(vec![1, 2, 3, 4]);
        assert_eq!(
            SignatureChannel::from_sig(&g).par,
            SignatureChannel::from_sig(&q).par,
            "DR-1: equal atom bytes ⇒ equal supply channel"
        );
        assert_eq!(
            g.lane_hash(),
            q.lane_hash(),
            "lane_hash must agree wherever from_sig agrees (shared basis)"
        );

        // Distinct atom bytes ⇒ distinct channels ⇒ distinct lane keys.
        let other = Sig::Ground(vec![9, 9, 9, 9]);
        assert_ne!(
            SignatureChannel::from_sig(&g).par,
            SignatureChannel::from_sig(&other).par
        );
        assert_ne!(
            g.lane_hash(),
            other.lane_hash(),
            "distinct supply channels ⇒ distinct lane keys"
        );

        // Permutation-invariance is inherited from `from_sig`
        // (`ParSortMatcher::sort_match`): `And(a, b)` and `And(b, a)` reflect
        // to the same channel, so they share a lane key.
        let a = Sig::Ground(vec![1]);
        let b = Sig::Ground(vec![2]);
        let ab = Sig::And(Box::new(a.clone()), Box::new(b.clone()));
        let ba = Sig::And(Box::new(b), Box::new(a));
        assert_eq!(
            SignatureChannel::from_sig(&ab).par,
            SignatureChannel::from_sig(&ba).par,
            "compound channel is permutation-invariant"
        );
        assert_eq!(
            ab.lane_hash(),
            ba.lane_hash(),
            "lane_hash inherits compound permutation-invariance from from_sig"
        );
    }

    /// Resetting the budget clears the lane pool, returning the reused budget
    /// to the N=1 scalar fast path.
    #[test]
    fn reset_clears_lane_pool() {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let sig = lane_sig(7);
        let _ = budget.attempt_in_lane(&sig, 10, attempt(0, 3).event, Some(Cost::create(3, "t")));
        assert!(!budget.lanes.is_empty());

        budget.reset_from_token(&Token::coalesced(Sig::Unit, 4));
        assert!(
            budget.lanes.is_empty(),
            "reset must clear the lane pool back to the scalar fast path"
        );
        assert_eq!(budget.lane_pool_total_cost(), None);
    }
}

#[cfg(kani)]
mod kani_cost_accounting {
    use super::*;

    #[kani::proof]
    fn cost_value_to_token_count_rejects_negative_values() {
        let value: i64 = kani::any();
        let tokens = cost_value_to_token_count(value);

        if value < 0 {
            assert_eq!(tokens, 0);
        } else {
            assert_eq!(tokens, value as u64);
        }
    }

    #[kani::proof]
    fn token_remaining_units_i64_saturates_to_i64_max() {
        let remaining: u64 = kani::any();
        let as_i64 = token_units_to_i64(remaining);

        if remaining > i64::MAX as u64 {
            assert_eq!(as_i64, i64::MAX);
        } else {
            assert_eq!(as_i64, remaining as i64);
        }
    }
}

#[cfg(test)]
mod envelope_sig_extraction_tests {
    //! The ONE extracted envelope-`Sig` derivation (WD-D2 §D2.2) shared by the
    //! runtime install, the D2 acceptance gate, and replay. These tests pin
    //! both the SHAPE (single ⇒ `Sig::Quote`; n-signer ⇒ left-associated
    //! `Sig::And`) and — the no-drift guarantee — that the extracted free
    //! functions yield EXACTLY the `Sig` the `RuntimeBudget` install path
    //! (`set_deploy_signature` / `set_deploy_signatures`) produces.
    use super::*;

    fn quote_of(domain: &[u8], sig: &[u8]) -> Sig {
        let mut buf = Vec::with_capacity(domain.len() + sig.len());
        buf.extend_from_slice(domain);
        buf.extend_from_slice(sig);
        Sig::Quote(Blake2b256::hash(buf))
    }

    /// Single signer ⇒ `Sig::Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ sig))`.
    #[test]
    fn envelope_sig_single_is_quote() {
        let sig = b"deploy-signature-bytes";
        let expected = quote_of(DEPLOY_SIGNATURE_DOMAIN, sig);
        assert_eq!(envelope_sig_single(sig), expected);
        assert!(matches!(envelope_sig_single(sig), Sig::Quote(_)));
    }

    /// Two signers ⇒ left-associated `Sig::And(Quote(h0), Quote(h1))` over the
    /// COMPOUND domain.
    #[test]
    fn envelope_sig_two_signers_is_left_assoc_and() {
        let s0: &[u8] = b"signer-zero";
        let s1: &[u8] = b"signer-one";
        let h0 = quote_of(COMPOUND_DEPLOY_SIGNATURE_DOMAIN, s0);
        let h1 = quote_of(COMPOUND_DEPLOY_SIGNATURE_DOMAIN, s1);
        let expected = Sig::And(Box::new(h0), Box::new(h1));
        assert_eq!(envelope_sig_compound(&[s0, s1]), expected);
    }

    /// Three signers ⇒ left-associated nesting
    /// `And(And(Quote(h0), Quote(h1)), Quote(h2))`.
    #[test]
    fn envelope_sig_three_signers_is_left_assoc_nested() {
        let s0: &[u8] = b"a";
        let s1: &[u8] = b"b";
        let s2: &[u8] = b"c";
        let h0 = quote_of(COMPOUND_DEPLOY_SIGNATURE_DOMAIN, s0);
        let h1 = quote_of(COMPOUND_DEPLOY_SIGNATURE_DOMAIN, s1);
        let h2 = quote_of(COMPOUND_DEPLOY_SIGNATURE_DOMAIN, s2);
        let expected = Sig::And(Box::new(Sig::And(Box::new(h0), Box::new(h1))), Box::new(h2));
        assert_eq!(envelope_sig_compound(&[s0, s1, s2]), expected);
    }

    /// A single-element COMPOUND call collapses to a bare `Sig::Quote` (the
    /// fold seed with no `And` applied) — distinct from the legacy single-sig
    /// path only in the domain separator.
    #[test]
    fn envelope_sig_compound_singleton_is_bare_quote() {
        let s0: &[u8] = b"only-signer";
        let expected = quote_of(COMPOUND_DEPLOY_SIGNATURE_DOMAIN, s0);
        assert_eq!(envelope_sig_compound(&[s0]), expected);
    }

    /// No-drift: the extracted single-sig derivation equals the `Sig` the
    /// runtime install (`set_deploy_signature`) actually stores. If this fires,
    /// the gate/replay would key the supply pool differently from the install.
    #[test]
    fn envelope_sig_single_matches_install_path() {
        let sig = b"on-chain-deploy-signature";
        let budget = RuntimeBudget::new(Cost::create(100, "install-equivalence"));
        budget.set_deploy_signature(sig);
        assert_eq!(envelope_sig_single(sig), budget.signature());
    }

    /// No-drift: the extracted compound derivation equals the `Sig` the runtime
    /// install (`set_deploy_signatures`) stores for a multi-signer deploy.
    #[test]
    fn envelope_sig_compound_matches_install_path() {
        let s0: &[u8] = b"cosigner-aaaa";
        let s1: &[u8] = b"cosigner-bbbb";
        let budget = RuntimeBudget::new(Cost::create(100, "install-equivalence"));
        budget.set_deploy_signatures(&[s0, s1]);
        assert_eq!(envelope_sig_compound(&[s0, s1]), budget.signature());
    }
}
