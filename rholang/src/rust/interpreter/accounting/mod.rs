//! # Cost accounting — formal correspondence (continued-gslt-cost-v2)
//!
//! This runtime is the operational image of the Rocq Cost endofunctor/monad
//! (`formal/rocq/cost_accounted_rho/`). The mapping is additive witnessing only —
//! no behavioral change (see `docs/theory/cost-accounting-as-monad-correspondence.md`):
//!  - η (unmetered embedding)   ↔ the system/unmetered budget mode
//!    (`CostMonad.cost_eta`, `CAAdjunctions.cost_install`).
//!  - μ (grade accumulation)    ↔ canonical operation-charge accumulation;
//!    non-idempotent
//!    (`CostMonad.cost_mu` / `cost_mu_modulus_accumulates`).
//!  - located capabilities      ↔ native authority events and exact physical
//!    settlement (`CALocatedPurses.draw_disjoint` /
//!    `ChannelSeparation.lane_pool_disjoint`).
//!  - graded transition ⟨a⟩_s   ↔ the signature key on billable events
//!    (`CAGradedTransition.graded_step`).
//!  - linear no-double-spend    ↔ the resource-logic / Δσ discipline
//!    (`CATypeDiscipline.ca_linear_no_contraction`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use costs::Cost;
use crossbeam_queue::SegQueue;
use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{CostAuthority, CostSignature, GPrivate, GUnforgeable, Par};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

use super::errors::InterpreterError;

pub mod cost_accounting;
pub mod costs;
pub mod delta_sigma;
pub mod has_cost;
pub mod lexical;
pub mod oslf;
pub mod resource_logic;
pub mod authority;
pub mod byte_accounting;

const DEPLOY_SIGNATURE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:deploy-signature:v1";
/// Domain separator for compound (multi-signer) deploy signatures. Distinct
/// from the legacy single-sig `DEPLOY_SIGNATURE_DOMAIN` so legacy deploys on
/// chain retain their existing `deploy_id`s, while multi-sig deploys get a
/// distinguishable id derived from the canonically-ordered set of signatures.
const COMPOUND_DEPLOY_SIGNATURE_DOMAIN: &[u8] =
    b"f1r3node:cost-accounted-rho:compound-deploy-signature:v1";
const COST_TRACE_DIGEST_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:cost-trace:v1";
/// Domain separator for the per-signature identity key (`Sig::lane_hash`). The
/// identity key digests the SAME canonical signature serialization that
/// `SignatureChannel::from_sig` uses to derive the supply channel `Σ⟦s⟧`
/// (`sig_canonical_bytes`), so a deploy's identity key for signature `s` and its
/// supply channel are anchored to one canonical basis (no drift — see
/// `docs/theory/cost-accounting-impl/supply-realization-c-d-handoff.md`,
/// "Integration invariant"). Distinct from the channel domain only by this
/// separator: `lane_hash` is a fixed-width evidence and purse-lookup key, while
/// the channel is a `GPrivate`-keyed `Par`; both are pure functions of the same
/// canonical bytes.
const SIGNATURE_LANE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:signature-lane:v1";
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
    diagnostic_record_count: Arc<AtomicU64>,
    canonical_consensus_attempts: Arc<Mutex<CanonicalAttemptWindow>>,
    persistent_introductions: Arc<Mutex<BTreeSet<([u8; 32], BillableKind)>>>,
    introduction_authorities:
        Arc<Mutex<BTreeMap<([u8; 32], authority::AuthorityByteEventKind), CostAuthority>>>,
    attempt_generation: Arc<AtomicU64>,
    reconciled_generation: Arc<AtomicU64>,
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
    comm_accounting_scopes: Arc<AtomicUsize>,
    authority_state: Arc<Mutex<AuthorityRuntimeState>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuthorityRuntimeState {
    allocation: authority::ResourceMultiset<[u8; 32]>,
    enforce_allocation: bool,
    events: BTreeMap<[u8; 32], AuthorityRuntimeEvent>,
    byte_events: Vec<authority::AuthorityByteEvent>,
    realized: authority::ResourceMultiset<[u8; 32]>,
    reserved: authority::ResourceMultiset<[u8; 32]>,
    frontier: BTreeMap<[u8; 32], CostAuthority>,
    stack_births: BTreeMap<[u8; 32], authority::AuthorityStackBirth>,
    pending_stack_transfers: BTreeMap<[u8; 32], PendingAuthorityStackTransfer>,
    pending_stack_event_ids: BTreeSet<[u8; 32]>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuthorityRuntimeEvent {
    authority: CostAuthority,
    debit: authority::ResourceMultiset<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingAuthorityStackTransfer {
    events: BTreeMap<[u8; 32], AuthorityRuntimeEvent>,
    debit: authority::ResourceMultiset<[u8; 32]>,
    birth: authority::AuthorityStackBirth,
}

#[must_use]
pub struct AuthorityStackTransferReservation {
    budget: RuntimeBudget,
    produce_hash: Option<[u8; 32]>,
}

impl AuthorityStackTransferReservation {
    pub fn commit(mut self) {
        if let Some(produce_hash) = self.produce_hash.take() {
            self.budget.commit_authority_stack_transfer(produce_hash);
        }
    }
}

impl Drop for AuthorityStackTransferReservation {
    fn drop(&mut self) {
        if let Some(produce_hash) = self.produce_hash.take() {
            self.budget.abort_authority_stack_transfer(produce_hash);
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

#[derive(Default)]
struct CanonicalAttemptWindow {
    records: BTreeMap<BillableTokenEvent, Vec<Option<Cost>>>,
    len: u64,
}

impl CanonicalAttemptWindow {
    fn insert(&mut self, record: AttemptRecord, limit: u64) {
        if limit == 0 {
            return;
        }
        let event = record.event;
        if self.len < limit {
            self.records.entry(event).or_default().push(record.amount);
            self.len += 1;
            return;
        }
        let Some(largest) = self.records.keys().next_back().cloned() else {
            return;
        };
        if event > largest {
            return;
        }
        self.records.entry(event).or_default().push(record.amount);
        let remove_key = self
            .records
            .keys()
            .next_back()
            .cloned()
            .expect("non-empty canonical attempt window");
        let remove_entry = self
            .records
            .get_mut(&remove_key)
            .expect("canonical attempt key");
        remove_entry.pop();
        if remove_entry.is_empty() {
            self.records.remove(&remove_key);
        }
    }

    fn attempts(&self) -> Vec<AttemptRecord> {
        self.records
            .iter()
            .flat_map(|(event, amounts)| {
                amounts.iter().cloned().map(|amount| AttemptRecord {
                    event: event.clone(),
                    amount,
                })
            })
            .collect()
    }

    fn clear(&mut self) {
        self.records.clear();
        self.len = 0;
    }
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

/// The kind of a billable token event. The consensus surface contains the
/// canonical RSpace operation footprint and committed COMM cost:
///
///   * [`BillableKind::Comm`] — one successful atomic RSpace match, weighted by
///     the calculus' one execution unit plus its canonical payload and trace
///     bytes.
///   * [`BillableKind::RSpaceProduce`] and [`BillableKind::RSpaceConsume`] —
///     canonical introduction footprints, charged before any tuple-space
///     mutation.
///   * [`BillableKind::Reduction`] — a non-COMM structural reduction
///     (`new` / `match` / `if`). Metered for DIAGNOSTIC fidelity (it walks
///     into the event log + digest with its per-op weight) but contributes
///     ZERO to the consensus consumed cost.
///
/// `Primitive` / `Substitution` are likewise DIAGNOSTIC-only (per-op gas):
/// they appear in the event log/digest but never gate consensus. The split
/// is INTERNAL (never on the wire) — it only affects how `reconcile_lane`
/// tallies canonical consensus cost versus the diagnostic stream.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BillableKind {
    /// Successful atomic RSpace match. Consensus cost = 1.
    Comm,
    RSpaceProduce,
    RSpaceConsume,
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
            diagnostic_record_count: Arc::new(AtomicU64::new(0)),
            canonical_consensus_attempts: Arc::new(Mutex::new(CanonicalAttemptWindow::default())),
            persistent_introductions: Arc::new(Mutex::new(BTreeSet::new())),
            introduction_authorities: Arc::new(Mutex::new(BTreeMap::new())),
            attempt_generation: Arc::new(AtomicU64::new(0)),
            reconciled_generation: Arc::new(AtomicU64::new(0)),
            attempt_accumulator: Arc::new(Mutex::new(Vec::new())),
            canonical_reconciliation: Arc::new(Mutex::new(None)),
            max_log_entries,
            unmetered: Arc::new(AtomicU64::new(0)),
            comm_accounting_scopes: Arc::new(AtomicUsize::new(0)),
            authority_state: Arc::new(Mutex::new(AuthorityRuntimeState::default())),
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

    fn billable_event(
        &self,
        identity: [u8; 32],
        kind: BillableKind,
        weight: u64,
    ) -> BillableTokenEvent {
        let source_path = SourcePath(
            identity
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("event identity chunk")))
                .collect(),
        );
        BillableTokenEvent {
            deploy_id: self.deploy_id(),
            sig_hash: self.signature().lane_hash(),
            source_path,
            redex_id: RedexId(u64::from_le_bytes(
                identity[..8].try_into().expect("event redex identity"),
            )),
            local_index: u64::from_le_bytes(
                identity[24..].try_into().expect("event local identity"),
            ),
            kind,
            weight,
        }
    }

    fn reserve_consensus_identity(
        &self,
        identity: [u8; 32],
        kind: BillableKind,
        weight: u64,
        description: &'static str,
    ) -> Result<(), InterpreterError> {
        if !self.has_comm_accounting_scope() {
            return Ok(());
        }
        self.reserve_canonical_with_cost(
            self.billable_event(identity, kind, weight),
            Cost::create(
                i64::try_from(weight).map_err(|_| InterpreterError::OutOfPhlogistonsError)?,
                description,
            ),
        )
    }

    pub fn reserve_comm_identity(&self, identity: [u8; 32]) -> Result<(), InterpreterError> {
        self.reserve_consensus_identity(identity, BillableKind::Comm, 1, "COMM reduction")
    }

    pub fn reserve_produce_introduction_identity(
        &self,
        identity: [u8; 32],
        cost_authority: &CostAuthority,
        byte_cost: u64,
        persistent: bool,
    ) -> Result<(), InterpreterError> {
        self.reserve_introduction_identity(
            identity,
            BillableKind::RSpaceProduce,
            authority::AuthorityByteEventKind::ProduceIntroduction,
            cost_authority,
            byte_cost,
            persistent,
            "RSpace produce introduction bytes",
        )
    }

    pub fn register_introduction_authority(
        &self,
        identity: [u8; 32],
        kind: authority::AuthorityByteEventKind,
        cost_authority: &CostAuthority,
    ) -> Result<(), InterpreterError> {
        if !self.has_comm_accounting_scope() || self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(());
        }
        let canonical = authority::canonical_authority(cost_authority)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let canonical = if canonical.regions.is_empty() {
            self.fallback_introduction_authority(identity, kind)?
        } else {
            canonical
        };
        let mut authorities = self
            .introduction_authorities
            .lock()
            .expect("introduction authority map");
        match authorities.get(&(identity, kind)) {
            Some(existing) if existing == &canonical => Ok(()),
            Some(_) => Err(InterpreterError::ReduceError(
                authority::AuthorityError::EventIdentityConflict.to_string(),
            )),
            None => {
                authorities.insert((identity, kind), canonical);
                Ok(())
            }
        }
    }

    pub fn introduction_authority(
        &self,
        identity: [u8; 32],
        kind: authority::AuthorityByteEventKind,
    ) -> Result<CostAuthority, InterpreterError> {
        if let Some(authority) = self
            .introduction_authorities
            .lock()
            .expect("introduction authority map")
            .get(&(identity, kind))
            .cloned()
        {
            return Ok(authority);
        }
        let fallback = self.fallback_introduction_authority(identity, kind)?;
        let mut authorities = self
            .introduction_authorities
            .lock()
            .expect("introduction authority map");
        Ok(authorities
            .entry((identity, kind))
            .or_insert(fallback)
            .clone())
    }

    fn fallback_introduction_authority(
        &self,
        identity: [u8; 32],
        kind: authority::AuthorityByteEventKind,
    ) -> Result<CostAuthority, InterpreterError> {
        let signature = authority::sig_to_cost_signature(&self.signature())
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let region = authority::cost_region(&signature, &identity, u32::from(kind.tag()))
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        authority::canonical_authority(&CostAuthority {
            regions: vec![region],
        })
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))
    }

    pub fn reserve_consume_introduction_identity(
        &self,
        identity: [u8; 32],
        cost_authority: &CostAuthority,
        byte_cost: u64,
        persistent: bool,
    ) -> Result<(), InterpreterError> {
        self.reserve_introduction_identity(
            identity,
            BillableKind::RSpaceConsume,
            authority::AuthorityByteEventKind::ConsumeIntroduction,
            cost_authority,
            byte_cost,
            persistent,
            "RSpace consume introduction bytes",
        )
    }

    fn reserve_introduction_identity(
        &self,
        identity: [u8; 32],
        kind: BillableKind,
        byte_kind: authority::AuthorityByteEventKind,
        cost_authority: &CostAuthority,
        byte_cost: u64,
        persistent: bool,
        description: &'static str,
    ) -> Result<(), InterpreterError> {
        if !self.has_comm_accounting_scope() || self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(());
        }
        let canonical_authority = authority::canonical_authority(cost_authority)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        if canonical_authority.regions.is_empty() {
            return Err(InterpreterError::ReduceError(
                authority::AuthorityError::MissingAuthority.to_string(),
            ));
        }
        if !persistent {
            self.reserve_consensus_identity(identity, kind, byte_cost, description)?;
            if byte_cost > 0 {
                self.authority_state
                    .lock()
                    .expect("authority state")
                    .byte_events
                    .push(authority::AuthorityByteEvent {
                        event_id: identity,
                        kind: byte_kind,
                        authority: canonical_authority,
                        amount: byte_cost,
                    });
            }
            return Ok(());
        }
        let key = (identity, kind.clone());
        let mut introductions = self
            .persistent_introductions
            .lock()
            .expect("persistent introduction set");
        if introductions.contains(&key) {
            return Ok(());
        }
        self.reserve_consensus_identity(identity, kind, byte_cost, description)?;
        if byte_cost > 0 {
            self.authority_state
                .lock()
                .expect("authority state")
                .byte_events
                .push(authority::AuthorityByteEvent {
                    event_id: identity,
                    kind: byte_kind,
                    authority: canonical_authority,
                    amount: byte_cost,
                });
        }
        introductions.insert(key);
        Ok(())
    }

    pub fn install_authority_allocation(&self, allocation: authority::ResourceMultiset<[u8; 32]>) {
        let mut state = self.authority_state.lock().expect("authority state");
        state.allocation = allocation;
        state.enforce_allocation = true;
        state.events.clear();
        state.byte_events.clear();
        state.realized = authority::ResourceMultiset::default();
        state.reserved = authority::ResourceMultiset::default();
        state.frontier.clear();
        state.stack_births.clear();
        state.pending_stack_transfers.clear();
        state.pending_stack_event_ids.clear();
    }

    pub fn reserve_comm_authority_identity(
        &self,
        identity: [u8; 32],
        cost_authority: &CostAuthority,
    ) -> Result<(), InterpreterError> {
        self.reserve_comm_authority_identity_with_byte_cost(identity, cost_authority, 0)
    }

    pub fn reserve_comm_authority_identity_with_byte_cost(
        &self,
        identity: [u8; 32],
        cost_authority: &CostAuthority,
        byte_cost: u64,
    ) -> Result<(), InterpreterError> {
        self.reserve_authority_identities(&[identity], cost_authority, Some(byte_cost), true)
    }

    pub fn prepare_authority_stack_transfer(
        &self,
        produce_hash: [u8; 32],
        cells: Vec<CostSignature>,
        cost_authority: &CostAuthority,
    ) -> Result<AuthorityStackTransferReservation, InterpreterError> {
        if !self.has_comm_accounting_scope() || self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(AuthorityStackTransferReservation {
                budget: self.clone(),
                produce_hash: None,
            });
        }
        if cells.is_empty() {
            return Err(InterpreterError::ReduceError(
                authority::AuthorityError::MissingSignature.to_string(),
            ));
        }
        let canonical_authority = authority::canonical_authority(cost_authority)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        if canonical_authority.regions.is_empty() {
            return Err(InterpreterError::ReduceError(
                authority::AuthorityError::MissingAuthority.to_string(),
            ));
        }
        let demand = authority::authority_demand(&canonical_authority)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let mut events = BTreeMap::new();
        for cell_index in 0..cells.len() {
            let cell_index = u64::try_from(cell_index).map_err(|_| {
                InterpreterError::ReduceError("cost-stack transfer index overflow".to_string())
            })?;
            let identity = authority::stack_transfer_event_id(&produce_hash, cell_index);
            events.insert(identity, AuthorityRuntimeEvent {
                authority: canonical_authority.clone(),
                debit: demand.clone(),
            });
        }
        let aggregate_demand = events
            .values()
            .try_fold(
                authority::ResourceMultiset::default(),
                |aggregate, event| aggregate.checked_add(&event.debit),
            )
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let birth = authority::AuthorityStackBirth {
            produce_hash,
            cells,
        };
        let mut state = self.authority_state.lock().expect("authority state");
        if state.stack_births.contains_key(&produce_hash)
            || state.pending_stack_transfers.contains_key(&produce_hash)
            || events.keys().any(|identity| {
                state.events.contains_key(identity)
                    || state.pending_stack_event_ids.contains(identity)
            })
        {
            return Err(InterpreterError::ReduceError(
                authority::AuthorityError::EventIdentityConflict.to_string(),
            ));
        }
        let next_reserved = state
            .reserved
            .checked_add(&aggregate_demand)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        if state.enforce_allocation && !state.allocation.dominates(&next_reserved) {
            for identity in events.keys() {
                state
                    .frontier
                    .insert(*identity, canonical_authority.clone());
            }
            return Err(InterpreterError::OutOfPhlogistonsError);
        }
        state.pending_stack_event_ids.extend(events.keys().copied());
        state
            .pending_stack_transfers
            .insert(produce_hash, PendingAuthorityStackTransfer {
                events,
                debit: aggregate_demand,
                birth,
            });
        state.reserved = next_reserved;
        Ok(AuthorityStackTransferReservation {
            budget: self.clone(),
            produce_hash: Some(produce_hash),
        })
    }

    fn commit_authority_stack_transfer(&self, produce_hash: [u8; 32]) {
        let mut state = self.authority_state.lock().expect("authority state");
        let pending = state
            .pending_stack_transfers
            .remove(&produce_hash)
            .expect("prepared authority stack transfer");
        for (identity, event) in pending.events {
            let removed = state.pending_stack_event_ids.remove(&identity);
            assert!(removed);
            let previous = state.events.insert(identity, event);
            assert!(previous.is_none());
        }
        state.realized = state
            .realized
            .checked_add(&pending.debit)
            .expect("prepared authority debit");
        let previous = state.stack_births.insert(produce_hash, pending.birth);
        assert!(previous.is_none());
    }

    fn abort_authority_stack_transfer(&self, produce_hash: [u8; 32]) {
        let mut state = self.authority_state.lock().expect("authority state");
        let Some(pending) = state.pending_stack_transfers.remove(&produce_hash) else {
            return;
        };
        for identity in pending.events.keys() {
            state.pending_stack_event_ids.remove(identity);
        }
        state.reserved = state
            .reserved
            .checked_sub(&pending.debit)
            .expect("pending authority debit is reserved");
    }

    pub fn rollback_authority_stack_transfers(&self) -> Result<(), InterpreterError> {
        let mut state = self.authority_state.lock().expect("authority state");
        let mut pending_debit = authority::ResourceMultiset::default();
        let mut expected_pending_ids = BTreeSet::new();
        for pending in state.pending_stack_transfers.values() {
            pending_debit = pending_debit
                .checked_add(&pending.debit)
                .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
            expected_pending_ids.extend(pending.events.keys().copied());
        }
        if expected_pending_ids != state.pending_stack_event_ids {
            return Err(InterpreterError::ReduceError(
                "pending authority stack identities disagree with their transfers".to_string(),
            ));
        }

        let mut committed_debit = authority::ResourceMultiset::default();
        let mut committed_event_ids = BTreeSet::new();
        for birth in state.stack_births.values() {
            for cell_index in 0..birth.cells.len() {
                let cell_index = u64::try_from(cell_index).map_err(|_| {
                    InterpreterError::ReduceError("cost-stack transfer index overflow".to_string())
                })?;
                let identity = authority::stack_transfer_event_id(&birth.produce_hash, cell_index);
                if !committed_event_ids.insert(identity) {
                    return Err(InterpreterError::ReduceError(
                        authority::AuthorityError::EventIdentityConflict.to_string(),
                    ));
                }
                let event = state.events.get(&identity).ok_or_else(|| {
                    InterpreterError::ReduceError(
                        "committed authority stack birth is missing its transfer event".to_string(),
                    )
                })?;
                committed_debit = committed_debit
                    .checked_add(&event.debit)
                    .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
            }
        }

        let released = pending_debit
            .checked_add(&committed_debit)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let next_reserved = state
            .reserved
            .checked_sub(&released)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let next_realized = state
            .realized
            .checked_sub(&committed_debit)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;

        for identity in committed_event_ids {
            state.events.remove(&identity);
        }
        state.pending_stack_transfers.clear();
        state.pending_stack_event_ids.clear();
        state.stack_births.clear();
        state.reserved = next_reserved;
        state.realized = next_realized;
        Ok(())
    }

    fn reserve_authority_identities(
        &self,
        identities: &[[u8; 32]],
        cost_authority: &CostAuthority,
        comm_byte_cost: Option<u64>,
        existing_is_idempotent: bool,
    ) -> Result<(), InterpreterError> {
        if !self.has_comm_accounting_scope() || self.unmetered.load(Ordering::Acquire) != 0 {
            return Ok(());
        }
        let canonical_authority = authority::canonical_authority(cost_authority)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        if canonical_authority.regions.is_empty() {
            return Err(InterpreterError::ReduceError(
                authority::AuthorityError::MissingAuthority.to_string(),
            ));
        }
        let mut state = self.authority_state.lock().expect("authority state");
        let mut unique_identities = BTreeSet::new();
        for identity in identities {
            if !unique_identities.insert(*identity) {
                return Err(InterpreterError::ReduceError(
                    authority::AuthorityError::EventIdentityConflict.to_string(),
                ));
            }
            if let Some(existing) = state.events.get(identity) {
                if !existing_is_idempotent || existing.authority != canonical_authority {
                    return Err(InterpreterError::ReduceError(
                        authority::AuthorityError::EventIdentityConflict.to_string(),
                    ));
                }
                unique_identities.remove(identity);
            }
            if state.pending_stack_event_ids.contains(identity) {
                return Err(InterpreterError::ReduceError(
                    authority::AuthorityError::EventIdentityConflict.to_string(),
                ));
            }
        }
        let demand = authority::authority_demand(&canonical_authority)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let aggregate_demand = unique_identities
            .iter()
            .try_fold(authority::ResourceMultiset::default(), |aggregate, _| {
                aggregate.checked_add(&demand)
            })
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        let next_reserved = state
            .reserved
            .checked_add(&aggregate_demand)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        if state.enforce_allocation && !state.allocation.dominates(&next_reserved) {
            for identity in unique_identities {
                state.frontier.insert(identity, canonical_authority.clone());
            }
            return Err(InterpreterError::OutOfPhlogistonsError);
        }
        if let Some(byte_cost) = comm_byte_cost.filter(|_| !demand.0.is_empty()) {
            let weight = byte_cost
                .checked_add(1)
                .ok_or(InterpreterError::OutOfPhlogistonsError)?;
            for identity in &unique_identities {
                if let Err(error) = self.reserve_consensus_identity(
                    *identity,
                    BillableKind::Comm,
                    weight,
                    "COMM authority and byte debit",
                ) {
                    state
                        .frontier
                        .insert(*identity, canonical_authority.clone());
                    return Err(error);
                }
            }
            if byte_cost > 0 && self.unmetered.load(Ordering::Acquire) == 0 {
                state
                    .byte_events
                    .extend(unique_identities.iter().map(|identity| {
                        authority::AuthorityByteEvent {
                            event_id: *identity,
                            kind: authority::AuthorityByteEventKind::Comm,
                            authority: canonical_authority.clone(),
                            amount: byte_cost,
                        }
                    }));
            }
        }
        for identity in unique_identities {
            state.events.insert(identity, AuthorityRuntimeEvent {
                authority: canonical_authority.clone(),
                debit: demand.clone(),
            });
        }
        state.realized = state
            .realized
            .checked_add(&aggregate_demand)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        state.reserved = next_reserved;
        Ok(())
    }

    pub fn authority_realized(&self) -> authority::ResourceMultiset<[u8; 32]> {
        self.authority_state
            .lock()
            .expect("authority state")
            .realized
            .clone()
    }

    pub fn authority_events(&self) -> Vec<authority::AuthorityEvent<[u8; 32]>> {
        self.authority_state
            .lock()
            .expect("authority state")
            .events
            .iter()
            .map(|(event_id, event)| authority::AuthorityEvent {
                event_id: *event_id,
                authority: event.authority.clone(),
                debit: event.debit.clone(),
            })
            .collect()
    }

    pub fn authority_byte_events(&self) -> Vec<authority::AuthorityByteEvent> {
        let mut events = self
            .authority_state
            .lock()
            .expect("authority state")
            .byte_events
            .clone();
        events.sort_by_key(authority::AuthorityByteEvent::canonical_key);
        events
    }

    pub fn authority_stack_births(&self) -> Vec<authority::AuthorityStackBirth> {
        self.authority_state
            .lock()
            .expect("authority state")
            .stack_births
            .values()
            .cloned()
            .collect()
    }

    pub fn authority_frontier(&self) -> Vec<CostAuthority> {
        self.authority_state
            .lock()
            .expect("authority state")
            .frontier
            .values()
            .cloned()
            .collect()
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

        let initial = self.initial_tokens.load(Ordering::Acquire);
        // Consensus RSpace operations consume their validated quantitative
        // weight. Diagnostic reductions, primitives, and substitutions cost
        // zero and cannot exhaust the budget.
        let cost_unit = Self::consensus_cost_unit(&event);

        // A zero-cost event always proceeds (it does not touch the budget).
        if cost_unit == 0 {
            if self.max_log_entries > 0
                && self
                    .diagnostic_record_count
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                        (count < self.max_log_entries as u64).then_some(count + 1)
                    })
                    .is_ok()
            {
                self.attempt_queue.push(AttemptRecord { event, amount });
                self.attempt_generation.fetch_add(1, Ordering::AcqRel);
            }
            return AttemptOutcome::Granted;
        }

        let record_limit = u64::try_from(initial.max(0))
            .unwrap_or_default()
            .saturating_add(1);
        self.canonical_consensus_attempts
            .lock()
            .expect("canonical consensus attempt window")
            .insert(
                AttemptRecord {
                    event: event.clone(),
                    amount,
                },
                record_limit,
            );
        self.attempt_generation.fetch_add(1, Ordering::AcqRel);

        // Lock-free CAS loop for a positive-weight consensus event. On overflow, return
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
            let Some(next) = current.checked_add(cost_unit) else {
                return AttemptOutcome::Oop;
            };
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

        let generation = self.attempt_generation.load(Ordering::Acquire);
        if !drained_any && self.reconciled_generation.load(Ordering::Acquire) == generation {
            if let Some(rec) = cache.as_ref() {
                return rec.clone();
            }
        }

        let initial = self.initial_tokens.load(Ordering::Acquire);

        // The canonical commit walk is a pure function of the immutable
        // capacity and complete attempt multiset.
        let mut attempts: Vec<AttemptRecord> = {
            let accumulator = self
                .attempt_accumulator
                .lock()
                .expect("attempt accumulator poisoned");
            accumulator.clone()
        };
        attempts.extend(
            self.canonical_consensus_attempts
                .lock()
                .expect("canonical consensus attempt window")
                .attempts(),
        );
        let rec = Self::reconcile_lane(initial, &attempts);

        // Repopulate the diagnostic `event_log` / `log` mirrors from the
        // canonical committed set. This moves their population OFF the
        // hot path (they were previously appended per-grant inside
        // `attempt_one`) and onto finalization, so `get_event_log` /
        // `get_log` now reflect the canonical committed set rather than a
        // schedule-dependent record of CAS-race winners.
        self.repopulate_diagnostic_logs(&attempts, &rec.committed, &rec.cost_amounts);

        *cache = Some(rec.clone());
        self.reconciled_generation
            .store(generation, Ordering::Release);
        rec
    }

    /// Pure canonical reconciliation over one signature's pool: given an
    /// `initial` budget and the multiset of reservation `attempts` recorded
    /// for that signature, return the canonical `CanonicalReconciliation`
    /// (committed set, OOP boundary, clamped `consumed_units`, reconstructed
    /// `cost_amounts`) in the schedule-INDEPENDENT canonical reduction order.
    ///
    /// This is the exact walk previously inlined in `reconcile()`. Located
    /// purse decomposition is enforced independently by native authority
    /// events and physical settlement; the scalar here is only the conserved
    /// aggregate execution-capacity ceiling.
    ///
    /// Pure: no `self` access, no interior mutation — output depends only on
    /// `(initial, attempts)`, never on Tokio scheduling.
    fn reconcile_lane(initial: i64, attempts: &[AttemptRecord]) -> CanonicalReconciliation {
        // Consensus RSpace events contribute their validated weights;
        // reductions, primitives, and substitutions remain diagnostic. The
        // OOP boundary is the first canonical positive-weight event whose
        // addition would exceed the immutable initial reservation.
        //
        // Canonical sort key is the derived Ord on BillableTokenEvent.
        // Multiplicity is preserved: a deploy that re-attempts the same
        // logical event (e.g. through a loop) MUST see the repeated
        // attempt contribute, just as it did under the pre-Option-E
        // commit_lock contract.
        let mut attempts: Vec<AttemptRecord> = attempts
            .iter()
            .filter(|attempt| Self::is_consensus_kind(&attempt.event.kind))
            .cloned()
            .collect();
        attempts.sort_by(|a, b| a.event.cmp(&b.event));

        // Simulate the canonical weighted commit walk.
        let mut committed = Vec::with_capacity(attempts.len());
        let mut cost_amounts: Vec<Cost> = Vec::new();
        let mut consumed_units: i64 = 0;
        let mut oop: Option<BillableTokenEvent> = None;

        for rec in attempts.into_iter() {
            let cost_unit = Self::consensus_cost_unit(&rec.event);
            let Some(next) = consumed_units.checked_add(cost_unit) else {
                oop = Some(rec.event);
                consumed_units = initial;
                break;
            };
            // A zero-cost diagnostic event leaves `consumed_units` unchanged.
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

    /// The per-event consensus cost contribution. Canonical RSpace events use
    /// their validated quantitative weight; diagnostic reductions, primitives,
    /// and substitutions contribute zero.
    #[inline]
    fn is_consensus_kind(kind: &BillableKind) -> bool {
        matches!(
            kind,
            BillableKind::Comm | BillableKind::RSpaceProduce | BillableKind::RSpaceConsume
        )
    }

    #[inline]
    fn consensus_cost_unit(event: &BillableTokenEvent) -> i64 {
        match event.kind {
            BillableKind::Comm | BillableKind::RSpaceProduce | BillableKind::RSpaceConsume => {
                i64::try_from(event.weight).expect("validated consensus event weight")
            }
            BillableKind::Reduction | BillableKind::Primitive(_) | BillableKind::Substitution => 0,
        }
    }

    /// Repopulate the bounded diagnostic `event_log` / `log` ring buffers
    /// from the canonical committed set. Called only from `reconcile()`
    /// (under the cache lock) at finalization. The ring buffers retain at
    /// most `max_log_entries` of the lowest-rank committed events/costs.
    fn repopulate_diagnostic_logs(
        &self,
        attempts: &[AttemptRecord],
        committed: &[BillableTokenEvent],
        cost_amounts: &[Cost],
    ) {
        if self.max_log_entries == 0 {
            return;
        }
        {
            let mut event_log = self.event_log.lock().expect("event log");
            event_log.clear();
            let mut events = attempts
                .iter()
                .filter(|attempt| !Self::is_consensus_kind(&attempt.event.kind))
                .map(|attempt| attempt.event.clone())
                .chain(committed.iter().cloned())
                .collect::<Vec<_>>();
            events.sort();
            events.truncate(self.max_log_entries);
            for event in events {
                event_log.push_back(event);
            }
        }
        {
            let mut log = self.log.lock().expect("cost log");
            log.clear();
            let mut amounts = attempts
                .iter()
                .filter(|attempt| !Self::is_consensus_kind(&attempt.event.kind))
                .filter_map(|attempt| attempt.amount.clone())
                .chain(cost_amounts.iter().cloned())
                .collect::<Vec<_>>();
            amounts.truncate(self.max_log_entries);
            for amount in amounts {
                log.push_back(amount);
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

    pub fn reset_for_system_deploy(&self) {
        *self.deploy_id.lock().expect("deploy id lock") = [0; 32];
        self.reset_from_token(&Token::coalesced(
            Sig::Unit,
            cost_value_to_token_count(Cost::unsafe_max().value),
        ));
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
        self.diagnostic_record_count.store(0, Ordering::Release);
        self.persistent_introductions
            .lock()
            .expect("persistent introduction set")
            .clear();
        self.introduction_authorities
            .lock()
            .expect("introduction authority map")
            .clear();
        self.canonical_consensus_attempts
            .lock()
            .expect("canonical consensus attempt window")
            .clear();
        self.attempt_generation.store(0, Ordering::Release);
        self.reconciled_generation.store(0, Ordering::Release);
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
        {
            let mut authority = self.authority_state.lock().expect("authority state");
            authority.allocation = authority::ResourceMultiset::default();
            authority.enforce_allocation = false;
            authority.events.clear();
            authority.byte_events.clear();
            authority.realized = authority::ResourceMultiset::default();
            authority.reserved = authority::ResourceMultiset::default();
            authority.frontier.clear();
            authority.stack_births.clear();
            authority.pending_stack_transfers.clear();
            authority.pending_stack_event_ids.clear();
        }
        *cache = None;
    }

    pub fn set_deploy_signature(&self, signature: &[u8]) {
        // Legacy single-sig install. The FUNDING signature is the wire-sig
        // `#P`/`Sig::Quote` envelope atom (`envelope_sig_single`), preserving
        // the pre-`funding_sig` behavior bit-for-bit — so every test/bench
        // caller (and its `Sig::Quote`-variant assertions) is unchanged.
        // PRODUCTION (`evaluate_cosigned`) routes through
        // `set_deploy_signature_funded` with the signer's GROUND public key, so
        // the funded pool is the genesis-seeded wallet `Σ⟦Ground(pk)⟧`
        // (`Σ⟦signer⟧ == Σ⟦wallet⟧`, cost-accounting WD-D2 §D2.9).
        self.set_deploy_signature_funded(signature, envelope_sig_single(signature));
    }

    /// Install a single-signer deploy with a DECOUPLED funding signature
    /// (cost-accounting WD-D2 §D2.9 — `Σ⟦signer⟧ == Σ⟦wallet⟧`).
    ///
    /// The `deploy_id` is derived from the WIRE signature `signature` and is
    /// byte-identical to the legacy [`set_deploy_signature`] (so a deploy's
    /// on-chain identity NEVER moves under the funding-key decoupling), while
    /// `funding_sig` keys the supply pool `Σ⟦s⟧` and the per-redex signer
    /// channels. Production passes `funding_sig = Sig::Ground(signer_pubkey)`
    /// (via [`funding_sig`]) so the pool is the genesis-seeded wallet; the
    /// legacy wrapper passes the wire-sig `Sig::Quote` envelope.
    pub fn set_deploy_signature_funded(&self, signature: &[u8], funding_sig: Sig) {
        // deploy_id = the wire-signature `#P`-atom digest, UNCHANGED. The funded
        // pool moves to `Σ⟦Ground(pk)⟧`; the on-chain deploy_id does not.
        let id_sig = envelope_sig_single(signature);
        let mut deploy_id = [0; 32];
        match &id_sig {
            Sig::Quote(hash) => deploy_id.copy_from_slice(&hash[..32]),
            // `envelope_sig_single` is total to `Sig::Quote`; this arm is
            // unreachable and exists only to keep the match exhaustive.
            _ => unreachable!("envelope_sig_single always yields Sig::Quote"),
        }
        *self.deploy_id.lock().expect("deploy id lock") = deploy_id;
        *self.signature.lock().expect("signature lock") = funding_sig;
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
        // Legacy compound install. The FUNDING signature is the wire-sig
        // `Sig::And` fold of `Sig::Quote` leaves (`envelope_sig_compound`),
        // preserving the pre-`funding_sig` behavior bit-for-bit (every
        // test/bench caller unchanged). PRODUCTION (`evaluate_cosigned`) routes
        // through `set_deploy_signatures_funded` with the cosigners' GROUND
        // public keys so each component pool is the genesis-seeded wallet
        // `Σ⟦Ground(pkᵢ)⟧` (P8-balanced over cosigners).
        //
        // Guard the empty case HERE (before the `envelope_sig_compound` argument
        // is evaluated) so the panic message is the legacy one, not the inner
        // `fold_compound_sig` expect.
        assert!(
            !signatures.is_empty(),
            "set_deploy_signatures requires at least one signature"
        );
        self.set_deploy_signatures_funded(signatures, envelope_sig_compound(signatures));
    }

    /// Install a compound (multi-signer) deploy with a DECOUPLED funding
    /// signature (cost-accounting WD-D2 §D2.9).
    ///
    /// The `deploy_id` is derived from the full ordered concatenation of the
    /// per-signer WIRE-signature hashes under the COMPOUND domain — byte-
    /// identical to the legacy [`set_deploy_signatures`] (canonical-order input
    /// ⇒ permutation-equal multi-sig deploys produce identical `deploy_id`s),
    /// while `funding_sig` keys the supply pools `Σ⟦sᵢ⟧`. Production passes the
    /// `And`-fold of `Sig::Ground(pkᵢ)` (via [`funding_sig`]) so each component
    /// pool is the genesis-seeded wallet `Σ⟦Ground(pkᵢ)⟧`.
    pub fn set_deploy_signatures_funded(&self, signatures: &[&[u8]], funding_sig: Sig) {
        assert!(
            !signatures.is_empty(),
            "set_deploy_signatures requires at least one signature"
        );

        // deploy_id derives from the full ordered concatenation of per-sig WIRE
        // hashes under the COMPOUND domain (UNCHANGED — the funded pool moves to
        // `Σ⟦Ground(pkᵢ)⟧`; the on-chain deploy_id does not). Per-signature
        // domain separation uses the COMPOUND domain so single-element calls
        // remain distinguishable from legacy single-sig deploys.
        let sig_hashes = compound_sig_hashes(signatures);
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
        *self.signature.lock().expect("signature lock") = funding_sig;
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

    pub fn enter_comm_accounting_scope(&self) -> CommAccountingScope {
        self.comm_accounting_scopes.fetch_add(1, Ordering::AcqRel);
        CommAccountingScope {
            budget: self.clone(),
        }
    }

    pub fn has_comm_accounting_scope(&self) -> bool {
        self.comm_accounting_scopes.load(Ordering::Acquire) != 0
    }

    pub fn is_unmetered(&self) -> bool { self.unmetered.load(Ordering::Acquire) != 0 }

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

    pub fn quantitative_byte_cost(&self) -> u64 {
        self.authority_byte_events()
            .iter()
            .try_fold(0_u64, |total, event| total.checked_add(event.amount))
            .expect("validated byte-cost trace overflow")
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
            update(&event.sig_hash);
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
                BillableKind::RSpaceProduce => update(&[5]),
                BillableKind::RSpaceConsume => update(&[6]),
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
            left_tag
                .cmp(right_tag)
                .then_with(|| left.deploy_id.cmp(&right.deploy_id))
                .then_with(|| left.sig_hash.cmp(&right.sig_hash))
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.weight.cmp(&right.weight))
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

pub struct CommAccountingScope {
    budget: RuntimeBudget,
}

impl Drop for CommAccountingScope {
    fn drop(&mut self) {
        let previous = self
            .budget
            .comm_accounting_scopes
            .fetch_sub(1, Ordering::AcqRel);
        assert!(previous != 0, "comm-accounting scope underflow");
    }
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
/// The envelope `Sig` drives the deploy's default unsigned authority and its
/// stable supply lookup. Explicit `CostSignedTerm` regions carry their own
/// authority, but gate, runtime, and replay must still derive the envelope key
/// identically. Anchoring those paths to this function is the no-drift guarantee.
pub fn envelope_sig<A>(cosigned: &crypto::rust::signatures::signed::Cosigned<A>) -> Sig
where A: std::fmt::Debug + serde::Serialize + crypto::rust::signatures::signed::ToMessage {
    if cosigned.is_compound() {
        let sigs: Vec<&[u8]> = cosigned.signers().iter().map(|s| s.sig.as_ref()).collect();
        envelope_sig_compound(&sigs)
    } else {
        envelope_sig_single(&cosigned.primary().sig)
    }
}

/// The FUNDING signature of a single signer: the spec's GROUND identity atom
/// `Sig::Ground(pk)` over the signer's raw public-key bytes (cost-accounted-rho
/// §"signature grammar" — a ground signature `g` is *"an Ed25519 public key, a
/// secp256k1 key hash"*). UNLIKE [`envelope_sig_single`] (which hashes the
/// per-deploy WIRE signature into a `#P`/`Sig::Quote` atom — a fresh value every
/// deploy), this keys the deploy's funding pool by the signer's STABLE identity,
/// so `Σ⟦signer⟧ == Σ⟦Ground(pk)⟧ ==` the genesis-seeded wallet `Σ⟦wallet⟧`
/// (cost-accounting WD-D2 §D2.9). DR-1 (ground/quote channel collapse) means
/// this reflects to the SAME channel as the genesis seed's `Sig::Ground(pk)`.
pub fn funding_sig_single(pubkey: &[u8]) -> Sig { Sig::Ground(pubkey.to_vec()) }

/// The FUNDING signature of a multi-signer deploy: the left-associated
/// `Sig::And` fold of each cosigner's ground identity atom `Sig::Ground(pkᵢ)`
/// (the compound `g₁∘g₂`). Fuel is drawn balanced across the cosigners' wallets
/// `Σ⟦Ground(pkᵢ)⟧` (P8) by the existing `compute_settlement_debits` +
/// `DefaultApportionment` over the `And`-fold's component pools. `pubkeys` MUST
/// be non-empty and in the canonical (pk-ascending) order the `Cosigned`
/// envelope is sorted by — so the fold (hence the on-chain `Sig`) is stable.
pub fn funding_sig_compound(pubkeys: &[&[u8]]) -> Sig {
    let mut iter = pubkeys.iter();
    let first = iter
        .next()
        .expect("funding_sig_compound requires at least one pubkey");
    iter.fold(Sig::Ground(first.to_vec()), |acc, pk| {
        Sig::And(Box::new(acc), Box::new(Sig::Ground(pk.to_vec())))
    })
}

/// The ONE function that derives a deploy's FUNDING signature from its
/// [`Cosigned`](crypto::rust::signatures::signed::Cosigned) envelope — keyed by
/// the signers' GROUND public keys, so the funded pool is the genesis-seeded
/// wallet (`Σ⟦signer⟧ == Σ⟦wallet⟧`, cost-accounting WD-D2 §D2.9). Used
/// IDENTICALLY by the runtime install
/// ([`RuntimeBudget::set_deploy_signature_funded`] /
/// [`RuntimeBudget::set_deploy_signatures_funded`]), the D2 acceptance gate
/// (`casper/.../util/rholang/acceptance.rs::build_candidate_with_logic`), and
/// replay (`recompute_settlement_debits_with_logic`) — the no-drift guarantee
/// that the gate, the install, and the replay recompute all key the SAME pool.
///
/// SECURITY (placeholder filter): only signers whose signature actually
/// VERIFIED contribute a funding atom. A Phase-2 threshold envelope may carry
/// empty-`sig` PLACEHOLDER cosigners (the un-signed members of an M-of-N set —
/// `Cosigned::from_signed_data_threshold`); funding from them would let a deploy
/// key a pool by an UNSIGNED victim's public key. Excluding empty-`sig` signers
/// means a deploy can only ever debit wallets whose owners SIGNED it. (Ingress
/// `from_proto_cosigned` already verifies every non-placeholder `sig` against
/// its `pk`, so a forger cannot present a victim's pk with a valid sig either.)
pub fn funding_sig<A>(cosigned: &crypto::rust::signatures::signed::Cosigned<A>) -> Sig
where A: std::fmt::Debug + serde::Serialize + crypto::rust::signatures::signed::ToMessage {
    let funders: Vec<&[u8]> = cosigned
        .signers()
        .iter()
        .filter(|signer| !signer.sig.is_empty())
        .map(|signer| signer.pk.bytes.as_ref())
        .collect();
    // A `Cosigned` constructed via `from_signed_data{,_threshold}` is guaranteed
    // ≥ threshold (≥ 1) VERIFIED (non-placeholder) signatures, so `funders` is
    // never empty for a well-formed envelope; a single funder is the single-sig
    // fast path, more than one is the balanced compound.
    match funders.as_slice() {
        [] => panic!(
            "funding_sig: a constructed Cosigned must carry ≥1 verified (non-placeholder) signer"
        ),
        [single] => funding_sig_single(single),
        _ => funding_sig_compound(&funders),
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
    /// that [`SignatureChannel::from_sig`] produces. Authority resources,
    /// purse snapshots, funding certificates, and SystemVault settlement are
    /// therefore anchored to the single function `from_sig`, so two
    /// signatures share a resource key iff they share a supply channel.
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

    /// The funding SIGNER CHANNELS this envelope `Sig` decomposes into — each a
    /// `(SignatureChannel::from_sig(leaf).par, leaf.lane_hash())` pair. A single
    /// atom (`Ground`/`Quote`/`Unit`) yields ONE entry whose channel and lane ARE
    /// the envelope's, so a single-signer deploy has exactly one signer channel
    /// (== the envelope) and the metering machine runs the scalar fast path. An
    /// `And`-fold (a multi-signer cosigned envelope — `envelope_sig_compound`)
    /// yields one entry per LEAF: the component pools native ALREADY funds via the
    /// `And`-fold + `compute_settlement_debits` component draws (DR-13 — no new
    /// pool-write path).
    ///
    /// This is the signer set used by the legacy per-redex channel-match
    /// diagnostic. Consensus settlement of located stacks and funding slots
    /// instead consumes native `CostAuthority` regions and does not infer
    /// authority from a data channel. Non-funding connectives never reach here
    /// (the envelope is funding-former by construction — F-A /
    /// [`is_funding_former`]); a non-`And` shape is treated as a single signer
    /// channel, which is correct for the funding grammar `g | #P | s ∘ s`.
    ///
    /// [`is_funding_former`]: Sig::is_funding_former
    pub fn signer_channels(&self) -> Vec<(Par, [u8; 32])> {
        match self {
            Sig::And(left, right) => {
                let mut channels = left.signer_channels();
                channels.extend(right.signer_channels());
                channels
            }
            atom => vec![(SignatureChannel::from_sig(atom).par, atom.lane_hash())],
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
mod runtime_budget_tests {
    use super::*;

    // Build a deterministic COMM attempt record with a fixed deploy/sig
    // context; `local_index` drives the canonical `Ord` rank within a lane and
    // `weight` includes the execution unit and quantitative byte cost.
    fn attempt(local_index: u64, weight: u64) -> AttemptRecord {
        attempt_kind(local_index, weight, BillableKind::Comm)
    }

    // Like [`attempt`] but with an explicit kind, so tests can exercise the
    // consensus-versus-diagnostic split.
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

    fn test_authority() -> CostAuthority {
        let signature = authority::sig_to_cost_signature(&Sig::Ground(b"payer".to_vec())).unwrap();
        CostAuthority {
            regions: vec![authority::cost_region(&signature, b"test", 0).unwrap()],
        }
    }

    /// The runtime reconciliation equals its extracted pure walk over the same
    /// attempt multiset field-for-field.
    #[test]
    fn runtime_reconciliation_matches_pure_weighted_consensus_walk() {
        let initial = 5_i64;
        let budget = RuntimeBudget::new(Cost::create(initial, "scalar fast path"));

        // Drive attempts through the PUBLIC scalar entry point — exactly what
        // every single-signature deploy does. The first two events consume the
        // full reservation and the third establishes the OOP boundary.
        let attempts = vec![attempt(0, 2), attempt(1, 3), attempt(2, 1)];
        for record in &attempts {
            let _ = budget.reserve_canonical_with_cost(
                record.event.clone(),
                record.amount.clone().expect("test amount"),
            );
        }

        // The budget's scalar reconciliation must equal the extracted canonical
        // walk over the same attempt multiset, field-for-field.
        let scalar = budget.reconcile();
        let reference = RuntimeBudget::reconcile_lane(initial, &attempts);
        assert_eq!(
            scalar, reference,
            "scalar reconcile() must equal reconcile_lane() field-for-field"
        );

        // Weighted canonical answer: two events commit, the third OOPs, and
        // consumed clamps to the immutable reservation.
        assert_eq!(scalar.consumed_units, initial);
        assert_eq!(scalar.committed.len(), 2);
        assert!(scalar.oop.is_some(), "the third COMM is the OOP boundary");

        assert_eq!(budget.total_cost().value, initial);
    }

    #[test]
    fn reduction_events_do_not_enter_the_forced_redex_trace() {
        let budget = RuntimeBudget::new(Cost::create(4, "weighted consensus budget"));
        // One weighted COMM interleaved with three diagnostic reductions.
        let attempts = vec![
            attempt_kind(0, 4, BillableKind::Comm),
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
        assert_eq!(rec.committed.len(), 1);
        assert!(
            rec.oop.is_none(),
            "no OOP — only the weighted COMM costs, and it fits"
        );
        assert_eq!(rec.consumed_units, 4);
        assert_eq!(budget.total_cost().value, 4);
    }

    /// A second weighted COMM beyond the reservation is the OOP boundary even
    /// when diagnostic reductions precede it.
    #[test]
    fn second_comm_over_budget_is_oop_despite_reductions() {
        let budget = RuntimeBudget::new(Cost::create(1, "one-comm budget"));
        let attempts = vec![
            attempt_kind(0, 1, BillableKind::Comm),
            attempt_kind(1, 100, BillableKind::Reduction),
            attempt_kind(2, 1, BillableKind::Comm),
        ];
        for record in &attempts {
            let _ = budget.reserve_canonical_with_cost(
                record.event.clone(),
                record.amount.clone().expect("test amount"),
            );
        }
        let rec = budget.reconcile();
        assert_eq!(rec.committed.len(), 1);
        assert!(
            rec.oop.is_some(),
            "the second COMM exceeds the 1-token budget"
        );
        assert_eq!(rec.consumed_units, 1, "consumed clamps to the reservation");
    }

    #[test]
    fn attempted_comm_trace_is_bounded_by_finite_capacity_plus_oop() {
        let budget = RuntimeBudget::new(Cost::create(2, "two-comm budget"));
        for index in 0..100 {
            let _ = budget.reserve_canonical(attempt(index, 1).event);
        }

        let rec = budget.reconcile();
        assert_eq!(rec.committed.len(), 2);
        assert!(rec.oop.is_some());
        assert_eq!(
            budget
                .canonical_consensus_attempts
                .lock()
                .expect("canonical consensus attempt window")
                .len,
            3
        );
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

    #[test]
    fn system_deploy_reset_removes_prior_user_authority_identity() {
        let budget = RuntimeBudget::new(Cost::create(7, "user deploy"));
        budget.set_deploy_signature_funded(b"wire signature", Sig::Ground(b"payer".to_vec()));
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(
            Sig::Ground(b"payer".to_vec()).lane_hash(),
            7,
        ));

        budget.reset_for_system_deploy();

        assert_eq!(budget.signature(), Sig::Unit);
        assert_eq!(budget.deploy_id(), [0; 32]);
        assert!(budget.authority_realized().0.is_empty());
    }

    #[test]
    fn a_region_is_debited_for_each_distinct_comm() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let region = authority::cost_region(&signature, b"wrapper", 0).unwrap();
        let authority = CostAuthority {
            regions: vec![region],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let budget = RuntimeBudget::new(Cost::create(2, "two comms"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(lane, 2));

        budget
            .reserve_comm_authority_identity([1; 32], &authority)
            .unwrap();
        budget
            .reserve_comm_authority_identity([2; 32], &authority)
            .unwrap();

        assert_eq!(budget.authority_realized().get(&lane), 2);
        let events = budget.authority_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].debit.get(&lane) + events[1].debit.get(&lane), 2);
    }

    #[test]
    fn unmetered_communication_requires_no_authority_or_byte_reservation() {
        let budget = RuntimeBudget::unmetered();
        let _scope = budget.enter_comm_accounting_scope();

        budget
            .reserve_comm_authority_identity_with_byte_cost(
                [9; 32],
                &CostAuthority::default(),
                u64::MAX,
            )
            .unwrap();

        assert!(budget.authority_events().is_empty());
        assert!(budget.authority_byte_events().is_empty());
        assert_eq!(budget.quantitative_byte_cost(), 0);
    }

    #[test]
    fn native_introduction_without_reducer_context_uses_the_deploy_payer() {
        let payer = Sig::Ground(b"native introduction payer".to_vec());
        let budget = RuntimeBudget::new(Cost::create(1_000, "native introduction"));
        budget.set_deploy_signature_funded(b"native introduction deploy", payer.clone());
        let _scope = budget.enter_comm_accounting_scope();

        let authority = budget
            .introduction_authority(
                [7; 32],
                authority::AuthorityByteEventKind::ProduceIntroduction,
            )
            .unwrap();

        assert_eq!(
            authority::authority_demand(&authority)
                .unwrap()
                .get(&payer.lane_hash()),
            1
        );
        assert_eq!(
            budget.register_introduction_authority(
                [7; 32],
                authority::AuthorityByteEventKind::ProduceIntroduction,
                &test_authority(),
            ),
            Err(InterpreterError::ReduceError(
                authority::AuthorityError::EventIdentityConflict.to_string()
            ))
        );
    }

    #[test]
    fn authority_neutral_introduction_is_pinned_to_the_deploy_payer() {
        let payer = Sig::Ground(b"authority neutral introduction payer".to_vec());
        let budget = RuntimeBudget::new(Cost::create(1_000, "authority neutral introduction"));
        budget.set_deploy_signature_funded(b"authority neutral deploy", payer.clone());
        let _scope = budget.enter_comm_accounting_scope();
        let identity = [13; 32];
        let kind = authority::AuthorityByteEventKind::ProduceIntroduction;

        budget
            .register_introduction_authority(identity, kind, &CostAuthority::default())
            .unwrap();
        let resolved = budget.introduction_authority(identity, kind).unwrap();

        assert_eq!(
            authority::authority_demand(&resolved)
                .unwrap()
                .get(&payer.lane_hash()),
            1
        );
        assert_eq!(
            budget.register_introduction_authority(identity, kind, &test_authority()),
            Err(InterpreterError::ReduceError(
                authority::AuthorityError::EventIdentityConflict.to_string()
            ))
        );
    }

    #[test]
    fn quantitative_bytes_share_the_fixed_runtime_budget() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"wrapper", 0).unwrap()],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let budget = RuntimeBudget::new(Cost::create(19, "authority and bytes"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(lane, 1));

        budget
            .reserve_produce_introduction_identity([1; 32], &authority, 5, false)
            .unwrap();
        budget
            .reserve_consume_introduction_identity([2; 32], &authority, 7, false)
            .unwrap();
        budget
            .reserve_comm_authority_identity_with_byte_cost([3; 32], &authority, 6)
            .unwrap();

        assert_eq!(budget.total_cost().value, 19);
        assert_eq!(budget.quantitative_byte_cost(), 18);
        assert_eq!(budget.authority_realized().get(&lane), 1);
        let byte_events = budget.authority_byte_events();
        assert_eq!(byte_events.len(), 3);
        assert_eq!(
            byte_events
                .iter()
                .map(|event| (event.kind, event.amount))
                .collect::<Vec<_>>(),
            vec![
                (authority::AuthorityByteEventKind::ProduceIntroduction, 5),
                (authority::AuthorityByteEventKind::ConsumeIntroduction, 7),
                (authority::AuthorityByteEventKind::Comm, 6),
            ]
        );
        assert!(byte_events
            .iter()
            .all(|event| authority::authority_demand(&event.authority)
                .unwrap()
                .get(&lane)
                == 1));
    }

    #[test]
    fn native_authority_is_the_only_per_purse_compute_and_byte_ledger() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let first = CostSignature {
            value: Some(Value::Ground(b"first payer".to_vec())),
        };
        let second = CostSignature {
            value: Some(Value::Ground(b"second payer".to_vec())),
        };
        let first_key = authority::cost_signature_to_sig(&first)
            .unwrap()
            .lane_hash();
        let second_key = authority::cost_signature_to_sig(&second)
            .unwrap()
            .lane_hash();
        let cost_authority = CostAuthority {
            regions: vec![
                authority::cost_region(&first, b"first region", 0).unwrap(),
                authority::cost_region(&second, b"second region", 0).unwrap(),
            ],
        };
        let allocation =
            authority::ResourceMultiset(BTreeMap::from([(first_key, 1), (second_key, 1)]));
        let budget = RuntimeBudget::new(Cost::create(4, "one comm and three bytes"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(allocation.clone());

        budget
            .reserve_comm_authority_identity_with_byte_cost([41; 32], &cost_authority, 3)
            .unwrap();

        assert_eq!(budget.total_cost().value, 4);
        assert_eq!(budget.authority_realized(), allocation);

        let signatures = BTreeMap::from([(first_key, first), (second_key, second)]);
        let inventory = authority::AuthorityPhysicalInventory {
            balances: authority::ResourceMultiset(BTreeMap::from([
                (first_key, 4),
                (second_key, 4),
            ])),
            ..Default::default()
        };
        let physical = authority::allocate_physical_settlement(
            &budget.authority_events(),
            &signatures,
            &inventory,
        )
        .unwrap();
        assert_eq!(physical.balance_debit, allocation);
        let after_compute = inventory
            .balances
            .checked_sub(&physical.balance_debit)
            .unwrap();
        let bytes = authority::allocate_quantitative_events(
            &budget.authority_byte_events(),
            &after_compute,
        )
        .unwrap();
        assert_eq!(
            bytes,
            authority::ResourceMultiset(BTreeMap::from([(first_key, 3), (second_key, 3),]))
        );
        assert!(after_compute.checked_sub(&bytes).unwrap().0.is_empty());
    }

    #[test]
    fn byte_exhaustion_does_not_commit_the_authority_event() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"wrapper", 0).unwrap()],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let budget = RuntimeBudget::new(Cost::create(4, "insufficient byte budget"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(lane, 1));

        assert_eq!(
            budget.reserve_comm_authority_identity_with_byte_cost([4; 32], &authority, 4),
            Err(InterpreterError::OutOfPhlogistonsError)
        );
        assert!(budget.authority_events().is_empty());
        assert!(budget.authority_byte_events().is_empty());
        assert!(budget.authority_realized().0.is_empty());
        assert_eq!(budget.total_cost().value, 4);
        assert_eq!(budget.quantitative_byte_cost(), 0);
    }

    #[test]
    fn byte_reconciliation_is_permutation_invariant() {
        fn run(order: [[u8; 32]; 3]) -> (i64, u64, Vec<BillableTokenEvent>) {
            let budget = RuntimeBudget::new(Cost::create(23, "byte permutation"));
            let _scope = budget.enter_comm_accounting_scope();
            for identity in order {
                budget
                    .reserve_produce_introduction_identity(identity, &test_authority(), 5, false)
                    .unwrap();
            }
            (
                budget.total_cost().value,
                budget.quantitative_byte_cost(),
                budget.get_canonical_event_log(),
            )
        }

        let first = run([[3; 32], [1; 32], [2; 32]]);
        let second = run([[2; 32], [3; 32], [1; 32]]);
        assert_eq!(first, second);
        assert_eq!(first.0, 15);
        assert_eq!(first.1, 15);
    }

    #[test]
    fn persistent_introduction_retries_are_charged_once() {
        let budget = RuntimeBudget::new(Cost::create(64, "persistent introductions"));
        let _scope = budget.enter_comm_accounting_scope();

        for _ in 0..16 {
            budget
                .reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true)
                .unwrap();
            budget
                .reserve_consume_introduction_identity([2; 32], &test_authority(), 7, true)
                .unwrap();
        }
        budget
            .reserve_produce_introduction_identity([3; 32], &test_authority(), 5, false)
            .unwrap();
        budget
            .reserve_produce_introduction_identity([3; 32], &test_authority(), 5, false)
            .unwrap();

        assert_eq!(budget.total_cost().value, 22);
        assert_eq!(budget.quantitative_byte_cost(), 22);
        assert_eq!(budget.authority_byte_events().len(), 4);
        assert_eq!(budget.cost_trace_event_count(), 4);
    }

    #[test]
    fn persistent_introduction_identity_is_reset_between_deploys() {
        let budget = RuntimeBudget::new(Cost::create(10, "first deploy"));
        let _scope = budget.enter_comm_accounting_scope();
        budget
            .reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true)
            .unwrap();
        assert_eq!(budget.total_cost().value, 5);

        budget.set(Cost::create(10, "second deploy"));
        budget
            .reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true)
            .unwrap();
        assert_eq!(budget.total_cost().value, 5);
    }

    #[test]
    fn unmetered_or_out_of_scope_introduction_does_not_mark_identity_paid() {
        let budget = RuntimeBudget::new(Cost::create(10, "metering boundary"));
        budget
            .reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true)
            .unwrap();

        let _scope = budget.enter_comm_accounting_scope();
        {
            let _unmetered = budget.enter_unmetered_scope();
            budget
                .reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true)
                .unwrap();
        }
        assert_eq!(budget.total_cost().value, 0);
        assert!(budget
            .persistent_introductions
            .lock()
            .expect("persistent introduction set")
            .is_empty());

        budget
            .reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true)
            .unwrap();
        budget
            .reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true)
            .unwrap();
        assert_eq!(budget.total_cost().value, 5);
        assert_eq!(budget.quantitative_byte_cost(), 5);
    }

    #[test]
    fn rejected_persistent_introduction_does_not_mark_identity_paid() {
        let budget = RuntimeBudget::new(Cost::create(4, "rejected introduction"));
        let _scope = budget.enter_comm_accounting_scope();
        assert_eq!(
            budget.reserve_produce_introduction_identity([1; 32], &test_authority(), 5, true),
            Err(InterpreterError::OutOfPhlogistonsError)
        );
        assert!(budget
            .persistent_introductions
            .lock()
            .expect("persistent introduction set")
            .is_empty());
    }

    #[test]
    fn concurrent_persistent_retries_commit_one_introduction() {
        let budget = RuntimeBudget::new(Cost::create(64, "concurrent persistent introduction"));
        let _scope = budget.enter_comm_accounting_scope();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let workers = (0..16)
            .map(|_| {
                let budget = budget.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    budget
                        .reserve_consume_introduction_identity([7; 32], &test_authority(), 9, true)
                        .unwrap();
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }

        assert_eq!(budget.total_cost().value, 9);
        assert_eq!(budget.quantitative_byte_cost(), 9);
        assert_eq!(budget.authority_byte_events().len(), 1);
        assert_eq!(budget.cost_trace_event_count(), 1);
    }

    proptest::proptest! {
        #[test]
        fn stack_transfer_reserves_exactly_one_authority_cell_per_output(
            cells in 1usize..65,
            slack in 0u64..65,
        ) {
            use models::rhoapi::cost_signature::Value;
            use models::rhoapi::{CostAuthority, CostSignature};

            let signature = CostSignature {
                value: Some(Value::Ground(b"payer".to_vec())),
            };
            let authority = CostAuthority {
                regions: vec![authority::cost_region(&signature, b"stack", 0).unwrap()],
            };
            let lane = authority::cost_signature_to_sig(&signature)
                .unwrap()
                .lane_hash();
            let cell_count = u64::try_from(cells).unwrap();
            let budget = RuntimeBudget::new(Cost::create(0, "stack transfer"));
            let _scope = budget.enter_comm_accounting_scope();
            budget.install_authority_allocation(authority::ResourceMultiset::singleton(
                lane,
                cell_count + slack,
            ));
            budget
                .prepare_authority_stack_transfer([1; 32], vec![signature; cells], &authority)
                .unwrap()
                .commit();

            proptest::prop_assert_eq!(budget.authority_realized().get(&lane), cell_count);
            proptest::prop_assert_eq!(budget.authority_events().len(), cells);
            proptest::prop_assert_eq!(budget.authority_stack_births().len(), 1);
            proptest::prop_assert_eq!(budget.total_cost().value, 0);
        }

        #[test]
        fn aborted_stack_transfer_restores_the_exact_physical_capacity(
            cells in 1usize..65,
            slack in 0u64..65,
        ) {
            use models::rhoapi::cost_signature::Value;
            use models::rhoapi::{CostAuthority, CostSignature};

            let signature = CostSignature {
                value: Some(Value::Ground(b"payer".to_vec())),
            };
            let authority = CostAuthority {
                regions: vec![authority::cost_region(&signature, b"stack", 0).unwrap()],
            };
            let lane = authority::cost_signature_to_sig(&signature)
                .unwrap()
                .lane_hash();
            let cell_count = u64::try_from(cells).unwrap();
            let allocation = cell_count + slack;
            let budget = RuntimeBudget::new(Cost::create(0, "stack transfer abort property"));
            let _scope = budget.enter_comm_accounting_scope();
            budget.install_authority_allocation(authority::ResourceMultiset::singleton(
                lane,
                allocation,
            ));

            let reservation = budget
                .prepare_authority_stack_transfer(
                    [1; 32],
                    vec![signature.clone(); cells],
                    &authority,
                )
                .unwrap();
            drop(reservation);
            proptest::prop_assert!(budget.authority_events().is_empty());
            proptest::prop_assert!(budget.authority_realized().0.is_empty());
            proptest::prop_assert!(budget.authority_stack_births().is_empty());

            budget
                .prepare_authority_stack_transfer(
                    [2; 32],
                    vec![signature; allocation as usize],
                    &authority,
                )
                .unwrap()
                .commit();
            proptest::prop_assert_eq!(budget.authority_realized().get(&lane), allocation);
            proptest::prop_assert_eq!(budget.authority_events().len(), allocation as usize);
        }

        #[test]
        fn failed_deploy_rolls_back_every_stack_transfer_but_preserves_committed_work(
            cells in 1usize..65,
            slack in 0u64..65,
        ) {
            use models::rhoapi::cost_signature::Value;
            use models::rhoapi::{CostAuthority, CostSignature};

            let signature = CostSignature {
                value: Some(Value::Ground(b"payer".to_vec())),
            };
            let authority = CostAuthority {
                regions: vec![authority::cost_region(&signature, b"stack", 0).unwrap()],
            };
            let lane = authority::cost_signature_to_sig(&signature)
                .unwrap()
                .lane_hash();
            let cell_count = u64::try_from(cells).unwrap();
            let allocation = cell_count + slack + 1;
            let budget = RuntimeBudget::new(Cost::create(64, "deploy rollback property"));
            let _scope = budget.enter_comm_accounting_scope();
            budget.install_authority_allocation(authority::ResourceMultiset::singleton(
                lane,
                allocation,
            ));
            budget
                .prepare_authority_stack_transfer(
                    [1; 32],
                    vec![signature.clone(); cells],
                    &authority,
                )
                .unwrap()
                .commit();
            budget
                .reserve_produce_introduction_identity([2; 32], &authority, 5, false)
                .unwrap();
            budget
                .reserve_comm_authority_identity_with_byte_cost([3; 32], &authority, 3)
                .unwrap();

            budget.rollback_authority_stack_transfers().unwrap();
            proptest::prop_assert_eq!(budget.authority_events().len(), 1);
            proptest::prop_assert_eq!(budget.authority_events()[0].event_id, [3; 32]);
            proptest::prop_assert_eq!(budget.authority_realized().get(&lane), 1);
            proptest::prop_assert!(budget.authority_stack_births().is_empty());
            proptest::prop_assert_eq!(budget.quantitative_byte_cost(), 8);

            budget
                .prepare_authority_stack_transfer(
                    [4; 32],
                    vec![signature; (cell_count + slack) as usize],
                    &authority,
                )
                .unwrap()
                .commit();
            proptest::prop_assert_eq!(budget.authority_realized().get(&lane), allocation);
        }

        #[test]
        fn introduction_authority_registration_is_stable_conflict_safe_and_reset_scoped(
            payer_bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 1..65),
            identity in proptest::array::uniform32(proptest::prelude::any::<u8>()),
            consume_kind in proptest::prelude::any::<bool>(),
        ) {
            use models::rhoapi::cost_signature::Value;
            use models::rhoapi::{CostAuthority, CostSignature};

            let payer = Sig::Ground(payer_bytes.clone());
            let budget = RuntimeBudget::new(Cost::create(1_000, "introduction registry property"));
            budget.set_deploy_signature_funded(b"first introduction deploy", payer.clone());
            let _scope = budget.enter_comm_accounting_scope();
            let kind = if consume_kind {
                authority::AuthorityByteEventKind::ConsumeIntroduction
            } else {
                authority::AuthorityByteEventKind::ProduceIntroduction
            };

            let first = budget.introduction_authority(identity, kind).unwrap();
            budget
                .register_introduction_authority(identity, kind, &CostAuthority::default())
                .unwrap();
            proptest::prop_assert_eq!(budget.introduction_authority(identity, kind).unwrap(), first.clone());
            proptest::prop_assert_eq!(
                authority::authority_demand(&first).unwrap(),
                authority::ResourceMultiset::singleton(payer.lane_hash(), 1)
            );

            let mut conflicting_bytes = payer_bytes;
            conflicting_bytes.push(0xff);
            let conflicting_signature = CostSignature {
                value: Some(Value::Ground(conflicting_bytes.clone())),
            };
            let conflicting_authority = CostAuthority {
                regions: vec![authority::cost_region(&conflicting_signature, &identity, u32::from(kind.tag())).unwrap()],
            };
            proptest::prop_assert_eq!(
                budget.register_introduction_authority(identity, kind, &conflicting_authority),
                Err(InterpreterError::ReduceError(
                    authority::AuthorityError::EventIdentityConflict.to_string()
                ))
            );

            let replacement = Sig::Ground(conflicting_bytes);
            budget.reset_from_token(&Token::coalesced(replacement.clone(), 1_000));
            let after_reset = budget.introduction_authority(identity, kind).unwrap();
            proptest::prop_assert_eq!(
                authority::authority_demand(&after_reset).unwrap(),
                authority::ResourceMultiset::singleton(replacement.lane_hash(), 1)
            );
        }
    }

    #[test]
    fn underfunded_stack_transfer_reservation_is_atomic() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"stack", 0).unwrap()],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let budget = RuntimeBudget::new(Cost::create(0, "stack transfer"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(lane, 2));

        assert!(matches!(
            budget.prepare_authority_stack_transfer(
                [1; 32],
                vec![signature.clone(), signature.clone(), signature],
                &authority,
            ),
            Err(InterpreterError::OutOfPhlogistonsError)
        ));
        assert!(budget.authority_realized().0.is_empty());
        assert!(budget.authority_events().is_empty());
        assert!(budget.authority_stack_births().is_empty());
        assert_eq!(budget.total_cost().value, 0);
    }

    #[test]
    fn deploy_rollback_removes_stack_custody_but_keeps_attempt_charges() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"stack", 0).unwrap()],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let budget = RuntimeBudget::new(Cost::create(64, "deploy rollback"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(lane, 3));
        budget
            .prepare_authority_stack_transfer(
                [1; 32],
                vec![signature.clone(), signature.clone()],
                &authority,
            )
            .unwrap()
            .commit();
        budget
            .reserve_produce_introduction_identity([2; 32], &authority, 5, false)
            .unwrap();
        budget
            .reserve_comm_authority_identity_with_byte_cost([3; 32], &authority, 3)
            .unwrap();

        budget.rollback_authority_stack_transfers().unwrap();
        assert_eq!(budget.authority_events().len(), 1);
        assert_eq!(budget.authority_events()[0].event_id, [3; 32]);
        assert_eq!(budget.authority_realized().get(&lane), 1);
        assert!(budget.authority_stack_births().is_empty());
        assert_eq!(budget.quantitative_byte_cost(), 8);

        budget
            .prepare_authority_stack_transfer(
                [4; 32],
                vec![signature.clone(), signature],
                &authority,
            )
            .unwrap()
            .commit();
        assert_eq!(budget.authority_realized().get(&lane), 3);
    }

    #[test]
    fn repeated_stack_transfer_identity_is_rejected_without_an_extra_debit() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"stack", 0).unwrap()],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let budget = RuntimeBudget::new(Cost::create(0, "stack transfer"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(lane, 2));

        budget
            .prepare_authority_stack_transfer([1; 32], vec![signature.clone()], &authority)
            .unwrap()
            .commit();
        assert!(matches!(
            budget.prepare_authority_stack_transfer([1; 32], vec![signature], &authority),
            Err(InterpreterError::ReduceError(message))
                if message == authority::AuthorityError::EventIdentityConflict.to_string()
        ));
        assert_eq!(budget.authority_realized().get(&lane), 1);
        assert_eq!(budget.authority_events().len(), 1);
        assert_eq!(budget.authority_stack_births().len(), 1);
    }

    #[test]
    fn aborted_stack_transfer_restores_capacity_and_committed_witnesses() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"stack", 0).unwrap()],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let budget = RuntimeBudget::new(Cost::create(1, "stack transfer abort"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::singleton(lane, 2));

        let reservation = budget
            .prepare_authority_stack_transfer(
                [1; 32],
                vec![signature.clone(), signature],
                &authority,
            )
            .unwrap();
        assert!(budget.authority_events().is_empty());
        assert!(budget.authority_realized().0.is_empty());
        assert!(budget.authority_stack_births().is_empty());
        drop(reservation);

        budget
            .reserve_comm_authority_identity([2; 32], &authority)
            .unwrap();
        assert_eq!(budget.authority_realized().get(&lane), 1);
        assert_eq!(budget.authority_events().len(), 1);
        assert!(budget.authority_stack_births().is_empty());
    }

    #[test]
    fn duplicate_comm_identity_requires_identical_authority() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let first = CostSignature {
            value: Some(Value::Ground(b"first".to_vec())),
        };
        let second = CostSignature {
            value: Some(Value::Ground(b"second".to_vec())),
        };
        let first = CostAuthority {
            regions: vec![authority::cost_region(&first, b"wrapper", 0).unwrap()],
        };
        let second = CostAuthority {
            regions: vec![authority::cost_region(&second, b"wrapper", 0).unwrap()],
        };
        let budget = RuntimeBudget::new(Cost::create(2, "two comms"));
        let _scope = budget.enter_comm_accounting_scope();

        budget
            .reserve_comm_authority_identity([3; 32], &first)
            .unwrap();
        assert!(matches!(
            budget.reserve_comm_authority_identity([3; 32], &second),
            Err(InterpreterError::ReduceError(message))
                if message == authority::AuthorityError::EventIdentityConflict.to_string()
        ));
    }

    #[test]
    fn exhausted_comm_exposes_authority_without_committing_a_debit() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"frontier".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"frontier", 0).unwrap()],
        };
        let budget = RuntimeBudget::new(Cost::create(0, "frontier"));
        let _scope = budget.enter_comm_accounting_scope();

        assert_eq!(
            budget.reserve_comm_authority_identity([4; 32], &authority),
            Err(InterpreterError::OutOfPhlogistonsError)
        );
        assert_eq!(budget.authority_frontier(), vec![authority]);
        assert!(budget.authority_events().is_empty());
        assert!(budget.authority_realized().0.is_empty());
    }

    #[test]
    fn allocation_exhaustion_exposes_authority_without_committing_a_debit() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"frontier".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"frontier", 0).unwrap()],
        };
        let budget = RuntimeBudget::new(Cost::create(1, "frontier"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::default());

        assert_eq!(
            budget.reserve_comm_authority_identity([5; 32], &authority),
            Err(InterpreterError::OutOfPhlogistonsError)
        );
        assert_eq!(budget.authority_frontier(), vec![authority]);
        assert!(budget.authority_events().is_empty());
        assert!(budget.authority_realized().0.is_empty());
        assert_eq!(budget.total_cost().value, 0);
    }

    #[test]
    fn unit_authority_records_the_comm_without_consuming_capacity() {
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(models::rhoapi::cost_signature::Value::Unit(true)),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"unit", 0).unwrap()],
        };
        let budget = RuntimeBudget::new(Cost::create(0, "unit authority"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(authority::ResourceMultiset::default());

        budget
            .reserve_comm_authority_identity([6; 32], &authority)
            .unwrap();

        assert_eq!(budget.total_cost().value, 0);
        assert!(budget.authority_realized().0.is_empty());
        let events = budget.authority_events();
        assert_eq!(events.len(), 1);
        assert!(events[0].debit.0.is_empty());
    }

    #[test]
    fn installing_an_allocation_starts_a_fresh_region_ledger() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::{CostAuthority, CostSignature};

        let signature = CostSignature {
            value: Some(Value::Ground(b"payer".to_vec())),
        };
        let authority = CostAuthority {
            regions: vec![authority::cost_region(&signature, b"wrapper", 0).unwrap()],
        };
        let lane = authority::cost_signature_to_sig(&signature)
            .unwrap()
            .lane_hash();
        let allocation = authority::ResourceMultiset::singleton(lane, 1);
        let budget = RuntimeBudget::new(Cost::create(2, "two executions"));
        let _scope = budget.enter_comm_accounting_scope();

        budget.install_authority_allocation(allocation.clone());
        budget
            .reserve_comm_authority_identity([4; 32], &authority)
            .unwrap();
        assert_eq!(budget.authority_realized().get(&lane), 1);

        budget.install_authority_allocation(allocation);
        budget
            .reserve_comm_authority_identity([5; 32], &authority)
            .unwrap();
        assert_eq!(budget.authority_realized().get(&lane), 1);
    }

    #[test]
    fn overlapping_comm_accounting_scopes_remain_active_until_the_last_exit() {
        let budget = RuntimeBudget::new(Cost::create(1, "scope"));
        let outer = budget.enter_comm_accounting_scope();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (exit_tx, exit_rx) = std::sync::mpsc::channel();
        let worker_budget = budget.clone();
        let worker = std::thread::spawn(move || {
            let _overlap = worker_budget.enter_comm_accounting_scope();
            entered_tx.send(()).unwrap();
            exit_rx.recv().unwrap();
        });

        entered_rx.recv().unwrap();
        drop(outer);
        assert!(budget.has_comm_accounting_scope());
        exit_tx.send(()).unwrap();
        worker.join().unwrap();
        assert!(!budget.has_comm_accounting_scope());
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

#[cfg(test)]
mod funding_sig_tests {
    //! WD-D2 §D2.9 — the FUNDING-`Sig` derivation that keys a deploy's supply
    //! pool by the signer's GROUND PUBLIC KEY (`Σ⟦signer⟧ == Σ⟦wallet⟧`),
    //! DECOUPLED from the wire-signature `deploy_id`. These pin (a) the shape
    //! (single ⇒ `Sig::Ground(pk)`; n-signer ⇒ left-associated `Sig::And` of
    //! `Ground` atoms) and (b) the decoupling invariant: `set_deploy_signature(s)_funded`
    //! installs the funding `Sig` while leaving the `deploy_id` byte-identical to
    //! the legacy wire-sig-derived install.
    use super::*;

    /// Single signer ⇒ `Sig::Ground(pk)` over the raw public-key bytes (the
    /// spec's ground identity atom `g`), NOT a wire-sig `Sig::Quote`.
    #[test]
    fn funding_sig_single_is_ground() {
        let pk = b"signer-ed25519-public-key-bytes";
        assert_eq!(funding_sig_single(pk), Sig::Ground(pk.to_vec()));
    }

    /// Two signers ⇒ left-associated `Sig::And(Ground(pk0), Ground(pk1))` — the
    /// compound `g₁∘g₂` over the cosigners' public keys (P8-balanced wallets).
    #[test]
    fn funding_sig_compound_is_left_assoc_and_of_ground() {
        let pk0: &[u8] = b"cosigner-pubkey-zero";
        let pk1: &[u8] = b"cosigner-pubkey-one";
        let expected = Sig::And(
            Box::new(Sig::Ground(pk0.to_vec())),
            Box::new(Sig::Ground(pk1.to_vec())),
        );
        assert_eq!(funding_sig_compound(&[pk0, pk1]), expected);
    }

    /// Three signers ⇒ left-associated nesting of `Ground` atoms.
    #[test]
    fn funding_sig_three_pubkeys_left_assoc_nested() {
        let pk0: &[u8] = b"pk-a";
        let pk1: &[u8] = b"pk-b";
        let pk2: &[u8] = b"pk-c";
        let expected = Sig::And(
            Box::new(Sig::And(
                Box::new(Sig::Ground(pk0.to_vec())),
                Box::new(Sig::Ground(pk1.to_vec())),
            )),
            Box::new(Sig::Ground(pk2.to_vec())),
        );
        assert_eq!(funding_sig_compound(&[pk0, pk1, pk2]), expected);
    }

    /// THE DECOUPLING (§D2.9): `set_deploy_signature_funded` leaves the
    /// `deploy_id` byte-identical to the legacy wire-sig-derived install while
    /// installing the GROUND-pubkey funding `Sig`. So a deploy's on-chain
    /// identity never moves, but the funded pool becomes `Σ⟦Ground(pk)⟧`.
    #[test]
    fn set_deploy_signature_funded_preserves_deploy_id_and_installs_ground() {
        let wire = b"on-chain-deploy-signature";
        let pk = b"signer-ground-public-key";

        let legacy = RuntimeBudget::new(Cost::create(100, "legacy"));
        legacy.set_deploy_signature(wire);

        let funded = RuntimeBudget::new(Cost::create(100, "funded"));
        funded.set_deploy_signature_funded(wire, funding_sig_single(pk));

        assert_eq!(
            funded.deploy_id(),
            legacy.deploy_id(),
            "deploy_id stays wire-sig-derived (byte-identical) under the funding decoupling"
        );
        assert_eq!(
            funded.signature(),
            Sig::Ground(pk.to_vec()),
            "the installed funding signature is the signer's Ground(pk) wallet key"
        );
        assert_ne!(
            funded.signature(),
            legacy.signature(),
            "funding moved off the wire-sig Quote pool onto the Ground(pk) wallet"
        );
    }

    /// The compound decoupling: `set_deploy_signatures_funded` keeps the
    /// compound wire-sig `deploy_id` while installing the `And`-fold of the
    /// cosigners' `Ground(pk)` atoms.
    #[test]
    fn set_deploy_signatures_funded_preserves_deploy_id_and_installs_ground_fold() {
        let w0: &[u8] = b"cosigner-wire-aaaa";
        let w1: &[u8] = b"cosigner-wire-bbbb";
        let pk0: &[u8] = b"ground-pk-aaaa";
        let pk1: &[u8] = b"ground-pk-bbbb";

        let legacy = RuntimeBudget::new(Cost::create(100, "legacy"));
        legacy.set_deploy_signatures(&[w0, w1]);

        let funded = RuntimeBudget::new(Cost::create(100, "funded"));
        funded.set_deploy_signatures_funded(&[w0, w1], funding_sig_compound(&[pk0, pk1]));

        assert_eq!(
            funded.deploy_id(),
            legacy.deploy_id(),
            "compound deploy_id stays wire-sig-derived (byte-identical)"
        );
        assert_eq!(
            funded.signature(),
            Sig::And(
                Box::new(Sig::Ground(pk0.to_vec())),
                Box::new(Sig::Ground(pk1.to_vec())),
            ),
            "the installed funding signature is And(Ground(pkᵢ)) over the cosigners' keys"
        );
    }
}
