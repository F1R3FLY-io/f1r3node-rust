//! The Reified RSpaces layer of the reducer (P4): the runtime, routing, use-context, system
//! channels, costs and errors, the R7 backend, and the factory, handle and system processes.
//!
//! Two files of this directory are child modules of other files, so this module does not
//! declare them. `reducer_hooks.rs` is `reduce::reified_hooks`, and `schedule.rs` is
//! `deterministic_reduction::reified_schedule`.

pub mod costs;
pub mod errors;
pub mod execution;
pub mod factory;
pub mod handle;
pub mod route;
pub mod runtime;
pub mod scope;
pub mod system_channels;
pub mod system_processes;
