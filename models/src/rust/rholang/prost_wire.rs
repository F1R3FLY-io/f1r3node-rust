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

// ===========================================================================
// §2  The object-safe traits the generated table implements
// ===========================================================================

use std::collections::BTreeMap;

use crate::rhoapi::Par;
use crate::rust::rhoapi_ext::EPathMap;

/// The `resume` value meaning **there is nothing to come back for**.
///
/// The generator knows each program's length statically, so it emits this
/// sentinel whenever the descending field was the node's LAST. It is the exact
/// counterpart of [`crate::rust::rholang::wire::NO_RESUME`], and it is a
/// SEPARATE constant with a separate derivation: the two emitters sort the same
/// fields differently, so a patch computed once — before the sort — would name
/// the wrong field for one of them.
pub const NO_RESUME: u16 = u16::MAX;

/// What a node's field walk ran into: nothing, or one arbitrarily deep child.
///
/// ⚠ Every descending variant carries its **`tag`**, which the bincode
/// [`crate::rust::rholang::wire::Descent`] has no need of. On the protobuf wire
/// a child is preceded by `key(tag, LengthDelimited)` and a varint length, and
/// for a oneof the tag is the ARM's, not the field's.
pub enum ProstDescent<'a> {
    /// The program is spent. ★ No resume point is pushed for this — a node's
    /// last field is a tail call, and so is a sequence's last element.
    Done,
    /// One child message: an `Option<Message>` field, or a oneof's message arm.
    ///
    /// For a oneof arm, `tag` is the **arm's own** declared tag.
    Node {
        resume: u16,
        tag: u32,
        node: &'a dyn ProstNode,
    },
    /// A non-empty repeated message field. Every element is written under the
    /// same `tag`, each with its own key and length prefix
    /// (`prost-0.14.3/src/encoding.rs:820-827`).
    Seq {
        resume: u16,
        tag: u32,
        len: usize,
        seq: &'a dyn ProstSeq,
    },
    /// The single `map<string, Par>` in the schema (`New.injections`).
    Map {
        resume: u16,
        tag: u32,
        map: &'a BTreeMap<String, Par>,
    },
}

/// A schema node on the protobuf wire.
///
/// Object-safe by construction — no associated constants, no generic methods —
/// because the driver's op stack holds `&dyn ProstNode`.
pub trait ProstNode {
    /// This node's fields, in ASCENDING MINIMUM TAG order.
    fn prost_program(&self) -> &'static [ProstField];

    /// **Measure** fields `[from..]` until the first descent.
    ///
    /// Returns `(bounded length contributed, the descent)`. The length is
    /// returned rather than accumulated through an `&mut` so that the caller can
    /// hold its frame stack borrowed across the call without aliasing.
    ///
    /// ⚠★ **Skip-at-default must be applied here exactly as in
    /// [`Self::prost_emit`].** `prost-derive` emits `if #ident != #default` in
    /// BOTH `encode` and `encoded_len` (`src/field/scalar.rs:116-125, 172-189`),
    /// and the two passes of the driver disagreeing about one `bool` would write
    /// a length prefix that does not match the bytes after it — a corrupt
    /// encoding, not a slow one. Both bodies are generated from the same field
    /// list by the same renderer, so the two cannot drift.
    fn prost_len_step(&self, from: usize) -> (u64, ProstDescent<'_>);

    /// **Emit** fields `[from..]` until the first descent.
    ///
    /// ★ ONE virtual call per node per suspension, not one per field — the same
    /// shape, and for the same measured reason, as
    /// [`crate::rust::rholang::wire::WireNode::wire_emit`] (see `wire.rs` §A2:
    /// the per-field factoring was 1.7× slower than the derive).
    fn prost_emit(&self, from: usize, out: &mut Vec<u8>) -> ProstDescent<'_>;

    /// ⚠ **The one node whose protobuf bytes are NOT a field walk.**
    ///
    /// `EPathMap` overrides this; every generated impl inherits `None`. See §D.
    ///
    /// ★ It is a *trait method* and not a pointer comparison against a program
    /// address, for the reason that cost a `SIGSEGV` on the bincode side:
    /// `&'static` slices with identical contents are **merged by the linker**,
    /// so program addresses do not identify a type.
    /// `wire_encode_space::program_addresses_do_not_identify_a_type` keeps that
    /// fact executable.
    #[inline]
    fn prost_opaque(&self) -> Option<&dyn ProstOpaque> {
        None
    }
}

/// A node whose protobuf encoding is **not** a field walk, and which is
/// therefore atomic to any driver.
///
/// ⚠ The two methods are a PAIR and must describe the same bytes: the driver
/// writes `opaque_encoded_len()` as a length prefix and then
/// `opaque_encode_raw()` as the body. That is the same obligation
/// `prost::Message` itself carries between `encoded_len` and `encode_raw`, and
/// the implementations here forward to exactly those, so it is parity rather
/// than a second opinion.
pub trait ProstOpaque {
    fn opaque_encoded_len(&self) -> usize;
    fn opaque_encode_raw(&self, out: &mut Vec<u8>);
}

/// A oneof on the protobuf wire.
///
/// ⚠★ **A oneof arm is ALWAYS written, even at its default.**
/// `prost-derive`'s `scalar::Field::new_oneof` rewrites `Kind::Plain` into
/// `Kind::Required` (`src/field/scalar.rs:92-106`), and the `Required` arm of
/// `encode`/`encoded_len` carries no `if #ident != #default` guard. That is
/// protobuf's presence semantics: a set-but-default oneof member must be
/// distinguishable from an absent one. A driver that inherited the plain-field
/// skip rule here would silently erase `GBool(false)`, `GInt(0)` and
/// `GString("")`.
pub trait ProstOneof {
    /// The bounded length this arm contributes, and — for a message arm — the
    /// child together with the tag THE ARM declares.
    fn prost_len_step(&self) -> (u64, Option<(u32, &dyn ProstNode)>);

    /// Write the bounded arm; report a message arm for the driver to descend
    /// into. ⚠ The key and length prefix of a message arm are written by the
    /// DRIVER, which is the only place that knows the child's length.
    fn prost_emit(&self, out: &mut Vec<u8>) -> Option<(u32, &dyn ProstNode)>;
}

/// A homogeneous sequence of nodes, erased.
///
/// One blanket impl covers every `Vec<T>` in the schema, so a new repeated field
/// needs no new code here at all.
pub trait ProstSeq {
    fn prost_len(&self) -> usize;
    fn prost_get(&self, i: usize) -> &dyn ProstNode;
}

impl<T: ProstNode> ProstSeq for Vec<T> {
    #[inline]
    fn prost_len(&self) -> usize {
        self.len()
    }
    #[inline]
    fn prost_get(&self, i: usize) -> &dyn ProstNode {
        &self[i]
    }
}

// ===========================================================================
// §3  ⚠ `EPathMap` — the OPAQUE leaf
// ===========================================================================

/// `EPathMap`'s protobuf program: **empty**, because it has none.
///
/// This is not "a message with no fields" — `WildcardMsg` is that, and encodes
/// to zero bytes by walking zero fields. `EPathMap` has four proto fields and
/// **three encodings**, and which one it uses is not a property of its fields.
/// See §D of this module's header. The empty program is what makes
/// [`ProstNode::prost_len_step`] and [`ProstNode::prost_emit`] unreachable for
/// it, and they say so rather than returning a plausible answer.
pub static EPATHMAP_PROST_PROGRAM: &[ProstField] = &[];

impl ProstOpaque for EPathMap {
    /// ⚠ `prost::Message::encoded_len`, verbatim — including its three arms.
    #[inline]
    fn opaque_encoded_len(&self) -> usize {
        prost::Message::encoded_len(self)
    }

    /// ⚠ `prost::Message::encode_raw`, verbatim — including its three arms.
    ///
    /// ★ Reading the cell TWICE (once for the length, once for the body) is the
    /// same exposure the derived path has: `prost::Message::encode_to_vec` calls
    /// `encoded_len()` and then `encode_raw()`, and another thread can fill the
    /// intern cell between them. All three arms are gated to produce the same
    /// bytes for a given value (the P0 prost goldens), so this is parity, not a
    /// new hazard — but it is why `EPathMap` may not be decomposed into field
    /// descents by a driver: a driver that measured under one arm and emitted
    /// under another would write a length prefix that does not match its body.
    #[inline]
    fn opaque_encode_raw(&self, out: &mut Vec<u8>) {
        prost::Message::encode_raw(self, out)
    }
}

impl ProstNode for EPathMap {
    #[inline]
    fn prost_program(&self) -> &'static [ProstField] {
        EPATHMAP_PROST_PROGRAM
    }

    #[inline]
    fn prost_opaque(&self) -> Option<&dyn ProstOpaque> {
        Some(self)
    }

    fn prost_len_step(&self, _from: usize) -> (u64, ProstDescent<'_>) {
        unreachable!(
            "EPathMap has no protobuf field program — its `encode_raw` has three arms, of \
             which only one is a field walk. A driver must intercept it through \
             `ProstNode::prost_opaque` and treat it as ONE node. Decomposing it here would \
             silently drop the interned-bytes and ground-`U(m)` arms, which changes the \
             event-hash preimage."
        )
    }

    fn prost_emit(&self, _from: usize, _out: &mut Vec<u8>) -> ProstDescent<'_> {
        unreachable!(
            "EPathMap has no protobuf field program — see `prost_len_step`."
        )
    }
}
