//! # `protobuf_schema` — the protobuf schema alphabet
//!
//! The hand-written half of the protobuf table. Its generated twin —
//! `<TY>_PROTOBUF_PROGRAM`, one per message, and `<ONEOF>_PROTOBUF_VARIANTS`, one per
//! oneof — is emitted by `models/codegen/schema_codegen.rs` into
//! `OUT_DIR/rhoapi_protobuf_schema.rs` and included by
//! [`crate::rust::rholang::protobuf_schema_tables`].
//!
//! This is the exact counterpart of [`crate::rust::rholang::bincode_schema`], which holds
//! the same split for the **bincode** table. The two are deliberately separate
//! types over one shared walk; see `models/codegen/schema_codegen.rs`'s header for why
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
//!    `models/tests/bincode_encoder_differential.rs` pins by requiring a ground and
//!    a non-ground `EPathMap` twin to differ under `prost::Message::encode_to_vec`
//!    and agree entry-for-entry otherwise. There is therefore no `EmptyBytes` in
//!    [`ProtobufKind`], and the generator maps those fields to [`ProtobufKind::Bytes`].
//!
//! 2. **`sint32`/`sint64` are ZIGZAG here and plain here-they-are-not.** The
//!    bincode alphabet folds `int32`/`sint32`/`sfixed32` into one `I32` kind,
//!    correctly: serde sees an `i32` whatever the `.proto` said, because
//!    `sint32` is a *protobuf* encoding that never reaches bincode. On the
//!    protobuf wire it is `(n << 1) ^ (n >> 31)`. `RhoTypes.proto` has five
//!    `sint32` fields and one `sint64`, so the fold is not hypothetical, and
//!    [`ProtobufKind`] keeps every protobuf scalar type distinct.
//!
//! ## §C  ★ The layout is prost's own, called — never restated
//!
//! Every bounded field is written by `prost::encoding::<module>::encode` and
//! measured by `prost::encoding::<module>::encoded_len` — the same functions
//! `prost-derive` emits calls to. [`ProtobufKind`] names the module; it does not
//! reimplement varint, zigzag, or length delimiting. Only the **recursion** is
//! replaced.
//!
//! That is the same discipline `bincode_schema.rs` §A2 arrived at for bincode, for the
//! same reason: a generator that restated a byte format is a second opinion
//! about consensus, and the two opinions drift.
//!
//! ## §D  `EPathMap` has a hand-written field program
//!
//! `EPathMap` is `extern_path`'d from Prost generation, so the descriptor emitter
//! cannot write its implementation. Its current encoding is nevertheless one
//! deterministic ascending-tag walk: metadata at tags 3/4/5 and the trie's byte
//! representation at tag 9. The hand-written program below mirrors that walk.
//! The tag-9 payload is emitted by a dedicated descent so `PathMap<Par>` values
//! remain inside the same generated PDA instead of recursively constructing
//! nested byte snapshots.

// ===========================================================================
// §1  The protobuf field alphabet
// ===========================================================================

/// The protobuf encoding of one field — which `prost::encoding` module writes
/// it, and whether reading it can descend into an arbitrarily deep child.
///
/// ⚠ **Closed on purpose, and finer-grained than [`crate::rust::rholang::bincode_schema::FieldKind`].**
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
/// | `EPathMapSnapshot` | EPM1 topology plus length-delimited `Par` values | yes |
/// | `Oneof` | the ARM's own tag ++ its payload, omitted when `None` | yes |
///
/// ⚠★ **Skip-at-default is part of the encoding**, not an optimization.
/// `prost-derive`'s `Kind::Plain` arm emits `if #ident != #default { … }` in
/// BOTH `encode` and `encoded_len` (`prost-derive-0.14.3/src/field/scalar.rs:
/// 116-125, 172-189`). A driver that applied it in one pass and not the other
/// would write a length prefix that disagrees with the bytes that follow, which
/// is a corrupt encoding rather than a slow one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtobufKind {
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
    /// The external `EPathMap.trie_snapshot` bytes field. Its EPM1 header and
    /// ACTree03 topology are bounded, but map-mode values are protobuf `Par`
    /// bodies and therefore participate in the generated traversal.
    EPathMapSnapshot,
    /// The oneof field. It occupies its message's slot at the position of its
    /// LOWEST member tag, but each ARM writes the tag it actually declares
    /// (`prost-derive-0.14.3/src/lib.rs:462-471`).
    Oneof,
}

impl ProtobufKind {
    /// Whether writing this field can descend into an arbitrarily deep child —
    /// i.e. whether a driver must suspend at it.
    #[inline]
    pub const fn descends(self) -> bool {
        matches!(
            self,
            ProtobufKind::Message
                | ProtobufKind::RepeatedMessage
                | ProtobufKind::MapStringMessage
                | ProtobufKind::EPathMapSnapshot
                | ProtobufKind::Oneof
        )
    }
}

/// One field of one message's protobuf program.
///
/// The generated `<TY>_PROTOBUF_PROGRAM` slices are in **ascending minimum tag**
/// order — see §A. `tag` is the field's own tag, except for a [`ProtobufKind::Oneof`]
/// where it is the MINIMUM member tag (the sort key); the arm writes its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtobufField {
    /// The proto tag. ⚠ For a oneof this is the SORT KEY, not what is written.
    pub tag: u32,
    pub kind: ProtobufKind,
    /// The Rust field name, without prost's `r#` keyword escape — for
    /// diagnostics and for the conformance probe.
    pub name: &'static str,
}

// ===========================================================================
// §2  The object-safe traits the generated table implements
// ===========================================================================

use std::collections::BTreeMap;

use prost::bytes::BufMut;

use crate::rhoapi::Par;
use crate::rust::rhoapi_ext::EPathMap;

/// The `resume` value meaning **there is nothing to come back for**.
///
/// The generator knows each program's length statically, so it emits this
/// sentinel whenever the descending field was the node's LAST. It is the exact
/// counterpart of [`crate::rust::rholang::bincode_schema::NO_RESUME`], and it is a
/// SEPARATE constant with a separate derivation: the two emitters sort the same
/// fields differently, so a patch computed once — before the sort — would name
/// the wrong field for one of them.
pub const NO_RESUME: u16 = u16::MAX;

/// What a node's field walk ran into: nothing, or one arbitrarily deep child.
///
/// ⚠ Every descending variant carries its **`tag`**, which the bincode
/// [`crate::rust::rholang::bincode_schema::Descent`] has no need of. On the protobuf wire
/// a child is preceded by `key(tag, LengthDelimited)` and a varint length, and
/// for a oneof the tag is the ARM's, not the field's.
pub enum ProtobufDescent<'a> {
    /// The program is spent. ★ No resume point is pushed for this — a node's
    /// last field is a tail call, and so is a sequence's last element.
    Done,
    /// One child message: an `Option<Message>` field, or a oneof's message arm.
    ///
    /// For a oneof arm, `tag` is the **arm's own** declared tag.
    Node {
        resume: u16,
        tag: u32,
        node: &'a dyn ProtobufNode,
    },
    /// A non-empty repeated message field. Every element is written under the
    /// same `tag`, each with its own key and length prefix
    /// (`prost-0.14.3/src/encoding.rs:820-827`).
    Seq {
        resume: u16,
        tag: u32,
        len: usize,
        seq: &'a dyn ProtobufSeq,
    },
    /// The single `map<string, Par>` in the schema (`New.injections`).
    Map {
        resume: u16,
        tag: u32,
        map: &'a BTreeMap<String, Par>,
    },
    /// The EPM1 bytes field of an external `EPathMap`. The encoder writes the
    /// cached PathMap topology, then visits map values with zipper cursors in
    /// the same length/emit order as every other child message.
    EPathMapSnapshot {
        resume: u16,
        tag: u32,
        map: &'a EPathMap,
    },
}

/// A schema node on the protobuf wire.
///
/// Object-safe by construction — no associated constants, no generic methods —
/// because the driver's op stack holds `&dyn ProtobufNode`.
pub trait ProtobufNode {
    /// This node's fields, in ASCENDING MINIMUM TAG order.
    fn protobuf_program(&self) -> &'static [ProtobufField];

    /// **Measure** fields `[from..]` until the first descent.
    ///
    /// Returns `(bounded length contributed, the descent)`. The length is
    /// returned rather than accumulated through an `&mut` so that the caller can
    /// hold its frame stack borrowed across the call without aliasing.
    ///
    /// ⚠★ **Skip-at-default must be applied here exactly as in
    /// [`Self::protobuf_emit`].** `prost-derive` emits `if #ident != #default` in
    /// BOTH `encode` and `encoded_len` (`src/field/scalar.rs:116-125, 172-189`),
    /// and the two passes of the driver disagreeing about one `bool` would write
    /// a length prefix that does not match the bytes after it — a corrupt
    /// encoding, not a slow one. Both bodies are generated from the same field
    /// list by the same renderer, so the two cannot drift.
    fn protobuf_len_step(&self, from: usize) -> (u64, ProtobufDescent<'_>);

    /// **Emit** fields `[from..]` until the first descent.
    ///
    /// ★ ONE virtual call per node per suspension, not one per field — the same
    /// shape, and for the same measured reason, as
    /// [`crate::rust::rholang::bincode_schema::BincodeNode::bincode_emit`] (see `bincode_schema.rs` §A2:
    /// the per-field factoring was 1.7× slower than the derive).
    fn protobuf_emit(&self, from: usize, out: &mut dyn BufMut) -> ProtobufDescent<'_>;
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
pub trait ProtobufOneof {
    /// The bounded length this arm contributes, and — for a message arm — the
    /// child together with the tag THE ARM declares.
    fn protobuf_len_step(&self) -> (u64, Option<(u32, &dyn ProtobufNode)>);

    /// Write the bounded arm; report a message arm for the driver to descend
    /// into. ⚠ The key and length prefix of a message arm are written by the
    /// DRIVER, which is the only place that knows the child's length.
    fn protobuf_emit(&self, out: &mut dyn BufMut) -> Option<(u32, &dyn ProtobufNode)>;
}

/// A homogeneous sequence of nodes, erased.
///
/// One blanket impl covers every `Vec<T>` in the schema, so a new repeated field
/// needs no new code here at all.
pub trait ProtobufSeq {
    fn protobuf_len(&self) -> usize;
    fn protobuf_get(&self, i: usize) -> &dyn ProtobufNode;
}

impl<T: ProtobufNode> ProtobufSeq for Vec<T> {
    #[inline]
    fn protobuf_len(&self) -> usize { self.len() }
    #[inline]
    fn protobuf_get(&self, i: usize) -> &dyn ProtobufNode { &self[i] }
}

// ===========================================================================
// §3  `EPathMap` — the hand-written external program
// ===========================================================================

pub static EPATHMAP_PROTOBUF_PROGRAM: &[ProtobufField] = &[
    ProtobufField {
        tag: 3,
        kind: ProtobufKind::Bytes,
        name: "locally_free",
    },
    ProtobufField {
        tag: 4,
        kind: ProtobufKind::Bool,
        name: "connective_used",
    },
    ProtobufField {
        tag: 5,
        kind: ProtobufKind::Message,
        name: "remainder",
    },
    ProtobufField {
        tag: 9,
        kind: ProtobufKind::EPathMapSnapshot,
        name: "trie_snapshot",
    },
];

impl ProtobufNode for EPathMap {
    #[inline]
    fn protobuf_program(&self) -> &'static [ProtobufField] { EPATHMAP_PROTOBUF_PROGRAM }

    fn protobuf_len_step(&self, from: usize) -> (u64, ProtobufDescent<'_>) {
        let mut n = 0u64;
        if from < 1 && !self.locally_free.is_empty() {
            n += prost::encoding::bytes::encoded_len(3u32, &self.locally_free) as u64;
        }
        if from < 2 && self.connective_used {
            n += prost::encoding::bool::encoded_len(4u32, &self.connective_used) as u64;
        }
        if from < 3 {
            if let Some(remainder) = &self.remainder {
                return (n, ProtobufDescent::Node {
                    resume: 3,
                    tag: 5,
                    node: remainder,
                });
            }
        }
        if from < 4 {
            return (n, ProtobufDescent::EPathMapSnapshot {
                resume: NO_RESUME,
                tag: 9,
                map: self,
            });
        }
        (n, ProtobufDescent::Done)
    }

    fn protobuf_emit(&self, from: usize, out: &mut dyn BufMut) -> ProtobufDescent<'_> {
        if from < 1 && !self.locally_free.is_empty() {
            prost::encoding::bytes::encode(3u32, &self.locally_free, &mut &mut *out);
        }
        if from < 2 && self.connective_used {
            prost::encoding::bool::encode(4u32, &self.connective_used, &mut &mut *out);
        }
        if from < 3 {
            if let Some(remainder) = &self.remainder {
                return ProtobufDescent::Node {
                    resume: 3,
                    tag: 5,
                    node: remainder,
                };
            }
        }
        if from < 4 {
            return ProtobufDescent::EPathMapSnapshot {
                resume: NO_RESUME,
                tag: 9,
                map: self,
            };
        }
        ProtobufDescent::Done
    }
}
