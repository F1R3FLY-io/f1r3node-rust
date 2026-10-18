//! # `protobuf_schema` — the protobuf schema alphabet
//!
//! The hand-written half of the protobuf table. Its generated twin —
//! `<TY>_PROTOBUF_PROGRAM`, one per message, and `<ONEOF>_PROTOBUF_VARIANTS`, one per
//! oneof — is emitted by `models/codegen/schema.rs` into
//! `OUT_DIR/rhoapi_protobuf_schema.rs` and included by
//! [`crate::rust::rholang::protobuf_schema_tables`].
//!
//! This is the exact counterpart of [`crate::rust::rholang::bincode_schema`], which holds
//! the same split for the **bincode** table. The two are deliberately separate
//! types over one shared walk; see `models/codegen/schema.rs`'s header for why
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
//! ## §D  external PathMap-backed nodes have hand-written programs
//!
//! `Par` and `EPathMap` are `extern_path`'d from Prost generation, so the
//! descriptor emitter cannot write their implementations.  `Par` is an opaque,
//! non-descending node to this driver: every canonical component protobuf body
//! is already stored in its PathMap key, so one zipper/prefix walk copies those
//! bodies directly to the caller's buffer without decoding or recursion.
//! `EPathMap`'s current encoding is nevertheless one
//! deterministic ascending-tag walk: metadata at tags 3/4/5 and the trie's byte
//! representation at tag 9. The hand-written program below mirrors that walk.
//! The tag-9 payload is emitted by a dedicated descent so `PathMap<Par>` values
//! remain inside the same generated PDA instead of recursively constructing
//! nested byte snapshots.

// ===========================================================================
// §1  The protobuf field alphabet
// ===========================================================================

/// The four protobuf wire encodings used by ordinary fields.
///
/// Groups (wire codes 3 and 4) are deliberately absent: the Rholang schema
/// declares no group field, so accepting one would not prove a schema arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProtobufWireType {
    Varint = 0,
    Fixed64 = 1,
    LengthDelimited = 2,
    Fixed32 = 5,
}

impl ProtobufWireType {
    #[inline]
    pub const fn code(self) -> u64 { self as u64 }
}

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
    /// The selected arm's wire encoding, when this kind denotes one concrete
    /// protobuf field. An enclosing [`ProtobufKind::Oneof`] has no wire type of
    /// its own because the selected arm supplies both tag and encoding.
    #[inline]
    pub const fn wire_type(self) -> Option<ProtobufWireType> {
        match self {
            ProtobufKind::Bool
            | ProtobufKind::Int32
            | ProtobufKind::Sint32
            | ProtobufKind::Uint32
            | ProtobufKind::Int64
            | ProtobufKind::Sint64
            | ProtobufKind::Uint64 => Some(ProtobufWireType::Varint),
            ProtobufKind::Sfixed64 | ProtobufKind::Fixed64 | ProtobufKind::Double => {
                Some(ProtobufWireType::Fixed64)
            }
            ProtobufKind::String
            | ProtobufKind::Bytes
            | ProtobufKind::RepeatedString
            | ProtobufKind::RepeatedBytes
            | ProtobufKind::Message
            | ProtobufKind::RepeatedMessage
            | ProtobufKind::MapStringMessage
            | ProtobufKind::EPathMapSnapshot => Some(ProtobufWireType::LengthDelimited),
            ProtobufKind::Sfixed32 | ProtobufKind::Fixed32 | ProtobufKind::Float => {
                Some(ProtobufWireType::Fixed32)
            }
            ProtobufKind::Oneof => None,
        }
    }

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

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use pathmap::zipper::ZipperReadOnlyIteration;
use prost::bytes::BufMut;
use prost::encoding::{encode_key, encode_varint, encoded_len_varint, key_len, WireType};

use crate::rhoapi::Par;
use crate::rust::par::ParProtobufError;
use crate::rust::par_bag::ParBagOwnerKey;
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
    /// Identify the external `Par` feedback vertex to schema-driven consumers.
    ///
    /// The ordinary protobuf encoder deliberately treats `Par` as an opaque
    /// node.  Construction planning needs the complementary fact: a generated
    /// message edge that reaches this node is a replaceable `Par` slot.  The
    /// default keeps every generated implementation source-compatible; the
    /// hand-maintained external implementation below is the sole override.
    fn as_par(&self) -> Option<&Par> { None }

    /// Identify the external EPathMap feedback vertex.  A construction plan
    /// keeps an already-authoritative EPathMap opaque and delegates its EPM1
    /// snapshot to the production protobuf PDA instead of projecting entries.
    fn as_epathmap(&self) -> Option<&EPathMap> { None }

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
// §2a  Certified bounded egress — allocation-free for the first owner
// ===========================================================================

/// A descriptor-certified, statically dispatched protobuf egress node.
///
/// Implementations are generated only for a message graph that is acyclic
/// after `Par` and `EPathMap` have been registered as descriptor cut vertices.
/// Their hand-certified boundaries remain truthful: `Par` emits its canonical
/// trie bytes, while `EPathMap` covers its bounded `Var` remainder and every
/// `PathMap<Par>` value. Consequently native calls are bounded by
/// [`Self::MAX_SCHEMA_DEPTH`], a compile-time descriptor fact rather than a
/// run-time traversal limit. The general [`ProtobufNode`] PDA remains the
/// authority for arbitrary recursive schemas.
pub trait CertifiedEgressNode: ProtobufNode {
    /// Maximum number of statically dispatched message frames from this type
    /// through a hand-certified cut boundary, including both endpoints.
    const MAX_SCHEMA_DEPTH: usize;

    /// Exact Prost-compatible encoded length while observing each protobuf-
    /// visible `Par` occurrence exactly once.
    fn certified_egress_encoded_len(
        &self,
        preflight: &mut CertifiedEgressPreflight,
    ) -> Result<usize, CertifiedEgressError>;

    /// Emit with statically dispatched bounded calls. Every nested body length
    /// comes from the sealed pre-order transcript produced by the mandatory
    /// preflight. Emission never rescans a child merely to recover a protobuf
    /// prefix length.
    fn try_encode_certified_egress<B, E, F>(
        &self,
        output: &mut B,
        replay: &mut CertifiedEgressReplay<'_>,
        emit_par: &mut F,
    ) -> Result<(), CertifiedEgressEncodeError<E>>
    where
        B: BufMut,
        F: FnMut(&Par, usize, &mut B) -> Result<(), E>;
}

/// The only fallible facts in a certified egress length pass.
#[derive(Debug)]
pub enum CertifiedEgressError {
    /// A `Par`'s multiplicity or root metadata cannot be represented by the
    /// protobuf wire contract.
    Par(ParProtobufError),
    /// A sequence of individually valid field sizes overflowed `usize`.
    LengthOverflow { context: &'static str },
    /// A length cannot be represented by protobuf's `u64` varint length.
    LengthExceedsU64 { context: &'static str },
    /// A generated preflight attempted to complete a slot that does not exist.
    TranscriptSlotInvalid { context: &'static str, index: usize },
    /// A generated preflight attempted to complete a slot through a token for
    /// a different length-delimited shape.
    TranscriptSlotKindMismatch {
        context: &'static str,
        expected: CertifiedLengthEventKind,
        actual: CertifiedLengthEventKind,
    },
    /// A generated preflight attempted to complete the same slot twice.
    TranscriptSlotAlreadyCompleted { context: &'static str, index: usize },
    /// A preflight ended with one or more reserved body lengths unresolved.
    TranscriptIncomplete { remaining: usize },
    /// Emission visited more length-delimited bodies than preflight measured.
    TranscriptExhausted {
        context: &'static str,
        expected: CertifiedLengthEventKind,
    },
    /// Preflight and emission reached different length-delimited shapes at the
    /// same schema position. Equal body lengths cannot mask this disagreement.
    TranscriptKindMismatch {
        context: &'static str,
        expected: CertifiedLengthEventKind,
        actual: CertifiedLengthEventKind,
    },
    /// Emission did not consume every preflight transcript event.
    TranscriptRemaining { remaining: usize },
    /// The cached EPM1 layout and its PathMap value zipper disagreed. This
    /// cannot arise from an admitted `EntryTrie`; reporting it keeps the
    /// certified boundary total even if an internal invariant regresses.
    EPathMapValueCountMismatch {
        phase: &'static str,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for CertifiedEgressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Par(error) => write!(formatter, "{error}"),
            Self::LengthOverflow { context } => {
                write!(
                    formatter,
                    "protobuf encoded length overflow while measuring {context}"
                )
            }
            Self::LengthExceedsU64 { context } => write!(
                formatter,
                "protobuf encoded length for {context} exceeds the u64 wire domain"
            ),
            Self::TranscriptSlotInvalid { context, index } => write!(
                formatter,
                "certified protobuf preflight used invalid transcript slot {index} for {context}"
            ),
            Self::TranscriptSlotKindMismatch {
                context,
                expected,
                actual,
            } => write!(
                formatter,
                "certified protobuf preflight completed {context} as {actual:?}, but its slot was reserved as {expected:?}"
            ),
            Self::TranscriptSlotAlreadyCompleted { context, index } => write!(
                formatter,
                "certified protobuf preflight completed transcript slot {index} for {context} twice"
            ),
            Self::TranscriptIncomplete { remaining } => write!(
                formatter,
                "certified protobuf preflight left {remaining} transcript slots incomplete"
            ),
            Self::TranscriptExhausted { context, expected } => write!(
                formatter,
                "certified protobuf emission reached {context} ({expected:?}) after exhausting its preflight transcript"
            ),
            Self::TranscriptKindMismatch {
                context,
                expected,
                actual,
            } => write!(
                formatter,
                "certified protobuf emission reached {context} as {expected:?}, but preflight recorded {actual:?}"
            ),
            Self::TranscriptRemaining { remaining } => write!(
                formatter,
                "certified protobuf emission left {remaining} preflight transcript events unconsumed"
            ),
            Self::EPathMapValueCountMismatch {
                phase,
                expected,
                actual,
            } => write!(
                formatter,
                "certified EPathMap {phase} visited {actual} values but its EPM1 layout declares {expected}"
            ),
        }
    }
}

impl std::error::Error for CertifiedEgressError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Par(error) => Some(error),
            Self::LengthOverflow { .. }
            | Self::LengthExceedsU64 { .. }
            | Self::TranscriptSlotInvalid { .. }
            | Self::TranscriptSlotKindMismatch { .. }
            | Self::TranscriptSlotAlreadyCompleted { .. }
            | Self::TranscriptIncomplete { .. }
            | Self::TranscriptExhausted { .. }
            | Self::TranscriptKindMismatch { .. }
            | Self::TranscriptRemaining { .. }
            | Self::EPathMapValueCountMismatch { .. } => None,
        }
    }
}

impl From<ParProtobufError> for CertifiedEgressError {
    fn from(error: ParProtobufError) -> Self { Self::Par(error) }
}

/// A certified structural error or the caller's `Par` emission error.
#[derive(Debug)]
pub enum CertifiedEgressEncodeError<E> {
    Length(CertifiedEgressError),
    Par(E),
}

impl<E> From<CertifiedEgressError> for CertifiedEgressEncodeError<E> {
    fn from(error: CertifiedEgressError) -> Self { Self::Length(error) }
}

/// Semantic kind of one pre-measured length-delimited body.
///
/// Kinds are recorded independently of body length so equal-sized siblings
/// cannot hide a disagreement between the generated preflight and emission
/// programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertifiedLengthEventKind {
    MessageBody,
    ParBody,
    MapEntryBody,
    EPathMapSnapshotBody,
}

/// Opaque stable index returned when preflight reserves a parent prefix before
/// descending into that parent's body.
#[derive(Clone, Copy, Debug)]
pub struct CertifiedLengthSlot {
    index: usize,
    kind: CertifiedLengthEventKind,
}

/// Exact response-local owner-reuse facts collected during the mandatory
/// encoded-length walk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParOwnerCensusStatistics {
    /// Every `Par` field actually present on the protobuf wire, including
    /// component-empty values which are ineligible for component reuse.
    pub par_occurrences: usize,
    /// Present `Par`s whose component Bag is empty. They are counted for schema
    /// coverage but deliberately do not create owner records.
    pub component_empty_occurrences: usize,
    /// Present `Par`s with at least one component.
    pub eligible_occurrences: usize,
    /// Distinct CoW component owners among eligible occurrences.
    pub unique_owners: usize,
    /// Eligible occurrences after an owner's first occurrence.
    pub repeated_occurrences: usize,
    /// Owners observed at least twice.
    pub repeated_owners: usize,
    /// Bytes retained if every distinct eligible owner were prepared once.
    pub unique_component_bytes: usize,
    /// Bytes retained if only owners proved reusable were prepared.
    pub reusable_component_bytes: usize,
    /// Component bytes whose regeneration/copy can be avoided after the first
    /// occurrence of each reusable owner.
    pub repeated_component_bytes: usize,
}

impl ParOwnerCensusStatistics {
    #[inline]
    pub const fn has_reuse(self) -> bool { self.repeated_occurrences != 0 }
}

/// Exact per-owner facts for caller-selected memoization thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParOwnerReuseStatistics {
    pub occurrences: usize,
    pub component_bytes: usize,
    pub repeated_component_bytes: usize,
}

#[derive(Debug)]
struct OwnerObservation {
    key: ParBagOwnerKey,
    state: OwnerState,
}

#[derive(Debug)]
struct OwnerState {
    component_bytes: usize,
    occurrences: usize,
    repeated_component_bytes: usize,
}

/// An exact owner census with no heap allocation for zero or one distinct
/// eligible owner.
///
/// A repeated first owner also remains allocation-free. The `HashMap` is
/// created only when a second *distinct* eligible owner is observed, at which
/// point an exact associative spill is necessary to distinguish later reuse.
#[derive(Debug, Default)]
pub struct ParOwnerCensus {
    first_owner: Option<OwnerObservation>,
    owner_spill: Option<HashMap<ParBagOwnerKey, OwnerState>>,
    statistics: ParOwnerCensusStatistics,
}

impl ParOwnerCensus {
    #[inline]
    pub const fn statistics(&self) -> ParOwnerCensusStatistics { self.statistics }

    #[inline]
    pub fn has_owner_spill(&self) -> bool { self.owner_spill.is_some() }

    /// Number of eligible occurrences sharing `owner`. Component-empty Pars
    /// are deliberately absent. Callers use this to restrict range recording
    /// and memo lookup to owners the census proved will be reused.
    pub fn owner_occurrences(&self, owner: &ParBagOwnerKey) -> usize {
        self.owner_reuse_statistics(owner)
            .map_or(0, |statistics| statistics.occurrences)
    }

    #[inline]
    pub fn is_reusable_owner(&self, owner: &ParBagOwnerKey) -> bool {
        self.owner_occurrences(owner) > 1
    }

    /// Return exact reusable-work and retention facts without cloning the
    /// owner key or mutating census state.
    pub fn owner_reuse_statistics(
        &self,
        owner: &ParBagOwnerKey,
    ) -> Option<ParOwnerReuseStatistics> {
        let state = if let Some(spill) = &self.owner_spill {
            spill.get(owner)
        } else {
            self.first_owner
                .as_ref()
                .filter(|observation| &observation.key == owner)
                .map(|observation| &observation.state)
        }?;
        Some(ParOwnerReuseStatistics {
            occurrences: state.occurrences,
            component_bytes: state.component_bytes,
            repeated_component_bytes: state.repeated_component_bytes,
        })
    }

    fn observe_existing(
        statistics: &mut ParOwnerCensusStatistics,
        observation: &mut OwnerState,
    ) -> Result<(), CertifiedEgressError> {
        observation.occurrences =
            observation
                .occurrences
                .checked_add(1)
                .ok_or(CertifiedEgressError::LengthOverflow {
                    context: "Par owner occurrence census",
                })?;
        statistics.repeated_occurrences = statistics.repeated_occurrences.checked_add(1).ok_or(
            CertifiedEgressError::LengthOverflow {
                context: "Par repeated-occurrence census",
            },
        )?;
        statistics.repeated_component_bytes = statistics
            .repeated_component_bytes
            .checked_add(observation.component_bytes)
            .ok_or(CertifiedEgressError::LengthOverflow {
                context: "Par repeated component bytes",
            })?;
        observation.repeated_component_bytes = observation
            .repeated_component_bytes
            .checked_add(observation.component_bytes)
            .ok_or(CertifiedEgressError::LengthOverflow {
                context: "per-owner repeated component bytes",
            })?;
        if observation.occurrences == 2 {
            statistics.repeated_owners = statistics.repeated_owners.checked_add(1).ok_or(
                CertifiedEgressError::LengthOverflow {
                    context: "Par repeated-owner census",
                },
            )?;
            statistics.reusable_component_bytes = statistics
                .reusable_component_bytes
                .checked_add(observation.component_bytes)
                .ok_or(CertifiedEgressError::LengthOverflow {
                    context: "Par reusable component bytes",
                })?;
        }
        Ok(())
    }

    fn observe_new(
        statistics: &mut ParOwnerCensusStatistics,
        component_bytes: usize,
    ) -> Result<(), CertifiedEgressError> {
        statistics.unique_owners = statistics.unique_owners.checked_add(1).ok_or(
            CertifiedEgressError::LengthOverflow {
                context: "Par unique-owner census",
            },
        )?;
        statistics.unique_component_bytes = statistics
            .unique_component_bytes
            .checked_add(component_bytes)
            .ok_or(CertifiedEgressError::LengthOverflow {
                context: "Par unique component bytes",
            })?;
        Ok(())
    }
}

impl ParOwnerCensus {
    #[inline]
    fn observe_par(&mut self, value: &Par) -> Result<usize, CertifiedEgressError> {
        let measurement = value.protobuf_egress_measurement()?;
        let encoded_len = measurement.encoded_len;
        self.statistics.par_occurrences = self.statistics.par_occurrences.checked_add(1).ok_or(
            CertifiedEgressError::LengthOverflow {
                context: "Par occurrence census",
            },
        )?;

        let component_bytes = measurement.component_fields_encoded_len;
        if component_bytes == 0 {
            self.statistics.component_empty_occurrences = self
                .statistics
                .component_empty_occurrences
                .checked_add(1)
                .ok_or(CertifiedEgressError::LengthOverflow {
                    context: "component-empty Par occurrence census",
                })?;
            return Ok(encoded_len);
        }
        self.statistics.eligible_occurrences =
            self.statistics.eligible_occurrences.checked_add(1).ok_or(
                CertifiedEgressError::LengthOverflow {
                    context: "eligible Par occurrence census",
                },
            )?;

        let key = value.protobuf_owner_key();
        if let Some(spill) = &mut self.owner_spill {
            if let Some(observation) = spill.get_mut(&key) {
                Self::observe_existing(&mut self.statistics, observation)?;
                return Ok(encoded_len);
            }
            Self::observe_new(&mut self.statistics, component_bytes)?;
            spill.insert(key, OwnerState {
                component_bytes,
                occurrences: 1,
                repeated_component_bytes: 0,
            });
            return Ok(encoded_len);
        }

        match &mut self.first_owner {
            None => {
                Self::observe_new(&mut self.statistics, component_bytes)?;
                self.first_owner = Some(OwnerObservation {
                    key,
                    state: OwnerState {
                        component_bytes,
                        occurrences: 1,
                        repeated_component_bytes: 0,
                    },
                });
            }
            Some(first) if first.key == key => {
                Self::observe_existing(&mut self.statistics, &mut first.state)?;
            }
            Some(_) => {
                Self::observe_new(&mut self.statistics, component_bytes)?;
                let mut spill = HashMap::with_capacity(2);
                if let Some(first) = self.first_owner.take() {
                    spill.insert(first.key, first.state);
                }
                spill.insert(key, OwnerState {
                    component_bytes,
                    occurrences: 1,
                    repeated_component_bytes: 0,
                });
                self.owner_spill = Some(spill);
            }
        }
        Ok(encoded_len)
    }
}

/// The common case keeps a complete nested-length plan inline. Wide repeated
/// fields spill once into a geometrically growing vector; slot indexes remain
/// stable across that transition.
pub const CERTIFIED_EGRESS_INLINE_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug)]
struct CertifiedLengthEvent {
    body_len: usize,
    kind: CertifiedLengthEventKind,
    completed: bool,
}

const _: () = assert!(CERTIFIED_EGRESS_INLINE_EVENTS <= u16::BITS as usize);

#[derive(Debug)]
struct CertifiedLengthTranscriptBuilder {
    inline_body_lens: [usize; CERTIFIED_EGRESS_INLINE_EVENTS],
    inline_kinds: [CertifiedLengthEventKind; CERTIFIED_EGRESS_INLINE_EVENTS],
    inline_completed: u16,
    inline_count: u8,
    spill: Option<Vec<CertifiedLengthEvent>>,
}

impl Default for CertifiedLengthTranscriptBuilder {
    fn default() -> Self {
        Self {
            inline_body_lens: [0; CERTIFIED_EGRESS_INLINE_EVENTS],
            inline_kinds: [CertifiedLengthEventKind::MessageBody; CERTIFIED_EGRESS_INLINE_EVENTS],
            inline_completed: 0,
            inline_count: 0,
            spill: None,
        }
    }
}

impl CertifiedLengthTranscriptBuilder {
    #[inline]
    fn inline_event(&self, index: usize) -> Option<CertifiedLengthEvent> {
        if index >= usize::from(self.inline_count) {
            return None;
        }
        Some(CertifiedLengthEvent {
            body_len: self.inline_body_lens[index],
            kind: self.inline_kinds[index],
            completed: self.inline_completed & (1u16 << index) != 0,
        })
    }

    fn spill_with(&mut self, event: CertifiedLengthEvent) {
        let mut spill = Vec::with_capacity(CERTIFIED_EGRESS_INLINE_EVENTS * 2);
        for index in 0..usize::from(self.inline_count) {
            spill.push(
                self.inline_event(index)
                    .expect("occupied inline transcript slots are materialized"),
            );
        }
        spill.push(event);
        self.spill = Some(spill);
    }

    #[inline]
    fn reserve(
        &mut self,
        kind: CertifiedLengthEventKind,
    ) -> Result<CertifiedLengthSlot, CertifiedEgressError> {
        let event = CertifiedLengthEvent {
            body_len: 0,
            kind,
            completed: false,
        };
        let index = if let Some(spill) = &mut self.spill {
            let index = spill.len();
            let _ = index
                .checked_add(1)
                .ok_or(CertifiedEgressError::LengthOverflow {
                    context: "certified egress transcript event count",
                })?;
            spill.push(event);
            index
        } else if usize::from(self.inline_count) < CERTIFIED_EGRESS_INLINE_EVENTS {
            let index = usize::from(self.inline_count);
            self.inline_body_lens[index] = 0;
            self.inline_kinds[index] = kind;
            self.inline_count += 1;
            index
        } else {
            let index = CERTIFIED_EGRESS_INLINE_EVENTS;
            self.spill_with(event);
            index
        };
        Ok(CertifiedLengthSlot { index, kind })
    }

    #[inline]
    fn push_completed(
        &mut self,
        kind: CertifiedLengthEventKind,
        body_len: usize,
    ) -> Result<(), CertifiedEgressError> {
        let event = CertifiedLengthEvent {
            body_len,
            kind,
            completed: true,
        };
        if let Some(spill) = &mut self.spill {
            let _ = spill
                .len()
                .checked_add(1)
                .ok_or(CertifiedEgressError::LengthOverflow {
                    context: "certified egress transcript event count",
                })?;
            spill.push(event);
        } else if usize::from(self.inline_count) < CERTIFIED_EGRESS_INLINE_EVENTS {
            let index = usize::from(self.inline_count);
            self.inline_body_lens[index] = body_len;
            self.inline_kinds[index] = kind;
            self.inline_completed |= 1u16 << index;
            self.inline_count += 1;
        } else {
            self.spill_with(event);
        }
        Ok(())
    }

    #[inline]
    fn complete(
        &mut self,
        slot: CertifiedLengthSlot,
        body_len: usize,
        context: &'static str,
    ) -> Result<(), CertifiedEgressError> {
        if let Some(spill) = &mut self.spill {
            let event =
                spill
                    .get_mut(slot.index)
                    .ok_or(CertifiedEgressError::TranscriptSlotInvalid {
                        context,
                        index: slot.index,
                    })?;
            if event.kind != slot.kind {
                return Err(CertifiedEgressError::TranscriptSlotKindMismatch {
                    context,
                    expected: slot.kind,
                    actual: event.kind,
                });
            }
            if event.completed {
                return Err(CertifiedEgressError::TranscriptSlotAlreadyCompleted {
                    context,
                    index: slot.index,
                });
            }
            event.body_len = body_len;
            event.completed = true;
            return Ok(());
        }

        if slot.index >= usize::from(self.inline_count) {
            return Err(CertifiedEgressError::TranscriptSlotInvalid {
                context,
                index: slot.index,
            });
        }
        let actual = self.inline_kinds[slot.index];
        if actual != slot.kind {
            return Err(CertifiedEgressError::TranscriptSlotKindMismatch {
                context,
                expected: slot.kind,
                actual,
            });
        }
        let completed = 1u16 << slot.index;
        if self.inline_completed & completed != 0 {
            return Err(CertifiedEgressError::TranscriptSlotAlreadyCompleted {
                context,
                index: slot.index,
            });
        }
        self.inline_body_lens[slot.index] = body_len;
        self.inline_completed |= completed;
        Ok(())
    }

    fn seal(self) -> Result<CertifiedLengthTranscript, CertifiedEgressError> {
        let remaining = self.spill.as_ref().map_or_else(
            || usize::from(self.inline_count) - self.inline_completed.count_ones() as usize,
            |spill| spill.iter().filter(|event| !event.completed).count(),
        );
        if remaining != 0 {
            return Err(CertifiedEgressError::TranscriptIncomplete { remaining });
        }
        Ok(CertifiedLengthTranscript {
            inline_body_lens: self.inline_body_lens,
            inline_kinds: self.inline_kinds,
            inline_completed: self.inline_completed,
            inline_count: self.inline_count,
            spill: self.spill,
        })
    }
}

#[derive(Debug)]
struct CertifiedLengthTranscript {
    inline_body_lens: [usize; CERTIFIED_EGRESS_INLINE_EVENTS],
    inline_kinds: [CertifiedLengthEventKind; CERTIFIED_EGRESS_INLINE_EVENTS],
    inline_completed: u16,
    inline_count: u8,
    spill: Option<Vec<CertifiedLengthEvent>>,
}

impl CertifiedLengthTranscript {
    #[inline]
    fn len(&self) -> usize {
        self.spill
            .as_ref()
            .map_or_else(|| usize::from(self.inline_count), Vec::len)
    }

    #[inline]
    fn event(&self, index: usize) -> Option<CertifiedLengthEvent> {
        if let Some(spill) = &self.spill {
            return spill.get(index).copied();
        }
        if index >= usize::from(self.inline_count) {
            return None;
        }
        Some(CertifiedLengthEvent {
            body_len: self.inline_body_lens[index],
            kind: self.inline_kinds[index],
            completed: self.inline_completed & (1u16 << index) != 0,
        })
    }
}

/// Mutable first-pass state. It combines exact owner census with a typed
/// pre-order transcript of every length-delimited body needed by emission.
#[derive(Debug, Default)]
pub struct CertifiedEgressPreflight {
    owners: ParOwnerCensus,
    transcript: CertifiedLengthTranscriptBuilder,
}

impl CertifiedEgressPreflight {
    #[inline]
    pub fn reserve_length(
        &mut self,
        kind: CertifiedLengthEventKind,
    ) -> Result<CertifiedLengthSlot, CertifiedEgressError> {
        self.transcript.reserve(kind)
    }

    #[inline]
    pub fn complete_length(
        &mut self,
        slot: CertifiedLengthSlot,
        body_len: usize,
        context: &'static str,
    ) -> Result<(), CertifiedEgressError> {
        self.transcript.complete(slot, body_len, context)
    }

    #[inline]
    pub fn par_encoded_len(&mut self, value: &Par) -> Result<usize, CertifiedEgressError> {
        let body_len = self.owners.observe_par(value)?;
        self.transcript
            .push_completed(CertifiedLengthEventKind::ParBody, body_len)?;
        Ok(body_len)
    }

    #[inline]
    fn seal(self, body_len: usize) -> Result<CertifiedEgressPlan, CertifiedEgressError> {
        Ok(CertifiedEgressPlan {
            body_len,
            owners: self.owners,
            transcript: self.transcript.seal()?,
        })
    }
}

/// Immutable proof object produced only after the complete preflight succeeds.
/// A replay cursor cannot exist for a partially measured message graph.
#[derive(Debug)]
pub struct CertifiedEgressPlan {
    body_len: usize,
    owners: ParOwnerCensus,
    transcript: CertifiedLengthTranscript,
}

impl CertifiedEgressPlan {
    #[inline]
    pub fn for_root<T: CertifiedEgressNode>(root: &T) -> Result<Self, CertifiedEgressError> {
        let mut preflight = CertifiedEgressPreflight::default();
        let body_len = root.certified_egress_encoded_len(&mut preflight)?;
        preflight.seal(body_len)
    }

    #[inline]
    pub const fn body_len(&self) -> usize { self.body_len }

    #[inline]
    pub const fn owner_census(&self) -> &ParOwnerCensus { &self.owners }

    #[inline]
    pub fn has_transcript_spill(&self) -> bool { self.transcript.spill.is_some() }

    #[inline]
    pub fn transcript_event_count(&self) -> usize { self.transcript.len() }

    pub fn replay(&self) -> CertifiedEgressReplay<'_> {
        CertifiedEgressReplay {
            transcript: &self.transcript,
            index: 0,
        }
    }
}

/// Linear cursor over one immutable, typed preflight transcript.
#[derive(Debug)]
pub struct CertifiedEgressReplay<'a> {
    transcript: &'a CertifiedLengthTranscript,
    index: usize,
}

impl CertifiedEgressReplay<'_> {
    #[inline]
    pub fn next_body_len(
        &mut self,
        expected: CertifiedLengthEventKind,
        context: &'static str,
    ) -> Result<usize, CertifiedEgressError> {
        let event = self
            .transcript
            .event(self.index)
            .ok_or(CertifiedEgressError::TranscriptExhausted { context, expected })?;
        if event.kind != expected {
            return Err(CertifiedEgressError::TranscriptKindMismatch {
                context,
                expected,
                actual: event.kind,
            });
        }
        self.index = self
            .index
            .checked_add(1)
            .ok_or(CertifiedEgressError::LengthOverflow {
                context: "certified egress replay cursor",
            })?;
        Ok(event.body_len)
    }

    #[inline]
    pub fn next_par_encoded_len(&mut self) -> Result<usize, CertifiedEgressError> {
        self.next_body_len(CertifiedLengthEventKind::ParBody, "Par")
    }

    /// Require exact agreement between the generated preflight and emission
    /// walks before the private frame can be published.
    pub fn finish(self) -> Result<(), CertifiedEgressError> {
        let remaining = self.transcript.len().saturating_sub(self.index);
        if remaining == 0 {
            Ok(())
        } else {
            Err(CertifiedEgressError::TranscriptRemaining { remaining })
        }
    }
}

/// Checked addition shared by the generated length programs.
#[inline]
pub fn checked_egress_add(
    total: &mut usize,
    amount: usize,
    context: &'static str,
) -> Result<(), CertifiedEgressError> {
    *total = total
        .checked_add(amount)
        .ok_or(CertifiedEgressError::LengthOverflow { context })?;
    Ok(())
}

/// Exact size of one length-delimited field from a pre-measured body.
#[inline]
pub fn certified_length_delimited_field_len(
    tag: u32,
    body_len: usize,
    context: &'static str,
) -> Result<usize, CertifiedEgressError> {
    let body_u64 =
        u64::try_from(body_len).map_err(|_| CertifiedEgressError::LengthExceedsU64 { context })?;
    key_len(tag)
        .checked_add(encoded_len_varint(body_u64))
        .and_then(|prefix| prefix.checked_add(body_len))
        .ok_or(CertifiedEgressError::LengthOverflow { context })
}

/// Emit the key and varint body length of one pre-measured nested message or
/// raw length-delimited field.
#[inline]
pub fn encode_certified_length_delimited_prefix<B: BufMut>(
    tag: u32,
    body_len: usize,
    output: &mut B,
    context: &'static str,
) -> Result<(), CertifiedEgressError> {
    let body_u64 =
        u64::try_from(body_len).map_err(|_| CertifiedEgressError::LengthExceedsU64 { context })?;
    encode_key(tag, WireType::LengthDelimited, output);
    encode_varint(body_u64, output);
    Ok(())
}

/// Exact size of one raw length-delimited body inside EPM1. Unlike a protobuf
/// message field, an EPM1 map value has no field key: it is only
/// `varint(body_len) ++ body` after the PathMap topology prefix.
#[inline]
fn certified_epathmap_value_len(
    body_len: usize,
    context: &'static str,
) -> Result<usize, CertifiedEgressError> {
    let body_u64 =
        u64::try_from(body_len).map_err(|_| CertifiedEgressError::LengthExceedsU64 { context })?;
    encoded_len_varint(body_u64)
        .checked_add(body_len)
        .ok_or(CertifiedEgressError::LengthOverflow { context })
}

/// Measure the EPM1 bytes body without constructing it. The topology prefix is
/// already cached by `EntryTrie`; map values remain in `PathMap<Par>` and are
/// visited in canonical zipper order.
fn certified_epathmap_snapshot_len(
    map: &EPathMap,
    preflight: &mut CertifiedEgressPreflight,
) -> Result<usize, CertifiedEgressError> {
    let snapshot_slot = preflight.reserve_length(CertifiedLengthEventKind::EPathMapSnapshotBody)?;
    let layout = map.entry_trie().epm_layout();
    let expected = layout.value_count();
    let mut total = layout.prefix().len();
    let mut actual = 0usize;

    if let Some(values) = map.entry_trie().map_trie() {
        let mut zipper = values.read_zipper();
        while let Some(value) = zipper.to_next_get_val() {
            // EPM1 map values are the opaque `Par` cut. Calling the preflight
            // primitive directly records the same ParBody event while making
            // it impossible to mistake this edge for recursive EPathMap
            // trait dispatch.
            let body_len = preflight.par_encoded_len(value)?;
            checked_egress_add(
                &mut total,
                certified_epathmap_value_len(body_len, "EPathMap map value")?,
                "EPathMap snapshot body",
            )?;
            actual = actual
                .checked_add(1)
                .ok_or(CertifiedEgressError::LengthOverflow {
                    context: "EPathMap map value count",
                })?;
        }
    }

    if actual != expected {
        return Err(CertifiedEgressError::EPathMapValueCountMismatch {
            phase: "length walk",
            expected,
            actual,
        });
    }
    preflight.complete_length(snapshot_slot, total, "EPathMap.trie_snapshot")?;
    Ok(total)
}

// ===========================================================================
// §3  `Par` and `EPathMap` — hand-written external programs
// ===========================================================================

/// The PathMap-backed `Par` is one opaque bounded operation to the generic
/// protobuf driver. Its own implementation streams the descriptor-ordered
/// component families and root metadata directly from the authoritative trie.
/// There is intentionally no synthetic per-field program: exposing one would
/// imply that the generic driver descends into decoded component values, which
/// is precisely the representation regression this external cutover avoids.
pub static PAR_PROTOBUF_PROGRAM: &[ProtobufField] = &[];

impl ProtobufNode for Par {
    #[inline]
    fn as_par(&self) -> Option<&Par> { Some(self) }

    #[inline]
    fn protobuf_program(&self) -> &'static [ProtobufField] { PAR_PROTOBUF_PROGRAM }

    fn protobuf_len_step(&self, from: usize) -> (u64, ProtobufDescent<'_>) {
        if from != 0 {
            return (0, ProtobufDescent::Done);
        }
        let length = self
            .protobuf_encoded_len()
            .expect("PathMap-backed Par protobuf length must be representable");
        (
            u64::try_from(length).expect("usize always fits u64 on supported Rust targets"),
            ProtobufDescent::Done,
        )
    }

    fn protobuf_emit(&self, from: usize, out: &mut dyn BufMut) -> ProtobufDescent<'_> {
        if from == 0 {
            self.encode_protobuf(&mut &mut *out)
                .expect("PathMap-backed Par protobuf length must be representable");
        }
        ProtobufDescent::Done
    }
}

impl CertifiedEgressNode for Par {
    const MAX_SCHEMA_DEPTH: usize = 1;

    #[inline]
    fn certified_egress_encoded_len(
        &self,
        preflight: &mut CertifiedEgressPreflight,
    ) -> Result<usize, CertifiedEgressError> {
        // The descriptor-generated outer walk reaches this cut vertex once per
        // protobuf-visible Par field. Observation therefore belongs here: no
        // outer message can accidentally count a slot twice, and the no-op
        // census used for emission-time length prefixes remains allocation-free.
        preflight.par_encoded_len(self)
    }

    #[inline]
    fn try_encode_certified_egress<B, E, F>(
        &self,
        output: &mut B,
        replay: &mut CertifiedEgressReplay<'_>,
        emit_par: &mut F,
    ) -> Result<(), CertifiedEgressEncodeError<E>>
    where
        B: BufMut,
        F: FnMut(&Par, usize, &mut B) -> Result<(), E>,
    {
        let encoded_len = replay.next_par_encoded_len()?;
        emit_par(self, encoded_len, output).map_err(CertifiedEgressEncodeError::Par)
    }
}

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
    fn as_epathmap(&self) -> Option<&EPathMap> { Some(self) }

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

/// Hand-certified descriptor cut for the PathMap-backed EPathMap.
///
/// `EPathMap` is a cut only for proving the generated descriptor graph
/// acyclic. Its boundary is not opaque to the wire: field 5 follows the
/// generated, acyclic `Var -> WildcardMsg` residual path and field 9 streams
/// every `PathMap<Par>` value through the same census/replay/callback protocol
/// as ordinary message fields. The maximum native call chain is therefore
/// `EPathMap -> Var -> WildcardMsg`, or `EPathMap -> Par`, hence depth three.
impl CertifiedEgressNode for EPathMap {
    const MAX_SCHEMA_DEPTH: usize = 3;

    fn certified_egress_encoded_len(
        &self,
        preflight: &mut CertifiedEgressPreflight,
    ) -> Result<usize, CertifiedEgressError> {
        let mut total = 0usize;
        if !self.locally_free.is_empty() {
            checked_egress_add(
                &mut total,
                prost::encoding::bytes::encoded_len(3u32, &self.locally_free),
                "EPathMap.locally_free",
            )?;
        }
        if self.connective_used {
            checked_egress_add(
                &mut total,
                prost::encoding::bool::encoded_len(4u32, &self.connective_used),
                "EPathMap.connective_used",
            )?;
        }
        if let Some(remainder) = &self.remainder {
            let remainder_slot = preflight.reserve_length(CertifiedLengthEventKind::MessageBody)?;
            let body_len = remainder.certified_egress_encoded_len(preflight)?;
            preflight.complete_length(remainder_slot, body_len, "EPathMap.remainder")?;
            checked_egress_add(
                &mut total,
                certified_length_delimited_field_len(5u32, body_len, "EPathMap.remainder")?,
                "EPathMap.remainder",
            )?;
        }

        let snapshot_len = certified_epathmap_snapshot_len(self, preflight)?;
        checked_egress_add(
            &mut total,
            certified_length_delimited_field_len(9u32, snapshot_len, "EPathMap.trie_snapshot")?,
            "EPathMap.trie_snapshot",
        )?;
        Ok(total)
    }

    fn try_encode_certified_egress<B, E, F>(
        &self,
        output: &mut B,
        replay: &mut CertifiedEgressReplay<'_>,
        emit_par: &mut F,
    ) -> Result<(), CertifiedEgressEncodeError<E>>
    where
        B: BufMut,
        F: FnMut(&Par, usize, &mut B) -> Result<(), E>,
    {
        if !self.locally_free.is_empty() {
            prost::encoding::bytes::encode(3u32, &self.locally_free, output);
        }
        if self.connective_used {
            prost::encoding::bool::encode(4u32, &self.connective_used, output);
        }
        if let Some(remainder) = &self.remainder {
            let body_len = replay
                .next_body_len(CertifiedLengthEventKind::MessageBody, "EPathMap.remainder")?;
            encode_certified_length_delimited_prefix(5u32, body_len, output, "EPathMap.remainder")?;
            remainder.try_encode_certified_egress(output, replay, emit_par)?;
        }

        let snapshot_len = replay.next_body_len(
            CertifiedLengthEventKind::EPathMapSnapshotBody,
            "EPathMap.trie_snapshot",
        )?;
        encode_certified_length_delimited_prefix(
            9u32,
            snapshot_len,
            output,
            "EPathMap.trie_snapshot",
        )?;

        let layout = self.entry_trie().epm_layout();
        output.put_slice(layout.prefix());
        let expected = layout.value_count();
        let mut actual = 0usize;
        if let Some(values) = self.entry_trie().map_trie() {
            let mut zipper = values.read_zipper();
            while let Some(value) = zipper.to_next_get_val() {
                let body_len = replay
                    .next_body_len(CertifiedLengthEventKind::ParBody, "EPathMap map value")?;
                let body_u64 = u64::try_from(body_len).map_err(|_| {
                    CertifiedEgressError::LengthExceedsU64 {
                        context: "EPathMap map value",
                    }
                })?;
                encode_varint(body_u64, output);
                emit_par(value, body_len, output).map_err(CertifiedEgressEncodeError::Par)?;
                actual = actual
                    .checked_add(1)
                    .ok_or(CertifiedEgressError::LengthOverflow {
                        context: "EPathMap map value count",
                    })?;
            }
        }
        if actual != expected {
            return Err(CertifiedEgressError::EPathMapValueCountMismatch {
                phase: "emission walk",
                expected,
                actual,
            }
            .into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod certified_egress_tests {
    use prost::Message as _;

    use super::{CertifiedEgressNode, CertifiedEgressPlan};
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::tagged_continuation::TaggedCont;
    use crate::rhoapi::var::{VarInstance, WildcardMsg};
    use crate::rhoapi::{
        EPathMap, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation, Var,
    };
    use crate::rholang_scala_rust_types::ConsumeResultReturn;
    use crate::rspace_plus_plus_types::ChannelsProto;
    use crate::rust::rholang::egress_protobuf_schema_tables::{
        rhoapi_bridge, rholang_scala_rust_types, rspace_plus_plus_types,
        CERTIFIED_EGRESS_CUT_DEPTH_REGISTRY, CERTIFIED_EGRESS_CUT_EDGE_DEPTH_REGISTRY,
        CERTIFIED_EGRESS_PUBLISH_ROOTS,
    };

    fn integer(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    fn certified_bytes<T: CertifiedEgressNode>(value: &T) -> (Vec<u8>, CertifiedEgressPlan) {
        let plan = CertifiedEgressPlan::for_root(value).expect("certified preflight");
        let encoded_len = plan.body_len();
        let mut output = Vec::with_capacity(encoded_len);
        let mut replay = plan.replay();
        value
            .try_encode_certified_egress(&mut output, &mut replay, &mut |par, expected, output| {
                par.encode_protobuf_direct_to_vec_prevalidated(output, expected)
            })
            .expect("certified emission");
        replay.finish().expect("exact transcript exhaustion");
        assert_eq!(output.len(), encoded_len);
        (output, plan)
    }

    #[test]
    fn owner_census_is_inline_for_four_occurrences_and_excludes_empty_components() {
        let shared = integer(17);
        let metadata_variant = shared
            .clone()
            .with_locally_free(vec![0x81, 0x01])
            .with_connective_used(true);
        let independent = integer(17);
        let empty = Par::default().with_connective_used(true);
        let message = ChannelsProto {
            channels: vec![shared.clone(), metadata_variant, independent.clone(), empty],
        };

        let (actual, plan) = certified_bytes(&message);
        let census = plan.owner_census();
        assert_eq!(actual, message.encode_to_vec());
        assert!(!plan.has_transcript_spill());
        assert!(census.has_owner_spill());
        assert!(census.is_reusable_owner(&shared.protobuf_owner_key()));
        assert!(!census.is_reusable_owner(&independent.protobuf_owner_key()));

        let statistics = census.statistics();
        assert_eq!(statistics.par_occurrences, 4);
        assert_eq!(statistics.component_empty_occurrences, 1);
        assert_eq!(statistics.eligible_occurrences, 3);
        assert_eq!(statistics.unique_owners, 2);
        assert_eq!(statistics.repeated_occurrences, 1);
        assert_eq!(statistics.repeated_owners, 1);
        assert!(statistics.has_reuse());
        assert!(statistics.reusable_component_bytes > 0);
        assert_eq!(
            statistics.repeated_component_bytes,
            statistics.reusable_component_bytes
        );
    }

    #[test]
    fn certified_transcript_rejects_under_and_over_consumption_exactly() {
        let value = integer(23);
        let empty = CertifiedEgressPlan::for_root(&ChannelsProto::default()).unwrap();
        empty.replay().finish().expect("empty transcript is exact");

        let plan = CertifiedEgressPlan::for_root(&value).unwrap();

        assert!(matches!(
            plan.replay().finish(),
            Err(super::CertifiedEgressError::TranscriptRemaining { remaining: 1 })
        ));

        let mut replay = plan.replay();
        assert_eq!(
            replay.next_par_encoded_len().unwrap(),
            value.protobuf_encoded_len().unwrap()
        );
        replay.finish().expect("one measured Par was consumed");

        let mut replay = plan.replay();
        let _ = replay.next_par_encoded_len().unwrap();
        assert!(matches!(
            replay.next_par_encoded_len(),
            Err(super::CertifiedEgressError::TranscriptExhausted {
                expected: super::CertifiedLengthEventKind::ParBody,
                ..
            })
        ));
    }

    #[test]
    fn transcript_slots_remain_stable_when_children_force_a_spill() {
        let mut builder = super::CertifiedLengthTranscriptBuilder::default();
        let parent = builder
            .reserve(super::CertifiedLengthEventKind::MessageBody)
            .unwrap();
        for ordinal in 0..super::CERTIFIED_EGRESS_INLINE_EVENTS {
            let child = builder
                .reserve(super::CertifiedLengthEventKind::ParBody)
                .unwrap();
            builder.complete(child, 100 + ordinal, "child").unwrap();
        }
        builder.complete(parent, 777, "parent").unwrap();
        let transcript = builder.seal().unwrap();
        assert!(transcript.spill.is_some());

        let mut replay = super::CertifiedEgressReplay {
            transcript: &transcript,
            index: 0,
        };
        assert_eq!(
            replay
                .next_body_len(super::CertifiedLengthEventKind::MessageBody, "parent")
                .unwrap(),
            777
        );
        for ordinal in 0..super::CERTIFIED_EGRESS_INLINE_EVENTS {
            assert_eq!(
                replay
                    .next_body_len(super::CertifiedLengthEventKind::ParBody, "child")
                    .unwrap(),
                100 + ordinal
            );
        }
        replay.finish().unwrap();
    }

    #[test]
    fn transcript_sealing_and_kinds_fail_closed() {
        let mut incomplete = super::CertifiedLengthTranscriptBuilder::default();
        let _ = incomplete
            .reserve(super::CertifiedLengthEventKind::MapEntryBody)
            .unwrap();
        assert!(matches!(
            incomplete.seal(),
            Err(super::CertifiedEgressError::TranscriptIncomplete { remaining: 1 })
        ));

        let mut duplicate = super::CertifiedLengthTranscriptBuilder::default();
        let slot = duplicate
            .reserve(super::CertifiedLengthEventKind::MessageBody)
            .unwrap();
        duplicate.complete(slot, 9, "message").unwrap();
        assert!(matches!(
            duplicate.complete(slot, 9, "message"),
            Err(super::CertifiedEgressError::TranscriptSlotAlreadyCompleted { .. })
        ));

        let transcript = duplicate.seal().unwrap();
        let mut replay = super::CertifiedEgressReplay {
            transcript: &transcript,
            index: 0,
        };
        assert!(matches!(
            replay.next_body_len(super::CertifiedLengthEventKind::ParBody, "equal-sized Par"),
            Err(super::CertifiedEgressError::TranscriptKindMismatch {
                expected: super::CertifiedLengthEventKind::ParBody,
                actual: super::CertifiedLengthEventKind::MessageBody,
                ..
            })
        ));
    }

    #[test]
    fn cross_package_bridge_visits_continuation_guard_body_and_datums() {
        let shared = integer(29);
        let message = ConsumeResultReturn {
            tagged_cont: Some(TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                    body: Some(shared.clone()),
                    ..ParWithRandom::default()
                })),
                guard: Some(shared.clone()),
                ..TaggedContinuation::default()
            }),
            datums: vec![ListParWithRandom {
                pars: vec![shared],
                ..ListParWithRandom::default()
            }],
        };

        let (actual, plan) = certified_bytes(&message);
        let census = plan.owner_census();
        assert_eq!(actual, message.encode_to_vec());
        assert_eq!(census.statistics().par_occurrences, 3);
        assert_eq!(census.statistics().repeated_occurrences, 2);
        assert_eq!(ConsumeResultReturn::MAX_SCHEMA_DEPTH, 7);
    }

    #[test]
    fn oneof_default_scalar_presence_matches_prost() {
        let message = ConsumeResultReturn {
            tagged_cont: Some(TaggedContinuation {
                tagged_cont: Some(TaggedCont::ScalaBodyRef(0)),
                ..TaggedContinuation::default()
            }),
            ..ConsumeResultReturn::default()
        };
        let (actual, plan) = certified_bytes(&message);
        let census = plan.owner_census();
        assert_eq!(actual, message.encode_to_vec());
        assert_eq!(census.statistics().par_occurrences, 0);
    }

    #[test]
    fn epathmap_boundary_streams_map_values_and_certifies_the_var_remainder() {
        let shared = integer(41);
        let message = EPathMap::new_map(
            [(integer(1), shared.clone()), (integer(2), shared.clone())],
            vec![0x81, 0x01],
            true,
            Some(Var {
                var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
            }),
        );

        let (actual, plan) = certified_bytes(&message);
        let census = plan.owner_census();
        assert_eq!(actual, message.encode_to_vec());
        assert_eq!(EPathMap::MAX_SCHEMA_DEPTH, 3);
        assert_eq!(census.statistics().par_occurrences, 2);
        assert_eq!(census.statistics().unique_owners, 1);
        assert_eq!(census.statistics().repeated_occurrences, 1);
        assert!(census.is_reusable_owner(&shared.protobuf_owner_key()));
    }

    #[test]
    fn certified_cut_registry_includes_the_truthful_epathmap_depth() {
        assert_eq!(CERTIFIED_EGRESS_CUT_DEPTH_REGISTRY, &[
            ("Par", 1),
            ("EPathMap", 3)
        ]);
    }

    #[test]
    fn all_nine_publish_roots_have_the_audited_compile_time_depths() {
        assert_eq!(CERTIFIED_EGRESS_PUBLISH_ROOTS, &[
            ("Par", 1),
            ("EvaluateResultProto", 2),
            ("SoftCheckpointProto", 10),
            ("CheckpointProto", 5),
            ("ConsumeResultReturn", 7),
            ("DatumsProto", 7),
            ("JoinsProto", 3),
            ("WaitingContinuationsProto", 8),
            ("StoreToMapResult", 10),
        ]);
    }

    #[test]
    fn every_generated_and_hand_certified_edge_strictly_decreases_depth() {
        let registries = [
            rhoapi_bridge::CERTIFIED_EGRESS_EDGE_DEPTH_REGISTRY,
            rholang_scala_rust_types::CERTIFIED_EGRESS_EDGE_DEPTH_REGISTRY,
            rspace_plus_plus_types::CERTIFIED_EGRESS_EDGE_DEPTH_REGISTRY,
            CERTIFIED_EGRESS_CUT_EDGE_DEPTH_REGISTRY,
        ];
        let mut edge_count = 0usize;
        for registry in registries {
            for &(parent, parent_depth, child, child_depth) in registry {
                assert!(
                    child_depth < parent_depth,
                    "certified edge {parent} ({parent_depth}) -> {child} ({child_depth}) must decrease"
                );
                edge_count += 1;
            }
        }
        assert!(
            edge_count > 0,
            "the certified graph must have descriptor edges"
        );
        assert_eq!(CERTIFIED_EGRESS_CUT_EDGE_DEPTH_REGISTRY, &[
            (".rhoapi.EPathMap", 3, ".rhoapi.Var", 2),
            (".rhoapi.EPathMap", 3, ".rhoapi.Par", 1),
        ]);
    }
}
