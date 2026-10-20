//! # `bincode_decoder` — the O(1)-native-stack cold-store DECODER for the `Par` family
//!
//! Decode only. The encoder is **not touched**: `Serialize` stays derived and
//! `CandidateOrderingBytes` (replay-visible COMM selection) is encode-only and
//! untouched, so byte identity of the encoding is preserved *by construction*
//! and is not this module's obligation. What this module owes is **language
//! identity** — see [`rspace_plus_plus::rspace::serializers::cold_store_decode`]
//! for the statement and the hazard it closes.
//!
//! ---
//!
//! ## 1. Why a hand-written parser, and why nothing smaller works
//!
//! The goal is to decode a `Par` whose nesting depth is attacker-controlled
//! without letting that depth reach the native stack. Every "just wrap serde"
//! shape was examined and each fails for a *structural* reason, not a
//! contingent one:
//!
//! ```text
//!    idea                        why it cannot work
//!    ─────────────────────────── ──────────────────────────────────────────────
//!    defer a child by skipping   to skip a child you need its BYTE EXTENT; in a
//!    its bytes, decode later     non-self-describing format an extent requires a
//!                                schema-driven parser — and that parser IS this
//!                                module. The reduction is total.
//!
//!    serde::de::IgnoredAny       bincode's `deserialize_ignored_any` returns
//!                                Err("Bincode does not support
//!                                Deserializer::deserialize_ignored_any")
//!                                (bincode 1.3.3, src/de/mod.rs:448-454).
//!
//!    DeserializeSeed             passes state *into* a child; it never returns
//!                                control *out* of a partially-completed
//!                                visitor, which is exactly the suspension
//!                                point a deferral needs.
//!
//!    serde_stacker               the only serde wrapper the ecosystem shipped
//!                                for this: it GROWS THE STACK. Explicitly out
//!                                of scope — a bigger stack moves the cliff, it
//!                                does not remove it.
//! ```
//!
//! So: an explicit machine over `&[u8]`.
//!
//! ## 2. The format, in one paragraph
//!
//! bincode 1.3.3 with `DefaultOptions::new().with_fixint_encoding()
//! .allow_trailing_bytes()` (`bincode/src/lib.rs:177-185`). Integers are
//! fixed-width little-endian; `bool` and `Option` tags are one byte; `u64`
//! lengths precede sequences, maps and byte/string payloads; structs are
//! **positional** (`deserialize_struct` → `deserialize_tuple(fields.len())`, so
//! field *names* never reach the wire and field order is declaration order);
//! enums are a `u32` variant index followed by the payload. There are no
//! back-references and no framing. The stream is therefore a **pre-order
//! prefix encoding of the term tree given the schema** — which is precisely
//! what makes a single left-to-right pass sufficient.
//!
//! ## 3. The machine
//!
//! ```text
//!            ┌─────────────────────────────────────────────────┐
//!            │  ops : Vec<Op>          (the OBLIGATION stack)   │
//!            │    …                                            │
//!            │    SendBuild{has_chan,n}   ← resume point        │
//!            │    Rep{Par, 2}             ← counted repeat      │
//!            │    ParStart                ← next to run         │  ◀── pop
//!            └─────────────────────────────────────────────────┘
//!                                  │ may read bytes, push Ops,
//!                                  │ and/or complete a value
//!                                  ▼
//!            ┌─────────────────────────────────────────────────┐
//!            │ pars, sends, receives, … (per-TYPE value stacks) │
//!            └─────────────────────────────────────────────────┘
//! ```
//!
//! An `Op` is one bounded step of one type's field program. Running it may read
//! a bounded number of bytes, push successor `Op`s, and finally push exactly
//! one completed value onto that type's stack. **All state lives on the heap**;
//! the interpreter loop itself is a flat `while let Some(op) = ops.pop()`.
//!
//! Three properties make this correct and cheap:
//!
//! * **One value per `*Start`.** Every `*Start` op eventually pushes exactly one
//!   value onto its own stack, and every `*Build` op removes exactly the values
//!   its own program pushed. Nesting is therefore properly LIFO and a builder
//!   can take its children with `Vec::split_off(len - n)` — which both preserves
//!   sibling ORDER (the stream's) and avoids a reversal.
//!
//! * **Counted repeats, never materialised.** A sequence of `n` elements does
//!   *not* push `n` ops. [`Op::Rep`] re-pushes itself with `remaining - 1` and
//!   one `*Start`, so a hostile `n = u64::MAX` costs O(1) memory and fails on
//!   the first element that runs out of input — exactly where the derived path
//!   fails.
//!
//! * **No unbounded allocation.** Every value pushed onto a stack consumes at
//!   least one input byte (the cheapest, `Expr { expr_instance: None }`, costs
//!   the 1-byte `Option` tag), so the total number of live values is bounded by
//!   `bytes.len()`. Byte and string payloads are bounds-checked *before*
//!   allocation. There is no input that makes this decoder allocate more than
//!   O(input).
//!
//! ## 4. Which types are on the machine, and why the rest are not
//!
//! **47 types transitively contain `Par`** and are decoded by the machine.
//! **16 do not** and keep the derived impl, reached through the bounded leaf
//! readers in [`Reader`]:
//!
//! ```text
//!   Var  VarInstance  WildcardMsg  VarRef  EVar  GUnforgeable  UnfInstance
//!   GPrivate  GDeployId  GDeployerId  GSysAuthToken  DeployId  DeployerId
//!   PCost  GBigRational  GFixedPoint
//! ```
//!
//! That partition is **principled, not a scope-down**: a type outside the `Par`
//! strongly-connected component has a fixed maximum nesting, hence a fixed
//! maximum stack, hence no depth-dependent frame. The counts are checked
//! against the generated schema by `bincode_decoder_type_partition` in
//! `models/tests/bincode_decoder_shapes.rs`.
//!
//! ## 5. ⚠ The four wire shapes where a hand-written codec drifts
//!
//! Each has a named test in `models/tests/bincode_decoder_shapes.rs`.
//!
//! 1. **`New.injections: BTreeMap<String, Par>`** — the only `btree_map` field
//!    in `RhoTypes.proto`. serde emits a **map**: an 8-byte count, then
//!    key/value *pairs*, keys interleaved with (arbitrarily deep) values. It is
//!    `deserialize_map`, not `deserialize_seq`, and duplicate keys **overwrite**
//!    exactly as `BTreeMap::insert` does — the stream is not required to be
//!    sorted. See [`Op::NewEntry`].
//!
//! 2. **The 12 `serialize_with = serialize_as_empty_bytes` sites** (`Par`,
//!    `Send`, `Receive`, `New`, `Match`, `If`, `EList`, `ETuple`, `ESet`,
//!    `EMap`, `EMethod`, `EZipper`; plus the hand-written equivalent inside
//!    `EPathMap::serialize`). These are **written** as `serialize_bytes(&[])`
//!    — eight zero bytes — but **read** back as a plain `Vec<u8>` through
//!    `deserialize_seq`. The asymmetry is deliberate
//!    (`models/src/rust/serde_helpers.rs`: `locally_free` is transient analysis
//!    data that must not affect RSpace channel hashes). The decoder therefore
//!    reads the stream's **real** length and never assumes zero — anything else
//!    would reject byte strings the derived decoder accepts.
//!
//! 3. **`EPathMap`** (`models/src/rust/rhoapi_ext.rs`) has **4 serde fields**.
//!    `locally_free` is blanked on *serialize only*, and the retained derived
//!    `Deserialize` reads the real bytes. `ps` is an `EntryTrie` that serializes
//!    as one canonical EPM1 byte array:
//!
//!    ```text
//!      u64-LE |EPM1(m)| ‖ EPM1(m)
//!    ```
//!
//!    EPM1 embeds the compact ACTree03 topology and the stack-safe map-value
//!    table, so bincode performs one contiguous byte-slice read and no entry
//!    projection or recursive value decode. See [`Op::PathmapBuild`].
//!
//! 4. **The oneofs** (`ExprInstance` 37 arms, `ConnectiveInstance` 9,
//!    `TaggedCont` 2, `CostSignature.Value` 6) are an `Option` tag (**1** byte) *then* a variant index
//!    (**4** bytes, `u32` fixint-LE) — two reads, not one. And the index is
//!    serde's **declaration order**, not the proto tag: `EPathmapBody` is proto
//!    tag 32 but serde index 25. The numbering lives in exactly one place,
//!    [`crate::rust::rholang::par_children::expr_instance_variant_index`], and
//!    this module is gated against it by `bincode_decoder_variant_indices_agree`.
//!
//! ## 6. ⚠ The rejection set is consensus-visible
//!
//! A node that accepts a byte string another node rejects **forks**. The
//! decoder therefore reproduces the derived decoder's failures exactly, not
//! merely "some failure":
//!
//! | derived (bincode 1.3.3) | here |
//! |---|---|
//! | `InvalidBoolEncoding(b)` for a `bool` byte ∉ {0,1} (`src/de/mod.rs:132-141`) | [`ColdStoreDecodeError::InvalidBoolEncoding`] |
//! | `InvalidTagEncoding(v)` for an `Option` tag > 1 (`src/de/mod.rs:332-342`) | [`ColdStoreDecodeError::InvalidTagEncoding`] |
//! | `Custom("invalid value: integer i, expected variant index 0 <= i < N")` | [`ColdStoreDecodeError::InvalidVariantIndex`] |
//! | `InvalidUtf8Encoding` from `String::from_utf8` (`src/de/mod.rs:99-102`) | [`ColdStoreDecodeError::InvalidUtf8Encoding`] |
//! | `Io(UnexpectedEof)` from `SliceReader::get_byte_slice` | [`ColdStoreDecodeError::UnexpectedEof`] |
//!
//! Two subtleties that are easy to get wrong:
//!
//! * **`size_hint::cautious`.** serde 1.0.228 caps a sequence's
//!   pre-allocation at `min(hint, 1 MiB / size_of::<Element>())`
//!   (`serde/src/core/private/size_hint.rs:12-23`). Without an equivalent cap a
//!   `u64` length near `usize::MAX` becomes an out-of-memory abort **that the
//!   derived path does not have**. [`cautious_capacity`] mirrors the formula;
//!   the counted-repeat design means the `Par`-bearing sequences never
//!   pre-allocate at all. (Historical note: older serde used a flat
//!   4,096-element cap; 1.0.228 uses the byte-budget form above, and the form
//!   pinned here is the one in the lockfile.)
//!
//! * **`allow_trailing_bytes`.** Trailing bytes are **permitted**. Tightening
//!   that to "must consume the whole buffer" would *narrow* the accepted
//!   language, which is itself a fork.
//!   [`ColdStoreDecode::cold_decode`] ignores the consumed count.
//!
//! ## 7. Teardown
//!
//! `<Par as Drop>` is itself a Θ(depth) recursive traversal (measured 470
//! B/level, debug). A decoder that *failed* at the last byte of a deep term
//! would therefore abort while releasing the partial result — converting a
//! clean rejection back into the `SIGSEGV` this work exists to remove. The
//! machine's [`Drop`] funnels every value stack through
//! [`crate::rust::rholang::par_children::dismantle_all`], the existing
//! explicit-worklist teardown, so an error path is O(1) native stack too.

use std::collections::BTreeMap;
use std::mem;

use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError,
};

use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::cost_signature::Value as CostValue;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::var::{VarInstance, WildcardMsg};
use crate::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, CostAuthority, CostRegion, CostSignature,
    CostSignatureCompound, CostSignedTerm, CostStack, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte,
    EMap, EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr,
    EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GAuthorityId,
    GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrincipalId, GPrivate, GSysAuthToken,
    GUnforgeable, If, KeyValuePair, ListBindPatterns, ListParWithRandom, Match, MatchCase, New,
    Par, ParWithRandom, Receive, ReceiveBind, Send, TaggedContinuation, Var, VarRef,
};
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::par_children::{
    dismantle_all, CONNECTIVE_INSTANCE_VARIANT_COUNT, EXPR_INSTANCE_VARIANT_COUNT,
};

type Res<T> = Result<T, ColdStoreDecodeError>;

// ===========================================================================
// §A  The serde variant indices, as named constants
// ===========================================================================
//
// Declaration order in the generated `expr::ExprInstance` — see
// `par_children::expr_instance_variant_index`, the canonical table this module
// is gated against.

// ★ ONE TABLE, BOTH DIRECTIONS. These indices are no longer transcribed here:
// they are `pub const`s emitted by `models/codegen/schema.rs` from the same
// protobuf `FileDescriptorSet` that drives the serializer
// (`crate::rust::rholang::bincode_encoder`). Thirty-six hand-written `EX_*`
// literals and nine `CN_*` literals used to live in this block; a 37th oneof
// arm would have left every one of them correct and every assertion about them
// passing, while the new arm went untested. Generation removes that failure
// mode by construction.
//
// ⚠ The indices are serde DECLARATION ORDER, never the proto tag —
// `EX_E_PATHMAP_BODY` is 25 and its proto tag is 32.
use crate::rust::rholang::bincode_schema_tables::{
    CN_CONN_AND_BODY, CN_CONN_BOOL, CN_CONN_BYTE_ARRAY, CN_CONN_INT, CN_CONN_NOT_BODY,
    CN_CONN_OR_BODY, CN_CONN_STRING, CN_CONN_URI, CN_VAR_REF_BODY, CS_BOUND_LEVEL, CS_COMPOUND,
    CS_GROUND, CS_NAME, CS_QUOTE, CS_UNIT, EX_E_AND_BODY, EX_E_DIV_BODY, EX_E_EQ_BODY,
    EX_E_GTE_BODY, EX_E_GT_BODY, EX_E_LIST_BODY, EX_E_LTE_BODY, EX_E_LT_BODY, EX_E_MAP_BODY,
    EX_E_MATCHES_BODY, EX_E_METHOD_BODY, EX_E_MINUS_BODY, EX_E_MINUS_MINUS_BODY, EX_E_MOD_BODY,
    EX_E_MULT_BODY, EX_E_NEG_BODY, EX_E_NEQ_BODY, EX_E_NOT_BODY, EX_E_OR_BODY, EX_E_PATHMAP_BODY,
    EX_E_PERCENT_PERCENT_BODY, EX_E_PLUS_BODY, EX_E_PLUS_PLUS_BODY, EX_E_SET_BODY, EX_E_TUPLE_BODY,
    EX_E_VAR_BODY, EX_E_ZIPPER_BODY, EX_G_BIG_INT, EX_G_BIG_RAT, EX_G_BOOL, EX_G_BYTE_ARRAY,
    EX_G_DOUBLE, EX_G_FIXED_POINT, EX_G_FLOAT32, EX_G_INT, EX_G_INT32, EX_G_STRING, EX_G_UINT16,
    EX_G_UINT32, EX_G_UINT64, EX_G_UINT8, EX_G_URI,
    TAGGED_CONT_VARIANT_COUNT as TAGGED_CONT_VARIANTS_LEN,
    UNF_INSTANCE_VARIANT_COUNT as UNF_INSTANCE_VARIANTS_LEN,
    VALUE_VARIANT_COUNT as COST_VALUE_VARIANTS_LEN,
    VAR_INSTANCE_VARIANT_COUNT as VAR_INSTANCE_VARIANTS_LEN,
};

/// `var::VarInstance`: `BoundVar`, `FreeVar`, `Wildcard` — ★ generated.
const VAR_INSTANCE_VARIANT_COUNT: u32 = VAR_INSTANCE_VARIANTS_LEN as u32;
/// `g_unforgeable::UnfInstance`: the six unforgeable bodies — ★ generated.
const UNF_INSTANCE_VARIANT_COUNT: u32 = UNF_INSTANCE_VARIANTS_LEN as u32;
const COST_VALUE_VARIANT_COUNT: u32 = COST_VALUE_VARIANTS_LEN as u32;
/// `tagged_continuation::TaggedCont`: `ParBody`, `ScalaBodyRef` — ★ generated.
const TAGGED_CONT_VARIANT_COUNT: u32 = TAGGED_CONT_VARIANTS_LEN as u32;

/// The `ExprInstance` arms whose wire shape is `(Option<Par>, Option<Par>)`.
///
/// Seventeen of the thirty-six arms share this shape exactly. Folding them into
/// one program is not a shortcut: it removes seventeen opportunities to write
/// the same three-step field walk seventeen times and get one of them wrong.
/// [`binary_expr_instance`] is the only place the arm identity re-enters.
const BINARY_EXPR_VARIANTS: [u32; 17] = [
    EX_E_MULT_BODY,
    EX_E_DIV_BODY,
    EX_E_PLUS_BODY,
    EX_E_MINUS_BODY,
    EX_E_LT_BODY,
    EX_E_LTE_BODY,
    EX_E_GT_BODY,
    EX_E_GTE_BODY,
    EX_E_EQ_BODY,
    EX_E_NEQ_BODY,
    EX_E_AND_BODY,
    EX_E_OR_BODY,
    EX_E_MATCHES_BODY,
    EX_E_PERCENT_PERCENT_BODY,
    EX_E_PLUS_PLUS_BODY,
    EX_E_MINUS_MINUS_BODY,
    EX_E_MOD_BODY,
];

/// Rebuild a binary `ExprInstance` arm from its variant index and two operands.
///
/// `EMatches` names its fields `target`/`pattern` rather than `p1`/`p2`; the
/// wire is identical (two `Option<Par>` in declaration order), which is why it
/// belongs to this family.
fn binary_expr_instance(variant: u32, p1: Option<Par>, p2: Option<Par>) -> Res<ExprInstance> {
    Ok(match variant {
        EX_E_MULT_BODY => ExprInstance::EMultBody(EMult { p1, p2 }),
        EX_E_DIV_BODY => ExprInstance::EDivBody(EDiv { p1, p2 }),
        EX_E_PLUS_BODY => ExprInstance::EPlusBody(EPlus { p1, p2 }),
        EX_E_MINUS_BODY => ExprInstance::EMinusBody(EMinus { p1, p2 }),
        EX_E_LT_BODY => ExprInstance::ELtBody(ELt { p1, p2 }),
        EX_E_LTE_BODY => ExprInstance::ELteBody(ELte { p1, p2 }),
        EX_E_GT_BODY => ExprInstance::EGtBody(EGt { p1, p2 }),
        EX_E_GTE_BODY => ExprInstance::EGteBody(EGte { p1, p2 }),
        EX_E_EQ_BODY => ExprInstance::EEqBody(EEq { p1, p2 }),
        EX_E_NEQ_BODY => ExprInstance::ENeqBody(ENeq { p1, p2 }),
        EX_E_AND_BODY => ExprInstance::EAndBody(EAnd { p1, p2 }),
        EX_E_OR_BODY => ExprInstance::EOrBody(EOr { p1, p2 }),
        EX_E_MATCHES_BODY => ExprInstance::EMatchesBody(EMatches {
            target: p1,
            pattern: p2,
        }),
        EX_E_PERCENT_PERCENT_BODY => ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 }),
        EX_E_PLUS_PLUS_BODY => ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 }),
        EX_E_MINUS_MINUS_BODY => ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 }),
        EX_E_MOD_BODY => ExprInstance::EModBody(EMod { p1, p2 }),
        _ => {
            return Err(ColdStoreDecodeError::MachineInvariant(
                "binary ExprInstance arm",
            ));
        }
    })
}

// ===========================================================================
// §B  The reader — bincode 1.3.3 legacy fixint-LE primitives over `&[u8]`
// ===========================================================================

/// serde 1.0.228's sequence pre-allocation cap, reproduced exactly
/// (`serde/src/core/private/size_hint.rs:12-23`).
///
/// Without it, a `u64` length near `usize::MAX` would become an out-of-memory
/// abort here while the derived path merely returns `Err` — a divergence in the
/// *disposition* of a malformed input, which is the consensus-visible half of a
/// decoder.
pub(crate) fn cautious_capacity<T>(hint: usize) -> usize {
    const MAX_PREALLOC_BYTES: usize = 1024 * 1024;
    if mem::size_of::<T>() == 0 {
        0
    } else {
        hint.min(MAX_PREALLOC_BYTES / mem::size_of::<T>())
    }
}

/// A cursor over the encoded bytes exposing exactly bincode's legacy
/// fixint-little-endian primitives, with bincode's bounds-check-before-allocate
/// discipline (`SliceReader::get_byte_slice`, `bincode/src/de/read.rs:46-54`).
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self { Reader { bytes, pos: 0 } }

    fn consumed(&self) -> usize { self.pos }

    fn remaining(&self) -> usize { self.bytes.len() - self.pos }

    /// Bounds-checked slice take. Every payload read goes through here, so no
    /// allocation is ever sized by an unvalidated stream length.
    fn take(&mut self, n: usize, wanted: &'static str) -> Res<&'a [u8]> {
        if n > self.remaining() {
            return Err(ColdStoreDecodeError::UnexpectedEof {
                wanted,
                needed: n,
                available: self.remaining(),
            });
        }
        let out = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn byte(&mut self, wanted: &'static str) -> Res<u8> { Ok(self.take(1, wanted)?[0]) }

    /// `deserialize_bool` — byte ∉ {0,1} is `InvalidBoolEncoding`
    /// (`bincode/src/de/mod.rs:132-141`).
    fn bool(&mut self) -> Res<bool> {
        match self.byte("a bool")? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(ColdStoreDecodeError::InvalidBoolEncoding(other)),
        }
    }

    /// `deserialize_option` — tag ∉ {0,1} is `InvalidTagEncoding`
    /// (`bincode/src/de/mod.rs:332-342`). A DIFFERENT error from the `bool`
    /// case even though both read one byte; reproducing the distinction keeps
    /// the two rejection reasons aligned with the derive.
    fn option_tag(&mut self) -> Res<bool> {
        match self.byte("an Option tag")? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(ColdStoreDecodeError::InvalidTagEncoding(other as usize)),
        }
    }

    fn u32(&mut self) -> Res<u32> {
        let b = self.take(4, "a u32")?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Res<i32> { self.u32().map(|v| v as i32) }

    fn u64(&mut self) -> Res<u64> {
        let b = self.take(8, "a u64")?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn i64(&mut self) -> Res<i64> { self.u64().map(|v| v as i64) }

    /// `IntEncoding::deserialize_len` — a `u64` narrowed to `usize`
    /// (`bincode/src/config/int.rs:69-73`, `cast_u64_to_usize` at :593).
    ///
    /// On a 64-bit target the narrowing never fails, and an oversized length is
    /// caught downstream by [`Reader::take`] or by the counted repeat running
    /// out of input — the same place the derived path catches it.
    fn len(&mut self) -> Res<usize> {
        let n = self.u64()?;
        usize::try_from(n).map_err(|_| ColdStoreDecodeError::LengthOverflow(n))
    }

    /// A `Vec<u8>` field, **borrowed** out of the input rather than copied.
    ///
    /// serde has **no** specialisation for `Vec<u8>`: it uses the generic
    /// sequence impl, so the wire is a `u64` count followed by that many
    /// single-byte elements — byte-identical to what `serialize_bytes` writes,
    /// which is why the `serialize_as_empty_bytes` asymmetry (shape 2) is
    /// invisible to the *format* and visible only to the *values*.
    ///
    /// ★ Exists for EPathMap's EPM1 snapshot, which the iterative ACT/value
    /// decoder validates in place — copying it would allocate a second image of
    /// a byte string already in the caller's buffer.
    fn byte_slice(&mut self) -> Res<&'a [u8]> {
        let n = self.len()?;
        self.take(n, "a byte sequence")
    }

    /// An owned `Vec<u8>` field — [`Reader::byte_slice`] plus the copy. The two
    /// share the length read and the `wanted` label, so their `UnexpectedEof`
    /// disposition cannot drift apart.
    fn byte_seq(&mut self) -> Res<Vec<u8>> { self.byte_slice().map(<[u8]>::to_vec) }

    /// `String::deserialize` → `deserialize_string` → `read_string`: read the
    /// length-prefixed bytes **first** (so a short buffer is `UnexpectedEof`,
    /// not a UTF-8 error), then validate UTF-8
    /// (`bincode/src/de/mod.rs:99-102`). The order matters: it decides which of
    /// two errors a doubly-malformed input produces, and both nodes must make
    /// the same choice.
    fn string(&mut self) -> Res<String> {
        let raw = self.byte_seq()?;
        String::from_utf8(raw).map_err(|_| ColdStoreDecodeError::InvalidUtf8Encoding)
    }

    fn string_seq(&mut self) -> Res<Vec<String>> {
        let n = self.len()?;
        let mut out = Vec::with_capacity(cautious_capacity::<String>(n));
        for _ in 0..n {
            out.push(self.string()?);
        }
        Ok(out)
    }

    fn byte_seq_seq(&mut self) -> Res<Vec<Vec<u8>>> {
        let n = self.len()?;
        let mut out = Vec::with_capacity(cautious_capacity::<Vec<u8>>(n));
        for _ in 0..n {
            out.push(self.byte_seq()?);
        }
        Ok(out)
    }

    /// A oneof variant index: `u32` fixint-LE, range-checked. The derived path
    /// reaches the same rejection through `serde::de::Error::invalid_value` on
    /// the `U32Deserializer` handed to the field seed
    /// (`bincode/src/de/mod.rs:280-287`).
    fn variant(&mut self, type_name: &'static str, count: u32) -> Res<u32> {
        let index = self.u32()?;
        if index >= count {
            return Err(ColdStoreDecodeError::InvalidVariantIndex {
                type_name,
                index,
                count,
            });
        }
        Ok(index)
    }

    // -------- bounded leaves: the 16 types outside the `Par` SCC -----------

    fn var(&mut self) -> Res<Var> {
        if !self.option_tag()? {
            return Ok(Var { var_instance: None });
        }
        let instance = match self.variant("VarInstance", VAR_INSTANCE_VARIANT_COUNT)? {
            0 => VarInstance::BoundVar(self.i32()?),
            1 => VarInstance::FreeVar(self.i32()?),
            // `WildcardMsg` is a zero-field struct: `deserialize_tuple(0)`
            // consumes NOTHING.
            _ => VarInstance::Wildcard(WildcardMsg {}),
        };
        Ok(Var {
            var_instance: Some(instance),
        })
    }

    fn opt_var(&mut self) -> Res<Option<Var>> {
        if self.option_tag()? {
            Ok(Some(self.var()?))
        } else {
            Ok(None)
        }
    }

    fn unforgeable(&mut self) -> Res<GUnforgeable> {
        if !self.option_tag()? {
            return Ok(GUnforgeable { unf_instance: None });
        }
        let instance = match self.variant("UnfInstance", UNF_INSTANCE_VARIANT_COUNT)? {
            0 => UnfInstance::GPrivateBody(GPrivate {
                id: self.byte_seq()?,
            }),
            1 => UnfInstance::GDeployIdBody(GDeployId {
                sig: self.byte_seq()?,
            }),
            2 => UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: self.byte_seq()?,
            }),
            3 => UnfInstance::GSysAuthTokenBody(GSysAuthToken {}),
            4 => UnfInstance::GAuthorityIdBody(GAuthorityId {
                id: self.byte_seq()?,
            }),
            5 => UnfInstance::GPrincipalIdBody(GPrincipalId {
                key_family: self.u32()?,
                public_key: self.byte_seq()?,
            }),
            _ => return Err(ColdStoreDecodeError::MachineInvariant("UnfInstance arm")),
        };
        Ok(GUnforgeable {
            unf_instance: Some(instance),
        })
    }

    fn unforgeable_seq(&mut self) -> Res<Vec<GUnforgeable>> {
        let n = self.len()?;
        let mut out = Vec::with_capacity(cautious_capacity::<GUnforgeable>(n));
        for _ in 0..n {
            out.push(self.unforgeable()?);
        }
        Ok(out)
    }
}

// ===========================================================================
// §C  The opcode alphabet
// ===========================================================================

/// The element type of a counted repeat.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Par,
    Send,
    Receive,
    Bind,
    New,
    Match,
    Case,
    If,
    Bundle,
    Expr,
    Connective,
    Kv,
    BindPattern,
    CostSignature,
    CostSignedTerm,
    CostStack,
    CostRegion,
    // Disabled by the merge of feature/cost-accounted-rho (lint fix): no code
    // constructs this kind. A cost authority is optional and single, so the
    // decoder pushes `Op::CostAuthorityStart` directly.
    // CostAuthority,
}

impl Kind {
    fn start(self) -> Op {
        match self {
            Kind::Par => Op::ParStart,
            Kind::Send => Op::SendStart,
            Kind::Receive => Op::ReceiveStart,
            Kind::Bind => Op::BindStart,
            Kind::New => Op::NewStart,
            Kind::Match => Op::MatchStart,
            Kind::Case => Op::CaseStart,
            Kind::If => Op::IfStart,
            Kind::Bundle => Op::BundleStart,
            Kind::Expr => Op::ExprStart,
            Kind::Connective => Op::ConnStart,
            Kind::Kv => Op::KvStart,
            Kind::BindPattern => Op::BindPatternStart,
            Kind::CostSignature => Op::CostSignatureStart,
            Kind::CostSignedTerm => Op::CostSignedTermStart,
            Kind::CostStack => Op::CostStackStart,
            Kind::CostRegion => Op::CostRegionStart,
            // Disabled with the `Kind::CostAuthority` variant above.
            // Kind::CostAuthority => Op::CostAuthorityStart,
        }
    }
}

/// One bounded step of one type's field program.
///
/// Naming: `*Start` opens a value, `*Build` closes it (pushing exactly one
/// completed value onto that type's stack), and anything in between is a
/// **resume point** — the state that has to survive a child's arbitrarily deep
/// descent. Scalars read before a descent are carried in the op itself when
/// they are small, and on a side stack when they are not (see [`ParFrame`],
/// [`ReceiveTail`], [`NewFrame`], and [`Machine::method_names`]).
#[derive(Clone, Copy, Debug)]
enum Op {
    /// The counted repeat. Never materialises `remaining` ops; see §3.
    Rep {
        kind: Kind,
        remaining: usize,
    },

    // ---- Par: sends, receives, news, exprs, matches, unforgeables,
    //           bundles, connectives, conditionals, locally_free,
    //           connective_used
    ParStart,
    /// `field` indexes the eight repeated `Par`-bearing fields in declaration
    /// order; `unforgeables` (a bounded leaf sequence) is read inline just
    /// before field 5, which is where it sits in the stream.
    ParField {
        field: u8,
    },
    ParBuild,
    ParCostSigned,
    ParCostStacks,

    // ---- Send: chan, data, persistent, locally_free, connective_used
    SendStart,
    SendData {
        has_chan: bool,
    },
    SendBuild {
        has_chan: bool,
        n_data: usize,
    },

    // ---- Receive: binds, body, persistent, peek, bind_count, locally_free,
    //               connective_used, condition
    ReceiveStart,
    ReceiveBody {
        n_binds: usize,
    },
    ReceiveCondition {
        n_binds: usize,
        has_body: bool,
    },
    ReceiveBuild {
        n_binds: usize,
        has_body: bool,
        has_condition: bool,
    },

    // ---- ReceiveBind: patterns, source, remainder, free_count
    BindStart,
    BindSource {
        n_patterns: usize,
    },
    BindBuild {
        n_patterns: usize,
        has_source: bool,
    },
    BindCostBuild {
        n_patterns: usize,
        has_source: bool,
        has_signature: bool,
    },

    CostSignatureStart,
    CostSignatureParBuild {
        variant: u32,
    },
    CostSignatureCompoundBuild {
        count: usize,
    },
    CostSignedTermStart,
    CostSignedTermSignature {
        has_body: bool,
    },
    CostSignedTermBuild {
        has_body: bool,
        has_signature: bool,
    },
    CostStackStart,
    CostStackParallelTail {
        n_cells: usize,
    },
    CostStackBuild {
        n_cells: usize,
        n_branches: usize,
    },
    CostRegionStart,
    CostRegionBuild {
        has_signature: bool,
    },
    CostAuthorityStart,
    CostAuthorityBuild {
        count: usize,
    },

    // ---- New: bind_count, p, uri, injections, locally_free
    NewStart,
    NewAfterP {
        has_p: bool,
    },
    /// One `BTreeMap` entry: read the key, then descend for the value.
    NewEntry {
        remaining: usize,
    },
    NewBuild {
        has_p: bool,
        n_entries: usize,
    },

    // ---- Match: target, cases, locally_free, connective_used
    MatchStart,
    MatchCases {
        has_target: bool,
    },
    MatchBuild {
        has_target: bool,
        n_cases: usize,
    },

    // ---- MatchCase: pattern, source, free_count, guard
    CaseStart,
    CaseAfterPattern {
        t_pattern: bool,
    },
    CaseAfterSource {
        t_pattern: bool,
        t_source: bool,
    },
    CaseBuild {
        t_pattern: bool,
        t_source: bool,
        free_count: i32,
        t_guard: bool,
    },

    // ---- If: condition, if_true, if_false, locally_free, connective_used
    IfStart,
    IfAfterCondition {
        t1: bool,
    },
    IfAfterTrue {
        t1: bool,
        t2: bool,
    },
    IfBuild {
        t1: bool,
        t2: bool,
        t3: bool,
    },

    // ---- Bundle: body, write_flag, read_flag
    BundleStart,
    BundleBuild {
        has_body: bool,
    },

    // ---- Expr / ExprInstance
    ExprStart,
    ExprUnaryBuild {
        variant: u32,
        has_p: bool,
    },
    ExprBinaryAfter1 {
        variant: u32,
        t1: bool,
    },
    ExprBinaryBuild {
        variant: u32,
        t1: bool,
        t2: bool,
    },
    /// `EList` / `ESet`: ps, locally_free, connective_used, remainder.
    ExprSeqBuild {
        variant: u32,
        n: usize,
    },
    /// `ETuple`: ps, locally_free, connective_used — **no** remainder.
    ExprTupleBuild {
        n: usize,
    },
    /// `EMap`: kvs, locally_free, connective_used, remainder.
    ExprMapBuild {
        n: usize,
    },
    ExprMethodArgs {
        has_target: bool,
    },
    ExprMethodBuild {
        has_target: bool,
        n_args: usize,
    },
    /// Wrap the `EPathMap` on top of the pathmap stack into an `Expr`.
    ExprFromPathmap,
    ExprZipperBuild {
        has_pathmap: bool,
    },

    // ---- EPathMap: EPM1 snapshot, locally_free, connective_used, remainder
    PathmapStart,
    /// The borrowed EPM1 slice rides on [`Machine::trie_snapshots`] because a
    /// `&[u8]` cannot live in a lifetime-free `Op` — the same side-frame
    /// discipline `ParFrame` and `NewFrame` follow.
    PathmapBuild,

    // ---- KeyValuePair: key, value
    KvStart,
    KvAfterKey {
        t_key: bool,
    },
    KvBuild {
        t_key: bool,
        t_value: bool,
    },

    // ---- Connective / ConnectiveInstance
    ConnStart,
    ConnBodyBuild {
        variant: u32,
        n: usize,
    },
    ConnNotBuild,

    // ---- TaggedContinuation: guard, cost_authority, tagged_cont
    TaggedContinuationStart,
    TaggedContinuationCont {
        t_guard: bool,
    },
    TaggedContinuationTagged {
        t_guard: bool,
        has_authority: bool,
    },
    /// `TaggedCont::ParBody(ParWithRandom { body, random_state })`.
    TaggedContinuationParBody {
        t_guard: bool,
        t_body: bool,
        has_authority: bool,
    },
    /// `TaggedCont::ScalaBodyRef(i64)`, value already read.
    TaggedContinuationScala {
        t_guard: bool,
        value: i64,
        has_authority: bool,
    },
    /// `tagged_cont` absent.
    TaggedContinuationAbsent {
        t_guard: bool,
        has_authority: bool,
    },

    // ---- ListParWithRandom: pars, random_state
    ListParWithRandomStart,
    ListParWithRandomBuild {
        n: usize,
    },
    ListParWithRandomCostStack {
        has_authority: bool,
    },
    ListParWithRandomFinish {
        has_stack: bool,
    },

    // ---- ParWithRandom: body, random_state (standalone entry)
    ParWithRandomStart,
    ParWithRandomBuild {
        has_body: bool,
    },

    // ---- BindPattern: patterns, remainder, free_count
    BindPatternStart,
    BindPatternBuild {
        n: usize,
    },

    // ---- ListBindPatterns: patterns
    ListBindPatternsStart,
    ListBindPatternsBuild {
        n: usize,
    },
}

// ===========================================================================
// §D  Side frames — state too large to ride in an `Op`
// ===========================================================================

/// `Par`'s eight repeated-child counts plus its `unforgeables` (read between
/// `matches` and `bundles`, and therefore obliged to survive three more
/// descents). Kept off the op stack because eight `usize`s in every `Op` would
/// inflate the whole alphabet.
#[derive(Default)]
struct ParFrame {
    counts: [usize; 8],
    unforgeables: Vec<GUnforgeable>,
    locally_free: Vec<u8>,
    connective_used: bool,
    n_cost_signed_terms: usize,
    n_cost_stacks: usize,
}

struct BindTail {
    remainder: Option<Var>,
    free_count: i32,
}

/// `Receive`'s five scalars, read between `body` and `condition` and therefore
/// obliged to survive the `condition` descent.
struct ReceiveTail {
    persistent: bool,
    peek: bool,
    bind_count: i32,
    locally_free: Vec<u8>,
    connective_used: bool,
}

/// `New`'s scalars and its injection-map keys. The keys are read interleaved
/// with the (arbitrarily deep) values — wire shape 1.
struct NewFrame {
    bind_count: i32,
    uri: Vec<String>,
    keys: Vec<String>,
}

// ===========================================================================
// §E  The machine
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum DecodeLayout {
    Legacy,
    CostV1,
    CostV2,
}

struct Machine<'a> {
    r: Reader<'a>,
    layout: DecodeLayout,
    ops: Vec<Op>,

    // per-type value stacks
    pars: Vec<Par>,
    sends: Vec<Send>,
    receives: Vec<Receive>,
    binds: Vec<ReceiveBind>,
    cost_signatures: Vec<CostSignature>,
    cost_signed_terms: Vec<CostSignedTerm>,
    cost_stacks: Vec<CostStack>,
    cost_regions: Vec<CostRegion>,
    cost_authorities: Vec<CostAuthority>,
    news: Vec<New>,
    matches: Vec<Match>,
    cases: Vec<MatchCase>,
    ifs: Vec<If>,
    bundles: Vec<Bundle>,
    exprs: Vec<Expr>,
    connectives: Vec<Connective>,
    kvs: Vec<KeyValuePair>,
    pathmaps: Vec<EPathMap>,
    bind_patterns: Vec<BindPattern>,
    list_bind_patterns: Vec<ListBindPatterns>,
    par_with_randoms: Vec<ParWithRandom>,
    list_par_with_randoms: Vec<ListParWithRandom>,
    tagged_continuations: Vec<TaggedContinuation>,

    // side frames
    par_frames: Vec<ParFrame>,
    bind_tails: Vec<BindTail>,
    region_ids: Vec<Vec<u8>>,
    receive_tails: Vec<ReceiveTail>,
    new_frames: Vec<NewFrame>,
    method_names: Vec<String>,
    /// One live EPM1 slice per open EPathMap, read at `PathmapStart` and
    /// consumed at `PathmapBuild` after the three metadata fields.
    ///
    /// ★ A **borrow** into the input buffer, never a copy: the slice is
    /// validated and then dropped, so an owned `Vec<u8>` would be a second image
    /// of bytes the caller already holds. It is a side frame rather than an
    /// `Op` payload because `Op` carries no lifetime.
    trie_snapshots: Vec<&'a [u8]>,
}

fn take_n<T>(stack: &mut Vec<T>, n: usize, what: &'static str) -> Res<Vec<T>> {
    if stack.len() < n {
        return Err(ColdStoreDecodeError::MachineInvariant(what));
    }
    Ok(stack.split_off(stack.len() - n))
}

fn take_one<T>(stack: &mut Vec<T>, what: &'static str) -> Res<T> {
    stack
        .pop()
        .ok_or(ColdStoreDecodeError::MachineInvariant(what))
}

/// Pop a `Par` when `present`, else `None`.
fn take_opt_par(stack: &mut Vec<Par>, present: bool, what: &'static str) -> Res<Option<Par>> {
    if present {
        Ok(Some(take_one(stack, what)?))
    } else {
        Ok(None)
    }
}

impl<'a> Machine<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Machine {
            r: Reader::new(bytes),
            layout: DecodeLayout::CostV2,
            ops: Vec::with_capacity(64),
            pars: Vec::new(),
            sends: Vec::new(),
            receives: Vec::new(),
            binds: Vec::new(),
            cost_signatures: Vec::new(),
            cost_signed_terms: Vec::new(),
            cost_stacks: Vec::new(),
            cost_regions: Vec::new(),
            cost_authorities: Vec::new(),
            news: Vec::new(),
            matches: Vec::new(),
            cases: Vec::new(),
            ifs: Vec::new(),
            bundles: Vec::new(),
            exprs: Vec::new(),
            connectives: Vec::new(),
            kvs: Vec::new(),
            pathmaps: Vec::new(),
            bind_patterns: Vec::new(),
            list_bind_patterns: Vec::new(),
            par_with_randoms: Vec::new(),
            list_par_with_randoms: Vec::new(),
            tagged_continuations: Vec::new(),
            par_frames: Vec::new(),
            bind_tails: Vec::new(),
            region_ids: Vec::new(),
            receive_tails: Vec::new(),
            new_frames: Vec::new(),
            method_names: Vec::new(),
            trie_snapshots: Vec::new(),
        }
    }

    fn new_legacy(bytes: &'a [u8]) -> Self {
        let mut machine = Self::new(bytes);
        machine.layout = DecodeLayout::Legacy;
        machine
    }

    fn new_cost_v1(bytes: &'a [u8]) -> Self {
        let mut machine = Self::new(bytes);
        machine.layout = DecodeLayout::CostV1;
        machine
    }

    /// Schedule `count` repetitions of `kind` as ONE op. See §3 on why this is
    /// not `for _ in 0..count { ops.push(..) }`.
    fn repeat(&mut self, kind: Kind, count: usize) {
        if count > 0 {
            self.ops.push(Op::Rep {
                kind,
                remaining: count,
            });
        }
    }

    /// Schedule a child `Par` decode iff `present`.
    fn opt_par_child(&mut self, present: bool) {
        if present {
            self.ops.push(Op::ParStart);
        }
    }

    fn push_expr(&mut self, instance: ExprInstance) {
        self.exprs.push(Expr {
            expr_instance: Some(instance),
        });
    }

    fn par_frame(&mut self) -> Res<&mut ParFrame> {
        self.par_frames
            .last_mut()
            .ok_or(ColdStoreDecodeError::MachineInvariant("Par frame"))
    }

    fn new_frame(&mut self) -> Res<&mut NewFrame> {
        self.new_frames
            .last_mut()
            .ok_or(ColdStoreDecodeError::MachineInvariant("New frame"))
    }

    /// The interpreter loop. Flat by construction: the only recursion in this
    /// module is inside the bounded leaf readers of [`Reader`].
    fn run(&mut self) -> Res<()> {
        while let Some(op) = self.ops.pop() {
            self.step(op)?;
        }
        Ok(())
    }

    /// After a successful run exactly one value must remain, on the root's own
    /// stack. Anything else means a `*Build` did not drain what its `*Start`
    /// produced — a decoder defect, not a bad input, and one that would
    /// otherwise show up as a silently truncated term.
    fn assert_drained(&self, allow: DrainedRoot) -> Res<()> {
        let counts = [
            ("pars", self.pars.len(), allow == DrainedRoot::Par),
            ("sends", self.sends.len(), false),
            ("receives", self.receives.len(), false),
            ("binds", self.binds.len(), false),
            ("cost_signatures", self.cost_signatures.len(), false),
            ("cost_signed_terms", self.cost_signed_terms.len(), false),
            ("cost_stacks", self.cost_stacks.len(), false),
            ("cost_regions", self.cost_regions.len(), false),
            ("cost_authorities", self.cost_authorities.len(), false),
            ("news", self.news.len(), false),
            ("matches", self.matches.len(), false),
            ("cases", self.cases.len(), false),
            ("ifs", self.ifs.len(), false),
            ("bundles", self.bundles.len(), false),
            ("exprs", self.exprs.len(), false),
            ("connectives", self.connectives.len(), false),
            ("kvs", self.kvs.len(), false),
            ("pathmaps", self.pathmaps.len(), false),
            (
                "bind_patterns",
                self.bind_patterns.len(),
                allow == DrainedRoot::BindPattern,
            ),
            ("list_bind_patterns", self.list_bind_patterns.len(), false),
            ("par_with_randoms", self.par_with_randoms.len(), false),
            (
                "list_par_with_randoms",
                self.list_par_with_randoms.len(),
                allow == DrainedRoot::ListParWithRandom,
            ),
            (
                "tagged_continuations",
                self.tagged_continuations.len(),
                allow == DrainedRoot::TaggedContinuation,
            ),
        ];
        for (name, len, is_root) in counts {
            if len != usize::from(is_root) {
                return Err(ColdStoreDecodeError::MachineInvariant(name));
            }
        }
        if !self.par_frames.is_empty()
            || !self.bind_tails.is_empty()
            || !self.region_ids.is_empty()
            || !self.receive_tails.is_empty()
            || !self.new_frames.is_empty()
            || !self.method_names.is_empty()
            || !self.trie_snapshots.is_empty()
        {
            return Err(ColdStoreDecodeError::MachineInvariant(
                "side frame not drained at end of run",
            ));
        }
        Ok(())
    }

    /// Executes the Expr start field program off the main dispatch frame.
    /// For this Op, reader, value stacks, side frames, op worklist and errors
    /// are exactly those of the original inline arm.
    #[inline(never)]
    fn step_expr_start(&mut self) -> Res<()> {
        if !self.r.option_tag()? {
            self.exprs.push(Expr {
                expr_instance: None,
            });
            return Ok(());
        }
        let variant = self
            .r
            .variant("ExprInstance", EXPR_INSTANCE_VARIANT_COUNT as u32)?;
        match variant {
            // ---- grounds and the variable arm: bounded, read inline
            EX_G_BOOL => {
                let v = self.r.bool()?;
                self.push_expr(ExprInstance::GBool(v));
            }
            EX_G_INT => {
                let v = self.r.i64()?;
                self.push_expr(ExprInstance::GInt(v));
            }
            EX_G_STRING => {
                let v = self.r.string()?;
                self.push_expr(ExprInstance::GString(v));
            }
            EX_G_URI => {
                let v = self.r.string()?;
                self.push_expr(ExprInstance::GUri(v));
            }
            EX_G_BYTE_ARRAY => {
                let v = self.r.byte_seq()?;
                self.push_expr(ExprInstance::GByteArray(v));
            }
            EX_G_DOUBLE => {
                // `fixed64` carrying the raw IEEE-754 bits.
                let v = self.r.u64()?;
                self.push_expr(ExprInstance::GDouble(v));
            }
            EX_G_FLOAT32 => {
                let v = self.r.u32()?;
                self.push_expr(ExprInstance::GFloat32(v));
            }
            EX_G_UINT64 => {
                let v = self.r.u64()?;
                self.push_expr(ExprInstance::GUint64(v));
            }
            EX_G_INT32 => {
                let v = self.r.i32()?;
                self.push_expr(ExprInstance::GInt32(v));
            }
            EX_G_UINT32 => {
                let v = self.r.u32()?;
                self.push_expr(ExprInstance::GUint32(v));
            }
            EX_G_UINT16 => {
                let v = self.r.u32()?;
                self.push_expr(ExprInstance::GUint16(v));
            }
            EX_G_UINT8 => {
                let v = self.r.u32()?;
                self.push_expr(ExprInstance::GUint8(v));
            }
            EX_G_BIG_INT => {
                let v = self.r.byte_seq()?;
                self.push_expr(ExprInstance::GBigInt(v));
            }
            EX_G_BIG_RAT => {
                let numerator = self.r.byte_seq()?;
                let denominator = self.r.byte_seq()?;
                self.push_expr(ExprInstance::GBigRat(GBigRational {
                    numerator,
                    denominator,
                }));
            }
            EX_G_FIXED_POINT => {
                let unscaled = self.r.byte_seq()?;
                let scale = self.r.u32()?;
                self.push_expr(ExprInstance::GFixedPoint(GFixedPoint { unscaled, scale }));
            }
            EX_E_VAR_BODY => {
                let v = self.r.opt_var()?;
                self.push_expr(ExprInstance::EVarBody(EVar { v }));
            }

            // ---- unary
            EX_E_NOT_BODY | EX_E_NEG_BODY => {
                let has_p = self.r.option_tag()?;
                self.ops.push(Op::ExprUnaryBuild { variant, has_p });
                self.opt_par_child(has_p);
            }

            // ---- collections
            EX_E_LIST_BODY | EX_E_SET_BODY => {
                let n = self.r.len()?;
                self.ops.push(Op::ExprSeqBuild { variant, n });
                self.repeat(Kind::Par, n);
            }
            EX_E_TUPLE_BODY => {
                let n = self.r.len()?;
                self.ops.push(Op::ExprTupleBuild { n });
                self.repeat(Kind::Par, n);
            }
            EX_E_MAP_BODY => {
                let n = self.r.len()?;
                self.ops.push(Op::ExprMapBuild { n });
                self.repeat(Kind::Kv, n);
            }

            // ---- method: the name is read FIRST and must survive two
            //      descents, so it goes on a side stack.
            EX_E_METHOD_BODY => {
                let name = self.r.string()?;
                self.method_names.push(name);
                let has_target = self.r.option_tag()?;
                self.ops.push(Op::ExprMethodArgs { has_target });
                self.opt_par_child(has_target);
            }

            // ---- the two path-map arms (wire shape 3)
            EX_E_PATHMAP_BODY => {
                self.ops.push(Op::ExprFromPathmap);
                self.ops.push(Op::PathmapStart);
            }
            EX_E_ZIPPER_BODY => {
                let has_pathmap = self.r.option_tag()?;
                self.ops.push(Op::ExprZipperBuild { has_pathmap });
                if has_pathmap {
                    self.ops.push(Op::PathmapStart);
                }
            }

            // ---- the seventeen `(Option<Par>, Option<Par>)` arms
            _ => {
                if !BINARY_EXPR_VARIANTS.contains(&variant) {
                    return Err(ColdStoreDecodeError::MachineInvariant(
                        "ExprInstance arm not covered by the decoder",
                    ));
                }
                let t1 = self.r.option_tag()?;
                self.ops.push(Op::ExprBinaryAfter1 { variant, t1 });
                self.opt_par_child(t1);
            }
        }
        Ok(())
    }

    /// Cost-field Op arms retain their original reader, stack, worklist, and
    /// charge order; only their fixed native dispatch frame is separated.
    #[inline(never)]
    fn step_cost(&mut self, op: Op) -> Res<()> {
        match op {
            Op::CostSignatureStart => {
                if !self.r.option_tag()? {
                    self.cost_signatures.push(CostSignature {
                        value: None,
                    });
                    return Ok(());
                }
                let variant = self
                    .r
                    .variant("CostSignature.Value", COST_VALUE_VARIANT_COUNT)?;
                match variant {
                    CS_GROUND => {
                        let ground = self.r.byte_seq()?;
                        self.cost_signatures.push(CostSignature {
                            value: Some(CostValue::Ground(ground)),
                        });
                    }
                    CS_BOUND_LEVEL => {
                        let level = self.r.i32()?;
                        self.cost_signatures.push(CostSignature {
                            value: Some(CostValue::BoundLevel(level)),
                        });
                    }
                    CS_UNIT => {
                        let unit = self.r.bool()?;
                        self.cost_signatures.push(CostSignature {
                            value: Some(CostValue::Unit(unit)),
                        });
                    }
                    CS_QUOTE | CS_NAME => {
                        self.ops.push(Op::CostSignatureParBuild { variant });
                        self.ops.push(Op::ParStart);
                    }
                    CS_COMPOUND => {
                        let count = self.r.len()?;
                        self.ops.push(Op::CostSignatureCompoundBuild { count });
                        self.repeat(Kind::CostSignature, count);
                    }
                    _ => return Err(ColdStoreDecodeError::MachineInvariant("CostSignature arm")),
                }
            }
            Op::CostSignatureParBuild { variant } => {
                let par = take_one(&mut self.pars, "CostSignature Par")?;
                let value = match variant {
                    CS_QUOTE => CostValue::Quote(par),
                    CS_NAME => CostValue::Name(par),
                    _ => {
                        return Err(ColdStoreDecodeError::MachineInvariant(
                            "CostSignature Par arm",
                        ))
                    }
                };
                self.cost_signatures.push(CostSignature {
                    value: Some(value),
                });
            }
            Op::CostSignatureCompoundBuild { count } => {
                let elements = take_n(&mut self.cost_signatures, count, "CostSignatureCompound.elements")?;
                self.cost_signatures.push(CostSignature {
                    value: Some(CostValue::Compound(CostSignatureCompound { elements })),
                });
            }
            Op::CostSignedTermStart => {
                let has_body = self.r.option_tag()?;
                self.ops.push(Op::CostSignedTermSignature { has_body });
                self.opt_par_child(has_body);
            }
            Op::CostSignedTermSignature { has_body } => {
                let has_signature = self.r.option_tag()?;
                self.ops.push(Op::CostSignedTermBuild {
                    has_body,
                    has_signature,
                });
                if has_signature {
                    self.ops.push(Op::CostSignatureStart);
                }
            }
            Op::CostSignedTermBuild {
                has_body,
                has_signature,
            } => {
                let signature = if has_signature {
                    Some(take_one(
                        &mut self.cost_signatures,
                        "CostSignedTerm.signature",
                    )?)
                } else {
                    None
                };
                let body = take_opt_par(&mut self.pars, has_body, "CostSignedTerm.body")?;
                self.cost_signed_terms.push(CostSignedTerm {
                    body,
                    signature,
                });
            }
            Op::CostStackStart => {
                let n_cells = self.r.len()?;
                self.ops.push(Op::CostStackParallelTail { n_cells });
                self.repeat(Kind::CostSignature, n_cells);
            }
            Op::CostStackParallelTail { n_cells } => {
                // The parallel tail was introduced only in CostV2; both
                // earlier layouts end immediately after the cells sequence.
                let n_branches = if self.layout == DecodeLayout::CostV2 {
                    self.r.len()?
                } else {
                    0
                };
                self.ops.push(Op::CostStackBuild {
                    n_cells,
                    n_branches,
                });
                self.repeat(Kind::CostStack, n_branches);
            }
            Op::CostStackBuild {
                n_cells,
                n_branches,
            } => {
                let parallel_tail = take_n(&mut self.cost_stacks, n_branches, "CostStack.parallel_tail")?;
                let cells = take_n(&mut self.cost_signatures, n_cells, "CostStack.cells")?;
                self.cost_stacks.push(CostStack {
                    cells,
                    parallel_tail,
                });
            }
            Op::CostRegionStart => {
                let instance_id = self.r.byte_seq()?;
                let has_signature = self.r.option_tag()?;
                self.region_ids.push(instance_id);
                self.ops.push(Op::CostRegionBuild { has_signature });
                if has_signature {
                    self.ops.push(Op::CostSignatureStart);
                }
            }
            Op::CostRegionBuild { has_signature } => {
                let signature = if has_signature {
                    Some(take_one(&mut self.cost_signatures, "CostRegion.signature")?)
                } else {
                    None
                };
                let instance_id = take_one(&mut self.region_ids, "CostRegion.instance_id")?;
                self.cost_regions.push(CostRegion {
                    instance_id,
                    signature,
                });
            }
            Op::CostAuthorityStart => {
                let count = self.r.len()?;
                self.ops.push(Op::CostAuthorityBuild { count });
                self.repeat(Kind::CostRegion, count);
            }
            Op::CostAuthorityBuild { count } => {
                let regions = take_n(&mut self.cost_regions, count, "CostAuthority.regions")?;
                self.cost_authorities.push(CostAuthority {
                    regions,
                });
            }
            _ => unreachable!("step_cost received a non-cost Op"),
        }
        Ok(())
    }

    /// Ordinary expression and map Op arms are frame-isolated without
    /// changing the single-step machine transition.
    #[inline(never)]
    fn step_mid(&mut self, op: Op) -> Res<()> {
        match op {
            Op::NewStart => {
                let bind_count = self.r.i32()?;
                let has_p = self.r.option_tag()?;
                self.new_frames.push(NewFrame {
                    bind_count,
                    uri: Vec::new(),
                    keys: Vec::new(),
                });
                self.ops.push(Op::NewAfterP { has_p });
                self.opt_par_child(has_p);
            }
            Op::NewAfterP { has_p } => {
                let uri = self.r.string_seq()?;
                let n_entries = self.r.len()?;
                let keys = Vec::with_capacity(cautious_capacity::<String>(n_entries));
                {
                    let frame = self.new_frame()?;
                    frame.uri = uri;
                    frame.keys = keys;
                }
                self.ops.push(Op::NewBuild { has_p, n_entries });
                self.ops.push(Op::NewEntry {
                    remaining: n_entries,
                });
            }
            Op::NewEntry { remaining } => {
                // Wire shape 1: an 8-byte count (already read) then key/value
                // PAIRS — keys interleaved with arbitrarily deep values, which
                // is why this is a resume point rather than a loop.
                if remaining > 0 {
                    let key = self.r.string()?;
                    let frame = self
                        .new_frames
                        .last_mut()
                        .ok_or(ColdStoreDecodeError::MachineInvariant("New frame"))?;
                    frame.keys.push(key);
                    self.ops.push(Op::NewEntry {
                        remaining: remaining - 1,
                    });
                    self.ops.push(Op::ParStart);
                }
            }
            Op::NewBuild { has_p, n_entries } => {
                let locally_free = self.r.byte_seq()?;
                let frame = self
                    .new_frames
                    .pop()
                    .ok_or(ColdStoreDecodeError::MachineInvariant("New frame"))?;
                let values = take_n(&mut self.pars, n_entries, "New.injections")?;
                let p = take_opt_par(&mut self.pars, has_p, "New.p")?;
                // `BTreeMap::insert` in stream order — a duplicate key
                // OVERWRITES, exactly as serde's map visitor does, and the
                // stream is not required to be sorted.
                let mut injections = BTreeMap::new();
                for (key, value) in frame.keys.into_iter().zip(values.into_iter()) {
                    injections.insert(key, value);
                }
                self.news.push(New {
                    bind_count: frame.bind_count,
                    p,
                    uri: frame.uri,
                    injections,
                    locally_free,
                });
            }

            // ---------------------------------------------------------------
            // Match / MatchCase
            // ---------------------------------------------------------------
            Op::MatchStart => {
                let has_target = self.r.option_tag()?;
                self.ops.push(Op::MatchCases { has_target });
                self.opt_par_child(has_target);
            }
            Op::MatchCases { has_target } => {
                let n_cases = self.r.len()?;
                self.ops.push(Op::MatchBuild {
                    has_target,
                    n_cases,
                });
                self.repeat(Kind::Case, n_cases);
            }
            Op::MatchBuild {
                has_target,
                n_cases,
            } => {
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let cases = take_n(&mut self.cases, n_cases, "Match.cases")?;
                let target = take_opt_par(&mut self.pars, has_target, "Match.target")?;
                self.matches.push(Match {
                    target,
                    cases,
                    locally_free,
                    connective_used,
                });
            }

            Op::CaseStart => {
                let t_pattern = self.r.option_tag()?;
                self.ops.push(Op::CaseAfterPattern { t_pattern });
                self.opt_par_child(t_pattern);
            }
            Op::CaseAfterPattern { t_pattern } => {
                let t_source = self.r.option_tag()?;
                self.ops.push(Op::CaseAfterSource {
                    t_pattern,
                    t_source,
                });
                self.opt_par_child(t_source);
            }
            Op::CaseAfterSource {
                t_pattern,
                t_source,
            } => {
                let free_count = self.r.i32()?;
                let t_guard = self.r.option_tag()?;
                self.ops.push(Op::CaseBuild {
                    t_pattern,
                    t_source,
                    free_count,
                    t_guard,
                });
                self.opt_par_child(t_guard);
            }
            Op::CaseBuild {
                t_pattern,
                t_source,
                free_count,
                t_guard,
            } => {
                let guard = take_opt_par(&mut self.pars, t_guard, "MatchCase.guard")?;
                let source = take_opt_par(&mut self.pars, t_source, "MatchCase.source")?;
                let pattern = take_opt_par(&mut self.pars, t_pattern, "MatchCase.pattern")?;
                self.cases.push(MatchCase {
                    pattern,
                    source,
                    free_count,
                    guard,
                });
            }

            // ---------------------------------------------------------------
            // If
            // ---------------------------------------------------------------
            Op::IfStart => {
                let t1 = self.r.option_tag()?;
                self.ops.push(Op::IfAfterCondition { t1 });
                self.opt_par_child(t1);
            }
            Op::IfAfterCondition { t1 } => {
                let t2 = self.r.option_tag()?;
                self.ops.push(Op::IfAfterTrue { t1, t2 });
                self.opt_par_child(t2);
            }
            Op::IfAfterTrue { t1, t2 } => {
                let t3 = self.r.option_tag()?;
                self.ops.push(Op::IfBuild { t1, t2, t3 });
                self.opt_par_child(t3);
            }
            Op::IfBuild { t1, t2, t3 } => {
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let if_false = take_opt_par(&mut self.pars, t3, "If.if_false")?;
                let if_true = take_opt_par(&mut self.pars, t2, "If.if_true")?;
                let condition = take_opt_par(&mut self.pars, t1, "If.condition")?;
                self.ifs.push(If {
                    condition,
                    if_true,
                    if_false,
                    locally_free,
                    connective_used,
                });
            }

            // ---------------------------------------------------------------
            // Bundle
            // ---------------------------------------------------------------
            Op::BundleStart => {
                let has_body = self.r.option_tag()?;
                self.ops.push(Op::BundleBuild { has_body });
                self.opt_par_child(has_body);
            }
            Op::BundleBuild { has_body } => {
                let write_flag = self.r.bool()?;
                let read_flag = self.r.bool()?;
                let body = take_opt_par(&mut self.pars, has_body, "Bundle.body")?;
                self.bundles.push(Bundle {
                    body,
                    write_flag,
                    read_flag,
                });
            }

            // ---------------------------------------------------------------
            // Expr — wire shapes 3 and 4 live here
            // ---------------------------------------------------------------
            Op::ExprStart => self.step_expr_start()?,
            Op::ExprUnaryBuild { variant, has_p } => {
                let p = take_opt_par(&mut self.pars, has_p, "unary ExprInstance operand")?;
                let instance = match variant {
                    EX_E_NOT_BODY => ExprInstance::ENotBody(ENot { p }),
                    EX_E_NEG_BODY => ExprInstance::ENegBody(ENeg { p }),
                    _ => {
                        return Err(ColdStoreDecodeError::MachineInvariant(
                            "unary ExprInstance arm",
                        ));
                    }
                };
                self.push_expr(instance);
            }
            Op::ExprBinaryAfter1 { variant, t1 } => {
                let t2 = self.r.option_tag()?;
                self.ops.push(Op::ExprBinaryBuild { variant, t1, t2 });
                self.opt_par_child(t2);
            }
            Op::ExprBinaryBuild { variant, t1, t2 } => {
                let p2 = take_opt_par(&mut self.pars, t2, "binary ExprInstance p2")?;
                let p1 = take_opt_par(&mut self.pars, t1, "binary ExprInstance p1")?;
                let instance = binary_expr_instance(variant, p1, p2)?;
                self.push_expr(instance);
            }
            Op::ExprSeqBuild { variant, n } => {
                let ps = take_n(&mut self.pars, n, "EList/ESet.ps")?;
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let remainder = self.r.opt_var()?;
                let instance = match variant {
                    EX_E_LIST_BODY => ExprInstance::EListBody(EList {
                        ps,
                        locally_free,
                        connective_used,
                        remainder,
                    }),
                    EX_E_SET_BODY => ExprInstance::ESetBody(ESet {
                        ps,
                        locally_free,
                        connective_used,
                        remainder,
                    }),
                    _ => {
                        return Err(ColdStoreDecodeError::MachineInvariant(
                            "sequence ExprInstance arm",
                        ));
                    }
                };
                self.push_expr(instance);
            }
            Op::ExprTupleBuild { n } => {
                let ps = take_n(&mut self.pars, n, "ETuple.ps")?;
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                // ⚠ `ETuple` has NO `remainder` field — unlike `EList`, `ESet`
                // and `EMap`. Reading one here would desynchronise the stream
                // by at least a byte for every tuple in the term.
                self.push_expr(ExprInstance::ETupleBody(ETuple {
                    ps,
                    locally_free,
                    connective_used,
                }));
            }
            Op::ExprMapBuild { n } => {
                let kvs = take_n(&mut self.kvs, n, "EMap.kvs")?;
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let remainder = self.r.opt_var()?;
                self.push_expr(ExprInstance::EMapBody(EMap {
                    kvs,
                    locally_free,
                    connective_used,
                    remainder,
                }));
            }
            Op::ExprMethodArgs { has_target } => {
                let n_args = self.r.len()?;
                self.ops.push(Op::ExprMethodBuild { has_target, n_args });
                self.repeat(Kind::Par, n_args);
            }
            Op::ExprMethodBuild { has_target, n_args } => {
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let arguments =
                    take_n(&mut self.pars, n_args, "EMethod.arguments")?;
                let target = take_opt_par(&mut self.pars, has_target, "EMethod.target")?;
                let method_name = take_one(&mut self.method_names, "EMethod.method_name")?;
                self.push_expr(ExprInstance::EMethodBody(EMethod {
                    method_name,
                    target,
                    arguments,
                    locally_free,
                    connective_used,
                }));
            }
            Op::ExprFromPathmap => {
                let pathmap = take_one(&mut self.pathmaps, "EPathmapBody")?;
                self.push_expr(ExprInstance::EPathmapBody(pathmap));
            }
            Op::ExprZipperBuild { has_pathmap } => {
                let current_path = self.r.byte_seq_seq()?;
                let is_write_zipper = self.r.bool()?;
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let cursor_kind = self.r.u32()?;
                let pathmap = if has_pathmap {
                    Some(take_one(&mut self.pathmaps, "EZipper.pathmap")?)
                } else {
                    None
                };
                self.push_expr(ExprInstance::EZipperBody(EZipper {
                    pathmap,
                    current_path,
                    is_write_zipper,
                    locally_free,
                    connective_used,
                    cursor_kind,
                }));
            }

            // ---------------------------------------------------------------
            // EPathMap — wire shape 3
            // ---------------------------------------------------------------
            Op::PathmapStart => {
                // The trie's complete EPM1 byte array comes first. Its ACTree03
                // topology and map-value table are one contiguous serde bytes
                // field; the remaining EPathMap metadata follows.
                let snapshot = self.r.byte_slice()?;
                self.trie_snapshots.push(snapshot);
                self.ops.push(Op::PathmapBuild);
            }
            Op::PathmapBuild => {
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let remainder = self.r.opt_var()?;
                let snapshot = take_one(&mut self.trie_snapshots, "EPathMap.trie_snapshot")?;
                // ★ THE ONE reader of EPM1, shared with the derived
                // `Deserialize` — two hand-written readers of one
                // wire shape is the defect `c705776c` closed.
                //
                // Malformed or noncanonical ACT topology and value ordinals are
                // rejected by the shared iterative validator; valid map values
                // are decoded by the generated stack-safe protobuf PDA.
                let mut map = EPathMap::new(Vec::new(), locally_free, connective_used, remainder);
                map.replace_trie_snapshot(snapshot)
                    .map_err(|error| ColdStoreDecodeError::Legacy(error.to_string()))?;
                self.pathmaps.push(map);
            }

            // ---------------------------------------------------------------
            // KeyValuePair
            // ---------------------------------------------------------------
            Op::KvStart => {
                let t_key = self.r.option_tag()?;
                self.ops.push(Op::KvAfterKey { t_key });
                self.opt_par_child(t_key);
            }
            Op::KvAfterKey { t_key } => {
                let t_value = self.r.option_tag()?;
                self.ops.push(Op::KvBuild { t_key, t_value });
                self.opt_par_child(t_value);
            }
            Op::KvBuild { t_key, t_value } => {
                let value = take_opt_par(&mut self.pars, t_value, "KeyValuePair.value")?;
                let key = take_opt_par(&mut self.pars, t_key, "KeyValuePair.key")?;
                self.kvs.push(KeyValuePair { key, value });
            }
            _ => unreachable!("step_mid received an unrelated Op"),
        }
        Ok(())
    }

    /// Continuation and trailing root Op arms are frame-isolated while
    /// preserving the exact explicit-worklist transition for each variant.
    #[inline(never)]
    fn step_tail(&mut self, op: Op) -> Res<()> {
        match op {
            Op::ConnStart => {
                if !self.r.option_tag()? {
                    self.connectives.push(Connective {
                        connective_instance: None,
                    });
                    return Ok(());
                }
                let variant = self.r.variant(
                    "ConnectiveInstance",
                    CONNECTIVE_INSTANCE_VARIANT_COUNT as u32,
                )?;
                match variant {
                    CN_CONN_AND_BODY | CN_CONN_OR_BODY => {
                        let n = self.r.len()?;
                        self.ops.push(Op::ConnBodyBuild { variant, n });
                        self.repeat(Kind::Par, n);
                    }
                    // ⚠ `ConnNotBody` carries a bare `Par`, NOT an
                    // `Option<Par>`: there is no tag byte before it.
                    CN_CONN_NOT_BODY => {
                        self.ops.push(Op::ConnNotBuild);
                        self.ops.push(Op::ParStart);
                    }
                    CN_VAR_REF_BODY => {
                        let index = self.r.i32()?;
                        let depth = self.r.i32()?;
                        self.connectives.push(Connective {
                            connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                                index,
                                depth,
                            })),
                        });
                    }
                    _ => {
                        let flag = self.r.bool()?;
                        let instance = match variant {
                            CN_CONN_BOOL => ConnectiveInstance::ConnBool(flag),
                            CN_CONN_INT => ConnectiveInstance::ConnInt(flag),
                            CN_CONN_STRING => ConnectiveInstance::ConnString(flag),
                            CN_CONN_URI => ConnectiveInstance::ConnUri(flag),
                            CN_CONN_BYTE_ARRAY => ConnectiveInstance::ConnByteArray(flag),
                            _ => {
                                return Err(ColdStoreDecodeError::MachineInvariant(
                                    "ConnectiveInstance arm not covered by the decoder",
                                ));
                            }
                        };
                        self.connectives.push(Connective {
                            connective_instance: Some(instance),
                        });
                    }
                }
            }
            Op::ConnBodyBuild { variant, n } => {
                let ps = take_n(&mut self.pars, n, "ConnectiveBody.ps")?;
                let body = ConnectiveBody { ps };
                let instance = match variant {
                    CN_CONN_AND_BODY => ConnectiveInstance::ConnAndBody(body),
                    CN_CONN_OR_BODY => ConnectiveInstance::ConnOrBody(body),
                    _ => return Err(ColdStoreDecodeError::MachineInvariant("ConnectiveBody arm")),
                };
                self.connectives.push(Connective {
                    connective_instance: Some(instance),
                });
            }
            Op::ConnNotBuild => {
                let p = take_one(&mut self.pars, "ConnNotBody")?;
                self.connectives.push(Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(p)),
                });
            }

            // ---------------------------------------------------------------
            // TaggedContinuation
            // ---------------------------------------------------------------
            Op::TaggedContinuationStart => {
                let t_guard = self.r.option_tag()?;
                self.ops.push(Op::TaggedContinuationCont { t_guard });
                self.opt_par_child(t_guard);
            }
            Op::TaggedContinuationCont { t_guard } => {
                let has_authority = if self.layout == DecodeLayout::Legacy {
                    false
                } else {
                    self.r.option_tag()?
                };
                self.ops.push(Op::TaggedContinuationTagged {
                    t_guard,
                    has_authority,
                });
                if has_authority {
                    self.ops.push(Op::CostAuthorityStart);
                }
            }
            Op::TaggedContinuationTagged {
                t_guard,
                has_authority,
            } => {
                if !self.r.option_tag()? {
                    self.ops.push(Op::TaggedContinuationAbsent {
                        t_guard,
                        has_authority,
                    });
                    return Ok(());
                }
                match self.r.variant("TaggedCont", TAGGED_CONT_VARIANT_COUNT)? {
                    0 => {
                        let t_body = self.r.option_tag()?;
                        self.ops.push(Op::TaggedContinuationParBody {
                            t_guard,
                            t_body,
                            has_authority,
                        });
                        self.opt_par_child(t_body);
                    }
                    _ => {
                        let value = self.r.i64()?;
                        self.ops.push(Op::TaggedContinuationScala {
                            t_guard,
                            value,
                            has_authority,
                        });
                    }
                }
            }
            Op::TaggedContinuationParBody {
                t_guard,
                t_body,
                has_authority,
            } => {
                let random_state = self.r.byte_seq()?;
                let body = take_opt_par(&mut self.pars, t_body, "ParWithRandom.body")?;
                let guard = take_opt_par(&mut self.pars, t_guard, "TaggedContinuation.guard")?;
                let cost_authority = if has_authority {
                    Some(take_one(
                        &mut self.cost_authorities,
                        "TaggedContinuation.cost_authority",
                    )?)
                } else {
                    None
                };
                self.tagged_continuations.push(TaggedContinuation {
                        guard,
                        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                            body,
                            random_state,
                        })),
                        cost_authority,
                    });
            }
            Op::TaggedContinuationScala {
                t_guard,
                value,
                has_authority,
            } => {
                let guard = take_opt_par(&mut self.pars, t_guard, "TaggedContinuation.guard")?;
                let cost_authority = if has_authority {
                    Some(take_one(
                        &mut self.cost_authorities,
                        "TaggedContinuation.cost_authority",
                    )?)
                } else {
                    None
                };
                self.tagged_continuations.push(TaggedContinuation {
                        guard,
                        tagged_cont: Some(TaggedCont::ScalaBodyRef(value)),
                        cost_authority,
                    });
            }
            Op::TaggedContinuationAbsent {
                t_guard,
                has_authority,
            } => {
                let guard = take_opt_par(&mut self.pars, t_guard, "TaggedContinuation.guard")?;
                let cost_authority = if has_authority {
                    Some(take_one(
                        &mut self.cost_authorities,
                        "TaggedContinuation.cost_authority",
                    )?)
                } else {
                    None
                };
                self.tagged_continuations.push(TaggedContinuation {
                        guard,
                        tagged_cont: None,
                        cost_authority,
                    });
            }
            // ---------------------------------------------------------------
            // ListParWithRandom / ParWithRandom
            // ---------------------------------------------------------------
            Op::ListParWithRandomStart => {
                let n = self.r.len()?;
                self.ops.push(Op::ListParWithRandomBuild { n });
                self.repeat(Kind::Par, n);
            }
            Op::ListParWithRandomBuild { n } => {
                let pars = take_n(&mut self.pars, n, "ListParWithRandom.pars")?;
                let random_state = self.r.byte_seq()?;
                self.list_par_with_randoms.push(ListParWithRandom {
                        pars,
                        random_state,
                        cost_authority: None,
                        cost_stack: None,
                    });
                if self.layout == DecodeLayout::Legacy {
                    return Ok(());
                }
                let has_authority = self.r.option_tag()?;
                self.ops.push(Op::ListParWithRandomCostStack { has_authority });
                if has_authority {
                    self.ops.push(Op::CostAuthorityStart);
                }
            }
            Op::ListParWithRandomCostStack { has_authority } => {
                let authority = if has_authority {
                    Some(take_one(
                        &mut self.cost_authorities,
                        "ListParWithRandom.cost_authority",
                    )?)
                } else {
                    None
                };
                self.list_par_with_randoms
                    .last_mut()
                    .ok_or(ColdStoreDecodeError::MachineInvariant("ListParWithRandom"))?
                    .cost_authority = authority;
                let has_stack = self.r.option_tag()?;
                self.ops.push(Op::ListParWithRandomFinish { has_stack });
                if has_stack {
                    self.ops.push(Op::CostStackStart);
                }
            }
            Op::ListParWithRandomFinish { has_stack } => {
                let stack = if has_stack {
                    Some(take_one(
                        &mut self.cost_stacks,
                        "ListParWithRandom.cost_stack",
                    )?)
                } else {
                    None
                };
                self.list_par_with_randoms
                    .last_mut()
                    .ok_or(ColdStoreDecodeError::MachineInvariant("ListParWithRandom"))?
                    .cost_stack = stack;
            }
            Op::ParWithRandomStart => {
                let has_body = self.r.option_tag()?;
                self.ops.push(Op::ParWithRandomBuild { has_body });
                self.opt_par_child(has_body);
            }
            Op::ParWithRandomBuild { has_body } => {
                let random_state = self.r.byte_seq()?;
                let body = take_opt_par(&mut self.pars, has_body, "ParWithRandom.body")?;
                self.par_with_randoms.push(ParWithRandom {
                    body,
                    random_state,
                });
            }

            // ---------------------------------------------------------------
            // BindPattern / ListBindPatterns
            // ---------------------------------------------------------------
            Op::BindPatternStart => {
                let n = self.r.len()?;
                self.ops.push(Op::BindPatternBuild { n });
                self.repeat(Kind::Par, n);
            }
            Op::BindPatternBuild { n } => {
                let patterns = take_n(&mut self.pars, n, "BindPattern.patterns")?;
                let remainder = self.r.opt_var()?;
                let free_count = self.r.i32()?;
                self.bind_patterns.push(BindPattern {
                    patterns,
                    remainder,
                    free_count,
                });
            }
            Op::ListBindPatternsStart => {
                let n = self.r.len()?;
                self.ops.push(Op::ListBindPatternsBuild { n });
                self.repeat(Kind::BindPattern, n);
            }
            Op::ListBindPatternsBuild { n } => {
                let patterns = take_n(&mut self.bind_patterns, n, "ListBindPatterns.patterns")?;
                self.list_bind_patterns.push(ListBindPatterns {
                    patterns,
                });
            }
            _ => unreachable!("step_tail received an unrelated Op"),
        }
        Ok(())
    }

    fn step(&mut self, op: Op) -> Res<()> {
        match op {
            // ---------------------------------------------------------------
            Op::Rep { kind, remaining } => {
                // `remaining` is stream-controlled and may be enormous; this
                // costs O(1) memory per iteration and fails on the first
                // element that runs out of input.
                if remaining > 0 {
                    self.ops.push(Op::Rep {
                        kind,
                        remaining: remaining - 1,
                    });
                    self.ops.push(kind.start());
                }
            }

            // ---------------------------------------------------------------
            // Par
            // ---------------------------------------------------------------
            Op::ParStart => {
                self.par_frames.push(ParFrame::default());
                self.ops.push(Op::ParField { field: 0 });
            }
            Op::ParField { field } => {
                // The eight repeated `Par`-bearing fields, in declaration
                // order. `unforgeables` sits between indices 4 and 5 and is a
                // bounded leaf, so it is read inline rather than scheduled.
                const KINDS: [Kind; 8] = [
                    Kind::Send,
                    Kind::Receive,
                    Kind::New,
                    Kind::Expr,
                    Kind::Match,
                    Kind::Bundle,
                    Kind::Connective,
                    Kind::If,
                ];
                if field as usize == KINDS.len() {
                    self.ops.push(Op::ParCostSigned);
                    return Ok(());
                }
                if field == 5 {
                    let unforgeables = self.r.unforgeable_seq()?;
                    self.par_frame()?.unforgeables = unforgeables;
                }
                let n = self.r.len()?;
                self.par_frame()?.counts[field as usize] = n;
                self.ops.push(Op::ParField { field: field + 1 });
                self.repeat(KINDS[field as usize], n);
            }
            Op::ParCostSigned => {
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let frame = self.par_frame()?;
                frame.locally_free = locally_free;
                frame.connective_used = connective_used;
                if self.layout == DecodeLayout::Legacy {
                    self.ops.push(Op::ParBuild);
                    return Ok(());
                }
                let count = self.r.len()?;
                let frame = self.par_frame()?;
                frame.n_cost_signed_terms = count;
                self.ops.push(Op::ParCostStacks);
                self.repeat(Kind::CostSignedTerm, count);
            }
            Op::ParCostStacks => {
                let count = self.r.len()?;
                self.par_frame()?.n_cost_stacks = count;
                self.ops.push(Op::ParBuild);
                self.repeat(Kind::CostStack, count);
            }
            Op::ParBuild => {
                let frame = self
                    .par_frames
                    .pop()
                    .ok_or(ColdStoreDecodeError::MachineInvariant("Par frame"))?;
                let cost_stacks = take_n(&mut self.cost_stacks, frame.n_cost_stacks, "Par.cost_stacks")?;
                let cost_signed_terms = take_n(&mut self.cost_signed_terms, frame.n_cost_signed_terms, "Par.cost_signed_terms")?;
                // Each of these lives on its OWN stack, so the drains are
                // independent; the order below simply mirrors declaration
                // order read backwards for legibility.
                let conditionals =
                    take_n(&mut self.ifs, frame.counts[7], "Par.conditionals")?;
                let connectives = take_n(&mut self.connectives, frame.counts[6], "Par.connectives")?;
                let bundles =
                    take_n(&mut self.bundles, frame.counts[5], "Par.bundles")?;
                let matches =
                    take_n(&mut self.matches, frame.counts[4], "Par.matches")?;
                let exprs = take_n(&mut self.exprs, frame.counts[3], "Par.exprs")?;
                let news = take_n(&mut self.news, frame.counts[2], "Par.news")?;
                let receives =
                    take_n(&mut self.receives, frame.counts[1], "Par.receives")?;
                let sends = take_n(&mut self.sends, frame.counts[0], "Par.sends")?;
                self.pars.push(Par {
                    sends,
                    receives,
                    news,
                    exprs,
                    matches,
                    unforgeables: frame.unforgeables,
                    bundles,
                    connectives,
                    conditionals,
                    locally_free: frame.locally_free,
                    connective_used: frame.connective_used,
                    cost_signed_terms,
                    cost_stacks,
                });
            }

            // ---------------------------------------------------------------
            // Send
            // ---------------------------------------------------------------
            Op::SendStart => {
                let has_chan = self.r.option_tag()?;
                self.ops.push(Op::SendData { has_chan });
                self.opt_par_child(has_chan);
            }
            Op::SendData { has_chan } => {
                let n_data = self.r.len()?;
                self.ops.push(Op::SendBuild { has_chan, n_data });
                self.repeat(Kind::Par, n_data);
            }
            Op::SendBuild { has_chan, n_data } => {
                let persistent = self.r.bool()?;
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                // `chan` was pushed BEFORE the `data` elements, so it must be
                // taken AFTER them.
                let data = take_n(&mut self.pars, n_data, "Send.data")?;
                let chan = take_opt_par(&mut self.pars, has_chan, "Send.chan")?;
                self.sends.push(Send {
                    chan,
                    data,
                    persistent,
                    locally_free,
                    connective_used,
                });
            }

            // ---------------------------------------------------------------
            // Receive
            // ---------------------------------------------------------------
            Op::ReceiveStart => {
                let n_binds = self.r.len()?;
                self.ops.push(Op::ReceiveBody { n_binds });
                self.repeat(Kind::Bind, n_binds);
            }
            Op::ReceiveBody { n_binds } => {
                let has_body = self.r.option_tag()?;
                self.ops.push(Op::ReceiveCondition { n_binds, has_body });
                self.opt_par_child(has_body);
            }
            Op::ReceiveCondition { n_binds, has_body } => {
                // These five must survive the `condition` descent.
                let tail = ReceiveTail {
                    persistent: self.r.bool()?,
                    peek: self.r.bool()?,
                    bind_count: self.r.i32()?,
                    locally_free: self.r.byte_seq()?,
                    connective_used: self.r.bool()?,
                };
                self.receive_tails.push(tail);
                let has_condition = self.r.option_tag()?;
                self.ops.push(Op::ReceiveBuild {
                    n_binds,
                    has_body,
                    has_condition,
                });
                self.opt_par_child(has_condition);
            }
            Op::ReceiveBuild {
                n_binds,
                has_body,
                has_condition,
            } => {
                let tail = self
                    .receive_tails
                    .pop()
                    .ok_or(ColdStoreDecodeError::MachineInvariant("Receive tail"))?;
                let condition = take_opt_par(&mut self.pars, has_condition, "Receive.condition")?;
                let body = take_opt_par(&mut self.pars, has_body, "Receive.body")?;
                let binds = take_n(&mut self.binds, n_binds, "Receive.binds")?;
                self.receives.push(Receive {
                    binds,
                    body,
                    persistent: tail.persistent,
                    peek: tail.peek,
                    bind_count: tail.bind_count,
                    locally_free: tail.locally_free,
                    connective_used: tail.connective_used,
                    condition,
                });
            }

            // ---------------------------------------------------------------
            // ReceiveBind: patterns, source, remainder, free_count
            // ---------------------------------------------------------------
            Op::BindStart => {
                let n_patterns = self.r.len()?;
                self.ops.push(Op::BindSource { n_patterns });
                self.repeat(Kind::Par, n_patterns);
            }
            Op::BindSource { n_patterns } => {
                let has_source = self.r.option_tag()?;
                self.ops.push(Op::BindBuild {
                    n_patterns,
                    has_source,
                });
                self.opt_par_child(has_source);
            }
            Op::BindBuild {
                n_patterns,
                has_source,
            } => {
                let remainder = self.r.opt_var()?;
                let free_count = self.r.i32()?;
                let has_signature = if self.layout == DecodeLayout::Legacy {
                    false
                } else {
                    self.r.option_tag()?
                };
                self.bind_tails.push(BindTail {
                    remainder,
                    free_count,
                });
                self.ops.push(Op::BindCostBuild {
                    n_patterns,
                    has_source,
                    has_signature,
                });
                if has_signature {
                    self.ops.push(Op::CostSignatureStart);
                }
            }
            Op::BindCostBuild {
                n_patterns,
                has_source,
                has_signature,
            } => {
                let cost_signature = if has_signature {
                    Some(take_one(
                        &mut self.cost_signatures,
                        "ReceiveBind.cost_signature",
                    )?)
                } else {
                    None
                };
                let tail = take_one(&mut self.bind_tails, "ReceiveBind tail")?;
                // `source` was pushed AFTER the patterns, so it comes off first.
                let source = take_opt_par(&mut self.pars, has_source, "ReceiveBind.source")?;
                let patterns =
                    take_n(&mut self.pars, n_patterns, "ReceiveBind.patterns")?;
                self.binds.push(ReceiveBind {
                    patterns,
                    source,
                    remainder: tail.remainder,
                    free_count: tail.free_count,
                    cost_signature,
                });
            }

            Op::CostSignatureStart
            | Op::CostSignatureParBuild { .. }
            | Op::CostSignatureCompoundBuild { .. }
            | Op::CostSignedTermStart
            | Op::CostSignedTermSignature { .. }
            | Op::CostSignedTermBuild { .. }
            | Op::CostStackStart
            | Op::CostStackParallelTail { .. }
            | Op::CostStackBuild { .. }
            | Op::CostRegionStart
            | Op::CostRegionBuild { .. }
            | Op::CostAuthorityStart
            | Op::CostAuthorityBuild { .. }
                => self.step_cost(op)?,

            // ---------------------------------------------------------------
            // New — wire shape 1 lives here
            // ---------------------------------------------------------------
            Op::NewStart
            | Op::NewAfterP { .. }
            | Op::NewEntry { .. }
            | Op::NewBuild { .. }
            | Op::MatchStart
            | Op::MatchCases { .. }
            | Op::MatchBuild { .. }
            | Op::CaseStart
            | Op::CaseAfterPattern { .. }
            | Op::CaseAfterSource { .. }
            | Op::CaseBuild { .. }
            | Op::IfStart
            | Op::IfAfterCondition { .. }
            | Op::IfAfterTrue { .. }
            | Op::IfBuild { .. }
            | Op::BundleStart
            | Op::BundleBuild { .. }
            | Op::ExprStart
            | Op::ExprUnaryBuild { .. }
            | Op::ExprBinaryAfter1 { .. }
            | Op::ExprBinaryBuild { .. }
            | Op::ExprSeqBuild { .. }
            | Op::ExprTupleBuild { .. }
            | Op::ExprMapBuild { .. }
            | Op::ExprMethodArgs { .. }
            | Op::ExprMethodBuild { .. }
            | Op::ExprFromPathmap
            | Op::ExprZipperBuild { .. }
            | Op::PathmapStart
            | Op::PathmapBuild
            | Op::KvStart
            | Op::KvAfterKey { .. }
            | Op::KvBuild { .. }
                => self.step_mid(op)?,

            // ---------------------------------------------------------------
            // Connective
            // ---------------------------------------------------------------
            Op::ConnStart
            | Op::ConnBodyBuild { .. }
            | Op::ConnNotBuild
            | Op::TaggedContinuationStart
            | Op::TaggedContinuationCont { .. }
            | Op::TaggedContinuationTagged { .. }
            | Op::TaggedContinuationParBody { .. }
            | Op::TaggedContinuationScala { .. }
            | Op::TaggedContinuationAbsent { .. }
            | Op::ListParWithRandomStart
            | Op::ListParWithRandomBuild { .. }
            | Op::ListParWithRandomCostStack { .. }
            | Op::ListParWithRandomFinish { .. }
            | Op::ParWithRandomStart
            | Op::ParWithRandomBuild { .. }
            | Op::BindPatternStart
            | Op::BindPatternBuild { .. }
            | Op::ListBindPatternsStart
            | Op::ListBindPatternsBuild { .. }
                => self.step_tail(op)?,
        }
        Ok(())
    }
}

/// Which stack the root value is expected to be left on. Used only by
/// [`Machine::assert_drained`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum DrainedRoot {
    Par,
    ListParWithRandom,
    BindPattern,
    TaggedContinuation,
}

impl<'a> Drop for Machine<'a> {
    fn drop(&mut self) { self.dismantle(); }
}

impl<'a> Machine<'a> {
    /// `true` iff no value stack holds anything — the state a successful run
    /// leaves behind once its root value has been taken.
    fn is_fully_drained(&self) -> bool {
        self.pars.is_empty()
            && self.sends.is_empty()
            && self.receives.is_empty()
            && self.binds.is_empty()
            && self.cost_signatures.is_empty()
            && self.cost_signed_terms.is_empty()
            && self.cost_stacks.is_empty()
            && self.cost_regions.is_empty()
            && self.cost_authorities.is_empty()
            && self.news.is_empty()
            && self.matches.is_empty()
            && self.cases.is_empty()
            && self.ifs.is_empty()
            && self.bundles.is_empty()
            && self.exprs.is_empty()
            && self.connectives.is_empty()
            && self.kvs.is_empty()
            && self.pathmaps.is_empty()
            && self.bind_patterns.is_empty()
            && self.list_bind_patterns.is_empty()
            && self.par_with_randoms.is_empty()
            && self.list_par_with_randoms.is_empty()
            && self.tagged_continuations.is_empty()
    }
}

impl<'a> Machine<'a> {
    /// Release every partially-decoded value **without recursion**.
    ///
    /// `<Par as Drop>` is Θ(depth) (470 B/level, debug). A decode that fails at
    /// the last byte of a deep term therefore holds a deep `Par` on a value
    /// stack, and letting it drop normally would abort the process while
    /// *rejecting* a hostile input — turning a clean `Err` back into the
    /// `SIGSEGV` this module exists to remove.
    ///
    /// The salvage funnels every stack into ONE `Par` shell and hands it to
    /// [`dismantle_all`], the existing explicit-worklist teardown. Using the
    /// shell is deliberate: it re-uses `par_children`'s canonical by-move child
    /// table rather than re-enumerating which fields of `Send`/`Receive`/… hold
    /// `Par`s — the fifth-enumeration hazard that module exists to prevent. The
    /// only extra knowledge encoded here is the seven single-parent
    /// containments the schema itself declares (`ReceiveBind` under `Receive`,
    /// `MatchCase` under `Match`, `KeyValuePair` under `EMap`, `EPathMap` under
    /// `EPathmapBody`, and the three `Par`-holding leaf wrappers).
    fn dismantle(&mut self) {
        // ⚠ `Drop` runs on the SUCCESS path too, where every stack is empty by
        // the time the root value has been popped (`assert_drained` has just
        // checked exactly that). Without this early-out the salvage below would
        // allocate a `Par` shell and a worklist on every cold-store read — a
        // per-datum cost paid to release nothing. Eighteen `is_empty` reads is
        // strictly cheaper than two allocations.
        if self.is_fully_drained() {
            return;
        }

        let mut loose: Vec<Par> = mem::take(&mut self.pars);
        let mut loose_cost_signatures = mem::take(&mut self.cost_signatures);
        for region in mem::take(&mut self.cost_regions) {
            loose_cost_signatures.extend(region.signature);
        }
        for authority in mem::take(&mut self.cost_authorities) {
            for region in authority.regions {
                loose_cost_signatures.extend(region.signature);
            }
        }

        let mut receives = mem::take(&mut self.receives);
        let binds = mem::take(&mut self.binds);
        if !binds.is_empty() {
            receives.push(Receive {
                binds,
                ..Default::default()
            });
        }

        let mut matches = mem::take(&mut self.matches);
        let cases = mem::take(&mut self.cases);
        if !cases.is_empty() {
            matches.push(Match {
                cases,
                ..Default::default()
            });
        }

        let mut exprs = mem::take(&mut self.exprs);
        let kvs = mem::take(&mut self.kvs);
        if !kvs.is_empty() {
            exprs.push(Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs,
                    ..Default::default()
                })),
            });
        }
        for pathmap in mem::take(&mut self.pathmaps) {
            exprs.push(Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
            });
        }

        for bp in mem::take(&mut self.bind_patterns) {
            loose.extend(bp.patterns);
        }
        for lbp in mem::take(&mut self.list_bind_patterns) {
            for bp in lbp.patterns {
                loose.extend(bp.patterns);
            }
        }
        for pwr in mem::take(&mut self.par_with_randoms) {
            loose.extend(pwr.body);
        }
        let mut loose_cost_stacks = mem::take(&mut self.cost_stacks);
        for lpwr in mem::take(&mut self.list_par_with_randoms) {
            loose.extend(lpwr.pars);
            if let Some(authority) = lpwr.cost_authority {
                for region in authority.regions {
                    loose_cost_signatures.extend(region.signature);
                }
            }
            loose_cost_stacks.extend(lpwr.cost_stack);
        }
        for tc in mem::take(&mut self.tagged_continuations) {
            loose.extend(tc.guard);
            if let Some(TaggedCont::ParBody(pwr)) = tc.tagged_cont {
                loose.extend(pwr.body);
            }
            if let Some(authority) = tc.cost_authority {
                for region in authority.regions {
                    loose_cost_signatures.extend(region.signature);
                }
            }
        }

        let mut root = Par::default();
        root.sends = mem::take(&mut self.sends);
        root.receives = receives;
        root.news = mem::take(&mut self.news);
        root.exprs = exprs;
        root.matches = matches;
        root.bundles = mem::take(&mut self.bundles);
        root.connectives = mem::take(&mut self.connectives);
        root.conditionals = mem::take(&mut self.ifs);
        root.cost_signed_terms = mem::take(&mut self.cost_signed_terms);
        root.cost_stacks = loose_cost_stacks;
        if !loose_cost_signatures.is_empty() {
            root.cost_stacks.push(CostStack {
                cells: loose_cost_signatures,
                parallel_tail: Vec::new(),
            });
        }
        loose.push(root);

        dismantle_all(loose);
    }
}

// ===========================================================================
// §F  The four public entry points
// ===========================================================================
//
// C, P, A, K of `RSpace<Par, BindPattern, ListParWithRandom,
// TaggedContinuation>` — the Rholang instantiation. Every other machine type is
// reachable only as a child of one of these, and is exercised through them.

impl ColdStoreDecode for Par {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut m = Machine::new(bytes);
        m.ops.push(Op::ParStart);
        m.run()?;
        m.assert_drained(DrainedRoot::Par)?;
        let consumed = m.r.consumed();
        let value = take_one(&mut m.pars, "Par (root)")?;
        #[cfg(feature = "phase7-depth-histograms")]
        crate::rust::rholang::phase7_depth_histogram::record_par("bincode_decoder", &value);
        Ok((value, consumed))
    }

    fn cold_decode_legacy_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_legacy(bytes);
        machine.ops.push(Op::ParStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::Par)?;
        let consumed = machine.r.consumed();
        Ok((take_one(&mut machine.pars, "legacy Par (root)")?, consumed))
    }

    fn cold_decode_cost_v1_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_cost_v1(bytes);
        machine.ops.push(Op::ParStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::Par)?;
        let consumed = machine.r.consumed();
        Ok((take_one(&mut machine.pars, "CostV1 Par (root)")?, consumed))
    }
}

impl ColdStoreDecode for ListParWithRandom {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut m = Machine::new(bytes);
        m.ops.push(Op::ListParWithRandomStart);
        m.run()?;
        m.assert_drained(DrainedRoot::ListParWithRandom)?;
        let consumed = m.r.consumed();
        let value = take_one(&mut m.list_par_with_randoms, "ListParWithRandom (root)")?;
        #[cfg(feature = "phase7-depth-histograms")]
        crate::rust::rholang::phase7_depth_histogram::record_list_par_with_random(
            "bincode_decoder",
            &value,
        );
        Ok((value, consumed))
    }

    fn cold_decode_legacy_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_legacy(bytes);
        machine.ops.push(Op::ListParWithRandomStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::ListParWithRandom)?;
        let consumed = machine.r.consumed();
        Ok((
            take_one(
                &mut machine.list_par_with_randoms,
                "legacy ListParWithRandom (root)",
            )?,
            consumed,
        ))
    }

    fn cold_decode_cost_v1_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_cost_v1(bytes);
        machine.ops.push(Op::ListParWithRandomStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::ListParWithRandom)?;
        let consumed = machine.r.consumed();
        Ok((
            take_one(
                &mut machine.list_par_with_randoms,
                "CostV1 ListParWithRandom (root)",
            )?,
            consumed,
        ))
    }
}

impl ColdStoreDecode for BindPattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut m = Machine::new(bytes);
        m.ops.push(Op::BindPatternStart);
        m.run()?;
        m.assert_drained(DrainedRoot::BindPattern)?;
        let consumed = m.r.consumed();
        let value = take_one(&mut m.bind_patterns, "BindPattern (root)")?;
        #[cfg(feature = "phase7-depth-histograms")]
        crate::rust::rholang::phase7_depth_histogram::record_bind_pattern(
            "bincode_decoder",
            &value,
        );
        Ok((value, consumed))
    }

    fn cold_decode_legacy_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_legacy(bytes);
        machine.ops.push(Op::BindPatternStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::BindPattern)?;
        let consumed = machine.r.consumed();
        Ok((
            take_one(&mut machine.bind_patterns, "legacy BindPattern (root)")?,
            consumed,
        ))
    }

    fn cold_decode_cost_v1_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_cost_v1(bytes);
        machine.ops.push(Op::BindPatternStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::BindPattern)?;
        let consumed = machine.r.consumed();
        Ok((
            take_one(&mut machine.bind_patterns, "CostV1 BindPattern (root)")?,
            consumed,
        ))
    }
}

impl ColdStoreDecode for TaggedContinuation {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut m = Machine::new(bytes);
        m.ops.push(Op::TaggedContinuationStart);
        m.run()?;
        m.assert_drained(DrainedRoot::TaggedContinuation)?;
        let consumed = m.r.consumed();
        let value = take_one(&mut m.tagged_continuations, "TaggedContinuation (root)")?;
        #[cfg(feature = "phase7-depth-histograms")]
        crate::rust::rholang::phase7_depth_histogram::record_tagged_continuation(
            "bincode_decoder",
            &value,
        );
        Ok((value, consumed))
    }

    fn cold_decode_legacy_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_legacy(bytes);
        machine.ops.push(Op::TaggedContinuationStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::TaggedContinuation)?;
        let consumed = machine.r.consumed();
        Ok((
            take_one(
                &mut machine.tagged_continuations,
                "legacy TaggedContinuation (root)",
            )?,
            consumed,
        ))
    }

    fn cold_decode_cost_v1_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        let mut machine = Machine::new_cost_v1(bytes);
        machine.ops.push(Op::TaggedContinuationStart);
        machine.run()?;
        machine.assert_drained(DrainedRoot::TaggedContinuation)?;
        let consumed = machine.r.consumed();
        Ok((
            take_one(
                &mut machine.tagged_continuations,
                "CostV1 TaggedContinuation (root)",
            )?,
            consumed,
        ))
    }
}

// ===========================================================================
// §G  Test-only entry points for the machine types that are not roots
// ===========================================================================

/// `ParWithRandom` and `ListBindPatterns` are on the machine (they transitively
/// contain `Par`) but are never a cold-store root: `ParWithRandom` is reached
/// through `TaggedCont::ParBody`, `ListBindPatterns` through the gRPC surface.
/// They are exposed here so the differential corpus can exercise their programs
/// directly rather than leaving two of the forty-seven untested.
#[doc(hidden)]
pub fn cold_decode_par_with_random_for_test(
    bytes: &[u8],
) -> Result<(ParWithRandom, usize), ColdStoreDecodeError> {
    let mut m = Machine::new(bytes);
    m.ops.push(Op::ParWithRandomStart);
    m.run()?;
    let consumed = m.r.consumed();
    let value = take_one(&mut m.par_with_randoms, "ParWithRandom (root)")?;
    Ok((value, consumed))
}

/// See [`cold_decode_par_with_random_for_test`].
#[doc(hidden)]
pub fn cold_decode_list_bind_patterns_for_test(
    bytes: &[u8],
) -> Result<(ListBindPatterns, usize), ColdStoreDecodeError> {
    let mut m = Machine::new(bytes);
    m.ops.push(Op::ListBindPatternsStart);
    m.run()?;
    let consumed = m.r.consumed();
    let value = take_one(&mut m.list_bind_patterns, "ListBindPatterns (root)")?;
    Ok((value, consumed))
}
