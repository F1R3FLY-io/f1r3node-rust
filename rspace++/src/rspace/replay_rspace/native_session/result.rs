use std::convert::Infallible;
use std::sync::Arc;

use shared::rust::fallible_sort::sort;

use crate::rspace::internal::ConsumeCandidate;
use crate::rspace::rspace_interface::RSpaceResult;

pub(super) struct PreparedData<C, A> {
    pub data: Vec<RSpaceResult<C, A>>,
    pub retirement: Vec<(usize, i32)>,
}

/// Changed by DR-131 S6: the results of a match and the retirement of their
/// stored, non-persistent data, with no host-work reservation. The
/// retirement is in descending datum index, then ascending position; the
/// sort is the heap sort of `fallible_sort` (D-F).
pub(super) fn prepare<C, A: Clone>(candidates: Vec<ConsumeCandidate<C, A>>) -> PreparedData<C, A> {
    let count = candidates.len();
    let mut data = Vec::with_capacity(count);
    let mut retirement = Vec::with_capacity(count);
    for (position, candidate) in candidates.into_iter().enumerate() {
        if candidate.datum_index >= 0 && !candidate.datum.persist {
            retirement.push((position, candidate.datum_index));
        }
        data.push(RSpaceResult {
            channel: candidate.channel,
            matched_datum: Arc::unwrap_or_clone(candidate.datum.a),
            removed_datum: Arc::unwrap_or_clone(candidate.removed_datum),
            persistent: candidate.datum.persist,
        });
    }
    let Ok(()) = sort(&mut retirement, |a, b| {
        Ok::<_, Infallible>(b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)))
    });
    PreparedData { data, retirement }
}

#[cfg(test)]
mod tests;
