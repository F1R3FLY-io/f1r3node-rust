//! Work item #124 — **THE BYTE-IDENTITY OBLIGATION OF THE EVENT-HASH LEG
//! CONVERSION**, settled by differential BEFORE the conversion is applied.
//!
//! # What is being decided
//!
//! `models::rust::spliced_event_bytes` opens each of the three event-hash legs
//! (datum, pattern, continuation) with a `contains_par` scan and, when the scan
//! answers `false` — the 95.43 % production case — returns
//!
//! ```ignore
//! fn direct<T: Serialize>(value: &T) -> Vec<u8> {
//!     bincode::serialize(value).expect("…")
//! }
//! ```
//!
//! i.e. the **derived** `Serialize`, which is Θ(depth) in native stack (measured
//! at 3,040 B/level debug and 160 B/level release by
//! `casper/tests/event_hash_leg_depth_probe.rs`). The proposed repair is to call
//! [`ColdStoreEncode::cold_encode`] instead — the single-walk trampolined
//! encoder, measured **flat** on the same fixtures by the same instrument.
//!
//! ⚠ **THIS IS AN EVENT-HASH PATH.** Event hashes enter the block Merkle log, so
//! if `cold_encode` and `bincode::serialize` disagree on so much as one byte for
//! one reachable value, the repair moves consensus bytes and post-state hashes
//! and is NOT a quiet conversion. The question therefore has to be *settled*, and
//! settled by observation over a corpus — not by reading the two implementations
//! and agreeing with oneself.
//!
//! # The obligation, stated exactly
//!
//! For every value `v` of each of the three leg root types
//! (`ListParWithRandom`, `BindPattern`, `TaggedContinuation`):
//!
//! ```text
//!     v.cold_encode()  ==  bincode::serialize(&v)
//! ```
//!
//! The right-hand side is *definitionally* what `direct(&v)` returns
//! (`spliced_event_bytes.rs:131`), so this equality IS "the conversion is
//! consensus-inert", with no inference step in between.
//!
//! # Why this file exists when two differentials already do
//!
//! | existing gate | what it covers | why it is not sufficient here |
//! |---|---|---|
//! | `bincode_encoder_differential` | `encode()` vs derived, exhaustive over `ExprInstance` / `ConnectiveInstance` arms | its roots are exercised through a corpus built for the **cold store**; it never asks the question at the *leg* boundary, and carries no **unfilled-cell** `EPathMap` at the three roots |
//! | `epathmap_spliced_event_bytes` | leg output vs derived, over filled/unfilled `EPathMap` shapes | it pins the **spliced** emitter; before the conversion its map-free arms exercise `direct()` = the oracle, so it is *vacuous* as evidence about `cold_encode` |
//!
//! ★ The gap both leave is the same one: **the value that actually reaches
//! `direct()` in production is a map-free-or-unfilled-cell term at a leg root**,
//! and neither gate compares *that* against `cold_encode`. This file does, and it
//! is written so that it is meaningful both before the conversion (it is then a
//! prediction) and after it (it is then the standing regression).
//!
//! # An `EPathMap` CAN reach `direct()`
//!
//! ⚠ This is the subtlety that makes an "it is all map-free, so who cares" reading
//! wrong. `contains_par` answers `true` only for a **filled** intern cell
//! (`EPathMap::interned_handle()`); it descends *unfilled* maps and returns
//! `false` if it finds no filled one. So a term densely populated with `EPathMap`s
//! whose cells have simply never been interned takes the `direct()` early return —
//! and after the conversion, `cold_encode` will be encoding those maps. §2 is
//! built around exactly that value class, and §5 refuses to let it be dropped.

mod fixtures;
mod par_corpus;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, EList, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation, Var,
    var::VarInstance,
};
use models::rust::rholang::bincode_encoder::ColdStoreEncode;
use models::rust::spliced_event_bytes::{
    event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
    event_hash_bytes_tagged_continuation,
};
use models::rust::test_utils::test_utils::generate_par;
use par_corpus as corpus;
use proptest::prelude::*;
use serde::Serialize;

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par, epathmap_remainder_connective,
    ezipper_value, nested_epathmap_value,
};

// ═══════════════════════════════════════════════════════════════════════════
// §0  The verdict, separated from both subjects
// ═══════════════════════════════════════════════════════════════════════════

/// The write-side verdict as a pure function of two observations.
///
/// ★ Kept apart from the encoders so that [`the_identity_differential_can_go_red`]
/// can hand it bytes that no encoder in this repo produced. A verdict welded to
/// its subject can only ever be shown observations the subject generated, every
/// one of which it agrees with by construction — which is a tautology wearing a
/// test's clothes.
fn identity_verdict(label: &str, machine: &[u8], oracle: &[u8]) -> Result<(), String> {
    if machine == oracle {
        return Ok(());
    }
    let first = machine
        .iter()
        .zip(oracle.iter())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| machine.len().min(oracle.len()));
    let window = |b: &[u8]| -> String {
        let lo = first.saturating_sub(8);
        let hi = (first + 8).min(b.len());
        b[lo..hi]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    Err(format!(
        "★ CONSENSUS-MOVING DIVERGENCE for `{label}`.\n\
         `cold_encode` and the derived `bincode::serialize` disagree, so routing \
         the event-hash leg through the trampolined encoder would change event \
         hashes, block Merkle roots and post-state hashes.\n\
         \n  lengths : machine {} B, oracle {} B\
         \n  first Δ : byte {first}\
         \n  machine : {}\
         \n  oracle  : {}\n\
         \n⚠ STOP. This is not a stack-safety repair; it is a consensus change and \
         needs a register entry and an owner ruling.",
        machine.len(),
        oracle.len(),
        window(machine),
        window(oracle),
    ))
}

/// The obligation, for one value: `cold_encode` == the derived encoder.
fn assert_cold_encode_is_direct<T>(label: &str, value: &T)
where
    T: ColdStoreEncode + Serialize,
{
    // `direct()` in `spliced_event_bytes` IS this call. Spelled out rather than
    // referenced so the oracle cannot silently follow the subject if the
    // production helper is later edited.
    let oracle = bincode::serialize(value).expect("oracle: derived bincode must encode");
    let machine = value.cold_encode();
    if let Err(why) = identity_verdict(label, &machine, &oracle) {
        panic!("{why}");
    }
}

/// The **leg-level** invariant: whatever branch the emitter takes, its bytes are
/// the derived encoder's bytes. True before the conversion and required after it.
fn assert_leg_is_derived_datum(label: &str, value: &ListParWithRandom) {
    let oracle = bincode::serialize(value).expect("oracle");
    let leg = event_hash_bytes_list_par_with_random(value);
    if let Err(why) = identity_verdict(&format!("leg/datum::{label}"), &leg, &oracle) {
        panic!("{why}");
    }
}

fn assert_leg_is_derived_pattern(label: &str, value: &BindPattern) {
    let oracle = bincode::serialize(value).expect("oracle");
    let leg = event_hash_bytes_bind_pattern(value);
    if let Err(why) = identity_verdict(&format!("leg/pattern::{label}"), &leg, &oracle) {
        panic!("{why}");
    }
}

fn assert_leg_is_derived_continuation(label: &str, value: &TaggedContinuation) {
    let oracle = bincode::serialize(value).expect("oracle");
    let leg = event_hash_bytes_tagged_continuation(value);
    if let Err(why) = identity_verdict(&format!("leg/continuation::{label}"), &leg, &oracle) {
        panic!("{why}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  Fixture construction — ITERATIVE, and map-free by default
// ═══════════════════════════════════════════════════════════════════════════

fn expr_par(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn elist(ps: Vec<Par>) -> Par {
    expr_par(ExprInstance::EListBody(EList {
        ps,
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    }))
}

/// `[[[…[0]…]]]` with `depth` bracket levels, built **iteratively** so the
/// fixture's own construction can never be what a deep case is measuring.
fn nested_list(depth: usize) -> Par {
    let mut par = expr_par(ExprInstance::GInt(0));
    for _ in 0..depth {
        par = elist(vec![par]);
    }
    par
}

fn datum_of(pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        random_state: vec![0xAA; 32],
    }
}

fn pattern_of(patterns: Vec<Par>) -> BindPattern {
    BindPattern {
        patterns,
        remainder: Some(Var {
            var_instance: Some(VarInstance::FreeVar(3)),
        }),
        free_count: 3,
    }
}

fn continuation_of(body: Par) -> TaggedContinuation {
    TaggedContinuation {
        guard: Some(expr_par(ExprInstance::GBool(true))),
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body),
            random_state: vec![0x5A; 16],
        })),
    }
}

/// Every `EPathMap` shape the fixtures module offers, **with the intern cell left
/// UNFILLED** — the value class that reaches `direct()` today and `cold_encode`
/// after the conversion.
///
/// ⚠ `intern()` is deliberately never called here. A filled cell would send the
/// value down the *spliced* spine, which is a different subject; §3 covers that
/// side separately and on purpose.
fn unfilled_map_pars() -> Vec<(&'static str, Par)> {
    vec![
        ("e6a-index", epathmap_par(e6a_index_epathmap())),
        ("nested", epathmap_par(nested_epathmap_value())),
        (
            "locally-free-entries",
            epathmap_par(epathmap_locally_free_entries()),
        ),
        (
            "remainder-connective",
            epathmap_par(epathmap_remainder_connective()),
        ),
        (
            "ezipper",
            expr_par(ExprInstance::EZipperBody(ezipper_value())),
        ),
        ("nonground", epathmap_par(corpus::nonground_pathmap())),
        ("ground", epathmap_par(corpus::ground_pathmap())),
    ]
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  THE OBLIGATION over the shared corpora and the unfilled-cell class
// ═══════════════════════════════════════════════════════════════════════════

/// `cold_encode` == derived, over the corpora the cold-store codec is already
/// specified against, at each of the three **leg root** types.
///
/// ★ The three root corpora are small (12 values between them — they vary the
/// *root* fields: `random_state`, `remainder`, `free_count`, the `TaggedCont`
/// arms). The per-arm breadth lives in `par_corpus()`, which carries one `Par`
/// per `ExprInstance` and per `ConnectiveInstance` arm; those are **lifted into
/// all three roots** here rather than tested as bare `Par`s, because a bare `Par`
/// is not a value any of the three legs is ever handed. The lift is what makes
/// this a differential about the *legs* and not about `Par`.
#[test]
fn cold_encode_is_byte_identical_to_direct_over_the_shared_corpora() {
    let mut root_cases = 0usize;
    for (label, v) in corpus::list_par_with_random_corpus() {
        assert_cold_encode_is_direct(&format!("ListParWithRandom::{label}"), &v);
        root_cases += 1;
    }
    for (label, v) in corpus::bind_pattern_corpus() {
        assert_cold_encode_is_direct(&format!("BindPattern::{label}"), &v);
        root_cases += 1;
    }
    for (label, v) in corpus::tagged_continuation_corpus() {
        assert_cold_encode_is_direct(&format!("TaggedContinuation::{label}"), &v);
        root_cases += 1;
    }

    // Every `ExprInstance` / `ConnectiveInstance` arm, lifted to each root.
    let mut lifted = 0usize;
    for (label, par) in corpus::par_corpus() {
        let d = datum_of(vec![par.clone()]);
        assert_cold_encode_is_direct(&format!("lift/datum::{label}"), &d);
        assert_leg_is_derived_datum(&label, &d);

        let bp = pattern_of(vec![par.clone()]);
        assert_cold_encode_is_direct(&format!("lift/pattern::{label}"), &bp);
        assert_leg_is_derived_pattern(&label, &bp);

        let k = continuation_of(par);
        assert_cold_encode_is_direct(&format!("lift/continuation::{label}"), &k);
        assert_leg_is_derived_continuation(&label, &k);
        lifted += 3;
    }

    // ★ ANTI-VACUITY, and the floors are stated separately because they fail for
    // different reasons. A differential over an empty corpus reports a
    // comfortable pass, and both corpora live in a file this one does not own.
    //
    // ⚠ The floors are DELIBERATELY just under the counts observed when this was
    // written (12 root values, 41 `par_corpus` entries ⇒ 123 lifted cases). They
    // are a collapse detector, not a target: a corpus that GROWS is fine, one
    // that shrinks by half is the failure mode being guarded.
    assert!(
        root_cases >= 12,
        "the three ROOT corpora collapsed to {root_cases} cases (12 when this gate \
         was written); a green verdict from that would mean nothing"
    );
    assert!(
        lifted >= 90,
        "the per-arm corpus collapsed: only {lifted} lifted cases (123 when this \
         gate was written). `par_corpus()` is where every `ExprInstance` and \
         `ConnectiveInstance` arm enters this differential, so a shrunken one \
         silently stops covering arms"
    );
}

/// ★★ **THE CASE THE OTHER GATES MISS**: `EPathMap`-bearing values whose intern
/// cells are UNFILLED, at all three leg roots. These take the `direct()` early
/// return today, so they are precisely the values the conversion re-encodes.
#[test]
fn cold_encode_is_byte_identical_to_direct_for_unfilled_cell_pathmaps() {
    let shapes = unfilled_map_pars();

    // ★ ANTI-VACUITY, before any assertion: if `epathmap_par` ever stopped
    // producing an `EPathMap`-bearing `Par`, every row below would still pass
    // while covering nothing.
    let carries_map = |p: &Par| -> bool {
        matches!(
            p.exprs.first().and_then(|e| e.expr_instance.as_ref()),
            Some(ExprInstance::EPathmapBody(_)) | Some(ExprInstance::EZipperBody(_))
        )
    };
    assert!(
        shapes.iter().all(|(_, p)| carries_map(p)),
        "a fixture in `unfilled_map_pars` is not map-bearing, so this test would \
         be covering the map-free case twice under a map-bearing name"
    );

    for (label, par) in shapes {
        // Bare, and nested one level down a container, so the map is exercised
        // both as a root child and as an interior node.
        for (position, p) in [
            ("bare", par.clone()),
            ("in-elist", elist(vec![par.clone()])),
        ] {
            let d = datum_of(vec![p.clone()]);
            assert_cold_encode_is_direct(&format!("datum/{label}/{position}"), &d);
            assert_leg_is_derived_datum(&format!("{label}/{position}"), &d);

            let bp = pattern_of(vec![p.clone()]);
            assert_cold_encode_is_direct(&format!("pattern/{label}/{position}"), &bp);
            assert_leg_is_derived_pattern(&format!("{label}/{position}"), &bp);

            let k = continuation_of(p);
            assert_cold_encode_is_direct(&format!("continuation/{label}/{position}"), &k);
            assert_leg_is_derived_continuation(&format!("{label}/{position}"), &k);
        }
    }
}

/// Depth past every ceiling this repo has historically tripped over, on all three
/// roots. Runs on a big stack because the ORACLE (`bincode::serialize` of the
/// derived impl) is the Θ(depth) side of the comparison — the subject is not.
///
/// ⚠ The stack is sized for the *oracle*, not the machine. That asymmetry is the
/// entire point of the conversion, and stating it here keeps a reader from
/// concluding the trampolined encoder needs 512 MiB.
#[test]
fn cold_encode_is_byte_identical_to_direct_on_deep_terms() {
    for depth in [1usize, 2, 16, 256, 4_096] {
        std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .name(format!("deep-{depth}"))
            .spawn(move || {
                let par = nested_list(depth);

                let d = datum_of(vec![par.clone()]);
                assert_cold_encode_is_direct(&format!("deep/datum/{depth}"), &d);
                let bp = pattern_of(vec![par.clone()]);
                assert_cold_encode_is_direct(&format!("deep/pattern/{depth}"), &bp);
                let k = continuation_of(par);
                assert_cold_encode_is_direct(&format!("deep/continuation/{depth}"), &k);

                // ★ ANTI-VACUITY: bincode 1.3.3 emits at least a u64-LE length
                // prefix per nesting level, so a genuine depth-`d` encoding is
                // strictly longer than `d` bytes. A fixture that stopped carrying
                // depth would compare two identical short buffers and pass.
                let bytes = d.cold_encode();
                assert!(
                    bytes.len() > depth,
                    "VACUOUS: depth-{depth} datum encoded to only {} B",
                    bytes.len()
                );

                // `<Par as Drop>` is itself Θ(depth); let it run here, on the big
                // stack, rather than on the harness thread.
                drop(d);
            })
            .expect("spawn deep case")
            .join()
            .expect("deep case panicked");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  The FILLED-cell side, kept explicitly out of the obligation
// ═══════════════════════════════════════════════════════════════════════════

/// ★ **A filled cell does not change the derived bytes, and therefore does not
/// change `cold_encode`'s obligation either.**
///
/// `EPathMap`'s hand-written `Serialize` (`rhoapi_ext.rs:676`) writes exactly four
/// fields and never reads the `intern` cell — the cell is `#[serde(skip)]`. This
/// test makes that executable rather than quoted, because "the shadow cell is
/// invisible to serde" is load-bearing for the whole splice design: if filling a
/// cell moved the derived bytes, the spliced emitter's byte-identity claim would
/// already be false today.
///
/// ⚠ It asserts `cold_encode` agrees with the derived encoder on a FILLED-cell
/// value too. That value does **not** reach `direct()` in production, so this is
/// not part of the conversion's obligation — it is the evidence that the
/// obligation is *cell-state independent*, which is what lets §2 use unfilled
/// fixtures without a caveat.
#[test]
fn filling_the_intern_cell_moves_neither_encoder() {
    let map = e6a_index_epathmap();
    let before_oracle = bincode::serialize(&epathmap_par(map.clone())).expect("oracle");
    let before_machine = epathmap_par(map.clone()).cold_encode();

    let filled = map.clone();

    let after_oracle = bincode::serialize(&epathmap_par(filled.clone())).expect("oracle");
    let after_machine = epathmap_par(filled).cold_encode();

    assert_eq!(
        before_oracle, after_oracle,
        "the DERIVED encoder's bytes moved when the intern cell was filled — the \
         `#[serde(skip)]` shadow cell is leaking onto the wire"
    );
    assert_eq!(
        before_machine, after_machine,
        "`cold_encode`'s bytes moved when the intern cell was filled"
    );
    if let Err(why) = identity_verdict("filled-cell", &after_machine, &after_oracle) {
        panic!("{why}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Proptest — beyond what enumeration reaches
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Arbitrary bounded `Par` trees at all three roots.
    #[test]
    fn cold_encode_is_byte_identical_to_direct_on_arbitrary_terms(
        par in generate_par(3),
        random_state in proptest::collection::vec(any::<u8>(), 0..16),
        free_count in 0i32..8,
    ) {
        let d = ListParWithRandom { pars: vec![par.clone()], random_state: random_state.clone() };
        prop_assert!(identity_verdict("prop/datum", &d.cold_encode(),
            &bincode::serialize(&d).expect("oracle")).is_ok());

        let bp = BindPattern { patterns: vec![par.clone()], remainder: None, free_count };
        prop_assert!(identity_verdict("prop/pattern", &bp.cold_encode(),
            &bincode::serialize(&bp).expect("oracle")).is_ok());

        let k = TaggedContinuation {
            guard: Some(par.clone()),
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(par), random_state,
            })),
        };
        prop_assert!(identity_verdict("prop/continuation", &k.cold_encode(),
            &bincode::serialize(&k).expect("oracle")).is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  The differential can go RED
// ═══════════════════════════════════════════════════════════════════════════

/// ★ **An executed demonstration that a green verdict is informative.**
///
/// Three ways this file could be green while the conversion was unsafe, each
/// closed by showing the verdict rejecting a value it would have to reject:
///
/// 1. a **byte** difference anywhere in the middle;
/// 2. a **length** difference (truncation — the classic hand-codec failure);
/// 3. the **empty** encoding, which a short-circuiting subject would produce and
///    which compares equal to nothing real.
#[test]
fn the_identity_differential_can_go_red() {
    let d = datum_of(vec![nested_list(3)]);
    let oracle = bincode::serialize(&d).expect("oracle");
    assert!(
        oracle.len() > 8,
        "the RED demonstration needs a non-trivial oracle; got {} B",
        oracle.len()
    );

    // The honest observation is accepted.
    identity_verdict("red/control", &d.cold_encode(), &oracle)
        .expect("the verdict must ACCEPT a byte-identical observation");

    // 1 — one flipped byte.
    let mut flipped = oracle.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 0xFF;
    assert!(
        identity_verdict("red/flipped", &flipped, &oracle).is_err(),
        "the verdict accepted a value differing at byte {mid}"
    );

    // 2 — truncation.
    let truncated = oracle[..oracle.len() - 1].to_vec();
    assert!(
        identity_verdict("red/truncated", &truncated, &oracle).is_err(),
        "the verdict accepted a truncated encoding"
    );

    // 3 — the empty encoding.
    assert!(
        identity_verdict("red/empty", &[], &oracle).is_err(),
        "the verdict accepted an EMPTY encoding, so a subject that returned \
         nothing at all would pass every test in this file"
    );
}
