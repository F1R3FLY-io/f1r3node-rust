use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::casper::protocol::casper_message::BlockMessage;
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::rust::casper::{CasperShardConf, MultiParentCasper};

pub mod evaluation;
pub mod reference;

pub const EVENT_CAPACITY: usize = 256;

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityInputs {
    pub fault_tolerance_threshold_ppm: i64,
    pub max_parent_depth: i32,
    pub deploy_lifespan: i64,
    pub min_phlo_price: i64,
    pub approved_block_hash: String,
    pub approved_post_state_hash: String,
}

pub struct CaptureEndpoint {
    pub(crate) dag: BlockDagKeyValueStorage,
    pub(crate) blocks: KeyValueBlockStore,
    pub(crate) authority: AuthorityInputs,
}

impl CaptureEndpoint {
    pub fn new(
        dag: BlockDagKeyValueStorage,
        blocks: KeyValueBlockStore,
        conf: &CasperShardConf,
        approved: &BlockMessage,
    ) -> Self {
        Self {
            dag,
            blocks,
            authority: AuthorityInputs {
                fault_tolerance_threshold_ppm: conf.fault_tolerance_threshold_ppm,
                max_parent_depth: conf.max_parent_depth,
                deploy_lifespan: conf.deploy_lifespan,
                min_phlo_price: conf.min_phlo_price,
                approved_block_hash: hex::encode(&approved.block_hash),
                approved_post_state_hash: hex::encode(&approved.body.state.post_state_hash),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentError {
    Unsupported,
    AlreadyAttached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    LiveDerivation,
    EffectAttempt,
    EffectReturn,
    DetachedDerivation,
    PersistedObservation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorOutcome {
    Advance,
    NoAdvance,
    ContainmentHold,
    AbsenceHold,
    IncompatibilityHold,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct ObservationEvent {
    pub installation: u64,
    pub sequence: u64,
    pub elapsed_ns: u64,
    pub kind: EventKind,
    pub operation: Option<u64>,
    pub outcome: Option<FloorOutcome>,
    pub hash: Option<[u8; 32]>,
    pub height: Option<i64>,
    pub success: Option<bool>,
    pub snapshot_digest: Option<[u8; 32]>,
}

struct Interval {
    installation: u64,
    started: Instant,
    active: AtomicBool,
    sequence: AtomicU64,
    operations: AtomicU64,
    lost: AtomicU64,
    overflow: AtomicBool,
    sender: mpsc::Sender<ObservationEvent>,
}

#[derive(Clone)]
pub struct ObserverBinding(Arc<Interval>);

#[derive(Clone, Debug, Serialize)]
pub struct Coverage {
    pub incarnation: String,
    pub instance_id: u64,
    pub installation: u64,
    pub attachment_elapsed_ns: u64,
    pub attempted_sequence: u64,
    pub lost: u64,
    pub complete: bool,
    pub active: bool,
    pub live_scope: &'static str,
}

impl ObserverBinding {
    pub fn operation(&self) -> Option<u64> {
        if !self.is_active() {
            return None;
        }
        match self
            .0
            .operations
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
        {
            Ok(n) => Some(n + 1),
            Err(_) => {
                self.0.overflow.store(true, Ordering::Release);
                None
            }
        }
    }

    fn lose(&self) {
        if self
            .0
            .lost
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .is_err()
        {
            self.0.overflow.store(true, Ordering::Release);
        }
    }

    pub fn emit(
        &self,
        kind: EventKind,
        operation: Option<u64>,
        outcome: Option<FloorOutcome>,
        hash: Option<&[u8]>,
        height: Option<i64>,
        success: Option<bool>,
    ) {
        self.emit_snapshot(kind, operation, outcome, hash, height, success, None);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_snapshot(
        &self,
        kind: EventKind,
        operation: Option<u64>,
        outcome: Option<FloorOutcome>,
        hash: Option<&[u8]>,
        height: Option<i64>,
        success: Option<bool>,
        snapshot_digest: Option<[u8; 32]>,
    ) {
        if !self.0.active.load(Ordering::Acquire) {
            return;
        }
        let sequence =
            match self
                .0
                .sequence
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            {
                Ok(n) => n + 1,
                Err(_) => {
                    self.0.overflow.store(true, Ordering::Release);
                    return;
                }
            };
        let hash = match hash.map(<[u8; 32]>::try_from).transpose() {
            Ok(hash) => hash,
            Err(_) => {
                self.lose();
                return;
            }
        };
        let Ok(elapsed_ns) = self.0.started.elapsed().as_nanos().try_into() else {
            self.0.overflow.store(true, Ordering::Release);
            return;
        };
        let event = ObservationEvent {
            installation: self.0.installation,
            sequence,
            elapsed_ns,
            kind,
            operation,
            outcome,
            hash,
            height,
            success,
            snapshot_digest,
        };
        if self.0.sender.try_send(event).is_err() {
            self.lose();
        }
    }

    fn close(&self) { self.0.active.store(false, Ordering::Release); }

    pub fn is_active(&self) -> bool { self.0.active.load(Ordering::Acquire) }
}

struct Installed {
    binding: ObserverBinding,
    endpoint: Arc<CaptureEndpoint>,
    attached_ns: u64,
}

struct ControllerState {
    installation: u64,
    current: Option<Installed>,
    status: &'static str,
}

pub struct ObserverController {
    incarnation: String,
    started: Instant,
    state: Mutex<ControllerState>,
    sender: mpsc::Sender<ObservationEvent>,
    receiver: Mutex<mpsc::Receiver<ObservationEvent>>,
    closed: Arc<AtomicBool>,
    busy: AtomicBool,
}

impl ObserverController {
    pub fn new(incarnation: String) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(EVENT_CAPACITY);
        Arc::new(Self {
            incarnation,
            started: Instant::now(),
            state: Mutex::new(ControllerState {
                installation: 0,
                current: None,
                status: "awaiting_casper",
            }),
            sender,
            receiver: Mutex::new(receiver),
            closed: Arc::new(AtomicBool::new(false)),
            busy: AtomicBool::new(false),
        })
    }

    pub fn install(&self, casper: Option<&(dyn MultiParentCasper + Send + Sync)>) {
        let mut state = self.state.lock();
        if let Some(old) = state.current.take() {
            old.binding.close();
        }
        if self.closed.load(Ordering::Acquire) {
            state.status = "closed";
            return;
        }
        let Some(installation) = state.installation.checked_add(1) else {
            state.status = "installation_overflow";
            return;
        };
        state.installation = installation;
        let Some(casper) = casper else {
            state.status = "awaiting_casper";
            return;
        };
        let binding = ObserverBinding(Arc::new(Interval {
            installation,
            started: Instant::now(),
            active: AtomicBool::new(false),
            sequence: AtomicU64::new(0),
            operations: AtomicU64::new(0),
            lost: AtomicU64::new(0),
            overflow: AtomicBool::new(false),
            sender: self.sender.clone(),
        }));
        match casper.attach_observer(binding.clone()) {
            Ok(endpoint) => {
                let Ok(attached_ns) = self.started.elapsed().as_nanos().try_into() else {
                    state.status = "clock_overflow";
                    return;
                };
                binding.0.active.store(true, Ordering::Release);
                state.current = Some(Installed {
                    binding,
                    endpoint: Arc::new(endpoint),
                    attached_ns,
                });
                state.status = "attached";
            }
            Err(AttachmentError::Unsupported) => state.status = "unsupported",
            Err(AttachmentError::AlreadyAttached) => state.status = "already_attached",
        }
    }

    pub fn status(&self) -> &'static str { self.state.lock().status }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let mut state = self.state.lock();
        if let Some(current) = state.current.take() {
            current.binding.close();
        }
        state.status = "closed";
    }

    fn coverage(&self, binding: &ObserverBinding, attached_ns: u64) -> Coverage {
        let lost = binding.0.lost.load(Ordering::Acquire);
        let active = binding.is_active();
        Coverage {
            incarnation: self.incarnation.clone(),
            instance_id: binding.0.installation,
            installation: binding.0.installation,
            attachment_elapsed_ns: attached_ns,
            attempted_sequence: binding.0.sequence.load(Ordering::Acquire),
            lost,
            live_scope: "finalizer_contexts_created_after_attachment",
            complete: active && lost == 0 && !binding.0.overflow.load(Ordering::Acquire),
            active,
        }
    }

    pub async fn authority_snapshot(
        &self,
        request: evaluation::AuthorityRequest,
        deadline: Instant,
    ) -> Result<evaluation::AuthorityResponse, evaluation::AuthorityFailure> {
        request.validate()?;
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("busy".to_string().into());
        }
        struct BusyGuard<'a>(&'a AtomicBool);
        impl Drop for BusyGuard<'_> {
            fn drop(&mut self) { self.0.store(false, Ordering::Release); }
        }
        let _busy = BusyGuard(&self.busy);
        let (binding, endpoint, attached_ns) = {
            let state = self.state.lock();
            let current = state
                .current
                .as_ref()
                .ok_or_else(|| state.status.to_string())?;
            (
                current.binding.clone(),
                current.endpoint.clone(),
                current.attached_ns,
            )
        };
        tokio::task::yield_now().await;
        let deadline = deadline.min(Instant::now() + std::time::Duration::from_secs(30));
        let mut response =
            evaluation::evaluate(&endpoint, &binding, &request, deadline, self.closed.clone())
                .await?;
        if !binding.is_active() || self.state.lock().installation != binding.0.installation {
            return Err("instance_changed".to_string().into());
        }
        if let Some(mut receiver) = self.receiver.try_lock() {
            for _ in 0..EVENT_CAPACITY {
                match receiver.try_recv() {
                    Ok(event) if event.installation == binding.0.installation => {
                        response.events.push(event)
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        } else {
            binding.lose();
        }
        response.coverage = Some(self.coverage(&binding, attached_ns));
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(controller: &ObserverController) -> ObserverBinding {
        ObserverBinding(Arc::new(Interval {
            installation: 1,
            started: Instant::now(),
            active: AtomicBool::new(true),
            sequence: AtomicU64::new(0),
            operations: AtomicU64::new(0),
            lost: AtomicU64::new(0),
            overflow: AtomicBool::new(false),
            sender: controller.sender.clone(),
        }))
    }

    #[test]
    fn event_counters_refuse_overflow_without_complete_coverage() {
        for counter in 0..3 {
            let controller = ObserverController::new("test".to_string());
            let binding = binding(&controller);
            match counter {
                0 => {
                    binding.0.sequence.store(u64::MAX, Ordering::Release);
                    binding.emit(EventKind::EffectReturn, None, None, None, None, Some(true));
                }
                1 => {
                    binding.0.operations.store(u64::MAX, Ordering::Release);
                    assert_eq!(binding.operation(), None);
                }
                _ => {
                    binding.0.lost.store(u64::MAX, Ordering::Release);
                    binding.lose();
                }
            }
            assert!(!controller.coverage(&binding, 0).complete);
            assert!(controller.receiver.lock().try_recv().is_err());
        }
    }

    #[test]
    fn closed_receiver_and_invalid_hash_record_loss() {
        let controller = ObserverController::new("test".to_string());
        let binding = binding(&controller);
        binding.emit(
            EventKind::LiveDerivation,
            None,
            None,
            Some(&[0; 33]),
            None,
            None,
        );
        assert_eq!(controller.coverage(&binding, 0).lost, 1);
        controller.receiver.lock().close();
        binding.emit(EventKind::EffectReturn, None, None, None, None, Some(true));
        assert_eq!(controller.coverage(&binding, 0).lost, 2);
        assert!(!controller.coverage(&binding, 0).complete);
    }
}
