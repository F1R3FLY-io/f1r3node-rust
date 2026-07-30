//! EPathMap fix P4.3 — SPLICED EVENT-HASH SERIALIZATION (amendment PM-1).
//!
//! Event hashes cover `bincode(vec![channel_hash_bytes, bincode(datum),
//! bincode(persist)])` (produce) and the per-pattern / continuation bincode
//! legs (consume) — see `rspace_plus_plus::…::stable_hash_provider`. The
//! datum leg re-serializes the WHOLE `ListParWithRandom` per produce, which
//! is size-proportional in the (potentially huge) EPathMap payload. A manual
//! `Serialize` impl cannot raw-splice cached bytes into a serde stream
//! (`serialize_bytes` length-prefixes; serde has no raw-bytes primitive), so
//! this module is the PM-1 HAND-ROLLED emitter for exactly the container
//! spine `datum → Vec<Par> → Par → Expr → Option<ExprInstance> → variant →
//! EPathMap`: it reproduces the bincode 1.3.3 legacy-default layout
//! byte-for-byte, SPLICES `InternedEPathMap.serde_bytes` at filled-cell
//! `EPathMap` nodes (populating that P1 `OnceLock` lazily, here), and uses
//! black-box `bincode::serialize` for every interned-map-free subtree —
//! sound because bincode 1.3.3's encoding is COMPOSITIONAL:
//! `serialize(&vec) = u64-LE len ++ concat(elem encodings)` and a struct is
//! the plain concatenation of its field encodings.
//!
//! FULL LAYOUT RULE SET (bincode 1.3.3 `bincode::serialize` defaults —
//! fixint, little-endian, the exact config the direct path uses):
//!   * `Vec<T>` / `BTreeMap<K, V>` / bytes / `String`: u64-LE length, then
//!     the elements (map entries as key ++ value, in order; `String` as its
//!     UTF-8 bytes);
//!   * `Option<T>`: ONE tag byte (0 = `None`, 1 = `Some`), then `T`;
//!   * enums: u32-LE variant index in DECLARATION order, then the payload;
//!   * structs: fields concatenated in DECLARATION order (names never hit
//!     the wire);
//!   * integers: fixint LE (`i32`/`u32` 4 bytes, `i64`/`u64` 8 bytes);
//!     `bool`: one byte;
//!   * `locally_free`: EVERY `.rhoapi` message serializes it as EMPTY bytes
//!     (`serialize_as_empty_bytes` → `serialize_bytes(&[])` → u64-LE 0) —
//!     the serialize-only normalization models/build.rs injects; the
//!     emitter honors it at EVERY hand-walked level, and the black-box
//!     fallback honors it via the derives.
//!
//! DISPATCH: the intern-aware path is taken only when the value CONTAINS a
//! filled-cell `EPathMap` (`EPathMap::interned_handle()` — a read-only cell
//! peek that never forces an intern); otherwise the TRAMPOLINED direct path
//! (`ColdStoreEncode::cold_encode`) runs, byte-identically and with zero new
//! work. The contains-scan short-circuits at filled cells and descends
//! unfilled maps (a filled inner map inside an unfilled outer map still
//! splices).
//!
//! ★ WORK ITEM #124 — WHY THE DIRECT PATH IS `cold_encode` AND NOT
//! `bincode::serialize`. The derived `Serialize` recurses once per term level
//! and was measured at **3,040 B/level debug, 160 B/level release** by
//! `casper/tests/event_hash_leg_depth_probe.rs`, i.e. a deep enough datum
//! overflows a node worker's stack while computing an EVENT HASH — during
//! `hash_produce`, after the deploy has been accepted. `cold_encode` is the
//! single-walk trampolined encoder from `rust::rholang::wire_encode`, flat in
//! native stack, and **byte-identical by contract**.
//!
//! ⚠ Byte identity here is a CONSENSUS obligation, not a nicety: these bytes
//! are hashed into the block Merkle log. It is discharged by
//! `models/tests/event_hash_leg_cold_encode_identity.rs`, which asks the
//! question at this module's own entry points, over every `ExprInstance` and
//! `ConnectiveInstance` arm lifted into all three root types, over `EPathMap`
//! shapes whose intern cell is UNFILLED (the class that reaches this branch),
//! to depth 4,096, under proptest, and with an executed demonstration that the
//! differential can go red.
//!
//! ⚠ THIS IS NOT THE WHOLE OF #124. Two Θ(depth) native-stack recursions
//! remain in this file and are deliberately NOT addressed by that conversion:
//!
//!   1. the `contains_*` SCAN below, which runs on EVERY event hash — including
//!      every one that then takes the flat `cold_encode` path — and must
//!      descend the entire term to prove the ABSENCE of a filled cell; and
//!   2. the hand-written `emit_*` spine, which is what runs once the scan says
//!      `true`.
//!
//! Both are measured; see the probe's `*-spliced` rows and the report in
//! `docs/design/stack-safety/`. Converting them is a separate change with its
//! own correctness argument, and the ruling recorded at `00ff9187` was not to
//! rewrite the spliced emitter as part of the channel-leg work.
//!
//! SPLICE UNIT: `InternedEPathMap.serde_bytes` = `bincode::serialize` of the
//! source `EPathMap` — the wrapper's derived serde impl (P3) already skips
//! the cell and blanks `locally_free` at every level, so the cached bytes
//! ARE the direct path's bytes for that subtree. The cache is sound to
//! share across the intern family: the store is content-addressed by
//! canonical prost bytes, which determine every field value, which
//! determine the serde bytes.
//!
//! GATES (risk R7): the P0 event-hash goldens re-asserted with filled cells
//! (`epathmap_canonical_fixtures`), the spliced-vs-direct proptest
//! (`epathmap_spliced_event_bytes` — arbitrary map-bearing trees, cells
//! force-filled), and the replay-equivalence test over an EPathMap-heavy
//! program (rholang).

use serde::Serialize;

use crate::rust::rholang::wire_encode::ColdStoreEncode;

use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::{
    BindPattern, Bundle, Connective, EList, EMap, EMatches, EMethod, EPathMap, ETuple, ESet,
    EZipper, Expr, If, KeyValuePair, ListParWithRandom, Match, MatchCase, New, Par, ParWithRandom,
    Receive, ReceiveBind, Send, TaggedContinuation,
};


// ─────────────────────────────────────────────────────────────────────────────
// Public entry points (the three event-hash legs)
// ─────────────────────────────────────────────────────────────────────────────

/// bincode-of-`ListParWithRandom` for `hash_produce`'s datum leg —
/// intern-aware when the datum contains a filled-cell EPathMap, the
/// TRAMPOLINED direct path otherwise.
pub fn event_hash_bytes_list_par_with_random(datum: &ListParWithRandom) -> Vec<u8> {
    if !datum.pars.iter().any(contains_par) {
        return datum.cold_encode();
    }
    let mut out = Vec::new();
    emit_vec(&datum.pars, contains_par, emit_par, &mut out);
    emit_bytes(&datum.random_state, &mut out);
    out
}

/// bincode-of-`BindPattern` for `hash_consume`'s per-pattern leg.
pub fn event_hash_bytes_bind_pattern(pattern: &BindPattern) -> Vec<u8> {
    if !pattern.patterns.iter().any(contains_par) {
        return pattern.cold_encode();
    }
    let mut out = Vec::new();
    emit_vec(&pattern.patterns, contains_par, emit_par, &mut out);
    emit_black_box_option(&pattern.remainder, &mut out); // Var: map-free
    emit_i32(pattern.free_count, &mut out);
    out
}

/// bincode-of-`TaggedContinuation` for `hash_consume`'s continuation leg.
pub fn event_hash_bytes_tagged_continuation(continuation: &TaggedContinuation) -> Vec<u8> {
    if !contains_tagged_continuation(continuation) {
        return continuation.cold_encode();
    }
    let mut out = Vec::new();
    // Declaration order: `guard` (Option<Par>), then `tagged_cont`.
    emit_option(&continuation.guard, contains_par, emit_par, &mut out);
    match &continuation.tagged_cont {
        None => out.push(0),
        Some(tc) => {
            out.push(1);
            match tc {
                TaggedCont::ParBody(pwr) => {
                    emit_u32(0, &mut out); // variant 0 in declaration order
                    emit_par_with_random(pwr, &mut out);
                }
                TaggedCont::ScalaBodyRef(body_ref) => {
                    emit_u32(1, &mut out);
                    out.extend_from_slice(&body_ref.to_le_bytes());
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Primitive emitters (the layout rule set)
// ─────────────────────────────────────────────────────────────────────────────

/// The direct path: black-box bincode of a whole (interned-map-free) value.
fn direct<T: Serialize>(value: &T) -> Vec<u8> {
    bincode::serialize(value).expect("spliced_event_bytes: bincode serialization must not fail")
}

/// Append the black-box bincode of a subtree (compositionality: a field's
/// encoding inside a struct/seq IS its standalone encoding).
fn emit_black_box<T: Serialize>(value: &T, out: &mut Vec<u8>) {
    out.extend_from_slice(&direct(value));
}

fn emit_u64(value: u64, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_u32(value: u32, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_i32(value: i32, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_i64(value: i64, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_bool(value: bool, out: &mut Vec<u8>) { out.push(value as u8); }

/// Length-prefixed raw bytes (`Vec<u8>` fields and `String` payloads share
/// this shape; a `String` is its UTF-8 bytes).
fn emit_bytes(value: &[u8], out: &mut Vec<u8>) {
    emit_u64(value.len() as u64, out);
    out.extend_from_slice(value);
}

fn emit_string(value: &str, out: &mut Vec<u8>) { emit_bytes(value.as_bytes(), out); }

/// The serialize-only `locally_free` normalization: EMPTY bytes at every
/// hand-walked level (u64-LE 0), regardless of content.
fn emit_locally_free_as_empty(out: &mut Vec<u8>) { emit_u64(0, out); }

/// A `Vec<T>` whose elements MAY contain interned maps: u64-LE length, then
/// per element either the hand walk (contains) or the black box.
fn emit_vec<T: Serialize>(
    items: &[T],
    contains: fn(&T) -> bool,
    emit: fn(&T, &mut Vec<u8>),
    out: &mut Vec<u8>,
) {
    emit_u64(items.len() as u64, out);
    for item in items {
        if contains(item) {
            emit(item, out);
        } else {
            emit_black_box(item, out);
        }
    }
}

/// An `Option<T>` whose payload MAY contain interned maps: one tag byte,
/// then the payload (hand walk or black box).
fn emit_option<T: Serialize>(
    value: &Option<T>,
    contains: fn(&T) -> bool,
    emit: fn(&T, &mut Vec<u8>),
    out: &mut Vec<u8>,
) {
    match value {
        None => out.push(0),
        Some(inner) => {
            out.push(1);
            if contains(inner) {
                emit(inner, out);
            } else {
                emit_black_box(inner, out);
            }
        }
    }
}

/// An `Option<T>` that can NEVER contain a map (e.g. `Var`): tag byte +
/// black box.
fn emit_black_box_option<T: Serialize>(value: &Option<T>, out: &mut Vec<u8>) {
    match value {
        None => out.push(0),
        Some(inner) => {
            out.push(1);
            emit_black_box(inner, out);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The contains-scan (short-circuits at filled cells; descends unfilled maps)
// ─────────────────────────────────────────────────────────────────────────────

fn contains_epathmap(map: &EPathMap) -> bool {
    map.interned_handle().is_some() || map.ps().iter().any(contains_par)
}

/// Initial worklist reservation for [`contains_par`]. Sized so that the common
/// shallow term never reallocates; deep ones grow it geometrically as usual.
const CONTAINS_WORKLIST_CAPACITY: usize = 32;

/// ★★ WORK ITEM #124 — **THE GUARD SCAN, AS AN EXPLICIT WORKLIST.**
///
/// # Why this one is not optional
///
/// This predicate runs on **every event hash**, before either emitter, and when
/// it answers `false` — the 95.43 % production case — it has had to descend the
/// *entire* term to prove the absence of a filled cell. So while
/// `event_hash_bytes_*` now returns the depth-flat `cold_encode()` on that
/// branch, the *guard* was still Θ(depth) in native stack and the leg as a whole
/// still overflowed. Measured on `casper/tests/event_hash_leg_depth_probe.rs`:
///
/// | leg min-stack slope, B/level | debug | release |
/// |---|---|---|
/// | before any conversion (`contains_par` + derived `Serialize`) | 3,040 | 160 |
/// | `cold_encode` only — **this scan is what is left** | 960 | 128 |
/// | both converted | **0** | **0** |
///
/// ★ The middle row is the point, and it is why converting the encoder alone was
/// not the repair: it bought 68 % of the debug slope and 20 % of the release
/// slope while leaving a leg still unbounded in term depth. A guard that costs
/// more stack than the operation it guards is still a stack-overflow bug — in
/// release the guard was, at 128 of the original 160 B/level, **80 % of the
/// whole defect**.
///
/// # The traversal
///
/// A LIFO worklist of `&Par`. Every type in the old mutually-recursive SCC
/// (`Send`, `Receive`, `ReceiveBind`, `New`, `Match`, `MatchCase`, `If`,
/// `Bundle`, `Connective`, `KeyValuePair`, `Expr`) reaches its own children as
/// `Par` or `Option<Par>` and never nests inside itself, so ONE stack of `&Par`
/// covers the whole term: each pop enumerates that `Par`'s children through
/// exactly one level of indirection and pushes them. No frame is added per level.
///
/// ⚠ `EPathMap` and `EZipper` are handled INLINE rather than by calling
/// [`contains_epathmap`]. That is deliberate: `contains_epathmap` calls
/// `contains_par`, so routing nested maps through it would start a fresh walk per
/// map-nesting level and reintroduce Θ(map depth) native frames — the exact defect
/// being removed, hidden one indirection deeper.
///
/// # Equivalence with the recursive form
///
/// The predicate is a pure existential over the term's `EPathMap` nodes —
/// "does any reachable map have a filled intern cell?" — with no side effects
/// (`interned_handle()` is a read-only cell peek that never forces an intern).
/// An existential is order-insensitive, so replacing in-order short-circuiting
/// recursion with LIFO short-circuiting iteration cannot change the answer; only
/// the order in which nodes are visited moves.
///
/// ★ And the answer only *selects an emitter*: both emitters are byte-identical,
/// asserted by `models/tests/epathmap_spliced_event_bytes.rs` and
/// `models/tests/event_hash_leg_cold_encode_identity.rs`. So this conversion
/// cannot move a consensus byte even if the predicate were wrong — but it is
/// gated as if it could be, because "cannot matter" is a bad reason to skip a
/// differential on an event-hash path.
///
/// ⚠ NOT FIXED HERE, and not a regression introduced here: this scan is still
/// Θ(term size) in TIME per call, and `emit_vec`/`emit_option` re-invoke it once
/// per level of the spliced walk, making the SPLICED path Θ(depth²) in time. That
/// is a separate finding with its own correctness argument (memoising or hoisting
/// the scan); it is recorded on the module header and deliberately left alone.
fn contains_par(root: &Par) -> bool {
    let mut worklist: Vec<&Par> = Vec::with_capacity(CONTAINS_WORKLIST_CAPACITY);
    worklist.push(root);

    while let Some(par) = worklist.pop() {
        for send in &par.sends {
            push_opt(&send.chan, &mut worklist);
            push_all(&send.data, &mut worklist);
        }
        for receive in &par.receives {
            for bind in &receive.binds {
                push_all(&bind.patterns, &mut worklist);
                push_opt(&bind.source, &mut worklist);
            }
            push_opt(&receive.body, &mut worklist);
            push_opt(&receive.condition, &mut worklist);
        }
        for new in &par.news {
            push_opt(&new.p, &mut worklist);
            worklist.extend(new.injections.values());
        }
        for expr in &par.exprs {
            // The ONLY place the predicate can answer `true`.
            if expr_answers_true(expr, &mut worklist) {
                return true;
            }
        }
        for match_proc in &par.matches {
            push_opt(&match_proc.target, &mut worklist);
            for case in &match_proc.cases {
                push_opt(&case.pattern, &mut worklist);
                push_opt(&case.source, &mut worklist);
                push_opt(&case.guard, &mut worklist);
            }
        }
        for bundle in &par.bundles {
            push_opt(&bundle.body, &mut worklist);
        }
        for connective in &par.connectives {
            match &connective.connective_instance {
                Some(ConnectiveInstance::ConnAndBody(body))
                | Some(ConnectiveInstance::ConnOrBody(body)) => push_all(&body.ps, &mut worklist),
                Some(ConnectiveInstance::ConnNotBody(inner)) => worklist.push(inner),
                _ => {}
            }
        }
        for conditional in &par.conditionals {
            push_opt(&conditional.condition, &mut worklist);
            push_opt(&conditional.if_true, &mut worklist);
            push_opt(&conditional.if_false, &mut worklist);
        }
        // `unforgeables` carry no Par (GPrivate/GDeployId/GDeployerId/
        // GSysAuthToken are byte/unit payloads) — never map-bearing.
    }
    false
}

fn push_opt<'a>(value: &'a Option<Par>, worklist: &mut Vec<&'a Par>) {
    if let Some(par) = value {
        worklist.push(par);
    }
}

fn push_all<'a>(values: &'a [Par], worklist: &mut Vec<&'a Par>) {
    worklist.extend(values);
}

/// The two operands of a binary `Expr` arm.
fn push_pair<'a>(p1: &'a Option<Par>, p2: &'a Option<Par>, worklist: &mut Vec<&'a Par>) {
    push_opt(p1, worklist);
    push_opt(p2, worklist);
}

/// One `Expr`, flattened: `true` iff it carries a FILLED intern cell directly;
/// otherwise its `Par` children are pushed and the walk continues.
///
/// ⚠ Mirrors [`contains_expr`]'s arm-for-arm structure deliberately, including
/// the `_ => false` fallthrough for ground scalars and `EVar`. The two must agree;
/// they are held to that by the differential gates named on [`contains_par`].
fn expr_answers_true<'a>(expr: &'a Expr, worklist: &mut Vec<&'a Par>) -> bool {
    match &expr.expr_instance {
        Some(ExprInstance::EPathmapBody(map)) => {
            if map.interned_handle().is_some() {
                return true;
            }
            push_all(map.ps(), worklist);
        }
        Some(ExprInstance::EZipperBody(zipper)) => {
            if let Some(map) = zipper.pathmap.as_ref() {
                if map.interned_handle().is_some() {
                    return true;
                }
                push_all(map.ps(), worklist);
            }
        }
        Some(ExprInstance::EListBody(list)) => push_all(&list.ps, worklist),
        Some(ExprInstance::ETupleBody(tuple)) => push_all(&tuple.ps, worklist),
        Some(ExprInstance::ESetBody(set)) => push_all(&set.ps, worklist),
        Some(ExprInstance::EMapBody(map)) => {
            for kv in &map.kvs {
                push_opt(&kv.key, worklist);
                push_opt(&kv.value, worklist);
            }
        }
        Some(ExprInstance::EMethodBody(method)) => {
            push_opt(&method.target, worklist);
            push_all(&method.arguments, worklist);
        }
        Some(ExprInstance::EMatchesBody(matches)) => {
            push_opt(&matches.target, worklist);
            push_opt(&matches.pattern, worklist);
        }
        Some(ExprInstance::ENotBody(e)) => push_opt(&e.p, worklist),
        Some(ExprInstance::ENegBody(e)) => push_opt(&e.p, worklist),
        // ⚠ The sixteen binary arms are spelled out rather than collapsed into an
        // or-pattern: `EMult`, `EDiv`, … are DISTINCT generated types that merely
        // share a field shape, so `|` cannot bind them to one name. Kept
        // arm-for-arm with [`contains_expr`] so the two read as the same list.
        Some(ExprInstance::EMultBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EDivBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EModBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EPlusBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EMinusBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::ELtBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::ELteBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EGtBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EGteBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EEqBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::ENeqBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EAndBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EOrBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EPercentPercentBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EPlusPlusBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        Some(ExprInstance::EMinusMinusBody(e)) => push_pair(&e.p1, &e.p2, worklist),
        // Ground scalars and EVar: map-free.
        _ => {}
    }
    false
}

fn contains_opt_par(par: &Option<Par>) -> bool { par.as_ref().is_some_and(contains_par) }

fn contains_send(send: &Send) -> bool {
    contains_opt_par(&send.chan) || send.data.iter().any(contains_par)
}

fn contains_receive_bind(bind: &ReceiveBind) -> bool {
    bind.patterns.iter().any(contains_par) || contains_opt_par(&bind.source)
}

fn contains_receive(receive: &Receive) -> bool {
    receive.binds.iter().any(contains_receive_bind)
        || contains_opt_par(&receive.body)
        || contains_opt_par(&receive.condition)
}

fn contains_new(new: &New) -> bool {
    contains_opt_par(&new.p) || new.injections.values().any(contains_par)
}

fn contains_match_case(case: &MatchCase) -> bool {
    contains_opt_par(&case.pattern) || contains_opt_par(&case.source) || contains_opt_par(&case.guard)
}

fn contains_match(match_proc: &Match) -> bool {
    contains_opt_par(&match_proc.target) || match_proc.cases.iter().any(contains_match_case)
}

fn contains_if(conditional: &If) -> bool {
    contains_opt_par(&conditional.condition)
        || contains_opt_par(&conditional.if_true)
        || contains_opt_par(&conditional.if_false)
}

fn contains_bundle(bundle: &Bundle) -> bool { contains_opt_par(&bundle.body) }

fn contains_connective(connective: &Connective) -> bool {
    match &connective.connective_instance {
        Some(ConnectiveInstance::ConnAndBody(body))
        | Some(ConnectiveInstance::ConnOrBody(body)) => body.ps.iter().any(contains_par),
        Some(ConnectiveInstance::ConnNotBody(par)) => contains_par(par),
        _ => false,
    }
}

fn contains_key_value_pair(kv: &KeyValuePair) -> bool {
    contains_opt_par(&kv.key) || contains_opt_par(&kv.value)
}

fn contains_expr(expr: &Expr) -> bool {
    match &expr.expr_instance {
        Some(ExprInstance::EPathmapBody(map)) => contains_epathmap(map),
        Some(ExprInstance::EZipperBody(zipper)) => {
            zipper.pathmap.as_ref().is_some_and(contains_epathmap)
        }
        Some(ExprInstance::EListBody(list)) => list.ps.iter().any(contains_par),
        Some(ExprInstance::ETupleBody(tuple)) => tuple.ps.iter().any(contains_par),
        Some(ExprInstance::ESetBody(set)) => set.ps.iter().any(contains_par),
        Some(ExprInstance::EMapBody(map)) => map.kvs.iter().any(contains_key_value_pair),
        Some(ExprInstance::EMethodBody(method)) => {
            contains_opt_par(&method.target) || method.arguments.iter().any(contains_par)
        }
        Some(ExprInstance::EMatchesBody(matches)) => {
            contains_opt_par(&matches.target) || contains_opt_par(&matches.pattern)
        }
        Some(ExprInstance::ENotBody(e)) => contains_opt_par(&e.p),
        Some(ExprInstance::ENegBody(e)) => contains_opt_par(&e.p),
        Some(ExprInstance::EMultBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EDivBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EModBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EPlusBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EMinusBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::ELtBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::ELteBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EGtBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EGteBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EEqBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::ENeqBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EAndBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EOrBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EPercentPercentBody(e)) => {
            contains_opt_par(&e.p1) || contains_opt_par(&e.p2)
        }
        Some(ExprInstance::EPlusPlusBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EMinusMinusBody(e)) => {
            contains_opt_par(&e.p1) || contains_opt_par(&e.p2)
        }
        // Ground scalars and EVar: map-free.
        _ => false,
    }
}

fn contains_par_with_random(pwr: &ParWithRandom) -> bool { contains_opt_par(&pwr.body) }

fn contains_tagged_continuation(continuation: &TaggedContinuation) -> bool {
    contains_opt_par(&continuation.guard)
        || matches!(&continuation.tagged_cont,
                    Some(TaggedCont::ParBody(pwr)) if contains_par_with_random(pwr))
}

// ─────────────────────────────────────────────────────────────────────────────
// Hand emitters for the container spine (declaration-order field walks)
// ─────────────────────────────────────────────────────────────────────────────

/// The SPLICE point. Filled cell ⇒ append the cached serde bytes (computing
/// them ONCE per interned entry via the P1 `OnceLock` — the lazy population
/// PM-1 assigns to P4); unfilled ⇒ hand-walk the fields (a nested map may
/// still splice).
fn emit_epathmap(map: &EPathMap, out: &mut Vec<u8>) {
    if let Some(interned) = map.interned_handle() {
        let bytes = interned.serde_bytes.get_or_init(|| direct(map));
        out.extend_from_slice(bytes);
        return;
    }
    // Declaration order: ps, locally_free (EMPTY), connective_used, remainder.
    //
    // ★ The ground/non-ground fork is GONE. It emitted `ground_canonical_ps(map)`
    // for a ground map and the stored `Vec` otherwise, because only the ground
    // arm had a trie to read a canonical order off. `map.ps()` IS that trie
    // read, for every map, so `emit == direct` holds in ANY construction order
    // without a branch to keep the two arms in agreement.
    emit_vec(map.ps(), contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(map.connective_used, out);
    emit_black_box_option(&map.remainder, out);
}

fn emit_par(par: &Par, out: &mut Vec<u8>) {
    // Declaration order: sends, receives, news, exprs, matches, unforgeables,
    // bundles, connectives, conditionals, locally_free (EMPTY),
    // connective_used. (NOTE: declaration order differs from proto tag
    // order — serde follows declaration.)
    emit_vec(&par.sends, contains_send, emit_send, out);
    emit_vec(&par.receives, contains_receive, emit_receive, out);
    emit_vec(&par.news, contains_new, emit_new, out);
    emit_vec(&par.exprs, contains_expr, emit_expr, out);
    emit_vec(&par.matches, contains_match, emit_match, out);
    emit_black_box(&par.unforgeables, out); // never map-bearing
    emit_vec(&par.bundles, contains_bundle, emit_bundle, out);
    emit_vec(&par.connectives, contains_connective, emit_connective, out);
    emit_vec(&par.conditionals, contains_if, emit_if, out);
    emit_locally_free_as_empty(out);
    emit_bool(par.connective_used, out);
}

fn emit_send(send: &Send, out: &mut Vec<u8>) {
    // chan, data, persistent, locally_free (EMPTY), connective_used.
    emit_option(&send.chan, contains_par, emit_par, out);
    emit_vec(&send.data, contains_par, emit_par, out);
    emit_bool(send.persistent, out);
    emit_locally_free_as_empty(out);
    emit_bool(send.connective_used, out);
}

fn emit_receive_bind(bind: &ReceiveBind, out: &mut Vec<u8>) {
    // patterns, source, remainder, free_count.
    emit_vec(&bind.patterns, contains_par, emit_par, out);
    emit_option(&bind.source, contains_par, emit_par, out);
    emit_black_box_option(&bind.remainder, out);
    emit_i32(bind.free_count, out);
}

fn emit_receive(receive: &Receive, out: &mut Vec<u8>) {
    // binds, body, persistent, peek, bind_count, locally_free (EMPTY),
    // connective_used, condition.
    emit_vec(&receive.binds, contains_receive_bind, emit_receive_bind, out);
    emit_option(&receive.body, contains_par, emit_par, out);
    emit_bool(receive.persistent, out);
    emit_bool(receive.peek, out);
    emit_i32(receive.bind_count, out);
    emit_locally_free_as_empty(out);
    emit_bool(receive.connective_used, out);
    emit_option(&receive.condition, contains_par, emit_par, out);
}

fn emit_new(new: &New, out: &mut Vec<u8>) {
    // bind_count, p, uri, injections, locally_free (EMPTY).
    emit_i32(new.bind_count, out);
    emit_option(&new.p, contains_par, emit_par, out);
    emit_black_box(&new.uri, out);
    emit_u64(new.injections.len() as u64, out);
    for (key, value) in &new.injections {
        emit_string(key, out);
        if contains_par(value) {
            emit_par(value, out);
        } else {
            emit_black_box(value, out);
        }
    }
    emit_locally_free_as_empty(out);
}

fn emit_match_case(case: &MatchCase, out: &mut Vec<u8>) {
    // pattern, source, free_count, guard.
    emit_option(&case.pattern, contains_par, emit_par, out);
    emit_option(&case.source, contains_par, emit_par, out);
    emit_i32(case.free_count, out);
    emit_option(&case.guard, contains_par, emit_par, out);
}

fn emit_match(match_proc: &Match, out: &mut Vec<u8>) {
    // target, cases, locally_free (EMPTY), connective_used.
    emit_option(&match_proc.target, contains_par, emit_par, out);
    emit_vec(&match_proc.cases, contains_match_case, emit_match_case, out);
    emit_locally_free_as_empty(out);
    emit_bool(match_proc.connective_used, out);
}

fn emit_if(conditional: &If, out: &mut Vec<u8>) {
    // condition, if_true, if_false, locally_free (EMPTY), connective_used.
    emit_option(&conditional.condition, contains_par, emit_par, out);
    emit_option(&conditional.if_true, contains_par, emit_par, out);
    emit_option(&conditional.if_false, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(conditional.connective_used, out);
}

fn emit_bundle(bundle: &Bundle, out: &mut Vec<u8>) {
    // body, write_flag, read_flag.
    emit_option(&bundle.body, contains_par, emit_par, out);
    emit_bool(bundle.write_flag, out);
    emit_bool(bundle.read_flag, out);
}

fn emit_connective(connective: &Connective, out: &mut Vec<u8>) {
    // connective_instance: Option<ConnectiveInstance> (9 variants,
    // declaration order 0..=8).
    match &connective.connective_instance {
        None => out.push(0),
        Some(instance) => {
            out.push(1);
            match instance {
                ConnectiveInstance::ConnAndBody(body) => {
                    emit_u32(0, out);
                    emit_vec(&body.ps, contains_par, emit_par, out);
                }
                ConnectiveInstance::ConnOrBody(body) => {
                    emit_u32(1, out);
                    emit_vec(&body.ps, contains_par, emit_par, out);
                }
                ConnectiveInstance::ConnNotBody(par) => {
                    emit_u32(2, out);
                    if contains_par(par) {
                        emit_par(par, out);
                    } else {
                        emit_black_box(par, out);
                    }
                }
                ConnectiveInstance::VarRefBody(var_ref) => {
                    emit_u32(3, out);
                    emit_black_box(var_ref, out);
                }
                ConnectiveInstance::ConnBool(b) => {
                    emit_u32(4, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnInt(b) => {
                    emit_u32(5, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnString(b) => {
                    emit_u32(6, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnUri(b) => {
                    emit_u32(7, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnByteArray(b) => {
                    emit_u32(8, out);
                    emit_bool(*b, out);
                }
            }
        }
    }
}

fn emit_key_value_pair(kv: &KeyValuePair, out: &mut Vec<u8>) {
    emit_option(&kv.key, contains_par, emit_par, out);
    emit_option(&kv.value, contains_par, emit_par, out);
}

fn emit_elist(list: &EList, out: &mut Vec<u8>) {
    // ps, locally_free (EMPTY), connective_used, remainder.
    emit_vec(&list.ps, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(list.connective_used, out);
    emit_black_box_option(&list.remainder, out);
}

fn emit_etuple(tuple: &ETuple, out: &mut Vec<u8>) {
    // ps, locally_free (EMPTY), connective_used.
    emit_vec(&tuple.ps, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(tuple.connective_used, out);
}

fn emit_eset(set: &ESet, out: &mut Vec<u8>) {
    // ps, locally_free (EMPTY), connective_used, remainder.
    emit_vec(&set.ps, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(set.connective_used, out);
    emit_black_box_option(&set.remainder, out);
}

fn emit_emap(map: &EMap, out: &mut Vec<u8>) {
    // kvs, locally_free (EMPTY), connective_used, remainder.
    emit_vec(&map.kvs, contains_key_value_pair, emit_key_value_pair, out);
    emit_locally_free_as_empty(out);
    emit_bool(map.connective_used, out);
    emit_black_box_option(&map.remainder, out);
}

fn emit_emethod(method: &EMethod, out: &mut Vec<u8>) {
    // method_name, target, arguments, locally_free (EMPTY), connective_used.
    emit_string(&method.method_name, out);
    emit_option(&method.target, contains_par, emit_par, out);
    emit_vec(&method.arguments, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(method.connective_used, out);
}

fn emit_ematches(matches: &EMatches, out: &mut Vec<u8>) {
    emit_option(&matches.target, contains_par, emit_par, out);
    emit_option(&matches.pattern, contains_par, emit_par, out);
}

fn emit_ezipper(zipper: &EZipper, out: &mut Vec<u8>) {
    // pathmap, current_path, is_write_zipper, locally_free (EMPTY),
    // connective_used, cursor_kind.
    emit_option(&zipper.pathmap, contains_epathmap, emit_epathmap, out);
    emit_black_box(&zipper.current_path, out);
    emit_bool(zipper.is_write_zipper, out);
    emit_locally_free_as_empty(out);
    emit_bool(zipper.connective_used, out);
    // `cursor_kind` (RhoTypes.proto): WHICH ENTRY `current_path` addresses.
    // serde emits every field regardless of value, so unlike prost — which
    // omits a default-valued scalar and therefore does not move — this
    // APPENDS four little-endian bytes to an EZipper's event-hash encoding.
    // Declaration order puts it last, so nothing before it shifts.
    emit_u32(zipper.cursor_kind, out);
}

/// Two-Par operator payload (`p1`, `p2`) — shared shape of the arithmetic /
/// comparison / logic / concatenation expr messages.
fn emit_binary_op(p1: &Option<Par>, p2: &Option<Par>, out: &mut Vec<u8>) {
    emit_option(p1, contains_par, emit_par, out);
    emit_option(p2, contains_par, emit_par, out);
}

fn emit_par_with_random(pwr: &ParWithRandom, out: &mut Vec<u8>) {
    // body, random_state.
    emit_option(&pwr.body, contains_par, emit_par, out);
    emit_bytes(&pwr.random_state, out);
}

fn emit_expr(expr: &Expr, out: &mut Vec<u8>) {
    // Expr { expr_instance: Option<ExprInstance> } — the 36 variants in
    // DECLARATION order (0-based indexes; the proto tags are NOT the serde
    // indexes).
    match &expr.expr_instance {
        None => out.push(0),
        Some(instance) => {
            out.push(1);
            match instance {
                ExprInstance::GBool(b) => {
                    emit_u32(0, out);
                    emit_bool(*b, out);
                }
                ExprInstance::GInt(i) => {
                    emit_u32(1, out);
                    emit_i64(*i, out);
                }
                ExprInstance::GString(s) => {
                    emit_u32(2, out);
                    emit_string(s, out);
                }
                ExprInstance::GUri(s) => {
                    emit_u32(3, out);
                    emit_string(s, out);
                }
                ExprInstance::GByteArray(bytes) => {
                    emit_u32(4, out);
                    emit_bytes(bytes, out);
                }
                ExprInstance::ENotBody(e) => {
                    emit_u32(5, out);
                    emit_option(&e.p, contains_par, emit_par, out);
                }
                ExprInstance::ENegBody(e) => {
                    emit_u32(6, out);
                    emit_option(&e.p, contains_par, emit_par, out);
                }
                ExprInstance::EMultBody(e) => {
                    emit_u32(7, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EDivBody(e) => {
                    emit_u32(8, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EPlusBody(e) => {
                    emit_u32(9, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EMinusBody(e) => {
                    emit_u32(10, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::ELtBody(e) => {
                    emit_u32(11, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::ELteBody(e) => {
                    emit_u32(12, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EGtBody(e) => {
                    emit_u32(13, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EGteBody(e) => {
                    emit_u32(14, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EEqBody(e) => {
                    emit_u32(15, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::ENeqBody(e) => {
                    emit_u32(16, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EAndBody(e) => {
                    emit_u32(17, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EOrBody(e) => {
                    emit_u32(18, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EVarBody(e) => {
                    emit_u32(19, out);
                    emit_black_box(e, out); // Var payload: map-free
                }
                ExprInstance::EListBody(list) => {
                    emit_u32(20, out);
                    emit_elist(list, out);
                }
                ExprInstance::ETupleBody(tuple) => {
                    emit_u32(21, out);
                    emit_etuple(tuple, out);
                }
                ExprInstance::ESetBody(set) => {
                    emit_u32(22, out);
                    emit_eset(set, out);
                }
                ExprInstance::EMapBody(map) => {
                    emit_u32(23, out);
                    emit_emap(map, out);
                }
                ExprInstance::EMethodBody(method) => {
                    emit_u32(24, out);
                    emit_emethod(method, out);
                }
                ExprInstance::EPathmapBody(map) => {
                    emit_u32(25, out);
                    emit_epathmap(map, out); // THE SPLICE
                }
                ExprInstance::EZipperBody(zipper) => {
                    emit_u32(26, out);
                    emit_ezipper(zipper, out);
                }
                ExprInstance::EMatchesBody(matches) => {
                    emit_u32(27, out);
                    emit_ematches(matches, out);
                }
                ExprInstance::EPercentPercentBody(e) => {
                    emit_u32(28, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EPlusPlusBody(e) => {
                    emit_u32(29, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EMinusMinusBody(e) => {
                    emit_u32(30, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EModBody(e) => {
                    emit_u32(31, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::GDouble(bits) => {
                    emit_u32(32, out);
                    out.extend_from_slice(&bits.to_le_bytes());
                }
                ExprInstance::GBigInt(bytes) => {
                    emit_u32(33, out);
                    emit_bytes(bytes, out);
                }
                ExprInstance::GBigRat(rational) => {
                    emit_u32(34, out);
                    emit_bytes(&rational.numerator, out);
                    emit_bytes(&rational.denominator, out);
                }
                ExprInstance::GFixedPoint(fixed_point) => {
                    emit_u32(35, out);
                    emit_bytes(&fixed_point.unscaled, out);
                    emit_u32(fixed_point.scale, out);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The StableHashSerialize overrides (the ONLY event-hash entry points)
// ─────────────────────────────────────────────────────────────────────────────

use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;

impl StableHashSerialize for ListParWithRandom {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_list_par_with_random(self) }
}

impl StableHashSerialize for BindPattern {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_bind_pattern(self) }
}

impl StableHashSerialize for TaggedContinuation {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_tagged_continuation(self) }
}

/// ★ PRODUCTION WIRING — the CHANNEL leg of every event hash.
///
/// `Par` is the channel type `C` of `RSpace<Par, BindPattern,
/// ListParWithRandom, TaggedContinuation>`, so this override puts the
/// single-walk trampolined encoder on `stable_hash_provider::hash` /
/// `hash_vec` / `hash_from_vec` — reached once per produce and once per
/// channel per consume.
///
/// ⚠ The trait's contract is exact and is quoted here because it is the whole
/// safety argument: *"An override may ONLY be a byte-identical faster
/// construction."* That obligation is discharged by
/// `models/tests/wire_encode_differential.rs`, which asserts BYTE IDENTITY
/// against the derived `Serialize` over every `ExprInstance` arm, every
/// `ConnectiveInstance` arm, the full variant × arity × awkward-combination
/// cross product, every awkward ground literal, both `EPathMap` serialize
/// arms, deep terms past every old ceiling, and proptest-generated terms — and
/// which carries an executed proof that it can go RED.
///
/// ⚠ This is the CHANNEL leg only. The datum, pattern and continuation legs
/// above keep the intern-aware spliced emitter, which reuses cached bytes at
/// filled-cell `EPathMap` nodes: a different and complementary optimization,
/// deliberately untouched.
impl StableHashSerialize for Par {
    fn stable_hash_bytes(&self) -> Vec<u8> {
        crate::rust::rholang::wire_encode::encode(self)
    }
}

// Default-body impls (direct bincode) for rhoapi types used as space type
// parameters in tests/tools without an EPathMap-bearing hot path.
impl StableHashSerialize for ParWithRandom {}

// ═════════════════════════════════════════════════════════════════════════════
// ★★ WORK ITEM #124 — THE PREDICATE-EQUIVALENCE DIFFERENTIAL
// ═════════════════════════════════════════════════════════════════════════════
//
// `contains_par` was converted from a mutually-recursive 15-function SCC to an
// explicit worklist (see its doc comment for the measurement that forced it).
// Every OTHER gate on this module compares BYTES — and every one of them is
// structurally blind to this change, because the predicate does not produce
// bytes: it only chooses which of two BYTE-IDENTICAL emitters runs. A predicate
// that answered `true` everywhere, or `false` everywhere, would keep
// `epathmap_spliced_event_bytes`, `epathmap_canonical_fixtures` and
// `event_hash_leg_cold_encode_identity` all green while silently disabling the
// splice optimisation (or, worse, forcing the hand-written spine onto values it
// was never exercised against).
//
// ⚠ So byte gates cannot discharge this conversion's obligation, and this
// module-private test is the only place the obligation CAN be discharged:
// `contains_par` is not public, by design.
//
// The oracle is the PRE-CONVERSION implementation, preserved verbatim from
// `42d5082a` with every name prefixed `ref_`. It is a frozen reference, not a
// second implementation to maintain: if the production predicate's *meaning* is
// ever intended to change, this oracle must be updated in the same commit and
// the change stated — which is exactly the review event that should happen.

#[cfg(test)]
mod contains_par_equivalence {
    use super::*;
    use crate::rhoapi::{ConnectiveBody, EMap, EMethod, ETuple, ESet, Var, var::VarInstance};

    // ── the frozen recursive oracle (verbatim from 42d5082a) ─────────────────

    fn ref_contains_epathmap(map: &EPathMap) -> bool {
        map.interned_handle().is_some() || map.ps().iter().any(ref_contains_par)
    }

    fn ref_contains_par(par: &Par) -> bool {
        par.sends.iter().any(ref_contains_send)
            || par.receives.iter().any(ref_contains_receive)
            || par.news.iter().any(ref_contains_new)
            || par.exprs.iter().any(ref_contains_expr)
            || par.matches.iter().any(ref_contains_match)
            || par.bundles.iter().any(ref_contains_bundle)
            || par.connectives.iter().any(ref_contains_connective)
            || par.conditionals.iter().any(ref_contains_if)
        // unforgeables carry no Par (GPrivate/GDeployId/GDeployerId/GSysAuthToken
        // are byte/unit payloads) — never map-bearing.
    }

    fn ref_contains_opt_par(par: &Option<Par>) -> bool { par.as_ref().is_some_and(ref_contains_par) }

    fn ref_contains_send(send: &Send) -> bool {
        ref_contains_opt_par(&send.chan) || send.data.iter().any(ref_contains_par)
    }

    fn ref_contains_receive_bind(bind: &ReceiveBind) -> bool {
        bind.patterns.iter().any(ref_contains_par) || ref_contains_opt_par(&bind.source)
    }

    fn ref_contains_receive(receive: &Receive) -> bool {
        receive.binds.iter().any(ref_contains_receive_bind)
            || ref_contains_opt_par(&receive.body)
            || ref_contains_opt_par(&receive.condition)
    }

    fn ref_contains_new(new: &New) -> bool {
        ref_contains_opt_par(&new.p) || new.injections.values().any(ref_contains_par)
    }

    fn ref_contains_match_case(case: &MatchCase) -> bool {
        ref_contains_opt_par(&case.pattern) || ref_contains_opt_par(&case.source) || ref_contains_opt_par(&case.guard)
    }

    fn ref_contains_match(match_proc: &Match) -> bool {
        ref_contains_opt_par(&match_proc.target) || match_proc.cases.iter().any(ref_contains_match_case)
    }

    fn ref_contains_if(conditional: &If) -> bool {
        ref_contains_opt_par(&conditional.condition)
            || ref_contains_opt_par(&conditional.if_true)
            || ref_contains_opt_par(&conditional.if_false)
    }

    fn ref_contains_bundle(bundle: &Bundle) -> bool { ref_contains_opt_par(&bundle.body) }

    fn ref_contains_connective(connective: &Connective) -> bool {
        match &connective.connective_instance {
            Some(ConnectiveInstance::ConnAndBody(body))
            | Some(ConnectiveInstance::ConnOrBody(body)) => body.ps.iter().any(ref_contains_par),
            Some(ConnectiveInstance::ConnNotBody(par)) => ref_contains_par(par),
            _ => false,
        }
    }

    fn ref_contains_key_value_pair(kv: &KeyValuePair) -> bool {
        ref_contains_opt_par(&kv.key) || ref_contains_opt_par(&kv.value)
    }

    fn ref_contains_expr(expr: &Expr) -> bool {
        match &expr.expr_instance {
            Some(ExprInstance::EPathmapBody(map)) => ref_contains_epathmap(map),
            Some(ExprInstance::EZipperBody(zipper)) => {
                zipper.pathmap.as_ref().is_some_and(ref_contains_epathmap)
            }
            Some(ExprInstance::EListBody(list)) => list.ps.iter().any(ref_contains_par),
            Some(ExprInstance::ETupleBody(tuple)) => tuple.ps.iter().any(ref_contains_par),
            Some(ExprInstance::ESetBody(set)) => set.ps.iter().any(ref_contains_par),
            Some(ExprInstance::EMapBody(map)) => map.kvs.iter().any(ref_contains_key_value_pair),
            Some(ExprInstance::EMethodBody(method)) => {
                ref_contains_opt_par(&method.target) || method.arguments.iter().any(ref_contains_par)
            }
            Some(ExprInstance::EMatchesBody(matches)) => {
                ref_contains_opt_par(&matches.target) || ref_contains_opt_par(&matches.pattern)
            }
            Some(ExprInstance::ENotBody(e)) => ref_contains_opt_par(&e.p),
            Some(ExprInstance::ENegBody(e)) => ref_contains_opt_par(&e.p),
            Some(ExprInstance::EMultBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EDivBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EModBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EPlusBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EMinusBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::ELtBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::ELteBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EGtBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EGteBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EEqBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::ENeqBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EAndBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EOrBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EPercentPercentBody(e)) => {
                ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2)
            }
            Some(ExprInstance::EPlusPlusBody(e)) => ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2),
            Some(ExprInstance::EMinusMinusBody(e)) => {
                ref_contains_opt_par(&e.p1) || ref_contains_opt_par(&e.p2)
            }
            // Ground scalars and EVar: map-free.
            _ => false,
        }
    }

    // ── fixture construction, ITERATIVE ──────────────────────────────────────

    fn expr_par(instance: ExprInstance) -> Par {
        Par { exprs: vec![Expr { expr_instance: Some(instance) }], ..Default::default() }
    }

    fn gint(value: i64) -> Par { expr_par(ExprInstance::GInt(value)) }

    fn map_par(filled: bool, entries: Vec<Par>) -> Par {
        let map = EPathMap::new(entries, Vec::new(), false, None);
        if filled {
            let _ = map.intern();
            assert!(map.interned_handle().is_some(), "intern() must fill the cell");
        }
        expr_par(ExprInstance::EPathmapBody(map))
    }

    /// Wrap `inner` in each container the predicate descends, one per element.
    /// ★ This is the coverage that matters: the two implementations can only
    /// disagree at a container whose child enumeration they spell differently.
    fn every_container(inner: Par) -> Vec<(&'static str, Par)> {
        let some = |p: &Par| Some(p.clone());
        vec![
            ("elist", expr_par(ExprInstance::EListBody(EList {
                ps: vec![inner.clone()], locally_free: vec![], connective_used: false, remainder: None }))),
            ("etuple", expr_par(ExprInstance::ETupleBody(ETuple {
                ps: vec![inner.clone()], locally_free: vec![], connective_used: false }))),
            ("eset", expr_par(ExprInstance::ESetBody(ESet {
                ps: vec![inner.clone()], locally_free: vec![], connective_used: false, remainder: None }))),
            ("emap-key", expr_par(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair { key: some(&inner), value: Some(gint(1)) }],
                locally_free: vec![], connective_used: false, remainder: None }))),
            ("emap-value", expr_par(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair { key: Some(gint(1)), value: some(&inner) }],
                locally_free: vec![], connective_used: false, remainder: None }))),
            ("emethod-target", expr_par(ExprInstance::EMethodBody(EMethod {
                method_name: "m".into(), target: some(&inner), arguments: vec![],
                locally_free: vec![], connective_used: false }))),
            ("emethod-arg", expr_par(ExprInstance::EMethodBody(EMethod {
                method_name: "m".into(), target: Some(gint(1)), arguments: vec![inner.clone()],
                locally_free: vec![], connective_used: false }))),
            ("ematches-target", expr_par(ExprInstance::EMatchesBody(EMatches {
                target: some(&inner), pattern: Some(gint(1)) }))),
            ("ematches-pattern", expr_par(ExprInstance::EMatchesBody(EMatches {
                target: Some(gint(1)), pattern: some(&inner) }))),
            ("eplus-p1", expr_par(ExprInstance::EPlusBody(
                crate::rhoapi::EPlus { p1: some(&inner), p2: Some(gint(1)) }))),
            ("eplus-p2", expr_par(ExprInstance::EPlusBody(
                crate::rhoapi::EPlus { p1: Some(gint(1)), p2: some(&inner) }))),
            ("enot", expr_par(ExprInstance::ENotBody(
                crate::rhoapi::ENot { p: some(&inner) }))),
            ("eneg", expr_par(ExprInstance::ENegBody(
                crate::rhoapi::ENeg { p: some(&inner) }))),
            ("send-chan", Par { sends: vec![Send {
                chan: some(&inner), data: vec![], persistent: false,
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("send-data", Par { sends: vec![Send {
                chan: Some(gint(1)), data: vec![inner.clone()], persistent: false,
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("receive-pattern", Par { receives: vec![Receive {
                binds: vec![ReceiveBind { patterns: vec![inner.clone()], source: Some(gint(1)),
                    remainder: None, free_count: 0 }],
                body: None, condition: None, persistent: false, peek: false, bind_count: 0,
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("receive-source", Par { receives: vec![Receive {
                binds: vec![ReceiveBind { patterns: vec![], source: some(&inner),
                    remainder: None, free_count: 0 }],
                body: None, condition: None, persistent: false, peek: false, bind_count: 0,
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("receive-body", Par { receives: vec![Receive {
                binds: vec![], body: some(&inner), condition: None, persistent: false, peek: false,
                bind_count: 0, locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("receive-condition", Par { receives: vec![Receive {
                binds: vec![], body: None, condition: some(&inner), persistent: false, peek: false,
                bind_count: 0, locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("new-body", Par { news: vec![New {
                bind_count: 0, p: some(&inner), uri: vec![], injections: Default::default(),
                locally_free: vec![] }], ..Default::default() }),
            ("new-injection", Par { news: vec![New {
                bind_count: 0, p: None, uri: vec![],
                injections: [("k".to_string(), inner.clone())].into_iter().collect(),
                locally_free: vec![] }], ..Default::default() }),
            ("match-target", Par { matches: vec![Match {
                target: some(&inner), cases: vec![], locally_free: vec![],
                connective_used: false }], ..Default::default() }),
            ("match-case-pattern", Par { matches: vec![Match {
                target: None, cases: vec![MatchCase { pattern: some(&inner), source: None,
                    guard: None, free_count: 0 }],
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("match-case-source", Par { matches: vec![Match {
                target: None, cases: vec![MatchCase { pattern: None, source: some(&inner),
                    guard: None, free_count: 0 }],
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("match-case-guard", Par { matches: vec![Match {
                target: None, cases: vec![MatchCase { pattern: None, source: None,
                    guard: some(&inner), free_count: 0 }],
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("bundle", Par { bundles: vec![Bundle {
                body: some(&inner), write_flag: false, read_flag: false }], ..Default::default() }),
            ("conn-and", Par { connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(
                    ConnectiveBody { ps: vec![inner.clone()] })) }], ..Default::default() }),
            ("conn-or", Par { connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnOrBody(
                    ConnectiveBody { ps: vec![inner.clone()] })) }], ..Default::default() }),
            ("conn-not", Par { connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnNotBody(
                    inner.clone())) }], ..Default::default() }),
            ("if-condition", Par { conditionals: vec![If {
                condition: some(&inner), if_true: None, if_false: None,
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("if-true", Par { conditionals: vec![If {
                condition: None, if_true: some(&inner), if_false: None,
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("if-false", Par { conditionals: vec![If {
                condition: None, if_true: None, if_false: some(&inner),
                locally_free: vec![], connective_used: false }], ..Default::default() }),
            ("epathmap-entry", map_par(false, vec![inner.clone()])),
            ("ezipper", expr_par(ExprInstance::EZipperBody(EZipper {
                pathmap: match EPathMap::new(vec![inner], Vec::new(), false, None) { m => Some(m) },
                current_path: vec![], is_write_zipper: false, locally_free: vec![],
                connective_used: false, cursor_kind: 0 }))),
        ]
    }

    /// ★★ **THE OBLIGATION**: at every container position, for a FILLED cell
    /// (answer `true`) and an UNFILLED one (answer `false`), the worklist and the
    /// frozen recursive oracle agree.
    ///
    /// Both polarities are asserted at every position on purpose. A worklist that
    /// forgot to push some container's children would answer `false` where the
    /// oracle answers `true` — visible only in the FILLED row. A worklist that
    /// answered `true` unconditionally would pass the filled row and fail the
    /// unfilled one. Neither row alone is a test.
    #[test]
    fn the_worklist_agrees_with_the_frozen_recursive_oracle_at_every_container() {
        let mut positions = 0usize;
        for filled in [true, false] {
            let leaf = map_par(filled, vec![gint(7)]);
            for (label, term) in every_container(leaf.clone()) {
                let machine = contains_par(&term);
                let oracle = ref_contains_par(&term);
                assert_eq!(
                    machine, oracle,
                    "PREDICATE DIVERGENCE at `{label}` with a {} cell: the worklist \
                     said {machine} and the frozen recursive oracle said {oracle}. \
                     The two emitters are byte-identical so this does not move \
                     consensus bytes, but it means the worklist's child \
                     enumeration has drifted from the recursion's.",
                    if filled { "FILLED" } else { "UNFILLED" }
                );
                // ★ ANTI-VACUITY: the FILLED row must actually be positive.
                // If `every_container` ever stopped embedding the leaf, both
                // sides would agree on `false` at every position and this test
                // would pass while covering nothing.
                if filled {
                    assert!(
                        machine,
                        "VACUOUS at `{label}`: a FILLED cell is embedded at this \
                         position and the predicate did not find it"
                    );
                } else {
                    assert!(!machine, "an UNFILLED fixture answered true at `{label}`");
                }
                positions += 1;
            }
        }
        assert!(
            positions >= 68,
            "container coverage collapsed to {positions} position/polarity pairs \
             (68 when this gate was written)"
        );
    }

    /// Depth: a filled cell at the BOTTOM of a long chain, which is the shape the
    /// depth probe uses and the one a short-circuiting bug would still pass.
    ///
    /// ⚠ The oracle is Θ(depth) in native stack — that is what this change
    /// removes — so it runs on an explicitly large stack. The MACHINE is the
    /// thing that must not need one, and
    /// `casper/tests/event_hash_leg_depth_probe.rs` is where that is measured.
    #[test]
    fn the_worklist_agrees_with_the_oracle_on_deep_chains() {
        std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .spawn(|| {
                for depth in [1usize, 2, 8, 64, 512, 4096] {
                    for filled in [true, false] {
                        let mut term = map_par(filled, vec![gint(7)]);
                        for _ in 0..depth {
                            term = expr_par(ExprInstance::EListBody(EList {
                                ps: vec![term], locally_free: vec![],
                                connective_used: false, remainder: None,
                            }));
                        }
                        assert_eq!(
                            contains_par(&term), ref_contains_par(&term),
                            "PREDICATE DIVERGENCE at depth {depth}, filled={filled}"
                        );
                        assert_eq!(contains_par(&term), filled, "VACUOUS at depth {depth}");
                    }
                }
            })
            .expect("spawn")
            .join()
            .expect("deep equivalence panicked");
    }

    /// A filled cell reachable ONLY through an unfilled outer map — the case the
    /// module header calls out ("a filled inner map inside an unfilled outer map
    /// still splices") and the one an implementation that stopped at the first
    /// `EPathMap` would get wrong.
    #[test]
    fn a_filled_inner_map_under_an_unfilled_outer_map_is_still_found() {
        let inner = map_par(true, vec![gint(1)]);
        let outer = map_par(false, vec![inner]);
        assert!(contains_par(&outer), "the worklist missed a nested filled cell");
        assert_eq!(contains_par(&outer), ref_contains_par(&outer));

        let all_unfilled = map_par(false, vec![map_par(false, vec![gint(1)])]);
        assert!(!contains_par(&all_unfilled));
        assert_eq!(contains_par(&all_unfilled), ref_contains_par(&all_unfilled));
    }

    /// The empty / ground cases: no maps at all, and a bare `Par`.
    #[test]
    fn map_free_terms_answer_false_in_both_implementations() {
        for (label, term) in [
            ("default", Par::default()),
            ("gint", gint(3)),
            ("gstring", expr_par(ExprInstance::GString("x".into()))),
            ("evar", expr_par(ExprInstance::EVarBody(crate::rhoapi::EVar {
                v: Some(Var { var_instance: Some(VarInstance::BoundVar(0)) }) }))),
        ] {
            assert!(!contains_par(&term), "`{label}` is map-free but answered true");
            assert_eq!(contains_par(&term), ref_contains_par(&term), "`{label}`");
        }
    }
}
