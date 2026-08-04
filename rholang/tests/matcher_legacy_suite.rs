//! Makes the ported Scala matcher corpus an actual Cargo integration target.
//!
//! The corpus historically lived under `tests/matcher/mod.rs`; Cargo does not
//! discover a directory module without a top-level test target, so none of its
//! semantic cases ran in an ordinary `cargo test`. Keep the source in its
//! existing focused directory and wire it explicitly here.

#[path = "matcher/match_test.rs"]
mod match_test;
