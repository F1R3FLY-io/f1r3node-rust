//! Crash-durable physical publication formats and state machines.
//!
//! The semantic authority remains the canonical Merkle DAG. These records
//! identify which verified root is current and where its immutable objects are
//! stored; they never redefine RSpace semantics.

mod catalog;
mod error;
mod index;
mod layout;
mod pack;

pub use catalog::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CatalogPackStateV1, CatalogSelectionV1, CatalogSlotV1,
};
pub use error::DurableFormatError;
pub(crate) use index::PreparedIndexAppendV1;
pub use index::{
    IndexFormatError, IndexGenerationV1, ObjectIndexFileViewV1, ObjectIndexHeaderV1,
    ObjectIndexRecordIter, ObjectIndexRecordV1, ObjectLocationV1,
};
pub use layout::{INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, PACK_HEADER_LEN_V1};
pub use pack::{PackFormatError, PackHeaderV1, PackTerminalIdentityV1};
