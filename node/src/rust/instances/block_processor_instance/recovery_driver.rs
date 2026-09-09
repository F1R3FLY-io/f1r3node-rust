use std::sync::Arc;
use std::time::Duration;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::util::startup_scan::StartupScanAction;
use casper::rust::blocks::block_processing_queue::{
    BlockAdmissionFailure, BlockPublicationError, RecoveryActiveStartup, RecoveryControl,
    RecoveryPass, RecoveryStartupContext, RecoveryWake, WeakBlockProcessingQueueSender,
};
use casper::rust::blocks::block_processor::BlockProcessor;
use casper::rust::casper::RetryCandidate;
use casper::rust::errors::CasperError;
use casper::rust::{ProposeFunction, ProposeRequestKind};
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use tokio::sync::watch;

const PAGE_VISITS: usize = 64;
const RETRY_DELAY: Duration = Duration::from_secs(1);

fn retry_demand(startup_active: bool, ordinary_active: bool) -> RecoveryWake {
    if startup_active || ordinary_active {
        RecoveryWake::Idle
    } else {
        RecoveryWake::Work { proposal: false }
    }
}

#[cfg(test)]
#[path = "recovery_driver_tests.rs"]
mod tests;

enum Step {
    Advanced,
    Parked,
    Complete,
}

struct StartupPass {
    active: RecoveryActiveStartup,
    pending: Option<BlockHash>,
}

impl StartupPass {
    fn step<T: TransportLayer + Send + Sync>(
        &mut self,
        queue: &WeakBlockProcessingQueueSender,
        processor: &BlockProcessor<T>,
    ) -> Result<Step, CasperError> {
        let Some(casper) = self.active.context()? else {
            return Ok(Step::Complete);
        };
        let Some(sender) = queue.upgrade() else {
            return Ok(Step::Complete);
        };
        if self.pending.is_none() {
            let action = self
                .active
                .next_scan_action(
                    || sender.capacity() != 0,
                    CasperBufferKeyValueStorage::is_block_candidate,
                )
                .map_err(|error| {
                    CasperError::RuntimeError(format!("Startup cursor failed: {error:?}"))
                })?;
            match action {
                StartupScanAction::CheckPresence(key) => {
                    let present = casper.block_store().contains(&key.0)?;
                    self.active.record_scan_presence(present).map_err(|error| {
                        CasperError::RuntimeError(format!("Startup presence failed: {error:?}"))
                    })?;
                    return Ok(Step::Advanced);
                }
                StartupScanAction::Process(key) => self.pending = Some(key.0),
                StartupScanAction::SkippedMetadata => return Ok(Step::Advanced),
                StartupScanAction::PhaseChanged => {
                    self.active.presence_complete()?;
                    return Ok(Step::Advanced);
                }
                StartupScanAction::Parked => return Ok(Step::Parked),
                StartupScanAction::Complete => {
                    self.active.scan_complete()?;
                    return Ok(Step::Complete);
                }
                StartupScanAction::Failed | StartupScanAction::Cancelled => {
                    return Ok(Step::Complete)
                }
            }
        }
        if sender.capacity() == 0 {
            return Ok(Step::Parked);
        }
        let hash = self
            .pending
            .as_ref()
            .expect("startup owns the pending candidate");
        let candidate = casper.prepare_startup_candidate(hash)?;
        let outcome = match candidate {
            RetryCandidate::AlreadyAdmitted => {
                match casper.remove_buffered_hash(hash) {
                    Ok(()) => {
                        if let Err(error) = processor.forget_hash_tracking(hash) {
                            tracing::warn!(%error, "Startup tracker reconciliation failed");
                        }
                    }
                    Err(error) => tracing::warn!(%error, "Startup buffer reconciliation failed"),
                }
                Step::Advanced
            }
            RetryCandidate::Ready(block) => {
                if processor.is_validation_failure_quarantined(hash)? {
                    Step::Advanced
                } else {
                    match sender.try_enqueue_with_receipt(casper.clone(), *block, |hash| {
                        processor.record_received(hash.clone()).map(|_| ())
                    }) {
                        Ok(()) => Step::Advanced,
                        Err(BlockPublicationError::Receipt(error)) => return Err(error),
                        Err(BlockPublicationError::Admission(error))
                            if error.failure == BlockAdmissionFailure::Duplicate =>
                        {
                            Step::Advanced
                        }
                        Err(BlockPublicationError::Admission(error))
                            if error.failure.is_temporary() =>
                        {
                            return Ok(Step::Parked)
                        }
                        Err(BlockPublicationError::Admission(error)) => {
                            return Err(CasperError::Other(error.to_string()))
                        }
                    }
                }
            }
            RetryCandidate::MissingBody => Step::Advanced,
            RetryCandidate::Absent
            | RetryCandidate::WaitingCertificate
            | RetryCandidate::MissingMetadata => {
                return Err(CasperError::RuntimeError(
                    "Startup candidate returned an ordinary retry disposition".into(),
                ));
            }
        };
        self.active.record_scan_processed().map_err(|error| {
            CasperError::RuntimeError(format!("Startup admission failed: {error:?}"))
        })?;
        self.pending = None;
        Ok(outcome)
    }
}

struct OrdinaryPass {
    origin: RecoveryStartupContext,
    pass: RecoveryPass,
    pending: Option<BlockHash>,
}

impl OrdinaryPass {
    fn complete(&self) -> bool { self.pass.remaining() == 0 && self.pending.is_none() }

    fn candidate(&mut self, next: impl FnOnce() -> Option<BlockHash>) -> Option<&BlockHash> {
        if self.pending.is_none() && self.pass.visit() {
            self.pending = next();
        }
        self.pending.as_ref()
    }

    fn finish_attempt(
        &mut self,
        result: Result<Step, CasperError>,
        before: usize,
        retrying: bool,
    ) -> Result<Step, CasperError> {
        match &result {
            Ok(Step::Parked) => {}
            Err(_) => {
                self.pending = None;
                self.pass.fail();
                if !retrying && self.pass.remaining() == before {
                    self.pass.visit();
                }
            }
            Ok(_) => self.pending = None,
        }
        result
    }

    fn step<T: TransportLayer + Send + Sync>(
        &mut self,
        queue: &WeakBlockProcessingQueueSender,
        processor: &BlockProcessor<T>,
    ) -> Result<Step, CasperError> {
        if self.complete() {
            return Ok(Step::Complete);
        }
        let Some(casper) = self.origin.context()? else {
            self.pass.fail();
            return Ok(Step::Complete);
        };
        let Some(sender) = queue.upgrade() else {
            self.pass.fail();
            return Ok(Step::Complete);
        };
        if sender.capacity() == 0 {
            return Ok(Step::Parked);
        }
        let Some(hash) = self.candidate(|| casper.next_retry_candidate()) else {
            return Ok(Step::Advanced);
        };
        if sender.identities().contains(hash)
            || processor.is_validation_failure_quarantined(hash)?
        {
            return Ok(Step::Advanced);
        }
        match casper.prepare_retry_candidate(hash)? {
            RetryCandidate::AlreadyAdmitted => {
                casper.remove_buffered_hash(hash)?;
                processor.forget_hash_tracking(hash)?;
                Ok(Step::Advanced)
            }
            RetryCandidate::Ready(block) => {
                match sender.try_enqueue_with_receipt(casper, *block, |hash| {
                    processor.record_received(hash.clone()).map(|_| ())
                }) {
                    Ok(()) => Ok(Step::Advanced),
                    Err(BlockPublicationError::Receipt(error)) => Err(error),
                    Err(BlockPublicationError::Admission(error))
                        if error.failure == BlockAdmissionFailure::Duplicate =>
                    {
                        Ok(Step::Advanced)
                    }
                    Err(BlockPublicationError::Admission(error))
                        if error.failure.is_temporary() =>
                    {
                        Ok(Step::Parked)
                    }
                    Err(BlockPublicationError::Admission(error)) => {
                        Err(CasperError::Other(error.to_string()))
                    }
                }
            }
            RetryCandidate::Absent
            | RetryCandidate::MissingBody
            | RetryCandidate::MissingMetadata
            | RetryCandidate::WaitingCertificate => Ok(Step::Advanced),
        }
    }
}

pub(super) async fn run<T: TransportLayer + Send + Sync>(
    queue: WeakBlockProcessingQueueSender,
    processor: Arc<BlockProcessor<T>>,
    control: Arc<RecoveryControl>,
    offers: watch::Sender<Option<RecoveryStartupContext>>,
) -> Result<(), CasperError> {
    let signal = control.signal();
    let owner = control.startup();
    let mut startup: Option<StartupPass> = None;
    let mut ordinary: Option<OrdinaryPass> = None;
    let mut successor = RecoveryWake::Idle;
    let mut retry =
        tokio::time::interval_at(tokio::time::Instant::now() + RETRY_DELAY, RETRY_DELAY);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if signal.is_stopped() {
            return Ok(());
        }
        if startup.is_none() {
            startup = owner.activate()?.map(|active| StartupPass {
                active,
                pending: None,
            });
        }
        if startup.is_none() && ordinary.is_none() {
            successor = successor.merge(signal.take());
            match successor {
                RecoveryWake::Stopped => return Ok(()),
                RecoveryWake::Idle => {}
                RecoveryWake::Work { proposal } => {
                    if let Some(origin) = owner.current_context()? {
                        if let Some(casper) = origin.context()? {
                            ordinary = Some(OrdinaryPass {
                                origin,
                                pass: RecoveryPass::new(casper.retry_candidate_count(), proposal),
                                pending: None,
                            });
                            successor = RecoveryWake::Idle;
                        }
                    }
                }
            }
        }
        let mut parked = startup.is_none() && ordinary.is_none();
        for _ in 0..PAGE_VISITS {
            if let Some(pass) = startup.as_mut() {
                let result = pass.step(&queue, &processor);
                match result {
                    Ok(Step::Advanced) => {}
                    Ok(Step::Parked) => {
                        parked = true;
                        break;
                    }
                    Ok(Step::Complete) => {
                        startup = None;
                        break;
                    }
                    Err(error) => {
                        pass.active.fail(error)?;
                        startup = None;
                        break;
                    }
                }
            } else if let Some(pass) = ordinary.as_mut() {
                let remaining = pass.pass.remaining();
                let retrying = pass.pending.is_some();
                let result = pass.step(&queue, &processor);
                let result = pass.finish_attempt(result, remaining, retrying);
                match result {
                    Ok(Step::Advanced) => {}
                    Ok(Step::Parked) => {
                        parked = true;
                        break;
                    }
                    Ok(Step::Complete) => {
                        if pass.complete()
                            && pass.pass.proposal_ready()
                            && pass.origin.is_current()?
                        {
                            offers.send_replace(Some(pass.origin.clone()));
                        }
                        ordinary = None;
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Recovery candidate failed; pass proposal disabled");
                    }
                }
            } else {
                break;
            }
        }
        if parked {
            tokio::select! {
                wake = signal.wait() => successor = successor.merge(wake),
                _ = owner.changed() => {},
                _ = retry.tick() => {
                    successor = successor.merge(retry_demand(startup.is_some(), ordinary.is_some()));
                },
            }
        } else {
            tokio::task::yield_now().await;
        }
    }
}

pub(super) async fn propose(
    mut offers: watch::Receiver<Option<RecoveryStartupContext>>,
    trigger: Option<Arc<ProposeFunction>>,
) -> Result<(), CasperError> {
    while offers.changed().await.is_ok() {
        let Some(origin) = offers.borrow_and_update().clone() else {
            continue;
        };
        let Some(trigger) = trigger
            .as_ref()
            .filter(|_| super::trigger_propose_after_block_processing_enabled())
        else {
            continue;
        };
        let Some(casper) = origin.context()? else {
            continue;
        };
        let bonded = if let Some(validator) = casper.get_validator() {
            match casper.get_snapshot().await {
                Ok(snapshot) => super::is_finalized_floor_validator(
                    &snapshot.finalized_floor_validators(),
                    &validator.public_key.bytes,
                ),
                Err(error) => {
                    tracing::warn!(%error, "Recovery proposal bond lookup failed");
                    false
                }
            }
        } else {
            false
        };
        drop(casper);
        if bonded && origin.is_current()? {
            if let Err(error) = trigger(ProposeRequestKind::PendingDeploy).await {
                tracing::error!(%error, "Recovery proposal failed");
            }
        }
    }
    Ok(())
}
