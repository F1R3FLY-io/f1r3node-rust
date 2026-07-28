//! # `wire` — the schema alphabet both directions are driven by
//!
//! This module holds the **hand-written** half of the codec: the closed
//! `FieldKind` alphabet, the two object-safe traits the generated table
//! implements, and the one program the descriptor cannot express
//! (`EPathMap`). The **generated** half — one `impl WireNode` per message, one
//! `impl WireOneof` per oneof, the variant tables and their indices — is
//! emitted by `models/build/wire_schema.rs` into `OUT_DIR/rhoapi_wire.rs` and
//! included by [`crate::rust::rholang::wire_schema`].
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
//! WireNode` erases the type in 16 bytes with the vtable in `.rodata`, so:
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

/// A field value that costs a **bounded** number of bytes and never descends.
#[derive(Clone, Copy, Debug)]
pub enum LeafVal<'a> {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Bytes(&'a [u8]),
    /// The `locally_free` normalization: eight zero bytes, whatever is stored.
    EmptyBytes,
    Str(&'a str),
    StrSeq(&'a [String]),
    BytesSeq(&'a [Vec<u8>]),
}

/// The payload of one oneof arm: a nested node, or a bounded leaf.
#[derive(Clone, Copy)]
pub enum Payload<'a> {
    Node(&'a dyn WireNode),
    Leaf(LeafVal<'a>),
}

/// One field of one node, read without copying anything it points at.
#[derive(Clone, Copy)]
pub enum FieldVal<'a> {
    Leaf(LeafVal<'a>),
    Opt(Option<&'a dyn WireNode>),
    Seq(&'a dyn WireSeq),
    /// The single `BTreeMap` field in the schema (`New.injections`). serde
    /// emits a **map**: a `u64` count, then key/value *pairs* — keys
    /// interleaved with arbitrarily deep values, and *not* required to be
    /// sorted on the wire (duplicate keys overwrite, as `BTreeMap::insert`).
    Map(&'a BTreeMap<String, Par>),
    Oneof(Option<(u32, Payload<'a>)>),
}

// ===========================================================================
// §B  The two object-safe traits the generated table implements
// ===========================================================================

/// A schema node: a `&'static` field program plus positional access to it.
///
/// Object-safe by construction — no associated constants, no generic methods —
/// because the driver's op stack holds `&dyn WireNode`.
pub trait WireNode {
    /// This node's fields, in serde declaration order.
    fn wire_program(&self) -> &'static [FieldKind];
    /// Field `i` of `wire_program()`. Borrowing only: nothing is cloned, so
    /// the encoder never allocates to *read* a term.
    fn wire_field(&self, i: usize) -> FieldVal<'_>;

    /// ⚠ **The one node whose field 0 must be CONSTRUCTED, not borrowed.**
    ///
    /// `EPathMap` overrides this; every generated impl inherits `None`. The
    /// driver asks each node once on the descent and takes the canonical path
    /// only for the map (see [`pathmap_ps`]).
    ///
    /// ★ It is a *trait method* and not a pointer comparison against
    /// [`EPATHMAP_PROGRAM`] for a reason that cost a `SIGSEGV` to learn:
    /// `&'static` slices with identical contents are **merged by the linker**,
    /// and `EPATHMAP_PROGRAM` is `[Seq, EmptyBytes, Bool, Opt]` — byte-for-byte
    /// the same as `ELIST_PROGRAM`, `ESET_PROGRAM` and `EMAP_PROGRAM`. Program
    /// addresses therefore do **not** identify a type, and any downcast built
    /// on them silently reinterprets an `EList` as an `EPathMap`.
    /// `wire_encode_space::program_addresses_do_not_identify_a_type` pins that
    /// fact so the trick cannot be reintroduced as an "optimization".
    #[inline]
    fn wire_as_pathmap(&self) -> Option<&EPathMap> {
        None
    }
}

/// A oneof: its declaration-order index and its payload.
pub trait WireOneof {
    fn wire_variant(&self) -> (u32, Payload<'_>);
}

/// A homogeneous sequence of nodes, erased.
///
/// One blanket impl covers every `Vec<T>` in the schema, so a new repeated
/// field needs no new code here at all.
pub trait WireSeq {
    fn wire_len(&self) -> usize;
    fn wire_get(&self, i: usize) -> &dyn WireNode;
}

impl<T: WireNode> WireSeq for Vec<T> {
    #[inline]
    fn wire_len(&self) -> usize {
        self.len()
    }
    #[inline]
    fn wire_get(&self, i: usize) -> &dyn WireNode {
        &self[i]
    }
}

// ⚠ No `impl WireSeq for [T]`. An unsized type cannot be coerced to a trait
// object, so `&[T] -> &dyn WireSeq` is not expressible; every sequence in the
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
/// invent one. Its layout is the **4-field** shape its hand-written
/// `Serialize` writes (`models/src/rust/rhoapi_ext.rs`): the private `intern`
/// shadow cell is `#[serde(skip)]` and never reaches the wire.
///
/// ⚠ The `ps` field is **not** always `self.ps`. For a GROUND, non-empty map
/// the `Serialize` impl emits `ground_canonical_ps(self)` — the entries in
/// canonical trie order, deduped and recursively canonical — so that a ground
/// map's event-hash preimage is a pure function of its entry SET, independent
/// of the order and multiplicity a producer happened to use. [`ground_ps`]
/// reproduces exactly that choice, and the encoder keeps the canonical vector
/// alive in an owned slot for the duration of the subtree.
pub static EPATHMAP_PROGRAM: &[FieldKind] = &[
    FieldKind::Seq,        // ps            (canonical order when ground)
    FieldKind::EmptyBytes, // locally_free  (always blanked on serialize)
    FieldKind::Bool,       // connective_used
    FieldKind::Opt,        // remainder
];

/// Which `ps` an `EPathMap` serializes.
///
/// `Canonical` carries an owned vector because canonicalization *constructs* a
/// new order; the encoder parks it in a slot rather than borrowing, which is
/// why this returns the vector rather than a slice.
pub enum PathmapPs<'a> {
    /// Ground and non-empty: canonical trie order.
    Canonical(Vec<Par>),
    /// Non-ground or empty: the stored order, borrowed.
    ///
    /// A `&Vec<Par>` rather than a `&[Par]` because only a *sized* type can be
    /// coerced to `&dyn WireSeq`.
    Stored(&'a Vec<Par>),
}

impl WireNode for EPathMap {
    #[inline]
    fn wire_program(&self) -> &'static [FieldKind] {
        EPATHMAP_PROGRAM
    }

    /// ⚠ Field 0 is **never** served from here — the encoder intercepts
    /// `EPathMap` before entering the generic field loop precisely because
    /// `ps` may have to be *constructed* (see [`PathmapPs`]), and a borrowed
    /// `FieldVal` cannot own a freshly built vector. Serving the stored order
    /// here would silently drop the ground canonicalization, so it panics
    /// instead of returning a plausible-but-wrong answer.
    #[inline]
    fn wire_as_pathmap(&self) -> Option<&EPathMap> {
        Some(self)
    }

    #[inline]
    fn wire_field(&self, i: usize) -> FieldVal<'_> {
        match i {
            0 => unreachable!(
                "EPathMap.ps must be read through `wire::pathmap_ps` — a ground map's `ps` is \
                 CONSTRUCTED in canonical order and cannot be served as a borrow"
            ),
            1 => FieldVal::Leaf(LeafVal::EmptyBytes),
            2 => FieldVal::Leaf(LeafVal::Bool(self.connective_used)),
            3 => FieldVal::Opt(self.remainder.as_ref().map(|v| v as &dyn WireNode)),
            _ => unreachable!("field index out of program range"),
        }
    }
}

/// The `ps` an `EPathMap` serializes — the same choice its `Serialize` makes.
///
/// The predicate and the canonicalizer are the *same two functions* the
/// `Serialize` impl calls (`models/src/rust/pathmap_crate_type_mapper.rs`), so
/// this cannot drift into a second opinion about what "ground" means.
pub fn pathmap_ps(map: &EPathMap) -> PathmapPs<'_> {
    use crate::rust::pathmap_crate_type_mapper::{eval_stable_epathmap, ground_canonical_ps};
    if eval_stable_epathmap(map) && !map.ps.is_empty() {
        PathmapPs::Canonical(ground_canonical_ps(map))
    } else {
        PathmapPs::Stored(&map.ps)
    }
}

/// `Var` is reachable as `Option<Var>` (`remainder`) from several programs and
/// is otherwise an ordinary generated node; this alias exists so the import
/// above is load-bearing and the module documents the containment.
#[allow(dead_code)]
type RemainderVar = Var;
