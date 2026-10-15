//! DR-131 P2b: the cold-store decode boundary (`ColdStoreDecode`) for the four
//! roots of the native history records, with O(1) native stack.
//!
//! The roots are `Par` (the channels of a join), `ListParWithRandom` (the
//! payload of a datum), `BindPattern` (the patterns of a continuation) and
//! `TaggedContinuation` (its body). Replay decodes them in its cold reads of
//! stored data. The derived bincode decoder recurses once for each nesting
//! level, so a deeply nested stored value would exhaust the native stack.
//!
//! The machine is adapted from `feature/f1r3lang-mettail-only`
//! (`e5102e945:models/src/rust/rholang/bincode_decoder.rs`). The adaptations
//! for this branch:
//!
//! 1. The variant indices are this branch's: 42 `ExprInstance` arms, with the
//!    five numeric arms `GUint64`, `GInt32`, `GUint32`, `GUint16` and `GUint8`.
//!    `GFloat32` is index 37 here (36 on that line). The indices are written
//!    here, and [`expr_instance_variant_index`] maps every arm to its index
//!    with an exhaustive `match`, so a new arm fails to compile. The tests pin
//!    every index against the encoding that bincode writes.
//! 2. This branch has no stored-format epoch, so the legacy layout of that
//!    line is gone.
//! 3. `EPathMap` is a plain generated message here (`ps`, `locally_free`,
//!    `connective_used`, `remainder`), not a trie snapshot. `EZipper` has no
//!    `cursor_kind`.
//! 4. The teardown is this branch's iterative `dismantle_terms`.
//!
//! `feature/mettail` implements the same trait for the same types in its own
//! machine (`5108d8543:models/src/rust/rholang/bincode_decoder.rs:2024-2116`)
//! over a different stored format. At the merge of that branch the two sets of
//! implementations collide (E0119), and the collision marks this file for
//! reconciliation.
//!
//! `formal/rocq/cost_accounted_rho/theories/StackSafeHistoryDecode.v` proves,
//! for a model of this machine, that it returns the value and the remainder of
//! the recursive decoder or rejects exactly when the recursive decoder
//! rejects, that its build steps never find too few values, and that it needs
//! a number of steps linear in the input length. The tests in
//! `models/tests/record_decoder_*.rs` compare it with the derived decoder.
//!
//! ## The format
//!
//! Bincode 1.3.3 with `DefaultOptions::new().with_fixint_encoding()
//! .allow_trailing_bytes()`. Integers are fixed-width little-endian. A `bool`
//! and an `Option` tag are one byte each. A `u64` length precedes each
//! sequence, map and byte or string payload. Structs are positional: field
//! order is declaration order, and names never reach the wire. An enum is a
//! `u32` variant index in declaration order, then its payload. A oneof is an
//! `Option` of such an enum. The stream is therefore a pre-order encoding of
//! the term tree, and one left-to-right pass decodes it.
//!
//! ## The machine
//!
//! `ops` is the work stack. Each `Op` is one bounded step of one type's field
//! program: it may read a bounded number of bytes, push successor ops, and
//! push exactly one finished value onto that type's value stack. A `*Build`
//! op takes exactly the children that its own program pushed, with
//! `Vec::split_off`, which keeps the stream order of siblings. A sequence of
//! `n` elements is one counted repeat ([`Op::Rep`]), so a hostile length
//! costs O(1) memory and fails at the first element that runs out of input,
//! where the derived decoder fails too. Every pushed value consumes at least
//! one input byte, so the live values are bounded by the input length.
//!
//! ## Wire shapes that a hand-written decoder can get wrong
//!
//! 1. `New.injections` is a `BTreeMap<String, Par>`: a count, then key and
//!    value pairs. A duplicate key overwrites, as `BTreeMap::insert` does.
//! 2. The 13 `locally_free` fields are written as empty bytes by
//!    `serialize_as_empty_bytes`, but read back at their real length. The
//!    decoder reads the real length.
//! 3. `ConnNotBody` holds a bare `Par` with no `Option` tag.
//! 4. `ETuple` has no `remainder` field, unlike `EList`, `ESet` and `EMap`.
//!
//! ## The rejection set
//!
//! A node that accepts a byte string that another node rejects forks, so the
//! decoder fails on exactly the inputs on which the derived decoder fails:
//! an invalid `bool` byte, an invalid `Option` tag, a variant index out of
//! range, invalid UTF-8 and a truncated input each give the matching error.
//! The error kinds can differ from bincode's; the Ok/Err disposition cannot.
//!
//! ## Teardown
//!
//! The derived `Drop` of a deep `Par` recurses once for each level. A decode
//! that fails late in a deep term holds deep partial values, so the
//! machine's `Drop` moves every value stack into one work list and releases
//! it with `dismantle_terms`, which uses O(1) native stack.

use std::collections::BTreeMap;
use std::mem;

use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    cautious_capacity, ColdStoreDecode, ColdStoreDecodeError,
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
    EMap, EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap,
    EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GAuthorityId,
    GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrincipalId, GPrivate, GSysAuthToken,
    GUnforgeable, If, KeyValuePair, ListBindPatterns, ListParWithRandom, Match, MatchCase, New,
    Par, ParWithRandom, Receive, ReceiveBind, Send, TaggedContinuation, Var, VarRef,
};
use crate::rust::rholang::dismantle::dismantle_terms;

type Res<T> = Result<T, ColdStoreDecodeError>;

// ===========================================================================
// §A  The serde variant indices of this branch's schema
// ===========================================================================

/// The `ExprInstance` arms in declaration order, which is the serde variant
/// order. The proto tags differ: `EPathmapBody` has tag 32 and index 25.
pub const EX_G_BOOL: u32 = 0;
pub const EX_G_INT: u32 = 1;
pub const EX_G_STRING: u32 = 2;
pub const EX_G_URI: u32 = 3;
pub const EX_G_BYTE_ARRAY: u32 = 4;
pub const EX_E_NOT_BODY: u32 = 5;
pub const EX_E_NEG_BODY: u32 = 6;
pub const EX_E_MULT_BODY: u32 = 7;
pub const EX_E_DIV_BODY: u32 = 8;
pub const EX_E_PLUS_BODY: u32 = 9;
pub const EX_E_MINUS_BODY: u32 = 10;
pub const EX_E_LT_BODY: u32 = 11;
pub const EX_E_LTE_BODY: u32 = 12;
pub const EX_E_GT_BODY: u32 = 13;
pub const EX_E_GTE_BODY: u32 = 14;
pub const EX_E_EQ_BODY: u32 = 15;
pub const EX_E_NEQ_BODY: u32 = 16;
pub const EX_E_AND_BODY: u32 = 17;
pub const EX_E_OR_BODY: u32 = 18;
pub const EX_E_VAR_BODY: u32 = 19;
pub const EX_E_LIST_BODY: u32 = 20;
pub const EX_E_TUPLE_BODY: u32 = 21;
pub const EX_E_SET_BODY: u32 = 22;
pub const EX_E_MAP_BODY: u32 = 23;
pub const EX_E_METHOD_BODY: u32 = 24;
pub const EX_E_PATHMAP_BODY: u32 = 25;
pub const EX_E_ZIPPER_BODY: u32 = 26;
pub const EX_E_MATCHES_BODY: u32 = 27;
pub const EX_E_PERCENT_PERCENT_BODY: u32 = 28;
pub const EX_E_PLUS_PLUS_BODY: u32 = 29;
pub const EX_E_MINUS_MINUS_BODY: u32 = 30;
pub const EX_E_MOD_BODY: u32 = 31;
pub const EX_G_DOUBLE: u32 = 32;
pub const EX_G_BIG_INT: u32 = 33;
pub const EX_G_BIG_RAT: u32 = 34;
pub const EX_G_FIXED_POINT: u32 = 35;
pub const EX_G_UINT64: u32 = 36;
pub const EX_G_FLOAT32: u32 = 37;
pub const EX_G_INT32: u32 = 38;
pub const EX_G_UINT32: u32 = 39;
pub const EX_G_UINT16: u32 = 40;
pub const EX_G_UINT8: u32 = 41;
pub const EXPR_INSTANCE_VARIANT_COUNT: u32 = 42;

/// The `ConnectiveInstance` arms in declaration order.
pub const CN_CONN_AND_BODY: u32 = 0;
pub const CN_CONN_OR_BODY: u32 = 1;
pub const CN_CONN_NOT_BODY: u32 = 2;
pub const CN_VAR_REF_BODY: u32 = 3;
pub const CN_CONN_BOOL: u32 = 4;
pub const CN_CONN_INT: u32 = 5;
pub const CN_CONN_STRING: u32 = 6;
pub const CN_CONN_URI: u32 = 7;
pub const CN_CONN_BYTE_ARRAY: u32 = 8;
pub const CONNECTIVE_INSTANCE_VARIANT_COUNT: u32 = 9;

/// The `CostSignature.Value` arms in declaration order.
pub const CS_GROUND: u32 = 0;
pub const CS_BOUND_LEVEL: u32 = 1;
pub const CS_QUOTE: u32 = 2;
pub const CS_COMPOUND: u32 = 3;
pub const CS_NAME: u32 = 4;
pub const CS_UNIT: u32 = 5;
pub const COST_VALUE_VARIANT_COUNT: u32 = 6;

/// `VarInstance`: `BoundVar`, `FreeVar`, `Wildcard`.
pub const VAR_INSTANCE_VARIANT_COUNT: u32 = 3;
/// `UnfInstance`: the six unforgeable bodies.
pub const UNF_INSTANCE_VARIANT_COUNT: u32 = 6;
/// `TaggedCont`: `ParBody`, `ScalaBodyRef`.
pub const TAGGED_CONT_VARIANT_COUNT: u32 = 2;

/// The serde variant index of each `ExprInstance` arm. The `match` has no
/// wildcard arm, so a new arm fails to compile here until it has an index,
/// and the pin tests then check that index against bincode.
#[doc(hidden)]
pub fn expr_instance_variant_index(instance: &ExprInstance) -> u32 {
    match instance {
        ExprInstance::GBool(_) => EX_G_BOOL,
        ExprInstance::GInt(_) => EX_G_INT,
        ExprInstance::GString(_) => EX_G_STRING,
        ExprInstance::GUri(_) => EX_G_URI,
        ExprInstance::GByteArray(_) => EX_G_BYTE_ARRAY,
        ExprInstance::ENotBody(_) => EX_E_NOT_BODY,
        ExprInstance::ENegBody(_) => EX_E_NEG_BODY,
        ExprInstance::EMultBody(_) => EX_E_MULT_BODY,
        ExprInstance::EDivBody(_) => EX_E_DIV_BODY,
        ExprInstance::EPlusBody(_) => EX_E_PLUS_BODY,
        ExprInstance::EMinusBody(_) => EX_E_MINUS_BODY,
        ExprInstance::ELtBody(_) => EX_E_LT_BODY,
        ExprInstance::ELteBody(_) => EX_E_LTE_BODY,
        ExprInstance::EGtBody(_) => EX_E_GT_BODY,
        ExprInstance::EGteBody(_) => EX_E_GTE_BODY,
        ExprInstance::EEqBody(_) => EX_E_EQ_BODY,
        ExprInstance::ENeqBody(_) => EX_E_NEQ_BODY,
        ExprInstance::EAndBody(_) => EX_E_AND_BODY,
        ExprInstance::EOrBody(_) => EX_E_OR_BODY,
        ExprInstance::EVarBody(_) => EX_E_VAR_BODY,
        ExprInstance::EListBody(_) => EX_E_LIST_BODY,
        ExprInstance::ETupleBody(_) => EX_E_TUPLE_BODY,
        ExprInstance::ESetBody(_) => EX_E_SET_BODY,
        ExprInstance::EMapBody(_) => EX_E_MAP_BODY,
        ExprInstance::EMethodBody(_) => EX_E_METHOD_BODY,
        ExprInstance::EPathmapBody(_) => EX_E_PATHMAP_BODY,
        ExprInstance::EZipperBody(_) => EX_E_ZIPPER_BODY,
        ExprInstance::EMatchesBody(_) => EX_E_MATCHES_BODY,
        ExprInstance::EPercentPercentBody(_) => EX_E_PERCENT_PERCENT_BODY,
        ExprInstance::EPlusPlusBody(_) => EX_E_PLUS_PLUS_BODY,
        ExprInstance::EMinusMinusBody(_) => EX_E_MINUS_MINUS_BODY,
        ExprInstance::EModBody(_) => EX_E_MOD_BODY,
        ExprInstance::GDouble(_) => EX_G_DOUBLE,
        ExprInstance::GBigInt(_) => EX_G_BIG_INT,
        ExprInstance::GBigRat(_) => EX_G_BIG_RAT,
        ExprInstance::GFixedPoint(_) => EX_G_FIXED_POINT,
        ExprInstance::GUint64(_) => EX_G_UINT64,
        ExprInstance::GFloat32(_) => EX_G_FLOAT32,
        ExprInstance::GInt32(_) => EX_G_INT32,
        ExprInstance::GUint32(_) => EX_G_UINT32,
        ExprInstance::GUint16(_) => EX_G_UINT16,
        ExprInstance::GUint8(_) => EX_G_UINT8,
    }
}

/// The serde variant index of each `ConnectiveInstance` arm, exhaustive like
/// [`expr_instance_variant_index`].
#[doc(hidden)]
pub fn connective_instance_variant_index(instance: &ConnectiveInstance) -> u32 {
    match instance {
        ConnectiveInstance::ConnAndBody(_) => CN_CONN_AND_BODY,
        ConnectiveInstance::ConnOrBody(_) => CN_CONN_OR_BODY,
        ConnectiveInstance::ConnNotBody(_) => CN_CONN_NOT_BODY,
        ConnectiveInstance::VarRefBody(_) => CN_VAR_REF_BODY,
        ConnectiveInstance::ConnBool(_) => CN_CONN_BOOL,
        ConnectiveInstance::ConnInt(_) => CN_CONN_INT,
        ConnectiveInstance::ConnString(_) => CN_CONN_STRING,
        ConnectiveInstance::ConnUri(_) => CN_CONN_URI,
        ConnectiveInstance::ConnByteArray(_) => CN_CONN_BYTE_ARRAY,
    }
}

/// The serde variant index of each `CostSignature.Value` arm, exhaustive like
/// [`expr_instance_variant_index`].
#[doc(hidden)]
pub fn cost_value_variant_index(value: &CostValue) -> u32 {
    match value {
        CostValue::Ground(_) => CS_GROUND,
        CostValue::BoundLevel(_) => CS_BOUND_LEVEL,
        CostValue::Quote(_) => CS_QUOTE,
        CostValue::Compound(_) => CS_COMPOUND,
        CostValue::Name(_) => CS_NAME,
        CostValue::Unit(_) => CS_UNIT,
    }
}

/// The `ExprInstance` arms whose wire shape is `(Option<Par>, Option<Par>)`.
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

/// Rebuilds a binary `ExprInstance` arm from its variant index and two
/// operands. `EMatches` names its fields `target` and `pattern`, but its wire
/// is the same: two `Option<Par>` in declaration order.
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
// §B  The reader: bincode 1.3.3 fixint little-endian primitives over `&[u8]`
// ===========================================================================

/// A cursor over the encoded bytes with bincode's primitives and bincode's
/// rule of checking a length against the remaining input before any
/// allocation (`SliceReader::get_byte_slice`, `bincode/src/de/read.rs:46-54`).
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self { Reader { bytes, pos: 0 } }

    fn consumed(&self) -> usize { self.pos }

    fn remaining(&self) -> usize { self.bytes.len() - self.pos }

    /// The bounds-checked slice read. Every payload read goes through here, so
    /// no allocation is sized by an unchecked stream length.
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

    /// `deserialize_bool`: a byte other than 0 or 1 is `InvalidBoolEncoding`
    /// (`bincode/src/de/mod.rs:132-141`).
    fn bool(&mut self) -> Res<bool> {
        match self.byte("a bool")? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(ColdStoreDecodeError::InvalidBoolEncoding(other)),
        }
    }

    /// `deserialize_option`: a tag other than 0 or 1 is `InvalidTagEncoding`
    /// (`bincode/src/de/mod.rs:332-342`).
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

    /// A `u64` length narrowed to `usize` (`bincode/src/config/int.rs:69-73`).
    /// An oversized length fails later in [`Reader::take`] or when a counted
    /// repeat runs out of input, where the derived decoder fails too.
    fn len(&mut self) -> Res<usize> {
        let n = self.u64()?;
        usize::try_from(n).map_err(|_| ColdStoreDecodeError::LengthOverflow(n))
    }

    /// A `Vec<u8>` field. serde reads it as a generic sequence: a `u64` count,
    /// then that many single bytes. That is byte-identical to what
    /// `serialize_bytes` writes.
    fn byte_seq(&mut self) -> Res<Vec<u8>> {
        let n = self.len()?;
        self.take(n, "a byte sequence").map(<[u8]>::to_vec)
    }

    /// `deserialize_string`: read the length-prefixed bytes first, so that a
    /// short buffer is `UnexpectedEof` and not a UTF-8 error, then check UTF-8
    /// (`bincode/src/de/mod.rs:99-102`).
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

    /// A oneof variant index: a `u32`, checked against the arm count.
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

    // -------- bounded leaves: the types outside the recursive `Par` family

    fn var(&mut self) -> Res<Var> {
        if !self.option_tag()? {
            return Ok(Var { var_instance: None });
        }
        let instance = match self.variant("VarInstance", VAR_INSTANCE_VARIANT_COUNT)? {
            0 => VarInstance::BoundVar(self.i32()?),
            1 => VarInstance::FreeVar(self.i32()?),
            // `WildcardMsg` has no field, so it consumes nothing.
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
        }
    }
}

/// One bounded step of one type's field program. `*Start` opens a value,
/// `*Build` closes it, and an op in between is a resume point: the state that
/// must survive the descent into a child. Small scalars read before a descent
/// ride in the op. Larger ones ride on a side stack ([`ParFrame`],
/// [`ReceiveTail`], [`NewFrame`], [`Machine::method_names`]).
#[derive(Clone, Copy, Debug)]
enum Op {
    /// The counted repeat. It never materializes `remaining` ops.
    Rep {
        kind: Kind,
        remaining: usize,
    },

    // ---- Par: sends, receives, news, exprs, matches, unforgeables, bundles,
    //           connectives, conditionals, locally_free, connective_used,
    //           cost_signed_terms, cost_stacks
    ParStart,
    /// `field` indexes the eight repeated `Par`-bearing fields in declaration
    /// order. `unforgeables`, a bounded leaf sequence, is read inline just
    /// before field 5, where it sits in the stream.
    ParField {
        field: u8,
    },
    ParCostSigned,
    ParCostStacks,
    ParBuild,

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

    // ---- ReceiveBind: patterns, source, remainder, free_count, cost_signature
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

    // ---- the cost family
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
    CostStackBuild {
        count: usize,
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
    /// One map entry: read the key, then descend for the value.
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

    // ---- Expr and ExprInstance
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
    /// `EList` and `ESet`: ps, locally_free, connective_used, remainder.
    ExprSeqBuild {
        variant: u32,
        n: usize,
    },
    /// `ETuple`: ps, locally_free, connective_used. It has no remainder.
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
    /// Wraps the `EPathMap` on top of the path-map stack into an `Expr`.
    ExprFromPathmap,
    ExprZipperBuild {
        has_pathmap: bool,
    },

    // ---- EPathMap: ps, locally_free, connective_used, remainder
    PathmapStart,
    PathmapBuild {
        n: usize,
    },

    // ---- KeyValuePair: key, value
    KvStart,
    KvAfterKey {
        t_key: bool,
    },
    KvBuild {
        t_key: bool,
        t_value: bool,
    },

    // ---- Connective and ConnectiveInstance
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
    /// `TaggedCont::ScalaBodyRef(i64)`, with the value already read.
    TaggedContinuationScala {
        t_guard: bool,
        value: i64,
        has_authority: bool,
    },
    /// `tagged_cont` is absent.
    TaggedContinuationAbsent {
        t_guard: bool,
        has_authority: bool,
    },

    // ---- ListParWithRandom: pars, random_state, cost_authority, cost_stack
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

    // ---- ParWithRandom: body, random_state (the test entry point)
    ParWithRandomStart,
    ParWithRandomBuild {
        has_body: bool,
    },

    // ---- BindPattern: patterns, remainder, free_count
    BindPatternStart,
    BindPatternBuild {
        n: usize,
    },

    // ---- ListBindPatterns: patterns (the test entry point)
    ListBindPatternsStart,
    ListBindPatternsBuild {
        n: usize,
    },
}

// ===========================================================================
// §D  Side frames: state too large to ride in an `Op`
// ===========================================================================

/// The counts of `Par`'s eight repeated children, plus the fields read
/// between descents.
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

/// `Receive`'s five scalars, read between `body` and `condition`.
struct ReceiveTail {
    persistent: bool,
    peek: bool,
    bind_count: i32,
    locally_free: Vec<u8>,
    connective_used: bool,
}

/// `New`'s scalars and its injection-map keys, which are read between the
/// (arbitrarily deep) values.
struct NewFrame {
    bind_count: i32,
    uri: Vec<String>,
    keys: Vec<String>,
}

// ===========================================================================
// §E  The machine
// ===========================================================================

struct Machine<'a> {
    r: Reader<'a>,
    ops: Vec<Op>,

    // the value stack of each type
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
}

/// Takes the last `n` values off a stack, keeping stream order. The children
/// were pushed in stream order, so the tail slice is the sibling list.
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

/// Pops a `Par` when `present`, and gives `None` otherwise.
fn take_opt_par(stack: &mut Vec<Par>, present: bool, what: &'static str) -> Res<Option<Par>> {
    if present {
        Ok(Some(take_one(stack, what)?))
    } else {
        Ok(None)
    }
}

/// The stack on which a successful run leaves its root value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DrainedRoot {
    Par,
    ListParWithRandom,
    BindPattern,
    TaggedContinuation,
    ParWithRandom,
    ListBindPatterns,
}

impl<'a> Machine<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Machine {
            r: Reader::new(bytes),
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
        }
    }

    /// Schedules `count` repetitions of `kind` as one op.
    fn repeat(&mut self, kind: Kind, count: usize) {
        if count > 0 {
            self.ops.push(Op::Rep {
                kind,
                remaining: count,
            });
        }
    }

    /// Schedules a child `Par` decode when `present`.
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

    /// The interpreter loop. The only recursion of this module is inside the
    /// bounded leaf readers of [`Reader`].
    fn run(&mut self) -> Res<()> {
        while let Some(op) = self.ops.pop() {
            self.step(op)?;
        }
        Ok(())
    }

    /// After a successful run exactly one value remains, on the root's own
    /// stack. Anything else is a decoder defect, not a bad input.
    fn assert_drained(&self, root: DrainedRoot) -> Res<()> {
        let counts = [
            ("pars", self.pars.len(), root == DrainedRoot::Par),
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
                root == DrainedRoot::BindPattern,
            ),
            (
                "list_bind_patterns",
                self.list_bind_patterns.len(),
                root == DrainedRoot::ListBindPatterns,
            ),
            (
                "par_with_randoms",
                self.par_with_randoms.len(),
                root == DrainedRoot::ParWithRandom,
            ),
            (
                "list_par_with_randoms",
                self.list_par_with_randoms.len(),
                root == DrainedRoot::ListParWithRandom,
            ),
            (
                "tagged_continuations",
                self.tagged_continuations.len(),
                root == DrainedRoot::TaggedContinuation,
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
        {
            return Err(ColdStoreDecodeError::MachineInvariant(
                "side frame not drained at end of run",
            ));
        }
        Ok(())
    }

    fn step(&mut self, op: Op) -> Res<()> {
        match op {
            Op::Rep { kind, remaining } => {
                // `remaining` comes from the stream and can be huge. Each
                // iteration costs O(1) memory and the repeat fails at the
                // first element that runs out of input.
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
                // The eight repeated `Par`-bearing fields in declaration
                // order. `unforgeables` sits between indices 4 and 5.
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
                let count = self.r.len()?;
                let frame = self.par_frame()?;
                frame.locally_free = locally_free;
                frame.connective_used = connective_used;
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
                let cost_stacks = take_n(
                    &mut self.cost_stacks,
                    frame.n_cost_stacks,
                    "Par.cost_stacks",
                )?;
                let cost_signed_terms = take_n(
                    &mut self.cost_signed_terms,
                    frame.n_cost_signed_terms,
                    "Par.cost_signed_terms",
                )?;
                // Each kind has its own stack, so the drains are independent.
                let conditionals = take_n(&mut self.ifs, frame.counts[7], "Par.conditionals")?;
                let connectives =
                    take_n(&mut self.connectives, frame.counts[6], "Par.connectives")?;
                let bundles = take_n(&mut self.bundles, frame.counts[5], "Par.bundles")?;
                let matches = take_n(&mut self.matches, frame.counts[4], "Par.matches")?;
                let exprs = take_n(&mut self.exprs, frame.counts[3], "Par.exprs")?;
                let news = take_n(&mut self.news, frame.counts[2], "Par.news")?;
                let receives = take_n(&mut self.receives, frame.counts[1], "Par.receives")?;
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
                // `chan` was pushed before the `data` elements, so it comes off
                // after them.
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
            // ReceiveBind
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
                let has_signature = self.r.option_tag()?;
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
                // `source` was pushed after the patterns, so it comes off first.
                let source = take_opt_par(&mut self.pars, has_source, "ReceiveBind.source")?;
                let patterns = take_n(&mut self.pars, n_patterns, "ReceiveBind.patterns")?;
                self.binds.push(ReceiveBind {
                    patterns,
                    source,
                    remainder: tail.remainder,
                    free_count: tail.free_count,
                    cost_signature,
                });
            }

            // ---------------------------------------------------------------
            // The cost family
            // ---------------------------------------------------------------
            Op::CostSignatureStart => {
                if !self.r.option_tag()? {
                    self.cost_signatures.push(CostSignature { value: None });
                    return Ok(());
                }
                let variant = self
                    .r
                    .variant("CostSignature.Value", COST_VALUE_VARIANT_COUNT)?;
                match variant {
                    CS_GROUND => self.cost_signatures.push(CostSignature {
                        value: Some(CostValue::Ground(self.r.byte_seq()?)),
                    }),
                    CS_BOUND_LEVEL => self.cost_signatures.push(CostSignature {
                        value: Some(CostValue::BoundLevel(self.r.i32()?)),
                    }),
                    CS_UNIT => self.cost_signatures.push(CostSignature {
                        value: Some(CostValue::Unit(self.r.bool()?)),
                    }),
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
                self.cost_signatures
                    .push(CostSignature { value: Some(value) });
            }
            Op::CostSignatureCompoundBuild { count } => {
                let elements = take_n(
                    &mut self.cost_signatures,
                    count,
                    "CostSignatureCompound.elements",
                )?;
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
                self.cost_signed_terms
                    .push(CostSignedTerm { body, signature });
            }
            Op::CostStackStart => {
                let count = self.r.len()?;
                self.ops.push(Op::CostStackBuild { count });
                self.repeat(Kind::CostSignature, count);
            }
            Op::CostStackBuild { count } => {
                let cells = take_n(&mut self.cost_signatures, count, "CostStack.cells")?;
                self.cost_stacks.push(CostStack { cells });
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
                self.cost_authorities.push(CostAuthority { regions });
            }

            // ---------------------------------------------------------------
            // New: wire shape 1
            // ---------------------------------------------------------------
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
                {
                    let frame = self.new_frame()?;
                    frame.uri = uri;
                    frame.keys = Vec::with_capacity(cautious_capacity::<String>(n_entries));
                }
                self.ops.push(Op::NewBuild { has_p, n_entries });
                self.ops.push(Op::NewEntry {
                    remaining: n_entries,
                });
            }
            Op::NewEntry { remaining } => {
                // The count is read. Then come key and value pairs, the keys
                // interleaved with arbitrarily deep values.
                if remaining > 0 {
                    let key = self.r.string()?;
                    self.new_frame()?.keys.push(key);
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
                // `BTreeMap::insert` in stream order: a duplicate key
                // overwrites, as serde's map visitor does, and the stream need
                // not be sorted.
                let mut injections = BTreeMap::new();
                for (key, value) in frame.keys.into_iter().zip(values) {
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
            // Match and MatchCase
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
            // Expr
            // ---------------------------------------------------------------
            Op::ExprStart => {
                if !self.r.option_tag()? {
                    self.exprs.push(Expr {
                        expr_instance: None,
                    });
                    return Ok(());
                }
                let variant = self
                    .r
                    .variant("ExprInstance", EXPR_INSTANCE_VARIANT_COUNT)?;
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
                        // `fixed64` that carries the IEEE-754 bits.
                        let v = self.r.u64()?;
                        self.push_expr(ExprInstance::GDouble(v));
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
                    EX_G_UINT64 => {
                        let v = self.r.u64()?;
                        self.push_expr(ExprInstance::GUint64(v));
                    }
                    EX_G_FLOAT32 => {
                        // `fixed32` that carries the IEEE-754 bits.
                        let v = self.r.u32()?;
                        self.push_expr(ExprInstance::GFloat32(v));
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
                        // A `u32` on the wire. The value range is not checked
                        // here, because the derived decoder does not check it.
                        let v = self.r.u32()?;
                        self.push_expr(ExprInstance::GUint16(v));
                    }
                    EX_G_UINT8 => {
                        // A `u32` on the wire, as for `GUint16`.
                        let v = self.r.u32()?;
                        self.push_expr(ExprInstance::GUint8(v));
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

                    // ---- method: the name is read first and must survive two
                    //      descents, so it goes on a side stack.
                    EX_E_METHOD_BODY => {
                        let name = self.r.string()?;
                        self.method_names.push(name);
                        let has_target = self.r.option_tag()?;
                        self.ops.push(Op::ExprMethodArgs { has_target });
                        self.opt_par_child(has_target);
                    }

                    // ---- the path-map arms
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
            }
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
                // `ETuple` has no `remainder` field. Reading one would shift
                // the stream.
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
                let arguments = take_n(&mut self.pars, n_args, "EMethod.arguments")?;
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
                }));
            }

            // ---------------------------------------------------------------
            // EPathMap
            // ---------------------------------------------------------------
            Op::PathmapStart => {
                let n = self.r.len()?;
                self.ops.push(Op::PathmapBuild { n });
                self.repeat(Kind::Par, n);
            }
            Op::PathmapBuild { n } => {
                let ps = take_n(&mut self.pars, n, "EPathMap.ps")?;
                let locally_free = self.r.byte_seq()?;
                let connective_used = self.r.bool()?;
                let remainder = self.r.opt_var()?;
                self.pathmaps.push(EPathMap {
                    ps,
                    locally_free,
                    connective_used,
                    remainder,
                });
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

            // ---------------------------------------------------------------
            // Connective
            // ---------------------------------------------------------------
            Op::ConnStart => {
                if !self.r.option_tag()? {
                    self.connectives.push(Connective {
                        connective_instance: None,
                    });
                    return Ok(());
                }
                let variant = self
                    .r
                    .variant("ConnectiveInstance", CONNECTIVE_INSTANCE_VARIANT_COUNT)?;
                match variant {
                    CN_CONN_AND_BODY | CN_CONN_OR_BODY => {
                        let n = self.r.len()?;
                        self.ops.push(Op::ConnBodyBuild { variant, n });
                        self.repeat(Kind::Par, n);
                    }
                    // `ConnNotBody` holds a bare `Par`, not an `Option<Par>`:
                    // no tag byte precedes it.
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
                let has_authority = self.r.option_tag()?;
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
                let cost_authority = self.take_authority(has_authority)?;
                self.tagged_continuations.push(TaggedContinuation {
                    guard,
                    cost_authority,
                    tagged_cont: Some(TaggedCont::ParBody(ParWithRandom { body, random_state })),
                });
            }
            Op::TaggedContinuationScala {
                t_guard,
                value,
                has_authority,
            } => {
                let guard = take_opt_par(&mut self.pars, t_guard, "TaggedContinuation.guard")?;
                let cost_authority = self.take_authority(has_authority)?;
                self.tagged_continuations.push(TaggedContinuation {
                    guard,
                    cost_authority,
                    tagged_cont: Some(TaggedCont::ScalaBodyRef(value)),
                });
            }
            Op::TaggedContinuationAbsent {
                t_guard,
                has_authority,
            } => {
                let guard = take_opt_par(&mut self.pars, t_guard, "TaggedContinuation.guard")?;
                let cost_authority = self.take_authority(has_authority)?;
                self.tagged_continuations.push(TaggedContinuation {
                    guard,
                    cost_authority,
                    tagged_cont: None,
                });
            }

            // ---------------------------------------------------------------
            // ListParWithRandom and ParWithRandom
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
                let has_authority = self.r.option_tag()?;
                self.ops
                    .push(Op::ListParWithRandomCostStack { has_authority });
                if has_authority {
                    self.ops.push(Op::CostAuthorityStart);
                }
            }
            Op::ListParWithRandomCostStack { has_authority } => {
                let authority = self.take_authority(has_authority)?;
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
                self.par_with_randoms
                    .push(ParWithRandom { body, random_state });
            }

            // ---------------------------------------------------------------
            // BindPattern and ListBindPatterns
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
                self.list_bind_patterns.push(ListBindPatterns { patterns });
            }
        }
        Ok(())
    }

    /// Pops the cost authority of a continuation or a datum when present.
    fn take_authority(&mut self, present: bool) -> Res<Option<CostAuthority>> {
        if present {
            Ok(Some(take_one(
                &mut self.cost_authorities,
                "cost_authority",
            )?))
        } else {
            Ok(None)
        }
    }

    /// Runs the program of one root and returns its value and the number of
    /// bytes it consumed.
    fn decode_root<T>(
        bytes: &'a [u8],
        start: Op,
        root: DrainedRoot,
        take: fn(&mut Machine<'a>) -> Res<T>,
    ) -> Res<(T, usize)> {
        let mut machine = Machine::new(bytes);
        machine.ops.push(start);
        machine.run()?;
        machine.assert_drained(root)?;
        let consumed = machine.r.consumed();
        let value = take(&mut machine)?;
        Ok((value, consumed))
    }

    /// `true` when no value stack holds anything, which is the state that a
    /// successful run leaves once its root value is taken.
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

    /// Releases every partly decoded value without recursion. Every value
    /// stack goes into one `Par` shell or the loose work list, and
    /// `dismantle_terms` takes the result apart with O(1) native stack.
    fn dismantle(&mut self) {
        // `Drop` also runs after success, when every stack is empty.
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

        for pattern in mem::take(&mut self.bind_patterns) {
            loose.extend(pattern.patterns);
        }
        for list in mem::take(&mut self.list_bind_patterns) {
            for pattern in list.patterns {
                loose.extend(pattern.patterns);
            }
        }
        for with_random in mem::take(&mut self.par_with_randoms) {
            loose.extend(with_random.body);
        }
        let mut loose_cost_stacks = mem::take(&mut self.cost_stacks);
        for list in mem::take(&mut self.list_par_with_randoms) {
            loose.extend(list.pars);
            if let Some(authority) = list.cost_authority {
                for region in authority.regions {
                    loose_cost_signatures.extend(region.signature);
                }
            }
            loose_cost_stacks.extend(list.cost_stack);
        }
        for continuation in mem::take(&mut self.tagged_continuations) {
            loose.extend(continuation.guard);
            if let Some(TaggedCont::ParBody(with_random)) = continuation.tagged_cont {
                loose.extend(with_random.body);
            }
            if let Some(authority) = continuation.cost_authority {
                for region in authority.regions {
                    loose_cost_signatures.extend(region.signature);
                }
            }
        }

        let mut root = Par {
            sends: mem::take(&mut self.sends),
            receives,
            news: mem::take(&mut self.news),
            exprs,
            matches,
            bundles: mem::take(&mut self.bundles),
            connectives: mem::take(&mut self.connectives),
            conditionals: mem::take(&mut self.ifs),
            cost_signed_terms: mem::take(&mut self.cost_signed_terms),
            cost_stacks: loose_cost_stacks,
            ..Default::default()
        };
        if !loose_cost_signatures.is_empty() {
            root.cost_stacks.push(CostStack {
                cells: loose_cost_signatures,
            });
        }
        loose.push(root);
        dismantle_terms(loose);
    }
}

impl Drop for Machine<'_> {
    fn drop(&mut self) { self.dismantle(); }
}

// ===========================================================================
// §F  The four roots of the native history records
// ===========================================================================

impl ColdStoreDecode for Par {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        Machine::decode_root(bytes, Op::ParStart, DrainedRoot::Par, |machine| {
            take_one(&mut machine.pars, "Par (root)")
        })
    }
}

impl ColdStoreDecode for ListParWithRandom {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        Machine::decode_root(
            bytes,
            Op::ListParWithRandomStart,
            DrainedRoot::ListParWithRandom,
            |machine| {
                take_one(
                    &mut machine.list_par_with_randoms,
                    "ListParWithRandom (root)",
                )
            },
        )
    }
}

impl ColdStoreDecode for BindPattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        Machine::decode_root(
            bytes,
            Op::BindPatternStart,
            DrainedRoot::BindPattern,
            |machine| take_one(&mut machine.bind_patterns, "BindPattern (root)"),
        )
    }
}

impl ColdStoreDecode for TaggedContinuation {
    fn cold_decode_prefix(bytes: &[u8]) -> Res<(Self, usize)> {
        Machine::decode_root(
            bytes,
            Op::TaggedContinuationStart,
            DrainedRoot::TaggedContinuation,
            |machine| {
                take_one(
                    &mut machine.tagged_continuations,
                    "TaggedContinuation (root)",
                )
            },
        )
    }
}

// ===========================================================================
// §G  Test entry points for the machine types that are not roots
// ===========================================================================

/// `ParWithRandom` and `ListBindPatterns` are on the machine, because they
/// contain `Par`, but no native history record has them as its root. These
/// entry points let the differential tests run their programs directly.
#[doc(hidden)]
pub fn cold_decode_par_with_random_for_test(
    bytes: &[u8],
) -> Result<(ParWithRandom, usize), ColdStoreDecodeError> {
    Machine::decode_root(
        bytes,
        Op::ParWithRandomStart,
        DrainedRoot::ParWithRandom,
        |machine| take_one(&mut machine.par_with_randoms, "ParWithRandom (root)"),
    )
}

/// See [`cold_decode_par_with_random_for_test`].
#[doc(hidden)]
pub fn cold_decode_list_bind_patterns_for_test(
    bytes: &[u8],
) -> Result<(ListBindPatterns, usize), ColdStoreDecodeError> {
    Machine::decode_root(
        bytes,
        Op::ListBindPatternsStart,
        DrainedRoot::ListBindPatterns,
        |machine| take_one(&mut machine.list_bind_patterns, "ListBindPatterns (root)"),
    )
}
