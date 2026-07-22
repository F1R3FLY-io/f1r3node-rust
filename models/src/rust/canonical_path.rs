//! The CANONICAL PATH CODEC — the navigable trie-key codec (Job A).
//!
//! A total, injective, invertible byte codec from the `eval_stable` ground
//! grammar to canonical path bytes, plus the parser-state DFS over a trie
//! keyed by these paths. This is THE key the PathMap zippers index on: the
//! Rholang→PathMap layer (`pathmap_integration::create_pathmap_from_elements`)
//! stores each map entry under `encode_trie_path(entry)`, and the interpreter
//! navigates and reconstructs entries through `encode_trie_segment`,
//! `decode_trie_path`, and `collect_child_segments_codec`.
//!
//! A ground map's VALUE serialization is NOT produced here: it is the U(m)
//! key stream a PathMap read-zipper walk emits over the intern trie (see
//! `pathmap_crate_type_mapper` / `rhoapi_ext`). This module owns only the
//! per-element key codec — there is NO top-level value-arm region codec, and
//! NO sort anywhere (nested-map order is the zipper trie-walk order).
//!
//! # The segment grammar (§1.2, one leading tag byte per segment)
//!
//! ```text
//! ┌──────┬───────────────────────┬───────────────────────────────────────────┐
//! │ Tag  │ Constructor           │ Payload after the tag byte                │
//! ├──────┼───────────────────────┼───────────────────────────────────────────┤
//! │ 0x00 │ TERM (top-level list  │ none — never begins a segment; terminates │
//! │      │ terminator)           │ the split form only                       │
//! │ 0x01 │ GBool(false)          │ none                                      │
//! │ 0x02 │ GBool(true)           │ none                                      │
//! │ 0x03 │ GInt(i)               │ sv(i) = uv(zigzag64(i))                   │
//! │ 0x04 │ GString(s)            │ uv(|s|) ++ UTF-8 bytes                    │
//! │ 0x05 │ GUri(u)               │ uv(|u|) ++ UTF-8 bytes                    │
//! │ 0x06 │ GByteArray(b)         │ uv(|b|) ++ raw bytes                      │
//! │ 0x07 │ GDouble(bits)         │ 8 bytes big-endian raw IEEE-754 bits      │
//! │ 0x08 │ GBigInt(b)            │ uv(|b|) ++ raw bytes (as the Par carries) │
//! │ 0x09 │ GBigRat{num, den}     │ uv(|num|) ++ num ++ uv(|den|) ++ den      │
//! │ 0x0A │ GFixedPoint{u, scale} │ uv(|u|) ++ u ++ uv(scale)                 │
//! │ 0x0B │ nested ground EList   │ uv(k) ++ enc(e₁) ‖ … ‖ enc(e_k)           │
//! │ 0x0C │ ground ETuple         │ uv(k) ++ enc(e₁) ‖ … ‖ enc(e_k)           │
//! │ 0x0D │ nested EPathmapBody   │ uv(|R|) ++ R, R = region(m) (ε admitted — │
//! │      │                       │ R3F-1: the canonical empty map)           │
//! │ 0x0E │ reflect GPrivate leaf │ uv(|id|) ++ id bytes                      │
//! │ 0x0F │ TRIE-ONLY escape arm  │ uv(|prost(par)|) ++ canonical prost bytes │
//! │      │ (R3F-2)               │ for ¬eval_stable entries; legal only at a │
//! │      │                       │ top-level segment position               │
//! │ 0x10 │ …0xFF reserved        │ decode rejects                            │
//! └──────┴───────────────────────┴───────────────────────────────────────────┘
//! ```
//!
//! # The top-level path rule (§1.3)
//!
//! * split-eligible entry (single-expr carrier of a default-metadata
//!   `EListBody` `[e₁..e_k]`): `path = enc(e₁) ‖ … ‖ enc(e_k) ‖ 0x00`
//!   (k = 0 ⇒ `path = 0x00`, one byte);
//! * otherwise: `path = enc(par)` (one bare segment), where a bare segment
//!   must NOT carry tag `0x0B` (a top-level ground list must use the split
//!   form — the canonical-form rule that closes the bare-list alias);
//! * decode is deterministic and position-driven: `0x00` is meaningful only
//!   at segment-boundary positions of a path frame; there is NO in-band
//!   separator anywhere.
//!
//! # The nested-map region (the `0x0D` arm)
//!
//! A nested ground map inside a key encodes as `0x0D ++ uv(|R|) ++ R`, where
//! `R = ‖ (uv(|path(e)|) ++ path(e))` over the entry paths in strictly
//! ascending raw-byte order (= PathMap trie iteration order; strictness
//! implies dedup, and under an injective codec path-dedup IS value-dedup).
//! The encoder produces that order with NO sort: it collects the entry paths
//! into a `PathMap<()>` (idempotent insertion = dedup) and emits them via a
//! read-zipper `to_next_val` walk (ascending byte-lex = trie order — the same
//! mechanism the top-level U(m) stream uses). Decode carries the per-region
//! strict-ascending order CHECK inline in the ONE parse pass at every nesting
//! level — STRICT-REJECT, never re-canonicalized on ingest.
//!
//! # Depth (§1.6 / R3F-7)
//!
//! The decode/validate depth limit counts COLLECTION levels (`0x0B`/`0x0C`/
//! `0x0D` nesting; the top-level split form is level 0) and is
//! [`COLLECTION_DEPTH_LIMIT`] = 32 — today's effective prost envelope
//! (prost `RECURSION_LIMIT` = 100 message levels ≈ 25-33 collection levels).
//! The TRIE encoder is TOTAL (R3F-2 — trie keys must build for every legal
//! runtime value) and therefore unlimited on encode; the DECODER enforces the
//! limit, with the documented consequence that a deeper-than-32 trie key
//! encodes but decode-rejects (such values are unrepresentable on today's wire
//! either way). Encode and decode are both ITERATIVE (explicit stacks — no
//! recursion anywhere).
//!
//! # The escape arm (R3F-2)
//!
//! The trie-key domain is ALL Pars (the intern store builds tries for every
//! map). [`encode_trie_path`] is total: an entry (or split element) that is
//! not `eval_stable_par` encodes as `0x0F ++ uv(|prost bytes|) ++ canonical
//! prost bytes` — injective (prost is deterministic + decodable), hereditary
//! (the escape absorbs the highest unstable node; a stable subtree never
//! contains an escape, so `0x0D` regions are escape-free), prefix-free, and
//! deterministically ordered (after `0x0E`; escape-vs-escape by prost bytes).
//! The trie decode rejects an escape appearing below a top-level segment
//! position, or one whose payload is non-canonical prost or is itself
//! `eval_stable` (any of which would alias a stable arm and break
//! decode-accepts ≡ encoder-image).
//!
//! # Domain gate = `eval_stable_par` — ONE grammar
//!
//! The codec's ground-domain gate IS `eval_stable_par`
//! (`pathmap_crate_type_mapper.rs`): a stable Par takes a structural arm and a
//! ¬stable Par takes the `0x0F` escape arm, so `encode_trie_path` is TOTAL
//! over all Pars. The codec grammar and `eval_stable_par` must stay ONE
//! grammar (pinned by the round-trip + totality property tests): every decoded
//! structural value is `eval_stable_par`, and every accepted escape payload is
//! canonical prost of a ¬`eval_stable_par` value.

use prost::Message;

use super::pathmap_crate_type_mapper::eval_stable_par;
use super::pathmap_integration::RholangPathMap;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{
    EList, EPathMap, ETuple, Expr, GBigRational, GFixedPoint, GPrivate, GUnforgeable, Par,
};

// ─────────────────────────────────────────────────────────────────────────────
// Tags and limits
// ─────────────────────────────────────────────────────────────────────────────

/// Segment tag bytes (§1.2). `TERM` never begins a segment.
pub mod tag {
    pub const TERM: u8 = 0x00;
    pub const GBOOL_FALSE: u8 = 0x01;
    pub const GBOOL_TRUE: u8 = 0x02;
    pub const GINT: u8 = 0x03;
    pub const GSTRING: u8 = 0x04;
    pub const GURI: u8 = 0x05;
    pub const GBYTE_ARRAY: u8 = 0x06;
    pub const GDOUBLE: u8 = 0x07;
    pub const GBIG_INT: u8 = 0x08;
    pub const GBIG_RAT: u8 = 0x09;
    pub const GFIXED_POINT: u8 = 0x0A;
    pub const ELIST: u8 = 0x0B;
    pub const ETUPLE: u8 = 0x0C;
    pub const EPATHMAP: u8 = 0x0D;
    pub const GPRIVATE: u8 = 0x0E;
    /// Trie-only (R3F-2); the wire grammar rejects it.
    pub const ESCAPE: u8 = 0x0F;
    /// First reserved tag; the reserved range is `0x10..=0xFF`.
    pub const RESERVED_FLOOR: u8 = 0x10;
}

/// R3F-7: the decode/validate depth limit in COLLECTION levels
/// (`0x0B`/`0x0C`/`0x0D` nesting; the top-level split form is level 0).
/// Accept at exactly 32 open levels; reject the 33rd. The wire ENCODER
/// enforces the same bound (image = accepted set); the trie encoder is
/// total and unlimited (module doc).
pub const COLLECTION_DEPTH_LIMIT: u32 = 32;

/// Defensive ceiling on the segment-scanner op stack (the DFS extent
/// parser): 32 collection levels never need more than 2 frames per level
/// plus slack, so a deeper stack only ever means adversarial bytes.
const SCANNER_STACK_CEILING: usize = 128;

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

/// Rejection reasons of the codec grammar and the region validator. Every
/// variant is a STRICT-REJECT — nothing is ever re-canonicalized on ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// A top-level path must be non-empty (§1.3 decode rule 1).
    EmptyInput,
    /// A read ran past the innermost frame boundary (truncated varint,
    /// payload, or segment; also mid-token end of input).
    Truncated,
    /// A LEB128 varint was not in minimal form (e.g. `80 00`).
    NonMinimalVarint,
    /// A varint exceeded 64 bits.
    VarintOverflow,
    /// A reserved tag (`0x10..=0xFF`) began a segment.
    ReservedTag(u8),
    /// `0x00` appeared somewhere other than the final segment-boundary
    /// position of a path frame (mid-stream terminator, terminator with
    /// trailing bytes, or `0x00` at a nested child-tag position).
    TerminatorMisplaced,
    /// A top-level (bare) segment carried tag `0x0B` — a ground list must
    /// use the split form (§1.3 canonical-form rule).
    BareTopLevelList,
    /// A path frame held more than one segment without a terminator.
    UnterminatedMultiSegment,
    /// A path frame held zero segments (empty path).
    EmptyPath,
    /// A `GString`/`GUri` payload was not valid UTF-8.
    InvalidUtf8,
    /// A `GFixedPoint` scale exceeded `u32` (unrepresentable in the Par).
    ScaleOutOfRange,
    /// Collection nesting exceeded [`COLLECTION_DEPTH_LIMIT`].
    DepthLimitExceeded,
    /// The trie-only escape tag `0x0F` appeared on the wire grammar.
    EscapeOnWire,
    /// The escape tag appeared below the top-level segment position (inside
    /// a collection or a nested region) — the hereditary rule forbids it.
    EscapeNested,
    /// An escape payload failed prost decode.
    EscapePayloadInvalid,
    /// An escape payload re-encoded to different prost bytes (non-canonical
    /// prost encoding — would alias the canonical form).
    EscapePayloadNonCanonical,
    /// An escape payload decoded to an `eval_stable` Par (would alias the
    /// stable arms and break injectivity).
    EscapeOfStablePar,
    /// The wire encoder was handed a Par outside the `eval_stable` domain.
    NotEvalStable,
    /// The top-level `value_entries` region must be non-empty (the canonical
    /// empty map is the all-defaults message; R3F-1 scopes non-emptiness to
    /// the top level ONLY — nested `0x0D` admits R = ε).
    EmptyTopLevelRegion,
    /// A region frame declared length 0 (no entry has an empty path).
    EmptyPathFrame,
    /// A region frame overran its region's end (framing does not tile).
    RegionFrameOverrun,
    /// Adjacent region paths were not strictly ascending.
    NonAscendingRegion,
    /// Adjacent region paths were byte-identical (dedup violation —
    /// strictness implies dedup).
    DuplicatePathInRegion,
    /// The malleability fence failed: `encode(decode(path)) != path`.
    CanonicalFormMismatch,
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CodecError {}

// ─────────────────────────────────────────────────────────────────────────────
// Varint primitives (§1.1)
// ─────────────────────────────────────────────────────────────────────────────

/// Write the minimal unsigned LEB128 form of `value` into `out`.
fn write_uv(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let group = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(group);
            return;
        }
        out.push(group | 0x80);
    }
}

/// R3F-8 nit honored: zigzag computed in `i64` arithmetic, ONE cast at the
/// end. (`<<`/`>>` on `i64` are bit shifts — no overflow trap.)
fn zigzag64(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

fn unzigzag64(encoded: u64) -> i64 {
    ((encoded >> 1) as i64) ^ -((encoded & 1) as i64)
}

/// Read a MINIMAL-form LEB128 varint from `bytes` at `*cursor`, bounded by
/// `limit` (exclusive). Rejects non-minimal encodings (`80 00`), >64-bit
/// values, and truncation.
fn read_uv(bytes: &[u8], cursor: &mut usize, limit: usize) -> Result<u64, CodecError> {
    let start = *cursor;
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        if *cursor >= limit {
            return Err(CodecError::Truncated);
        }
        let byte = bytes[*cursor];
        *cursor += 1;
        if shift > 63 {
            return Err(CodecError::VarintOverflow);
        }
        let group = (byte & 0x7F) as u64;
        if shift == 63 && group > 1 {
            return Err(CodecError::VarintOverflow);
        }
        value |= group << shift;
        if byte & 0x80 == 0 {
            if byte == 0 && *cursor - start > 1 {
                return Err(CodecError::NonMinimalVarint);
            }
            return Ok(value);
        }
        shift += 7;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Domain classifiers
// ─────────────────────────────────────────────────────────────────────────────

/// The split-eligibility classifier (§1.3): `Some(list)` iff `par` is a
/// PURE single-expr carrier (every other Par field empty/default) of an
/// `EListBody` whose LIST-level metadata is at ground defaults (`remainder`
/// None, `locally_free` empty, `connective_used` false). Element stability
/// is deliberately NOT required — the trie grammar splits such carriers
/// with per-element escape (R3F-2's «v»-survival requirement); on the wire
/// grammar the enclosing `eval_stable_par` check makes the elements stable.
fn split_carrier_list(par: &Par) -> Option<&EList> {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
        || !par.unforgeables.is_empty()
        || !par.locally_free.is_empty()
        || par.connective_used
    {
        return None;
    }
    match par.exprs.as_slice() {
        [expr] => match &expr.expr_instance {
            Some(ExprInstance::EListBody(list))
                if list.remainder.is_none()
                    && list.locally_free.is_empty()
                    && !list.connective_used =>
            {
                Some(list)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Ground Par builders shared by the decoder (the decode side of the
/// carrier bijection: ExprInstance-or-GPrivate ↔ ground Par, §1.1).
fn expr_carrier(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn gprivate_carrier(id: Vec<u8>) -> Par {
    Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }],
        ..Default::default()
    }
}

fn ground_elist_carrier(elements: Vec<Par>) -> Par {
    expr_carrier(ExprInstance::EListBody(EList {
        ps: elements,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

fn ground_etuple_carrier(elements: Vec<Par>) -> Par {
    expr_carrier(ExprInstance::ETupleBody(ETuple {
        ps: elements,
        locally_free: Vec::new(),
        connective_used: false,
    }))
}

fn epathmap_carrier(entries: Vec<Par>) -> Par {
    expr_carrier(ExprInstance::EPathmapBody(EPathMap::new(
        entries,
        Vec::new(),
        false,
        None,
    )))
}

// ─────────────────────────────────────────────────────────────────────────────
// The iterative encoder (explicit op + context stacks — no recursion)
// ─────────────────────────────────────────────────────────────────────────────

/// Detour contexts above the root buffer: an entry-path buffer, or a nested
/// region's collected entry paths (canonicalized — ascending + deduplicated —
/// via a PathMap zipper trie-walk on close; NO sort).
enum EncCtx {
    Buf(Vec<u8>),
    Region(Vec<Vec<u8>>),
}

enum EncOp<'p> {
    /// Emit one segment for `par`. `known_stable` short-circuits the domain
    /// walk inside proven-stable subtrees; `top_level` marks a top-level
    /// segment position (the only place the trie escape may appear).
    Segment {
        par: &'p Par,
        known_stable: bool,
        top_level: bool,
    },
    /// Open a buffered path for one region entry (entries of a `0x0D`
    /// region are stable by heredity — module doc).
    EntryPath { par: &'p Par },
    CloseEntryPath,
    CloseNestedRegion,
    EmitTerminator,
}

/// The iterative TRIE-key encoder. Total on ALL Pars (¬eval_stable nodes at
/// top-level segment positions take the `0x0F` escape arm); unlimited depth
/// (trie keys must build for every legal runtime value — module doc). Root
/// bytes accumulate in `root`; detours buffer entry paths / nested regions.
struct EncMachine<'p> {
    root: Vec<u8>,
    detours: Vec<EncCtx>,
    ops: Vec<EncOp<'p>>,
}

impl<'p> EncMachine<'p> {
    fn new() -> Self {
        EncMachine {
            root: Vec::new(),
            detours: Vec::new(),
            ops: Vec::new(),
        }
    }

    fn emit(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        match self.detours.last_mut() {
            Some(EncCtx::Buf(out)) => {
                out.extend_from_slice(bytes);
                Ok(())
            }
            Some(EncCtx::Region(_)) => {
                // Machine invariant: region contexts receive whole PATHS via
                // CloseEntryPath, never raw bytes.
                unreachable!("encoder invariant: raw emit into a region context")
            }
            None => {
                self.root.extend_from_slice(bytes);
                Ok(())
            }
        }
    }

    fn emit_byte(&mut self, byte: u8) -> Result<(), CodecError> {
        self.emit(&[byte])
    }

    fn emit_uv(&mut self, value: u64) -> Result<(), CodecError> {
        let mut scratch = Vec::with_capacity(10);
        write_uv(&mut scratch, value);
        self.emit(&scratch)
    }

    /// Schedule the §1.3 top-level path ops for `par` (split / bare / escape
    /// on the trie grammar). Total over ALL Pars — a ¬eval_stable node at a
    /// top-level segment position takes the `0x0F` escape arm.
    fn push_path_ops(&mut self, par: &'p Par, known_stable: bool) -> Result<(), CodecError> {
        let stable = known_stable || eval_stable_par(par);
        if let Some(list) = split_carrier_list(par) {
            // Split form: per-element segments + the terminator. LIFO push:
            // terminator first so it emits LAST.
            self.ops.push(EncOp::EmitTerminator);
            for element in list.ps.iter().rev() {
                self.ops.push(EncOp::Segment {
                    par: element,
                    known_stable: stable,
                    top_level: true,
                });
            }
        } else {
            // Bare form (a stable list carrier always splits above, so a
            // bare 0x0B segment is never emitted — the canonical-form rule
            // holds by construction).
            self.ops.push(EncOp::Segment {
                par,
                known_stable: stable,
                top_level: true,
            });
        }
        Ok(())
    }

    fn run(&mut self) -> Result<(), CodecError> {
        while let Some(op) = self.ops.pop() {
            match op {
                EncOp::Segment {
                    par,
                    known_stable,
                    top_level,
                } => self.encode_segment(par, known_stable, top_level)?,
                EncOp::EntryPath { par } => {
                    self.detours.push(EncCtx::Buf(Vec::new()));
                    self.ops.push(EncOp::CloseEntryPath);
                    // Entries under a 0x0D arm are stable by heredity (the
                    // arm is reachable only inside a stable subtree).
                    self.push_path_ops(par, true)?;
                }
                EncOp::CloseEntryPath => {
                    let Some(EncCtx::Buf(path)) = self.detours.pop() else {
                        unreachable!("encoder invariant: CloseEntryPath without a path buffer")
                    };
                    let Some(EncCtx::Region(paths)) = self.detours.last_mut() else {
                        unreachable!("encoder invariant: entry path outside a region context")
                    };
                    paths.push(path);
                }
                EncOp::CloseNestedRegion => {
                    let Some(EncCtx::Region(paths)) = self.detours.pop() else {
                        unreachable!("encoder invariant: CloseNestedRegion without a region")
                    };
                    // Canonical nested-region order (§2.2): ascending raw-byte
                    // path order, deduplicated — path-dedup IS value-dedup under
                    // an injective codec. Realized by a PathMap zipper trie-walk,
                    // NO sort: `FromIterator<&[u8]>` builds a `PathMap<()>` whose
                    // idempotent insertion dedups and whose read-zipper
                    // `to_next_val` depth-first traversal yields the entry paths
                    // in ascending byte-lex (= trie) order. `region.len()` equals
                    // the old `Σ (uv_len(|p|) + |p|)`, so the emitted
                    // `uv(|R|) ++ R` is byte-identical to the retired
                    // `Vec::sort()/dedup()` implementation.
                    use pathmap::zipper::{ZipperIteration, ZipperMoving};
                    use pathmap::PathMap;
                    let region_trie: PathMap<()> =
                        paths.iter().map(|path| path.as_slice()).collect();
                    let mut region = Vec::new();
                    let mut rz = region_trie.read_zipper();
                    while rz.to_next_val() {
                        let path = rz.path();
                        write_uv(&mut region, path.len() as u64);
                        region.extend_from_slice(path);
                    }
                    self.emit_uv(region.len() as u64)?;
                    self.emit(&region)?;
                }
                EncOp::EmitTerminator => self.emit_byte(tag::TERM)?,
            }
        }
        Ok(())
    }

    /// Emit one segment (`enc`, §1.2). Iterative: collection arms push
    /// child ops instead of recursing.
    fn encode_segment(
        &mut self,
        par: &'p Par,
        known_stable: bool,
        top_level: bool,
    ) -> Result<(), CodecError> {
        let stable = known_stable || eval_stable_par(par);
        if !stable {
            // The trie escape arm (`0x0F`) is legal ONLY at top-level segment
            // positions (hereditary rule): nested positions are inside stable
            // subtrees where `stable` always holds.
            debug_assert!(top_level, "escape below a top-level segment position");
            let prost_bytes = par.encode_to_vec();
            self.emit_byte(tag::ESCAPE)?;
            self.emit_uv(prost_bytes.len() as u64)?;
            return self.emit(&prost_bytes);
        }
        match (par.exprs.as_slice(), par.unforgeables.as_slice()) {
            ([], [unforgeable]) => {
                let Some(UnfInstance::GPrivateBody(private)) = &unforgeable.unf_instance else {
                    unreachable!("eval_stable unforgeable leaf must be GPrivate")
                };
                self.emit_byte(tag::GPRIVATE)?;
                self.emit_uv(private.id.len() as u64)?;
                self.emit(&private.id)
            }
            ([expr], []) => {
                let Some(instance) = &expr.expr_instance else {
                    unreachable!("eval_stable expr carrier must hold an instance")
                };
                match instance {
                    ExprInstance::GBool(false) => self.emit_byte(tag::GBOOL_FALSE),
                    ExprInstance::GBool(true) => self.emit_byte(tag::GBOOL_TRUE),
                    ExprInstance::GInt(value) => {
                        self.emit_byte(tag::GINT)?;
                        self.emit_uv(zigzag64(*value))
                    }
                    ExprInstance::GString(text) => {
                        self.emit_byte(tag::GSTRING)?;
                        self.emit_uv(text.len() as u64)?;
                        self.emit(text.as_bytes())
                    }
                    ExprInstance::GUri(uri) => {
                        self.emit_byte(tag::GURI)?;
                        self.emit_uv(uri.len() as u64)?;
                        self.emit(uri.as_bytes())
                    }
                    ExprInstance::GByteArray(bytes) => {
                        self.emit_byte(tag::GBYTE_ARRAY)?;
                        self.emit_uv(bytes.len() as u64)?;
                        self.emit(bytes)
                    }
                    ExprInstance::GDouble(bits) => {
                        self.emit_byte(tag::GDOUBLE)?;
                        self.emit(&bits.to_be_bytes())
                    }
                    ExprInstance::GBigInt(bytes) => {
                        self.emit_byte(tag::GBIG_INT)?;
                        self.emit_uv(bytes.len() as u64)?;
                        self.emit(bytes)
                    }
                    ExprInstance::GBigRat(rational) => {
                        self.emit_byte(tag::GBIG_RAT)?;
                        self.emit_uv(rational.numerator.len() as u64)?;
                        self.emit(&rational.numerator)?;
                        self.emit_uv(rational.denominator.len() as u64)?;
                        self.emit(&rational.denominator)
                    }
                    ExprInstance::GFixedPoint(fixed) => {
                        self.emit_byte(tag::GFIXED_POINT)?;
                        self.emit_uv(fixed.unscaled.len() as u64)?;
                        self.emit(&fixed.unscaled)?;
                        self.emit_uv(u64::from(fixed.scale))
                    }
                    ExprInstance::EListBody(list) => {
                        self.emit_byte(tag::ELIST)?;
                        self.emit_uv(list.ps.len() as u64)?;
                        for child in list.ps.iter().rev() {
                            self.ops.push(EncOp::Segment {
                                par: child,
                                known_stable: true,
                                top_level: false,
                            });
                        }
                        Ok(())
                    }
                    ExprInstance::ETupleBody(tuple) => {
                        self.emit_byte(tag::ETUPLE)?;
                        self.emit_uv(tuple.ps.len() as u64)?;
                        for child in tuple.ps.iter().rev() {
                            self.ops.push(EncOp::Segment {
                                par: child,
                                known_stable: true,
                                top_level: false,
                            });
                        }
                        Ok(())
                    }
                    ExprInstance::EPathmapBody(map) => {
                        self.emit_byte(tag::EPATHMAP)?;
                        self.detours.push(EncCtx::Region(Vec::new()));
                        self.ops.push(EncOp::CloseNestedRegion);
                        for entry in map.ps.iter().rev() {
                            self.ops.push(EncOp::EntryPath { par: entry });
                        }
                        Ok(())
                    }
                    _ => unreachable!("eval_stable expr outside the stable alphabet"),
                }
            }
            _ => unreachable!("eval_stable par outside the two carrier shapes"),
        }
    }

    fn into_buffer(self) -> Vec<u8> {
        debug_assert!(self.detours.is_empty(), "encoder finished with open detours");
        self.root
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public encode surface
// ─────────────────────────────────────────────────────────────────────────────

/// `path` on the TRIE grammar: TOTAL on all Pars (R3F-2) — ¬eval_stable
/// nodes at top-level segment positions take the `0x0F` escape arm;
/// unlimited depth; iterative; no panics. This is THE navigable trie key
/// the PathMap zippers index on (`create_pathmap_from_elements`).
pub fn encode_trie_path(par: &Par) -> Vec<u8> {
    let mut machine = EncMachine::new();
    machine
        .push_path_ops(par, false)
        .expect("trie path scheduling is total");
    machine.run().expect("trie path encoding is total");
    machine.into_buffer()
}

/// One TRIE-grammar segment (escape-capable, total) — the building block of
/// zipper `current_path` segments and the DFS differential expectations.
pub fn encode_trie_segment(par: &Par) -> Vec<u8> {
    let mut machine = EncMachine::new();
    machine.ops.push(EncOp::Segment {
        par,
        known_stable: false,
        top_level: true,
    });
    machine.run().expect("trie segment encoding is total");
    machine.into_buffer()
}

// ─────────────────────────────────────────────────────────────────────────────
// The iterative decoder (explicit frame stack — no recursion)
// ─────────────────────────────────────────────────────────────────────────────

enum DecFrame {
    /// An open `0x0B`/`0x0C` collection segment.
    Col {
        is_tuple: bool,
        remaining: u64,
        elems: Vec<Par>,
    },
    /// An open nested-map region (a `0x0D` segment payload). The top-level
    /// value-arm region codec is NOT part of this module — a ground map's
    /// value serialization is the U(m) key stream produced by a PathMap
    /// zipper walk over the intern trie, not a region parse.
    Region {
        region_end: usize,
        entries: Vec<Par>,
        /// Byte range of the previous path (order check anchor).
        prev_path: Option<(usize, usize)>,
    },
    /// An open path frame (a region frame's payload, or the whole top-level
    /// path input).
    Path {
        frame_end: usize,
        path_start: usize,
        segments: Vec<Par>,
    },
}

enum DecOutcome {
    Entry(Par),
}

struct DecMachine<'b> {
    bytes: &'b [u8],
    cursor: usize,
    depth: u32,
    /// Open Col/Region frame count — the trie escape is legal only at
    /// top-level segment positions, i.e. while this is 0.
    col_or_region_frames: u32,
    frames: Vec<DecFrame>,
    /// Innermost byte boundary stack: input length at the bottom, then one
    /// entry per open Region (region_end) and Path (frame_end).
    limits: Vec<usize>,
}

impl<'b> DecMachine<'b> {
    fn new(bytes: &'b [u8]) -> Self {
        DecMachine {
            bytes,
            cursor: 0,
            depth: 0,
            col_or_region_frames: 0,
            frames: Vec::new(),
            limits: vec![bytes.len()],
        }
    }

    fn limit(&self) -> usize {
        *self
            .limits
            .last()
            .expect("decoder invariant: the limit stack is never empty")
    }

    fn read_uv(&mut self) -> Result<u64, CodecError> {
        let limit = self.limit();
        read_uv(self.bytes, &mut self.cursor, limit)
    }

    fn take(&mut self, count: usize) -> Result<&'b [u8], CodecError> {
        let end = self
            .cursor
            .checked_add(count)
            .ok_or(CodecError::Truncated)?;
        if end > self.limit() {
            return Err(CodecError::Truncated);
        }
        let slice = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(slice)
    }

    fn enter_collection(&mut self) -> Result<(), CodecError> {
        if self.depth >= COLLECTION_DEPTH_LIMIT {
            return Err(CodecError::DepthLimitExceeded);
        }
        self.depth += 1;
        Ok(())
    }

    /// Decode one whole top-level path (the `decode_trie_path` driver).
    fn run_path(mut self) -> Result<Par, CodecError> {
        if self.bytes.is_empty() {
            return Err(CodecError::EmptyInput);
        }
        self.frames.push(DecFrame::Path {
            frame_end: self.bytes.len(),
            path_start: 0,
            segments: Vec::new(),
        });
        self.limits.push(self.bytes.len());
        let DecOutcome::Entry(par) = self.drive()?;
        Ok(par)
    }

    /// Read the next NESTED-region frame header (`uv(|path|)`) and open its
    /// Path frame. Caller guarantees `cursor < region_end`.
    fn open_next_region_frame(&mut self) -> Result<(), CodecError> {
        let frame_len = self.read_uv()?;
        if frame_len == 0 {
            return Err(CodecError::EmptyPathFrame);
        }
        let frame_len = usize::try_from(frame_len).map_err(|_| CodecError::Truncated)?;
        let frame_end = self
            .cursor
            .checked_add(frame_len)
            .ok_or(CodecError::RegionFrameOverrun)?;
        if frame_end > self.limit() {
            return Err(CodecError::RegionFrameOverrun);
        }
        self.frames.push(DecFrame::Path {
            frame_end,
            path_start: self.cursor,
            segments: Vec::new(),
        });
        self.limits.push(frame_end);
        Ok(())
    }

    /// The single decode loop.
    fn drive(&mut self) -> Result<DecOutcome, CodecError> {
        loop {
            match self.frames.last() {
                Some(DecFrame::Path { frame_end, .. }) => {
                    let frame_end = *frame_end;
                    if self.cursor == frame_end {
                        // Bare close (no terminator consumed).
                        if let Some(outcome) = self.close_path(false)? {
                            return Ok(outcome);
                        }
                        continue;
                    }
                    if self.bytes[self.cursor] == tag::TERM {
                        self.cursor += 1;
                        if self.cursor != frame_end {
                            return Err(CodecError::TerminatorMisplaced);
                        }
                        if let Some(outcome) = self.close_path(true)? {
                            return Ok(outcome);
                        }
                        continue;
                    }
                    self.parse_segment_start()?;
                }
                Some(DecFrame::Col { .. }) => {
                    // A child segment of an open collection. `0x00` is not a
                    // segment tag: at a child position it is a misplaced
                    // terminator.
                    if self.cursor >= self.limit() {
                        return Err(CodecError::Truncated);
                    }
                    if self.bytes[self.cursor] == tag::TERM {
                        return Err(CodecError::TerminatorMisplaced);
                    }
                    self.parse_segment_start()?;
                }
                Some(DecFrame::Region { .. }) => {
                    unreachable!("decoder invariant: a region always has an open path frame")
                }
                None => unreachable!("decoder invariant: drive on an empty frame stack"),
            }
        }
    }

    /// Parse one segment beginning at `cursor` (tag dispatch). Scalars
    /// deliver immediately; collections push frames.
    fn parse_segment_start(&mut self) -> Result<(), CodecError> {
        if self.cursor >= self.limit() {
            return Err(CodecError::Truncated);
        }
        let tag_byte = self.bytes[self.cursor];
        self.cursor += 1;
        match tag_byte {
            tag::GBOOL_FALSE => self.deliver(expr_carrier(ExprInstance::GBool(false))),
            tag::GBOOL_TRUE => self.deliver(expr_carrier(ExprInstance::GBool(true))),
            tag::GINT => {
                let encoded = self.read_uv()?;
                self.deliver(expr_carrier(ExprInstance::GInt(unzigzag64(encoded))))
            }
            tag::GSTRING | tag::GURI => {
                let len = self.read_uv()?;
                let len = usize::try_from(len).map_err(|_| CodecError::Truncated)?;
                let payload = self.take(len)?.to_vec();
                let text = String::from_utf8(payload).map_err(|_| CodecError::InvalidUtf8)?;
                let instance = if tag_byte == tag::GSTRING {
                    ExprInstance::GString(text)
                } else {
                    ExprInstance::GUri(text)
                };
                self.deliver(expr_carrier(instance))
            }
            tag::GBYTE_ARRAY | tag::GBIG_INT => {
                let len = self.read_uv()?;
                let len = usize::try_from(len).map_err(|_| CodecError::Truncated)?;
                let payload = self.take(len)?.to_vec();
                let instance = if tag_byte == tag::GBYTE_ARRAY {
                    ExprInstance::GByteArray(payload)
                } else {
                    ExprInstance::GBigInt(payload)
                };
                self.deliver(expr_carrier(instance))
            }
            tag::GDOUBLE => {
                let raw = self.take(8)?;
                let bits = u64::from_be_bytes(raw.try_into().expect("8-byte slice"));
                self.deliver(expr_carrier(ExprInstance::GDouble(bits)))
            }
            tag::GBIG_RAT => {
                let num_len = self.read_uv()?;
                let num_len = usize::try_from(num_len).map_err(|_| CodecError::Truncated)?;
                let numerator = self.take(num_len)?.to_vec();
                let den_len = self.read_uv()?;
                let den_len = usize::try_from(den_len).map_err(|_| CodecError::Truncated)?;
                let denominator = self.take(den_len)?.to_vec();
                self.deliver(expr_carrier(ExprInstance::GBigRat(GBigRational {
                    numerator,
                    denominator,
                })))
            }
            tag::GFIXED_POINT => {
                let unscaled_len = self.read_uv()?;
                let unscaled_len =
                    usize::try_from(unscaled_len).map_err(|_| CodecError::Truncated)?;
                let unscaled = self.take(unscaled_len)?.to_vec();
                let scale = self.read_uv()?;
                let scale = u32::try_from(scale).map_err(|_| CodecError::ScaleOutOfRange)?;
                self.deliver(expr_carrier(ExprInstance::GFixedPoint(GFixedPoint {
                    unscaled,
                    scale,
                })))
            }
            tag::ELIST | tag::ETUPLE => {
                let count = self.read_uv()?;
                self.enter_collection()?;
                let is_tuple = tag_byte == tag::ETUPLE;
                if count == 0 {
                    self.depth -= 1;
                    let par = if is_tuple {
                        ground_etuple_carrier(Vec::new())
                    } else {
                        ground_elist_carrier(Vec::new())
                    };
                    return self.deliver(par);
                }
                // Bomb-safe preallocation: every child costs ≥ 1 byte, so a
                // count beyond the remaining frame bytes is already doomed.
                let remaining = (self.limit() - self.cursor) as u64;
                if count > remaining {
                    return Err(CodecError::Truncated);
                }
                self.frames.push(DecFrame::Col {
                    is_tuple,
                    remaining: count,
                    elems: Vec::with_capacity(count as usize),
                });
                self.col_or_region_frames += 1;
                Ok(())
            }
            tag::EPATHMAP => {
                let region_len = self.read_uv()?;
                self.enter_collection()?;
                if region_len == 0 {
                    // R3F-1: the 0x0D arm admits R = ε — the canonical empty
                    // map. Top-level region non-emptiness does NOT recurse.
                    self.depth -= 1;
                    return self.deliver(epathmap_carrier(Vec::new()));
                }
                let region_len = usize::try_from(region_len).map_err(|_| CodecError::Truncated)?;
                let region_end = self
                    .cursor
                    .checked_add(region_len)
                    .ok_or(CodecError::Truncated)?;
                if region_end > self.limit() {
                    return Err(CodecError::Truncated);
                }
                self.frames.push(DecFrame::Region {
                    region_end,
                    entries: Vec::new(),
                    prev_path: None,
                });
                self.col_or_region_frames += 1;
                self.limits.push(region_end);
                self.open_next_region_frame()
            }
            tag::GPRIVATE => {
                let len = self.read_uv()?;
                let len = usize::try_from(len).map_err(|_| CodecError::Truncated)?;
                let id = self.take(len)?.to_vec();
                self.deliver(gprivate_carrier(id))
            }
            tag::ESCAPE => {
                // The escape arm is trie-only and legal ONLY at a top-level
                // segment position (hereditary rule): a nested position sits
                // inside a stable subtree, so any open Col/Region frame here
                // means the bytes are non-canonical.
                if self.col_or_region_frames > 0 {
                    return Err(CodecError::EscapeNested);
                }
                let len = self.read_uv()?;
                let len = usize::try_from(len).map_err(|_| CodecError::Truncated)?;
                let payload = self.take(len)?;
                let par = Par::decode(payload).map_err(|_| CodecError::EscapePayloadInvalid)?;
                // Canonicality of the payload (decode-accepts ≡ image): the
                // encoder writes canonical prost bytes of ¬eval_stable Pars
                // only.
                if par.encode_to_vec() != payload {
                    return Err(CodecError::EscapePayloadNonCanonical);
                }
                if eval_stable_par(&par) {
                    return Err(CodecError::EscapeOfStablePar);
                }
                self.deliver(par)
            }
            tag::TERM => {
                // Reached only via direct calls (drive() pre-filters the
                // path-frame terminator and the Col-frame misplacement).
                Err(CodecError::TerminatorMisplaced)
            }
            _ => Err(CodecError::ReservedTag(tag_byte)),
        }
    }

    /// Bubble a completed value up the frame stack.
    fn deliver(&mut self, mut par: Par) -> Result<(), CodecError> {
        loop {
            match self.frames.last_mut() {
                Some(DecFrame::Col {
                    is_tuple,
                    remaining,
                    elems,
                }) => {
                    elems.push(par);
                    *remaining -= 1;
                    if *remaining > 0 {
                        return Ok(());
                    }
                    let is_tuple = *is_tuple;
                    let Some(DecFrame::Col { elems, .. }) = self.frames.pop() else {
                        unreachable!("collection frame vanished")
                    };
                    self.col_or_region_frames -= 1;
                    self.depth -= 1;
                    par = if is_tuple {
                        ground_etuple_carrier(elems)
                    } else {
                        ground_elist_carrier(elems)
                    };
                }
                Some(DecFrame::Path { segments, .. }) => {
                    segments.push(par);
                    return Ok(());
                }
                Some(DecFrame::Region { .. }) => {
                    unreachable!("decoder invariant: a value delivered straight into a region")
                }
                None => unreachable!("decoder invariant: delivery on an empty frame stack"),
            }
        }
    }

    /// Close the innermost path frame (§1.3 decode rules 2-3) and hand the
    /// decoded ENTRY to the enclosing region (order-checked), or finish the
    /// run. Returns `Some(outcome)` when the bottom frame completed.
    fn close_path(&mut self, terminated: bool) -> Result<Option<DecOutcome>, CodecError> {
        let Some(DecFrame::Path {
            frame_end,
            path_start,
            mut segments,
        }) = self.frames.pop()
        else {
            unreachable!("close_path without a path frame")
        };
        self.limits.pop();
        let entry = if terminated {
            // Split form: the entry is the ground EList of the parsed
            // segments, k = 0 admitted (`path = 0x00`).
            ground_elist_carrier(segments)
        } else {
            match segments.len() {
                0 => return Err(CodecError::EmptyPath),
                1 => {
                    if self.bytes[path_start] == tag::ELIST {
                        // A bare 0x0B segment aliases the split form — the
                        // canonical-form rule rejects it.
                        return Err(CodecError::BareTopLevelList);
                    }
                    segments.pop().expect("one segment")
                }
                _ => return Err(CodecError::UnterminatedMultiSegment),
            }
        };
        match self.frames.last_mut() {
            None => Ok(Some(DecOutcome::Entry(entry))),
            Some(DecFrame::Region {
                region_end,
                entries,
                prev_path,
            }) => {
                // Per-region strict-ascending adjacent order — part of the ONE
                // parse pass, at every nesting level. A strict-REJECT CHECK
                // (the bytes must already be canonical): the encoder produced
                // nested-region paths in this order via the PathMap zipper
                // walk, so a well-formed nested region always passes; foreign
                // or malleated bytes are rejected here rather than silently
                // re-canonicalized.
                let current = (path_start, frame_end);
                let region_end = *region_end;
                if let Some((prev_start, prev_end)) = *prev_path {
                    let previous = &self.bytes[prev_start..prev_end];
                    let this = &self.bytes[current.0..current.1];
                    match previous.cmp(this) {
                        std::cmp::Ordering::Less => {}
                        std::cmp::Ordering::Equal => {
                            return Err(CodecError::DuplicatePathInRegion)
                        }
                        std::cmp::Ordering::Greater => {
                            return Err(CodecError::NonAscendingRegion)
                        }
                    }
                }
                *prev_path = Some(current);
                entries.push(entry);
                if self.cursor == region_end {
                    // Region complete: frames tiled exactly.
                    let Some(DecFrame::Region { entries, .. }) = self.frames.pop() else {
                        unreachable!("region frame vanished")
                    };
                    self.col_or_region_frames -= 1;
                    self.limits.pop();
                    self.depth -= 1;
                    let map_par = epathmap_carrier(entries);
                    self.deliver(map_par)?;
                    return Ok(None);
                }
                self.open_next_region_frame()?;
                Ok(None)
            }
            Some(_) => unreachable!("decoder invariant: a path frame above a non-region frame"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public decode surface
// ─────────────────────────────────────────────────────────────────────────────

/// Decode one top-level TRIE path — the inverse of [`encode_trie_path`]:
/// deterministic, left-to-right, iterative, depth-limited. The `0x0F` escape
/// arm is admitted at top-level segment positions (canonical-prost +
/// ¬eval_stable enforced on payloads); every non-canonical byte shape is
/// rejected (decode-accepts ≡ encoder-image).
pub fn decode_trie_path(path: &[u8]) -> Result<Par, CodecError> {
    DecMachine::new(path).run_path()
}

// ─────────────────────────────────────────────────────────────────────────────
// The segment-extent scanner (incremental, clonable — the DFS parser state)
// ─────────────────────────────────────────────────────────────────────────────

/// One byte-at-a-time step result of the [`SegmentScanner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStep {
    /// The byte was consumed; the segment is not yet complete.
    NeedMore,
    /// The byte was consumed and COMPLETED the segment.
    Complete,
    /// The byte cannot extend any valid segment (reserved tag, non-minimal
    /// varint, misplaced terminator, over-deep nesting).
    Invalid,
}

#[derive(Clone, Copy)]
enum VarintRole {
    /// Value discarded (GInt payload, GFixedPoint scale).
    Scalar,
    /// Value = a byte-payload length; on completion push `Bytes`.
    LenThenBytes,
    /// Value = a child count; on completion push `Kids`.
    Count,
}

#[derive(Clone)]
enum ScanFrame {
    Tag,
    Varint {
        role: VarintRole,
        value: u64,
        shift: u32,
        byte_count: u8,
    },
    Bytes {
        remaining: u64,
    },
    Kids {
        remaining: u64,
    },
}

/// The incremental "is this byte run a complete segment yet?" parser — the
/// parser state carried by [`collect_child_segments_codec`] in place of the
/// retired separator logic. Mirrors the codec grammar's EXTENT rules
/// exactly (`0x0D`/`0x0F` payloads are length-delimited and opaque for
/// extent purposes; minimal varints enforced; collection nesting capped at
/// [`COLLECTION_DEPTH_LIMIT`]).
#[derive(Clone)]
pub struct SegmentScanner {
    stack: Vec<ScanFrame>,
    kids_frames: u32,
}

impl Default for SegmentScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl SegmentScanner {
    pub fn new() -> Self {
        SegmentScanner {
            stack: vec![ScanFrame::Tag],
            kids_frames: 0,
        }
    }

    /// Feed one byte. After [`ScanStep::Complete`] or [`ScanStep::Invalid`]
    /// the scanner must not be fed again.
    pub fn feed(&mut self, byte: u8) -> ScanStep {
        if self.stack.len() > SCANNER_STACK_CEILING {
            return ScanStep::Invalid;
        }
        match self.stack.last_mut() {
            Some(ScanFrame::Tag) => {
                self.stack.pop();
                match byte {
                    tag::GBOOL_FALSE | tag::GBOOL_TRUE => self.bubble(),
                    tag::GINT => {
                        self.push_varint(VarintRole::Scalar);
                        ScanStep::NeedMore
                    }
                    tag::GSTRING
                    | tag::GURI
                    | tag::GBYTE_ARRAY
                    | tag::GBIG_INT
                    | tag::GPRIVATE
                    | tag::EPATHMAP
                    | tag::ESCAPE => {
                        self.push_varint(VarintRole::LenThenBytes);
                        ScanStep::NeedMore
                    }
                    tag::GDOUBLE => {
                        self.stack.push(ScanFrame::Bytes { remaining: 8 });
                        ScanStep::NeedMore
                    }
                    tag::GBIG_RAT => {
                        // Two length-prefixed fields; LIFO — numerator first.
                        self.push_varint(VarintRole::LenThenBytes);
                        self.push_varint(VarintRole::LenThenBytes);
                        ScanStep::NeedMore
                    }
                    tag::GFIXED_POINT => {
                        // unscaled bytes, then the scale varint.
                        self.push_varint(VarintRole::Scalar);
                        self.push_varint(VarintRole::LenThenBytes);
                        ScanStep::NeedMore
                    }
                    tag::ELIST | tag::ETUPLE => {
                        if self.kids_frames >= COLLECTION_DEPTH_LIMIT {
                            return ScanStep::Invalid;
                        }
                        self.push_varint(VarintRole::Count);
                        ScanStep::NeedMore
                    }
                    // 0x00 at a tag position (misplaced terminator) and the
                    // reserved range.
                    _ => ScanStep::Invalid,
                }
            }
            Some(ScanFrame::Varint {
                role,
                value,
                shift,
                byte_count,
            }) => {
                if *shift > 63 || (*shift == 63 && (byte & 0x7F) > 1) {
                    return ScanStep::Invalid;
                }
                let group = (byte & 0x7F) as u64;
                *value |= group << *shift;
                *shift += 7;
                *byte_count += 1;
                if byte & 0x80 != 0 {
                    return ScanStep::NeedMore;
                }
                if byte == 0 && *byte_count > 1 {
                    // Non-minimal trailing zero group.
                    return ScanStep::Invalid;
                }
                let role = *role;
                let value = *value;
                self.stack.pop();
                match role {
                    VarintRole::Scalar => self.bubble(),
                    VarintRole::LenThenBytes => {
                        if value > 0 {
                            self.stack.push(ScanFrame::Bytes { remaining: value });
                            ScanStep::NeedMore
                        } else {
                            self.bubble()
                        }
                    }
                    VarintRole::Count => {
                        if value > 0 {
                            self.kids_frames += 1;
                            self.stack.push(ScanFrame::Kids { remaining: value });
                            self.stack.push(ScanFrame::Tag);
                            ScanStep::NeedMore
                        } else {
                            self.bubble()
                        }
                    }
                }
            }
            Some(ScanFrame::Bytes { remaining }) => {
                *remaining -= 1;
                if *remaining > 0 {
                    return ScanStep::NeedMore;
                }
                self.stack.pop();
                self.bubble()
            }
            Some(ScanFrame::Kids { .. }) => {
                // Kids frames surface only through bubble(); a feed here
                // means the scanner was misused after completion.
                ScanStep::Invalid
            }
            None => ScanStep::Invalid,
        }
    }

    fn push_varint(&mut self, role: VarintRole) {
        self.stack.push(ScanFrame::Varint {
            role,
            value: 0,
            shift: 0,
            byte_count: 0,
        });
    }

    /// A node completed: consume pending child slots upward.
    fn bubble(&mut self) -> ScanStep {
        loop {
            match self.stack.last_mut() {
                None => return ScanStep::Complete,
                Some(ScanFrame::Kids { remaining }) => {
                    *remaining -= 1;
                    if *remaining == 0 {
                        self.stack.pop();
                        self.kids_frames -= 1;
                        continue;
                    }
                    self.stack.push(ScanFrame::Tag);
                    return ScanStep::NeedMore;
                }
                Some(_) => return ScanStep::NeedMore,
            }
        }
    }
}

/// Slice-based extent of the FIRST complete segment of `bytes`:
/// `Some(len)` when `bytes[..len]` is one complete segment, `None` when the
/// bytes are invalid or exhausted mid-segment. (The reference twin of the
/// DFS — both sides run the SAME scanner.)
pub fn segment_extent(bytes: &[u8]) -> Option<usize> {
    let mut scanner = SegmentScanner::new();
    for (index, byte) in bytes.iter().enumerate() {
        match scanner.feed(*byte) {
            ScanStep::Complete => return Some(index + 1),
            ScanStep::Invalid => return None,
            ScanStep::NeedMore => {}
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// The parser-state DFS (the collect_child_segments successor — side-by-side,
// wired to NOTHING; the existing 0xFF-separator queries are untouched)
// ─────────────────────────────────────────────────────────────────────────────

use pathmap::zipper::{Zipper, ZipperMoving};

/// Collect the distinct immediate child SEGMENTS below `prefix` in a trie
/// whose keys are CODEC paths (`path()` bytes — split or bare form), in
/// ascending raw byte-lexicographic segment order, truncated to `limit`.
///
/// "Segment complete" is when the codec grammar's extent parser finishes
/// (tag → header → known extent) — the parser-state successor to the
/// retired "separator seen" rule. The `0x00` branch at the prefix boundary
/// means "an entry ends here" and is EXCLUDED from child segments.
///
/// # Identity sketch (the pathmap_native_query proof obligations, re-proved
/// for the parser-state DFS)
///
/// * **Membership.** A byte string `s` is emitted iff the trie path
///   `prefix ∥ s` exists and `s` is one complete segment: the DFS descends
///   exactly the trie's child bytes, feeding each into the scanner, and
///   emits precisely on [`ScanStep::Complete`]. Because keys are codec
///   paths, every key strictly extending `prefix` continues with either the
///   terminator (excluded by the spec'd rule) or a complete next segment,
///   so the emitted set is exactly the child-segment set.
/// * **Distinctness.** Distinct segments are distinct trie paths; a DFS
///   visits each path once.
/// * **Order.** `enc` is PREFIX-FREE (v2 §1.4): once a byte run completes a
///   segment, no extension of it is a segment, so emitted segments are
///   pairwise non-prefix and plain ascending traversal (children visited in
///   ascending byte order at every node) yields ascending byte-lex segment
///   order DIRECTLY — the separator-first special case and the
///   `BitMask::clear_bit`-is-XOR workaround of the retired implementation
///   both dissolve.
/// * **Early stop.** Emission is already in final order, so the `limit`
///   truncation is sound.
pub fn collect_child_segments_codec(
    map: &RholangPathMap,
    prefix: &[u8],
    limit: Option<usize>,
) -> Vec<Vec<u8>> {
    let mut children: Vec<Vec<u8>> = Vec::new();
    if limit == Some(0) {
        return children;
    }

    let mut zipper = map.read_zipper_at_borrowed_path(prefix);
    // Levels hold (remaining sibling bytes, scanner state BEFORE any byte
    // of this level was consumed). The root level's scanner is fresh.
    let root_mask = zipper.child_mask();
    let mut levels: Vec<(pathmap::utils::ByteMaskIter, SegmentScanner)> =
        vec![(root_mask.iter(), SegmentScanner::new())];
    let mut segment: Vec<u8> = Vec::new();

    while let Some((mask_iter, scanner_base)) = levels.last_mut() {
        match mask_iter.next() {
            Some(byte) => {
                // At the ROOT level no byte has been descended yet, so
                // `segment` is empty — the borrow-clean equivalent of
                // `levels.len() == 1` (each descent pushes one byte to
                // `segment` and one frame to `levels` in lockstep).
                if segment.is_empty() && byte == tag::TERM {
                    // The prefix boundary's 0x00 branch: "an entry ends
                    // here" — excluded from child segments. (The scanner
                    // would reject it as a tag anyway; the explicit skip
                    // documents the spec'd rule.)
                    continue;
                }
                let mut scanner = scanner_base.clone();
                match scanner.feed(byte) {
                    ScanStep::Complete => {
                        segment.push(byte);
                        children.push(segment.clone());
                        segment.pop();
                        if limit == Some(children.len()) {
                            return children;
                        }
                        // Prefix-freeness: no extension of a complete
                        // segment is a segment — nothing to descend into.
                    }
                    ScanStep::NeedMore => {
                        zipper.descend_to_byte(byte);
                        segment.push(byte);
                        let mask = zipper.child_mask();
                        levels.push((mask.iter(), scanner));
                    }
                    ScanStep::Invalid => {
                        // Not a viable segment byte (foreign key material);
                        // skip the branch.
                    }
                }
            }
            None => {
                levels.pop();
                if segment.pop().is_some() {
                    zipper.ascend_byte();
                }
            }
        }
    }

    children
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests — the Job-A navigable trie-key codec: round-trip, canonicity,
// injectivity, nested map-of-map canonicalization (the PathMap zipper walk,
// no sort), the DFS differential, and adversarial rejection. The top-level
// value-arm region codec was never brought over, so there are no region /
// wire / fence tests here.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::var::VarInstance;
    use crate::rhoapi::{ESet, EVar, Send, Var};
    use proptest::prelude::*;

    // ── Par builders ─────────────────────────────────────────────────────────

    fn gbool(value: bool) -> Par {
        expr_carrier(ExprInstance::GBool(value))
    }
    fn gint(value: i64) -> Par {
        expr_carrier(ExprInstance::GInt(value))
    }
    fn gstring(text: &str) -> Par {
        expr_carrier(ExprInstance::GString(text.to_string()))
    }
    fn guri(text: &str) -> Par {
        expr_carrier(ExprInstance::GUri(text.to_string()))
    }
    fn gbytes(bytes: &[u8]) -> Par {
        expr_carrier(ExprInstance::GByteArray(bytes.to_vec()))
    }
    fn gdouble(value: f64) -> Par {
        expr_carrier(ExprInstance::GDouble(value.to_bits()))
    }
    fn gprivate(id: &[u8]) -> Par {
        gprivate_carrier(id.to_vec())
    }
    fn glist(elements: Vec<Par>) -> Par {
        ground_elist_carrier(elements)
    }
    fn gtuple(elements: Vec<Par>) -> Par {
        ground_etuple_carrier(elements)
    }
    fn gmap(entries: Vec<Par>) -> Par {
        epathmap_carrier(entries)
    }

    /// `wrappers` list levels around a GInt leaf, built ITERATIVELY.
    fn deep_list(wrappers: u32) -> Par {
        let mut par = gint(7);
        for _ in 0..wrappers {
            par = glist(vec![par]);
        }
        par
    }

    /// `wrappers` tuple levels around a GInt leaf (tuples never split, so
    /// every wrapper is a counted collection level on decode).
    fn deep_tuple(wrappers: u32) -> Par {
        let mut par = gint(7);
        for _ in 0..wrappers {
            par = gtuple(vec![par]);
        }
        par
    }

    /// Alternating tuple/map chain of `wrappers` genuine collection levels.
    fn deep_mixed(wrappers: u32) -> Par {
        let mut par = gint(7);
        for level in 0..wrappers {
            par = if level % 2 == 0 {
                gtuple(vec![par])
            } else {
                gmap(vec![par])
            };
        }
        par
    }

    // Unstable shapes (the ¬eval_stable side — escape-arm inputs on the trie).
    fn nil_par() -> Par {
        Par::default()
    }
    fn evar_par() -> Par {
        expr_carrier(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(0)),
            }),
        }))
    }
    fn eset_par() -> Par {
        expr_carrier(ExprInstance::ESetBody(ESet {
            ps: vec![gint(1)],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }))
    }
    fn multi_expr_par() -> Par {
        Par {
            exprs: vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(2)),
                },
            ],
            ..Default::default()
        }
    }
    fn send_par() -> Par {
        Par {
            sends: vec![Send::default()],
            ..Default::default()
        }
    }
    fn gprivate_plus_expr_par() -> Par {
        let mut par = gprivate(b"mix");
        par.exprs = vec![Expr {
            expr_instance: Some(ExprInstance::GInt(3)),
        }];
        par
    }
    fn connective_tagged_par() -> Par {
        let mut par = gint(9);
        par.connective_used = true;
        par
    }
    fn locally_free_tagged_par() -> Par {
        let mut par = gint(9);
        par.locally_free = vec![1];
        par
    }

    fn unstable_corpus() -> Vec<Par> {
        vec![
            nil_par(),
            evar_par(),
            eset_par(),
            multi_expr_par(),
            send_par(),
            gprivate_plus_expr_par(),
            connective_tagged_par(),
            locally_free_tagged_par(),
        ]
    }

    // ── proptest strategies ──────────────────────────────────────────────────

    fn stable_scalar() -> impl Strategy<Value = Par> {
        prop_oneof![
            any::<bool>().prop_map(gbool),
            any::<i64>().prop_map(gint),
            "[a-z0-9]{0,12}".prop_map(|s| gstring(&s)),
            "[a-z]{0,8}".prop_map(|s| guri(&s)),
            proptest::collection::vec(any::<u8>(), 0..12).prop_map(|b| gbytes(&b)),
            any::<u64>().prop_map(|bits| expr_carrier(ExprInstance::GDouble(bits))),
            proptest::collection::vec(any::<u8>(), 0..8)
                .prop_map(|b| expr_carrier(ExprInstance::GBigInt(b))),
            (
                proptest::collection::vec(any::<u8>(), 0..6),
                proptest::collection::vec(any::<u8>(), 0..6)
            )
                .prop_map(|(numerator, denominator)| expr_carrier(ExprInstance::GBigRat(
                    GBigRational {
                        numerator,
                        denominator,
                    }
                ))),
            (proptest::collection::vec(any::<u8>(), 0..6), any::<u32>()).prop_map(
                |(unscaled, scale)| expr_carrier(ExprInstance::GFixedPoint(GFixedPoint {
                    unscaled,
                    scale,
                }))
            ),
            proptest::collection::vec(any::<u8>(), 0..10).prop_map(gprivate_carrier),
        ]
    }

    fn stable_par() -> impl Strategy<Value = Par> {
        stable_scalar().prop_recursive(3, 24, 4, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..4).prop_map(ground_elist_carrier),
                proptest::collection::vec(inner.clone(), 0..4).prop_map(ground_etuple_carrier),
                proptest::collection::vec(inner, 0..3).prop_map(epathmap_carrier),
            ]
        })
    }

    fn unstable_par() -> impl Strategy<Value = Par> {
        prop_oneof![
            Just(nil_par()),
            Just(evar_par()),
            Just(eset_par()),
            Just(multi_expr_par()),
            Just(send_par()),
            Just(gprivate_plus_expr_par()),
            Just(connective_tagged_par()),
            Just(locally_free_tagged_par()),
        ]
    }

    fn any_par() -> impl Strategy<Value = Par> {
        prop_oneof![3 => stable_par(), 1 => unstable_par()]
    }

    // ── varint + zigzag primitives ───────────────────────────────────────────

    #[test]
    fn varint_minimal_roundtrip_and_rejections() {
        for value in [0u64, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            let mut out = Vec::new();
            write_uv(&mut out, value);
            let mut cursor = 0;
            assert_eq!(read_uv(&out, &mut cursor, out.len()), Ok(value));
            assert_eq!(cursor, out.len());
        }
        // Non-minimal: 0x80 0x00 encodes 0 in two bytes.
        let mut cursor = 0;
        assert_eq!(
            read_uv(&[0x80, 0x00], &mut cursor, 2),
            Err(CodecError::NonMinimalVarint)
        );
        // Truncated continuation.
        let mut cursor = 0;
        assert_eq!(read_uv(&[0x80], &mut cursor, 1), Err(CodecError::Truncated));
        // 10th-byte overflow (group > 1 at shift 63).
        let mut cursor = 0;
        let overflow = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02];
        assert_eq!(
            read_uv(&overflow, &mut cursor, overflow.len()),
            Err(CodecError::VarintOverflow)
        );
        // 11-byte varint.
        let mut cursor = 0;
        let eleven = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert_eq!(
            read_uv(&eleven, &mut cursor, eleven.len()),
            Err(CodecError::VarintOverflow)
        );
    }

    #[test]
    fn zigzag_is_a_bijection_at_the_extremes() {
        for value in [0i64, 1, -1, 2, -2, i64::MAX, i64::MIN, 4242, -4242] {
            assert_eq!(unzigzag64(zigzag64(value)), value);
        }
        assert_eq!(zigzag64(0), 0);
        assert_eq!(zigzag64(-1), 1);
        assert_eq!(zigzag64(1), 2);
    }

    // ── per-arm byte pins (the trie codec on eval_stable inputs) ──────────────

    #[test]
    fn per_arm_byte_pins() {
        assert_eq!(encode_trie_path(&gbool(false)), vec![0x01]);
        assert_eq!(encode_trie_path(&gbool(true)), vec![0x02]);
        assert_eq!(encode_trie_path(&gint(0)), vec![0x03, 0x00]);
        assert_eq!(encode_trie_path(&gint(-1)), vec![0x03, 0x01]);
        assert_eq!(encode_trie_path(&gint(1)), vec![0x03, 0x02]);
        assert_eq!(
            encode_trie_path(&gstring("hi")),
            vec![0x04, 0x02, b'h', b'i']
        );
        assert_eq!(encode_trie_path(&gstring("")), vec![0x04, 0x00]);
        assert_eq!(encode_trie_path(&guri("")), vec![0x05, 0x00]);
        assert_eq!(encode_trie_path(&gbytes(&[])), vec![0x06, 0x00]);
        let double_bytes = encode_trie_path(&gdouble(1.5));
        assert_eq!(double_bytes[0], 0x07);
        assert_eq!(&double_bytes[1..], 1.5f64.to_bits().to_be_bytes());
        // −0.0 and +0.0 are bit-distinct paths (Par == is u64 equality).
        assert_ne!(encode_trie_path(&gdouble(-0.0)), encode_trie_path(&gdouble(0.0)));
        // The empty-list entry dissolves to the one-byte terminator path.
        assert_eq!(encode_trie_path(&glist(vec![])), vec![0x00]);
        // k = 0 TUPLE is a bare segment — distinct from the k = 0 list.
        assert_eq!(encode_trie_path(&gtuple(vec![])), vec![0x0C, 0x00]);
        // Split multi-segment path.
        assert_eq!(
            encode_trie_path(&glist(vec![gstring("a"), gstring("b")])),
            vec![0x04, 0x01, b'a', 0x04, 0x01, b'b', 0x00]
        );
        // Nested empty list INSIDE a split (the 0x0B arm with k = 0).
        assert_eq!(
            encode_trie_path(&glist(vec![glist(vec![])])),
            vec![0x0B, 0x00, 0x00]
        );
        // GPrivate leaf.
        assert_eq!(encode_trie_path(&gprivate(&[0xAA])), vec![0x0E, 0x01, 0xAA]);
        // Nested empty map (the 0x0D arm admits R = ε).
        assert_eq!(encode_trie_path(&gmap(vec![])), vec![0x0D, 0x00]);
        // {| {||} |} — a map holding the empty map.
        assert_eq!(
            encode_trie_path(&gmap(vec![gmap(vec![])])),
            vec![0x0D, 0x03, 0x02, 0x0D, 0x00]
        );
    }

    #[test]
    fn roundtrip_every_scalar_arm() {
        let corpus = vec![
            gbool(false),
            gbool(true),
            gint(0),
            gint(i64::MIN),
            gint(i64::MAX),
            gstring(""),
            gstring("hello"),
            gstring("with\u{0}nul"),
            guri(""),
            guri("rho:io:stdout"),
            gbytes(&[]),
            gbytes(&[0x00, 0xFF, 0x0B]),
            gdouble(1.5),
            gdouble(-0.0),
            gdouble(f64::NAN),
            gdouble(f64::INFINITY),
            expr_carrier(ExprInstance::GBigInt(vec![])),
            expr_carrier(ExprInstance::GBigInt(vec![0x01, 0x00])),
            expr_carrier(ExprInstance::GBigRat(GBigRational {
                numerator: vec![0x02],
                denominator: vec![0x03],
            })),
            expr_carrier(ExprInstance::GBigRat(GBigRational {
                numerator: vec![],
                denominator: vec![],
            })),
            expr_carrier(ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![0x2A],
                scale: 2,
            })),
            expr_carrier(ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![],
                scale: u32::MAX,
            })),
            gprivate(&[]),
            gprivate(b"reflect-tag"),
            gtuple(vec![]),
            gtuple(vec![gint(1), gstring("x")]),
            glist(vec![]),
            glist(vec![glist(vec![]), gtuple(vec![])]),
            gmap(vec![]),
            gmap(vec![gmap(vec![])]),
            gmap(vec![gint(1), gint(2)]),
        ];
        for par in &corpus {
            let bytes = encode_trie_path(par);
            let decoded = decode_trie_path(&bytes).expect("trie decode");
            // Byte-level inversion (the stronger check — canonical fixed
            // point; == would ignore locally_free).
            assert_eq!(
                encode_trie_path(&decoded),
                bytes,
                "encode∘decode must be byte-identity on {par:?}"
            );
            assert!(eval_stable_par(&decoded), "decoded value must be stable");
        }
    }

    // ── top-level forms ──────────────────────────────────────────────────────

    #[test]
    fn top_level_split_vs_bare_vs_reject() {
        // Scalar vs singleton list: distinct paths, both decode.
        let scalar = gint(5);
        let singleton = glist(vec![gint(5)]);
        let scalar_path = encode_trie_path(&scalar);
        let singleton_path = encode_trie_path(&singleton);
        assert_eq!(scalar_path, vec![0x03, 0x0A]);
        assert_eq!(singleton_path, vec![0x03, 0x0A, 0x00]);
        assert_eq!(
            encode_trie_path(&decode_trie_path(&scalar_path).expect("d")),
            scalar_path
        );
        assert_eq!(
            encode_trie_path(&decode_trie_path(&singleton_path).expect("d")),
            singleton_path
        );
        // A bare 0x0B segment is rejected (the split form is canonical).
        assert_eq!(
            decode_trie_path(&[0x0B, 0x01, 0x01]),
            Err(CodecError::BareTopLevelList)
        );
        // Two bare segments without a terminator.
        assert_eq!(
            decode_trie_path(&[0x03, 0x02, 0x03, 0x04]),
            Err(CodecError::UnterminatedMultiSegment)
        );
        // The empty split form is the empty-list entry.
        let empty_list = decode_trie_path(&[0x00]).expect("empty split");
        assert_eq!(encode_trie_path(&empty_list), vec![0x00]);
    }

    // ── nested-map paths (the 0x0D arm inside a single key) ───────────────────

    #[test]
    fn nested_map_paths_roundtrip() {
        // Bare [0x0D, 0x00] accepts as the empty-map entry.
        let empty_map = decode_trie_path(&[0x0D, 0x00]).expect("empty map");
        assert_eq!(encode_trie_path(&empty_map), vec![0x0D, 0x00]);
        // {| {||}, {|{||}|} |} as a single-key path: the two nested-map entries
        // order by the prefix rule via the zipper walk.
        let pair = gmap(vec![gmap(vec![]), gmap(vec![gmap(vec![])])]);
        let bytes = encode_trie_path(&pair);
        assert_eq!(
            bytes,
            vec![0x0D, 0x09, 0x02, 0x0D, 0x00, 0x05, 0x0D, 0x03, 0x02, 0x0D, 0x00]
        );
        let decoded = decode_trie_path(&bytes).expect("decode nested pair");
        assert_eq!(encode_trie_path(&decoded), bytes);
    }

    // ── depth: the total-encode / bounded-decode asymmetry ────────────────────

    #[test]
    fn depth_limit_decode_rejects_beyond_32() {
        // At the limit: encodes and decodes and round-trips.
        let at_limit = deep_tuple(COLLECTION_DEPTH_LIMIT);
        let bytes = encode_trie_path(&at_limit);
        let decoded = decode_trie_path(&bytes).expect("depth-32 decodes");
        assert_eq!(encode_trie_path(&decoded), bytes);
        // Lists: the OUTER list is the split form (level 0), so 33 wrappers
        // carry exactly 32 counted levels and still decode.
        let list_at_limit = deep_list(COLLECTION_DEPTH_LIMIT + 1);
        assert!(decode_trie_path(&encode_trie_path(&list_at_limit)).is_ok());
        // Mixed tuple/map chain at the boundary round-trips.
        let mixed = deep_mixed(COLLECTION_DEPTH_LIMIT);
        let mixed_bytes = encode_trie_path(&mixed);
        assert_eq!(
            encode_trie_path(&decode_trie_path(&mixed_bytes).expect("mixed depth-32 decodes")),
            mixed_bytes
        );
        // The trie encoder is TOTAL beyond the limit (documented asymmetry):
        // encoding succeeds, decode rejects.
        let deep = deep_tuple(COLLECTION_DEPTH_LIMIT + 8);
        assert_eq!(
            decode_trie_path(&encode_trie_path(&deep)),
            Err(CodecError::DepthLimitExceeded)
        );
        // A hand-built depth bomb REJECTS on decode before materializing.
        let mut bomb = Vec::new();
        for _ in 0..(COLLECTION_DEPTH_LIMIT + 1) {
            bomb.extend_from_slice(&[0x0B, 0x01]);
        }
        bomb.extend_from_slice(&[0x03, 0x0E]);
        bomb.push(0x00); // split-form terminator (outermost list splits)
        assert_eq!(decode_trie_path(&bomb), Err(CodecError::DepthLimitExceeded));
    }

    // ── adversarial corpus (decode strict-reject) ─────────────────────────────

    #[test]
    fn adversarial_paths_reject() {
        assert_eq!(decode_trie_path(&[]), Err(CodecError::EmptyInput));
        assert_eq!(decode_trie_path(&[0x10]), Err(CodecError::ReservedTag(0x10)));
        assert_eq!(decode_trie_path(&[0xFF]), Err(CodecError::ReservedTag(0xFF)));
        // Non-minimal varint in a length position.
        assert_eq!(
            decode_trie_path(&[0x04, 0x80, 0x00]),
            Err(CodecError::NonMinimalVarint)
        );
        // Varint overflow in a scalar position.
        assert_eq!(
            decode_trie_path(&[0x03, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02]),
            Err(CodecError::VarintOverflow)
        );
        // Truncations: mid-varint, mid-payload, mid-fixed-width.
        assert_eq!(decode_trie_path(&[0x03]), Err(CodecError::Truncated));
        assert_eq!(decode_trie_path(&[0x04, 0x05, b'a']), Err(CodecError::Truncated));
        assert_eq!(decode_trie_path(&[0x07, 0x00]), Err(CodecError::Truncated));
        // Terminator misplacement: mid-stream at a boundary; child position.
        assert_eq!(
            decode_trie_path(&[0x00, 0x01]),
            Err(CodecError::TerminatorMisplaced)
        );
        assert_eq!(
            decode_trie_path(&[0x0C, 0x01, 0x00]),
            Err(CodecError::TerminatorMisplaced)
        );
        // Invalid UTF-8 in a string payload.
        assert_eq!(
            decode_trie_path(&[0x04, 0x01, 0xFF]),
            Err(CodecError::InvalidUtf8)
        );
        // GFixedPoint scale beyond u32.
        assert_eq!(
            decode_trie_path(&[0x0A, 0x00, 0x80, 0x80, 0x80, 0x80, 0x10]),
            Err(CodecError::ScaleOutOfRange)
        );
        // A NUL byte INSIDE a payload is fine — no in-band separator.
        let nul_string = decode_trie_path(&[0x04, 0x01, 0x00]).expect("NUL payload");
        assert_eq!(encode_trie_path(&nul_string), vec![0x04, 0x01, 0x00]);
    }

    /// The nested-region order CHECK fires DURING the parse at every level:
    /// a nested map whose inner frames descend (non-ascending) is rejected,
    /// never re-canonicalized on ingest.
    #[test]
    fn nested_region_order_check_rejects() {
        let inner_swapped = [
            0x0D, 0x06, // nested map, |R| = 6
            0x02, 0x03, 0x04, // frame: path [GInt 2]  (zigzag 4)
            0x02, 0x03, 0x02, // frame: path [GInt 1]  (zigzag 2) — descends
        ];
        assert_eq!(
            decode_trie_path(&inner_swapped),
            Err(CodecError::NonAscendingRegion)
        );
        // Byte-identical duplicate inner frames are rejected too.
        let inner_dup = [
            0x0D, 0x06, // nested map, |R| = 6
            0x02, 0x03, 0x02, // frame: path [GInt 1]
            0x02, 0x03, 0x02, // frame: path [GInt 1] — duplicate
        ];
        assert_eq!(
            decode_trie_path(&inner_dup),
            Err(CodecError::DuplicatePathInRegion)
        );
    }

    // ── the escape arm (0x0F — trie-only, total on ¬eval_stable) ──────────────

    #[test]
    fn escape_arm_roundtrip_ordering_and_rejections() {
        for par in unstable_corpus() {
            // Trie is total and round-trips through the escape arm.
            let trie_bytes = encode_trie_path(&par);
            assert_eq!(trie_bytes[0], tag::ESCAPE);
            let decoded = decode_trie_path(&trie_bytes).expect("trie decode");
            assert_eq!(
                decoded.encode_to_vec(),
                par.encode_to_vec(),
                "escape round-trip must preserve prost bytes"
            );
            assert_eq!(encode_trie_path(&decoded), trie_bytes);
        }
        // Escape sorts AFTER the GPrivate arm (0x0F > 0x0E) and
        // escape-vs-escape by prost bytes (byte order of the payload).
        let private_segment = encode_trie_segment(&gprivate(b"zzz"));
        let escape_segment = encode_trie_segment(&eset_par());
        assert!(private_segment < escape_segment);
        // Split carriers with unstable ELEMENTS split with per-element escape:
        // location components stay separate segments.
        let v_like = glist(vec![gstring("loc"), eset_par()]);
        let trie_bytes = encode_trie_path(&v_like);
        assert_eq!(&trie_bytes[..5], &[0x04, 0x03, b'l', b'o', b'c']);
        assert_eq!(trie_bytes[5], tag::ESCAPE);
        assert_eq!(*trie_bytes.last().expect("terminator"), 0x00);
        let decoded = decode_trie_path(&trie_bytes).expect("trie decode");
        assert_eq!(encode_trie_path(&decoded), trie_bytes);
        // An escape of a STABLE par aliases the stable arms — rejected.
        let stable_payload = gint(5).encode_to_vec();
        let mut aliasing = vec![tag::ESCAPE];
        write_uv(&mut aliasing, stable_payload.len() as u64);
        aliasing.extend_from_slice(&stable_payload);
        assert_eq!(
            decode_trie_path(&aliasing),
            Err(CodecError::EscapeOfStablePar)
        );
        // A non-canonical prost payload (doubled field, last-wins decode) is
        // rejected.
        let unstable_bytes = connective_tagged_par().encode_to_vec();
        let doubled: Vec<u8> = [unstable_bytes.clone(), unstable_bytes].concat();
        let mut non_canonical = vec![tag::ESCAPE];
        write_uv(&mut non_canonical, doubled.len() as u64);
        non_canonical.extend_from_slice(&doubled);
        assert_eq!(
            decode_trie_path(&non_canonical),
            Err(CodecError::EscapePayloadNonCanonical)
        );
        // The escape below a top-level segment position is hereditarily
        // forbidden.
        assert_eq!(
            decode_trie_path(&[0x0C, 0x01, 0x0F, 0x00]),
            Err(CodecError::EscapeNested)
        );
    }

    // ── Option C: nested-map canonicalization via the PathMap zipper walk ─────

    /// {|1,2|} ≡ {|2,1|} as a single trie key: permuting a ground map's
    /// entries never changes the key. The nested region is ordered by the
    /// zipper trie-walk (no sort), and the result is deterministic bytes.
    #[test]
    fn map_key_is_entry_order_insensitive() {
        let forward = gmap(vec![gint(1), gint(2)]);
        let backward = gmap(vec![gint(2), gint(1)]);
        let bytes = encode_trie_path(&forward);
        assert_eq!(bytes, vec![0x0D, 0x06, 0x02, 0x03, 0x02, 0x02, 0x03, 0x04]);
        assert_eq!(encode_trie_path(&backward), bytes);
        // Duplicated entries collapse (idempotent trie insertion = dedup).
        let with_dup = gmap(vec![gint(1), gint(2), gint(1)]);
        assert_eq!(encode_trie_path(&with_dup), bytes);
    }

    /// {|{|1,2|}|} ≡ {|{|2,1|}|} — the nested map-of-map twin: canonicalization
    /// recurses bottom-up through the zipper walk at every level.
    #[test]
    fn nested_map_of_map_canonicity() {
        let forward = gmap(vec![gmap(vec![gint(1), gint(2)])]);
        let backward = gmap(vec![gmap(vec![gint(2), gint(1)])]);
        let bytes = encode_trie_path(&forward);
        assert_eq!(
            bytes,
            vec![0x0D, 0x09, 0x08, 0x0D, 0x06, 0x02, 0x03, 0x02, 0x02, 0x03, 0x04]
        );
        assert_eq!(encode_trie_path(&backward), bytes);
        let decoded = decode_trie_path(&bytes).expect("decode nested map-of-map");
        assert_eq!(encode_trie_path(&decoded), bytes);
    }

    /// Injectivity on the ground domain: distinct canonical values encode to
    /// distinct trie keys (so distinct entries never collide in the trie).
    #[test]
    fn distinct_pars_have_distinct_keys() {
        let corpus = vec![
            gbool(false),
            gbool(true),
            gint(0),
            gint(1),
            gint(-1),
            gstring(""),
            gstring("a"),
            guri("a"),
            gbytes(&[0x61]),
            gprivate(b"a"),
            gtuple(vec![]),
            gtuple(vec![gint(1)]),
            glist(vec![]),
            glist(vec![gint(1)]),
            gmap(vec![]),
            gmap(vec![gint(1)]),
            gmap(vec![gint(1), gint(2)]),
        ];
        let mut keys: Vec<Vec<u8>> = corpus.iter().map(encode_trie_path).collect();
        let count = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), count, "distinct ground pars must have distinct keys");
    }

    // ── the parser-state DFS differential ─────────────────────────────────────

    /// Reference twin of the DFS: full scan + the SAME extent scanner.
    fn reference_child_segments(map: &RholangPathMap, prefix: &[u8]) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        for (key, _) in map.iter() {
            if key.starts_with(prefix) && key.len() > prefix.len() {
                let remaining = &key[prefix.len()..];
                if remaining[0] == tag::TERM {
                    continue; // an entry ends here — excluded
                }
                if let Some(extent) = segment_extent(remaining) {
                    out.push(remaining[..extent].to_vec());
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn codec_trie(entries: &[Par]) -> RholangPathMap {
        let mut map = RholangPathMap::new();
        for entry in entries {
            map.insert(encode_trie_path(entry), entry.clone());
        }
        map
    }

    #[test]
    fn dfs_differential_on_constructed_tries() {
        // A trie mixing split forms sharing first segments, bare scalars that
        // are prefixes of splits, escapes, and nested collections.
        let entries = vec![
            glist(vec![gstring("a"), gstring("x")]),
            glist(vec![gstring("a"), gstring("y")]),
            glist(vec![gstring("a")]),
            glist(vec![gstring("ab")]),
            glist(vec![gstring("b"), gint(1), gint(2)]),
            gstring("a"), // bare — a byte-prefix of the splits above
            gint(-7),
            gtuple(vec![gint(1)]),
            gmap(vec![gint(1), gstring("m")]),
            glist(vec![]),
            eset_par(),                            // bare escape
            glist(vec![gstring("a"), eset_par()]), // split with escape elem
        ];
        let map = codec_trie(&entries);

        // Prefixes: root, each first segment, a two-segment prefix.
        let mut prefixes: Vec<Vec<u8>> = vec![Vec::new()];
        prefixes.push(encode_trie_segment(&gstring("a")));
        prefixes.push(encode_trie_segment(&gstring("b")));
        let mut two_seg = encode_trie_segment(&gstring("a"));
        two_seg.extend(encode_trie_segment(&gstring("x")));
        prefixes.push(two_seg);
        // A prefix that exists only as a full entry (children = ∅).
        prefixes.push(encode_trie_path(&gint(-7)));

        for prefix in &prefixes {
            let expected = reference_child_segments(&map, prefix);
            let actual = collect_child_segments_codec(&map, prefix, None);
            assert_eq!(
                actual, expected,
                "DFS/reference divergence below prefix {prefix:02X?}"
            );
            // The limit truncation preserves the final order.
            for limit in 0..=expected.len() {
                assert_eq!(
                    collect_child_segments_codec(&map, prefix, Some(limit)),
                    expected[..limit].to_vec(),
                    "limit {limit} truncation below prefix {prefix:02X?}"
                );
            }
        }

        // The terminator branch is excluded: below the "a" segment the key for
        // the split entry ["a"] ends (0x00 branch) — child segments must
        // contain only the REAL next segments.
        let seg_a = encode_trie_segment(&gstring("a"));
        let children = collect_child_segments_codec(&map, &seg_a, None);
        assert!(children.iter().all(|segment| segment[0] != tag::TERM));
        // "x", "y" and the escape element are the children (ascending).
        let seg_x = encode_trie_segment(&gstring("x"));
        let seg_y = encode_trie_segment(&gstring("y"));
        let seg_escape = encode_trie_segment(&eset_par());
        assert_eq!(children, vec![seg_x, seg_y, seg_escape]);
    }

    // ── properties ────────────────────────────────────────────────────────────

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Trie round-trip: decode is total on the encoder image, decoded
        /// values are eval_stable, and encode∘decode reaches the canonical
        /// fixed point in one step.
        #[test]
        fn prop_trie_roundtrip(par in stable_par()) {
            let bytes = encode_trie_path(&par);
            let decoded = decode_trie_path(&bytes).expect("encoder image must decode");
            prop_assert!(eval_stable_par(&decoded));
            prop_assert_eq!(encode_trie_path(&decoded), bytes);
        }

        /// The trie codec is total on ALL Pars: a ¬eval_stable par takes the
        /// 0x0F escape arm, and the image always decodes back to itself.
        #[test]
        fn prop_trie_total_over_all_pars(par in any_par()) {
            let bytes = encode_trie_path(&par);
            if !eval_stable_par(&par) {
                prop_assert_eq!(bytes[0], tag::ESCAPE);
            }
            let decoded = decode_trie_path(&bytes).expect("trie decode is total on the image");
            prop_assert_eq!(encode_trie_path(&decoded), bytes);
        }

        /// Prefix-freeness of `enc` (§1.4): among distinct segments neither is
        /// a proper prefix of the other.
        #[test]
        fn prop_segments_are_prefix_free(a in stable_par(), b in stable_par()) {
            let seg_a = encode_trie_segment(&a);
            let seg_b = encode_trie_segment(&b);
            if seg_a != seg_b {
                prop_assert!(!seg_a.starts_with(&seg_b));
                prop_assert!(!seg_b.starts_with(&seg_a));
            }
        }

        /// Size property: |path(p)| ≤ |prost entry bytes| on the stable domain
        /// (the codec keys carry no protobuf field-tag overhead).
        #[test]
        fn prop_path_never_exceeds_prost_entry(par in stable_par()) {
            let bytes = encode_trie_path(&par);
            prop_assert!(
                bytes.len() <= par.encoded_len(),
                "path {} bytes vs prost {} bytes",
                bytes.len(),
                par.encoded_len()
            );
        }

        /// PathMap trie iteration over the encoded paths equals ascending
        /// raw-byte order — THE property the U(m) walk and the nested-region
        /// zipper walk both rely on.
        #[test]
        fn prop_trie_iteration_is_ascending_byte_order(
            pars in proptest::collection::vec(stable_par(), 1..12)
        ) {
            let mut paths: Vec<Vec<u8>> =
                pars.iter().map(encode_trie_path).collect();
            paths.sort();
            paths.dedup();
            let map: pathmap::PathMap<u64> = paths
                .iter()
                .enumerate()
                .map(|(index, path)| (path.clone(), index as u64))
                .collect();
            let iterated: Vec<Vec<u8>> = map.iter().map(|(key, _)| key.to_vec()).collect();
            prop_assert_eq!(iterated, paths);
        }

        /// Option C at the codec level: permuting a nested map's entries never
        /// changes the encoded path (zipper-walk canonicalization, no sort).
        #[test]
        fn prop_nested_map_order_insensitive(
            entries in proptest::collection::vec(stable_par(), 0..5)
        ) {
            let forward = gmap(entries.clone());
            let mut reversed_entries = entries;
            reversed_entries.reverse();
            let reversed = gmap(reversed_entries);
            prop_assert_eq!(encode_trie_path(&forward), encode_trie_path(&reversed));
        }

        /// Truncation property: every strict prefix of a BARE path rejects.
        #[test]
        fn prop_bare_path_prefixes_reject(par in stable_scalar()) {
            let bytes = encode_trie_path(&par);
            if split_carrier_list(&par).is_none() {
                for cut in 0..bytes.len() {
                    prop_assert!(decode_trie_path(&bytes[..cut]).is_err());
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        /// DFS ≡ reference over generated tries and generated prefixes.
        #[test]
        fn prop_dfs_matches_reference(
            entries in proptest::collection::vec(any_par(), 1..10),
            prefix_pick in any::<proptest::sample::Index>(),
        ) {
            let map = codec_trie(&entries);
            let root: Vec<u8> = Vec::new();
            prop_assert_eq!(
                collect_child_segments_codec(&map, &root, None),
                reference_child_segments(&map, &root)
            );
            let entry = &entries[prefix_pick.index(entries.len())];
            let path = encode_trie_path(entry);
            if let Some(extent) = segment_extent(&path) {
                let prefix = &path[..extent];
                prop_assert_eq!(
                    collect_child_segments_codec(&map, prefix, None),
                    reference_child_segments(&map, prefix)
                );
            }
        }
    }
}
