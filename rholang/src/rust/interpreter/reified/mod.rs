//! The Reified RSpaces layer of the reducer (P4): the runtime, routing, use-context, system
//! channels, costs and errors, the R7 backend, and the factory, handle and system processes.
//! P8 adds the spaces that user agents implement (`user_agent`).
//!
//! One file of this directory is a child module of another file, so this module does not
//! declare it: `reducer_hooks.rs` is `reduce::reified_hooks`.
//!
//! Changed by decision 90 (DR-132, decision 69): `schedule.rs`, the child module
//! `deterministic_reduction::reified_schedule`, is deleted with the deterministic scheduler, and
//! `legacy_charges.rs` (decision 45, Option P) is deleted with the legacy meter. Restore either
//! file with `git show 6bf10a721:rholang/src/rust/interpreter/reified/<file>`.

pub mod costs;
pub mod errors;
pub mod execution;
pub mod factory;
pub mod handle;
// Deleted by decision 90 (decision 69): Option P is gone.
// pub mod legacy_charges;
// Retired by decision 114: the mirror is an oracle store, and nothing repairs it (File I/O
// oracle mode). The file `mirror_reconciler.rs` stays unchanged, and nothing of it compiles.
// To restore it, undo each change with a decision 114 note in `rholang`, `node` and the tests.
// pub mod mirror_reconciler;
pub mod route;
pub mod runtime;
pub mod scope;
pub mod seq;
pub mod system_channels;
pub mod system_processes;
pub mod user_agent;
pub mod vectordb_services;
