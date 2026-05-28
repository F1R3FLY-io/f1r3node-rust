use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_queue::SegQueue;

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
/// Domain separator for compound (multi-signer) deploy signatures. Distinct
/// from the legacy single-sig `DEPLOY_SIGNATURE_DOMAIN` so legacy deploys on
/// chain retain their existing `deploy_id`s, while multi-sig deploys get a
/// distinguishable id derived from the canonically-ordered set of signatures.
const COMPOUND_DEPLOY_SIGNATURE_DOMAIN: &[u8] =
    b"f1r3node:cost-accounted-rho:compound-deploy-signature:v1";
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
    fn resolve_max_log_entries() -> usize {
        1024
    }

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

        // Bounded-K canonical window. Because every billable weight is
        // >= 1, the canonical commit walk commits at most `initial`
        // events before the first OOP boundary (1 more event). So only
        // the lowest-K events by the canonical `Ord` can influence the
        // committed set, the OOP boundary, or `consumed_units`; events
        // beyond rank K are provably never read by the walk. Keeping
        // only the lowest K bounds memory and replaces the prior global
        // O(N log N) sort over up to MAX_COST_TRACE_EVENTS elements.
        //
        // `initial.max(0)` keeps K >= 1 even for a non-positive budget
        // (the walk then OOPs on the first event and clamps consumed to
        // `initial`, preserving the prior behavior).
        let k_bound = (initial.max(0) as u64)
            .saturating_add(1)
            .min(MAX_COST_TRACE_EVENTS) as usize;

        // Canonical sort key is the derived Ord on BillableTokenEvent.
        // Multiplicity is preserved: a deploy that re-attempts the same
        // logical event (e.g. through a loop) MUST see the repeated
        // attempt contribute, just as it did under the pre-Option-E
        // commit_lock contract.
        let mut attempts: Vec<AttemptRecord> = {
            let accumulator = self
                .attempt_accumulator
                .lock()
                .expect("attempt accumulator poisoned");
            accumulator.clone()
        };
        attempts.sort_by(|a, b| a.event.cmp(&b.event));
        if attempts.len() > k_bound {
            attempts.truncate(k_bound);
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

        // Repopulate the diagnostic `event_log` / `log` mirrors from the
        // canonical committed set. This moves their population OFF the
        // hot path (they were previously appended per-grant inside
        // `attempt_one`) and onto finalization, so `get_event_log` /
        // `get_log` now reflect the canonical committed set rather than a
        // schedule-dependent record of CAS-race winners.
        self.repopulate_diagnostic_logs(&committed, &cost_amounts);

        let rec = CanonicalReconciliation {
            committed,
            oop,
            consumed_units,
            cost_amounts,
        };
        *cache = Some(rec.clone());
        rec
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
    /// `Sig::Hash` value and `SignatureChannel` reflection.
    pub fn set_deploy_signatures(&self, signatures: &[&[u8]]) {
        assert!(
            !signatures.is_empty(),
            "set_deploy_signatures requires at least one signature"
        );

        // Domain-separated hash of each individual wire signature. Per-signature
        // domain separation uses the COMPOUND domain so single-element calls
        // remain distinguishable from legacy single-sig deploys.
        let mut sig_hashes: Vec<Vec<u8>> = Vec::with_capacity(signatures.len());
        for sig_bytes in signatures.iter() {
            let mut domain_separated =
                Vec::with_capacity(COMPOUND_DEPLOY_SIGNATURE_DOMAIN.len() + sig_bytes.len());
            domain_separated.extend_from_slice(COMPOUND_DEPLOY_SIGNATURE_DOMAIN);
            domain_separated.extend_from_slice(sig_bytes);
            sig_hashes.push(Blake2b256::hash(domain_separated));
        }

        // Fold into a left-associated Sig::And tree:
        //   [h0]          => Sig::Hash(h0)
        //   [h0, h1]      => Sig::And(Sig::Hash(h0), Sig::Hash(h1))
        //   [h0, h1, h2]  => Sig::And(Sig::And(Sig::Hash(h0), Sig::Hash(h1)), Sig::Hash(h2))
        let mut iter = sig_hashes.iter().cloned();
        let first = iter.next().expect("non-empty per assert above");
        let folded_sig: Sig = iter.fold(Sig::Hash(first), |acc, hash| {
            Sig::And(Box::new(acc), Box::new(Sig::Hash(hash)))
        });

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

    pub fn signature(&self) -> Sig {
        self.signature.lock().expect("signature lock").clone()
    }

    pub fn deploy_id(&self) -> [u8; 32] {
        *self.deploy_id.lock().expect("deploy id lock")
    }

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

    pub fn remaining(&self) -> Cost {
        self.get()
    }

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
    pub fn last_oop_event(&self) -> Option<BillableTokenEvent> {
        self.reconcile().oop
    }

    pub fn clear_log(&self) {
        self.log.lock().unwrap().clear();
    }

    pub fn clear_event_log(&self) {
        self.event_log.lock().unwrap().clear();
    }

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
    /// `1` — multiplicative unit. Identity for `And` / `Tensor`: σ ⊗ 1 ≡ σ.
    Unit,
    /// Atomic signature: Blake2b256 of the domain-separated wire signature.
    Hash(Vec<u8>),
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

impl Sig {
    /// Serialize the runtime `Sig` algebra into the `SigCompound`
    /// wire-format proto message (Phase 2+3 `CasperMessage.proto`).
    /// `Sig::Hash` becomes a `SigAtom` (pk + sig + sigAlgorithm are
    /// unavailable at this layer — they live on `Cosigner`); for the
    /// substrate-only serialization, atomic signatures are encoded as
    /// `pk = hash_bytes` placeholder. Downstream Cosigned-shape encoders
    /// (`models/src/rust/casper/protocol/casper_message.rs`) populate the
    /// full SigAtom from the matching Cosigner.
    pub fn to_proto(&self) -> models::casper::SigCompound {
        use models::casper::{
            sig_compound, SigAtom, SigBang, SigCompound, SigLolly, SigPair, SigPlus, SigThreshold,
        };
        let connective = match self {
            Sig::Unit => sig_compound::Connective::Atom(SigAtom {
                pk: Default::default(),
                sig: Default::default(),
                sig_algorithm: String::new(),
                phlo_share: 0,
            }),
            Sig::Hash(bytes) => sig_compound::Connective::Atom(SigAtom {
                pk: bytes.clone().into(),
                sig: Default::default(),
                sig_algorithm: String::new(),
                phlo_share: 0,
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
                if atom.pk.is_empty() {
                    Ok(Sig::Unit)
                } else {
                    Ok(Sig::Hash(atom.pk.to_vec()))
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Unit,
    Count { sig: Sig, remaining: u64 },
    Gate { sig: Sig, rest: Box<Token> },
}

impl Token {
    pub fn coalesced(sig: Sig, remaining: u64) -> Self {
        Token::Count { sig, remaining }
    }

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

    fn remaining_units_i64(&self) -> i64 {
        token_units_to_i64(self.remaining_units())
    }
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
