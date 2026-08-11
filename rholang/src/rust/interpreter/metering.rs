use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use models::rhoapi::Par;

use super::accounting::costs::Cost;
use super::accounting::delta_sigma::match_channel_to_lane;
use super::accounting::{
    BillableKind, BillableTokenEvent, CostReservationBatch, RedexId, RuntimeBudget, SourcePath,
};
use super::errors::InterpreterError;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContinuationKey {
    pub deploy_id: [u8; 32],
    pub source_path: SourcePath,
    pub redex_id: RedexId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeteredFrame {
    Billable(BillableTokenEvent),
    BillableCost {
        event: BillableTokenEvent,
        amount: Cost,
    },
    InstallGate(ContinuationKey),
    FireGate(ContinuationKey),
    Resume(ContinuationKey),
}

#[derive(Clone)]
pub struct MeteredMachine {
    budget: RuntimeBudget,
    pending: Arc<Mutex<VecDeque<MeteredFrame>>>,
    source_path: SourcePath,
    next_local_index: Arc<AtomicU64>,
    /// Deploy id cached by value (`[u8;32]`, constant during a deploy — the
    /// signature is installed before evaluation begins). Read once per fork in
    /// [`MeteredMachine::child`] so the per-charge hot path never locks the
    /// budget's `deploy_id` Mutex.
    deploy_id: [u8; 32],
    /// Per-signature lane key (`Sig::lane_hash`) of the deploy's signature,
    /// cached by value alongside `deploy_id`. Like `deploy_id` it is constant
    /// during a deploy (the signature is installed before evaluation begins),
    /// so it is computed once per fork (in [`MeteredMachine::new`] /
    /// [`MeteredMachine::child`]) and stamped onto every emitted
    /// [`BillableTokenEvent`] without locking the budget's signature Mutex on
    /// the per-charge hot path. In D-scope a deploy carries exactly ONE
    /// compound lane (Def 7.4 — no per-component split; intra-deploy multi-σ
    /// is a later funding-slots stage), so this value is identical across all
    /// of a deploy's events and the N=1 scalar fast path is unaffected.
    sig_hash: [u8; 32],
}

impl MeteredMachine {
    pub fn new(budget: RuntimeBudget) -> Self {
        let deploy_id = budget.deploy_id();
        let sig_hash = budget.signature().lane_hash();
        Self {
            budget,
            pending: Arc::new(Mutex::new(VecDeque::new())),
            source_path: SourcePath(Vec::new()),
            next_local_index: Arc::new(AtomicU64::new(0)),
            deploy_id,
            sig_hash,
        }
    }

    pub fn budget(&self) -> RuntimeBudget { self.budget.clone() }

    pub fn child(&self, component: u32) -> Self {
        let mut source_path = self.source_path.0.clone();
        source_path.push(component);
        Self {
            budget: self.budget.clone(),
            pending: self.pending.clone(),
            source_path: SourcePath(source_path),
            next_local_index: Arc::new(AtomicU64::new(0)),
            // Re-read the deploy id once per fork (constant during eval), so
            // the per-charge path uses the cached value instead of a Mutex.
            deploy_id: self.budget.deploy_id(),
            // The signature (hence its lane key) is likewise constant during
            // eval; inheriting the parent's cached value avoids recomputing the
            // `from_sig` channel digest per fork.
            sig_hash: self.sig_hash,
        }
    }

    /// Charge a token-consuming COMM reduction (send / receive). D3 (DR-9,
    /// OD-3): this is THE consensus cost unit — `reconcile_lane` counts each
    /// committed `Comm` as 1. `amount` is the per-op diagnostic weight (still
    /// recorded in the event log/digest), but only the COMM COUNT gates
    /// consensus.
    pub fn reserve_comm(&self, amount: Cost) -> Result<(), InterpreterError> {
        self.reserve_cost(BillableKind::Comm, amount)
    }

    /// W1 Phase 3 — per-redex located-stack attribution. AFTER a COMM's channel is
    /// resolved, match it against the installed signer channels: a COMM on a
    /// NON-envelope signer channel (`Σ⟦sᵢ⟧`) is tallied to that signer's lane for
    /// the diagnostic per-lane projection ([`RuntimeBudget::per_lane_demand`]). The
    /// COMM itself was ALREADY charged (scalar, to the envelope) by `reserve_comm`,
    /// so this records only the per-lane VIEW — it never re-charges and never
    /// touches the consensus reconciliation (`reconcile`/`total_cost`/the supply
    /// pools). Cheap on the single-signer fast path: the `any_signed_regions` gate
    /// short-circuits before any channel encode / snapshot.
    ///
    /// Under the s₀ collapse every COMM is on a DATA channel (never a `Σ⟦s⟧` supply
    /// channel — the §5 no-alias audit), so the match never fires and the
    /// projection stays the singleton envelope lane. The shared decision is
    /// [`match_channel_to_lane`] — the SAME one the static dual
    /// [`demand_by_sig`](super::accounting::delta_sigma::demand_by_sig) uses, so the
    /// two cannot drift (the consensus bridge).
    pub fn note_channel_lane(&self, channel: &Par) {
        if !self.budget.any_signed_regions() {
            return;
        }
        let signer_channels = self.budget.signer_channels_snapshot();
        if let Some(lane) = match_channel_to_lane(channel, &signer_channels) {
            self.budget.note_lane_comm(lane);
        }
    }

    /// Charge a non-COMM structural reduction (new / match / if). D3 (DR-9,
    /// OD-3): DIAGNOSTIC only — it is metered for fidelity (event log/digest)
    /// but contributes ZERO to the consensus consumed cost.
    pub fn reserve_reduction(&self, amount: Cost) -> Result<(), InterpreterError> {
        self.reserve_cost(BillableKind::Reduction, amount)
    }

    pub fn reserve_primitive(&self, amount: Cost) -> Result<(), InterpreterError> {
        let operation = amount.operation.to_string();
        self.reserve_cost(BillableKind::Primitive(operation), amount)
    }

    pub fn reserve_incremental_primitive(&self, amount: Cost) -> Result<(), InterpreterError> {
        if amount.value < 0 {
            return Err(InterpreterError::BugFoundError(format!(
                "Incremental billable primitive cost must be non-negative for {}",
                amount.operation
            )));
        }

        if amount.value == 0 {
            return Ok(());
        }

        self.reserve_primitive(amount)
    }

    pub fn reserve_substitution(&self, amount: Cost) -> Result<(), InterpreterError> {
        self.reserve_cost(BillableKind::Substitution, amount)
    }

    fn reserve_cost(&self, kind: BillableKind, amount: Cost) -> Result<(), InterpreterError> {
        // Live (production) charging path. Each charge records exactly one
        // attempt, lock-free, into the budget's `SegQueue` via
        // `reserve_canonical_with_cost` (→ `attempt_one`), then consults the
        // liveness gate. There is no shared Mutex on the hot path — concurrent
        // forks never contend on a frame queue. The consensus `total_cost` is
        // derived at finalization from the recorded attempt multiset by the
        // canonical reconciliation, independent of which fork recorded when.
        // (The `MeteredFrame`/`pending` queue below is retained only as test
        // and gate-protocol infrastructure; the production charge path bypasses
        // it entirely.)
        if amount.value <= 0 {
            return Err(InterpreterError::BugFoundError(format!(
                "Billable metering cost must be positive for {}",
                amount.operation
            )));
        }

        let local_index = self.next_local_index.fetch_add(1, Ordering::AcqRel);
        let source_path = self.event_source_path(local_index);
        let event = BillableTokenEvent {
            deploy_id: self.deploy_id,
            sig_hash: self.sig_hash,
            source_path,
            redex_id: RedexId(local_index),
            local_index,
            kind,
            weight: amount.value as u64,
        };

        self.budget.reserve_canonical_with_cost(event, amount)
    }

    pub fn enqueue_billable(
        &self,
        source_path: SourcePath,
        kind: BillableKind,
        weight: u64,
    ) -> ContinuationKey {
        let local_index = self.next_local_index.fetch_add(1, Ordering::AcqRel);
        let event = BillableTokenEvent {
            deploy_id: self.deploy_id,
            sig_hash: self.sig_hash,
            source_path: source_path.clone(),
            redex_id: RedexId(local_index),
            local_index,
            kind,
            weight,
        };
        let key = ContinuationKey {
            deploy_id: event.deploy_id,
            source_path,
            redex_id: event.redex_id.clone(),
        };
        self.pending
            .lock()
            .expect("metered frame queue")
            .push_back(MeteredFrame::Billable(event));
        key
    }

    fn event_source_path(&self, local_index: u64) -> SourcePath {
        let mut path = self.source_path.0.clone();
        path.push(local_index.min(u32::MAX as u64) as u32);
        SourcePath(path)
    }

    pub fn enqueue_frame(&self, frame: MeteredFrame) {
        self.pending
            .lock()
            .expect("metered frame queue")
            .push_back(frame);
    }

    pub fn drain_canonical(&self) -> Result<(), InterpreterError> {
        // Drain outside the mutex so token reservation never holds the queue
        // lock across a potentially contended budget CAS loop.
        let frames = {
            let mut pending = self.pending.lock().expect("metered frame queue");
            pending.drain(..).collect::<Vec<_>>()
        };
        self.drain_frames(frames)
    }

    fn drain_frames(&self, mut frames: Vec<MeteredFrame>) -> Result<(), InterpreterError> {
        frames.sort_by(|left, right| frame_order_key(left).cmp(&frame_order_key(right)));

        let mut billable = Vec::new();
        let mut nonbillable = Vec::new();
        for frame in frames {
            match frame {
                MeteredFrame::Billable(event) => billable.push((event, None)),
                MeteredFrame::BillableCost { event, amount } => {
                    billable.push((event, Some(amount)))
                }
                frame => nonbillable.push(frame),
            }
        }

        billable.sort_by(|(left, _), (right, _)| left.cmp(right));
        // Record the batch lock-free; `commit_canonical_batch` routes each
        // event through `attempt_one`, which pushes into the budget's SegQueue.
        // (Frame-path callers are test/gate-protocol infrastructure; the live
        // production charge path bypasses this via `reserve_canonical_with_cost`.)
        let commit = self.budget.commit_canonical_batch(CostReservationBatch {
            events: billable
                .iter()
                .map(|(event, _)| event.clone())
                .collect::<Vec<_>>(),
        })?;
        if commit.oop.is_some() {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        for frame in nonbillable {
            match frame {
                MeteredFrame::InstallGate(key) => self.enqueue_frame(MeteredFrame::FireGate(key)),
                MeteredFrame::FireGate(key) => self.enqueue_frame(MeteredFrame::Resume(key)),
                MeteredFrame::Resume(_) => {}
                MeteredFrame::Billable(_) | MeteredFrame::BillableCost { .. } => {}
            }
        }

        Ok(())
    }
}

fn frame_order_key(frame: &MeteredFrame) -> (u8, Option<&SourcePath>, Option<&RedexId>, u64) {
    match frame {
        MeteredFrame::Billable(event) => (
            0,
            Some(&event.source_path),
            Some(&event.redex_id),
            event.local_index,
        ),
        MeteredFrame::BillableCost { event, .. } => (
            0,
            Some(&event.source_path),
            Some(&event.redex_id),
            event.local_index,
        ),
        MeteredFrame::InstallGate(key) => (1, Some(&key.source_path), Some(&key.redex_id), 0),
        MeteredFrame::FireGate(key) => (2, Some(&key.source_path), Some(&key.redex_id), 0),
        MeteredFrame::Resume(key) => (3, Some(&key.source_path), Some(&key.redex_id), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drains_billable_frames_in_canonical_source_order() {
        let budget = RuntimeBudget::new(Cost::create(10, "test"));
        let machine = MeteredMachine::new(budget.clone());

        machine.enqueue_billable(SourcePath(vec![2]), BillableKind::Comm, 1);
        machine.enqueue_billable(SourcePath(vec![1]), BillableKind::Substitution, 2);
        machine.drain_canonical().unwrap();

        let event_log = budget.get_canonical_event_log();
        assert_eq!(event_log.len(), 2);
        assert_eq!(event_log[0].source_path, SourcePath(vec![1]));
        assert_eq!(event_log[1].source_path, SourcePath(vec![2]));
        // D3 (DR-9, OD-3): consensus cost is the COMM count. One COMM + one
        // (diagnostic) Substitution ⇒ total_cost = 1 (the Substitution is 0).
        assert_eq!(budget.total_cost().value, 1);
    }

    #[test]
    fn live_reserve_records_lock_free_without_frame_queue() {
        let budget = RuntimeBudget::new(Cost::create(1, "test"));
        let machine = MeteredMachine::new(budget.clone());

        // D3 (DR-9, OD-3): a single COMM costs ONE token (its `Cost` weight is
        // diagnostic), so it fits the 1-token budget and commits.
        machine
            .child(0)
            .reserve_comm(Cost::create(11, "send eval"))
            .ok();
        assert_eq!(budget.total_cost().value, 1);
    }

    #[test]
    fn zero_incremental_primitive_work_is_non_billable() {
        let budget = RuntimeBudget::new(Cost::create(1, "test"));
        let machine = MeteredMachine::new(budget.clone());

        machine
            .reserve_incremental_primitive(Cost::create(0, "empty append"))
            .unwrap();

        assert!(budget.get_event_log().is_empty());
        assert_eq!(budget.total_cost().value, 0);
        assert_eq!(budget.cost_trace_event_count(), 0);
    }

    #[test]
    fn negative_incremental_primitive_work_is_rejected() {
        let budget = RuntimeBudget::new(Cost::create(1, "test"));
        let machine = MeteredMachine::new(budget);

        let err = machine
            .reserve_incremental_primitive(Cost::create(-1, "invalid incremental work"))
            .unwrap_err();

        assert!(matches!(err, InterpreterError::BugFoundError(_)));
    }

    #[test]
    fn child_machines_use_stable_source_paths() {
        let budget = RuntimeBudget::new(Cost::create(10, "test"));
        let machine = MeteredMachine::new(budget.clone());

        machine
            .child(1)
            .reserve_comm(Cost::create(11, "right branch"))
            .unwrap();
        machine
            .child(0)
            .reserve_comm(Cost::create(11, "left branch"))
            .unwrap();

        let canonical = budget.get_canonical_event_log();
        assert_eq!(canonical[0].source_path, SourcePath(vec![0, 0]));
        assert_eq!(canonical[1].source_path, SourcePath(vec![1, 0]));
    }

    #[test]
    fn canonical_drain_records_stable_oop_descriptor() {
        // D3 (DR-9, OD-3): the OOP boundary is per-COMM. A 1-COMM budget admits
        // one COMM; the second COMM is the OOP boundary. A diagnostic
        // Substitution (cost 0) interleaved between them commits for free and
        // never triggers OOP. Canonical order puts source_path [1] before [2],
        // so the lower-ranked COMM commits and the higher-ranked COMM OOPs.
        let budget = RuntimeBudget::new(Cost::create(1, "test"));
        let machine = MeteredMachine::new(budget.clone());

        machine.enqueue_billable(SourcePath(vec![2]), BillableKind::Comm, 11);
        machine.enqueue_billable(SourcePath(vec![3]), BillableKind::Substitution, 3);
        machine.enqueue_billable(SourcePath(vec![1]), BillableKind::Comm, 11);

        assert_eq!(
            machine.drain_canonical(),
            Err(InterpreterError::OutOfPhlogistonsError)
        );
        // Consensus consumed = the COMM budget (1); the over-budget COMM clamps.
        assert_eq!(budget.total_cost().value, 1);
        assert_eq!(
            budget.last_oop_event().map(|event| event.source_path),
            Some(SourcePath(vec![2]))
        );
    }

    #[test]
    fn nonbillable_frames_do_not_enter_cost_trace() {
        let budget = RuntimeBudget::new(Cost::create(10, "test"));
        let machine = MeteredMachine::new(budget.clone());
        let before = budget.cost_trace_digest();
        let key = ContinuationKey {
            deploy_id: [0; 32],
            source_path: SourcePath(vec![0]),
            redex_id: RedexId(0),
        };

        machine.enqueue_frame(MeteredFrame::InstallGate(key));
        machine.drain_canonical().unwrap();
        machine.drain_canonical().unwrap();
        machine.drain_canonical().unwrap();

        assert_eq!(budget.total_cost().value, 0);
        assert_eq!(budget.remaining().value, 10);
        assert_eq!(budget.cost_trace_event_count(), 0);
        assert_eq!(budget.cost_trace_digest(), before);
    }
}
