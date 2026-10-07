//! The native spaces of Reified RSpaces (P4, P5): the namespace, the agent interface, the space
//! errors, the records and counters, the generic space with its outer stores and disciplines,
//! the family agent, the space types, and the factory parts.
//!
//! A native space keeps its state in the host store under the `0xE0` namespace. Every space
//! operation goes through the substrate `SpaceStoreHandle` and `SpaceTxn`.

pub mod agent;
pub mod allocator;
pub mod counter;
pub mod disciplines;
pub mod errors;
pub mod factory;
pub mod families;
pub mod generic;
pub mod join;
pub mod matching;
pub mod namespace;
pub mod outer;
pub mod pathmap;
pub mod record;
pub mod settle;
pub mod types;
