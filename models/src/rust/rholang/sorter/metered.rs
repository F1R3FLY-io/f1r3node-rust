use std::cell::Cell;
use std::mem::size_of;

use shared::rust::clone_backing::{self, BackingError, BackingMeter, CloneBacking};

pub struct SorterMeter<'a> {
    backing: &'a dyn BackingMeter,
}

thread_local! {
    static SORT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub struct SortDepthGuard;

impl Drop for SortDepthGuard {
    fn drop(&mut self) { SORT_DEPTH.with(|depth| depth.set(depth.get() - 1)); }
}

impl BackingMeter for SorterMeter<'_> {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self.backing.reserve(operations, scanned, backing)
    }
}

impl<'a> SorterMeter<'a> {
    pub fn new(backing: &'a dyn BackingMeter) -> Self { Self { backing } }

    pub fn enter(&self) -> Result<SortDepthGuard, BackingError> {
        self.reserve(1, size_of::<usize>(), 0)?;
        SORT_DEPTH.with(|depth| {
            let next = depth.get().checked_add(1).ok_or(BackingError::Overflow)?;
            if next > 256 {
                return Err(BackingError::Rejected);
            }
            depth.set(next);
            Ok(SortDepthGuard)
        })
    }

    pub fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self.backing.reserve(operations, scanned, backing)
    }

    pub fn clone<T: Clone + CloneBacking>(&self, value: &T) -> Result<T, BackingError> {
        clone_backing::reserve(value, self.backing)?;
        Ok(value.clone())
    }

    pub fn inspect<T: CloneBacking>(&self, value: &T) -> Result<(), BackingError> {
        clone_backing::inspect(value, self.backing)
    }

    pub fn clone_slice<T: Clone + CloneBacking>(
        &self,
        values: &[T],
    ) -> Result<Vec<T>, BackingError> {
        clone_backing::reserve_slice(values, self.backing)?;
        Ok(values.to_vec())
    }

    pub fn vec<T>(&self, capacity: usize) -> Result<Vec<T>, BackingError> {
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .ok_or(BackingError::Overflow)?;
        self.backing.reserve(capacity, 0, bytes)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(capacity)
            .map_err(|_| BackingError::Allocation)?;
        Ok(values)
    }

    pub fn push<T>(&self, values: &mut Vec<T>, value: T) -> Result<(), BackingError> {
        if values.len() == values.capacity() {
            let next = values
                .len()
                .checked_mul(2)
                .and_then(|n| n.checked_add(1))
                .ok_or(BackingError::Overflow)?;
            let bytes = next
                .checked_mul(size_of::<T>())
                .ok_or(BackingError::Overflow)?;
            let scanned = values
                .len()
                .checked_mul(size_of::<T>())
                .ok_or(BackingError::Overflow)?;
            self.backing.reserve(1, scanned, bytes)?;
            values
                .try_reserve_exact(next - values.len())
                .map_err(|_| BackingError::Allocation)?;
        }
        values.push(value);
        Ok(())
    }
}
