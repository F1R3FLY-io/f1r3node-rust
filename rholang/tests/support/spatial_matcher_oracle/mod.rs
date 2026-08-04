//! Test-only recursive spatial-matcher oracle captured from `6799b406`.
//!
//! The production matcher is the explicit PDA. These modules preserve the
//! immediately preceding recursive implementation at a separate source
//! address so differential tests cannot accidentally call the subject twice.

#![allow(dead_code)]

pub mod exports {
    pub use crate::rust::interpreter::matcher::exports::*;
}

pub mod list_match {
    pub use crate::rust::interpreter::matcher::list_match::*;
}

pub mod match_pars {
    pub use crate::rust::interpreter::matcher::match_pars::*;
}

pub mod par_count {
    pub use crate::rust::interpreter::matcher::par_count::*;
}

pub mod sub_pars {
    pub use crate::rust::interpreter::matcher::sub_pars::*;
}

pub mod fold_match;
pub mod has_locally_free;
pub mod spatial_matcher;
mod epathmap_match;
