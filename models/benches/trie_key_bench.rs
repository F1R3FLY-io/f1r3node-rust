//! Paired benchmark for the current canonical PathMap-key path.
//!
//! The ground-domain cell compares the retained recursive specification with
//! the explicit-state classifier PDA. The protobuf cell compares the public
//! `Message` surface with the direct generated encoder; both now enter the same
//! generated PDA, so that cell measures API overhead rather than a recursive
//! predecessor. `encode_trie_path` is reported as an absolute composition.
//!
//! Every paired arm is checked for semantic or byte equality before timing.
//! `TRIE_KEY_ARM` runs exactly one arm for deterministic instruction/memory
//! profiling; subtract the `null` arm before forming ratios because fixture
//! construction and generated stack-safe teardown are part of the process
//! constant. Wall-clock figures are corroboration only.
//!
//! ```text
//! for arm in null protobuf_message protobuf_direct stable_recursive stable_pda encode_trie_path; do
//!   TRIE_KEY_ARM=$arm valgrind --tool=cachegrind --cache-sim=yes \
//!     target/release/deps/trie_key_bench-<hash>
//! done
//! ```
//!
//! The depth ladder contains former protocol boundaries as regression witnesses,
//! not implementation thresholds. No arm changes behavior at 64 or any other
//! artificial traversal depth.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{Connective, ETuple, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_crate_type_mapper::{eval_stable_epathmap, eval_stable_par_for_test};
use models::rust::rholang::protobuf_encoder;
use paired::{loadavg, measure_arms};
use prost::Message;

/// Depth samples, not runtime bounds.
const LADDER: &[usize] = &[1, 8, 33, 64, 256, 1_024];

/// Terms per pass at each rung, so a pass is comparable work across the ladder
/// rather than comparable *count*.
fn terms_at(depth: usize) -> usize { (4_096 / (depth + 1)).max(1) }

// ===========================================================================
// §A  fixtures
// ===========================================================================

fn gint(value: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

fn tuple_of(child: Par) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![child],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }],
        ..Default::default()
    }
}

/// A ground chain: `eval_stable_par` walks all of it and answers `true`.
fn stable_chain(depth: usize, seed: i64) -> Par {
    let mut par = gint(seed);
    for _ in 0..depth {
        par = tuple_of(par);
    }
    par
}

/// A chain whose bottom is a bare `Connective`: `eval_stable_par` walks all of it
/// and answers `false`, and `encode_trie_path` takes the ESCAPE arm.
fn unstable_chain(depth: usize) -> Par {
    let mut par = models::par_from_default! {
        connectives: vec![Connective {
            connective_instance: None,
        }],
        ..Default::default()
    };
    for _ in 0..depth {
        par = tuple_of(par);
    }
    par
}

// ===========================================================================
// §B  ★ the "before" arm of the classifier cell — the pre-change body, verbatim
// ===========================================================================

/// The pre-change `eval_stable_par`, transcribed.
///
/// ⚠ Quoted in full so a reader can check the transcription rather than trust it.
/// `models/src/rust/pathmap_crate_type_mapper.rs`, before the PDA conversion:
///
/// ```text
///   pub(crate) fn eval_stable_par(par: &Par) -> bool {
///       if !par.sends.is_empty() || !par.receives.is_empty() || !par.news.is_empty()
///           || !par.matches.is_empty() || !par.bundles.is_empty()
///           || !par.connectives.is_empty() || !par.conditionals.is_empty()
///           || !par.locally_free.is_empty() || par.connective_used
///       { return false; }
///       match (par.exprs.as_slice(), par.unforgeables.as_slice()) {
///           ([], [unforgeable]) => matches!(unforgeable.unf_instance,
///                                          Some(UnfInstance::GPrivateBody(_))),
///           ([expr], []) => eval_stable_expr(expr),
///           _ => false,
///       }
///   }
///   fn eval_stable_expr(expr: &Expr) -> bool {
///       match &expr.expr_instance {
///           Some(GBool(_) | GInt(_) | GString(_) | GUri(_) | GByteArray(_)
///                | GDouble(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_)) => true,
///           Some(EListBody(list)) => list.remainder.is_none()
///               && list.locally_free.is_empty() && !list.connective_used
///               && list.ps.iter().all(eval_stable_par),
///           Some(ETupleBody(tuple)) => tuple.locally_free.is_empty()
///               && !tuple.connective_used && tuple.ps.iter().all(eval_stable_par),
///           Some(EPathmapBody(inner)) => eval_stable_epathmap(inner),
///           _ => false,
///       }
///   }
/// ```
fn recursive_stable_par(par: &Par) -> bool {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
        || !par.locally_free.is_empty()
        || par.connective_used
    {
        return false;
    }
    match (par.exprs.as_slice(), par.unforgeables.as_slice()) {
        ([], [unforgeable]) => {
            matches!(unforgeable.unf_instance, Some(UnfInstance::GPrivateBody(_)))
        }
        ([expr], []) => recursive_stable_expr(expr),
        _ => false,
    }
}

fn recursive_stable_expr(expr: &Expr) -> bool {
    match &expr.expr_instance {
        Some(
            ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_),
        ) => true,
        Some(ExprInstance::EListBody(list)) => {
            list.remainder.is_none()
                && list.locally_free.is_empty()
                && !list.connective_used
                && list.ps.iter().all(recursive_stable_par)
        }
        Some(ExprInstance::ETupleBody(tuple)) => {
            tuple.locally_free.is_empty()
                && !tuple.connective_used
                && tuple.ps.iter().all(recursive_stable_par)
        }
        Some(ExprInstance::EPathmapBody(inner)) => eval_stable_epathmap(inner),
        _ => false,
    }
}

// ===========================================================================
// §C  the agreement check — a strawman arm cannot report a number
// ===========================================================================

/// ★ Both classifier arms must answer identically on every workload item, and both
/// encoder arms must produce identical bytes. Run before any timing; a mismatch
/// aborts rather than printing a ratio between two different computations.
fn the_two_classifiers_agree(workload: &[Par]) {
    for (i, par) in workload.iter().enumerate() {
        assert_eq!(
            recursive_stable_par(par),
            eval_stable_par_for_test(par),
            "workload[{i}]: the transcribed recursive classifier and the PDA disagree. \
             The 'before' arm has DRIFTED from the body it quotes, so every ratio in this run \
             would be between two different computations."
        );
        assert_eq!(
            Message::encode_to_vec(par),
            protobuf_encoder::encode_to_vec(par),
            "workload[{i}]: the Message and direct protobuf surfaces disagree. These bytes are \
             an escape payload inside a trie key inside EPM1 — a difference is a \
             consensus fork, not a benchmark artefact."
        );
    }
}

// ===========================================================================
// §D  the arms
// ===========================================================================

fn encoder_arms(workload: &[Par]) -> paired::Arms<2> {
    let mut a = |p: &Par| Message::encode_to_vec(p).len();
    let mut b = |p: &Par| protobuf_encoder::encode_to_vec(p).len();
    measure_arms(
        ["protobuf_message", "protobuf_direct"],
        workload,
        &mut [&mut a, &mut b],
        &mut |drain| {
            for n in drain {
                std::hint::black_box(n);
            }
        },
    )
}

fn classifier_arms(workload: &[Par]) -> paired::Arms<2> {
    let mut a = |p: &Par| recursive_stable_par(p);
    let mut b = |p: &Par| eval_stable_par_for_test(p);
    measure_arms(
        ["stable_recursive", "stable_pda"],
        workload,
        &mut [&mut a, &mut b],
        &mut |drain| {
            for v in drain {
                std::hint::black_box(v);
            }
        },
    )
}

/// The composed entry point, one arm only — there is no "before" for this one
/// (the pre-change `encode_trie_path` cannot be called from here), so it is
/// reported as an absolute and never as a ratio.
fn composed_arm(workload: &[Par]) -> paired::Arms<1> {
    let mut a = |p: &Par| encode_trie_path(p).len();
    measure_arms(
        ["encode_trie_path"],
        workload,
        &mut [&mut a],
        &mut |drain| {
            for n in drain {
                std::hint::black_box(n);
            }
        },
    )
}

// ===========================================================================
// §E  the deterministic single-arm mode
// ===========================================================================

/// One arm, one process, nothing else — the `cachegrind` subject.
///
/// ⚠ The workload is built and released **outside** the loop so that
/// `drop_in_place::<Par>` (itself Θ(depth), gate subject `par_drop`) is not
/// attributed to the arm. It is still inside the process, so it appears in the
/// fixed harness constant that cancels in the ratio between two arms — which is
/// only true because **both** arms are run with the same `LADDER` and the same
/// `terms_at`.
fn deterministic_arm(name: &str) {
    let mut checksum = 0usize;
    for &depth in LADDER {
        let count = terms_at(depth);
        let workload: Vec<Par> = (0..count)
            .map(|i| match i % 2 {
                0 => stable_chain(depth, i as i64),
                _ => unstable_chain(depth),
            })
            .collect();
        for par in &workload {
            checksum += match name {
                "protobuf_message" => Message::encode_to_vec(par).len(),
                "protobuf_direct" => protobuf_encoder::encode_to_vec(par).len(),
                "stable_recursive" => usize::from(recursive_stable_par(par)),
                "stable_pda" => usize::from(eval_stable_par_for_test(par)),
                "encode_trie_path" => encode_trie_path(par).len(),
                // ★★ THE HARNESS CONSTANT, measured rather than assumed to
                // cancel. `deterministic_arm` builds and releases the fixtures,
                // and cachegrind counts the WHOLE PROCESS — so a percentage
                // computed from two whole-process totals is DILUTED by whatever
                // share the fixtures take. For the encoder cell that share is
                // negligible (1.34 G vs 76 M instructions); for the classifier
                // cell it is not, and a report that quoted the whole-process
                // percentage as the arm's would understate the cost by however
                // much this arm turns out to be.
                //
                // ⚠ `black_box` on the term, so the fixture build cannot be
                // elided as dead in this arm and kept in the others.
                "null" => {
                    std::hint::black_box(par);
                    0
                }
                other => panic!(
                    "TRIE_KEY_ARM={other:?} is not an arm. One of: protobuf_message, \
                     protobuf_direct, stable_recursive, stable_pda, encode_trie_path, null."
                ),
            };
        }
    }
    // Printed so the arm cannot be optimized away and so two runs can be shown
    // to have done the same work.
    println!("{name} checksum {checksum}");
}

fn environment() {
    println!("ENVIRONMENT");
    println!(
        "  loadavg    {}   ◀── every ns below is conditional on this",
        loadavg()
    );
    println!("  profile    {}", match cfg!(debug_assertions) {
        true => "DEBUG — ⚠ a throughput claim from a debug build is not a throughput claim",
        false => "release",
    });
    println!(
        "  ⚠ ARM ORDER within each cell is fixed as printed; `paired::measure_arms` rotates \
         which arm runs first per repetition. A BETWEEN-RUN check must record this line."
    );
    println!();
}

fn main() {
    if let Ok(arm) = std::env::var("TRIE_KEY_ARM") {
        deterministic_arm(&arm);
        return;
    }

    environment();

    for &depth in LADDER {
        let count = terms_at(depth);

        // The ESCAPE cell: unstable chains, which is what the escape arm encodes.
        let escape: Vec<Par> = (0..count).map(|_| unstable_chain(depth)).collect();
        the_two_classifiers_agree(&escape);
        let bytes: usize = escape.iter().map(|p| p.encoded_len()).sum();
        println!("DEPTH {depth} — ESCAPE PAYLOAD ({count} terms/pass, {bytes} B/pass)");
        let arms = encoder_arms(&escape);
        arms.print();
        arms.report("direct PDA vs Message surface", 0, 1);

        // The CLASSIFIER cell: ground chains, which the classifier must walk in
        // full (an unstable chain short-circuits at the bottom, which measures
        // the same walk; a ground one guarantees no early exit at all).
        let ground: Vec<Par> = (0..count).map(|i| stable_chain(depth, i as i64)).collect();
        the_two_classifiers_agree(&ground);
        println!("DEPTH {depth} — GROUND-DOMAIN GATE ({count} terms/pass)");
        let arms = classifier_arms(&ground);
        arms.print();
        arms.report("PDA vs recursive specification", 0, 1);

        // The composed entry point, absolute only.
        println!("DEPTH {depth} — encode_trie_path, ABSOLUTE (no 'before' arm exists here)");
        composed_arm(&ground).print();
        println!();
    }

    println!(
        "⚠ Every ratio above is WALL CLOCK and is corroboration only. The primary verdict is \
         `TRIE_KEY_ARM=<arm> valgrind --tool=cachegrind --cache-sim=yes <this binary>` — see \
         the module header."
    );
}
