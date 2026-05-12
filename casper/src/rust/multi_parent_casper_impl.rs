//! Shim — `MultiParentCasperImpl` and the public event helpers have moved
//! to [`crate::rust::casper_engine`] as part of Phase 3 of the slashing-audit
//! remediation. This module re-exports them at the legacy path so that
//! external callers in `node/`, `casper/tests/`, and
//! `blocks/proposer/proposer.rs` keep compiling without source changes.

pub use crate::rust::casper_engine::events::{added_event, created_event, finalised_event};
pub use crate::rust::casper_engine::types::MultiParentCasperImpl;
