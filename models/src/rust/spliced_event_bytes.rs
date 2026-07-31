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
//! single-walk trampolined encoder from `rust::rholang::bincode_encoder`, flat in
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


use crate::rust::rholang::bincode_encoder::ColdStoreEncode;

use crate::rhoapi::{BindPattern, ListParWithRandom, Par, ParWithRandom, TaggedContinuation};


// ─────────────────────────────────────────────────────────────────────────────
// Public entry points (the three event-hash legs)
// ─────────────────────────────────────────────────────────────────────────────

/// bincode-of-`ListParWithRandom` for `hash_produce`'s datum leg —
/// intern-aware when the datum contains a filled-cell EPathMap, the
/// TRAMPOLINED direct path otherwise.
// ═══════════════════════════════════════════════════════════════════════════════════════
// ⛔ THE SPLICED EMITTER IS UNREACHABLE, AND THESE THREE LEGS NOW HAVE ONE BRANCH
// ═══════════════════════════════════════════════════════════════════════════════════════
//
// Each leg used to scan with `contains_par` and, when the answer was `true`, hand off to
// a hand-written emitter that spliced cached bytes out of the intern store. With the
// store deleted that predicate is **CONSTANT FALSE**, so only the `cold_encode` branch
// can ever run.
//
// ★ Why constant, stated precisely — this was got wrong once and is worth pinning.
// `contains_epathmap` read
//
//     map.interned_handle().is_some() || map.ps().iter().any(contains_par)
//
// and it is tempting to read the second disjunct as independent. It is not: it recurses
// into `contains_par`, whose walker `expr_answers_true` has **no arm that returns true**
// — every arm merely pushes to the worklist. The cell check was the SOLE source of
// `true` in the entire predicate, and the recursion bottomed out there. Remove it and
// the disjunction has no base case that can answer yes.
//
// ⇒ Checking a predicate's top-level SHAPE is not checking whether it can still answer
// TRUE. The method that settles it is to enumerate every `true`-producing arm.
//
// ★ The consequence is a WIN, not merely a simplification: `cold_encode` is the
// trampolined, depth-FLAT encoder. The leg was Θ(depth) on the branch it used to take
// most often; it is now flat on all of it, and work item #124's recursion 2b is
// discharged by deletion rather than by repair.

pub fn event_hash_bytes_list_par_with_random(datum: &ListParWithRandom) -> Vec<u8> {
    datum.cold_encode()
}

/// bincode-of-`BindPattern` for `hash_consume`'s per-pattern leg.
pub fn event_hash_bytes_bind_pattern(pattern: &BindPattern) -> Vec<u8> {
    pattern.cold_encode()
}

/// bincode-of-`TaggedContinuation` for `hash_consume`'s continuation leg.
pub fn event_hash_bytes_tagged_continuation(continuation: &TaggedContinuation) -> Vec<u8> {
    continuation.cold_encode()
}

// ─────────────────────────────────────────────────────────────────────────────
// Primitive emitters (the layout rule set)
// ─────────────────────────────────────────────────────────────────────────────

thread_local! {
    /// Address → answer. Read and written **only** while [`MEMO_DEPTH`] is non-zero.
    static CONTAINS_MEMO: std::cell::RefCell<std::collections::HashMap<usize, bool>> =
        std::cell::RefCell::new(std::collections::HashMap::new());

    /// Live [`MemoScope`] count on this thread. Zero ⇒ the memo is not consulted, which is
    /// what makes the pointer key sound for EVERY caller rather than for the callers we
    /// happen to have today.
    static MEMO_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Opens the window in which the address key is meaningful: for the duration of one
/// `event_hash_bytes_*` call the term is borrowed and immutable, so no node can be freed and
/// another allocated at the same address.
///
/// Clears the table on construction **and** on drop. ★ Both, deliberately — clearing only on
/// entry leaves it populated afterwards; clearing only on exit leaves it dirty if a previous
/// call unwound. Doing both makes *"the memo is empty outside a scope"* hold on every path,
/// including a panic.
struct MemoScope;

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
/// `models/tests/bincode_encoder_differential.rs`, which asserts BYTE IDENTITY
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
        crate::rust::rholang::bincode_encoder::encode(self)
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


// ⛔ The `contains_par_equivalence` module is DELETED. It differentialled the worklist
// predicate against a frozen recursive oracle — a genuinely valuable gate that caught a
// real memoisation bug earlier in this campaign. It goes because its SUBJECT goes:
// `contains_par` was the spliced emitter's trigger, and with the intern store removed the
// predicate is constant false and the emitter unreachable.
//
// ★ The suite announced this itself rather than passing quietly: its "VACUOUS at depth
// {depth}" assertions fired the moment the predicate went inert. A differential that can
// detect its own subject becoming constant is worth more than one that merely agrees.

