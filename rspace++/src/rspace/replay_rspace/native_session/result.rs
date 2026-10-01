use std::mem::size_of;

use shared::rust::fallible_sort::sort;

use crate::rspace::errors::RSpaceError;
use crate::rspace::internal::ConsumeCandidate;
use crate::rspace::rspace_interface::RSpaceResult;

pub(super) struct PreparedData<C, A> {
    pub data: Vec<RSpaceResult<C, A>>,
    pub retirement: Vec<(usize, i32)>,
}

fn backing<C, A>(count: usize) -> Result<usize, RSpaceError> {
    count
        .checked_mul(size_of::<RSpaceResult<C, A>>())
        .and_then(|results| {
            count
                .checked_mul(size_of::<(usize, i32)>())
                .and_then(|retirement| results.checked_add(retirement))
        })
        .ok_or(RSpaceError::HostWorkRejected)
}

pub(super) fn prepare<C, A: Clone>(
    candidates: Vec<ConsumeCandidate<C, A>>,
    reserve: impl Fn(usize, usize) -> Result<(), RSpaceError>,
) -> Result<PreparedData<C, A>, RSpaceError> {
    let count = candidates.len();
    let operations = count.checked_mul(3).ok_or(RSpaceError::HostWorkRejected)?;
    reserve(operations, backing::<C, A>(count)?)?;
    let mut data = Vec::new();
    let mut retirement = Vec::new();
    data.try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    retirement
        .try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    for (position, candidate) in candidates.into_iter().enumerate() {
        if candidate.datum_index >= 0 && !candidate.datum.persist {
            retirement.push((position, candidate.datum_index));
        }
        data.push(RSpaceResult {
            channel: candidate.channel,
            matched_datum: candidate.datum.a,
            removed_datum: candidate.removed_datum,
            persistent: candidate.datum.persist,
        });
    }
    sort(&mut retirement, |a, b| -> Result<_, RSpaceError> {
        reserve(1, 0)?;
        Ok(b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)))
    })?;
    Ok(PreparedData { data, retirement })
}

#[cfg(test)]
mod tests;
