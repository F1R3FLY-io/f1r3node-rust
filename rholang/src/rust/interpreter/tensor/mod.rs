//! The exact numeric core of the VectorDB family (P6, design v3 4.1; decision 54).
//!
//! - `num`: the exact numbers, the exact vectors and the correctly rounded `f64` values;
//! - `exact`: the kernels, the scores, the bounds and the exact order of the built-in metrics;
//! - `costs`: the host-work reservations of both (decision 42).

pub mod costs;
pub mod exact;
pub mod num;
