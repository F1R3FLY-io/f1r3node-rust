use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use costs::Cost;
use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

use super::errors::InterpreterError;

pub mod cost_accounting;
pub mod costs;
pub mod has_cost;

const DEPLOY_SIGNATURE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:deploy-signature:v1";
const COST_TRACE_DIGEST_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:cost-trace:v1";
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
    // Append log of every reservation ATTEMPT (whether or not the
    // runtime CAS race granted it). Snapshot-cloned (not drained) by
    // `reconcile()` so mid-deploy reads do not lose later attempts.
    // The Mutex is held only briefly per push (acquire, push, release —
    // bounded by O(1) work); the per-event CAS on `consumed_tokens` is
    // outside the lock. Cost-accounted-rho paper §3 Rule 1 (single
    // shared signature/token within a deploy) — the canonical reduction
    // order is structurally determined by the program, not by Tokio
    // scheduling. See `formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla`
    // and `formal/rocq/cost_accounted_rho/theories/RuntimeBudgetRefinement.v`.
    attempt_log: Arc<Mutex<Vec<AttemptRecord>>>,
    // Cached canonical reconciliation. Populated by the first call to
    // `reconcile()` at deploy finalization; reset by `reset_from_token`
    // when the budget is reused for a new deploy. Read by `total_cost`,
    // `cost_trace_digest`, `cost_trace_event_count`, `last_oop_event`.
    canonical_reconciliation: Arc<Mutex<Option<CanonicalReconciliation>>>,
    // Serializes `reset_from_token` against in-flight reservation
    // attempts. Writer: reset (waits for all in-flight attempts to
    // complete). Readers: per-event `attempt_one` calls (uncontested
    // when no reset is pending — effectively atomic). Ensures the
    // RuntimeBudget never observes a mid-reset state from an attempt.
    reset_serializer: Arc<RwLock<()>>,
    max_log_entries: usize,
    unmetered: Arc<AtomicU64>,
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BillableKind {
    SourceStep,
    Primitive(String),
    Substitution,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BillableTokenEvent {
    pub deploy_id: [u8; 32],
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
            attempt_log: Arc::new(Mutex::new(Vec::new())),
            canonical_reconciliation: Arc::new(Mutex::new(None)),
            reset_serializer: Arc::new(RwLock::new(())),
            max_log_entries,
            unmetered: Arc::new(AtomicU64::new(0)),
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
        let _reset_guard = self
            .reset_serializer
            .read()
            .expect("reset serializer poisoned");
        let outcome = self.attempt_one(event, Some(amount));
        match outcome {
            AttemptOutcome::Granted => Ok(()),
            AttemptOutcome::Oop => Err(InterpreterError::OutOfPhlogistonsError),
        }
    }

    pub(crate) fn append_cost_log(&self, amount: Cost) {
        if self.max_log_entries > 0 {
            let mut log = self.log.lock().unwrap();
            if log.len() >= self.max_log_entries {
                let _ = log.pop_front();
            }
            log.push_back(amount);
        }
    }

    fn append_event_log(&self, event: BillableTokenEvent) {
        if self.max_log_entries > 0 {
            let mut log = self.event_log.lock().unwrap();
            if log.len() >= self.max_log_entries {
                let _ = log.pop_front();
            }
            log.push_back(event);
        }
    }

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
        let _reset_guard = self
            .reset_serializer
            .read()
            .expect("reset serializer poisoned");
        let outcome = self.attempt_one(event, None);
        match outcome {
            AttemptOutcome::Granted => Ok(()),
            AttemptOutcome::Oop => Err(InterpreterError::OutOfPhlogistonsError),
        }
    }

    /// Record one reservation attempt and try to claim its weight from
    /// `consumed_tokens` via lock-free CAS. The attempt is ALWAYS pushed
    /// to `attempt_log` (so the canonical reconciliation sees it even if
    /// the CAS race grants nothing). Returns whether the runtime should
    /// let the caller's branch proceed (`Granted`) or abort it (`Oop`).
    ///
    /// The runtime's grant/oop decision is for liveness only; the
    /// consensus-relevant commit set is computed post-hoc by `reconcile()`.
    ///
    /// API contract: callers must NOT call `reconcile()` (or any reader
    /// that triggers it — `total_cost`, `cost_trace_digest`, etc.) before
    /// all `attempt_one` calls for a given deploy have completed. Doing
    /// so caches a partial reconciliation, and later attempts will be
    /// silently absent from the final consensus output. Per-deploy
    /// finalization is single-threaded by contract at the call sites in
    /// `runtime.rs::process_deploy` and `replay_runtime.rs::replay`.
    /// Inner per-event reservation. Caller MUST hold a read-lock on
    /// `reset_serializer` for the entire batch so reset cannot interleave
    /// with attempts mid-batch.
    fn attempt_one(&self, event: BillableTokenEvent, amount: Option<Cost>) -> AttemptOutcome {
        // Unmetered fast path: system deploys + scoped unmetered scopes
        // bypass billing entirely. Don't touch the attempt log so the
        // reconciliation stays empty (unmetered budgets are not subject
        // to consensus authentication of cost trace).
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return AttemptOutcome::Granted;
        }

        // Record the attempt for the canonical reconciliation. Pushed
        // before the CAS so the reconciliation sees every attempt even
        // if the CAS race grants nothing. Invalidate the reconciliation
        // cache so the next read recomputes including this attempt
        // (otherwise mid-deploy readers would see a stale snapshot).
        {
            let mut log = self
                .attempt_log
                .lock()
                .expect("attempt log poisoned");
            log.push(AttemptRecord {
                event: event.clone(),
                amount: amount.clone(),
            });
        }
        {
            let mut cache = self
                .canonical_reconciliation
                .lock()
                .expect("reconciliation cache lock");
            *cache = None;
        }

        let initial = self.initial_tokens.load(Ordering::Acquire);
        let weight = event.weight as i64;

        // Lock-free CAS loop. On overflow, return Oop without writing the
        // clamp — the canonical reconciliation establishes the consensus
        // consumed/OOP values; this counter is just a liveness gate.
        let mut current = self.consumed_tokens.load(Ordering::Acquire);
        loop {
            if current < 0 || initial < 0 {
                return AttemptOutcome::Oop;
            }
            if current >= initial {
                return AttemptOutcome::Oop;
            }
            let next = current.saturating_add(weight);
            if next > initial {
                return AttemptOutcome::Oop;
            }
            match self.consumed_tokens.compare_exchange(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Runtime grant: mirror the event into the bounded
                    // diagnostic event log (only successful CASes, matching
                    // the prior contract that `get_event_log` reflects
                    // runtime-granted events). And mirror the amount into
                    // the diagnostic cost log. Both are diagnostic; the
                    // consensus-relevant values come from `reconcile()`.
                    self.append_event_log(event);
                    if let Some(amount) = amount {
                        self.append_cost_log(amount);
                    }
                    return AttemptOutcome::Granted;
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Batch entry point retained for backward compatibility with callers
    /// that issue multi-event reservations (notably
    /// `MeteredMachine::drain_frames`). Each event is processed via the
    /// same lock-free `attempt_one` path; permits/oop are aggregated.
    /// The entire batch holds one read-lock on `reset_serializer` so
    /// `reset_from_token` cannot interleave with mid-batch events.
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

        let _reset_guard = self
            .reset_serializer
            .read()
            .expect("reset serializer poisoned");

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
                    permits.push(ExecutionPermit {
                        weight,
                        event,
                    });
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

    /// Drain the attempt log and compute the canonical reconciliation.
    /// Pure function of (attempts ∪ cached, initial_tokens). Idempotent:
    /// subsequent calls return the cached value without re-walking.
    ///
    /// Called at deploy finalization (single-threaded by contract — the
    /// caller is `runtime.rs::process_deploy` / `replay_runtime.rs::replay`
    /// after `evaluate()` joins the parallel reducer). Calls before
    /// finalization snapshot the in-flight attempts and cache them; if
    /// further attempts arrive later, `attempt_one` invalidates the cache.
    fn reconcile(&self) -> CanonicalReconciliation {
        let mut cache = self
            .canonical_reconciliation
            .lock()
            .expect("reconciliation cache lock");
        if let Some(rec) = cache.as_ref() {
            return rec.clone();
        }

        let initial = self.initial_tokens.load(Ordering::Acquire);

        // Snapshot the attempt log (clone — don't drain). Mid-deploy
        // callers see a partial reconciliation; later attempts invalidate
        // the cache so the next call sees the augmented snapshot.
        let mut attempts: Vec<AttemptRecord> = {
            let log = self.attempt_log.lock().expect("attempt log poisoned");
            log.clone()
        };

        // Canonical sort key is the derived Ord on BillableTokenEvent.
        // Multiplicity is preserved: a deploy that re-attempts the same
        // logical event (e.g. through a loop) MUST see the repeated
        // attempt contribute to the digest, just as it did under the
        // pre-Option-E commit_lock contract.
        attempts.sort_by(|a, b| a.event.cmp(&b.event));

        // Cap the canonical event window to prevent unbounded memory
        // attribution per deploy. Excess attempts are treated as OOP
        // candidates beyond the cap.
        if attempts.len() as u64 > MAX_COST_TRACE_EVENTS {
            attempts.truncate(MAX_COST_TRACE_EVENTS as usize);
        }

        // Simulate the canonical commit walk.
        let mut committed = Vec::with_capacity(attempts.len());
        let mut cost_amounts: Vec<Cost> = Vec::new();
        let mut consumed_units: i64 = 0;
        let mut oop: Option<BillableTokenEvent> = None;

        for rec in attempts.into_iter() {
            let weight = rec.event.weight as i64;
            let next = consumed_units.saturating_add(weight);
            if next > initial {
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

        let rec = CanonicalReconciliation {
            committed,
            oop,
            consumed_units,
            cost_amounts,
        };
        *cache = Some(rec.clone());
        rec
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
        // Block in-flight `attempt_one` calls for the duration of the
        // reset. The read-lock acquisition in `attempt_one` is uncontested
        // when no reset is pending, so this writer-lock only matters on
        // the rare reset-vs-commit race (test
        // `runtime_budget_reset_from_token_serializes_with_batch_commit`
        // documents this invariant).
        let _reset_guard = self
            .reset_serializer
            .write()
            .expect("reset serializer poisoned");
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
        self.attempt_log.lock().expect("attempt log poisoned").clear();
        *cache = None;
    }

    pub fn set_deploy_signature(&self, signature: &[u8]) {
        let mut domain_separated_signature =
            Vec::with_capacity(DEPLOY_SIGNATURE_DOMAIN.len() + signature.len());
        domain_separated_signature.extend_from_slice(DEPLOY_SIGNATURE_DOMAIN);
        domain_separated_signature.extend_from_slice(signature);
        let hash = Blake2b256::hash(domain_separated_signature);
        let mut deploy_id = [0; 32];
        deploy_id.copy_from_slice(&hash[..32]);
        *self.deploy_id.lock().expect("deploy id lock") = deploy_id;
        // Cost-accounting channels are internal capabilities derived from,
        // but not equal to, the wire signature. Domain separation prevents
        // accidental reuse of raw signature bytes as another protocol hash.
        *self.signature.lock().expect("signature lock") = Sig::Hash(hash);
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
    pub fn total_cost(&self) -> Cost {
        if self.unmetered.load(Ordering::Acquire) != 0 {
            return Cost::create(0, "unmetered token budget");
        }
        Cost::create(
            self.reconcile().consumed_units,
            "consumed source-token units",
        )
    }

    pub fn remaining(&self) -> Cost { self.get() }

    /// Diagnostic running cost log of runtime-granted charges. Returns
    /// the bounded ring-buffer contents as appended during evaluation —
    /// schedule-dependent by design (it tracks what the CAS race
    /// actually granted, not the canonical reconciliation). For the
    /// consensus-relevant aggregate cost see `total_cost()`. `clear_log`
    /// empties it without affecting any consensus observable.
    pub fn get_log(&self) -> Vec<Cost> { self.log.lock().unwrap().iter().cloned().collect() }

    pub fn get_event_log(&self) -> Vec<BillableTokenEvent> {
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
    pub fn last_oop_event(&self) -> Option<BillableTokenEvent> {
        self.reconcile().oop
    }

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
            match &event.kind {
                BillableKind::SourceStep => update(&[0]),
                BillableKind::Primitive(name) => {
                    update(&[1]);
                    feed_len_prefixed(update, name.as_bytes());
                }
                BillableKind::Substitution => update(&[2]),
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
    Unit,
    Hash(Vec<u8>),
    And(Box<Sig>, Box<Sig>),
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
    pub fn from_sig(sig: &Sig) -> Self {
        match sig {
            Sig::Unit => SignatureChannel {
                par: Par::default(),
            },
            Sig::Hash(bytes) => SignatureChannel {
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
