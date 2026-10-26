use std::convert::Infallible;

use super::{Arc, ChannelLockGuard, HeldLock};
use crate::rspace::errors::RSpaceError;

pub(crate) struct PreparedLocks<'a> {
    stripes: &'a [Arc<tokio::sync::Mutex<()>>],
    indices: Vec<usize>,
    held: Vec<HeldLock>,
}

/// Changed by DR-131 S6: the preparation sorts and deduplicates the stripe
/// indices of `keys` and allocates the guard vector, with no host-work
/// reservation. The sort is the heap sort of `fallible_sort` (D-F).
pub(crate) fn prepare<'a>(
    stripes: &'a [Arc<tokio::sync::Mutex<()>>],
    keys: &[u64],
) -> Result<PreparedLocks<'a>, RSpaceError> {
    if stripes.is_empty() {
        return Err(RSpaceError::InterpreterError("native lock stripes are empty".to_owned()));
    }
    let mut indices = Vec::with_capacity(keys.len());
    indices.extend(keys.iter().map(|key| (*key as usize) % stripes.len()));
    let Ok(()) = shared::rust::fallible_sort::sort(&mut indices, |left, right| {
        Ok::<_, Infallible>(left.cmp(right))
    });
    indices.dedup();
    let held = Vec::with_capacity(indices.len());
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
