//! # `trie_key_bench` — the two conversions on `encode_trie_path`, measured
//!
//! ## What is being compared, and why the arms are what they are
//!
//! Two traversals on the trie-key write path were converted, and each has a
//! *callable* predecessor, which is what makes this a paired A/B rather than a
//! cross-run comparison:
//!
//! | cell | arm A (before) | arm B (after) |
//! |---|---|---|
//! | **escape payload** | `<Par as prost::Message>::encode_to_vec` — prost's derived encoder, still present | `prost_encode::encode_to_vec` — the iterative one the escape arm now calls |
//! | **ground-domain gate** | [`recursive_stable_par`], the pre-change body transcribed in full below | `eval_stable_par_for_test` — the budgeted one |
//!
//! ⚠★ The "before" arm of the second cell is a **transcription**, and that is a
//! real hazard: a transcription that drifted would measure a strawman. It is
//! mitigated the only way it can be — the pre-change body is **quoted verbatim**
//! beside it (it is 30 lines), and [`the_two_classifiers_agree`] asserts the two
//! arms return the same verdict on every workload item, so an arm that had drifted
//! semantically could not report a number at all.
//!
//! ## ⚠⚠ The wall clock here is CORROBORATION, not the verdict
//!
//! `paired.rs`'s header records why, and the brief this work was commissioned
//! under sharpens it: an invariant control bounds **within-run** resolution only —
//! four alternating runs of a duplicate-arm control reversed their ordering
//! (`+13.3%` then `−12.9%`) while every run's control drift stayed under 2%.
//!
//! ⇒ **The primary verdict is deterministic.** Run one arm per process under
//! `cachegrind` and read `Ir` / `Dw`:
//!
//! ```text
//!   for arm in null prost_derived prost_iterative stable_recursive stable_budgeted; do
//!     TRIE_KEY_ARM=$arm valgrind --tool=cachegrind --cache-sim=yes \
//!       target/release/deps/trie_key_bench-<hash> 2>&1 | tee /tmp/cg_$arm.log
//!   done
//! ```
//!
//! `TRIE_KEY_ARM` makes the process do **one** arm and nothing else, so the
//! instruction count is that arm's plus a fixed harness constant (the fixture
//! build and release, identical for every arm). No sampling, no skid,
//! byte-reproducible.
//!
//! ### ⚠⚠ THE CONSTANT DOES NOT CANCEL IN A PERCENTAGE — subtract the `null` arm
//!
//! It cancels in a *difference*, not in a *ratio of totals*, and the difference
//! between those two is 18-fold here. Measured on 2026-07-30, release,
//! `--cache-sim=yes`:
//!
//! ```text
//!   arm                whole-process Ir      arm-only Ir (− null)
//!   null                    26,450,298                    —
//!   stable_recursive        27,971,082             1,520,784
//!   stable_budgeted         28,505,137             2,054,839
//!
//!   whole-process ratio  28,505,137 / 27,971,082  =  +1.90 %   ← WRONG to quote
//!   arm-only      ratio   2,054,839 /  1,520,784  =  +35.1 %   ← the cost
//! ```
//!
//! ⇒ The `null` arm exists because the first version of this file had no way to
//! tell those apart, and `+1.90%` was the number it would have reported. It runs
//! the identical fixture construction and release and touches each term with
//! `black_box`, so subtracting it leaves the arm.
//!
//! ★ For the encoder cell the dilution is immaterial (1.34 G against a 26 M
//! constant) — which is exactly why a *single* rule of thumb about harness
//! constants is not safe: the same constant is negligible in one cell of one file
//! and dominant in another.
//!
//! ⚠ If a work reduction is real but the wall clock cannot resolve it, it is
//! reported **as a work reduction with no throughput claim**. This file prints the
//! ordering of its arms and the load average so a between-run check has the two
//! things it needs.
//!
//! ## The workload
//!
//! A depth ladder, because both conversions are depth-sensitive and in *opposite*
//! ways:
//!
//! * the derived prost encoder is `` $\Theta(d^2)$ `` in **work** (it re-measures
//!   every subtree: `encoding::message::encode` calls `msg.encoded_len()` for
//!   every nested message it writes) where the iterative one is `` $\Theta(n)$ ``,
//!   so the ratio should *grow* with depth;
//! * the budgeted classifier is byte-for-byte the same instruction stream as the
//!   recursion below `STABILITY_DESCEND_BUDGET = 64` levels plus one non-allocating
//!   `Vec::new()`, so the ratio should be ~1 at shallow depth and diverge only past
//!   the budget, where the recursion is paying stack traffic the worklist is not.
//!
//! ★ A single "typical" depth would report one point on two curves with different
//! shapes, which is why the ladder is the workload and each rung is printed.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{Connective, ETuple, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_crate_type_mapper::{eval_stable_epathmap, eval_stable_par_for_test};
use models::rust::rholang::prost_encode;
use paired::{loadavg, measure_arms};
use prost::Message;

/// The rungs. `64` is [`STABILITY_DESCEND_BUDGET`]; `65` is the first rung that
/// suspends, so the pair `(64, 65)` isolates the suspension's cost from the
/// walk's.
const LADDER: &[usize] = &[1, 8, 33, 64, 65, 256, 1_024];

/// Terms per pass at each rung, so a pass is comparable work across the ladder
/// rather than comparable *count*.
fn terms_at(depth: usize) -> usize {
    (4_096 / (depth + 1)).max(1)
}

// ===========================================================================
// §A  fixtures
// ===========================================================================

fn gint(value: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

fn tuple_of(child: Par) -> Par {
    Par {
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
    let mut par = Par {
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
/// `models/src/rust/pathmap_crate_type_mapper.rs`, before the descend budget:
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
        ([], [unforgeable]) => matches!(
            unforgeable.unf_instance,
            Some(UnfInstance::GPrivateBody(_))
        ),
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
            "workload[{i}]: the transcribed recursive classifier and the budgeted one disagree. \
             The 'before' arm has DRIFTED from the body it quotes, so every ratio in this run \
             would be between two different computations."
        );
        assert_eq!(
            Message::encode_to_vec(par),
            prost_encode::encode_to_vec(par),
            "workload[{i}]: the derived and iterative prost encoders disagree. These bytes are \
             an escape payload inside a trie key inside proto field 8 — a difference is a \
             consensus fork, not a benchmark artefact."
        );
    }
}

// ===========================================================================
// §D  the arms
// ===========================================================================

fn encoder_arms(workload: &[Par]) -> paired::Arms<2> {
    let mut a = |p: &Par| Message::encode_to_vec(p).len();
    let mut b = |p: &Par| prost_encode::encode_to_vec(p).len();
    measure_arms(
        ["prost_derived", "prost_iterative"],
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
        ["stable_recursive", "stable_budgeted"],
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
                "prost_derived" => Message::encode_to_vec(par).len(),
                "prost_iterative" => prost_encode::encode_to_vec(par).len(),
                "stable_recursive" => usize::from(recursive_stable_par(par)),
                "stable_budgeted" => usize::from(eval_stable_par_for_test(par)),
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
                    "TRIE_KEY_ARM={other:?} is not an arm. One of: prost_derived, \
                     prost_iterative, stable_recursive, stable_budgeted, encode_trie_path, null."
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
    println!("  loadavg    {}   ◀── every ns below is conditional on this", loadavg());
    println!(
        "  profile    {}",
        match cfg!(debug_assertions) {
            true => "DEBUG — ⚠ a throughput claim from a debug build is not a throughput claim",
            false => "release",
        }
    );
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
        println!(
            "DEPTH {depth} — ESCAPE PAYLOAD ({count} terms/pass, {bytes} B/pass)"
        );
        let arms = encoder_arms(&escape);
        arms.print();
        arms.report("iterative vs derived", 0, 1);

        // The CLASSIFIER cell: ground chains, which the classifier must walk in
        // full (an unstable chain short-circuits at the bottom, which measures
        // the same walk; a ground one guarantees no early exit at all).
        let ground: Vec<Par> = (0..count)
            .map(|i| stable_chain(depth, i as i64))
            .collect();
        the_two_classifiers_agree(&ground);
        println!("DEPTH {depth} — GROUND-DOMAIN GATE ({count} terms/pass)");
        let arms = classifier_arms(&ground);
        arms.print();
        arms.report("budgeted vs recursive", 0, 1);

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
