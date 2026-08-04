//! # `bincode_schema` — the bincode schema alphabet used in both directions
//!
//! This module holds the **hand-written** half of the codec: the closed
//! `FieldKind` alphabet, the two object-safe traits the generated table
//! implements, and the one program the descriptor cannot express
//! (`EPathMap`). The **generated** half — one `impl BincodeNode` per message, one
//! `impl BincodeOneof` per oneof, the variant tables and their indices — is
//! emitted by `models/codegen/schema.rs` into `OUT_DIR/rhoapi_bincode_schema.rs` and
//! included by [`crate::rust::rholang::bincode_schema_tables`].
//!
//! ## The format, in one paragraph
//!
//! `bincode::serialize` is `config::legacy()`: **fixint, little-endian, no
//! size limit, trailing bytes allowed**. Integers are fixed width; `bool` and
//! `Option` tags are one byte; sequence, map, string and byte-payload lengths
//! are `u64` LE; structs are **positional** in declaration order (field names
//! never reach the wire); enums are a `u32` LE declaration-order index then
//! the payload. There are no back-references, no framing, and no byte-length
//! prefixes.
//!
//! ★ **Byte identity of a single-pass emitter therefore follows
//! STRUCTURALLY**, not by luck: with no back-references and no length
//! prefixes, the emission order of `serde`'s derived `Serialize` *is* the
//! pre-order walk of the term, which is exactly what an explicit stack yields.
//! The differential tests confirm this; they do not establish it.
//!
//! ## Why traits and not one big `enum Ref`
//!
//! The op stack must hold a pointer to a node of *any* of the ~60 schema
//! types. A 60-variant enum would work, but every one of its arms would be
//! hand-maintained and a new message would be a silent omission. `&dyn
//! BincodeNode` erases the type in 16 bytes with the vtable in `.rodata`, so:
//!
//! * the op stack entry is small and `Copy`;
//! * a new message is covered the moment the generator emits its `impl`;
//! * nothing is allocated at run time to describe the schema.
//!
//! ## The closed alphabet
//!
//! [`FieldKind`] is **closed on purpose**. Every protobuf construct in
//! `RhoTypes.proto` maps to exactly one kind, and the generator *panics* on
//! anything it cannot classify rather than widening to the nearest neighbour —
//! because "nearest neighbour" in a byte format is a consensus fork.

use std::collections::BTreeMap;

use crate::rhoapi::{Par, Var};
use crate::rust::rhoapi_ext::EPathMap;

// ===========================================================================
// §A  The closed field alphabet
// ===========================================================================

/// The shape of one serde field, in declaration order.
///
/// | kind | bincode bytes | descends? |
/// |---|---|---|
/// | `Bool` | 1 | no |
/// | `I32` / `U32` | 4 LE | no |
/// | `I64` / `U64` | 8 LE | no |
/// | `Bytes` | `u64` LE length ++ payload | no |
/// | `EmptyBytes` | **always** eight zero bytes | no |
/// | `Str` | `u64` LE length ++ UTF-8 | no |
/// | `StrSeq` | `u64` LE count ++ each `Str` | no |
/// | `BytesSeq` | `u64` LE count ++ each `Bytes` | no |
/// | `Opt` | 1-byte tag ++ payload when `Some` | yes |
/// | `Seq` | `u64` LE count ++ each element | yes |
/// | `Map` | `u64` LE count ++ key/value PAIRS | yes |
/// | `Oneof` | 1-byte `Option` tag ++ `u32` LE index ++ payload | yes |
///
/// ⚠ `EmptyBytes` is the **serialize-only** normalization `models/build.rs`
/// injects on every `locally_free` field (`serialize_as_empty_bytes`). It is
/// *written* as eight zero bytes and *read back* at the stream's real length —
/// a deliberate asymmetry, because `locally_free` is transient analysis data
/// that must not reach an RSpace channel hash. A decoder that assumed zero
/// would reject byte strings the derived decoder accepts, which is a fork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    I32,
    U32,
    I64,
    U64,
    Bytes,
    EmptyBytes,
    Str,
    StrSeq,
    BytesSeq,
    Opt,
    Seq,
    Map,
    Oneof,
}

impl FieldKind {
    /// Whether reading this field can descend into an arbitrarily deep child —
    /// i.e. whether the driver must suspend at it.
    #[inline]
    pub const fn descends(self) -> bool {
        matches!(
            self,
            FieldKind::Opt | FieldKind::Seq | FieldKind::Map | FieldKind::Oneof
        )
    }
}

// ===========================================================================
// §A2  The emission primitives — the ONE place bincode's layout is written
// ===========================================================================
//
// ⚠★ WHY THESE ARE PUBLIC AND CALLED FROM GENERATED CODE.
//
// The first design had the generated table expose `fn bincode_field(i) -> FieldVal`
// and let a hand-written driver interpret it. That is the obvious factoring and
// it is 1.7× SLOWER than the derived `Serialize` — measured, then profiled:
// `Par` has eleven fields, so each node cost eleven indirect `bincode_field` calls
// returning a 32-byte enum by value, plus nine more indirect `bincode_len` calls
// on the returned `&dyn BincodeSeq`, plus `bincode_program` — **21 indirect calls per
// node where the derived path has none**, because serde's derive is
// monomorphic and fully inlined. `perf` put 31.8% of the profile in the driver
// loop, 10.1% in `Par::bincode_field` alone, and ~8.6% across the `bincode_len`
// thunks.
//
// So the split moved to where it belongs: **bounded-field EMISSION is
// generated and monomorphic; the TRAMPOLINE is hand-written and generic.** The
// generated `bincode_emit` runs a node's leaf fields straight-line — the same
// codegen the derive gets — and returns at the first descent. The driver still
// owns every suspension, the counted repeat, the op stack and the map
// iterator: it just never touches a field it does not have to suspend at.
//
// These functions remain the single source of truth for the layout. Generated
// code CALLS them; it does not restate them.

/// `u64` little-endian — every length, count and `usize` on the wire.
#[inline(always)]
pub fn put_u64(out: &mut Vec<u8>, v: u64) { out.extend_from_slice(&v.to_le_bytes()); }

/// `u32` little-endian — the enum variant index, and `uint32`/`fixed32` fields.
#[inline(always)]
pub fn put_u32(out: &mut Vec<u8>, v: u32) { out.extend_from_slice(&v.to_le_bytes()); }

/// `i32` little-endian. ⚠ NOT zigzag: serde sees an `i32` whatever the proto
/// said (`sint32` is a *protobuf* encoding and never reaches bincode).
#[inline(always)]
pub fn put_i32(out: &mut Vec<u8>, v: i32) { out.extend_from_slice(&v.to_le_bytes()); }

/// `i64` little-endian.
#[inline(always)]
pub fn put_i64(out: &mut Vec<u8>, v: i64) { out.extend_from_slice(&v.to_le_bytes()); }

/// One byte, `0` or `1`.
#[inline(always)]
pub fn put_bool(out: &mut Vec<u8>, v: bool) { out.push(u8::from(v)); }

/// A length-prefixed byte payload: `u64` LE length ++ raw bytes.
///
/// `serialize_bytes` and `Vec<u8>`-as-a-seq coincide, because each `u8` element
/// of a seq is one byte — so one primitive serves both spellings.
#[inline(always)]
pub fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_u64(out, b.len() as u64);
    out.extend_from_slice(b);
}

/// A length-prefixed UTF-8 payload.
#[inline(always)]
pub fn put_str(out: &mut Vec<u8>, s: &str) { put_bytes(out, s.as_bytes()); }

/// ⚠ The serialize-only `locally_free` normalization: **eight zero bytes**,
/// whatever is stored. `models/build.rs` injects the equivalent
/// `serialize_with` on the derived path; the DECODER reads the stream's real
/// length. A decoder that assumed zero would reject byte strings the derived
/// decoder accepts, which is a fork.
#[inline(always)]
pub fn put_empty_bytes(out: &mut Vec<u8>) { put_u64(out, 0); }

/// `u64` LE count ++ each length-prefixed string.
#[inline]
pub fn put_str_seq(out: &mut Vec<u8>, ss: &[String]) {
    put_u64(out, ss.len() as u64);
    for s in ss {
        put_str(out, s);
    }
}

/// `u64` LE count ++ each length-prefixed byte payload.
#[inline]
pub fn put_bytes_seq(out: &mut Vec<u8>, bs: &[Vec<u8>]) {
    put_u64(out, bs.len() as u64);
    for b in bs {
        put_bytes(out, b);
    }
}

// ===========================================================================
// §B  The two object-safe traits the generated table implements
// ===========================================================================

/// The `resume` value meaning **there is nothing to come back for**.
///
/// ★ The generator knows each program's length statically, so it emits this
/// sentinel whenever the descending field was the node's LAST. Without it the
/// driver had to ask — `node.bincode_program().len()` — which is a virtual call on
/// EVERY descent, and descents are what a deep term is made of. Measured: the
/// production-weighted mix went from 2.75% slower than the derived path to
/// faster, and depth 6 from 9.0% slower to a win.
pub const NO_RESUME: u16 = u16::MAX;

/// What a node's emission ran into: nothing, or one arbitrarily deep child.
///
/// `resume` is the field index the driver must re-enter at once the child's
/// subtree is complete, or [`NO_RESUME`] when the program is spent. It is a
/// `u16` so `Op` stays four words.
pub enum Descent<'a> {
    /// The program is spent. ★ The driver pushes no resume point for this —
    /// a node's last field is a tail call, and so is a sequence's last element.
    Done,
    /// One child value (an `Option<Message>` field, or a oneof's message arm).
    Node {
        resume: u16,
        node: &'a dyn BincodeNode,
    },
    /// A non-empty sequence. `len` is carried so the driver never has to make
    /// a virtual call to ask — that call was ~8.6% of the first profile.
    Seq {
        resume: u16,
        len: usize,
        seq: &'a dyn BincodeSeq,
    },
    /// The single `BTreeMap` field in the schema (`New.injections`). serde
    /// emits a **map**: a `u64` count, then key/value *pairs* — keys
    /// interleaved with arbitrarily deep values, and not required to be sorted
    /// on the wire (duplicate keys overwrite, as `BTreeMap::insert`).
    Map {
        resume: u16,
        map: &'a BTreeMap<String, Par>,
    },
}

/// A schema node: a `&'static` field program, and monomorphic emission of it.
///
/// Object-safe by construction — no associated constants, no generic methods —
/// because the driver's op stack holds `&dyn BincodeNode`.
pub trait BincodeNode {
    /// This node's fields, in serde declaration order.
    ///
    /// The TABLE. Consumed by the decoder and by the conformance probe; the
    /// encoder does not interpret it at run time (see §A2), it is *generated
    /// from* it, and the two are pinned to the same oracle by the write
    /// differential.
    fn bincode_program(&self) -> &'static [FieldKind];

    /// Emit fields `[from..]` until the first descent, and report it.
    ///
    /// ★ ONE virtual call per node per suspension — not one per field. The
    /// body is generated per type, so every bounded field inlines exactly as
    /// serde's derive does.
    fn bincode_emit(&self, from: usize, out: &mut Vec<u8>) -> Descent<'_>;

    /// ⚠ **The one node whose field 0 must be CONSTRUCTED, not borrowed.**
    ///
    /// `EPathMap` overrides this; every generated impl inherits `None`.
    ///
    /// ★ It is a *trait method* and not a pointer comparison against
    /// [`EPATHMAP_PROGRAM`] for a reason that cost a `SIGSEGV` to learn:
    /// `&'static` slices with identical contents are **merged by the linker**,
    /// and `EPATHMAP_PROGRAM` is `[Seq, EmptyBytes, Bool, Opt]` — byte-for-byte
    /// the same as `ELIST_PROGRAM`, `ESET_PROGRAM` and `EMAP_PROGRAM`. Program
    /// addresses therefore do **not** identify a type, and any downcast built
    /// on them silently reinterprets an `EList` as an `EPathMap`.
    /// `bincode_encoder_space::program_addresses_do_not_identify_a_type` pins that
    /// fact so the trick cannot be reintroduced as an "optimization".
    #[inline]
    fn bincode_as_pathmap(&self) -> Option<&EPathMap> { None }
}

/// A oneof: writes its declaration-order index and any bounded payload, and
/// reports a message payload for the driver to descend into.
///
/// ⚠ The index is serde DECLARATION ORDER, never the proto tag.
pub trait BincodeOneof {
    fn bincode_emit(&self, out: &mut Vec<u8>) -> Option<&dyn BincodeNode>;
}

/// A homogeneous sequence of nodes, erased.
///
/// One blanket impl covers every `Vec<T>` in the schema, so a new repeated
/// field needs no new code here at all.
pub trait BincodeSeq {
    fn bincode_len(&self) -> usize;
    fn bincode_get(&self, i: usize) -> &dyn BincodeNode;
}

impl<T: BincodeNode> BincodeSeq for Vec<T> {
    #[inline]
    fn bincode_len(&self) -> usize { self.len() }
    #[inline]
    fn bincode_get(&self, i: usize) -> &dyn BincodeNode { &self[i] }
}

// ⚠ No `impl BincodeSeq for [T]`. An unsized type cannot be coerced to a trait
// object, so `&[T] -> &dyn BincodeSeq` is not expressible; every sequence in the
// schema is reached as a `&Vec<T>`, which is.

/// One arm of a oneof, as the DECODER needs it: the serde index, the Rust
/// variant name (for diagnostics), and the arm's own field program.
#[derive(Clone, Copy, Debug)]
pub struct VariantProgram {
    /// ⚠ serde DECLARATION ORDER, never the proto tag.
    pub serde_index: u32,
    pub name: &'static str,
    pub payload: &'static [FieldKind],
}

// ===========================================================================
// §C  The one program the descriptor cannot express
// ===========================================================================

/// `EPathMap`'s serde field program.
///
/// `EPathMap` is `extern_path`'d in `models/build.rs`, so prost generates no
/// struct for it and the descriptor-driven generator deliberately refuses to
/// invent one. Its layout is the **4-field** shape its hand-written `Serialize`
/// writes (`models/src/rust/rhoapi_ext.rs`). The first field is the complete
/// canonical EPM1 snapshot, so it is one `Bytes` instruction here as well.
///
/// ★ **EPM1** — `ps` serializes as the entry trie's canonical compact snapshot:
///
/// ```text
///   Bytes       EPM1(m)          ← u64-LE |EPM1(m)| ‖ ACTree03 arena/value table
///   EmptyBytes  locally_free
///   Bool        connective_used
///   Opt         remainder
/// ```
///
/// The snapshot preserves PathMap prefix compression and carries either unit
/// membership or keyed `Par` values. No `Vec<Par>` projection is serialized.
pub static EPATHMAP_PROGRAM: &[FieldKind] = &[
    FieldKind::Bytes,      // EPM1 snapshot (PathMap ACTree03 arena)
    FieldKind::EmptyBytes, // locally_free  (always blanked on serialize)
    FieldKind::Bool,       // connective_used
    FieldKind::Opt,        // remainder
];

/// What an `EPathMap` serializes for its entries: one borrowed canonical EPM1
/// byte slice.
///
/// # ★ Nothing is constructed, and that is what deleted the arena
///
/// The named wrapper keeps the encoder interface explicit while borrowing the
/// memoized snapshot directly; the driver does not park an entry vector or a
/// second representation of the trie.
pub enum PathmapSnapshot<'a> {
    /// The complete canonical EPM1 snapshot, borrowed.
    Stored {
        /// Prefix-compressed ACTree03 data plus the homogeneous value table,
        /// emitted verbatim as one bincode byte slice.
        trie_snapshot: &'a [u8],
    },
}

impl BincodeNode for EPathMap {
    #[inline]
    fn bincode_program(&self) -> &'static [FieldKind] { EPATHMAP_PROGRAM }

    #[inline]
    fn bincode_as_pathmap(&self) -> Option<&EPathMap> { Some(self) }

    /// Field 0 is accessor-reached because it is the memoized EPM1 snapshot,
    /// not a generated struct field. The driver opens the `EPathMap` there and
    /// re-enters here at field 1.
    fn bincode_emit(&self, from: usize, out: &mut Vec<u8>) -> Descent<'_> {
        let mut i = from;
        loop {
            match i {
                0 => unreachable!(
                    "EPathMap's entries must be opened through \
                     `bincode_schema::pathmap_snapshot` — field 0 is the canonical EPM1 byte \
                     snapshot and is accessor-reached rather than a plain field read"
                ),
                1 => put_empty_bytes(out),
                2 => put_bool(out, self.connective_used),
                // `remainder` is the LAST field, so a descent into it needs no
                // resume point — the encoder's tail call.
                3 => match &self.remainder {
                    Some(v) => {
                        put_bool(out, true);
                        return Descent::Node {
                            resume: NO_RESUME,
                            node: v,
                        };
                    }
                    None => put_bool(out, false),
                },
                _ => return Descent::Done,
            }
            i += 1;
        }
    }
}

/// Return the same canonical EPM1 byte slice used by the hand-written
/// `Serialize` implementation. Both bincode encoders therefore have one source
/// of truth and cannot split keys from values.
pub fn pathmap_snapshot(map: &EPathMap) -> PathmapSnapshot<'_> {
    PathmapSnapshot::Stored {
        trie_snapshot: map.trie_snapshot(),
    }
}

/// `Var` is reachable as `Option<Var>` (`remainder`) from several programs and
/// is otherwise an ordinary generated node; this alias exists so the import
/// above is load-bearing and the module documents the containment.
#[allow(dead_code)]
type RemainderVar = Var;
