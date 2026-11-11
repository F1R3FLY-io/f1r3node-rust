//! Block ingest: takes a block from wherever it arrives (gossip, a reply to a
//! request, the local block store) until it is in the DAG.
//!
//! One owner holds every record of what the node holds and what it needs:
//! `IngestState` is that owner's state, a synchronous state machine with no
//! I/O. It composes the `DependencyTracker` (held blocks and what they wait
//! on), the `FetchScheduler` (blocks and state roots to fetch, and when) and
//! `PeerReputation` (peers ignored for pushing invalid blocks), and publishes
//! read-only answers through `IngestView`.

pub mod block_ingest;
pub mod checked_block;
pub mod dependency_tracker;
pub mod fetch_scheduler;
pub mod ingest_state;
pub mod ingest_step;
pub mod ingest_view;
pub mod intake_worker;
pub mod peer_reputation;
pub mod receipt_verifier;
pub mod validation_worker;

/// The target of every block ingest decision log, so one filter directive
/// (`f1r3fly.casper.block-ingest=debug`) shows a block's whole path.
pub const LOG_TARGET: &str = "f1r3fly.casper.block-ingest";

#[cfg(test)]
mod ingest_state_tests;
