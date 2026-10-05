//! The frontend-neutral layer of the Reified RSpaces syntax (plan decisions 18 and 21).
//!
//! Each Reified construct is an `ETuple` whose head is an unforgeable 2-byte marker name
//! `GPrivate([0xE0, m])`. The constructs use only existing `Par` fields, so the protobuf
//! and bincode bytes of every ordinary term stay unchanged. The module has no parser
//! dependency: the tree-sitter normalizers and later frontends call the same builders.

pub mod builders;
pub mod encoding;
pub mod params;
pub mod query_spec;
pub mod validation;
