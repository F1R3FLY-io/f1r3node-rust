pub mod drive;
pub mod grpc_client;
pub mod implicits;
pub mod par_children;
pub mod pooled_stack;
pub mod bincode_decoder;
pub mod protobuf_encoder;
pub mod protobuf_schema;
pub mod schema_meta;
pub mod sorter;
pub mod bincode_schema;
pub mod bincode_encoder;

// Measurement support is physically separated from production sources. The
// selected feature includes it into the library only so production entry points
// can report the shapes the interpreter corpus actually sends through them.
#[cfg(feature = "phase7-depth-histograms")]
#[path = "../../../tests/support/phase7_depth_histogram.rs"]
pub(crate) mod phase7_depth_histogram;

// ---------------------------------------------------------------------------
// ★ THE FIVE GENERATED MODULES — ONE build-script pass, five outputs
//
// `models/codegen/schema_codegen.rs` resolves every `rhoapi` message ONCE and emits
// five files into `OUT_DIR`; `models/build.rs` writes each one. All five are
// included here, including the one that is currently empty, so the pipeline a
// later stage fills is exercised from the stage that built it: five files
// written, five modules compiled, one walk of the descriptor.
//
// ⚠★ The two field tables are the SAME resolved fields under TWO SORT KEYS —
// `identity` for serde/bincode, `sort_by_key(min_tag)` for protobuf — and the
// two orders genuinely differ for `Par` and `TaggedContinuation`. See
// `models/codegen/schema_codegen.rs`'s header; the second of those is the message
// whose serde order already cost this campaign a 95-byte encoding with its
// halves exchanged, and for protobuf the correct order is the OPPOSITE of that
// fix.
// ---------------------------------------------------------------------------

/// The GENERATED **bincode** schema table — one `impl BincodeNode` per
/// message, one `impl BincodeOneof` per oneof, and the oneof index tables.
///
/// ★ ONE table, BOTH directions: [`bincode_encoder`] drives the serializer from
/// it and [`bincode_decoder`] drives the deserializer from it. The variant counts
/// here are `.len()` of the generated tables, never literals, so a new oneof
/// arm cannot leave an assertion passing while it goes untested.
///
/// ⚠ Its bytes are a FIXED POINT: the cold store already holds byte strings
/// written by the encoding it drives.
#[allow(clippy::all, unused_imports)]
pub mod bincode_schema_tables {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_bincode_schema.rs"));
}

/// The GENERATED **protobuf** field table — one `<TY>_PROTOBUF_PROGRAM` per
/// message in ASCENDING MINIMUM TAG order, one `<ONEOF>_PROTOBUF_VARIANTS` per
/// oneof carrying the tag each ARM writes.
///
/// Its alphabet ([`protobuf_schema::ProtobufField`], [`protobuf_schema::ProtobufKind`]) is
/// hand-written; see that module for the two asymmetries against the bincode
/// alphabet that a shared table would have erased — `locally_free` is NOT
/// blanked on the protobuf wire, and `sint32`/`sint64` are zigzag.
#[allow(clippy::all, unused_imports)]
pub mod protobuf_schema_tables {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_protobuf_schema.rs"));
}

/// The GENERATED **term-operation PDAs**, including the iterative implementations
/// selected by the derive-disposition and recursive-SCC analysis.
#[allow(clippy::all, unused_imports)]
pub mod term_ops {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_term_ops.rs"));
}

/// The GENERATED **schema meta**: the child relation, Tarjan's SCC of it, the
/// types that can contain themselves, and the DERIVE DISPOSITION REGISTRY.
///
/// ★ Computed in the same pass as the two field tables, from the same resolved
/// fields, so "which types can contain themselves" is a fact about the table the
/// drivers are generated from rather than a second reading of the `.proto`. See
/// [`schema_meta`] for why the registry is a lower bound and what carries the
/// remainder.
#[allow(clippy::all, unused_imports)]
pub mod schema_meta_tables {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_schema_meta.rs"));
}

/// The GENERATED **protobuf deserializer** — the read half of the protobuf
/// wire, the counterpart of [`protobuf_encoder`].
///
/// The descriptor pass emits the heterogeneous message `Node`/`Value`/`Attach`
/// machine and its iterative unknown-field skipper together, so field coverage
/// cannot drift from the generated `rhoapi` message closure. The skipper replaces
/// `prost::encoding::skip_field`, whose `StartGroup` arm self-recurses once per
/// nesting level over a depth the SENDER chooses: proto3 has no groups, so no
/// message this node writes can carry wire type 3, but skipping an *unknown*
/// field is a walk over bytes a peer supplied and is bounded by nothing the
/// schema says. See the generated file's own header for the exactness contract
/// against prost and for why its two error values are minted by calling prost
/// rather than written out.
///
/// It is generated rather than checked in because all `rhoapi` messages are
/// regenerated from the descriptor on every build; the per-message decode arms
/// must come from that same walk or they can drift from the schema in silence.
///
/// The repository-owned module and `OUT_DIR` file are named for the protobuf
/// format. Its signature necessarily uses Prost's `WireType`, `Buf`, and
/// `DecodeError`, because byte-for-byte agreement with that implementation is
/// the compatibility contract.
#[allow(clippy::all, unused_imports)]
pub mod protobuf_decoder {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_protobuf_decoder.rs"));
}
