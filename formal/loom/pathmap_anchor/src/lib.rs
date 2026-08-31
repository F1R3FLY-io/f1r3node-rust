//! Loom refinement models for anchored, branch-safe PathMap ownership.
//!
//! The executable shadow model lives in `tests/anchor_schedules.rs`. It checks
//! the inter-thread ownership schedules that complement the sequential Rocq and
//! Verus teardown proofs. Production PathMap topology is intentionally absent:
//! the model is solely about `Arc<AnchorRoot<_>>` authority, final-owner
//! election, fresh-anchor registration, and exactly-once PDA invocation.
