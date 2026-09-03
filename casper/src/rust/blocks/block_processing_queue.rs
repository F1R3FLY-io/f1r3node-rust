use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::Message;
use tokio::sync::{mpsc, Mutex, OwnedMutexGuard};

use crate::rust::casper::MultiParentCasper;
use crate::rust::metrics_constants::{
    BLOCK_PROCESSING_ADMISSION_BYTES_LIMIT_METRIC, BLOCK_PROCESSING_ADMISSION_BYTES_METRIC,
    BLOCK_PROCESSING_ADMISSION_DEFERRED_TOTAL_METRIC, BLOCK_PROCESSING_QUEUE_PENDING_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE,
};

pub struct BlockProcessingQueueItem {
    pub casper: Arc<dyn MultiParentCasper + Send + Sync>,
    pub block: BlockMessage,
    pub reservation: BlockAdmissionReservation,
}

pub type BlockProcessingQueueReceiver = mpsc::Receiver<BlockProcessingQueueItem>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockAdmissionFailure {
    ByteCapacity,
    CountCapacity,
    Oversized,
    Closed,
    InvalidConfiguration,
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

fn reserved_after(used: usize, requested: usize, capacity: usize) -> Option<usize> {
    requested.checked_add(used).filter(|next| *next <= capacity)
}

#[derive(Debug)]
struct BlockAdmissionBudget {
    capacity: usize,
    used: AtomicUsize,
}

impl BlockAdmissionBudget {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: AtomicUsize::new(0),
        }
    }

    fn try_reserve(self: &Arc<Self>, bytes: usize) -> Result<BlockAdmissionReservation, usize> {
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let Some(next) = reserved_after(used, bytes, self.capacity) else {
                return Err(used);
            };
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    metrics::gauge!(
                        BLOCK_PROCESSING_ADMISSION_BYTES_METRIC,
                        "source" => BLOCK_PROCESSOR_METRICS_SOURCE
                    )
                    .set(next as f64);
                    return Ok(BlockAdmissionReservation {
                        bytes,
                        budget: self.clone(),
                    });
                }
                Err(observed) => used = observed,
            }
        }
    }

    fn used(&self) -> usize { self.used.load(Ordering::Acquire) }
}

#[derive(Debug)]
pub struct BlockAdmissionReservation {
    bytes: usize,
    budget: Arc<BlockAdmissionBudget>,
}

impl BlockAdmissionReservation {
    pub fn bytes(&self) -> usize { self.bytes }
}

impl Drop for BlockAdmissionReservation {
    fn drop(&mut self) {
        let previous = self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
        debug_assert!(previous >= self.bytes);
        metrics::gauge!(
            BLOCK_PROCESSING_ADMISSION_BYTES_METRIC,
            "source" => BLOCK_PROCESSOR_METRICS_SOURCE
        )
        .set(previous.saturating_sub(self.bytes) as f64);
    }
}

#[derive(Clone)]
pub struct BlockProcessingQueueSender {
    sender: mpsc::Sender<BlockProcessingQueueItem>,
    budget: Arc<BlockAdmissionBudget>,
    dependency_scan_lock: Arc<Mutex<()>>,
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
            dependency_scan_lock: Arc::new(Mutex::new(())),
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
        let block_bytes = block.to_proto().encoded_len().max(1);
        if block_bytes > self.budget.capacity {
            return Err(self.error(BlockAdmissionFailure::Oversized, block_bytes));
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
        match self.sender.try_send(BlockProcessingQueueItem {
            casper,
            block,
            reservation,
        }) {
            Ok(()) => {
                self.update_pending_metric();
                Ok(())
            }
            Err(error) => {
                let failure = match &error {
                    mpsc::error::TrySendError::Full(_) => BlockAdmissionFailure::CountCapacity,
                    mpsc::error::TrySendError::Closed(_) => BlockAdmissionFailure::Closed,
                };
                drop(error);
                if failure.is_temporary() {
                    metrics::counter!(
                        BLOCK_PROCESSING_ADMISSION_DEFERRED_TOTAL_METRIC,
                        "source" => BLOCK_PROCESSOR_METRICS_SOURCE,
                        "reason" => failure.metric_label()
                    )
                    .increment(1);
                }
                Err(self.error(failure, block_bytes))
            }
        }
    }

    pub fn byte_capacity(&self) -> usize { self.budget.capacity }

    pub fn used_bytes(&self) -> usize { self.budget.used() }

    pub fn max_capacity(&self) -> usize { self.sender.max_capacity() }

    pub fn capacity(&self) -> usize { self.sender.capacity() }

    pub fn is_closed(&self) -> bool { self.sender.is_closed() }

    pub fn record_dequeue(&self) { self.update_pending_metric(); }

    pub async fn acquire_dependency_scan(&self) -> OwnedMutexGuard<()> {
        self.dependency_scan_lock.clone().lock_owned().await
    }

    fn error(&self, failure: BlockAdmissionFailure, block_bytes: usize) -> BlockAdmissionError {
        BlockAdmissionError {
            failure,
            block_bytes,
            used_bytes: self.budget.used(),
            byte_capacity: self.budget.capacity,
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
mod tests {
    use std::sync::Arc;

    use proptest::prelude::*;
    use tokio::sync::mpsc;

    use super::{
        reserved_after, BlockAdmissionBudget, BlockAdmissionFailure, BlockProcessingQueueSender,
    };

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
