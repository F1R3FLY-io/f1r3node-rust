// See models/src/main/scala/coop/rchain/models/rholang/sorter/ReceiveSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

use super::score_tree::ScoredTerm;
use super::sort_drive::{sort_bind_ref, sort_receive};
use super::sortable::Sortable;
use crate::rhoapi::{Receive, ReceiveBind};

pub struct ReceiveSortMatcher;

impl ReceiveSortMatcher {
    /// Sort one bind.
    ///
    /// The by-value signature is retained because callers outside the sorter
    /// use it; the driver borrows, so the pre-conversion `r.binds.clone()` — a
    /// Θ(depth) `<Par as Clone>::clone` of every pattern and source, once per
    /// receive — is gone from the internal path.
    pub fn sort_bind(bind: ReceiveBind) -> ScoredTerm<ReceiveBind> {
        sort_bind_ref(&bind)
    }

    /// Sort one bind without taking ownership. This is what
    /// [`Sortable<Receive>::sort_match`] uses.
    pub fn sort_bind_by_ref(bind: &ReceiveBind) -> ScoredTerm<ReceiveBind> {
        sort_bind_ref(bind)
    }
}

impl Sortable<Receive> for ReceiveSortMatcher {
    // The order of the binds must already be presorted by the time this is called.
    // This function will then sort the insides of the preordered binds.
    fn sort_match(r: &Receive) -> ScoredTerm<Receive> {
        sort_receive(r)
    }
}
