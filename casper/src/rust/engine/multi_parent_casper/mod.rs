//! `engine::multi_parent_casper` — decomposed home of `MultiParentCasperImpl`.
//!
//! Phase 3 of the slashing-audit remediation split the 1,900-line
//! `multi_parent_casper_impl.rs` god-object into nine sub-modules
//! (Layout C of the second Plan-agent design). The merge of `dev` into
//! `feature/slashing` subsequently renamed the parent directory from
//! `casper_engine/` to `engine/multi_parent_casper/` to eliminate the
//! confusing parallel-`engine`-modules layout and deleted the
//! transitional `multi_parent_casper_impl.rs` re-export shim.
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
//! the three event helpers) through this module's explicit re-exports.
//! The historical `multi_parent_casper_impl.rs` shim is gone.

// C15 / Arch-1: encapsulate the sub-modules at crate scope. The
// intended external API is the explicit `pub use` re-exports below
// (`MultiParentCasperImpl` and the three event helpers); without
// this tightening, any `pub` item inside any sub-module was
// reachable as `engine::multi_parent_casper::<sub_module>::<item>` from outside
// the crate, defeating the encapsulation intent of the
// nine-module decomposition.
pub(crate) mod block_admission;
pub(crate) mod buffer_resolver;
pub(crate) mod events;
pub(crate) mod finalization_runner;
pub(crate) mod snapshot;
pub(crate) mod dispatch;
pub(crate) mod types;
pub(crate) mod validation_dispatcher;

// Phase 7 (C-1): explicit re-exports replace the previously transitional
// glob `pub use crate::rust::engine::multi_parent_casper::*;` which formed a
// circular alias path (engine → shim → engine).
pub use events::{added_event, created_event, finalised_event};
pub use types::MultiParentCasperImpl;
