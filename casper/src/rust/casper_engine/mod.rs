//! `casper_engine` — decomposed home of `MultiParentCasperImpl`.
//!
//! Phase 3 of the slashing-audit remediation split the 1,900-line
//! `multi_parent_casper_impl.rs` god-object into nine sub-modules
//! (Layout C of the second Plan-agent design):
//!
//! * [`types`] — `pub struct MultiParentCasperImpl` + module constants.
//! * [`dispatch`] — `impl Casper` + `impl MultiParentCasper` (thin delegates).
//! * [`snapshot`] — `compute_snapshot`, `get_on_chain_state`,
//!   `record_dag_cardinality_metrics`, `estimator`.
//! * [`validation_dispatcher`] — `validate`, `validate_self_created`,
//!   `handle_invalid_block`.
//! * [`block_admission`] — `contains`, `dag_contains`, `buffer_contains`,
//!   `get_approved_block`, `deploy`, `handle_valid_block`, `add_deploy`.
//! * [`buffer_resolver`] — `get_dependency_free_from_buffer`,
//!   `get_all_from_buffer`.
//! * [`finalization_runner`] — `compute_last_finalized_block`,
//!   `run_queued_finalizer`, `update_last_finalized_block`.
//! * [`events`] — `block_event`, `created_event`, `added_event`,
//!   `finalised_event`, `pending_deploy_is_future_for_next_block`.
//!
//! External callers reach the public surface (`MultiParentCasperImpl`,
//! the three event helpers) either through this module's explicit
//! re-exports or via the legacy `multi_parent_casper_impl` shim, which
//! itself re-exports from here.

pub mod block_admission;
pub mod buffer_resolver;
pub mod events;
pub mod finalization_runner;
pub mod snapshot;
pub mod dispatch;
pub mod types;
pub mod validation_dispatcher;

// Phase 7 (C-1): explicit re-exports replace the previously transitional
// glob `pub use crate::rust::multi_parent_casper_impl::*;` which formed a
// circular alias path (engine → shim → engine).
pub use events::{added_event, created_event, finalised_event};
pub use types::MultiParentCasperImpl;
