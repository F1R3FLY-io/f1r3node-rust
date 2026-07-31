pub mod drive;
pub mod grpc_client;
pub mod implicits;
pub mod par_children;
pub mod pooled_stack;
pub mod bincode_decoder;
pub mod protobuf_encoder;
pub mod prost_wire;
pub mod schema_meta;
pub mod sorter;
pub mod wire;
pub mod bincode_encoder;

// ---------------------------------------------------------------------------
// ★ THE FIVE GENERATED MODULES — ONE build-script pass, five outputs
//
// `models/build/wire_schema.rs` resolves every `rhoapi` message ONCE and emits
// five files into `OUT_DIR`; `models/build.rs` writes each one. All five are
// included here, including the one that is currently empty, so the pipeline a
// later stage fills is exercised from the stage that built it: five files
// written, five modules compiled, one walk of the descriptor.
//
// ⚠★ The two field tables are the SAME resolved fields under TWO SORT KEYS —
// `identity` for serde/bincode, `sort_by_key(min_tag)` for protobuf — and the
// two orders genuinely differ for `Par` and `TaggedContinuation`. See
// `models/build/wire_schema.rs`'s header; the second of those is the message
// whose serde order already cost this campaign a 95-byte encoding with its
// halves exchanged, and for protobuf the correct order is the OPPOSITE of that
// fix.
// ---------------------------------------------------------------------------

/// The GENERATED **bincode** wire-schema table — one `impl WireNode` per
/// message, one `impl WireOneof` per oneof, and the oneof index tables.
///
/// ★ ONE table, BOTH directions: [`bincode_encoder`] drives the serializer from
/// it and [`bincode_decoder`] drives the deserializer from it. The variant counts
/// here are `.len()` of the generated tables, never literals, so a new oneof
/// arm cannot leave an assertion passing while it goes untested.
///
/// ⚠ Its bytes are a FIXED POINT: the cold store already holds byte strings
/// written by the encoding it drives.
#[allow(clippy::all, unused_imports)]
pub mod wire_schema {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_wire.rs"));
}

/// The GENERATED **protobuf** field table — one `<TY>_PROST_PROGRAM` per
/// message in ASCENDING MINIMUM TAG order, one `<ONEOF>_PROST_VARIANTS` per
/// oneof carrying the tag each ARM writes.
///
/// Its alphabet ([`prost_wire::ProstField`], [`prost_wire::ProstKind`]) is
/// hand-written; see that module for the two asymmetries against the bincode
/// alphabet that a shared table would have erased — `locally_free` is NOT
/// blanked on the protobuf wire, and `sint32`/`sint64` are zigzag.
#[allow(clippy::all, unused_imports)]
pub mod prost_wire_schema {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_prost_wire.rs"));
}

/// The GENERATED **term-op** slot.
///
/// ⚠ EMPTY at present, deliberately, and included all the same. The term-op
/// drivers belong to a later stage; the module exists now so that "four files
/// written, four modules compiled" is a property the build already has rather
/// than one a later stage would have to establish. A file emitted but not
/// included would be a slot nobody had proved reachable; a placeholder constant
/// would be a stub.
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
/// ⚠ At stage S0 it carries exactly ONE item, `skip_unknown_field`, and
/// **nothing calls it**. It is the iterative re-implementation of
/// `prost::encoding::skip_field`, whose `StartGroup` arm self-recurses once per
/// nesting level over a depth the SENDER chooses: proto3 has no groups, so no
/// message this node writes can carry wire type 3, but skipping an *unknown*
/// field is a walk over bytes a peer supplied and is bounded by nothing the
/// schema says. See the generated file's own header for the exactness contract
/// against prost and for why its two error values are minted by calling prost
/// rather than written out.
///
/// ★ It is GENERATED rather than checked in because 57 `rhoapi` messages are
/// regenerated from the descriptor on every build, and S1's per-message decode
/// arms must be derived from that same walk or they drift from the derive in
/// silence. Starting the deserializer as a renderer means S1 extends one; a
/// checked-in file would have to become one.
///
/// ⚠★ Named for the WIRE, never for the crate: `prost` is one implementation of
/// the protobuf codec and is swappable, so a `prost_*` module path would bake a
/// swappable dependency into the namespace. The module path, the `OUT_DIR` file
/// name and the renderer's name all say `protobuf`. Its *signature* necessarily
/// names prost's `WireType`, `Buf` and `DecodeError` — that is inherent, since
/// agreeing with prost's accept set byte-for-byte is the whole contract.
#[allow(clippy::all, unused_imports)]
pub mod protobuf_decoder {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_protobuf_decoder.rs"));
}
