//! Crash-durable physical publication formats and state machines.
//!
//! The semantic authority remains the canonical Merkle DAG. These records
//! identify which verified root is current and where its immutable objects are
//! stored; they never redefine RSpace semantics.

mod catalog;
mod error;

pub use catalog::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CatalogPackStateV1, CatalogSelectionV1, CatalogSlotV1,
};
pub use error::DurableFormatError;
