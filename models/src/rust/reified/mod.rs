//! The frontend-neutral layer of the Reified RSpaces syntax (plan decisions 18 and 21).
// Changed by decision 112: each construct has a protobuf field. The text before:
// ```
// //! Each Reified construct is an `ETuple` whose head is an unforgeable 2-byte marker name
// //! `GPrivate([0xE0, m])`. The constructs use only existing `Par` fields, so the protobuf
// //! and bincode bytes of every ordinary term stay unchanged. The module has no parser
// //! dependency: the tree-sitter normalizers and later frontends call the same builders.
// ```
//!
//! Each Reified construct has a protobuf field or message of `Par`, `Send`, `ReceiveBind`, `New`
//! or `Expr` (decision 112). Ordinary terms keep their prost bytes, and their bincode grows
//! once. The module has no parser dependency: the tree-sitter normalizers and later frontends
//! call the same builders.

pub mod builders;
pub mod encoding;
pub mod params;
pub mod query_spec;
pub mod validation;
