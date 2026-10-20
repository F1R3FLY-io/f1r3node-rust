// See models/src/main/scala/coop/rchain/models/rholang/sorter/ReceiveSortMatcher.scala
//
// Leg-2 Stage C-2: see `sort_drive` / `sort_combine`.

// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::par_sort_matcher::ParSortMatcher;
// use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::score_tree::ScoredTerm;
use super::sort_drive::{sort_bind_ref, sort_receive};
use super::sortable::Sortable;
// Changed by the merge of feature/cost-accounted-rho (DR-131): only the removed
// metered sorter used the names that this edit takes out of these imports.
// use super::var_sort_matcher::VarSortMatcher;
// use crate::rhoapi::{Par, Receive, ReceiveBind};
use crate::rhoapi::{Receive, ReceiveBind};

pub struct ReceiveSortMatcher;

impl ReceiveSortMatcher {
    /// Sort one bind.
    ///
    /// The by-value signature is retained because callers outside the sorter
    /// use it; the driver borrows, so the pre-conversion `r.binds.clone()` — a
    /// Θ(depth) `<Par as Clone>::clone` of every pattern and source, once per
    /// receive — is gone from the internal path.
    pub fn sort_bind(bind: ReceiveBind) -> ScoredTerm<ReceiveBind> { sort_bind_ref(&bind) }

    /// Sort one bind without taking ownership. This is what
    /// [`Sortable<Receive>::sort_match`] uses.
    pub fn sort_bind_by_ref(bind: &ReceiveBind) -> ScoredTerm<ReceiveBind> { sort_bind_ref(bind) }
}

impl Sortable<Receive> for ReceiveSortMatcher {
    // The order of the binds must already be presorted by the time this is called.
    // This function will then sort the insides of the preordered binds.
    fn sort_match(r: &Receive) -> ScoredTerm<Receive> { sort_receive(r) }
}
