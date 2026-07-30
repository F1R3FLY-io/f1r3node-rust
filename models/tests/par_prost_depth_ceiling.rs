//! # The `prost` read ceiling on `Par`, exhibited
//!
//! **The asymmetry this file makes executable.** A `Par` of nesting depth 34
//! *encodes* — `prost` places no limit on the write side — and then *does not
//! decode*. `prost`'s `DecodeContext` carries a recursion budget,
//! `RECURSION_LIMIT = 100` message levels (`prost-0.14.3/src/lib.rs:30`), which
//! is a **private** constant: the only knob the crate exposes is the
//! `no-recursion-limit` feature, which *removes* the limit entirely and is
//! enabled nowhere in this workspace. So a term that this node can build,
//! reduce and serialise is a term that a peer — or this node's own replay pass
//! — will refuse to read.
//!
//! This is recorded in
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md` §7.3, which states
//! the obligation this file discharges: the ceiling *"must be surfaced before
//! it is discovered by a validator."*
//!
//! ⚠ **This file surfaces the ceiling. It does not move it.** Widening what a
//! node accepts is a consensus-visible change and belongs to F1r3node's
//! protocol surface; capping the write side would be a *new* protocol-level
//! nesting cap, which the standing decision in §7.3 forbids. Everything here is
//! `#[test]`-only.
//!
//! ## The arithmetic, and why it is per envelope
//!
//! The recursive cycle is `Par → Expr → EList → Par`
//! (`RhoTypes.proto`: `Par.exprs = 5`, `Expr.e_list_body = 20`, `EList.ps = 1`),
//! so **one bracket costs three nested-message levels**. The innermost `Par`
//! spends one more level on the leaf `Expr` that holds the ground value. An
//! envelope that transports the `Par` inside other messages spends its own
//! levels first. Writing `W(E)` for the number of nested-message levels an
//! envelope `E` interposes before the outermost `Par` is entered, the last
//! depth that decodes is
//!
//! ```math
//! D_{\max}(E) \;=\; \left\lfloor \frac{L - 1 - W(E)}{3} \right\rfloor,
//! \qquad L = 100 .
//! ```
//!
//! ⚠ **The wrapped ceilings below are measured, not derived by subtracting one
//! from the bare one.** The closed form is asserted *against* the measurement
//! by [`the_read_ceiling_follows_the_envelope_arithmetic`], so a transcription
//! error in either direction fails the test rather than propagating.
//!
//! Because the bracket costs 3 and `W` is small, several distinct envelopes
//! share a ceiling while others do not — the register in
//! [`READ_CEILING_ENVELOPES`] carries **three** different values (33, 32, 31)
//! over nine real production envelopes. Any statement of this ceiling as *one
//! number* is wrong for at least two of them.
//!
//! ## ★ The envelope with the *highest* ceiling is the consensus-class one
//!
//! `ProduceEventProto.outputValue` is `repeated bytes`
//! (`CasperMessage.proto:393`), not a nested message. A block body therefore
//! carries those `Par`s as **opaque byte strings**: decoding the block does not
//! descend into them and cannot fail on them. The failure is deferred to the
//! point where the bytes are read as a `Par` — `reduce.rs:1065`, on the
//! **validator's replay pass** — and because that is a fresh top-level
//! `Par::decode`, it gets the full budget and sits at the **bare** ceiling,
//! `W = 0`, 33/34.
//!
//! The executable form of that consensus fault is
//! `rholang/tests/replay_output_value_depth_ceiling.rs`, which drives the same
//! bytes through the real reducer and shows red on replay and green on play.
//! It lives there and not here because `models` is *below* `rholang` in the
//! dependency order — a `rholang` dev-dependency here would be a cycle.
//!
//! ## ★★ The sibling ceiling is GONE, and the "they move together" claim was
//! ## TOO STRONG — measured, not reasoned
//!
//! `models/src/rust/canonical_path.rs` used to set `COLLECTION_DEPTH_LIMIT = 32`
//! on the EPathMap trie-key decoder, anchored in its own module documentation to
//! *"today's effective prost envelope"*. This file used to conclude from that
//! anchoring: *"raising one and leaving the other moves the binding constraint by
//! one level and fixes nothing — they move together or not at all."*
//!
//! ⚠ **That conclusion holds on one transport path and is false on the other**,
//! and the difference is which protobuf field carries the value:
//!
//! | how a deep `Par` reaches a reader | prost message levels spent | binding constraint |
//! |---|---|---|
//! | nested `Par` / `EPathMap` tag 1 (`ps`, `repeated Par`) | 3 per bracket | **prost**, at 33 |
//! | ★ `EPathMap` tag 8 (`serialized_paths`, `bytes`) | **0** — opaque payload | `COLLECTION_DEPTH_LIMIT` **alone** |
//! | `ProduceEventProto.outputValue` (`repeated bytes`) | **0** — opaque payload | the fresh `Par::decode`, at 33 |
//!
//! A `bytes` field is not descended into: `prost::encoding::bytes::merge` reads a
//! varint length and copies that many bytes. `models/tests/
//! epathmap_tag8_read_totality.rs` exhibits a **depth-400** trie key whose
//! envelope was refused by the *trie codec's* limit and **not** by prost's — so
//! on that path there was no second constraint behind the cap, and lifting it
//! alone bought the whole depth range rather than nothing.
//!
//! ⇒ `COLLECTION_DEPTH_LIMIT`, `SCANNER_STACK_CEILING`, `enter_collection` and
//! `CodecError::DepthLimitExceeded` are **deleted**, not raised, per the standing
//! owner ruling (2026-07-29) that there is no artificial depth cap for consensus.
//! [`the_trie_reader_is_total_and_prost_is_the_only_remaining_ceiling`] replaces
//! the anchoring test and holds the *corrected* relationship by execution: the
//! trie reader is total, prost's ceiling is unchanged, and the two are therefore
//! now **independent** — which is a statement that can go red in either direction.
//!
//! ## Shape
//!
//! [`depth_34_builds_and_writes_and_does_not_read`] is staged so that a failure
//! says *which* thing broke:
//!
//! | stage | what a failure there means |
//! |---|---|
//! | **build** | the fixture could not construct the term — a fixture bug, not a finding |
//! | **write** | the *encoder* refused — the defect INVERTED; something capped the write side |
//! | **read** | ✅ the decoder refused — the defect, and only this |
//!
//! The read stage matches the error **kind**, not merely `is_err()`: a bare
//! `is_err()` also passes on a truncated buffer or a wrong field number, and
//! would prove nothing. The depth-33 control runs in the **same body, same
//! process, same run**, immediately adjacent, so the boundary is *exhibited*
//! rather than asserted as a constant. That is the shape
//! `canonical_path.rs::depth_limit_decode_rejects_beyond_32` already uses for
//! the sibling ceiling, mirrored here.

use models::casper::v1::{
    continuation_at_name_response, rho_data_response, ContinuationAtNamePayload,
    ContinuationAtNameResponse, RhoDataPayload, RhoDataResponse,
};
use models::casper::{
    ContinuationsWithBlockInfo, DataAtNameByBlockQuery, DataWithBlockInfo, WaitingContinuationInfo,
};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::canonical_path::{decode_trie_path, encode_trie_path};
use models::rust::utils::new_gint_par;
use prost::Message;

// ---------------------------------------------------------------------------
// the constants this file pins, and where they come from
// ---------------------------------------------------------------------------

/// `prost`'s nested-message recursion budget. Private in the crate
/// (`prost-0.14.3/src/lib.rs:30`, no `pub`), so it cannot be imported and is
/// restated here; [`the_read_ceiling_follows_the_envelope_arithmetic`] checks
/// the restatement against nine measured boundaries, which is what stops it
/// from being a transcription.
const PROST_RECURSION_LIMIT: usize = 100;

/// Nested-message levels one bracket of `[[…]]` costs:
/// `Par.exprs → Expr.e_list_body → EList.ps`.
const LEVELS_PER_BRACKET: usize = 3;

/// The one extra level the innermost `Par` spends on the leaf `Expr` that
/// carries the ground value.
const LEAF_EXPR_LEVELS: usize = 1;

/// ★ **The read ceiling, PER ENVELOPE.** Each row is
/// `(name, W = interposed message levels, last depth that decodes)`.
///
/// Every row is *executed* by [`the_read_ceiling_is_pinned_per_envelope`] at
/// both `d` and `d + 1`, so no entry can be stale without failing.
///
/// ⚠ Three distinct ceilings appear here. The `listenForContinuationAtName`
/// egress envelope accepts **31**, two levels below the bare `Par` — pinning
/// this ceiling as a single number would silently over-state that endpoint's
/// capacity by two nesting levels.
const READ_CEILING_ENVELOPES: &[(&str, usize, usize)] = &[
    // ★★ The consensus-class member. `ProduceEventProto.outputValue` is
    // `repeated bytes`, so this is a FRESH top-level decode with the full
    // budget — the replay decode at `reduce.rs:1065` and the system-contract
    // decode at `contract_call.rs:90` both land here.
    ("Par (bare) — the replay decode of ProduceEventProto.outputValue", 0, 33),
    // gRPC ingress: `getDataAtName`'s request. Fails safe, and is correct as is.
    ("DataAtNameByBlockQuery.par — getDataAtName ingress", 1, 32),
    ("DataWithBlockInfo.postBlockData[0]", 1, 32),
    ("RhoDataPayload.par[0]", 1, 32),
    ("WaitingContinuationInfo.postBlockContinuation", 1, 32),
    // gRPC egress: `getDataAtName`'s response.
    ("RhoDataResponse > Payload > par[0] — getDataAtName egress", 2, 32),
    ("ContinuationsWithBlockInfo > WCI > postBlockContinuation", 2, 32),
    ("ContinuationAtNamePayload > CWBI > WCI > postBlockContinuation", 3, 32),
    // ★ gRPC egress: `listenForContinuationAtName`'s response — the LOWEST
    // ceiling in the inventory, and the reason this register is a table.
    (
        "ContinuationAtNameResponse > .. > postBlockContinuation — listenForContinuationAtName egress",
        4,
        31,
    ),
];

// ---------------------------------------------------------------------------
// term construction — ITERATIVE, so the builder is never the constraint
// ---------------------------------------------------------------------------

fn elist(ps: Vec<Par>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

/// `[[[…[0]…]]]` with `depth` bracket levels. Built by a loop, so a deep
/// fixture can never abort the builder and be mistaken for the decoder's
/// refusal.
fn nested_list(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = elist(vec![p]);
    }
    p
}

/// Bracket levels actually present in a `Par`. Iterative, for the same reason.
fn par_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        match cur.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EListBody(l)) if !l.ps.is_empty() => {
                n += 1;
                cur = &l.ps[0];
            }
            _ => return n,
        }
    }
}

/// The error-KIND discriminator, and it is load-bearing.
///
/// `prost`'s `Debug for DecodeError` prints
/// `DecodeError { description: <kind>, stack: [(message, field), ..] }`, and
/// `DecodeErrorKind` derives `Debug`. Matching on `description: ` therefore
/// matches the *root cause* field and cannot be satisfied by anything in the
/// location stack, whose entries are protobuf message and field names.
///
/// A bare `is_err()` in its place would pass on a truncated buffer, a wrong
/// field number, or a non-minimal varint — every one of which is a fixture bug
/// wearing the finding's clothes.
fn is_recursion_limit(e: &prost::DecodeError) -> bool {
    let debug = format!("{e:?}");
    let display = format!("{e}");
    debug.contains("description: RecursionLimitReached")
        && display.ends_with("recursion limit reached")
}

// ---------------------------------------------------------------------------
// ★★ THE FIXTURE — three stages, and the adjacent control
// ---------------------------------------------------------------------------

/// ★★ **The defect, staged.**
///
/// Depth 34 is built, is written, and is not read — and the depth-33 control
/// immediately below it is built, written, read, and *round-trips*, in the same
/// body and the same process. The boundary is exhibited by the pair; neither
/// half asserts it alone.
#[test]
fn depth_34_builds_and_writes_and_does_not_read() {
    // ── stage 1 — BUILD. Asserted FIRST: a failure here is a fixture bug. ──
    let over = nested_list(34);
    assert_eq!(
        par_depth(&over),
        34,
        "STAGE 1 (build): the fixture term does not carry 34 bracket levels, so \
         nothing measured below is about depth 34. This is a fixture defect, not \
         a finding about the codec."
    );

    // ── stage 2 — WRITE. The write side must NOT be capped. ──
    let bytes = over.encode_to_vec();
    assert!(
        bytes.len() >= 34,
        "STAGE 2 (write): `encode_to_vec` produced {} bytes for a depth-34 term — \
         fewer than one byte per bracket. The encoder has been bounded. That is \
         the defect INVERTED: `prost` places no limit on the write side, this \
         file forbids adding one, and a short buffer here would make stage 3's \
         `Err` meaningless (a truncated buffer fails to decode for the wrong \
         reason).",
        bytes.len()
    );

    // ── stage 3 — READ. THE defect, and only this. ──
    let err = match Par::decode(&bytes[..]) {
        Ok(decoded) => panic!(
            "STAGE 3 (read): a depth-{} `Par` DECODED. The `prost` recursion \
             ceiling this file exists to surface has moved. If that was \
             deliberate it is a consensus-visible widening of the set of byte \
             strings this node accepts, and it needs a coordinated version bump \
             — see the audit, §7.3.",
            par_depth(&decoded)
        ),
        Err(e) => e,
    };
    assert!(
        is_recursion_limit(&err),
        "STAGE 3 (read): decode failed, but NOT on the recursion limit — {err:?}. \
         The fixture is producing malformed bytes and proving nothing about the \
         ceiling."
    );

    // ── the CONTROL, adjacent: depth 33, same body, same process, same run. ──
    let under = nested_list(33);
    assert_eq!(
        par_depth(&under),
        33,
        "control (build): the depth-33 control term does not carry 33 bracket levels"
    );
    let under_bytes = under.encode_to_vec();
    assert!(
        under_bytes.len() >= 33,
        "control (write): the depth-33 control encoded to {} bytes",
        under_bytes.len()
    );
    let decoded = Par::decode(&under_bytes[..]).unwrap_or_else(|e| {
        panic!(
            "control (read): depth 33 must DECODE — it is the last depth that \
             does. It failed with {e:?}, which means the ceiling moved DOWN and \
             this node now rejects byte strings it used to accept. That is a \
             consensus-visible narrowing."
        )
    });
    assert_eq!(
        par_depth(&decoded),
        33,
        "control (read): depth 33 decoded, but not to depth 33 — the round trip \
         lost nesting, so the control is not the term the boundary is about"
    );
    assert_eq!(
        decoded.encode_to_vec(),
        under_bytes,
        "control (read): the depth-33 round trip is not a byte-level fixed point"
    );

    println!(
        "  bare `Par`: depth 33 round-trips ({} bytes), depth 34 encodes ({} bytes) \
         and decode-rejects with RecursionLimitReached",
        under_bytes.len(),
        bytes.len()
    );
}

// ---------------------------------------------------------------------------
// ★ the ENVELOPE leg — the ceiling is per envelope, and that is executed
// ---------------------------------------------------------------------------

/// Wrap a term in envelope row `index` of [`READ_CEILING_ENVELOPES`] and return
/// `Ok(())` if the envelope decodes, `Err(kind_is_recursion_limit)` if not.
///
/// One `match` over the row index rather than nine near-identical tests,
/// because the register and the driver must not be able to disagree about which
/// row is which.
fn envelope_round_trip(index: usize, term: Par) -> Result<(), bool> {
    fn wci(t: Par) -> WaitingContinuationInfo {
        WaitingContinuationInfo {
            post_block_patterns: vec![],
            post_block_continuation: Some(t),
        }
    }
    fn cwbi(t: Par) -> ContinuationsWithBlockInfo {
        ContinuationsWithBlockInfo {
            post_block_continuations: vec![wci(t)],
            block: None,
        }
    }
    fn canp(t: Par) -> ContinuationAtNamePayload {
        ContinuationAtNamePayload {
            block_results: vec![cwbi(t)],
            length: 0,
        }
    }
    fn verdict<M: Message + Default>(bytes: Vec<u8>) -> Result<(), bool> {
        match M::decode(&bytes[..]) {
            Ok(_) => Ok(()),
            Err(e) => Err(is_recursion_limit(&e)),
        }
    }
    match index {
        0 => verdict::<Par>(term.encode_to_vec()),
        1 => verdict::<DataAtNameByBlockQuery>(
            DataAtNameByBlockQuery {
                par: Some(term),
                block_hash: String::new(),
                use_pre_state_hash: false,
            }
            .encode_to_vec(),
        ),
        2 => verdict::<DataWithBlockInfo>(
            DataWithBlockInfo {
                post_block_data: vec![term],
                block: None,
            }
            .encode_to_vec(),
        ),
        3 => verdict::<RhoDataPayload>(
            RhoDataPayload {
                par: vec![term],
                block: None,
            }
            .encode_to_vec(),
        ),
        4 => verdict::<WaitingContinuationInfo>(wci(term).encode_to_vec()),
        5 => verdict::<RhoDataResponse>(
            RhoDataResponse {
                message: Some(rho_data_response::Message::Payload(RhoDataPayload {
                    par: vec![term],
                    block: None,
                })),
            }
            .encode_to_vec(),
        ),
        6 => verdict::<ContinuationsWithBlockInfo>(cwbi(term).encode_to_vec()),
        7 => verdict::<ContinuationAtNamePayload>(canp(term).encode_to_vec()),
        8 => verdict::<ContinuationAtNameResponse>(
            ContinuationAtNameResponse {
                message: Some(continuation_at_name_response::Message::Payload(canp(term))),
            }
            .encode_to_vec(),
        ),
        other => panic!(
            "par_prost_depth_ceiling: envelope row {other} has no driver. Every row \
             in READ_CEILING_ENVELOPES must be executable, or the register is prose."
        ),
    }
}

/// ★★ **The boundary, pinned per envelope, by execution.**
///
/// For every row: the recorded depth decodes, and one deeper does not — and
/// fails on the recursion limit specifically. Adjacent, in one run, exactly as
/// [`depth_34_builds_and_writes_and_does_not_read`] does for the bare case.
#[test]
fn the_read_ceiling_is_pinned_per_envelope() {
    assert!(
        !READ_CEILING_ENVELOPES.is_empty(),
        "the envelope register is empty — the test would be vacuously green"
    );
    for (index, (name, w, d_max)) in READ_CEILING_ENVELOPES.iter().enumerate() {
        let ok = nested_list(*d_max);
        assert_eq!(par_depth(&ok), *d_max, "fixture: `{name}` at depth {d_max}");
        envelope_round_trip(index, ok).unwrap_or_else(|kind| {
            panic!(
                "`{name}` (W={w}) no longer decodes at its recorded ceiling of \
                 {d_max} (recursion-limit kind: {kind}). The ceiling moved DOWN: \
                 this node now rejects byte strings it used to accept."
            )
        });

        let over = nested_list(d_max + 1);
        assert_eq!(par_depth(&over), d_max + 1, "fixture: `{name}` at depth {}", d_max + 1);
        match envelope_round_trip(index, over) {
            Ok(()) => panic!(
                "`{name}` (W={w}) DECODED at depth {}, one past its recorded \
                 ceiling of {d_max}. The ceiling moved UP: this node now accepts \
                 byte strings it used to reject, which is a consensus-visible \
                 widening and needs a coordinated version bump — see the audit, \
                 §7.3.",
                d_max + 1
            ),
            Err(true) => {}
            Err(false) => panic!(
                "`{name}` (W={w}) rejected depth {}, but not on the recursion \
                 limit. The fixture is producing malformed bytes for this \
                 envelope and proving nothing about its ceiling.",
                d_max + 1
            ),
        }
    }

    let distinct: std::collections::BTreeSet<usize> =
        READ_CEILING_ENVELOPES.iter().map(|(_, _, d)| *d).collect();
    assert!(
        distinct.len() >= 3,
        "the register carries only {} distinct ceiling(s) ({distinct:?}). This \
         test exists to refute 'the ceiling is one number'; with fewer than three \
         distinct values over the enumerated envelopes it no longer does.",
        distinct.len()
    );
    println!(
        "  {} envelopes, {} distinct ceilings: {:?}",
        READ_CEILING_ENVELOPES.len(),
        distinct.len(),
        distinct
    );
}

/// The closed form and the measurement must agree, in both directions.
///
/// The register above could be transcribed wrong and
/// [`the_read_ceiling_is_pinned_per_envelope`] would still pass — it would just
/// be pinning the wrong boundary consistently. This test closes that: each
/// row's recorded ceiling must equal the arithmetic
/// `⌊(L − 1 − W)/3⌋`, so a wrong `W` or a wrong depth cannot survive together.
#[test]
fn the_read_ceiling_follows_the_envelope_arithmetic() {
    for (name, w, d_max) in READ_CEILING_ENVELOPES {
        let predicted =
            (PROST_RECURSION_LIMIT - LEAF_EXPR_LEVELS - w) / LEVELS_PER_BRACKET;
        assert_eq!(
            predicted, *d_max,
            "`{name}`: recorded ceiling {d_max}, but ⌊({PROST_RECURSION_LIMIT} − \
             {LEAF_EXPR_LEVELS} − {w})/{LEVELS_PER_BRACKET}⌋ = {predicted}. Either \
             the recorded depth or the recorded envelope level count W is wrong; \
             the measurement in `the_read_ceiling_is_pinned_per_envelope` says \
             which."
        );
    }

    // The budget really is spent three levels per bracket plus one for the leaf:
    // at the bare ceiling the outermost decode consumes exactly `L`.
    let bare = READ_CEILING_ENVELOPES
        .iter()
        .find(|(_, w, _)| *w == 0)
        .expect("the register must carry the bare `Par` case");
    assert_eq!(
        LEVELS_PER_BRACKET * bare.2 + LEAF_EXPR_LEVELS,
        PROST_RECURSION_LIMIT,
        "the bare ceiling of {} does not exhaust the {PROST_RECURSION_LIMIT}-level \
         budget exactly, so the per-bracket cost of {LEVELS_PER_BRACKET} levels no \
         longer describes `Par → Expr → EList → Par`",
        bare.2
    );
}

// ---------------------------------------------------------------------------
// ★★ the sibling ceiling is GONE — and the two are now INDEPENDENT
// ---------------------------------------------------------------------------

/// ★★ **The trie-key reader is TOTAL in depth; `prost` is the only read ceiling
/// left — and the two are now independent, which is a claim that can go red in
/// either direction.**
///
/// # What this replaces, and why the replacement is a correction
///
/// The retired `the_two_read_ceilings_are_anchored_together` asserted that
/// `COLLECTION_DEPTH_LIMIT = 32` and prost's `RECURSION_LIMIT = 100` coincide on
/// the nested-list shape, and concluded: *"Raise the prost ceiling alone and the
/// trie decoder becomes the binding constraint one level lower; raise
/// `COLLECTION_DEPTH_LIMIT` alone and prost becomes it. Either move buys nothing.
/// They move together or not at all."*
///
/// The **coincidence** was real and is re-measured below. The **conclusion** was
/// too strong, and the counter-example is a transport path the test never drove:
/// a ground `EPathMap` writes its entries as proto field 8,
/// `serialized_paths`, of type `bytes`. A `bytes` field is opaque to protobuf —
/// `prost::encoding::bytes::merge` reads a varint length and copies that many
/// bytes — so the trie-key stream inside costs **zero** nested-message levels and
/// prost's ceiling never engages. On that path `COLLECTION_DEPTH_LIMIT` was the
/// only constraint standing between a writer that is total by requirement (R3F-2)
/// and a reader that refused at 33. `models/tests/epathmap_tag8_read_totality.rs`
/// carries the depth-400 measurement.
///
/// It is the same structure as `ProduceEventProto.outputValue`
/// (`repeated bytes`), which this file's own header already relies on to explain
/// why the *highest* ceiling is the consensus-class one.
///
/// # The three things asserted here, and what each failure direction means
///
/// | # | assertion | a failure means |
/// |---|---|---|
/// | 1 | the trie reader accepts every depth in a probe range 12× the retired cap, and each is a byte-level fixed point | the cap is back, or a level is being dropped |
/// | 2 | prost still refuses one past its own ceiling, on the recursion limit | prost's ceiling moved without this file noticing |
/// | 3 | at the *retired* boundary the two used to coincide — re-measured, so the historical claim stays checkable | the coincidence was misremembered |
///
/// ⚠ Assertion 2 is the standing residual: **the prost read ceiling is NOT lifted
/// by this change.** Lifting it needs an unbounded protobuf reader for the whole
/// `Par` family, which is a separate deliverable; what is lifted here is the
/// ceiling that had no second constraint behind it.
#[test]
fn the_trie_reader_is_total_and_prost_is_the_only_remaining_ceiling() {
    /// The retired cap, kept as a NUMBER in exactly one place so the historical
    /// coincidence stays measurable after the constant it came from is gone.
    const RETIRED_COLLECTION_DEPTH_LIMIT: usize = 32;
    /// Twelve times the retired cap: reaching it means no limit was hit.
    const PROBE_CEILING: usize = 384;

    let bare_ceiling = READ_CEILING_ENVELOPES
        .iter()
        .find(|(_, w, _)| *w == 0)
        .expect("the register must carry the bare `Par` case")
        .2;

    // ── 1. THE TRIE READER IS TOTAL ───────────────────────────────────────────
    //
    // Searched, not transcribed: a constant cannot distinguish "total" from
    // "capped very high", and the whole point of the change is which of those
    // this is.
    let mut last_accepting = 0usize;
    for wrappers in 1..=PROBE_CEILING {
        let bytes = encode_trie_path(&nested_list(wrappers));
        assert!(
            !bytes.is_empty(),
            "non-vacuity: the trie ENCODER is documented TOTAL (R3F-2) and produced \
             nothing at {wrappers} wrappers"
        );
        match decode_trie_path(&bytes) {
            Ok(back) => {
                assert_eq!(
                    encode_trie_path(&back),
                    bytes,
                    "{wrappers} wrappers decoded but is not a byte-level fixed point — a \
                     level was dropped, which is worse than a refusal"
                );
                last_accepting = wrappers;
            }
            Err(e) => panic!(
                "★ the trie reader refused {wrappers} wrappers with {e:?}; it accepts \
                 {last_accepting}. The writer produced {} bytes at that depth, so a bound \
                 here means this node emits proto field-8 `serialized_paths` byte strings \
                 it will not read back. The retired cap was \
                 {RETIRED_COLLECTION_DEPTH_LIMIT} counted levels \
                 (= {} wrappers); if this stopped there, the cap is back.",
                bytes.len(),
                RETIRED_COLLECTION_DEPTH_LIMIT + 1
            ),
        }
    }
    assert_eq!(
        last_accepting, PROBE_CEILING,
        "the trie reader must be total across the whole probe range"
    );

    // ── 2. PROST'S CEILING IS UNCHANGED — the standing residual ───────────────
    let at_prost_limit = nested_list(bare_ceiling);
    let prost_bytes = at_prost_limit.encode_to_vec();
    let prost_back = Par::decode(&prost_bytes[..]).unwrap_or_else(|e| {
        panic!(
            "{bare_ceiling} wrappers must still decode through prost — that is the \
             register's own bare-`Par` ceiling. It failed with {e:?}."
        )
    });
    assert_eq!(
        par_depth(&prost_back),
        bare_ceiling,
        "the prost round trip at its own ceiling lost nesting"
    );

    let past_prost = nested_list(bare_ceiling + 1);
    let past_prost_err = Par::decode(&past_prost.encode_to_vec()[..]).expect_err(
        "★ prost ACCEPTED one past its recorded ceiling. Either the ceiling moved (which \
         is the deliverable this change does NOT contain) or the register row is wrong.",
    );
    assert!(
        is_recursion_limit(&past_prost_err),
        "prost refused one past its ceiling, but not on the recursion limit: \
         {past_prost_err:?}"
    );

    // ── 3. AND THE TRIE READER NOW ACCEPTS WHAT PROST STILL REFUSES ───────────
    //
    // This is the independence, stated as the one observation that used to be
    // impossible: the same term, accepted by one reader and refused by the other.
    let trie_bytes_past_prost = encode_trie_path(&past_prost);
    assert!(
        decode_trie_path(&trie_bytes_past_prost).is_ok(),
        "★ the trie reader must now accept a term prost refuses — that IS the \
         independence. If it refuses too, the two ceilings are still coupled."
    );

    // ── the historical coincidence, re-measured so it stays checkable ─────────
    assert_eq!(
        RETIRED_COLLECTION_DEPTH_LIMIT + 1,
        bare_ceiling,
        "the historical claim was that the retired trie cap and the bare prost ceiling \
         coincided at {} nested-list wrappers. They no longer constrain each other, but \
         the coincidence is a matter of record and this is where it is checked.",
        bare_ceiling
    );

    println!(
        "  trie reader: TOTAL (accepted every depth up to {PROBE_CEILING}); prost: \
         unchanged at {bare_ceiling} wrappers, RecursionLimitReached at {}. The two \
         coincided at {bare_ceiling} before the cap was retired and are now independent.",
        bare_ceiling + 1
    );
}
