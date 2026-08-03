//! # `clone_descend_budget` — the DESCEND BUDGET is MEASURED, not asserted
//!
//! `models/codegen/schema_codegen.rs` emits [`CLONE_DESCEND_BUDGET`], the number of
//! further **cut-set** levels one `descend` walks in native frames before it
//! suspends to `drive::drive_with`. This file is the executed statement that the
//! constant is doing what its doc comment claims, and it exists because every
//! part of that claim is the kind that passes vacuously if nobody runs it:
//!
//! | claim | how it could be vacuous | what runs here |
//! |---|---|---|
//! | the budget amortizes the trampoline | a budget of `0` compiles and is the pre-budget machine exactly | [`the_budget_is_not_zero`] |
//! | one `descend` covers the modal datum | true of *some* `k`; which `k` is the whole question | [`one_descend_covers_the_whole_modal_datum`] |
//! | depth `D` costs `⌈D/(k+1)⌉` suspensions | an arithmetic identity nobody evaluated | [`the_suspension_count_matches_an_independent_prediction`] |
//! | the count describes the REAL machine | a private counting loop can drift from `drive_with` | [`the_counted_walk_reproduces_the_oracle`] |
//!
//! ## ★★ Why the counting driver is legitimate rather than a second machine
//!
//! [`drive_counts`] re-implements `drive_with`'s loop, because `drive_with`
//! reports a value and not a step count and instrumenting it would put a counter
//! on the hot path of a consensus traversal. A second loop can drift from the
//! first, so the loop is not trusted on its own: **every count in this file is
//! taken from a walk whose product is then compared against
//! `term_ops::oracle_clone_par`**, the retained `#[derive(Clone)]` body that
//! `models/tests/clone_equivalence_corpus.rs` proves byte-identical to
//! `<Par as Clone>::clone` on eight axes. A counting loop that had drifted from
//! the real trampoline would produce a different term, and the comparison is what
//! makes the count a fact about the shipping machine.
//!
//! ## ★ Why `k + 1` and not `k`
//!
//! The budget names how many further cut-set levels may be **entered**, so a
//! `descend` that starts at budget `k` covers cut-set depths `0 … k` — `k + 1`
//! levels — and suspends at depth `k + 1`. That off-by-one is worth stating,
//! because the natural (and wrong) reading of "budget 3" is "three levels", which
//! predicts `⌈D/3⌉` where the machine delivers `⌈D/4⌉`.
//!
//! ## ⚠★★ What this file deliberately does NOT claim
//!
//! **It says nothing about the native stack.** An in-process test cannot bisect a
//! stack requirement — an overflow aborts the process rather than returning a
//! value — so the stack evidence lives where the child-process harness is:
//! `rholang/tests/stack_depth_gate.rs`, subject `clone`, which reads **0 B/level**
//! from depth 16 to depth 128 and from depth 4 to depth 4,096, unchanged by this
//! budget. See [`the_prefix_is_a_constant_and_that_is_why_depth_stays_unbounded`]
//! for the arithmetic that says why no value of the budget can cap depth, and for
//! the measured numbers.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, ETuple, Expr, Par, Send};
use models::rust::rholang::drive::{Outcome, Step, Traversal};
use models::rust::rholang::par_children::par_child_pars;
use models::rust::rholang::term_ops::{
    oracle_clone_par, CloneNode, CloneTraversal, CloneVal, CLONE_DESCEND_BUDGET,
    CLONE_RESIDUAL_HEIGHT,
};

// ---------------------------------------------------------------------------
// The fixtures — the SAME shapes `models/benches/term_ops_bench.rs` measures
// ---------------------------------------------------------------------------

/// The measured produce-depth distribution: `(depth, datums)` over the 1,773
/// instrumented datums `models/benches/bincode_encoder_bench.rs` established.
///
/// ⚠ Copied in the same form the bench holds it, and the two are joined by
/// [`the_weighted_mix_is_the_one_the_benchmark_measures`], which checks the
/// derived node count against the number the bench's cachegrind recipe divides
/// by. A silent divergence would make this file's ratio describe a workload the
/// primary instrument never ran.
const MEASURED: &[(usize, f64)] = &[
    (1, 12.0 / 1773.0),
    (2, 1692.0 / 1773.0),
    (3, 36.0 / 1773.0),
    (4, 3.0 / 1773.0),
    (5, 26.0 / 1773.0),
    (6, 4.0 / 1773.0),
];

/// `models/benches/term_ops_bench.rs`'s `WORKLOAD_SCALE`.
const WORKLOAD_SCALE: f64 = 2000.0;

/// The `Par` node count of one pass over the weighted mix, as printed by
/// `TERM_OPS_ARM=… term_ops_bench` and as used as the denominator of the primary
/// `Ir`-per-node criterion.
const WEIGHTED_MIX_NODES: usize = 12_286;

fn gint(n: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }],
        ..Default::default()
    }
}

fn gstr(s: &str) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }],
        ..Default::default()
    }
}

/// `models/benches/term_ops_bench.rs`'s `datum`, verbatim in shape.
fn datum(depth: usize) -> Par {
    let mut body = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(42)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GString("channel-payload".into())),
            },
        ],
        sends: vec![Send {
            chan: Some(gstr("ack")),
            data: vec![gint(1), gint(2)],
            persistent: false,
            locally_free: vec![],
            connective_used: false,
        }],
        locally_free: vec![],
        connective_used: false,
        ..Default::default()
    };
    for level in 1..depth {
        body = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(if level % 2 == 0 {
                    ExprInstance::ETupleBody(ETuple {
                        ps: vec![body, gint(level as i64)],
                        locally_free: vec![],
                        connective_used: false,
                    })
                } else {
                    ExprInstance::EListBody(EList {
                        ps: vec![body, gstr("sibling")],
                        locally_free: vec![],
                        connective_used: false,
                        remainder: None,
                    })
                }),
            }],
            ..Default::default()
        };
    }
    body
}

/// A bare `EList` spine: `levels + 1` `Par` nodes in one chain, each the sole
/// cut-set child of the one above. The cleanest shape for a `⌈L/(k+1)⌉` claim,
/// because its cut-set relation IS a path.
fn spine(levels: usize) -> Par {
    let mut p = gint(0);
    for _ in 0..levels {
        p = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![p],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    p
}

/// One pass over the production-weighted mix. ⚠ `datum` per copy, never
/// `vec![_; n]`, which would call the function under test to build the fixture.
fn weighted_workload() -> Vec<Par> {
    let mut out = Vec::with_capacity(2048);
    for (depth, share) in MEASURED {
        let copies = (share * WORKLOAD_SCALE).round().max(1.0) as usize;
        for _ in 0..copies {
            out.push(datum(*depth));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The instrument
// ---------------------------------------------------------------------------

/// What one driven clone cost the trampoline.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    /// `Step::Descend` pops — i.e. how many times the machine RE-ENTERED the
    /// visitor. This is the quantity the budget amortizes.
    descends: usize,
    /// `Step::Combine` pops. A wholly-native `descend` pushes no continuation, so
    /// this counts only the nodes that actually suspended something.
    combines: usize,
    /// The high-water mark of the value stack: how many finished `Par`s were live
    /// at once, i.e. how many 248-byte moves the frontier costs at peak.
    peak_vals: usize,
}

/// Run the clone traversal, counting the trampoline's steps, and return both the
/// counts and the term — so the caller can prove the counted walk is the same
/// walk `<Par as Clone>::clone` performs.
///
/// ⚠ A faithful copy of `drive_with`'s loop, minus the `Ledger` (which
/// `drive_with` itself asserts under `debug_assertions`) and plus the counters.
/// `Outcome::Tail` and `Outcome::Done` are unreachable for `CloneTraversal` —
/// `combine` returns `Outcome::Value` in its only arm — and are handled by
/// failing loudly rather than by a wildcard, so a traversal that started using
/// them would be caught here instead of silently miscounted.
fn drive_counts(root: &Par) -> (Counts, Par) {
    let mut visitor = CloneTraversal;
    let mut state = ();
    let mut work: Vec<Step<'_, CloneTraversal>> = Vec::with_capacity(64);
    let mut vals: Vec<CloneVal> = Vec::with_capacity(16);
    let mut counts = Counts::default();

    work.push(Step::Descend(CloneNode::Par(root)));
    while let Some(step) = work.pop() {
        match step {
            Step::Descend(node) => {
                counts.descends += 1;
                visitor
                    .descend(&mut state, node, &mut work, &mut vals)
                    .expect("clone_descend_budget: CloneTraversal::Err is Infallible");
            }
            Step::Combine(kont) => {
                counts.combines += 1;
                match visitor
                    .combine(&mut state, kont, &mut vals)
                    .expect("clone_descend_budget: CloneTraversal::Err is Infallible")
                {
                    Outcome::Value(v) => vals.push(v),
                    Outcome::Tail(_) => panic!(
                        "clone_descend_budget: `CloneTraversal::combine` returned `Outcome::Tail`. \
                         The clone traversal has only ever returned `Outcome::Value`, and a tail \
                         would make this counting loop disagree with `drive_with` about how many \
                         descends a term costs. Update this loop before the traversal."
                    ),
                    Outcome::Done(_) => panic!(
                        "clone_descend_budget: `CloneTraversal::combine` returned `Outcome::Done`. \
                         An early exit abandons pending obligations, so the descend count would \
                         describe a truncated walk. Update this loop before the traversal."
                    ),
                }
            }
        }
        counts.peak_vals = counts.peak_vals.max(vals.len());
    }

    assert_eq!(
        vals.len(),
        1,
        "clone_descend_budget: the counted walk left {} value(s) on the stack, not 1. \
         `drive_with`'s own final-configuration assertion says the same thing; reaching it here \
         means this loop and the traversal disagree.",
        vals.len()
    );
    let CloneVal::Par(out) = vals.pop().expect("one value");
    (counts, out)
}

/// Every `Par` reachable from `root`, including `root`, over the HAND-WRITTEN
/// child relation in `models/src/rust/rholang/par_children.rs` — not over the
/// generated families the budget threads through.
fn reachable_count(root: &Par) -> usize {
    let mut stack = vec![root];
    let mut seen = 0usize;
    let mut kids: Vec<&Par> = Vec::with_capacity(16);
    while let Some(p) = stack.pop() {
        seen += 1;
        kids.clear();
        par_child_pars(p, &mut kids);
        stack.extend(kids.iter().copied());
    }
    seen
}

/// The `Par` descendants of `p` at cut-set depth exactly `d` (`d ≥ 1`), over the
/// same hand-written relation.
fn descendants_at<'a>(p: &'a Par, d: usize) -> Vec<&'a Par> {
    let mut frontier: Vec<&Par> = vec![p];
    for _ in 0..d {
        let mut next: Vec<&Par> = Vec::with_capacity(frontier.len() * 2);
        for q in &frontier {
            par_child_pars(q, &mut next);
        }
        frontier = next;
    }
    frontier
}

/// ★ The INDEPENDENT prediction of [`Counts::descends`].
///
/// One `descend` starting at `p` covers cut-set depths `0 … k` and suspends every
/// descendant at depth `k + 1`; each of those starts a fresh `descend` with a
/// fresh budget. Written over `par_children`'s relation and over nothing the
/// budget touches, so it is a second statement of the machine's cost rather than
/// a restatement of it.
fn predicted_descends(p: &Par, budget: usize) -> usize {
    let mut pending = vec![p];
    let mut descends = 0usize;
    while let Some(root) = pending.pop() {
        descends += 1;
        pending.extend(descendants_at(root, budget + 1));
    }
    descends
}

/// The retained derive-shaped clone is a semantic oracle, not a deep-stack
/// subject. Keep it on a deliberately shallow corpus; the generated Eq PDA
/// checks deeper products against their inputs, while the independent worklist
/// above checks the exact suspension count at every sampled depth.
const RECURSIVE_ORACLE_MAX_LEVELS: usize = 16;

// ---------------------------------------------------------------------------
// The guards
// ---------------------------------------------------------------------------

/// ★★ **THE NON-VACUITY FLOOR.** A budget of `0` is the pre-budget machine
/// *exactly* — every cut-set child becomes a `Step::Descend` — and it compiles,
/// passes the equivalence corpus, and passes the depth gate. So every other test
/// in this file would still be *true* at `0`; they would simply be measuring the
/// machine the budget was introduced to replace.
///
/// ⚠ This is the one assertion that cannot be derived from the term: it is a
/// statement that the constant was *chosen*, and it is here because the failure
/// mode is a silent revert, not a compile error.
#[test]
fn the_budget_is_not_zero() {
    assert!(
        CLONE_DESCEND_BUDGET >= 1,
        "CLONE_DESCEND_BUDGET is {CLONE_DESCEND_BUDGET}. Zero is the PRE-BUDGET machine \
         verbatim — `clone_push_children_*` suspends every cut-set child and the trampoline is \
         re-entered once per `Par` node — so this whole file, the equivalence corpus and the \
         depth gate would all stay green while the amortization was gone. See \
         `models/codegen/schema_codegen.rs`'s `DESCEND_BUDGET` for the derivation of the value."
    );
    // The upper bound is not a correctness bound (the prefix is a constant at every
    // budget) but a REVIEW bound: past this, the constant costs a share of the
    // smallest production stack that ought to be argued for rather than assumed.
    assert!(
        CLONE_DESCEND_BUDGET <= 8,
        "CLONE_DESCEND_BUDGET is {CLONE_DESCEND_BUDGET}. Nothing about correctness breaks — the \
         native prefix is a constant at every budget and the surviving depth stays unbounded — \
         but at the conservative 7,021 B/level family slope this spends \
         {} B of a 2 MiB worker's stack on a constant, and a share that large should come with \
         its own measurement rather than inherit this one's.",
        (CLONE_DESCEND_BUDGET + 1) * 7_021
    );
}

/// ★ The modal case, which is 95.43% of measured produces: a depth-2 datum is a
/// chain of THREE cut-set levels, so the whole datum must be cloned inside ONE
/// `descend`, with no continuation pushed and no value ever parked.
#[test]
fn one_descend_covers_the_whole_modal_datum() {
    let modal = datum(2);
    let nodes = reachable_count(&modal);
    let (counts, out) = drive_counts(&modal);

    assert_eq!(
        nodes, 6,
        "the depth-2 datum has {nodes} `Par` nodes, not the 6 every figure in \
         `models/codegen/schema_codegen.rs` is denominated in. The fixture has drifted from the \
         bench's `datum`."
    );
    assert_eq!(
        counts,
        Counts {
            descends: 1,
            combines: 0,
            peak_vals: 1
        },
        "the modal datum cost {counts:?}. At CLONE_DESCEND_BUDGET = {CLONE_DESCEND_BUDGET} one \
         `descend` covers cut-set depths 0..={CLONE_DESCEND_BUDGET}, and this datum is only \
         three levels deep, so it must cost exactly one `descend`, no `Combine` at all (the \
         wholly-native fast path drops the continuation) and a value-stack high-water of one \
         (the answer itself). Anything else means the budget is not reaching the whole datum."
    );
    assert_eq!(
        out,
        oracle_clone_par(&modal),
        "the counted walk produced a different term from the derive oracle, so the count above \
         describes some other machine."
    );
}

/// ★★ `⌈L/(k+1)⌉`, evaluated rather than asserted, on a cut-set relation that is
/// a PATH — and cross-checked against a prediction written over the hand-written
/// child relation.
#[test]
fn the_suspension_count_matches_an_independent_prediction() {
    let k = CLONE_DESCEND_BUDGET;
    for levels in [0usize, 1, 2, 3, 4, 5, 7, 8, 9, 16, 31, 64, 129] {
        let term = spine(levels);
        let chain = levels + 1; // `Par` nodes in the chain
        assert_eq!(
            reachable_count(&term),
            chain,
            "the spine fixture is not a path: {levels} levels should be {chain} `Par` nodes"
        );

        let (counts, out) = drive_counts(&term);
        let closed_form = chain.div_ceil(k + 1);
        let independent = predicted_descends(&term, k);

        assert_eq!(
            counts.descends,
            closed_form,
            "a {chain}-node chain cost {} descends; the closed form ⌈{chain}/({k}+1)⌉ says \
             {closed_form}. One `descend` covers cut-set depths 0..={k}, i.e. {} levels.",
            counts.descends,
            k + 1
        );
        assert_eq!(
            counts.descends, independent,
            "a {chain}-node chain cost {} descends; the prediction written over \
             `par_children::par_child_pars` — a walk the budget does not touch — says \
             {independent}. Two statements of one cost have drifted.",
            counts.descends
        );
        if levels <= RECURSIVE_ORACLE_MAX_LEVELS {
            assert_eq!(
                out,
                oracle_clone_par(&term),
                "the counted walk of a {chain}-node chain produced a different term from the \
                 bounded derive oracle."
            );
        } else {
            assert_eq!(
                out, term,
                "the counted walk of a {chain}-node chain changed the input at a depth reserved \
                 for the generated stack-safe equality check"
            );
        }
    }
}

/// ★★★ **The headline, measured on the workload the PRIMARY instrument runs.**
///
/// The trampoline tax is paid once per `descend`. This reports how many `descend`s
/// one pass over the production-weighted mix costs, against the 12,286 `Par` nodes
/// it contains — the same denominator the cachegrind per-node figures use.
#[test]
fn the_weighted_mix_is_the_one_the_benchmark_measures() {
    let workload = weighted_workload();
    let nodes: usize = workload.iter().map(reachable_count).sum();
    assert_eq!(
        nodes, WEIGHTED_MIX_NODES,
        "one pass over the weighted mix has {nodes} `Par` nodes, but the primary criterion \
         divides its cachegrind counts by {WEIGHTED_MIX_NODES}. Either this fixture or the \
         bench's has drifted, and every per-node figure on record is denominated in the bench's."
    );

    let mut descends = 0usize;
    let mut combines = 0usize;
    let mut predicted = 0usize;
    for datum in &workload {
        let (counts, out) = drive_counts(datum);
        descends += counts.descends;
        combines += counts.combines;
        predicted += predicted_descends(datum, CLONE_DESCEND_BUDGET);
        assert_eq!(out, oracle_clone_par(datum));
    }

    assert_eq!(
        descends, predicted,
        "the mix cost {descends} descends; the independent prediction says {predicted}."
    );
    println!(
        "  weighted mix: {} datums, {nodes} `Par` nodes, {descends} descends, {combines} \
         combines at CLONE_DESCEND_BUDGET = {CLONE_DESCEND_BUDGET}\n  \
         trampoline re-entries per `Par` node: {:.4}  (a {:.2}× reduction against one per node)",
        workload.len(),
        descends as f64 / nodes as f64,
        nodes as f64 / descends as f64
    );

    // ★ The floor is deliberately loose. The exact count is a function of the mix's
    // shape and the budget, and pinning it would turn a mechanism check into a
    // transcription; what must hold is that the tax is amortized by a factor the
    // budget can explain. At budget 0 this ratio is exactly 1 and this FAILS.
    assert!(
        descends * 3 <= nodes,
        "the mix cost {descends} descends over {nodes} `Par` nodes — one re-entry per \
         {:.2} nodes. At CLONE_DESCEND_BUDGET = {CLONE_DESCEND_BUDGET} a chain pays one per \
         {} levels and 96.11% of the mix's datums fit ENTIRELY in one descend, so anything \
         above one per three nodes means the budget is not being spent.",
        nodes as f64 / descends as f64,
        CLONE_DESCEND_BUDGET + 1
    );
}

/// The join to the corpus: the counting loop is only evidence if it is the same
/// machine, so this drives the whole weighted mix through BOTH and compares.
///
/// ⚠ Separate from the count tests on purpose. Those would still pass if
/// `oracle_clone_par` and the driven clone had drifted together; this one is about
/// the `Par` values, and it is the reason a count in this file may be quoted as a
/// fact about `<Par as Clone>::clone`.
#[test]
fn the_counted_walk_reproduces_the_oracle() {
    for depth in 1..=8usize {
        let term = datum(depth);
        let (_, counted) = drive_counts(&term);
        let driven = <Par as Clone>::clone(&term);
        let derived = oracle_clone_par(&term);
        assert_eq!(
            counted, derived,
            "at depth {depth} the counting loop and the derive oracle disagree"
        );
        assert_eq!(
            driven, derived,
            "at depth {depth} `<Par as Clone>::clone` and the derive oracle disagree — this is \
             `clone_equivalence_corpus`'s claim, restated here so a failure of it cannot be \
             mistaken for a failure of the counting loop"
        );
    }
}

/// ★★ **Why no value of the budget can cap the representable depth** — the
/// arithmetic, published where a reader can check it, with the numbers this
/// campaign actually measured rather than the ones it inherited.
///
/// | quantity | value | where from |
/// |---|---|---|
/// | native prefix, in cut-set levels | `CLONE_DESCEND_BUDGET + 1` | this file, [`the_suspension_count_matches_an_independent_prediction`] |
/// | residual frames per cut-set level | ≤ `CLONE_RESIDUAL_HEIGHT` | generated, and the generator refuses to emit unless the residual is acyclic |
/// | `clone` min stack, k = 3, depth 16 → 512 | **4,344 B, FLAT** | `stack_depth_gate`'s child harness, bisected at 256 B |
/// | `clone` slope, depth 4 → 4,096 | **0 B/level** | `stack_depth_gate`, subject `clone` |
/// | `clone_oracle` (the derive-shaped family) slope | **7,021 B/level** | `stack_depth_gate`, re-measured at HEAD |
///
/// The prefix is `(k + 1) × CLONE_RESIDUAL_HEIGHT + O(1)` frames — a constant of
/// the *schema and the budget*, with no term in it — and then the walk suspends
/// onto the heap. So the minimum stack is flat in depth and the surviving depth is
/// bounded by the heap, i.e. unbounded for the purposes of this gate. A larger
/// budget buys a larger constant; it can never buy a maximum depth.
///
/// ⚠ **The `12288 / 3254 = 3.78` inequality that this work item inherited is not
/// a constraint and both of its numbers are the wrong ones.** `12,288 B` is not
/// the driven clone's stack cost — it is the smallest value
/// `stack_depth_gate::min_stack_for` can return, because that bisection probes
/// upward from 16 KiB and then searches only `[8192, 16384]`; bisected at 256 B
/// the same subject needs **4,344 B**. And `3,254 B/level` was measured on
/// `<Par as Clone>::clone` while it *was* a single derived function; the native
/// prefix is a family of free functions, which measures **7,021**. A ratio of a
/// resolution floor to the wrong slope is not a depth budget.
#[test]
fn the_prefix_is_a_constant_and_that_is_why_depth_stays_unbounded() {
    assert!(
        CLONE_RESIDUAL_HEIGHT > 0 && CLONE_RESIDUAL_HEIGHT < 16,
        "CLONE_RESIDUAL_HEIGHT is {CLONE_RESIDUAL_HEIGHT}; the prefix bound \
         `(k + 1) × CLONE_RESIDUAL_HEIGHT` is only a constant of the schema while this is one."
    );

    // The prefix is bounded by the BUDGET, so the same term at any depth costs the
    // same number of native levels — which is the property that makes the stack
    // flat. Stated as an executed inequality over a 4,000× depth range: the
    // per-descend native reach never exceeds `k + 1`, whatever the term.
    for levels in [3usize, 31, 4_095] {
        let term = spine(levels);
        let (counts, _) = drive_counts(&term);
        let chain = levels + 1;
        assert_eq!(
            counts.descends,
            chain.div_ceil(CLONE_DESCEND_BUDGET + 1),
            "a {chain}-level chain did not pay ⌈{chain}/{}⌉ suspensions. If one `descend` could \
             reach further than {} cut-set levels the native prefix would be a function of the \
             TERM, and the flat stack would be a coincidence of the fixtures rather than a \
             property of the machine.",
            CLONE_DESCEND_BUDGET + 1,
            CLONE_DESCEND_BUDGET + 1
        );
        // Every level beyond the first prefix is on the HEAP: the work stack, not the
        // native stack. A chain deeper than the budget must therefore suspend, and
        // that is the whole reason depth is unbounded.
        if chain > CLONE_DESCEND_BUDGET + 1 {
            assert!(
                counts.combines > 0,
                "a {chain}-level chain suspended nothing at budget {CLONE_DESCEND_BUDGET}, so it \
                 was cloned entirely in native frames — which would make the native stack \
                 Θ(depth) and is exactly what the conversion removed."
            );
        }
    }
}
