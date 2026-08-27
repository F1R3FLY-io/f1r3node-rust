//! Fixed widths shared by physical publication codecs.
//!
//! These are protocol constants, not Rust structure sizes. Every format is
//! encoded field-by-field and remains independent of ABI padding or target
//! pointer width.

pub const PACK_HEADER_LEN_V1: u64 = 128;
pub const INDEX_HEADER_LEN_V1: u64 = 64;
pub const INDEX_RECORD_LEN_V1: u64 = 72;
