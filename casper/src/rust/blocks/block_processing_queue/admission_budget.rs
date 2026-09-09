use super::{record_admission_bytes, Arc, AtomicUsize, Ordering};

pub(super) fn reserved_after(used: usize, requested: usize, capacity: usize) -> Option<usize> {
    requested.checked_add(used).filter(|next| *next <= capacity)
}

#[derive(Debug)]
pub(super) struct BlockAdmissionBudget {
    capacity: usize,
    used: AtomicUsize,
}

impl BlockAdmissionBudget {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: AtomicUsize::new(0),
        }
    }

    pub(super) fn capacity(&self) -> usize { self.capacity }

    pub(super) fn try_reserve(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Result<BlockAdmissionReservation, usize> {
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
                    record_admission_bytes(next);
                    return Ok(BlockAdmissionReservation {
                        bytes,
                        budget: self.clone(),
                    });
                }
                Err(observed) => used = observed,
            }
        }
    }

    pub(super) fn used(&self) -> usize { self.used.load(Ordering::Acquire) }
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
        record_admission_bytes(previous.saturating_sub(self.bytes));
    }
}
