use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::accounting::costs::Cost;
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
}

impl MeteredMachine {
    pub fn new(budget: RuntimeBudget) -> Self {
        Self {
            budget,
            pending: Arc::new(Mutex::new(VecDeque::new())),
            source_path: SourcePath(Vec::new()),
            next_local_index: Arc::new(AtomicU64::new(0)),
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
        }
    }

    pub fn reserve_source_step(&self, amount: Cost) -> Result<(), InterpreterError> {
        self.reserve_cost(BillableKind::SourceStep, amount)
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
        // The formal specification presents metering as a recursive reflection
        // relation. Runtime evaluation uses an explicit frame queue instead:
        // each live charge uses a local frame queue, the frame is drained in
        // canonical key order, and the budget reservation is a single atomic CAS.
        // Keeping live queues local preserves error ownership when independent
        // evaluator branches race only on token reservation.
        if amount.value <= 0 {
            return Err(InterpreterError::BugFoundError(format!(
                "Billable metering cost must be positive for {}",
                amount.operation
            )));
        }

        let local_index = self.next_local_index.fetch_add(1, Ordering::AcqRel);
        let source_path = self.event_source_path(local_index);
        let event = BillableTokenEvent {
            deploy_id: self.budget.deploy_id(),
            source_path,
            redex_id: RedexId(local_index),
            local_index,
            kind,
            weight: amount.value as u64,
        };

        self.drain_frames(vec![MeteredFrame::BillableCost { event, amount }])
    }

    pub fn enqueue_billable(
        &self,
        source_path: SourcePath,
        kind: BillableKind,
        weight: u64,
    ) -> ContinuationKey {
        let local_index = self.next_local_index.fetch_add(1, Ordering::AcqRel);
        let event = BillableTokenEvent {
            deploy_id: self.budget.deploy_id(),
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
        let commit = self.budget.commit_canonical_batch(CostReservationBatch {
            events: billable
                .iter()
                .map(|(event, _)| event.clone())
                .collect::<Vec<_>>(),
        })?;
        for (_, amount) in billable.iter().take(commit.permits.len()) {
            if let Some(amount) = amount {
                self.budget.append_cost_log(amount.clone());
            }
        }
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

        machine.enqueue_billable(SourcePath(vec![2]), BillableKind::SourceStep, 1);
        machine.enqueue_billable(SourcePath(vec![1]), BillableKind::Substitution, 2);
        machine.drain_canonical().unwrap();

        let event_log = budget.get_event_log();
        assert_eq!(event_log.len(), 2);
        assert_eq!(event_log[0].source_path, SourcePath(vec![1]));
        assert_eq!(event_log[1].source_path, SourcePath(vec![2]));
        assert_eq!(budget.total_cost().value, 3);
    }

    #[test]
    fn live_reserve_does_not_drain_shared_pending_batch_queue() {
        let budget = RuntimeBudget::new(Cost::create(1, "test"));
        let machine = MeteredMachine::new(budget.clone());

        machine.enqueue_billable(SourcePath(vec![0]), BillableKind::SourceStep, 2);

        assert!(machine
            .reserve_source_step(Cost::create(1, "live branch"))
            .is_ok());
        assert_eq!(
            machine.drain_canonical(),
            Err(InterpreterError::OutOfPhlogistonsError)
        );
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
            .reserve_source_step(Cost::create(1, "right branch"))
            .unwrap();
        machine
            .child(0)
            .reserve_source_step(Cost::create(1, "left branch"))
            .unwrap();

        let canonical = budget.get_canonical_event_log();
        assert_eq!(canonical[0].source_path, SourcePath(vec![0, 0]));
        assert_eq!(canonical[1].source_path, SourcePath(vec![1, 0]));
    }

    #[test]
    fn canonical_drain_records_stable_oop_descriptor() {
        let budget = RuntimeBudget::new(Cost::create(3, "test"));
        let machine = MeteredMachine::new(budget.clone());

        machine.enqueue_billable(SourcePath(vec![2]), BillableKind::SourceStep, 3);
        machine.enqueue_billable(SourcePath(vec![1]), BillableKind::Substitution, 3);

        assert_eq!(
            machine.drain_canonical(),
            Err(InterpreterError::OutOfPhlogistonsError)
        );
        assert_eq!(budget.total_cost().value, 3);
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
