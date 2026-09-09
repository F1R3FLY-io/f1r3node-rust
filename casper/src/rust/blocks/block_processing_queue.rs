use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as IdentityMutex};

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::Message;
use tokio::sync::mpsc;

mod admission_budget;
mod admission_identity;
mod recovery_control;
mod recovery_pass;
mod startup_completion;
mod startup_owner;
mod startup_snapshot_lease;
#[cfg(any(test, kani))]
use admission_budget::reserved_after;
use admission_budget::BlockAdmissionBudget;
pub use admission_budget::BlockAdmissionReservation;
pub(crate) use recovery_control::RecoveryRegistration;
use recovery_control::RecoveryReleaseWake;
pub use recovery_control::{
    PreparedRecoveryContext, RecoveryActiveStartup, RecoveryControl, RecoverySignal,
    RecoveryStartupContext, RecoveryStartupOwner, RecoveryStopGuard, RecoveryWake,
};
pub use recovery_pass::RecoveryPass;
pub use startup_completion::{StartupCompletion, StartupKey, StartupPhase, StartupRequest};
pub use startup_owner::{ActiveStartup, StartupContext, StartupError, StartupOwner, StartupTicket};
pub type BlockProcessingIdentities = admission_identity::AdmissionIdentities<BlockHash>;

#[derive(Debug)]
pub struct BlockProcessingLease {
    reservation: BlockAdmissionReservation,
    _identity: admission_identity::AdmissionIdentity<BlockHash>,
    _wake: Option<RecoveryReleaseWake>,
}

impl BlockProcessingLease {
    pub fn bytes(&self) -> usize { self.reservation.bytes() }
}

use crate::rust::casper::MultiParentCasper;
use crate::rust::metrics_constants::{
    BLOCK_PROCESSING_ADMISSION_BYTES_LIMIT_METRIC, BLOCK_PROCESSING_ADMISSION_BYTES_METRIC,
    BLOCK_PROCESSING_ADMISSION_DEFERRED_TOTAL_METRIC, BLOCK_PROCESSING_QUEUE_PENDING_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE,
};

pub struct BlockProcessingQueueItem {
    pub casper: Arc<dyn MultiParentCasper + Send + Sync>,
    pub block: BlockMessage,
    pub reservation: BlockProcessingLease,
}

impl BlockProcessingQueueItem {
    pub fn into_parts(
        self,
    ) -> (
        BlockProcessingLease,
        Arc<dyn MultiParentCasper + Send + Sync>,
        BlockMessage,
    ) {
        (self.reservation, self.casper, self.block)
    }
}

pub type BlockProcessingQueueReceiver = mpsc::Receiver<BlockProcessingQueueItem>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAdmissionFailure {
    ByteCapacity,
    CountCapacity,
    Oversized,
    Closed,
    InvalidConfiguration,
    Duplicate,
}

impl BlockAdmissionFailure {
    pub fn is_temporary(self) -> bool { matches!(self, Self::ByteCapacity | Self::CountCapacity) }

    fn metric_label(self) -> &'static str {
        match self {
            Self::ByteCapacity => "byte-capacity",
            Self::CountCapacity => "count-capacity",
            Self::Oversized => "oversized",
            Self::Closed => "closed",
            Self::InvalidConfiguration => "invalid-configuration",
            Self::Duplicate => "duplicate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "block admission failed: {failure:?} (block_bytes={block_bytes}, used_bytes={used_bytes}, byte_capacity={byte_capacity})"
)]
pub struct BlockAdmissionError {
    pub failure: BlockAdmissionFailure,
    pub block_bytes: usize,
    pub used_bytes: usize,
    pub byte_capacity: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum BlockPublicationError<E> {
    #[error(transparent)]
    Admission(#[from] BlockAdmissionError),
    #[error("block receipt failed: {0}")]
    Receipt(E),
}

fn record_admission_bytes(bytes: usize) {
    metrics::gauge!(
        BLOCK_PROCESSING_ADMISSION_BYTES_METRIC,
        "source" => BLOCK_PROCESSOR_METRICS_SOURCE
    )
    .set(bytes as f64);
}

#[derive(Clone)]
pub struct BlockProcessingQueueSender {
    sender: mpsc::Sender<BlockProcessingQueueItem>,
    budget: Arc<BlockAdmissionBudget>,
    identities: Arc<BlockProcessingIdentities>,
    recovery: Arc<RecoveryControl>,
}

#[derive(Clone)]
pub struct WeakBlockProcessingQueueSender {
    sender: mpsc::WeakSender<BlockProcessingQueueItem>,
    budget: Arc<BlockAdmissionBudget>,
    identities: Arc<BlockProcessingIdentities>,
    recovery: Arc<RecoveryControl>,
}

impl WeakBlockProcessingQueueSender {
    pub fn record_dequeue(&self, pending: usize) {
        metrics::gauge!(
            BLOCK_PROCESSING_QUEUE_PENDING_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .set(pending as f64);
        self.recovery.signal().request(false);
    }

    pub fn upgrade(&self) -> Option<BlockProcessingQueueSender> {
        Some(BlockProcessingQueueSender {
            sender: self.sender.upgrade()?,
            budget: self.budget.clone(),
            identities: self.identities.clone(),
            recovery: self.recovery.clone(),
        })
    }
}

impl BlockProcessingQueueSender {
    pub fn channel(
        count_capacity: usize,
        byte_capacity: usize,
    ) -> Result<(Self, BlockProcessingQueueReceiver), BlockAdmissionError> {
        if count_capacity == 0 || byte_capacity == 0 {
            return Err(BlockAdmissionError {
                failure: BlockAdmissionFailure::InvalidConfiguration,
                block_bytes: 0,
                used_bytes: 0,
                byte_capacity,
            });
        }
        let (sender, receiver) = mpsc::channel(count_capacity);
        let this = Self {
            sender,
            budget: Arc::new(BlockAdmissionBudget::new(byte_capacity)),
            identities: Arc::new(BlockProcessingIdentities::new()),
            recovery: Arc::new(RecoveryControl::default()),
        };
        metrics::gauge!(
            BLOCK_PROCESSING_ADMISSION_BYTES_LIMIT_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .set(byte_capacity as f64);
        metrics::gauge!(
            BLOCK_PROCESSING_QUEUE_PENDING_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .set(0.0);
        Ok((this, receiver))
    }

    pub fn try_enqueue(
        &self,
        casper: Arc<dyn MultiParentCasper + Send + Sync>,
        block: BlockMessage,
    ) -> Result<(), BlockAdmissionError> {
        self.try_enqueue_with_receipt(casper, block, |_| Ok::<(), std::convert::Infallible>(()))
            .map_err(|error| match error {
                BlockPublicationError::Admission(error) => error,
                BlockPublicationError::Receipt(error) => match error {},
            })
    }

    pub fn try_enqueue_with_receipt<E>(
        &self,
        casper: Arc<dyn MultiParentCasper + Send + Sync>,
        block: BlockMessage,
        receipt: impl FnOnce(&BlockHash) -> Result<(), E>,
    ) -> Result<(), BlockPublicationError<E>> {
        let identity = self
            .identities
            .try_claim(block.block_hash.clone())
            .ok_or_else(|| self.error(BlockAdmissionFailure::Duplicate, 0))?;
        let owned_block = block;
        let block_bytes = owned_block.to_proto().encoded_len().max(1);
        if block_bytes > self.budget.capacity() {
            return Err(self
                .error(BlockAdmissionFailure::Oversized, block_bytes)
                .into());
        }
        let reservation = self.budget.try_reserve(block_bytes).map_err(|_| {
            metrics::counter!(
                BLOCK_PROCESSING_ADMISSION_DEFERRED_TOTAL_METRIC,
                "source" => BLOCK_PROCESSOR_METRICS_SOURCE,
                "reason" => BlockAdmissionFailure::ByteCapacity.metric_label()
            )
            .increment(1);
            self.error(BlockAdmissionFailure::ByteCapacity, block_bytes)
        })?;
        let mut item = BlockProcessingQueueItem {
            casper,
            block: owned_block,
            reservation: BlockProcessingLease {
                reservation,
                _identity: identity,
                _wake: None,
            },
        };
        match self.sender.try_reserve() {
            Ok(permit) => {
                receipt(&item.block.block_hash).map_err(BlockPublicationError::Receipt)?;
                item.reservation._wake = Some(RecoveryReleaseWake::new(self.recovery.signal()));
                permit.send(item);
                self.update_pending_metric();
                Ok(())
            }
            Err(error) => {
                let failure = match &error {
                    mpsc::error::TrySendError::Full(_) => BlockAdmissionFailure::CountCapacity,
                    mpsc::error::TrySendError::Closed(_) => BlockAdmissionFailure::Closed,
                };
                drop(item);
                if failure.is_temporary() {
                    metrics::counter!(
                        BLOCK_PROCESSING_ADMISSION_DEFERRED_TOTAL_METRIC,
                        "source" => BLOCK_PROCESSOR_METRICS_SOURCE,
                        "reason" => failure.metric_label()
                    )
                    .increment(1);
                }
                Err(self.error(failure, block_bytes).into())
            }
        }
    }

    pub fn byte_capacity(&self) -> usize { self.budget.capacity() }

    pub fn identities(&self) -> Arc<BlockProcessingIdentities> { self.identities.clone() }

    pub fn used_bytes(&self) -> usize { self.budget.used() }

    pub fn max_capacity(&self) -> usize { self.sender.max_capacity() }

    pub fn capacity(&self) -> usize { self.sender.capacity() }

    pub fn is_closed(&self) -> bool { self.sender.is_closed() }

    pub fn record_dequeue(&self) {
        self.update_pending_metric();
        self.recovery.signal().request(false);
    }

    pub fn recovery(&self) -> Arc<RecoveryControl> { self.recovery.clone() }

    pub fn downgrade(&self) -> WeakBlockProcessingQueueSender {
        WeakBlockProcessingQueueSender {
            sender: self.sender.downgrade(),
            budget: self.budget.clone(),
            identities: self.identities.clone(),
            recovery: self.recovery.clone(),
        }
    }

    fn error(&self, failure: BlockAdmissionFailure, block_bytes: usize) -> BlockAdmissionError {
        BlockAdmissionError {
            failure,
            block_bytes,
            used_bytes: self.budget.used(),
            byte_capacity: self.budget.capacity(),
        }
    }

    fn update_pending_metric(&self) {
        let pending = self.max_capacity().saturating_sub(self.capacity());
        metrics::gauge!(
            BLOCK_PROCESSING_QUEUE_PENDING_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .set(pending as f64);
    }
}

#[cfg(test)]
#[path = "block_processing_queue/receipt_tests.rs"]
mod receipt_tests;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use proptest::prelude::*;
    use tokio::sync::mpsc;

    use super::{
        reserved_after, BlockAdmissionBudget, BlockAdmissionFailure, BlockProcessingQueueSender,
    };

    #[tokio::test]
    async fn weak_admission_endpoint_does_not_prevent_input_closure() {
        let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1024).unwrap();
        let weak = sender.downgrade();
        let recovery = sender.recovery();
        assert!(weak.upgrade().is_some());
        drop(sender);
        assert!(weak.upgrade().is_none());
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
                .is_none()
        );
        assert!(recovery.context().unwrap().is_none());
    }

    #[test]
    fn identity_release_checks_the_exact_owner() {
        super::admission_identity::previous_owner_cannot_release_replacement_identity();
    }

    proptest! {
        #[test]
        fn arbitrary_identity_histories_match_live_guards(
            operations in prop::collection::vec((0u8..4, any::<u8>()), 0..512),
        ) {
            let identities = Arc::new(super::admission_identity::AdmissionIdentities::<u8>::new());
            let mut held = std::collections::BTreeMap::new();
            for (operation, key) in operations {
                if operation < 2 {
                    let identity = identities.try_claim(key);
                    prop_assert_eq!(identity.is_some(), !held.contains_key(&key));
                    if let Some(identity) = identity {
                        held.insert(key, identity);
                    }
                } else {
                    drop(held.remove(&key));
                }
                prop_assert_eq!(identities.len(), held.len());
                for expected in 0..=u8::MAX {
                    prop_assert_eq!(identities.contains(&expected), held.contains_key(&expected));
                }
            }
            drop(held);
            prop_assert!(identities.is_empty());
        }
    }

    #[test]
    fn reservations_release_exactly() {
        let budget = Arc::new(BlockAdmissionBudget::new(7));
        let first = budget.try_reserve(3).expect("first reservation");
        let second = budget.try_reserve(4).expect("second reservation");
        assert_eq!(budget.used(), 7);
        assert!(budget.try_reserve(1).is_err());
        drop(first);
        assert_eq!(budget.used(), 4);
        drop(second);
        assert_eq!(budget.used(), 0);
    }

    #[tokio::test]
    async fn queue_ownership_holds_bytes_until_the_received_reservation_is_dropped() {
        let budget = Arc::new(BlockAdmissionBudget::new(7));
        let (sender, mut receiver) = mpsc::channel(1);
        sender
            .try_send(budget.try_reserve(7).expect("reservation"))
            .expect("queue admission");
        assert_eq!(budget.used(), 7);

        let reservation = receiver.recv().await.expect("queued reservation");
        assert_eq!(budget.used(), 7);
        drop(reservation);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn rejected_count_admission_releases_only_the_rejected_reservation() {
        let budget = Arc::new(BlockAdmissionBudget::new(7));
        let (sender, mut receiver) = mpsc::channel(1);
        sender
            .try_send(budget.try_reserve(3).expect("first reservation"))
            .expect("first queue admission");
        let error = sender
            .try_send(budget.try_reserve(4).expect("second reservation"))
            .expect_err("second queue admission must be full");
        assert_eq!(budget.used(), 7);
        drop(error);
        assert_eq!(budget.used(), 3);
        drop(receiver.try_recv().expect("first queued reservation"));
        assert_eq!(budget.used(), 0);
    }

    #[tokio::test]
    async fn cancelled_worker_releases_the_actual_reservation() {
        let budget = Arc::new(BlockAdmissionBudget::new(7));
        let reservation = budget.try_reserve(7).unwrap();
        let worker = tokio::spawn(async move {
            let _reservation = reservation;
            std::future::pending::<()>().await;
        });
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn unwinding_worker_releases_the_actual_reservation() {
        let budget = Arc::new(BlockAdmissionBudget::new(7));
        let worker_budget = budget.clone();
        let result = std::panic::catch_unwind(move || {
            let _reservation = worker_budget.try_reserve(7).unwrap();
            panic!("injected worker failure");
        });
        assert!(result.is_err());
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn dropping_a_receiver_releases_all_queued_reservations() {
        let budget = Arc::new(BlockAdmissionBudget::new(7));
        let (sender, receiver) = mpsc::channel(2);
        sender.try_send(budget.try_reserve(3).unwrap()).unwrap();
        sender.try_send(budget.try_reserve(4).unwrap()).unwrap();
        drop(receiver);
        assert_eq!(budget.used(), 0);
        assert!(sender.is_closed());
    }

    #[test]
    fn zero_capacities_are_rejected_as_invalid_configuration() {
        for (count, bytes) in [(0, 1), (1, 0), (0, 0)] {
            let error = BlockProcessingQueueSender::channel(count, bytes)
                .err()
                .expect("zero capacity must fail");
            assert_eq!(error.failure, BlockAdmissionFailure::InvalidConfiguration);
        }
    }

    proptest! {
        #[test]
        fn reservation_arithmetic_never_wraps_or_exceeds_capacity(
            used in any::<usize>(),
            requested in any::<usize>(),
            capacity in any::<usize>(),
        ) {
            if let Some(next) = reserved_after(used, requested, capacity) {
                prop_assert!(next >= used);
                prop_assert!(next >= requested);
                prop_assert!(next <= capacity);
                prop_assert_eq!(next, used + requested);
            }
        }

        #[test]
        fn arbitrary_reserve_release_sequences_are_exact(
            capacity in 1usize..4096,
            operations in prop::collection::vec((any::<bool>(), 1usize..1024), 0..256),
        ) {
            let budget = Arc::new(BlockAdmissionBudget::new(capacity));
            let mut reservations = Vec::new();
            let mut expected = 0usize;
            for (reserve, bytes) in operations {
                if reserve {
                    match budget.try_reserve(bytes) {
                        Ok(reservation) => {
                            expected += bytes;
                            reservations.push(reservation);
                        }
                        Err(_) => prop_assert!(expected.checked_add(bytes).is_none_or(|next| next > capacity)),
                    }
                } else if !reservations.is_empty() {
                    let index = bytes % reservations.len();
                    expected -= reservations.swap_remove(index).bytes();
                }
                prop_assert_eq!(budget.used(), expected);
                prop_assert!(expected <= capacity);
            }
            drop(reservations);
            prop_assert_eq!(budget.used(), 0);
        }
    }
}

#[cfg(kani)]
mod verification {
    use super::reserved_after;

    #[kani::proof]
    fn successful_reservation_is_exact_and_bounded() {
        let used: usize = kani::any();
        let requested: usize = kani::any();
        let capacity: usize = kani::any();
        if let Some(next) = reserved_after(used, requested, capacity) {
            assert!(next >= used);
            assert!(next >= requested);
            assert!(next <= capacity);
            assert_eq!(next, used + requested);
        }
    }

    #[kani::proof]
    fn failed_reservation_cannot_fit() {
        let used: usize = kani::any();
        let requested: usize = kani::any();
        let capacity: usize = kani::any();
        if reserved_after(used, requested, capacity).is_none() {
            assert!(used
                .checked_add(requested)
                .is_none_or(|next| next > capacity));
        }
    }
}
