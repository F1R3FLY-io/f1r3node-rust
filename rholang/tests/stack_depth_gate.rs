//! # The Θ(depth) regression gate
//!
//! **What this gate is for.** A term's *nesting depth* — or its *sibling
//! width* — must not be able to determine how much NATIVE STACK the reducer
//! consumes. When it can, a 30-byte program
//! (`@"OUT"!([[[[[[[[[[0]]]]]]]]]])`) aborts the process: a stack overflow is a
//! `SIGSEGV`, not a catchable error, so it takes down the whole node rather
//! than failing one deploy.
//!
//! This gate is what makes "fixed" *checkable* rather than asserted, and what
//! stops the class from being silently reintroduced by the next traversal
//! somebody adds over the `Par` / `Expr` / `ExprInstance` family.
//!
//! Full analysis, enumeration and proof standard:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
//!
//! ## Two axes, not one
//!
//! The audit's original enumeration was a Tarjan SCC over `RhoTypes.proto`,
//! which sees *nesting*. It cannot see a traversal that recurses on a **list
//! tail** — and two do: `ScoredTerm::sort_vec`'s `compare_score_nodes`
//! (measured 201 B per sibling, debug) and `FoldMatch::free_check` (483 B per
//! sibling, debug). Both are program-controlled. Every subject therefore has a
//! DEPTH ladder and, where the shape admits one, a WIDTH ladder.
//!
//! ## How it works
//!
//! Each traversal runs on a thread created with an **explicit `stack_size`**,
//! so neither `RUST_MIN_STACK` nor `ulimit -s` can mask a regression. Three
//! assertions are available:
//!
//! * [`assert_depth_independent`] / [`assert_width_independent`] — the real
//!   bar. The traversal survives a FIXED, small stack across a wide ladder,
//!   **and** its bisected minimum stack does not grow between the two ends of a
//!   much wider ladder. Both halves are needed; see the note on
//!   [`assert_depth_independent`] for the measurement that shows why the fixed
//!   ladder alone is not sufficient for the cheaper members.
//!
//! * [`assert_slope_below`] — for traversals that are still Θ(depth), pins the
//!   measured bytes-per-level to a ceiling. This is a **tripwire, not a pass**:
//!   it detects a traversal getting *worse* while making the residual explicit
//!   in code. Every use names why its subject is still here.
//!
//! ## ⚠ Why the constants are per-profile, and why the gate does not hardcode one
//!
//! Frame sizes differ by ~2–12× between profiles because `rustc` does not
//! overlap the stack slots of mutually exclusive `match` arms at `-O0`, and the
//! family's hot functions are 36-arm matches over `ExprInstance`:
//!
//! | traversal                | debug (`-O0`) | release (`-O2`) | ratio |
//! |--------------------------|---------------|-----------------|-------|
//! | `Substitute::substitute` | 195,728 B     | 27,179 B        | 7.2×  |
//! | `ParSortMatcher`         |  78,579 B     |  6,495 B        | 12.1× |
//! | `PrettyPrinter`          |  41,840 B     |  4,242 B        |  9.9× |
//! | `<Par as Clone>`         |  15,875 B     |  2,852 B        |  5.6× |
//!
//! (mettail-rust additionally sets `codegen-backend = "cranelift"` for
//! `[profile.dev]`, which inflates frames again; f1r3node does not.)
//!
//! A gate that hardcoded one profile's byte count would therefore be
//! backend-fragile and would silently pass or fail for the wrong reason. So the
//! real assertions are **profile-independent by construction** — they assert a
//! *shape* (no growth), never a constant — and only the tripwire takes
//! ceilings, selected per profile via `cfg!(debug_assertions)` and set ~1.5×
//! above measured so that ordinary codegen drift does not flake while a real
//! (order-of-magnitude) regression still trips.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, New, Par, Receive, ReceiveBind};
use models::rust::rholang::par_children::{dismantle, dismantle_all};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::score_tree::{ScoreAtom, ScoredTerm, Tree};
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// term construction — ITERATIVE, so the builder itself is never the constraint
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

/// `[[[…[0]…]]]` with `depth` bracket levels — the reported shape.
fn nested_list(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = elist(vec![p]);
    }
    p
}

/// `[0, 1, …, width-1]` — the WIDTH counterpart of [`nested_list`].
fn wide_list(width: usize) -> Par {
    let mut ps = Vec::with_capacity(width);
    for i in 0..width {
        ps.push(new_gint_par(i as i64, vec![], false));
    }
    elist(ps)
}

/// The same chain lifted out of its `Par`, so two can be made siblings.
fn nested_list_expr(depth: usize, leaf: i64) -> Expr {
    let mut p = new_gint_par(leaf, vec![], false);
    for _ in 0..depth {
        p = elist(vec![p]);
    }
    p.exprs
        .pop()
        .expect("stack_depth_gate: nested_list_expr built a Par with no exprs")
}

fn wide_list_expr(width: usize) -> Expr {
    wide_list(width)
        .exprs
        .pop()
        .expect("stack_depth_gate: wide_list_expr built a Par with no exprs")
}

/// `new x in { for (_ <- @0) { … } }` nested `depth` times: the shape that
/// makes `substitute` construct a shifted `Env` at every level. A pure `EList`
/// chain never does, so it cannot see `Env::shift`'s `..(*self).clone()`.
fn nested_binders(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![Par::default()],
                source: Some(new_gint_par(0, vec![], false)),
                remainder: None,
                free_count: 0,
            }],
            body: Some(p),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: vec![],
            connective_used: false,
            condition: None,
        };
        p = Par {
            news: vec![New {
                bind_count: 1,
                p: Some(Par {
                    receives: vec![receive],
                    ..Default::default()
                }),
                uri: vec![],
                injections: BTreeMap::new(),
                locally_free: vec![],
            }],
            ..Default::default()
        };
    }
    p
}

/// `{{{…{0}…}}}` — `depth` nested `ESet`s.
///
/// ⚠ THE NAMED RESIDUAL OF STAGE C-2, and a TRIPWIRE rather than a conversion
/// target. The `ESetBody` / `EMapBody` / `EPathmapBody` arms of the sorter
/// re-enter `ParSortMatcher::sort_match` on OWNED intermediates (a deduplicated
/// `HashSet`, a canonicalised trie order) rather than on sub-terms of the
/// input, so they cannot be borrowed onto the worklist.
///
/// What bounds the residual is measured, not asserted: each of those arms sorts
/// every element THREE times, so a chain of `n` nested sets costs `3^n` sorts.
/// Depth 10 finishes; depth 20 (3.5e9 sorts) did NOT terminate in either
/// profile. Deep set nesting is infeasible in *time* long before the stack
/// residual could bite — hence a ceiling, not a conversion.
///
/// Underneath the sorter sits a floor no conversion of it can lift:
/// `HashSet<Par>` invokes the DERIVED `Par: Clone + Hash + Eq`, each Θ(depth)
/// in its own right. `clone_nested_set` is that control, and `sort_nested_set`
/// now measures BELOW it.
fn nested_sets(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ESetBody(models::rhoapi::ESet {
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

/// The `EMap` counterpart of [`nested_sets`], for the same reason.
fn nested_maps(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMapBody(models::rhoapi::EMap {
                    kvs: vec![models::rhoapi::KeyValuePair {
                        key: Some(p),
                        value: Some(new_gint_par(1, vec![], false)),
                    }],
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

fn substitute_instance() -> Substitute {
    Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::create(
            i64::MAX / 4,
            "stack_depth_gate".to_string(),
        ))),
    }
}

// ---------------------------------------------------------------------------
// ⚠ Some subject SETUPS are themselves Θ(depth)
//
// The score-tree subjects (`score_cmp`, `score_cmp_wide`, `tree_drop`,
// `tree_clone`) need a score tree before they can measure anything, and
// building one means running `sort_match` — which is the very traversal they
// are meant to be independent of (78,579 B/level, debug). Building the input
// on the gated thread would make every reading `max(sort_match, subject)`, and
// the first run of this gate proved it: `score_cmp` reported 78,573 B/level,
// i.e. the sorter's constant to within 0.01%, not the comparator's 1,329.
//
// Setups therefore run on a stack that is never the constraint, exactly as
// `stack_depth_probe.rs` does. One traversal per number.
// ---------------------------------------------------------------------------

/// Run `f` on a thread whose stack is large enough never to bind.
/// 1 GiB is address space, not resident memory.
fn on_a_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(1024 * 1024 * 1024)
        .name("gate-setup".to_string())
        .spawn(f)
        .expect("stack_depth_gate: failed to spawn the setup thread")
        .join()
        .expect("stack_depth_gate: setup thread panicked")
}

/// A score tree of nesting `depth`, built ITERATIVELY.
///
/// Building it by scoring a deep `Par` would make every score-tree reading
/// `max(sort_match, subject)` — `sort_match` is still Θ(depth) — and would cost
/// a 322 MiB setup stack at the gate's 4,096-deep rung. The traversals under
/// test (`compare_score`, `Tree`'s `Clone` / `PartialEq` / `Drop`) are generic
/// over shape, so the shape is built directly.
fn deep_score(depth: usize, leaf: i64) -> Tree<ScoreAtom> {
    let mut t = Tree::Leaf(ScoreAtom::create_from_i64(leaf));
    for _ in 0..depth {
        t = Tree::Node(vec![Tree::Leaf(ScoreAtom::create_from_i64(0)), t]);
    }
    t
}

/// A score tree of one level and `width` siblings, built ITERATIVELY.
fn wide_score(width: usize) -> Tree<ScoreAtom> {
    let mut children = Vec::with_capacity(width);
    for i in 0..width {
        children.push(Tree::Leaf(ScoreAtom::create_from_i64(i as i64)));
    }
    Tree::Node(children)
}

// ---------------------------------------------------------------------------
// ⚠ Probing has to FORK.
//
// A stack overflow is a `SIGSEGV` caught by the runtime's guard-page handler,
// which prints and `abort()`s. It is NOT a panic and NOT unwindable, so a probe
// that overflows in-process takes the whole test binary with it — including
// every assertion that had already passed, and every OTHER test in the file.
//
// So a probe runs in a CHILD process: the gate re-execs its own test binary with
// `GATE_SUBJECT` / `GATE_DEPTH` / `GATE_STACK` set, and reads the exit status.
// 0 = survived; anything else = did not. This is the same discipline the
// measurement harness (`stack_depth_probe.rs` + `scripts/stack_depth_probe.sh`)
// uses, and it is why a RED gate reports a clean failure instead of a truncated
// test run.
// ---------------------------------------------------------------------------

/// Names the subject a child process should run. Kept in one place so the parent
/// and the child cannot drift.
///
/// `GATE_DEPTH` means *bracket nesting* for the depth subjects and *sibling
/// count* for the `*_wide` subjects.
fn subject(name: &str) -> fn(usize) {
    match name {
        // -------- depth axis --------
        "substitute" => substitute_body,
        "substitute_no_sort" => substitute_no_sort_body,
        "substitute_binders" => substitute_binders_body,
        "substitute_deep_binding" => substitute_deep_binding_body,
        "sort" => sort_body,
        "sort_nested_set" => sort_nested_set_body,
        "sort_nested_map" => sort_nested_map_body,
        "clone_nested_set" => clone_nested_set_body,
        "score_cmp" => score_cmp_body,
        "tree_drop" => tree_drop_body,
        "tree_clone" => tree_clone_body,
        "pretty" => pretty_body,
        "clone" => clone_body,
        "drop" => drop_body,
        "encode" => encode_body,
        "bincode_ser" => bincode_ser_body,
        "bincode_de" => bincode_de_body,
        // -------- width axis --------
        "substitute_wide" => substitute_wide_body,
        "sort_wide" => sort_wide_body,
        "score_cmp_wide" => score_cmp_wide_body,
        "pretty_wide" => pretty_wide_body,
        "free_check" => free_check_body,
        "eval_with_nots" => eval_with_nots_body,
        other => panic!("stack_depth_gate: unknown GATE_SUBJECT={:?}", other),
    }
}

/// The child entry point. `#[ignore]`d so a normal `cargo test` run never
/// executes it directly; the parent always invokes it explicitly.
///
/// ⚠ It must be a NO-OP when its environment is absent. `cargo test
/// -- --include-ignored` (and `cargo nextest run --run-ignored all`) will run
/// every `#[ignore]`d test, including this one, with no `GATE_SUBJECT` set — so
/// a child entry point that *required* its environment would fail the suite for
/// a reason that has nothing to do with the property being gated. Skipping is
/// correct here precisely because this test is a mechanism, not an assertion:
/// the assertions live in its callers.
#[test]
#[ignore = "child process of the gate; driven via GATE_SUBJECT"]
fn gate_child() {
    let Ok(name) = std::env::var("GATE_SUBJECT") else {
        println!("gate_child: no GATE_SUBJECT — not a child invocation, nothing to do");
        return;
    };
    let depth: usize = std::env::var("GATE_DEPTH")
        .expect("GATE_DEPTH must accompany GATE_SUBJECT")
        .parse()
        .expect("GATE_DEPTH must be an integer");
    let stack: usize = std::env::var("GATE_STACK")
        .expect("GATE_STACK must accompany GATE_SUBJECT")
        .parse()
        .expect("GATE_STACK must be an integer");

    std::thread::Builder::new()
        .stack_size(stack)
        .name("gate".to_string())
        .spawn(move || subject(&name)(depth))
        .expect("stack_depth_gate: failed to spawn")
        .join()
        .expect("stack_depth_gate: subject panicked");
}

/// Run one probe point in a child process. `true` iff it survived.
fn runs_within(stack: usize, depth: usize, subject_name: &str) -> bool {
    let exe = std::env::current_exe().expect("stack_depth_gate: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "gate_child"])
        .env("GATE_SUBJECT", subject_name)
        .env("GATE_DEPTH", depth.to_string())
        .env("GATE_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("stack_depth_gate: failed to run child")
        .success()
}

/// Bisection resolution, in bytes. Both the zero-slope tolerance and the
/// tripwire's derived slope are quantised to this.
const RESOLUTION: usize = 4096;

/// Smallest stack (to `RESOLUTION` granularity) on which `name` survives at
/// `depth`. Exponential probe, then bisect.
fn min_stack_for(name: &str, depth: usize) -> usize {
    let mut hi = 16 * 1024;
    while hi <= 512 * 1024 * 1024 && !runs_within(hi, depth, name) {
        hi *= 2;
    }
    assert!(
        hi <= 512 * 1024 * 1024,
        "`{}` needed more than 512 MiB at parameter {}",
        name,
        depth
    );
    let mut lo = hi / 2;
    while hi - lo > RESOLUTION {
        let mid = (lo + hi) / 2;
        if runs_within(mid, depth, name) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// The maximum growth in minimum-stack, across the whole zero-slope ladder,
/// that still counts as "no growth". Four bisection buckets over a ~4,000-step
/// ladder is under 4 bytes per step — far below any real per-level frame (the
/// cheapest measured member of this family is `Tree`'s `Drop` at 370 B/level).
const ZERO_SLOPE_TOLERANCE: usize = 4 * RESOLUTION;

/// **The real bar, depth axis.**
///
/// Two halves, and both are load-bearing:
///
/// 1. `body` survives a FIXED `stack` at depths 4, 16, 64, 256.
/// 2. Its bisected minimum stack at depth 4 and at depth 4,096 agree to within
///    [`ZERO_SLOPE_TOLERANCE`].
///
/// ⚠ **Half (1) alone is not sufficient, and this is measured, not
/// hypothetical.** `compare_score` costs 1,329 B/level in debug with a ~40 KiB
/// intercept, so at depth 256 it needs 381 KiB — comfortably inside a 1 MiB
/// fixed stack. A still-Θ(depth) comparator would therefore *pass* half (1).
/// Half (2) spans 4 → 4,096 and cannot be passed by anything with a non-zero
/// slope, whatever its constant.
fn assert_depth_independent(name: &str, stack: usize) {
    for depth in [4usize, 16, 64, 256] {
        assert!(
            runs_within(stack, depth, name),
            "DEPTH-INDEPENDENCE GATE FAILED for `{}` at depth {} with a {} KiB stack.\n\
             This traversal's native stack grows with term nesting depth. See\n\
             docs/design/audits/theta-depth-traversals-2026-07-26.md for the conversion pattern.",
            name,
            depth,
            stack / 1024
        );
    }
    assert_no_slope(name, 4, 4096, "depth");
}

/// **The real bar, width axis.** Same shape as [`assert_depth_independent`],
/// with the parameter meaning sibling count. The ladder runs further because
/// per-sibling costs are an order of magnitude below per-level costs.
fn assert_width_independent(name: &str, stack: usize) {
    for width in [4usize, 16, 64, 256] {
        assert!(
            runs_within(stack, width, name),
            "WIDTH-INDEPENDENCE GATE FAILED for `{}` at width {} with a {} KiB stack.\n\
             This traversal's native stack grows with SIBLING COUNT. `compare_score_nodes`\n\
             and `FoldMatch::free_check` both recurse on the list TAIL; see\n\
             docs/design/audits/theta-depth-traversals-2026-07-26.md.",
            name,
            width,
            stack / 1024
        );
    }
    assert_no_slope(name, 4, 65536, "width");
}

/// The zero-slope half: minimum stack must not grow across a ~1,000× parameter
/// range. Profile-independent — it compares a traversal against ITSELF.
fn assert_no_slope(name: &str, lo_param: usize, hi_param: usize, axis: &str) {
    let lo = min_stack_for(name, lo_param);
    let hi = min_stack_for(name, hi_param);
    let growth = hi.saturating_sub(lo);
    assert!(
        growth <= ZERO_SLOPE_TOLERANCE,
        "ZERO-SLOPE GATE FAILED for `{}` on the {} axis: minimum stack grew {} KiB \
         between {} = {} ({} KiB) and {} = {} ({} KiB), which is {} B per step.\n\
         A converted traversal's native stack must not depend on {}. See\n\
         docs/design/audits/theta-depth-traversals-2026-07-26.md.",
        name,
        axis,
        growth / 1024,
        axis,
        lo_param,
        lo / 1024,
        axis,
        hi_param,
        hi / 1024,
        growth / (hi_param - lo_param),
        axis
    );
    println!("  {name} ({axis}): O(1) — {} KiB at {lo_param}, {} KiB at {hi_param}", lo / 1024, hi / 1024);
}

/// **Tripwire for traversals not yet converted.** Bisects the minimum stack at
/// two depths, derives bytes-per-level, and fails if it exceeds `ceiling`.
///
/// This deliberately does NOT claim the traversal is fixed. It claims only that
/// it has not got worse. Every caller documents why its subject is still here.
fn assert_slope_below(name: &str, ceiling_bytes_per_level: usize, lo: usize, hi_depth: usize) {
    // Two depths far enough apart that (a) the fixed intercept is a small share
    // of the difference and (b) the CHEAP traversals clear the minimum viable
    // thread stack at the deeper point — otherwise both probes bottom out on the
    // same floor and the derived slope is a meaningless 0.
    let s_lo = min_stack_for(name, lo);
    let s_hi = min_stack_for(name, hi_depth);
    let per_level = s_hi.saturating_sub(s_lo) / (hi_depth - lo);

    assert!(
        per_level <= ceiling_bytes_per_level,
        "Θ(DEPTH) TRIPWIRE for `{}`: {} B/level exceeds the {} B/level ceiling \
         ({} KiB @ depth {} -> {} KiB @ depth {}).\n\
         Either a traversal regressed, or codegen changed materially. See\n\
         docs/design/audits/theta-depth-traversals-2026-07-26.md.",
        name,
        per_level,
        ceiling_bytes_per_level,
        s_lo / 1024,
        lo,
        s_hi / 1024,
        hi_depth
    );
    println!("  {name}: {per_level} B/level (ceiling {ceiling_bytes_per_level})");
}

/// Per-profile ceiling. Debug frames are ~2–12× release because `-O0` does not
/// overlap `match`-arm stack slots; see the module docs.
fn ceiling(debug: usize, release: usize) -> usize {
    if cfg!(debug_assertions) {
        debug
    } else {
        release
    }
}

// ---------------------------------------------------------------------------
// subjects
// ---------------------------------------------------------------------------

fn substitute_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the substitute input's nesting", par_depth(&term), depth);
    let s = substitute_instance();
    let env: Env<Par> = Env::new();
    let out = s
        .substitute(term, 0, &env)
        .expect("stack_depth_gate: substitute failed");
    assert_carries("the substitute OUTPUT's nesting", par_depth(&out), depth);
    dismantle(out);
}

fn substitute_no_sort_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the substitute_no_sort input's nesting", par_depth(&term), depth);
    let s = substitute_instance();
    let env: Env<Par> = Env::new();
    let out = s
        .substitute_no_sort(term, 0, &env)
        .expect("stack_depth_gate: substitute_no_sort failed");
    assert_carries("the substitute_no_sort OUTPUT's nesting", par_depth(&out), depth);
    dismantle(out);
}

fn substitute_binders_body(depth: usize) {
    // Binder scopes under a POPULATED environment whose bound value is itself
    // DEEP — the hostile shape. The recursive form built `env.shift(bind_count)`
    // at every level, and `Env::shift` is `Env { shift: .., ..(*self).clone() }`,
    // i.e. a `HashMap<i32, Par>` clone, i.e. a Θ(depth) `<Par as Clone>::clone`
    // of every binding, per level. Measured before the conversion: 48,878
    // B/level here against 33,242 B/level with a ground binding.
    //
    // The environment is BUILT on a big stack because `Env::put` clones the map
    // too; the gated thread must measure the traversal, not the fixture.
    let env = on_a_big_stack(move || {
        let mut base: Env<Par> = Env::new();
        base.put(nested_list(depth))
    });
    let term = nested_binders(depth);
    assert_carries("the binder-scope nesting", binder_depth(&term), depth);
    // The bound value must be deep too — that is the whole point of this
    // subject against `substitute_binders_ground_env`.
    for bound in env.env_map.values() {
        assert_carries("the BOUND value's nesting", par_depth(bound), depth);
    }
    let s = substitute_instance();
    let out = s
        .substitute_no_sort(term, 0, &env)
        .expect("stack_depth_gate: substitute_binders failed");
    assert_carries("the substitute_binders OUTPUT's binder nesting", binder_depth(&out), depth);
    dismantle(out);
    // ⚠ The ENVIRONMENT has to be torn down iteratively too. It holds a
    // depth-`depth` bound value, and `Env`'s `HashMap<i32, Par>` drops through
    // the derived recursive `drop_in_place` — 470 B/level. The first run of
    // this subject reported 443 B/step for exactly that reason: the harness was
    // measuring its own teardown, not the traversal.
    dismantle_all(env.env_map.into_values());
}

/// ⚠ The NAMED RESIDUAL, not a converted traversal.
///
/// Substituting a `BoundVar` splices the bound term into the result, and
/// `Env::get` therefore returns it **cloned**. That clone is
/// `<Par as Clone>::clone`: a derived impl over 39 `prost` messages, row 5 of
/// the audit's table, whose disposition is explicitly "Leg-1 only — remove the
/// call sites, not the impl". This call site cannot be removed, because the
/// copy IS the meaning of substitution.
///
/// So this subject is Θ(depth of the BOUND VALUE) — measured 15,850 B/level,
/// which is `Par::clone`'s 15,875 to within 0.2% — and it is identical in the
/// recursive form. It lives in the tripwire, never in the converted list, so
/// that the residual is visible rather than folded into a claim of full
/// depth-independence.
fn substitute_deep_binding_body(depth: usize) {
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::{EVar, Var};
    let env = on_a_big_stack(move || {
        let mut base: Env<Par> = Env::new();
        base.put(nested_list(depth))
    });
    let term = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            })),
        }],
        ..Default::default()
    };
    let s = substitute_instance();
    let out = s
        .substitute_no_sort(term, 0, &env)
        .expect("stack_depth_gate: substitute_deep_binding failed");
    // ⚠ The strongest anti-vacuity check in this file: the subject measures the
    // `Env::get` deep CLONE, so the spliced-in bound value must actually be
    // there at full depth. If substitution had not substituted, the output
    // would be the `EVar` and this reads 0.
    assert_carries("the SPLICED bound value's nesting", par_depth(&out), depth);
    dismantle(out);
    dismantle_all(env.env_map.into_values());
}

fn substitute_wide_body(width: usize) {
    let term = wide_list(width);
    assert_carries("the substitute_wide input's sibling count", par_width(&term), width);
    let s = substitute_instance();
    let env: Env<Par> = Env::new();
    let out = s
        .substitute(term, 0, &env)
        .expect("stack_depth_gate: substitute_wide failed");
    assert_carries("the substitute_wide OUTPUT's sibling count", par_width(&out), width);
    dismantle(out);
}

fn sort_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the sort input's nesting", par_depth(&term), depth);
    let out = ParSortMatcher::sort_match(&term);
    assert_carries("the sort OUTPUT's nesting", par_depth(&out.term), depth);
    dismantle(out.term);
    dismantle(term);
}

fn sort_nested_set_body(depth: usize) {
    let term = nested_sets(depth);
    assert_carries("the nested-ESet input's nesting", eset_depth(&term), depth);
    let out = ParSortMatcher::sort_match(&term);
    assert_carries("the nested-ESet OUTPUT's nesting", eset_depth(&out.term), depth);
    dismantle(out.term);
    dismantle(term);
}

fn sort_nested_map_body(depth: usize) {
    let term = nested_maps(depth);
    assert_carries("the nested-EMap input's nesting", emap_depth(&term), depth);
    let out = ParSortMatcher::sort_match(&term);
    assert_carries("the nested-EMap OUTPUT's nesting", emap_depth(&out.term), depth);
    dismantle(out.term);
    dismantle(term);
}

/// The DERIVED control for [`sort_nested_set_body`]: `<Par as Clone>::clone`
/// over the same shape. If the sorter subject sits at or below this, what is
/// left in the set arm is the derived-traversal class and not the sorter.
fn clone_nested_set_body(depth: usize) {
    let term = nested_sets(depth);
    assert_carries("the clone_nested_set input's nesting", eset_depth(&term), depth);
    let c = term.clone();
    assert_carries("the CLONE's nesting", eset_depth(&c), depth);
    dismantle(c);
    dismantle(term);
}

fn sort_wide_body(width: usize) {
    // TWO wide lists, so the sorter has siblings to order and its comparator
    // has `width` score-tree children to walk.
    let term = Par {
        exprs: vec![wide_list_expr(width), wide_list_expr(width)],
        ..Default::default()
    };
    assert_carries("the sort_wide input's sibling count", par_width(&term), width);
    let out = ParSortMatcher::sort_match(&term);
    assert_carries("the sort_wide OUTPUT's sibling count", par_width(&out.term), width);
    dismantle(out.term);
    dismantle(term);
}

fn score_cmp_body(depth: usize) {
    // Two chains differing ONLY at the leaf, so `compare_score` must descend
    // every level before it can decide.
    //
    // ⚠ The plain `sort` subject CANNOT reach this code: a linear chain sorts
    // as a ONE-element vector, and `Vec::sort_by` on one element performs zero
    // comparisons. Converting `sort_match` alone would leave the comparator
    // Θ(depth) and this gate would still pass — which is why the comparator is
    // a subject in its own right.
    let mut scored = vec![
        ScoredTerm {
            term: 1usize,
            score: deep_score(depth, 1),
        },
        ScoredTerm {
            term: 0usize,
            score: deep_score(depth, 0),
        },
    ];
    for s in &scored {
        assert_carries("a compared score tree's nesting", tree_depth(&s.score), depth);
    }
    ScoredTerm::sort_vec(&mut scored);
    assert_eq!(scored[0].term, 0, "the comparator did not order the pair");
}

fn score_cmp_wide_body(width: usize) {
    // Two IDENTICAL wide score trees: every head pair compares Equal, so the
    // sibling walk must run the whole width.
    let mut scored = vec![
        ScoredTerm {
            term: 0usize,
            score: wide_score(width),
        },
        ScoredTerm {
            term: 1usize,
            score: wide_score(width),
        },
    ];
    for s in &scored {
        assert_carries("a compared score tree's sibling count", tree_width(&s.score), width);
    }
    ScoredTerm::sort_vec(&mut scored);
    // Stable sort + `Equal` comparison must preserve the input order. That is
    // the property `sig.rs` depends on, asserted at every width the gate probes.
    assert_eq!(scored[0].term, 0, "a stable sort reordered two equal scores");
}

fn tree_drop_body(depth: usize) {
    // `Tree<ScoreAtom>` is a recursive RUST type, not a proto message, so the
    // audit's Tarjan-over-the-proto enumeration could not see it.
    let score = deep_score(depth, 0);
    assert_carries("the dropped score tree's nesting", tree_depth(&score), depth);
    drop(score);
}

fn tree_clone_body(depth: usize) {
    let score = deep_score(depth, 0);
    assert_carries("the cloned score tree's nesting", tree_depth(&score), depth);
    let c = score.clone();
    assert_carries("the CLONE's nesting", tree_depth(&c), depth);
    assert!(c == score, "the iterative Clone did not reproduce its input");
    drop(c);
    drop(score);
}

fn pretty_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the pretty input's nesting", par_depth(&term), depth);
    let mut pp = PrettyPrinter::new();
    let s = pp.build_string_from_message(&term);
    // ⚠ `!s.is_empty()` was the old check and it is far too weak: a printer
    // that returned "Nil" — or the `<unprintable: …>` fallback — would pass it
    // while doing no traversal at all. The RENDERED nesting is the real proof.
    assert_carries("the PRINTED nesting", printed_bracket_depth(&s), depth);
    dismantle(term);
}

fn pretty_wide_body(width: usize) {
    let term = wide_list(width);
    assert_carries("the pretty_wide input's sibling count", par_width(&term), width);
    let mut pp = PrettyPrinter::new();
    let s = pp.build_string_from_message(&term);
    // `[a, b, c]` — one comma fewer than there are elements.
    assert_carries("the PRINTED sibling count", s.matches(", ").count() + 1, width);
    dismantle(term);
}

fn clone_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the clone input's nesting", par_depth(&term), depth);
    let c = term.clone();
    assert_carries("the CLONE's nesting", par_depth(&c), depth);
    dismantle(c);
    dismantle(term);
}

fn drop_body(depth: usize) {
    // The ONE subject that must be allowed to drop recursively — that is the
    // thing under test.
    let term = nested_list(depth);
    assert_carries("the dropped term's nesting", par_depth(&term), depth);
    drop(term);
}

fn encode_body(depth: usize) {
    use prost::Message;
    let term = nested_list(depth);
    assert_carries("the encode input's nesting", par_depth(&term), depth);
    let bytes = term.encode_to_vec();
    // Each `EList` level contributes at least its three length-delimited keys,
    // so a collapsed fixture cannot produce a plausible byte count.
    assert!(
        bytes.len() >= 3 * depth,
        "VACUOUS PROBE: encoding a depth-{} term produced only {} bytes",
        depth,
        bytes.len()
    );
    dismantle(term);
}

// ---------------------------------------------------------------------------
// ⚠ ANTI-VACUITY: every subject must PROVE its input carries the parameter it
// claims, and — where the subject produces a structure — that its output does
// too.
//
// This is the same failure mode as intercept-masking, approached from the other
// side. A probe whose fixture silently collapsed would run in O(1) stack and
// report a comfortable ZERO, and the gate would certify depth-independence for
// a traversal that was never given any depth. That has already happened three
// times in this work:
//
//   * the `prost` decode probe used a wrong field number, so the decoder SKIPPED
//     the payload as an unknown field and reconstructed a shallow term (audit
//     §4.3);
//   * `generate_par` sized every collection with an exclusive `0..1`, so the
//     sub-generators NEVER RAN and the corpus was two values (audit §11.3);
//   * the score-tree subjects built their inputs on the gated thread and
//     reported the SORTER's 78,573 B/level instead of the comparator's 1,329.
//
// So the discipline is applied UNIFORMLY, at every subject, rather than at the
// one subject where somebody happened to think of it. Every walker below is
// ITERATIVE: a recursive checker would measure itself.
// ---------------------------------------------------------------------------

/// The uniform anti-vacuity assertion.
fn assert_carries(what: &str, actual: usize, claimed: usize) {
    assert_eq!(
        actual, claimed,
        "VACUOUS PROBE: {} is {} but the subject was asked for {}. The reading would be \
         meaningless — a fixture that collapses runs in O(1) stack and reports a \
         comfortable zero for a traversal that was never given any depth or width.",
        what, actual, claimed
    );
}

/// Count `[[…[x]…]]` nesting levels ITERATIVELY.
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

/// Sibling count of the outermost `EList`.
fn par_width(p: &Par) -> usize {
    match p.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
        Some(ExprInstance::EListBody(l)) => l.ps.len(),
        _ => 0,
    }
}

/// Count `!(!(…))` nesting levels ITERATIVELY.
fn enot_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        match cur.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::ENotBody(not)) => match not.p.as_ref() {
                Some(inner) => {
                    n += 1;
                    cur = inner;
                }
                None => return n,
            },
            _ => return n,
        }
    }
}

/// Count `{{…{x}…}}` nesting levels (the `ESet` shape) ITERATIVELY.
fn eset_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        match cur.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::ESetBody(set)) if !set.ps.is_empty() => {
                n += 1;
                cur = &set.ps[0];
            }
            _ => return n,
        }
    }
}

/// Count nested `EMap` levels ITERATIVELY, following the KEY.
fn emap_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        match cur.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EMapBody(map)) if !map.kvs.is_empty() => {
                match map.kvs[0].key.as_ref() {
                    Some(k) => {
                        n += 1;
                        cur = k;
                    }
                    None => return n,
                }
            }
            _ => return n,
        }
    }
}

/// Count `new … in { for (…) { … } }` binder scopes ITERATIVELY.
fn binder_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        let Some(new) = cur.news.first() else {
            return n;
        };
        let Some(inner) = new.p.as_ref() else {
            return n;
        };
        let Some(recv) = inner.receives.first() else {
            return n;
        };
        match recv.body.as_ref() {
            Some(body) => {
                n += 1;
                cur = body;
            }
            None => return n,
        }
    }
}

/// Nesting depth of a score tree, following the LAST child (the shape
/// [`deep_score`] builds), ITERATIVELY.
fn tree_depth(t: &Tree<ScoreAtom>) -> usize {
    let mut n = 0usize;
    let mut cur = t;
    loop {
        match cur {
            Tree::Node(children) => match children.last() {
                Some(last) => {
                    n += 1;
                    cur = last;
                }
                None => return n,
            },
            Tree::Leaf(_) => return n,
        }
    }
}

/// Sibling count at the root of a score tree.
fn tree_width(t: &Tree<ScoreAtom>) -> usize {
    match t {
        Tree::Node(children) => children.len(),
        Tree::Leaf(_) => 0,
    }
}

/// Leading `[` characters — the depth a printed `nested_list` must show.
fn printed_bracket_depth(s: &str) -> usize {
    s.chars().take_while(|c| *c == '[').count()
}

/// ⚠ THE FAMILY'S BINDING MEMBER, AND UNTIL NOW IT HAD NO GATE COVERAGE AT ALL.
///
/// `models/build.rs` attaches `serde::Serialize`/`Deserialize` to every
/// `.rhoapi` message, and RSpace serialises datums and continuations with
/// **bincode 1.3.3** (`rspace++/src/rspace/serializers/serializers.rs`), which
/// — unlike `prost`, capped at 100 nested messages — has **no recursion limit
/// at all**. Bisected directly: bincode round-trips at depths 33, 34, 40, 100,
/// 200, 400 and 800, where `prost` returns `Err` from 34 onward.
///
/// The encode side is ~9x (debug) to ~39x (release) cheaper per level than the
/// decode side, so a term can be *written* on a stack that cannot *read it
/// back* — and the read-back failure is an `abort()`, not an `Err`. A datum
/// written to LMDB above the decode ceiling therefore aborts the node on every
/// restart, because the datum persists.
///
/// Measured 2026-07-26: encode 3,052 / 329 B/level (debug / release), decode
/// 28,362 / 12,894.
fn bincode_ser_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the bincode_ser input's nesting", par_depth(&term), depth);
    let bytes = bincode::serialize(&term).expect("stack_depth_gate: bincode_ser failed");
    assert!(
        bytes.len() >= depth,
        "VACUOUS PROBE: bincode-encoding a depth-{} term produced only {} bytes",
        depth,
        bytes.len()
    );
    dismantle(term);
}

/// The DECODE side. See [`bincode_ser_body`] for why this member matters.
///
/// ★ CONVERTED. This subject now exercises `Par::cold_decode`
/// (`models/src/rust/rholang/par_codec.rs`), the explicit-worklist decoder that
/// replaced the derived `Deserialize` on the cold-store read path. The derived
/// impl is retained as a `#[cfg(test)]` oracle and is differentially compared
/// against the machine over ~1.9M inputs, including every truncation of every
/// corpus encoding (`models/tests/par_codec_malformed.rs`) — but it is no
/// longer what the node runs, so it is no longer what this gate measures.
///
/// ⚠ `bincode_ser` stays in the tripwire. The ENCODER is deliberately
/// untouched: leaving `Serialize` derived is what preserves byte identity of
/// the cold-store leaves by construction, so its Θ(depth) residual (3,052 / 329
/// B per level) is a separate, still-open item.
fn bincode_de_body(depth: usize) {
    // ⚠ Encode on a stack that never binds, so this subject isolates the
    // DECODER. Encoding on the gated thread would make every reading
    // `max(encode, decode)` — the same defect that once made the score-tree
    // subjects report 78,573 B/level (the SORTER's constant) instead of the
    // comparator's 1,329. It matters twice as much now: the encoder is STILL
    // Θ(depth), so an un-isolated probe would report the encoder's slope and
    // this conversion would look like it had not happened.
    let bytes = on_a_big_stack(move || {
        let term = nested_list(depth);
        let b = bincode::serialize(&term).expect("stack_depth_gate: bincode encode failed");
        dismantle(term);
        b
    });
    let decoded: Par =
        Par::cold_decode(&bytes).expect("stack_depth_gate: bincode_de failed");
    assert_carries("the DECODED term's nesting", par_depth(&decoded), depth);
    dismantle(decoded);
}

/// ⚠ THE SECOND WIDTH-AXIS MEMBER, and it is in the MATCHER rather than the
/// sorter or the substitution SCC.
///
/// `FoldMatch::free_check` recursed on the slice TAIL, so its native stack grew
/// with the surplus-target count — 483 B per sibling in debug. It is now a
/// `for` loop (see `fold_match.rs` for why a loop and not a driver).
fn free_check_body(width: usize) {
    use rholang::rust::interpreter::matcher::fold_match::FoldMatch;
    let mut trem = Vec::with_capacity(width);
    for i in 0..width {
        trem.push(new_gint_par(i as i64, vec![], false));
    }
    assert_carries("the free_check input's sibling count", trem.len(), width);
    let ctx = SpatialMatcherContext::new();
    let out = FoldMatch::<Par, Par>::free_check(&ctx, &trem, 0, Vec::new())
        .expect("stack_depth_gate: free_check rejected a locally-free-empty list");
    assert_carries("the free_check OUTPUT's element count", out.len(), width);
    dismantle_all(out);
    dismantle_all(trem);
}

/// ⚠ `rho-pure-eval::eval_with`'s OWN Θ(depth) SCC — a separate crate, and one
/// the audit's enumeration reached only by measurement (§11.2).
///
/// The shape matters: a nested `EList` chain does NOT drive this recursion at
/// all, because the `EListBody` arm returns `par_with_expr(expr.clone())`
/// without descending. On that shape the probe measures `<Par as Clone>::clone`
/// and nothing else. `!(!(…(!true)…))` is the shape that actually recurses;
/// measured 21,584 B/level debug / 3,359 release before the conversion.
fn eval_with_nots_body(depth: usize) {
    use rho_pure_eval::{eval_with, NoSpatialMatch};
    let mut p = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(true)),
        }],
        ..Default::default()
    };
    for _ in 0..depth {
        p = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(models::rhoapi::ENot {
                    p: Some(p),
                })),
            }],
            ..Default::default()
        };
    }
    assert_carries("the eval_with input's ENot nesting", enot_depth(&p), depth);
    let env: Env<Par> = Env::new();
    let out = eval_with(&p, &env, &NoSpatialMatch).expect("stack_depth_gate: eval_with failed");
    // An even number of negations of `true` is `true`; an odd number `false`.
    let expected = depth % 2 == 0;
    assert_eq!(
        out.exprs.first().and_then(|e| e.expr_instance.as_ref()),
        Some(&ExprInstance::GBool(expected)),
        "stack_depth_gate: {} negations of `true` should be {}",
        depth,
        expected
    );
    dismantle(p);
    dismantle(out);
}

// ---------------------------------------------------------------------------
// THE GATE
// ---------------------------------------------------------------------------

/// Traversals that have been converted to a heap-bounded (explicit worklist)
/// form. Membership of this list is the deliverable; it only ever grows.
///
/// ⚠ EMPTY AT PRESENT — see `theta_depth_tripwire` below and
/// `docs/design/audits/theta-depth-traversals-2026-07-26.md` § "Disposition". No
/// traversal over the `Par` family is heap-bounded yet, so asserting
/// depth-independence for any of them here would be a false claim. This test
/// exists, named and wired, so that converting a traversal is a one-line
/// addition rather than a new piece of infrastructure somebody has to invent
/// under pressure.
///
/// **Done-for-the-family** is this list containing every hand-written member —
/// `substitute`, `substitute_no_sort`, `substitute_binders`, `sort`,
/// `score_cmp`, `tree_drop`, `tree_clone`, `pretty` — on the depth axis, and
/// `substitute_wide`, `sort_wide`, `score_cmp_wide`, `pretty_wide` on the width
/// axis, **in both profiles**.
#[test]
fn converted_traversals_are_depth_independent() {
    let converted_depth: &[&str] = &[
        "substitute_no_sort",   // Stage B — the driver
        "substitute_binders",   // Stage B — the driver, under a deep environment
        "substitute",           // Stage C-2 (the sorter) + Stage E (the intermediate
        //                         is dismantled instead of dropped recursively)
        "sort",                 // Stage C-2 — ParSortMatcher
        "score_cmp",            // Stage C-1 — the score-tree comparator
        "tree_drop",            // Stage C-1 — Tree's hand-written Drop
        "tree_clone",           // Stage C-1 — Tree's hand-written Clone
        "eval_with_nots",       // Stage E — rho-pure-eval's own SCC
        "bincode_de",           // Stage F — the cold-store DECODER (par_codec)
        // "pretty",               // Stage D
    ];
    let converted_width: &[&str] = &[
        "substitute_wide", // Stage B
        "sort_wide",       // Stage C-2
        "score_cmp_wide",  // Stage C-1 — the sibling walk
        "free_check",      // Stage E — the matcher's width-axis member, now a `for` loop
        // "pretty_wide",          // Stage D
    ];

    for name in converted_depth {
        // 1 MiB is well below what ANY Θ(depth) member needs at depth 256.
        assert_depth_independent(name, 1024 * 1024);
    }
    for name in converted_width {
        assert_width_independent(name, 1024 * 1024);
    }
    if converted_depth.is_empty() && converted_width.is_empty() {
        println!(
            "no traversal converted yet — see docs/design/audits/theta-depth-traversals-2026-07-26.md"
        );
    }
}

/// Tripwire over every traversal still known to be Θ(depth). Ceilings are ~1.5×
/// the values measured on 2026-07-26 (recorded in the audit document), so
/// ordinary codegen drift will not flake while an order-of-magnitude regression
/// still trips.
///
/// A traversal LEAVES this list only by moving to `converted_traversals_are_
/// depth_independent`, never by having its ceiling raised.
#[test]
fn theta_depth_tripwire() {
    // measured 2026-07-26 (debug / release), bytes per nesting level:
    //   substitute 195,728 / 27,179    sort 78,579 /  6,495
    //   pretty      41,840 /  4,242    clone      15,875 /  2,852
    //   encode       1,948 /    422    drop          470 /    219
    //   score_cmp    1,329 /    128    tree_clone  1,578 /    485
    //   tree_drop      370 /    204
    // Expensive traversals are probed shallow (a 64-deep `substitute` already
    // needs ~12 MiB in debug); cheap ones are probed deep so their per-level
    // cost rises clear of the minimum viable thread stack.
    // ⚠ `substitute` (the SORTED entry point) is `substitute_no_sort` followed
    // by exactly one `ParSortMatcher::sort_match`. Stage B converted the first
    // half — `substitute_no_sort` is now in the converted list at 0 B/level —
    // so what this tripwire measures is now the SORTER, and the figure moved
    // from 195,754 to 78,583 B/level, matching `sort` to within 0.01%. It
    // leaves this list when Stage C converts the sorter, not before.
    // ⚠ `substitute` and `sort` have LEFT this list — they are in
    // `converted_traversals_are_depth_independent` and measure 0 B/level over
    // 4 -> 4,096 in both profiles. A traversal only ever leaves by being
    // converted, never by having its ceiling raised.
    //
    // ⚠⚠ AND THIS LIST TAUGHT ITS OWN LESSON. While `substitute` still had a
    // 437 B/level residual (the recursive teardown of the un-sorted
    // intermediate), the ladder above read it as **0 B/level** — because both
    // probe points, depth 16 and depth 64, sat inside the subject's ~136 KiB
    // INTERCEPT, where the bisection cannot resolve 48 levels x 437 B. A large
    // intercept reads as a zero slope on a short ladder. Every ladder here must
    // therefore span far enough that BOTH ends clear the subject's own
    // intercept; that is also why the real bar is `assert_no_slope` over
    // 4 -> 4,096 rather than a two-point slope.
    // The named residual: `Env::get`'s `<Par as Clone>::clone` of a deep bound
    // value. See `substitute_deep_binding_body`.
    //
    // Measured 2026-07-26: 15,850 B/level debug — `<Par as Clone>::clone`'s
    // 15,872 to within 0.2% — and 7,247 B/level release against `clone`'s own
    // 2,867. The release factor of ~2.5 is inlining: at `-O2` LLVM folds a
    // couple of levels of the recursive `Par::clone` into the enclosing frame
    // at THIS call site, which does not happen when `clone` is probed on its
    // own. Same traversal, larger per-level frame. Ceilings are ~1.5× measured,
    // per profile, as everywhere else in this test.
    assert_slope_below("substitute_deep_binding", ceiling(25_000, 12_000), 16, 128);
    assert_slope_below("pretty", ceiling(65_000, 7_000), 16, 64);
    assert_slope_below("clone", ceiling(25_000, 5_000), 16, 128);
    assert_slope_below("drop", ceiling(1_500, 800), 256, 4096);
    assert_slope_below("encode", ceiling(4_000, 1_500), 64, 1024);
    // ⚠ The RSpace codec — see `bincode_ser_body`. UNCAPPED, unlike `prost`.
    // Probed SHALLOW: at 3,052 B/level (debug) the ENCODER is the cheap half,
    // but both probe points still clear its own intercept at both ends, so a
    // large intercept cannot read as a zero slope on a short ladder.
    //
    // ⚠ Only the ENCODER is left here. `bincode_de` has LEFT this list — it is
    // in `converted_traversals_are_depth_independent` (Stage F: the cold-store
    // decoder became `models/src/rust/rholang/par_codec.rs`, an explicit
    // obligation-stack machine). Measured immediately before the conversion by
    // direct bisection of this very subject: 28,331 B/level debug (262,144 B at
    // depth 8, 942,080 B at depth 32) and 12,971 B/level release (122,880 and
    // 434,176) — i.e. D_max 73 / 161 on a 2 MiB worker, the SHALLOWEST member
    // of this family. As always, a traversal leaves this list only by being
    // converted, never by having its ceiling raised.
    //
    // The ENCODER stays Θ(depth) on purpose: leaving `Serialize` derived is
    // what makes the cold-store leaf bytes byte-identical by construction, and
    // an encode is only ever performed on a term the node itself built, so it
    // is a transient worker fault rather than the permanent, replicated one the
    // decoder was.
    assert_slope_below("bincode_ser", ceiling(5_000, 800), 64, 512);
    // ⚠ THE NAMED STAGE C-2 RESIDUAL. Ceilings are the MEASURED PRE-CONVERSION
    // BASELINES (79,053 / 82,534 B/level, debug), so this list can only ever
    // certify that the self-contained set/map arms did not get worse. Measured
    // after the per-arm frame split: 14,336 / 17,818 — 5.5x and 4.6x BELOW
    // those baselines, and below the derived `clone_nested_set` control
    // (16,384) as well. Probed on a SHORT ladder because of the 3^n sort
    // blow-up documented on `nested_sets`.
    assert_slope_below("sort_nested_set", ceiling(79_053, 7_680), 2, 8);
    assert_slope_below("sort_nested_map", ceiling(82_534, 10_394), 2, 8);
    // The derived floor the two above are measured against.
    assert_slope_below("clone_nested_set", ceiling(25_000, 9_000), 2, 8);
}

/// The WIDTH tripwire. `compare_score_nodes` recurses on the list tail, so its
/// native stack grows with sibling count — an axis the proto-level SCC
/// enumeration structurally cannot see.
///
/// ⚠ In RELEASE the tail call is optimised into a loop and the measured slope
/// is 0. That is a codegen accident, not a property: nothing obliges LLVM to
/// keep doing it, and it does not hold at `-O0` (201 B/sibling measured). The
/// ceiling is therefore per-profile and the axis stays gated until the
/// comparator is an explicit loop.
#[test]
fn theta_width_tripwire() {
    // Every WIDTH-axis member found so far has been converted (Stage C-1: the
    // score-tree sibling walk). The remaining Θ(width) traversal in the family
    // is `FoldMatch::free_check` (483 B per sibling, debug), which is in the
    // MATCHER rather than in the sorter or the substitution SCC; it is measured
    // by `stack_depth_probe.rs` (subject `free_check`) and is not on this
    // conversion's path.
    //
    // This test is retained, named and wired, so that a newly discovered
    // Θ(width) traversal is one line rather than new infrastructure.
    println!("no un-converted width-axis subject in this gate's scope");
}

/// The reported reproducer, at the depth that aborted the reducer, on the stack
/// a tokio worker actually gets when `RUST_MIN_STACK` is unset (Rust's default
/// spawned-thread size is 2 MiB).
///
/// This is the end-to-end statement of the bug in one assertion: it is the
/// depth at which `@"OUT"!([[…[0]…]])` stopped working.
///
/// ★ GREEN as of Stage B (the explicit-worklist conversion of the substitution
/// SCC). Leg-1 (de-cloning) removed the O(D²) heap churn and 25% of the release
/// per-level cost but — exactly as the eval-SCC Leg-1 verdict predicted for
/// itself (`bb7fcd20`) — did not change the CLASS; Leg-2 did.
///
/// ⚠ **THIS TEST GOING GREEN IS NOT THE END OF THE WORK.** `substitute` for
/// `Par` is `substitute_no_sort` followed by `ParSortMatcher::sort_match`
/// (`substitute.rs`, `SubstituteTrait<Par>::substitute`). Converting only
/// `substitute_no_sort` leaves the sorter in the path at 78,579 B/level debug,
/// which supports depth ≈ 26 on a 2 MiB worker — comfortably past the depth-10
/// reproducer. So this assertion flips green while
///   * `ParSortMatcher` is still Θ(depth),
///   * `compare_score` is still Θ(depth),
///   * `compare_score_nodes` is still Θ(**width**), and
///   * `PrettyPrinter` is still Θ(depth).
///
/// The definition of done for the FAMILY is
/// `converted_traversals_are_depth_independent` carrying every member at depth
/// **and width** 256, in **both** profiles — not this test.
#[test]
fn reported_reproducer_depth_survives_a_default_worker_stack() {
    const DEFAULT_SPAWNED_THREAD_STACK: usize = 2 * 1024 * 1024;
    // Debug frames are ~7× release, so the depth a 2 MiB worker can carry is
    // profile-dependent while the CLASS is not. Pin each profile to the depth
    // the measurements say it must reach.
    let depth = if cfg!(debug_assertions) { 10 } else { 70 };
    assert!(
        runs_within(DEFAULT_SPAWNED_THREAD_STACK, depth, "substitute"),
        "REGRESSION: `[[…[0]…]]` at depth {} no longer survives substitution on a \
         {} MiB thread — the reported consensus-liveness bug is back or worse.",
        depth,
        DEFAULT_SPAWNED_THREAD_STACK / (1024 * 1024)
    );
}
