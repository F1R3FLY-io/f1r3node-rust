use std::mem::size_of;

use super::{Arc, ChannelLockGuard, HeldLock};
use crate::rspace::errors::RSpaceError;

pub(crate) struct PreparedLocks<'a> {
    stripes: &'a [Arc<tokio::sync::Mutex<()>>],
    indices: Vec<usize>,
    held: Vec<HeldLock>,
}

fn reserve_vector<T>(
    count: usize,
    reserve: &impl Fn(usize, usize) -> Result<(), RSpaceError>,
) -> Result<Vec<T>, RSpaceError> {
    let operations = count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?;
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    reserve(operations, bytes)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(values)
}

pub(crate) fn prepare<'a>(
    stripes: &'a [Arc<tokio::sync::Mutex<()>>],
    keys: &[u64],
    reserve: impl Fn(usize, usize) -> Result<(), RSpaceError>,
) -> Result<PreparedLocks<'a>, RSpaceError> {
    if stripes.is_empty() {
        return Err(RSpaceError::InterpreterError("native lock stripes are empty".to_owned()));
    }
    let mut indices = reserve_vector::<usize>(keys.len(), &reserve)?;
    indices.extend(keys.iter().map(|key| (*key as usize) % stripes.len()));
    shared::rust::fallible_sort::sort(&mut indices, |left, right| {
        reserve(4, 0)?;
        Ok::<_, RSpaceError>(left.cmp(right))
    })?;
    reserve(indices.len(), 0)?;
    indices.dedup();
    let held = reserve_vector::<HeldLock>(indices.len(), &reserve)?;
    reserve(
        indices
            .len()
            .checked_mul(4)
            .ok_or(RSpaceError::HostWorkRejected)?,
        0,
    )?;
    Ok(PreparedLocks {
        stripes,
        indices,
        held,
    })
}

impl PreparedLocks<'_> {
    pub(crate) async fn acquire(mut self) -> ChannelLockGuard {
        for index in self.indices {
            let guard = match self.stripes[index].clone().try_lock_owned() {
                Ok(guard) => guard,
                Err(_) => self.stripes[index].clone().lock_owned().await,
            };
            self.held.push(HeldLock { _guard: guard });
        }
        ChannelLockGuard { _held: self.held }
    }
}

#[cfg(test)]
mod tests;
