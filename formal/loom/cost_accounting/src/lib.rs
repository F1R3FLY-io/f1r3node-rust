//! Loom shadow models for cost-accounting concurrency verification.
//!
//! This crate carries no production code; the models live in `tests/`:
//!   - `loom_concurrent_admission.rs` — two disjoint signature pools admit
//!     concurrently with no global lock / no lost update (CA-P-171, the Rust
//!     complement to TLA+ `EvalScheduling.tla:DisjointPoolsAdmitConcurrentlyNoGlobalLock`).
//!   - `loom_join_atomic.rs` — an N-ary join's combined token is debited exactly
//!     once or not at all under racing partial surface arrivals (CA-P-052/108,
//!     the Rust complement to TLA+ `TokenGatedJoin.tla:Inv_M1_AtomicNoPartialPrefix`).
//!   - `loom_block_heap_lifecycle.rs` — concurrent completion boundaries preserve
//!     semantic commits while enforcing the configured allocator-reclamation cadence.
//!   - `loom_parallel_validator_publication.rs` — shared current-root churn cannot
//!     authorize validator capture or publication, floor tuples are atomic, and
//!     distinct validator promotions commute.
//!   - `loom_multi_shard_resource_isolation.rs` — shard-local ledgers conserve
//!     deposits under concurrent top-up and charge while shared workers retain
//!     unique bounded ownership.
//!
//! Under `RUSTFLAGS="--cfg loom"` loom explores ALL thread interleavings
//! exhaustively; under plain `cargo test` each `loom::model` runs once.
