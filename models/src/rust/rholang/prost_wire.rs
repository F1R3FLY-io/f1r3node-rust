//! # `prost_wire` — the PROTOBUF schema alphabet
//!
//! The hand-written half of the protobuf table. Its generated twin —
//! `<TY>_PROST_PROGRAM`, one per message, and `<ONEOF>_PROST_VARIANTS`, one per
//! oneof — is emitted by `models/build/wire_schema.rs` into
//! `OUT_DIR/rhoapi_prost_wire.rs` and included by
//! [`crate::rust::rholang::prost_wire_schema`].
//!
//! This is the exact counterpart of [`crate::rust::rholang::wire`], which holds
//! the same split for the **bincode** table. The two are deliberately separate
//! types over one shared walk; see `models/build/wire_schema.rs`'s header for why
//! sharing the *order* would be a defect rather than a simplification.
//!
//! ---
//!
//! ## ⚠★ §A  This table's order is NOT the bincode table's
//!
//! `prost-derive-0.14.3/src/lib.rs:87-92` sorts a message's fields by
//! **ascending minimum tag** before building `encode_raw` and `encoded_len`:
//!
//! ```text
//!   // Sort the fields by tag number so that fields will be encoded in tag order.
//!   // TODO: This encodes oneof fields in the position of their lowest tag,
//!   // regardless of the currently occupied variant, is that consequential?
//!   fields.sort_by_key(|(_, field)| field.tags().into_iter().min().unwrap());
//! ```
//!
//! while `Debug` is built from the **unsorted** list (`:85, :214`). Two orders
//! inside one derive. Over the 57 generated messages exactly two differ from
//! declaration order:
//!
//! | message | declaration order (serde / bincode / `Debug`) | prost encode order |
//! |---|---|---|
//! | `Par` | 1,2,4,5,6,7,**11**,8,**12**,9,10 | 1,2,4,5,6,7,8,9,10,11,12 |
//! | `TaggedContinuation` | **3** (`guard`), **1** (oneof) | 1 (oneof), 3 (`guard`) |
//!
//! ⚠⚠ `TaggedContinuation` is the message whose *serde* order already cost this
//! campaign a 95-byte encoding with its halves exchanged — same length, same
//! byte multiset, different order, invisible to a length check and to a
//! round-trip. For the protobuf wire the correct order is **the opposite of that
//! fix**. A table that reused one order for both formats would reproduce the
//! defect in mirror, on the hottest type in the schema.
//!
//! ## §B  Two asymmetries against the bincode alphabet, both load-bearing
//!
//! 1. **`locally_free` is NOT blanked here.** `models/build.rs` injects
//!    `serialize_with = serialize_as_empty_bytes` on every `locally_free` field,
//!    so bincode writes eight zero bytes and the bincode alphabet has a
//!    dedicated `FieldKind::EmptyBytes` for it. That is a **serde-only**
//!    normalization: prost RETAINS the field, which
//!    `models/tests/wire_encode_differential.rs` pins by requiring a ground and
//!    a non-ground `EPathMap` twin to differ under `prost::Message::encode_to_vec`
//!    and agree entry-for-entry otherwise. There is therefore no `EmptyBytes` in
//!    [`ProstKind`], and the generator maps those fields to [`ProstKind::Bytes`].
//!
//! 2. **`sint32`/`sint64` are ZIGZAG here and plain here-they-are-not.** The
//!    bincode alphabet folds `int32`/`sint32`/`sfixed32` into one `I32` kind,
//!    correctly: serde sees an `i32` whatever the `.proto` said, because
//!    `sint32` is a *protobuf* encoding that never reaches bincode. On the
//!    protobuf wire it is `(n << 1) ^ (n >> 31)`. `RhoTypes.proto` has five
//!    `sint32` fields and one `sint64`, so the fold is not hypothetical, and
//!    [`ProstKind`] keeps every protobuf scalar type distinct.
//!
//! ## §C  ★ The layout is prost's own, called — never restated
//!
//! Every bounded field is written by `prost::encoding::<module>::encode` and
//! measured by `prost::encoding::<module>::encoded_len` — the same functions
//! `prost-derive` emits calls to. [`ProstKind`] names the module; it does not
//! reimplement varint, zigzag, or length delimiting. Only the **recursion** is
//! replaced.
//!
//! That is the same discipline `wire.rs` §A2 arrived at for bincode, for the
//! same reason: a generator that restated a byte format is a second opinion
//! about consensus, and the two opinions drift.
//!
//! ## §D  ⚠ `EPathMap` is an OPAQUE LEAF here, unlike on the bincode side
//!
//! `wire.rs` gives `EPathMap` a hand-written four-field program, because its
//! `Serialize` is an ordinary positional struct serialization of a canonical
//! `ps` projection. Its **protobuf** encoding is not:
//! `EPathMap::encode_raw` (`models/src/rust/rhoapi_ext.rs`) has three arms —
//!
//! * a `memcpy` of the interned canonical bytes, when the shadow cell is filled;
//! * `encode_ground_field8(U(m))`, an entirely different field, for a non-empty
//!   ground map;
//! * the ordinary field walk.
//!
//! Only the last is a field walk at all, and which arm fires depends on a
//! `OnceLock` another thread may fill between two reads. So this table gives
//! `EPathMap` **no program**: a prost driver treats it as one opaque node whose
//! bytes are `Message::encode_raw` and whose length is `Message::encoded_len` —
//! exact parity with what `prost::encoding::message::encode` does at that
//! position, and with what `prost::Message::encode_to_vec` does at the root.
//!
//! ⚠ That is a NAMED RESIDUAL, not a silent one: a term nested through an
//! `EPathMap` recurses inside `EPathMap::encode_raw` exactly as the derived path
//! does. It is correct (byte-identical) and it is not depth-independent, and the
//! two statements are separate.

// ===========================================================================
// §1  The protobuf field alphabet
// ===========================================================================

/// The protobuf encoding of one field — which `prost::encoding` module writes
/// it, and whether reading it can descend into an arbitrarily deep child.
///
/// ⚠ **Closed on purpose, and finer-grained than [`crate::rust::rholang::wire::FieldKind`].**
/// The bincode alphabet may fold `int32` and `sint32` together because serde
/// never sees a protobuf encoding; this one may not, because they are different
/// bytes. Every protobuf scalar type in `RhoTypes.proto` has its own variant,
/// and the generator *panics* on anything it cannot classify rather than
/// widening to the nearest neighbour — "nearest neighbour" in a byte format is a
/// consensus fork.
///
/// | kind | protobuf wire | descends? |
/// |---|---|---|
/// | `Bool` | key(Varint) ++ varint, **skipped at `false`** | no |
/// | `Int32` / `Int64` / `Uint32` / `Uint64` | key(Varint) ++ varint, skipped at 0 | no |
/// | `Sint32` / `Sint64` | key(Varint) ++ **zigzag** varint, skipped at 0 | no |
/// | `Fixed32` / `Sfixed32` / `Float` | key(32-bit) ++ 4 LE, skipped at 0 | no |
/// | `Fixed64` / `Sfixed64` / `Double` | key(64-bit) ++ 8 LE, skipped at 0 | no |
/// | `String` / `Bytes` | key(LD) ++ varint len ++ payload, skipped when empty | no |
/// | `RepeatedString` / `RepeatedBytes` | one key ++ len ++ payload PER element | no |
/// | `Message` | key(LD) ++ varint len ++ body, **omitted when `None`** | yes |
/// | `RepeatedMessage` | one key ++ len ++ body PER element | yes |
/// | `MapStringMessage` | one length-delimited entry per pair | yes |
/// | `Oneof` | the ARM's own tag ++ its payload, omitted when `None` | yes |
///
/// ⚠★ **Skip-at-default is part of the encoding**, not an optimization.
/// `prost-derive`'s `Kind::Plain` arm emits `if #ident != #default { … }` in
/// BOTH `encode` and `encoded_len` (`prost-derive-0.14.3/src/field/scalar.rs:
/// 116-125, 172-189`). A driver that applied it in one pass and not the other
/// would write a length prefix that disagrees with the bytes that follow, which
/// is a corrupt encoding rather than a slow one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProstKind {
    Bool,
    Int32,
    Sint32,
    Sfixed32,
    Uint32,
    Fixed32,
    Int64,
    Sint64,
    Sfixed64,
    Uint64,
    Fixed64,
    Float,
    Double,
    String,
    Bytes,
    RepeatedString,
    RepeatedBytes,
    Message,
    RepeatedMessage,
    /// The single `map<string, Par>` in the schema (`New.injections`).
    ///
    /// ⚠ A protobuf map is **not** a repeated value. Each pair is a
    /// length-delimited synthetic *entry message* carrying the key at tag 1 and
    /// the value at tag 2, and BOTH are skipped at their defaults
    /// (`prost-0.14.3/src/encoding.rs:1044-1059`). ★ The value's default test is
    /// `val == &Par::default()`, which goes through the **hand-written**
    /// `PartialEq` in `models/src/lib.rs` — and that impl deliberately ignores
    /// `locally_free`, so a `Par` carrying only `locally_free` IS skipped. Any
    /// driver must call the same `==` rather than restate the predicate.
    MapStringMessage,
    /// The oneof field. It occupies its message's slot at the position of its
    /// LOWEST member tag, but each ARM writes the tag it actually declares
    /// (`prost-derive-0.14.3/src/lib.rs:462-471`).
    Oneof,
}

impl ProstKind {
    /// Whether writing this field can descend into an arbitrarily deep child —
    /// i.e. whether a driver must suspend at it.
    #[inline]
    pub const fn descends(self) -> bool {
        matches!(
            self,
            ProstKind::Message
                | ProstKind::RepeatedMessage
                | ProstKind::MapStringMessage
                | ProstKind::Oneof
        )
    }
}

/// One field of one message's protobuf program.
///
/// The generated `<TY>_PROST_PROGRAM` slices are in **ascending minimum tag**
/// order — see §A. `tag` is the field's own tag, except for a [`ProstKind::Oneof`]
/// where it is the MINIMUM member tag (the sort key); the arm writes its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProstField {
    /// The proto tag. ⚠ For a oneof this is the SORT KEY, not what is written.
    pub tag: u32,
    pub kind: ProstKind,
    /// The Rust field name, without prost's `r#` keyword escape — for
    /// diagnostics and for the conformance probe.
    pub name: &'static str,
}
