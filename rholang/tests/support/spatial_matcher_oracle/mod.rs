//! Test-only recursive spatial-matcher oracle captured from `6799b406`.
//!
//! The production matcher is the explicit PDA. These modules preserve the
//! immediately preceding recursive implementation at a separate source
//! address so differential tests cannot accidentally call the subject twice.

#![allow(dead_code)]

pub mod exports {
    pub use crate::rust::interpreter::matcher::exports::*;
}

pub mod list_match;

pub mod match_pars {
    pub use crate::rust::interpreter::matcher::match_pars::*;
}

pub mod par_count {
    pub use crate::rust::interpreter::matcher::par_count::*;
}

pub mod sub_pars {
    use models::rhoapi::Par;

    use super::par_count::ParCount;
    // Changed by the merge of feature/cost-accounted-rho (finding C5): the
    // unmetered `sub_pars` takes no work token, so no matcher context is
    // needed.
    // use crate::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

    /// The recursive semantic oracle deliberately shares the production
    /// subset enumeration, as it did before the metered matcher gained its
    /// private work token. Only the recursive search/control flow is compared.
    pub fn sub_pars(
        par: &Par,
        min: &ParCount,
        max: &ParCount,
        min_prune: &ParCount,
        max_prune: &ParCount,
    ) -> impl Iterator<Item = (Par, Par)> {
        // Changed by the merge of feature/cost-accounted-rho (finding C5):
        // DR-131 removed the work token. The production enumeration returns
        // `None` only when a count does not fit `isize`, and it yields the
        // candidate pairs directly.
        // let context = SpatialMatcherContext::new();
        // crate::rust::interpreter::matcher::sub_pars::sub_pars(
        //     par,
        //     min,
        //     max,
        //     min_prune,
        //     max_prune,
        //     context.work().expect("unmetered matcher work"),
        // )
        // .expect("unmetered subset enumeration")
        // .map(|pair| pair.expect("unmetered candidate copy"))
        crate::rust::interpreter::matcher::sub_pars::sub_pars(par, min, max, min_prune, max_prune)
            .expect("every count of the subset enumeration fits isize")
    }
}

pub mod fold_match;
pub mod has_locally_free;
pub mod spatial_matcher;
mod epathmap_match;
