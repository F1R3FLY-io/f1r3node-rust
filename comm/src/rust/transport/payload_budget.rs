use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::rust::metrics_constants::{
    TRANSPORT_METRICS_SOURCE, TRANSPORT_PAYLOAD_ACTIVE_METRIC,
    TRANSPORT_PAYLOAD_BYTES_LIMIT_METRIC, TRANSPORT_PAYLOAD_BYTES_METRIC,
    TRANSPORT_PAYLOAD_DEFERRED_TOTAL_METRIC,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PayloadBudgetError {
    #[error("payload byte capacity exhausted")]
    ByteCapacity,
    #[error("payload item capacity exhausted")]
    ItemCapacity,
    #[error("payload size arithmetic overflow")]
    Overflow,
    #[error("payload budget capacity must be positive")]
    InvalidCapacity,
}

#[derive(Debug)]
pub struct PayloadBudget {
    direction: &'static str,
    byte_capacity: usize,
    item_capacity: usize,
    used_bytes: AtomicUsize,
    active_items: AtomicUsize,
}

impl PayloadBudget {
    pub fn new(
        direction: &'static str,
        byte_capacity: usize,
        item_capacity: usize,
    ) -> Result<Arc<Self>, PayloadBudgetError> {
        if byte_capacity == 0 || item_capacity == 0 {
            return Err(PayloadBudgetError::InvalidCapacity);
        }
        let budget = Arc::new(Self {
            direction,
            byte_capacity,
            item_capacity,
            used_bytes: AtomicUsize::new(0),
            active_items: AtomicUsize::new(0),
        });
        metrics::gauge!(
            TRANSPORT_PAYLOAD_BYTES_LIMIT_METRIC,
            "source" => TRANSPORT_METRICS_SOURCE,
            "direction" => direction
        )
        .set(byte_capacity as f64);
        budget.update_metrics();
        Ok(budget)
    }

    pub fn try_reserve(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Result<PayloadReservation, PayloadBudgetError> {
        reserve_atomic(
            &self.active_items,
            1,
            self.item_capacity,
            PayloadBudgetError::ItemCapacity,
        )
        .inspect_err(|error| {
            metrics::counter!(
                TRANSPORT_PAYLOAD_DEFERRED_TOTAL_METRIC,
                "source" => TRANSPORT_METRICS_SOURCE,
                "direction" => self.direction,
                "reason" => metric_reason(*error)
            )
            .increment(1);
        })?;
        if let Err(error) = reserve_atomic(
            &self.used_bytes,
            bytes,
            self.byte_capacity,
            PayloadBudgetError::ByteCapacity,
        ) {
            self.active_items.fetch_sub(1, Ordering::AcqRel);
            metrics::counter!(
                TRANSPORT_PAYLOAD_DEFERRED_TOTAL_METRIC,
                "source" => TRANSPORT_METRICS_SOURCE,
                "direction" => self.direction,
                "reason" => metric_reason(error)
            )
            .increment(1);
            self.update_metrics();
            return Err(error);
        }
        self.update_metrics();
        Ok(PayloadReservation {
            bytes,
            budget: self.clone(),
        })
    }

    pub fn byte_capacity(&self) -> usize { self.byte_capacity }

    pub fn item_capacity(&self) -> usize { self.item_capacity }

    pub fn used_bytes(&self) -> usize { self.used_bytes.load(Ordering::Acquire) }

    pub fn active_items(&self) -> usize { self.active_items.load(Ordering::Acquire) }

    fn update_metrics(&self) {
        metrics::gauge!(
            TRANSPORT_PAYLOAD_BYTES_METRIC,
            "source" => TRANSPORT_METRICS_SOURCE,
            "direction" => self.direction
        )
        .set(self.used_bytes() as f64);
        metrics::gauge!(
            TRANSPORT_PAYLOAD_ACTIVE_METRIC,
            "source" => TRANSPORT_METRICS_SOURCE,
            "direction" => self.direction
        )
        .set(self.active_items() as f64);
    }
}

#[derive(Debug)]
pub struct PayloadReservation {
    bytes: usize,
    budget: Arc<PayloadBudget>,
}

impl PayloadReservation {
    pub fn try_grow(&mut self, bytes: usize) -> Result<(), PayloadBudgetError> {
        let next_bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(PayloadBudgetError::Overflow)?;
        reserve_atomic(
            &self.budget.used_bytes,
            bytes,
            self.budget.byte_capacity,
            PayloadBudgetError::ByteCapacity,
        )
        .inspect_err(|error| {
            metrics::counter!(
                TRANSPORT_PAYLOAD_DEFERRED_TOTAL_METRIC,
                "source" => TRANSPORT_METRICS_SOURCE,
                "direction" => self.budget.direction,
                "reason" => metric_reason(*error)
            )
            .increment(1);
        })?;
        self.bytes = next_bytes;
        self.budget.update_metrics();
        Ok(())
    }

    pub fn bytes(&self) -> usize { self.bytes }
}

impl Drop for PayloadReservation {
    fn drop(&mut self) {
        let previous_bytes = self
            .budget
            .used_bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
        let previous_items = self.budget.active_items.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous_bytes >= self.bytes);
        debug_assert!(previous_items >= 1);
        self.budget.update_metrics();
    }
}

fn reserve_atomic(
    used: &AtomicUsize,
    requested: usize,
    capacity: usize,
    capacity_error: PayloadBudgetError,
) -> Result<(), PayloadBudgetError> {
    let mut current = used.load(Ordering::Acquire);
    loop {
        let next = current
            .checked_add(requested)
            .ok_or(PayloadBudgetError::Overflow)?;
        if next > capacity {
            return Err(capacity_error);
        }
        match used.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}

fn metric_reason(error: PayloadBudgetError) -> &'static str {
    match error {
        PayloadBudgetError::ByteCapacity => "byte-capacity",
        PayloadBudgetError::ItemCapacity => "item-capacity",
        PayloadBudgetError::Overflow => "overflow",
        PayloadBudgetError::InvalidCapacity => "invalid-capacity",
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{PayloadBudget, PayloadBudgetError};

    #[test]
    fn reservations_and_growth_release_exactly() {
        let budget = PayloadBudget::new("test", 10, 2).unwrap();
        let mut first = budget.try_reserve(3).unwrap();
        let second = budget.try_reserve(5).unwrap();
        assert_eq!(budget.used_bytes(), 8);
        assert_eq!(budget.active_items(), 2);
        assert_eq!(first.try_grow(2), Ok(()));
        assert_eq!(budget.used_bytes(), 10);
        assert_eq!(first.try_grow(1), Err(PayloadBudgetError::ByteCapacity));
        drop(second);
        assert_eq!(budget.used_bytes(), 5);
        assert_eq!(budget.active_items(), 1);
        drop(first);
        assert_eq!(budget.used_bytes(), 0);
        assert_eq!(budget.active_items(), 0);
    }

    proptest! {
        #[test]
        fn arbitrary_reservation_sequences_never_exceed_or_drift(
            requests in prop::collection::vec(0usize..256, 0..128),
            capacity in 1usize..1024,
            item_capacity in 1usize..32,
        ) {
            let budget = PayloadBudget::new("test", capacity, item_capacity).unwrap();
            let mut reservations = Vec::new();
            for request in requests {
                if let Ok(reservation) = budget.try_reserve(request) {
                    reservations.push(reservation);
                }
                prop_assert!(budget.used_bytes() <= budget.byte_capacity());
                prop_assert!(budget.active_items() <= budget.item_capacity());
                if reservations.len() > item_capacity / 2 {
                    reservations.remove(0);
                }
                let live_sum: usize = reservations.iter().map(|reservation| reservation.bytes()).sum();
                prop_assert_eq!(budget.used_bytes(), live_sum);
                prop_assert_eq!(budget.active_items(), reservations.len());
            }
            drop(reservations);
            prop_assert_eq!(budget.used_bytes(), 0);
            prop_assert_eq!(budget.active_items(), 0);
        }
    }
}
