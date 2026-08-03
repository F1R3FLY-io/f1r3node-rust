//! Differential proof for the generated event-hash bincode encoder.
//!
//! For each production root, this suite compares `ColdStoreEncode::cold_encode`
//! and the event-hash entry point with the legacy derived-serde bytes. Coverage
//! includes every expression/connective arm, all EPathMap fixture shapes,
//! bounded-depth reference comparisons, generated terms, and an executed
//! negative control. The separate Casper depth gate proves the generated path
//! remains flat through depth 65,536 without giving the recursive reference an
//! oversized stack.

mod fixtures;
mod par_corpus;

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par, epathmap_remainder_connective,
    ezipper_value, nested_epathmap_value,
};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, EList, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation, Var,
};
use models::rust::event_hash_bytes::{
    event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
    event_hash_bytes_tagged_continuation,
};
use models::rust::rholang::bincode_encoder::ColdStoreEncode;
use models::rust::test_utils::test_utils::generate_par;
use par_corpus as corpus;
use proptest::prelude::*;
use serde::Serialize;

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

fn assert_generated_matches_legacy<T>(label: &str, value: &T)
where T: ColdStoreEncode + Serialize {
    let oracle = bincode::serialize(value).expect("oracle: derived bincode must encode");
    let machine = value.cold_encode();
    if let Err(why) = identity_verdict(label, &machine, &oracle) {
        panic!("{why}");
    }
}

fn assert_production_matches_legacy_datum(label: &str, value: &ListParWithRandom) {
    let oracle = bincode::serialize(value).expect("oracle");
    let leg = event_hash_bytes_list_par_with_random(value);
    if let Err(why) = identity_verdict(&format!("leg/datum::{label}"), &leg, &oracle) {
        panic!("{why}");
    }
}

fn assert_production_matches_legacy_pattern(label: &str, value: &BindPattern) {
    let oracle = bincode::serialize(value).expect("oracle");
    let leg = event_hash_bytes_bind_pattern(value);
    if let Err(why) = identity_verdict(&format!("leg/pattern::{label}"), &leg, &oracle) {
        panic!("{why}");
    }
}

fn assert_production_matches_legacy_continuation(label: &str, value: &TaggedContinuation) {
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
    models::par_from_default! {
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

fn epathmap_pars() -> Vec<(&'static str, Par)> {
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
// §2  The obligation over the shared corpora and EPathMap shapes
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
fn generated_encoder_matches_legacy_over_the_shared_corpora() {
    let mut root_cases = 0usize;
    for (label, v) in corpus::list_par_with_random_corpus() {
        assert_generated_matches_legacy(&format!("ListParWithRandom::{label}"), &v);
        root_cases += 1;
    }
    for (label, v) in corpus::bind_pattern_corpus() {
        assert_generated_matches_legacy(&format!("BindPattern::{label}"), &v);
        root_cases += 1;
    }
    for (label, v) in corpus::tagged_continuation_corpus() {
        assert_generated_matches_legacy(&format!("TaggedContinuation::{label}"), &v);
        root_cases += 1;
    }

    // Every `ExprInstance` / `ConnectiveInstance` arm, lifted to each root.
    let mut lifted = 0usize;
    for (label, par) in corpus::par_corpus() {
        let d = datum_of(vec![par.clone()]);
        assert_generated_matches_legacy(&format!("lift/datum::{label}"), &d);
        assert_production_matches_legacy_datum(&label, &d);

        let bp = pattern_of(vec![par.clone()]);
        assert_generated_matches_legacy(&format!("lift/pattern::{label}"), &bp);
        assert_production_matches_legacy_pattern(&label, &bp);

        let k = continuation_of(par);
        assert_generated_matches_legacy(&format!("lift/continuation::{label}"), &k);
        assert_production_matches_legacy_continuation(&label, &k);
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

#[test]
fn generated_encoder_matches_legacy_for_epathmap_shapes() {
    let shapes = epathmap_pars();

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
        "a fixture in `epathmap_pars` is not map-bearing, so this test would \
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
            assert_generated_matches_legacy(&format!("datum/{label}/{position}"), &d);
            assert_production_matches_legacy_datum(&format!("{label}/{position}"), &d);

            let bp = pattern_of(vec![p.clone()]);
            assert_generated_matches_legacy(&format!("pattern/{label}/{position}"), &bp);
            assert_production_matches_legacy_pattern(&format!("{label}/{position}"), &bp);

            let k = continuation_of(p);
            assert_generated_matches_legacy(&format!("continuation/{label}/{position}"), &k);
            assert_production_matches_legacy_continuation(&format!("{label}/{position}"), &k);
        }
    }
}

#[test]
fn generated_encoder_matches_legacy_at_bounded_reference_depths() {
    for depth in [1usize, 2, 16, 64, 128] {
        let par = nested_list(depth);

        let datum = datum_of(vec![par.clone()]);
        assert_generated_matches_legacy(&format!("depth/datum/{depth}"), &datum);

        let pattern = pattern_of(vec![par.clone()]);
        assert_generated_matches_legacy(&format!("depth/pattern/{depth}"), &pattern);

        let continuation = continuation_of(par);
        assert_generated_matches_legacy(&format!("depth/continuation/{depth}"), &continuation);

        let bytes = datum.cold_encode();
        assert!(
            bytes.len() > depth,
            "VACUOUS: depth-{depth} datum encoded to only {} B",
            bytes.len()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Proptest — beyond what enumeration reaches
// ═══════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Arbitrary bounded `Par` trees at all three roots.
    #[test]
    fn generated_encoder_matches_legacy_on_arbitrary_terms(
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
