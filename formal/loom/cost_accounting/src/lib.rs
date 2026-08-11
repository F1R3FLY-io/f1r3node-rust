//! Loom shadow models for cost-accounting concurrency verification.
//!
//! This crate carries no production code; the models live in `tests/`:
//!   - `loom_concurrent_admission.rs` — two disjoint signature pools admit
//!     concurrently with no global lock / no lost update (CA-P-171, the Rust
//!     complement to TLA+ `EvalScheduling.tla:DisjointPoolsAdmitConcurrentlyNoGlobalLock`).
//!   - `loom_join_atomic.rs` — an N-ary join's combined token is debited exactly
//!     once or not at all under racing partial surface arrivals (CA-P-052/108,
//!     the Rust complement to TLA+ `TokenGatedJoin.tla:Inv_M1_AtomicNoPartialPrefix`).
//!
//! Under `RUSTFLAGS="--cfg loom"` loom explores ALL thread interleavings
//! exhaustively; under plain `cargo test` each `loom::model` runs once.
