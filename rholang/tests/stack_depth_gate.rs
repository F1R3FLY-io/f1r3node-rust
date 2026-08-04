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

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, New, Par, Receive, ReceiveBind};
use models::rust::rholang::par_children::{dismantle, dismantle_all};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::score_tree::{ScoreAtom, ScoredTerm, Tree};
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::{new_freevar_par, new_gint_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{RuntimeBudget, SignedProcess};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;

// ---------------------------------------------------------------------------
// term construction — ITERATIVE, so the builder itself is never the constraint
// ---------------------------------------------------------------------------

fn elist(ps: Vec<Par>) -> Par {
    models::par_from_default! {
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
fn nested_list(depth: usize) -> Par { nested_list_leaf(depth, 0) }

/// The non-vacuous spatial pattern: a free leaf and a true cached
/// `connective_used` bit at every enclosing list.
fn nested_list_pattern(depth: usize) -> Par {
    let mut p = new_freevar_par(0, Vec::new());
    for _ in 0..depth {
        p = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![p],
                    locally_free: Vec::new(),
                    connective_used: true,
                    remainder: None,
                })),
            }],
            connective_used: true,
            ..Default::default()
        };
    }
    p
}

/// [`nested_list`] with the LEAF integer chosen.
///
/// ★ Exists for the comparison subjects (`eq`, `ord`, `hash`). Two terms that
/// differ only at the deepest level cannot let a short-circuiting comparator
/// stop early: `<Par as PartialEq>::eq` returns `false` at the FIRST unequal
/// field, so a probe built from two terms that differ at level 0 would measure
/// one frame and report a comfortable zero for a Θ(depth) traversal. Differing
/// only at the leaf forces the full descent, and the subject bodies assert the
/// verdict (`Equal` / `Less` / "hashes differ") that only a full descent can
/// produce.
fn nested_list_leaf(depth: usize, leaf: i64) -> Par {
    let mut p = new_gint_par(leaf, vec![], false);
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
        p = models::par_from_default! {
            news: vec![New {
                bind_count: 1,
                p: Some(models::par_from_default! {
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
/// Stage C-2b exposes each raw set member to the same explicit-worklist sorter
/// that handles every other `Par` child. Post-order combination canonical-sorts
/// and deduplicates the already-scored children without re-entering
/// `ParSortMatcher`. A unary chain therefore performs one sort per level, not
/// an exponential cascade of nested sorts, and this shape is a converted-depth
/// subject rather than a residual tripwire.
fn nested_sets(depth: usize) -> Par { nested_sets_leaf(depth, 0) }

/// [`nested_sets`] with a selectable deepest value, for full-descent trait
/// probes whose verdict must depend on the leaf.
fn nested_sets_leaf(depth: usize, leaf: i64) -> Par {
    let mut p = new_gint_par(leaf, vec![], false);
    for _ in 0..depth {
        p = models::par_from_default! {
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
        p = models::par_from_default! {
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
// ★ THE SYNTHETIC CONTROL — a subject whose slope is KNOWN BY CONSTRUCTION
//
// Every assertion in this file is a *rejecter*: it exists to refuse a traversal
// whose native stack grows with a program-controlled parameter. Until 2026-07-27
// none of them had ever been SHOWN to refuse one. `assert_depth_independent`'s
// claim that its zero-slope half "cannot be passed by anything with a non-zero
// slope" was DERIVED from `compare_score`'s recorded numbers, and
// `theta_width_tripwire`'s entire body was a `println!` — a green test executing
// nothing. A checker that accepted everything would have satisfied both.
//
// The real Θ(depth) traversals cannot supply the missing evidence: they have all
// been converted, and "the defect is fixed, so it cannot be reproduced" would
// retire the obligation for every guard whose defect has been fixed — which is
// all of them. Where the excluded-class datum comes from is an implementation
// detail; that it is IN the excluded class is the whole requirement. So the gate
// grows its own.
//
// `synthetic_sloped` recurses `param` levels deep with a fixed, non-elidable
// ballast in every frame — a traversal that is Θ(parameter) on whichever axis it
// is driven, by construction rather than by measurement. `synthetic_flat` does
// the identical arithmetic in a `for` loop, so it is Θ(1) by construction. They
// are the two ends of the property the gate exists to separate, and the checkers
// must put them on opposite sides.
//
// ⚠ Deliberately CHEAP: at 96 B of ballast the depth ladder (4 → 4,096) spans
// ~400 KB and the width ladder (4 → 65,536) ~6 MB, so a full bisection is
// seconds. `normalize_recursive` would have served as a real-implementation
// control, but at 43,542 B/level its 4,096 rung is 178 MB and the bisection is
// slow — the wrong trade for a control whose only job is to have a slope.
// ---------------------------------------------------------------------------

/// Bytes of non-elidable ballast in each synthetic frame. Small enough that the
/// ladders stay cheap, large enough that ~4,000 of them dwarf the gate's
/// [`ZERO_SLOPE_TOLERANCE`] by more than an order of magnitude.
const SYNTHETIC_FRAME_BYTES: usize = 96;

/// One frame of KNOWN cost.
///
/// Three things keep the frame from being optimised away, and all three are
/// needed for the control to hold in RELEASE as well as debug:
///
/// * `#[inline(never)]` — otherwise `-O2` collapses the recursion into the
///   caller and the slope disappears into a single frame;
/// * `ballast` is observed AFTER the recursive call, so it is live across it and
///   cannot share storage with the callee's;
/// * there is arithmetic after the call, so this is not a tail call and LLVM
///   cannot rewrite it into a loop — the very optimisation that makes the real
///   `compare_score_nodes` read as 0 B/sibling at `-O2` and which the width
///   tripwire's doc comment warns is "a codegen accident, not a property".
#[inline(never)]
fn synthetic_recurse(level: usize) -> u64 {
    let mut ballast = [0u8; SYNTHETIC_FRAME_BYTES];
    ballast[0] = level as u8;
    ballast[SYNTHETIC_FRAME_BYTES - 1] = (level >> 8) as u8;
    let deeper = match level {
        0 => 0,
        n => synthetic_recurse(n - 1),
    };
    std::hint::black_box(&ballast);
    deeper + u64::from(ballast[0]) + u64::from(ballast[SYNTHETIC_FRAME_BYTES - 1])
}

/// Θ(parameter) native stack, by construction. The subject the checkers must
/// REJECT.
fn synthetic_sloped_body(param: usize) { std::hint::black_box(synthetic_recurse(param)); }

// ---------------------------------------------------------------------------
// ★ THE SYNTHETIC **DESTRUCTOR** CONTROL — a recursive `Drop`, by construction
//
// [`synthetic_recurse`] is a recursive *function*: the gate calls it directly.
// `drop_in_place::<Par>` is recursive *drop glue*: nothing calls it, the
// compiler emits it from the type and runs it when a value falls out of scope.
// Those are different mechanisms, and a control for one is not a control for
// the other — a harness could in principle observe a direct call and miss a
// destructor (it cannot here, because the bisection observes only whether the
// child process survived, but "it cannot" is exactly the kind of claim this
// campaign has repeatedly found to be inherited rather than derived).
//
// So the destructor gets its own control, and it is deliberately recursive:
// `DropChain`'s `Drop` runs `drop_in_place::<Box<DropChain>>` on its tail from
// inside its own frame, which is precisely the shape `EList.ps: Vec<Par>` gives
// `Par`. Its flat twin tears the identical structure down with an explicit
// worklist — the shape `par_children::dismantle` gives `Par`.
//
// The pair is what makes the new `par_drop` / `normalize_drop` subjects
// non-vacuous: it demonstrates that these checkers, applied to a DESTRUCTOR,
// reject a recursive one and accept an iterative one over the same data.
// ---------------------------------------------------------------------------

/// Deep end of the DESTRUCTOR control's ladder. The shallow end is 4, as
/// everywhere else in this file.
///
/// ## Why it is not the function control's 4,096
///
/// [`the_depth_checkers_reject_a_recursive_destructor`] must show its control
/// clearing [`ZERO_SLOPE_TOLERANCE`] eight-fold
/// ([`clears_tolerance_by_an_order_of_magnitude`]), and what a ladder *produces*
/// is `slope × span`. The two controls do not have the same slope — bisected on
/// this tree over 4 → 4,096, 2026-07-27:
///
/// | control                             | debug       | release         |
/// |-------------------------------------|-------------|-----------------|
/// | [`synthetic_recurse`] (a function)  | 159 B/level | **111 B/level** |
/// | [`DropChain`]'s glue (a destructor) |  95 B/level | **32 B/level**  |
///
/// The gap is not a defect and cannot be closed by enlarging
/// [`SYNTHETIC_FRAME_BYTES`]. A function observes its ballast with `black_box`
/// *after* the recursive call, so the array is live across it and must occupy a
/// frame slot; `drop_in_place::<DropChain>` never reads the ballast at all
/// (`[u8; N]` has no destructor), so at `-O2` the frame holds the tail pointer
/// and the saved registers and nothing else — 32 B whatever `N` is. That is
/// spelled out on the `RETURN_ADDRESS_BYTES` floor in the leg itself.
///
/// So the destructor's ladder must span further, in inverse proportion, to carry
/// the same weight of evidence. Release is the binding profile; bisected on this
/// tree, 2026-07-27:
///
/// | span (4 → hi) | release growth | vs. `8 × ZERO_SLOPE_TOLERANCE` (131,072 B) |
/// |---------------|----------------|--------------------------------------------|
/// | 4,096         |        126,976 | **0.97× — RED, short by one 4 KiB bucket** |
/// | 8,192         |        258,048 | 1.97×                                      |
/// | **16,384**    |    **520,192** | **3.97×**                                  |
/// | 32,768        |      1,044,480 | 7.97×                                      |
///
/// Debug never saw it: at 95 B/level the same 4 → 4,096 ladder grows 389,120 B,
/// i.e. 2.97× the bar, which is why the leg was calibrated there and shipped
/// green. This is the drop-glue counterpart of the general fact recorded in this
/// module's header — release inlining shrinks frames, and a margin that was only
/// ever checked at `-O0` is a margin that was never checked.
///
/// 16,384 is chosen because it restores *parity of evidence*: over its own
/// 4 → 4,096 ladder the function control grows 454,656 B in release, i.e. 3.47×
/// the same bar. The destructor control now stands at 3.97× — the same order,
/// derived from the same arithmetic, rather than a number tuned until a test
/// went green.
///
/// ★ **Lengthening a ladder can only make a control easier to accept, so it is
/// exactly the change that must be shown not to have gone vacuous.** The leg
/// therefore runs [`clears_tolerance_by_an_order_of_magnitude`] on the ITERATIVE
/// twin over this same 16,384-rung ladder and requires `false`: at 12,288 B from
/// end to end its growth is 0, so the bar still rejects a genuinely flat subject
/// after the change.
///
/// ⚠ It is bounded ABOVE, too. `slope_below_verdict`'s flat ceiling is
/// `ZERO_SLOPE_TOLERANCE / span` in integer arithmetic, so a span past 16,384
/// drives that ceiling to a literal 0 — the one zero-margin comparison this file
/// warns against, in
/// [`the_depth_checkers_reject_a_known_theta_depth_subject`]'s `flat_ceiling`
/// note. At 16,380 steps it is 1 B/level, which admits a 7-bucket disagreement
/// between the flat twin's two bisections; `zero_slope_verdict` binds first at
/// 4 buckets, so the tripwire half stays the looser of the two and no new flake
/// is introduced.
const DESTRUCTOR_CONTROL_HI: usize = 16_384;

/// A linked list whose derived `Drop` is recursive: dropping the head drops the
/// `Box`, which drops the next `DropChain`, from inside the head's frame.
///
/// `ballast` makes each frame cost something measurable, for the same reason
/// [`synthetic_recurse`] carries one.
struct DropChain {
    ballast: [u8; SYNTHETIC_FRAME_BYTES],
    next: Option<Box<DropChain>>,
}

/// Build `param` links ITERATIVELY, so construction is never the constraint —
/// the same discipline every term builder in this file follows.
fn drop_chain(param: usize) -> DropChain {
    let mut head = DropChain {
        ballast: [0u8; SYNTHETIC_FRAME_BYTES],
        next: None,
    };
    for level in 0..param {
        let mut ballast = [0u8; SYNTHETIC_FRAME_BYTES];
        ballast[0] = level as u8;
        head = DropChain {
            ballast,
            next: Some(Box::new(head)),
        };
    }
    head
}

/// Count the links ITERATIVELY — the anti-vacuity walker for this fixture.
fn drop_chain_len(head: &DropChain) -> usize {
    let mut n = 0usize;
    let mut cur = head.next.as_deref();
    while let Some(link) = cur {
        n += 1;
        cur = link.next.as_deref();
    }
    n
}

/// Θ(parameter) native stack **in a destructor**, by construction. The subject
/// the checkers must REJECT.
///
/// The `Drop` here is the one Rust derives for a type owning a `Box` of itself;
/// it is not written out, exactly as `drop_in_place::<Par>` is not.
fn synthetic_drop_body(param: usize) {
    let head = drop_chain(param);
    assert_carries(
        "the synthetic drop chain's length",
        drop_chain_len(&head),
        param,
    );
    std::hint::black_box(head.ballast[0]);
    drop(head);
}

/// Θ(1) native stack, same structure, iterative teardown — the subject the
/// checkers must ACCEPT, so a checker that rejected every destructor fails too.
///
/// This is `par_children::dismantle` in miniature: detach each link into a local
/// before the previous one is released, so no destructor is ever running inside
/// another destructor's frame.
fn synthetic_drop_flat_body(param: usize) {
    let mut head = drop_chain(param);
    assert_carries(
        "the synthetic drop chain's length",
        drop_chain_len(&head),
        param,
    );
    std::hint::black_box(head.ballast[0]);
    let mut cursor = head.next.take();
    while let Some(mut link) = cursor {
        cursor = link.next.take();
        // `link` is released here with its `next` already detached, so its
        // `Drop` has nothing to recurse into.
    }
}

/// Θ(1) native stack, by construction — the same ballast and the same
/// arithmetic, iteratively. The subject the checkers must ACCEPT, so that a
/// checker which rejected everything fails too.
fn synthetic_flat_body(param: usize) {
    let mut acc = 0u64;
    for level in 0..=param {
        let mut ballast = [0u8; SYNTHETIC_FRAME_BYTES];
        ballast[0] = level as u8;
        ballast[SYNTHETIC_FRAME_BYTES - 1] = (level >> 8) as u8;
        std::hint::black_box(&ballast);
        acc += u64::from(ballast[0]) + u64::from(ballast[SYNTHETIC_FRAME_BYTES - 1]);
    }
    std::hint::black_box(acc);
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
        // ★ The METERED wrapper — what the reducer calls. See
        // `subst_and_charge_body`; the name is `stack_depth_probe.rs`'s, so one
        // subject has one name across the measurement harness and the gate.
        "subst_and_charge" => subst_and_charge_body,
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
        // ★★ The two stage-F-4 ladders that `clone` alone cannot stand in for.
        // `clone_send_chain` nests through a DIFFERENT `Vec` field so that a
        // regression to `<Vec<Send> as Clone>::clone` reddens something; and
        // `clone_pathmap_chain` measures the EXTERN obligation the driver's
        // bounded treatment of `EPathMap` rests on. See their bodies.
        "clone_send_chain" => clone_send_chain_body,
        "clone_pathmap_chain" => clone_pathmap_chain_body,
        // ★★ The DERIVED CONTROL for `clone`, in the `bincode_ser_derived` idiom
        // — and the instrument that makes the conversion's "before" leg a
        // MEASUREMENT rather than a manual revert. See `clone_oracle_body`.
        "clone_oracle" => clone_oracle_body,
        // ★ The TEARDOWN control that decides whether `clone_pathmap_chain`'s
        // reading is the CLONE's or the fixture's `Drop`. See its body.
        "pathmap_chain_drop" => pathmap_chain_drop_body,
        // ⚠ RENAMED 2026-07-27, from `drop`. The subject is `drop_in_place::<Par>`
        // and it has been here since the gate was written — but under a name that
        // says only *which operation*, never *on what*. mettail-rust's twin gate
        // calls the identical traversal `par_drop`, so a cross-repo search for
        // `par_drop` found the twin and MISSED this one, and the absence was
        // reported as "the repo that defines `Par` is not measuring its `Drop`".
        // It was measuring it. One name for one traversal, in both repos.
        "par_drop" => par_drop_body,
        "normalize_drop" => normalize_drop_body,
        // ★ `inj_attempt`'s `set-initial-cost` phase. Named for the CLONE it used
        // to perform, in the same spirit as `normalize_drop`: the name records the
        // defect the subject guards against coming back, not the code that is
        // there today.
        "inj_attempt_clone" => inj_attempt_clone_body,
        "encode" => encode_body,
        // ── ★ The four-quadrant S0 baseline subjects. ──
        //
        // Five traversals over the `Par` family that this gate had never
        // measured at S0. All five have since moved into `CONVERTED_DEPTH`:
        // generated PDAs now implement Eq, Hash, Ord, Debug, and protobuf
        // decode. Historical absence meant UNMEASURED, never FLAT.
        //
        // They are deliberately in NEITHER register list: `four_quadrant_s0_
        // baseline` measures them and records the numbers, it does not yet claim
        // a ceiling for any of them. A claim is a later stage's deliverable, and
        // adding a name to `TRIPWIRE_DEPTH` without an `assert_slope_below` fails
        // `theta_depth_tripwire`'s register loop by construction.
        "eq" => eq_body,
        "hash" => hash_body,
        "hash_nested_set" => hash_nested_set_body,
        "hash_pathmap_set" => hash_pathmap_set_body,
        "hash_pathmap_map" => hash_pathmap_map_body,
        "ord" => ord_body,
        "debug" => debug_body,
        "protobuf_de" => protobuf_de_body,
        "message_clear" => message_clear_body,
        "bincode_ser" => bincode_ser_body,
        "bincode_ser_derived" => bincode_ser_derived_body,
        "bincode_de" => bincode_de_body,
        "normalize" => normalize_body,
        "spatial_binding" => spatial_binding_body,
        "spatial_concrete_binders" => spatial_concrete_binders_body,
        // -------- width axis --------
        "substitute_wide" => substitute_wide_body,
        "sort_wide" => sort_wide_body,
        "score_cmp_wide" => score_cmp_wide_body,
        "pretty_wide" => pretty_wide_body,
        "free_check" => free_check_body,
        "normalize_wide" => normalize_wide_body,
        "eval_with_nots" => eval_with_nots_body,
        // -------- Phase 7 measurement controls (not production claims) --------
        "phase7_invariant" => phase7_invariant_body,
        "heap_fixture_par" => heap_fixture_par_body,
        "heap_fixture_normalize" => heap_fixture_normalize_body,
        "heap_fixture_eval" => heap_fixture_eval_body,
        // -------- synthetic controls (either axis; see SYNTHETIC_FRAME_BYTES) --------
        "synthetic_sloped" => synthetic_sloped_body,
        "synthetic_flat" => synthetic_flat_body,
        "synthetic_drop" => synthetic_drop_body,
        "synthetic_drop_flat" => synthetic_drop_flat_body,
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

/// Export the converted register for the external deterministic-time harness.
///
/// The `PHASE7_MANIFEST` prefix is intentionally machine-readable. The harness
/// must consume these constants rather than carrying a second subject list that
/// can drift away from the gate.
#[test]
#[ignore = "machine-readable input for scripts/bench/stack-safety-phase7.sh"]
fn phase7_measurement_manifest() {
    // `--nocapture` may print libtest's `test ...` prefix without a newline.
    // End that line explicitly so the first machine-readable row starts at
    // column zero like every row after it.
    println!();
    for name in CONVERTED_DEPTH {
        println!("PHASE7_MANIFEST\tdepth\t{name}");
    }
    for name in CONVERTED_WIDTH {
        println!("PHASE7_MANIFEST\twidth\t{name}");
    }
    println!(
        "PHASE7_MANIFEST_COUNT\t{}",
        CONVERTED_DEPTH.len() + CONVERTED_WIDTH.len()
    );
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

// ---------------------------------------------------------------------------
// ★ THE SUBJECT REGISTER — one place, and the audit is checked AGAINST it
//
// Until 2026-07-27 the converted and tripwired sets existed twice: here as
// executable fact, and in `docs/design/audits/theta-depth-traversals-2026-07-26.md`
// as prose. Two copies of one truth do not stay equal, and these did not: the
// audit's §11.4 named 7 converted subjects, its §12.6 and §12.9 named 13, and
// the gate carried 17. Both prose copies had gone stale WITHIN THE HOUR of being
// reconciled, twice.
//
// Reconciling them a third time would buy another hour. So the direction of
// truth is now declared and CHECKED: these constants are the source, the audit
// carries one generated block, and `the_audit_agrees_with_the_gate` fails at the
// commit that separates them rather than at the next audit.
//
// ⚠ The constants are not themselves a transcription of the assertions below
// them. `converted_traversals_are_depth_independent` *iterates* these lists — a
// name here is a test that runs — and `theta_depth_tripwire` RECORDS every
// subject it asserts and refuses to finish unless the recorded set is exactly
// [`TRIPWIRE_DEPTH`]. So a subject cannot be gated without appearing here, and
// cannot appear here without being gated.
// ---------------------------------------------------------------------------

/// Depth-axis traversals converted to a heap-bounded (explicit worklist) form.
/// Membership is the deliverable; it only ever grows. A traversal enters only by
/// being converted, never by having a ceiling raised.
const CONVERTED_DEPTH: &[&str] = &[
    // Stage B — the driver
    "substitute_no_sort",
    // Stage B — the driver, under a deep environment
    "substitute_binders",
    // Stage C-2 (the sorter) + Stage E (the intermediate is dismantled instead
    // of dropped recursively)
    "substitute",
    // Stage C-2 — ParSortMatcher
    "sort",
    // Stage C-2b — ESet/EMap members are children of the same PDA; their
    // combines consume already-scored children and never re-enter the sorter.
    "sort_nested_set",
    "sort_nested_map",
    // Stage C-1 — the score-tree comparator
    "score_cmp",
    // Stage C-1 — Tree's hand-written Drop
    "tree_drop",
    // Stage C-1 — Tree's hand-written Clone
    "tree_clone",
    // Stage E — rho-pure-eval's own SCC
    "eval_with_nots",
    // Stage F — the cold-store DECODER (bincode_decoder)
    "bincode_de",
    // Stage H — the cold-store ENCODER (bincode_encoder). It LEFT the tripwire
    // list below by being CONVERTED, never by having its ceiling raised: the
    // derived `Serialize` is Θ(depth) at 3,052 / 329 B per level (debug /
    // release, measured 2026-07-26) and the single-walk machine holds its
    // native stack flat from depth 4 to 4,096 on both profiles.
    "bincode_ser",
    // Stage D — PrettyPrinter's explicit pushdown driver
    "pretty",
    // Stage G — `normalize_ann_proc`'s 26-function SCC becomes
    // `compiler::normalize_drive::norm_drive`. 43,542 → 0 B/level debug and
    // 7,261 → 0 release; the ONLY member that runs before metering exists, so
    // nothing else could have bounded it.
    "normalize",
    // ★ `inj_attempt`'s `set-initial-cost` phase. It entered this list by being
    // CONVERTED, not by having a ceiling raised: the write-then-read-back
    // `source_process().cloned()` became the by-move `into_source_process()`, and
    // the composition went from 2,852 B/level release (max depth 729 on a 2 MiB
    // worker) to a FLAT 32,768 B across 4 → 4,096. See `inj_attempt_clone_body`
    // for the before/after table and the bisection evidence.
    "inj_attempt_clone",
    // ★★ Stage F-4 — `<Par as Clone>::clone`. It LEFT `TRIPWIRE_DEPTH` below by
    // being CONVERTED, never by having its ceiling raised: `models/build.rs`
    // STRIPS the `Clone` derive from the 55 non-`Copy` `rhoapi` items and
    // `models/codegen/schema.rs` GENERATES the impls, `Par`'s over
    // `drive::drive_with`. The derived form measured 16,493 B/level debug and
    // 3,254 release (`four_quadrant_s0_baseline`, re-measured at HEAD before the
    // conversion); the driven form holds its native stack flat from depth 16 to
    // 4,096 on both profiles.
    //
    // ⚠ The conversion is at a FEEDBACK VERTEX SET of the child relation, not at
    // every recursive type: `{Par}`, derived and verified by the generator. The
    // other 54 `Clone`s are generated FIELD-WISE and are flat because every cycle
    // passes through `Par`, reaching a driven clone within a residual height of 3.
    "clone",
    // ★ The `Vec`-NESTED ladder, and it is not a duplicate of `clone`. See
    // `clone_send_chain_body`: a ladder that nests through only ONE collection
    // field cannot show that the others were converted too, and the sibling
    // repo's eight Θ(depth) "iterative" drivers all escaped through exactly this
    // boundary — `Category → Vec<Elem> → Elem`.
    "clone_send_chain",
    // Stage F-5 — `<Par as Ord>::cmp`. `models/build.rs` strips `Ord` and
    // `PartialOrd` exactly at the descriptor-derived feedback vertex set and
    // `models/codegen/schema.rs` emits the lexicographic PDA. Its recursive
    // oracle is test-only; the generated differential and this gate cover
    // semantic equivalence and native-stack flatness through depth 4,096.
    "ord",
    // Stage F-6 — `<Par as Debug>::fmt`. `models/build.rs` injects prost's
    // `skip_debug` exactly at the descriptor-derived feedback vertex set and
    // `models/codegen/schema.rs` emits a declaration-order formatting PDA.
    // A generated recursive builder oracle checks both compact and alternate
    // output on a shallow corpus; this gate checks stack flatness through 4,096.
    "debug",
    // ★★ Stage F-4, SECOND ORDER — `Env::get`'s deep splice. It LEFT
    // `TRIPWIRE_DEPTH` below by being CONVERTED, never by having a ceiling
    // raised, and the conversion is the one `0eac9c3a` already landed: this
    // subject's entire slope WAS `<Par as Clone>::clone` over the bound value
    // (measured 15,850 B/level debug, "`Par::clone`'s 15,875 to within 0.2%"),
    // so driving `Par::clone` drove this too.
    //
    // ⚠ Nobody re-measured it on the REAL bar, and the tripwire could not see
    // the change: `assert_slope_below` ran it on 16 → 128, where the subject
    // reported a FALSE ZERO both before and after — a large intercept reads as a
    // flat ladder, the failure mode this file already records for `substitute`'s
    // 437 B/level residual. It is admitted here only because it clears
    // `assert_depth_independent`'s 4 → 4,096 span, which a non-zero slope
    // cannot pass at any constant.
    //
    // ★ **The parenthetical that stood here — "`slope_below_verdict` consults only
    // `per_step`" — was TRUE and is now SUPERSEDED, and the distinction matters
    // because the two failure modes were being conflated.** `per_step` is still
    // what the verdict compares, but it is no longer a quotient of a bare
    // `saturating_sub`: a floor reading is now a typed [`MinStack::BelowResolution`]
    // and the difference is an explicit BOUND rather than a number, so the three
    // paths that used to reach a verdict as `0` — both ends at the floor, one end
    // at the floor, and a NEGATIVE difference — are each dispositioned on their own
    // terms. See [`Unresolved`].
    //
    // ⚠⚠ **That repair does NOT fix this subject's reading, and it was never going
    // to.** The intercept problem is a property of the LADDER, not of the
    // arithmetic: at 16 → 128 both probe points sit inside this subject's own
    // ~136 KiB intercept, both readings are genuine measurements well above the
    // instrument floor, and their difference is genuinely small. No amount of type
    // safety recovers a slope from a ladder too short to express it. **The remedy
    // for an intercept is a LONGER LADDER; the remedy for a floor reading is a
    // typed refusal to call it a number. Two defects, two repairs, and reading
    // either as the other is how the wrong prescription got written down.**
    //
    // ⚠ The superseded rationale is kept verbatim so it cannot be restored as a
    // bug fix: *"`Env::get` clones a deep bound value. It STAYS here: `Env::shift`
    // is `Env { shift: .., ..(*self).clone() }`, i.e. a `HashMap<i32, Par>`
    // clone, and while each entry's `Par::clone` is now DRIVEN, the map walk
    // that reaches them is a different traversal from the one stage F-4
    // converted."* That is true of `Env::shift`, which `substitute_binders`
    // measures; it is NOT true of this subject, which never shifts. The map
    // here holds ONE binding and `Env::get` clones that one value.
    "substitute_deep_binding",
    // Stage F-4, THIRD ORDER — `<Par as Clone>::clone` over the nested-`ESet`
    // shape. Same conversion as `clone` and `clone_send_chain`, reached through
    // a third container field: `Par -> Expr -> ESetBody -> ESet.ps: Vec<Par>`.
    // The generated driver enters it (`clone_push_children_e_set`), the cut set
    // is `["Par"]` and `CLONE_RESIDUAL_HEIGHT` is 3, so the chain reaches a
    // driven clone after a bounded native prefix and the ladder is flat.
    //
    // It is measured independently from nested collection sorting; all three
    // subjects now satisfy the same full 4 → 4,096 depth-independence bar.
    "clone_nested_set",
    // Generated stack-safe trait and codec surfaces over the same schema cut
    // set. Each is held to the full 4 -> 4,096 depth-independence ladder.
    "subst_and_charge",
    "par_drop",
    "normalize_drop",
    "encode",
    "protobuf_de",
    "eq",
    "hash",
    // The generated Hash PDA's three collection-specific ladders. The first is
    // the historical control promised when the legacy ESet sorter still had a
    // HashSet<Par> residue. The latter two cover the homogeneous EPathMap
    // specializations directly: PathMap<()> membership bytes and PathMap<Par>
    // values. They are independent because neither representation may be
    // flattened through the other's API.
    "hash_nested_set",
    "hash_pathmap_set",
    "hash_pathmap_map",
    // `prost::Message::clear` has its own generated PDA. The protobuf envelope
    // test proves its semantics; this subject independently proves the cost is
    // native-stack flat rather than inheriting that conclusion from Drop.
    "message_clear",
    // PathMap<Par>-specific clone and teardown ladders. Their fixtures use map
    // values rather than recursively re-encoding set keys, so construction and
    // validation are linear in depth and need no capped ladder or large stack.
    "clone_pathmap_chain",
    "pathmap_chain_drop",
    // Matcher SCC closure. The first forces binding/connective semantics; the
    // second forces the formerly recursive concrete Receive/New fast path.
    "spatial_binding",
    "spatial_concrete_binders",
];

/// Width-axis traversals converted to a heap-bounded form. Same rule.
const CONVERTED_WIDTH: &[&str] = &[
    "substitute_wide", // Stage B
    "sort_wide",       // Stage C-2
    "score_cmp_wide",  // Stage C-1 — the sibling walk
    "free_check",      // Stage E — the matcher's width-axis member, now a `for` loop
    "pretty_wide",     // Stage D — the same driver, sibling axis
    "normalize_wide",  // Stage G — the same driver, collection-element axis
];

/// No production depth traversal remains under a slope ceiling. The tripwire
/// test still executes its recursive and iterative synthetic controls so an
/// accidentally permissive checker cannot turn this empty register vacuous.
const TRIPWIRE_DEPTH: &[&str] = &[];

/// Width-axis traversals still Θ(width). Empty, and that is an EXECUTED claim —
/// see `theta_width_tripwire`'s `UNCONVERTED_WIDTH_SUBJECTS`.
const TRIPWIRE_WIDTH: &[&str] = &[];

// Every subject [`assert_slope_below`] was driven with on this thread. Each
// `#[test]` runs on its own thread, so a thread-local is both correct and
// lock-free here — no shared state to interleave, no mutex to contend.
thread_local! {
    static SLOPE_SUBJECTS_DRIVEN: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Bisection resolution, in bytes. Both the zero-slope tolerance and the
/// tripwire's derived slope are quantised to this.
const RESOLUTION: usize = 4096;

/// ⚠★★ **THE INSTRUMENT FLOOR.** The smallest value [`min_stack_for`] can return for any
/// subject on any build — a property of the ALGORITHM, not of any traversal.
///
/// `min_stack_for` opens with `hi = 16 KiB`. A subject that survives 16 KiB never enters the
/// doubling loop, so the bisection runs on `[8192, 16384]`: `hi - lo = 8192 > 4096` gives
/// `mid = 12288`; the probe succeeds, `hi` becomes 12288, and `hi - lo = 4096` is not
/// `> RESOLUTION`, so the loop exits returning **12,288**. Nothing below that is reachable.
///
/// **And the floor sits far above the true minimum**, measured: `gate_child` runs on
/// `thread::Builder::stack_size(stack)`, which `std` clamps to `PTHREAD_STACK_MIN` plus TLS.
/// `GATE_STACK=1` — one byte — SURVIVES. ⇒ **Every probe below ~16 KiB is the same probe.**
///
/// ★ This constant exists so a floor reading can be RENDERED as a floor instead of as a
/// number. `d0279621` offered `"release 12 KiB at 4 → 12 KiB at 4,096, O(1)"` as measured
/// evidence for an admission — in a commit whose own thesis is that instrument geometry is
/// not measurement.
///
/// ⚠ Ported from `mettail-rust`'s gate (`125065a8`), which named THIS gate as affected and
/// fixed only its own. **Port the TYPE, not the reason**: mettail uses
/// `setrlimit(RLIMIT_STACK)` before `exec`, so nothing clamps and its floor is
/// environment-size-bound; this gate uses `thread::Builder::stack_size`, which `std` clamps.
/// Same artefact, different mechanism.
const SMALLEST_POSEABLE_STACK: usize = 12 * 1024;

/// A bisected minimum-stack reading, or the admission that the instrument could not resolve
/// one.
///
/// ⚠ **The whole mechanism is that [`MinStack::BelowResolution`] never renders as a number.**
/// A `usize` return makes a floor reading indistinguishable from a measurement, and
/// arithmetic on it — a difference, a quotient, a per-step slope — yields figures that look
/// measured and are artefacts of the probe window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MinStack {
    /// A resolved reading, strictly above the floor.
    Bytes(usize),
    /// The bisection bottomed out at [`SMALLEST_POSEABLE_STACK`]. The subject's true minimum
    /// is *at or below* that; the instrument cannot say where.
    BelowResolution,
}

impl std::fmt::Display for MinStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MinStack::Bytes(bytes) => write!(f, "{} KiB", bytes / 1024),
            // ⚠ Never a number. That is the entire mechanism.
            MinStack::BelowResolution => write!(
                f,
                "<{} KiB (BELOW THE INSTRUMENT FLOOR)",
                SMALLEST_POSEABLE_STACK / 1024
            ),
        }
    }
}

impl MinStack {
    /// An **UPPER BOUND** on the reading, as a number — for the one thing that genuinely
    /// needs one: re-probing at a stack size that is known to suffice for this subject.
    ///
    /// ★★ A floor reading yields [`SMALLEST_POSEABLE_STACK`], and that is sound *for this
    /// use specifically*, not a relaxation. Every caller asks the same question:
    ///
    /// > *"On a stack that suffices for X, does Y survive?"*
    ///
    /// A floor reading says X's true need lies in `[0, FLOOR]`, so `FLOOR` is an upper bound
    /// on it. Probing Y at an upper bound makes the claim **harder** to establish — Y is
    /// given more stack than X actually needed — so a `false` result holds a fortiori.
    ///
    /// ⚠ **This is the one direction in which a floor value may be used as a number**, and
    /// the reason is that the conclusion is monotone in it. It must NOT be reached for by
    /// slope arithmetic, where the same substitution would manufacture the very
    /// zero-difference this type exists to prevent — [`Ladder::growth`] handles that case
    /// and refuses where refusal is right.
    ///
    /// The `subject` parameter is kept for the panic-free call sites' readability and for
    /// any future clause that needs to name which reading it bounded.
    fn bytes_upper_bound(self, _subject: &str) -> usize {
        match self {
            MinStack::Bytes(bytes) => bytes,
            MinStack::BelowResolution => SMALLEST_POSEABLE_STACK,
        }
    }
}

/// Smallest stack (to `RESOLUTION` granularity) on which `name` survives at
/// `depth`. Exponential probe, then bisect.
///
/// ★ Returns [`MinStack`], not `usize`, so a reading that bottomed out at the instrument
/// floor cannot be arithmetic'd into a slope. See [`SMALLEST_POSEABLE_STACK`].
fn min_stack_for(name: &str, depth: usize) -> MinStack {
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
    if hi <= SMALLEST_POSEABLE_STACK {
        MinStack::BelowResolution
    } else {
        MinStack::Bytes(hi)
    }
}

/// The maximum growth in minimum-stack, across the whole zero-slope ladder,
/// that still counts as "no growth". Four bisection buckets over a ~4,000-step
/// ladder is under 4 bytes per step — far below any real per-level frame (the
/// cheapest measured member of this family is `Tree`'s `Drop` at 370 B/level).
const ZERO_SLOPE_TOLERANCE: usize = 4 * RESOLUTION;

/// **The evidence-margin VERDICT.** `true` iff a control's measured growth
/// clears [`ZERO_SLOPE_TOLERANCE`] by an order of magnitude.
///
/// Every reddening leg asserts this of the sloped subject it drives, for one
/// reason: [`Ladder::growth`] is a difference of two bisections each quantised
/// to [`RESOLUTION`], so it carries `±2 * RESOLUTION` of quantisation
/// uncertainty. A control whose growth merely *exceeds* the tolerance could be
/// clearing it on rounding; one that exceeds it eight-fold cannot.
///
/// ★ It is a named function rather than three copies of one expression because
/// the three legs must not drift, and because a leg that must demonstrate this
/// clause can then REJECT with the very same expression it asserts — see
/// [`the_depth_checkers_reject_a_recursive_destructor`], which runs it on its
/// flat twin and requires a `false`.
fn clears_tolerance_by_an_order_of_magnitude(l: Ladder) -> bool {
    // ⚠ An UNRESOLVED ladder answers `false`, and that is the conservative direction: this
    // predicate claims "definitely sloped, by a wide margin", and an instrument that could
    // not read the ladder has not established that. The effect is that a POSITIVE control
    // built on an unreadable ladder fails loudly rather than certifying itself — which is
    // what a control that cannot be read should do.
    l.growth()
        .map(|g| g > 8 * ZERO_SLOPE_TOLERANCE)
        .unwrap_or(false)
}

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
///
/// ★ Both of those sentences were, until 2026-07-27, ARITHMETIC ON RECORDED
/// NUMBERS — no subject in this gate had ever been a known-Θ(depth) traversal
/// fed to [`assert_no_slope`] and seen to fail, so nothing distinguished this
/// checker from one that accepts everything. `the_depth_checkers_reject_a_known_
/// theta_depth_subject` (run as the first phase of `theta_depth_tripwire`) now
/// executes both: the synthetic Θ(depth) control PASSES half (1) at depths 4
/// through 256 and is REJECTED by half (2), with the rejecting clause asserted.
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

/// One rung-pair of a ladder: the bisected minimum stack at each end.
///
/// Splitting this out is what makes the verdicts below **pure functions of
/// observations**, separable from the code that produces them. A verdict that
/// can only be reached by running a subject cannot be exercised on a subject the
/// tree no longer contains — which is exactly how [`assert_no_slope`] came to
/// have no executed rejection for the class it exists to reject. With the
/// measurement and the decision apart, `the_gate_checkers_can_go_red` can hand
/// either verdict any observations at all, including a control's.
#[derive(Debug, Clone, Copy)]
struct Ladder {
    lo_param: usize,
    lo_stack: MinStack,
    hi_param: usize,
    hi_stack: MinStack,
}

/// Bisect both ends of a ladder for `name`.
fn measure_ladder(name: &str, lo_param: usize, hi_param: usize) -> Ladder {
    Ladder {
        lo_param,
        lo_stack: min_stack_for(name, lo_param),
        hi_param,
        hi_stack: min_stack_for(name, hi_param),
    }
}

/// Why a ladder's growth is not a number.
///
/// ⚠★★ **The three FALSE-PASS PATHS this type exists for**, each of which reached a verdict
/// as `0` while `growth()` was `hi_stack.saturating_sub(lo_stack)` over two bare `usize`s:
///
/// | path | old reading | what is actually true |
/// |---|---|---|
/// | both ends at the instrument floor | `0` | both true values lie in `[0, FLOOR]`, so growth ≤ FLOOR — a SOUND but uninformative bound |
/// | one end at the floor | `0` or a wrong difference | an INTERVAL: with a resolved deep end `h`, growth ∈ `[h − FLOOR, h]` |
/// | `hi_stack < lo_stack` | `0`, via `saturating_sub` | genuinely **unresolvable** — the probe disagrees with itself about direction |
///
/// ★★ **Only the third is an unknown, and that is the correction that matters.** The first
/// two were tempting to refuse outright — an early draft of this repair did, and it reddened
/// the gate's own controls, because a sloped subject legitimately needs almost no stack at
/// depth 4 and a great deal at depth 4,096, and a flat one legitimately sits under the floor
/// at both. **A floor reading is a BOUND, not an absence of information.** What must never
/// happen is for the bound to be reported as a measurement — and that is [`MinStack`]'s
/// `Display`'s job, not a refusal's.
///
/// ⇒ [`Ladder::growth`] returns the UPPER bound (what both verdicts need: *"is the slope at
/// most X?"*) and [`Ladder::growth_at_least`] the LOWER one (what a positive control needs:
/// *"is this definitely sloped?"*). Neither can be reached for through a bare subtraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unresolved {
    /// `hi_stack < lo_stack`. Deeper needed LESS stack — the probe is unstable.
    NegativeDifference { lo: usize, hi: usize },
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unresolved::NegativeDifference { lo, hi } => write!(
                f,
                "the deep end needed LESS stack than the shallow end ({} KiB at the top vs {} KiB \
                 at the bottom) — the probe is unstable, and `saturating_sub` used to render this \
                 as zero growth",
                hi / 1024,
                lo / 1024
            ),
        }
    }
}

/// Render a possibly-unresolved figure for a human-readable line.
///
/// ★ `UNRESOLVED` rather than `0`. Every printed line in this gate is read by someone
/// deciding whether a subject regressed, and `0 B/level` from a subject known to be sloped
/// is the most misleading thing the instrument can say.
fn show(v: Result<usize, Unresolved>) -> String {
    match v {
        Ok(n) => n.to_string(),
        Err(_) => "UNRESOLVED".to_string(),
    }
}

impl Ladder {
    /// Growth in minimum stack across the ladder, in bytes — or why it is not a number.
    ///
    /// ★ Replaces `hi_stack.saturating_sub(lo_stack)`, whose three zero-producing paths are
    /// enumerated on [`Unresolved`].
    /// ★★ **A floor reading is an INTERVAL, not an unknown, and conflating the two is a
    /// second way to get this wrong.** A shallow end below the floor means its true value
    /// lies in `[0, FLOOR]`, so with a resolved deep end `h` the growth lies in
    /// `[h - FLOOR, h]`. That is usable — it is only *imprecise*. Refusing it outright
    /// would redden the gate's own positive controls, whose sloped subjects legitimately
    /// need almost no stack at depth 4 and a great deal at depth 4,096.
    ///
    /// ⇒ This returns the **UPPER** bound, because both verdicts ask *"is the slope at most
    /// X?"* and an upper bound is the conservative answer to that question. Callers that
    /// need the other direction — *"is this definitely sloped?"* — use
    /// [`Ladder::growth_at_least`].
    ///
    /// Only two states are genuinely unreadable: **both** ends at the floor (the difference
    /// is bounded by the probe window and its sign is unknown) and a **negative** difference
    /// (the probe disagreeing with itself).
    fn growth(&self) -> Result<usize, Unresolved> {
        match (self.lo_stack, self.hi_stack) {
            // BOTH ends below the floor ⇒ both true values lie in `[0, FLOOR]`, so the
            // growth is at most FLOOR. ★ SOUND, and deliberately not a refusal: the bound
            // holds, it is simply uninformative about the subject. What must not happen is
            // for it to be REPORTED as a measurement — and that is `MinStack`'s `Display`'s
            // job, which renders both ends as `<12 KiB (BELOW THE INSTRUMENT FLOOR)`.
            (MinStack::BelowResolution, MinStack::BelowResolution) => Ok(SMALLEST_POSEABLE_STACK),
            // Shallow end below the floor ⇒ growth ≤ hi (the floor end is at worst 0).
            (MinStack::BelowResolution, MinStack::Bytes(hi)) => Ok(hi),
            // Deep end below the floor while the shallow end resolved ABOVE it: the deep end
            // needs strictly less than the shallow one. Not an interval — the readings
            // disagree about direction.
            (MinStack::Bytes(lo), MinStack::BelowResolution) => {
                Err(Unresolved::NegativeDifference {
                    lo,
                    hi: SMALLEST_POSEABLE_STACK,
                })
            }
            (MinStack::Bytes(lo), MinStack::Bytes(hi)) if hi < lo => {
                Err(Unresolved::NegativeDifference { lo, hi })
            }
            (MinStack::Bytes(lo), MinStack::Bytes(hi)) => Ok(hi - lo),
        }
    }

    /// The **LOWER** bound on growth — the smallest value consistent with the readings.
    ///
    /// ★ For a POSITIVE control, whose claim is *"this subject is definitely sloped"*, the
    /// lower bound is the honest one: asserting a floor on the slope from an over-estimate
    /// would let a flat subject certify itself as the sloped control.
    fn growth_at_least(&self) -> Result<usize, Unresolved> {
        match (self.lo_stack, self.hi_stack) {
            // Nothing is established: both true values lie in `[0, FLOOR]`, so the growth
            // may be zero.
            (MinStack::BelowResolution, MinStack::BelowResolution) => Ok(0),
            (MinStack::BelowResolution, MinStack::Bytes(hi)) => {
                Ok(hi.saturating_sub(SMALLEST_POSEABLE_STACK))
            }
            _ => self.growth(),
        }
    }

    /// Derived cost per parameter step, in bytes — or why it is not a number.
    fn per_step(&self) -> Result<usize, Unresolved> {
        self.growth().map(|g| g / (self.hi_param - self.lo_param))
    }

    /// The per-step slope of a CONTROL subject, or a panic naming why it could not be read.
    ///
    /// ★ Controls are the one population where an unresolved ladder must ABORT rather than
    /// be handled: a control exists to prove a checker discriminates, and **a control that
    /// cannot be read certifies nothing**. Swallowing the `Err` here would leave the RED
    /// cells asserting against a fabricated `0` — which is the very defect
    /// [`slope_below_verdict`]'s new refusal clause exists to close, reintroduced one layer
    /// up.
    fn per_step_of_control(&self, subject: &str) -> usize {
        self.growth_at_least()
            .map(|g| g / (self.hi_param - self.lo_param))
            .unwrap_or_else(|why| {
                panic!(
                    "CONTROL `{subject}` produced an UNREADABLE ladder: {why}.\n\
                 A control that cannot be read is not a control. Move the ladder's ends so \
                 both clear the {} KiB instrument floor; do not relax the assertion that \
                 depends on it.",
                    SMALLEST_POSEABLE_STACK / 1024
                )
            })
    }

    /// The growth of a CONTROL subject, or a panic naming why it could not be read.
    /// See [`Ladder::per_step_of_control`].
    fn growth_of_control(&self, subject: &str) -> usize {
        self.growth_at_least().unwrap_or_else(|why| {
            panic!(
                "CONTROL `{subject}` produced an UNREADABLE ladder: {why}.\n\
                 A control that cannot be read is not a control."
            )
        })
    }
}

/// **The zero-slope VERDICT.** A pure function of one [`Ladder`]: `Ok` iff the
/// minimum stack did not grow across it. `Err` carries the message, so a caller
/// can assert WHICH clause rejected rather than only that something did.
///
/// ★★ **A floor reading is a legitimate PASS here, and that asymmetry with
/// [`slope_below_verdict`] is deliberate.** This gate asks *"is the cost independent of the
/// parameter?"* A subject that still fits under the instrument floor at parameter 4,096 has
/// answered yes — its per-level cost is bounded below ~4 B/level by the floor itself, which
/// is a real bound however coarse. The tripwire asks a different question and cannot accept
/// the same reading; see there.
///
/// ⚠ A NEGATIVE difference is refused in both. It is not a small slope — it is the probe
/// disagreeing with itself, and no verdict can be drawn from it.
fn zero_slope_verdict(name: &str, axis: &str, l: Ladder) -> Result<(), String> {
    let growth = match l.growth() {
        Ok(g) => g,
        // A bound, not a measurement — but a bound in the direction this gate needs.
        // The only unresolvable state left is a NEGATIVE difference; a floor reading is a
        // sound (if coarse) upper bound and flows through `growth()` as a number.
        Err(why @ Unresolved::NegativeDifference { .. }) => {
            return Err(format!(
                "ZERO-SLOPE GATE CANNOT DECIDE for `{name}` on the {axis} axis: {why}.\n\
                 This is an INSTRUMENT fault, not a subject fault, and it must not be read as \
                 a pass: the old `saturating_sub` reported exactly this case as zero growth. \
                 Re-run; if it persists, the probe is contended or the fixture is \
                 nondeterministic."
            ))
        }
    };
    if growth <= ZERO_SLOPE_TOLERANCE {
        return Ok(());
    }
    Err(format!(
        "ZERO-SLOPE GATE FAILED for `{}` on the {} axis: minimum stack grew {} KiB \
         between {} = {} ({}) and {} = {} ({}), which is {} B per step.\n\
         A converted traversal's native stack must not depend on {}. See\n\
         docs/design/audits/theta-depth-traversals-2026-07-26.md.",
        name,
        axis,
        growth / 1024,
        axis,
        l.lo_param,
        l.lo_stack,
        axis,
        l.hi_param,
        l.hi_stack,
        growth / (l.hi_param - l.lo_param),
        axis
    ))
}

/// **The tripwire VERDICT.** A pure function of one [`Ladder`] and a ceiling.
///
/// ⚠★★★ **A FLOOR READING IS A FAILURE HERE, and this is the opposite of
/// [`zero_slope_verdict`]'s disposition.** The asymmetry is the whole repair.
///
/// A subject is in `TRIPWIRE_DEPTH` precisely *because it is known to be sloped*. The gate's
/// job is to notice when its slope gets worse. So a reading of `0 B/level` from this subject
/// is not the good news it looks like — it is the instrument failing to see a slope that is
/// known to be there. The old code computed `per_step` from a `saturating_sub` of two
/// `usize`s and compared it to the ceiling, so **all three** paths on [`Unresolved`] produced
/// `0`, cleared any ceiling, and passed:
///
/// * both ends at the floor — the subject fits under the probe window at both rungs;
/// * one end at the floor — a measurement minus an unknown;
/// * a negative difference — the deep end needing less stack than the shallow end.
///
/// ⇒ **This gate could not have gone red on the regression it exists to catch**, for any
/// subject whose ladder landed in one of those states. Measured at the time of the repair:
/// none of the six live `assert_slope_below` sites is currently floor-bound, so this is a
/// live weakness that had not yet fired — a distinction worth keeping, because "it never
/// failed" is not evidence that it could have.
///
/// ★ The remedy is to REFUSE rather than to widen the ceiling. An unresolvable ladder means
/// *re-measure on a discriminating window* — the ladder's ends must straddle the instrument
/// floor — not *accept the subject*.
fn slope_below_verdict(
    name: &str,
    ceiling_bytes_per_level: usize,
    l: Ladder,
) -> Result<(), String> {
    let per_step = match l.per_step() {
        Ok(p) => p,
        Err(why) => {
            return Err(format!(
                "Θ(DEPTH) TRIPWIRE CANNOT DECIDE for `{name}`: {why}.\n\
                 \n\
                 `{name}` is in the tripwire set BECAUSE it is known to be sloped, so an \
                 unreadable ladder is an INSTRUMENT failure and must not be scored as a pass. \
                 Before this clause existed the same ladder produced 0 B/level, cleared the \
                 {ceiling_bytes_per_level} B/level ceiling, and passed while measuring nothing.\n\
                 \n\
                 Observed: {} @ {} = {}, {} @ {} = {}.\n\
                 \n\
                 ⚠ The remedy is a DISCRIMINATING WINDOW — move the ladder's ends so both clear \
                 the {} KiB instrument floor — NOT a wider ceiling. Raising the ceiling on an \
                 unreadable ladder buys a green run and no information.",
                l.lo_stack,
                "depth",
                l.lo_param,
                l.hi_stack,
                "depth",
                l.hi_param,
                SMALLEST_POSEABLE_STACK / 1024,
            ))
        }
    };
    if per_step <= ceiling_bytes_per_level {
        return Ok(());
    }
    Err(format!(
        "Θ(DEPTH) TRIPWIRE for `{}`: {} B/level exceeds the {} B/level ceiling \
         ({} @ depth {} -> {} @ depth {}).\n\
         Either a traversal regressed, or codegen changed materially. See\n\
         docs/design/audits/theta-depth-traversals-2026-07-26.md.",
        name, per_step, ceiling_bytes_per_level, l.lo_stack, l.lo_param, l.hi_stack, l.hi_param
    ))
}

/// The zero-slope half: minimum stack must not grow across a ~1,000× parameter
/// range. Profile-independent — it compares a traversal against ITSELF.
fn assert_no_slope(name: &str, lo_param: usize, hi_param: usize, axis: &str) {
    let l = measure_ladder(name, lo_param, hi_param);
    if let Err(why) = zero_slope_verdict(name, axis, l) {
        panic!("{why}");
    }
    // ★ The readings print through `MinStack`'s `Display`, so a floor reading renders as
    // `<12 KiB (BELOW THE INSTRUMENT FLOOR)` rather than as the number 12.
    println!(
        "  {name} ({axis}): O(1) — {} at {lo_param}, {} at {hi_param}",
        l.lo_stack, l.hi_stack
    );
}

/// **Tripwire for traversals not yet converted.** Bisects the minimum stack at
/// two depths, derives bytes-per-level, and fails if it exceeds `ceiling`.
///
/// This deliberately does NOT claim the traversal is fixed. It claims only that
/// it has not got worse. Every caller documents why its subject is still here.
fn assert_slope_below(name: &str, ceiling_bytes_per_level: usize, lo: usize, hi_depth: usize) {
    // Record the drive, so `TRIPWIRE_DEPTH` is checked against what actually ran
    // rather than transcribed alongside it. See the subject-register note.
    SLOPE_SUBJECTS_DRIVEN.with(|s| s.borrow_mut().push(name.to_string()));
    // Two depths far enough apart that (a) the fixed intercept is a small share
    // of the difference and (b) the CHEAP traversals clear the minimum viable
    // thread stack at the deeper point — otherwise both probes bottom out on the
    // same floor and the derived slope is a meaningless 0.
    let l = measure_ladder(name, lo, hi_depth);
    if let Err(why) = slope_below_verdict(name, ceiling_bytes_per_level, l) {
        panic!("{why}");
    }
    // ★ An unresolved ladder prints WHY rather than a number — the printed line is the only
    // record a reader sees, and `0 B/level` from a tripwire subject reads as good news.
    match l.per_step() {
        Ok(per_step) => {
            println!("  {name}: {per_step} B/level (ceiling {ceiling_bytes_per_level})")
        }
        Err(why) => println!(
            "  {name}: UNRESOLVED — {why} (ceiling {ceiling_bytes_per_level}, not applied)"
        ),
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
    assert_carries(
        "the substitute_no_sort input's nesting",
        par_depth(&term),
        depth,
    );
    let s = substitute_instance();
    let env: Env<Par> = Env::new();
    let out = s
        .substitute_no_sort(term, 0, &env)
        .expect("stack_depth_gate: substitute_no_sort failed");
    assert_carries(
        "the substitute_no_sort OUTPUT's nesting",
        par_depth(&out),
        depth,
    );
    dismantle(out);
}

/// ★★ **THE METERED WRAPPER — the deploy path's binding constraint, and a
/// NAMED RESIDUAL rather than a converted traversal.**
///
/// `Substitute::substitute_and_charge` is what the reducer actually calls; the
/// bare `substitute` this file already gates is reached only through it.
///
/// ## What it measured before 2026-07-28, and what it measures now
///
/// The wrapper took `term: &A` and opened with `term.clone()`, so **every
/// substitution deep-copied its input** — on the ordinary send path, with no
/// binder and no COMM, which is what distinguished it from
/// [`substitute_deep_binding_body`]'s `Env::get` clone. Measured end to end
/// through the real runtime on a 2 MiB tokio worker
/// (`rholang/tests/deploy_depth_ceiling.rs`), that copy was **7,253 B/level** and
/// it was the reason a deploy stopped at depth ~286 even though `substitute`,
/// `sort`, `normalize` and `pretty` are all converted and flat. Measured HERE,
/// pre-repair: **2,852 B/level release / 15,872 debug**, which is the standalone
/// `clone` subject's constant to the byte — the wrapper's entire slope was the
/// copy.
///
/// The wrapper then took its term **by value**, and this subject moved `t` into
/// it. The table records that intermediate state before the generated
/// `prost::Message` PDA replaced recursive `encoded_len`:
///
/// | profile | before          | after         | factor |
/// |---------|-----------------|---------------|-------:|
/// | release |  2,852 B/level  |   146 B/level | 19.5×  |
/// | debug   | 15,872 B/level  | 1,462 B/level | 10.9×  |
///
/// End to end at that intermediate commit, on the 2 MiB production worker,
/// `plain_deploy` went from a maximum nesting depth of **286 to 6,831** —
/// 23.9×.
///
/// At that same intermediate commit, `env_get_deploy` did not move: **283
/// before, 283 after**. Its semantic copy remained, so the later repair had to
/// make `<Par as Clone>::clone` itself stack-safe rather than delete the copy.
///
/// The subject stays after conversion so the defect cannot return unnoticed.
/// It is now in [`CONVERTED_DEPTH`], not [`TRIPWIRE_DEPTH`]: the generated
/// `prost::Message::encoded_len` implementation delegates to the memoized
/// bottom-up protobuf PDA, and the generated `Clone` implementation breaks the
/// recursive schema cycle at its feedback vertex. The live 4 → 4,096 ladder is
/// flat; the 146 / 1,462 B-per-level figures above are retained only as the
/// measured intermediate state.
///
/// ## ★★ Why this body is written EXACTLY like this
///
/// It is `stack_depth_probe.rs`'s `subst_and_charge` probe, and the two files
/// are kept byte-for-byte in step: one subject with one name must not report two
/// numbers.
///
/// * **The term is passed the way production passes it** — by value, from a
///   local the caller owns. That is the repair; writing it any other way
///   measures a different program. Before the repair this read `&t`, and the
///   `&` on a fresh local WAS the defect.
/// * **`std::mem::forget` on the result, never `drop` and never `dismantle`.**
///   `Par`'s derived `drop_in_place` is itself Θ(depth) (it is the `par_drop`
///   subject, 144 B/level release / 470 debug), so forgetting keeps teardown out
///   of the reading STRUCTURALLY — the number is the wrapper and nothing else.
///   Pre-repair this subject held TWO deep `Par`s at the end, the input and the
///   result, and forgot both; the input is now moved into the call, so there is
///   one.
///
/// ## ⚠ The shape was checked, and it does NOT move the number
///
/// The brief for this port warned that a semantically equivalent rewrite can
/// shift a reading by 60%, so before this subject was committed all four
/// plausible spellings of the PRE-REPAIR body were measured against each other —
/// `mem::forget` on both values, `dismantle` on both, `drop` on both, and the
/// fresh-temporary `s.substitute_and_charge(&nested_list(depth), 0, &env)
/// .map(drop)`. Bisected 2026-07-28, at depths 16 and 128:
///
/// | spelling                | release            | debug                  |
/// |-------------------------|--------------------|------------------------|
/// | `mem::forget`           | 57,344 → 376,832 B | 278,528 → 2,056,192 B  |
/// | `dismantle`             | 57,344 → 376,832 B | 278,528 → 2,056,192 B  |
/// | `drop`                  | 57,344 → 376,832 B | 278,528 → 2,056,192 B  |
/// | temporary + `map(drop)` | 57,344 → 376,832 B | 278,528 → 2,056,192 B  |
///
/// Identical to the byte, in both profiles: 2,852 B/level release, 15,872 debug.
/// That is not luck, and the reason is the one this file already records for
/// `normalize_drop`: the teardowns run AFTER the wrapper returns, so they do not
/// nest inside its frames and the composition costs
/// $`\max(S_{\text{wrapper}}, S_{\text{teardown}})`$ rather than the sum. At
/// 2,852 against 144 the max cannot move. A spelling could only matter here by
/// making a destructor run *while* the wrapper is still on the stack, and none
/// of the four does.
///
/// So the shape is chosen because it is structurally the right measurement and
/// because it agrees with the probe harness — not because the alternatives were
/// assumed to differ. They were measured, and they do not.
///
/// The anti-vacuity check is on the INPUT's depth: it is what the wrapper is
/// handed, and — pre-repair — what it copied. The output is not walked: doing so
/// would be another Θ(depth) traversal in the same frame region, for no evidence
/// the input check does not already give.
fn subst_and_charge_body(depth: usize) {
    let t = nested_list(depth);
    assert_carries(
        "the substitute_and_charge input's nesting",
        par_depth(&t),
        depth,
    );
    let s = substitute_instance();
    let env: Env<Par> = Env::new();
    let out = s
        .substitute_and_charge(t, 0, &env)
        .expect("stack_depth_gate: substitute_and_charge failed");
    std::mem::forget(out);
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
    let mut base: Env<Par> = Env::new();
    let env = base.put(nested_list(depth));
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
    assert_carries(
        "the substitute_binders OUTPUT's binder nesting",
        binder_depth(&out),
        depth,
    );
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
    let mut base: Env<Par> = Env::new();
    let env = base.put(nested_list(depth));
    let term = models::par_from_default! {
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
    assert_carries(
        "the substitute_wide input's sibling count",
        par_width(&term),
        width,
    );
    let s = substitute_instance();
    let env: Env<Par> = Env::new();
    let out = s
        .substitute(term, 0, &env)
        .expect("stack_depth_gate: substitute_wide failed");
    assert_carries(
        "the substitute_wide OUTPUT's sibling count",
        par_width(&out),
        width,
    );
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
    assert_carries(
        "the nested-ESet OUTPUT's nesting",
        eset_depth(&out.term),
        depth,
    );
    dismantle(out.term);
    dismantle(term);
}

fn sort_nested_map_body(depth: usize) {
    let term = nested_maps(depth);
    assert_carries("the nested-EMap input's nesting", emap_depth(&term), depth);
    let out = ParSortMatcher::sort_match(&term);
    assert_carries(
        "the nested-EMap OUTPUT's nesting",
        emap_depth(&out.term),
        depth,
    );
    dismantle(out.term);
    dismantle(term);
}

/// `<Par as Clone>::clone` over the nested-`ESet` shape — a third `Vec`-nested
/// ladder for the stage F-4 driver, alongside `clone` and `clone_send_chain`.
///
/// It is independent of the sorter subjects: all three are now converted and
/// independently admitted by the full-depth gate.
fn clone_nested_set_body(depth: usize) {
    let term = nested_sets(depth);
    assert_carries(
        "the clone_nested_set input's nesting",
        eset_depth(&term),
        depth,
    );
    let c = term.clone();
    assert_carries("the CLONE's nesting", eset_depth(&c), depth);
    dismantle(c);
    dismantle(term);
}

fn sort_wide_body(width: usize) {
    // TWO wide lists, so the sorter has siblings to order and its comparator
    // has `width` score-tree children to walk.
    let term = models::par_from_default! {
        exprs: vec![wide_list_expr(width), wide_list_expr(width)],
        ..Default::default()
    };
    assert_carries(
        "the sort_wide input's sibling count",
        par_width(&term),
        width,
    );
    let out = ParSortMatcher::sort_match(&term);
    assert_carries(
        "the sort_wide OUTPUT's sibling count",
        par_width(&out.term),
        width,
    );
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
        assert_carries(
            "a compared score tree's nesting",
            tree_depth(&s.score),
            depth,
        );
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
        assert_carries(
            "a compared score tree's sibling count",
            tree_width(&s.score),
            width,
        );
    }
    ScoredTerm::sort_vec(&mut scored);
    // Stable sort + `Equal` comparison must preserve the input order. That is
    // the property `sig.rs` depends on, asserted at every width the gate probes.
    assert_eq!(
        scored[0].term, 0,
        "a stable sort reordered two equal scores"
    );
}

fn tree_drop_body(depth: usize) {
    // `Tree<ScoreAtom>` is a recursive RUST type, not a proto message, so the
    // audit's Tarjan-over-the-proto enumeration could not see it.
    let score = deep_score(depth, 0);
    assert_carries(
        "the dropped score tree's nesting",
        tree_depth(&score),
        depth,
    );
    drop(score);
}

fn tree_clone_body(depth: usize) {
    let score = deep_score(depth, 0);
    assert_carries("the cloned score tree's nesting", tree_depth(&score), depth);
    let c = score.clone();
    assert_carries("the CLONE's nesting", tree_depth(&c), depth);
    assert!(
        c == score,
        "the iterative Clone did not reproduce its input"
    );
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
    assert_carries(
        "the pretty_wide input's sibling count",
        par_width(&term),
        width,
    );
    let mut pp = PrettyPrinter::new();
    let s = pp.build_string_from_message(&term);
    // `[a, b, c]` — one comma fewer than there are elements.
    assert_carries(
        "the PRINTED sibling count",
        s.matches(", ").count() + 1,
        width,
    );
    dismantle(term);
}

/// Rholang SOURCE text `[[[…[0]…]]]` with `depth` bracket levels, built
/// ITERATIVELY so the fixture never measures itself.
fn nested_list_source(depth: usize) -> String {
    let mut s = String::with_capacity(2 * depth + 1);
    for _ in 0..depth {
        s.push('[');
    }
    s.push('0');
    for _ in 0..depth {
        s.push(']');
    }
    s
}

/// Rholang SOURCE text `[0, 1, …, n-1]` with `width` siblings, built
/// iteratively.
fn wide_list_source(width: usize) -> String {
    let mut s = String::with_capacity(8 * width + 2);
    s.push('[');
    for i in 0..width {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&i.to_string());
    }
    s.push(']');
    s
}

/// Count the leading `[` run of a source string — the SOURCE nesting the
/// normalizer's recursion actually follows. Iterative by construction.
fn source_bracket_depth(src: &str) -> usize { src.bytes().take_while(|b| *b == b'[').count() }

/// Count a flat source list's elements — `[a, b, c]` has one comma fewer than
/// it has elements. Iterative by construction.
fn source_sibling_count(src: &str) -> usize { src.matches(", ").count() + 1 }

/// ★★ **THE DEPLOY PATH, WITH THE TEARDOWN THE DEPLOY PATH ACTUALLY HAS.**
///
/// [`normalize_body`] hands its result to `par_children::dismantle`. That is
/// correct *for a probe* — it isolates the normalizer — but it is not what any
/// caller of `Compiler::source_to_adt` does, and it is why the gate's own
/// headline regression test
/// [`the_577_byte_reproducer_is_a_deploy_and_not_a_node_abort`] certifies depth
/// **100,000** on a 2 MiB worker while a deploy of depth ~4,400 aborts the
/// process. The difference between those two numbers is one function call in a
/// test fixture.
///
/// This subject is [`normalize_body`] with `dismantle` replaced by `drop` —
/// i.e. the composition `InterpreterImpl::inj_attempt` performs:
///
/// ```text
///   Compiler::source_to_adt(source)   Θ(1) native stack   (Stage G, converted)
///        │  returns a `Par` of the source's nesting depth
///        ▼
///   … the Par falls out of scope …    Θ(depth)            (derived `Drop`)
/// ```
///
/// ## ⚠ IT RAN ON INGRESS TOO, WHERE THERE IS NO BUDGET AT ALL — ★ NOW CONVERTED
///
/// This doc described only the EVALUATION path until 2026-07-27, which
/// understated the exposure. The same composition — build a full `Par` from
/// attacker-supplied source, then release it recursively — was what
/// `casper/src/rust/engine/multi_parent_casper/block_admission.rs` ran on the
/// **gRPC deploy-intake** path, in both `admit_deploy` and
/// `admit_deploy_cosigned`:
///
/// ```text
///   mk_term(&deploy.data.term, normalizer_env)      = Compiler::source_to_adt_with_normalizer_env
///        │  Ok(_parsed_term)  ← BOUND AND DISCARDED; the match is a validity check
///        ▼
///   … the arm ends, the Par falls out of scope …    Θ(depth) (derived `Drop`)
/// ```
///
/// and that was **worse** than the evaluation instance in three ways:
///
/// * it fired on a **2 MiB spawned worker** BEFORE the deploy was stored, BEFORE
///   consensus, and before any replica had agreed to spend anything on it;
/// * `admit_deploy_cosigned` never touches a `RuntimeBudget`, so — exactly as
///   with `build-normalized-term` — cost accounting could not bound it, not
///   because the charge would be too small but because no charge exists yet; and
/// * the term was **discarded**. `_parsed_term` was bound with a leading
///   underscore and never read — the `match` is a validity check, what admission
///   stores is the deploy's SOURCE, and the block creator re-normalizes it later.
///   The whole traversal was the destructor of a value nothing reads.
///
/// `normalizer_env` is depth-independent (a handful of shallow `GUnforgeable`
/// `Par`s), so ingress and this subject measure the same composition — and the
/// measurements agree on the class. Bisected on a 2 MiB thread in release,
/// `casper/tests/deploy_ingress_depth_ceiling.rs` put ingress at a maximum source
/// depth of **21,781**, against this subject's independently bisected **144
/// B/level** and `par_drop`'s **14,525**.
///
/// ⚠ The two files' *slopes* differ — 96 B/level there against 144 here — and
/// that is CODEGEN, not composition. The identical function
/// `drop_in_place::<models::rhoapi::Par>` is emitted with 5 pushes and no
/// `sub rsp` (48 B) in that binary and 7 pushes (64 B) in this one; the
/// mutually-recursive cycle it forms with `drop_in_place::<ExprInstance>` is
/// therefore 96 B per level there and 144 here. Both are Θ(depth); only the
/// constant is a property of the build.
///
/// ## ★ THE INGRESS INSTANCE IS NOW CONVERTED — this subject is the one that is not
///
/// `casper::rust::util::rholang::interpreter_util::validate_deploy_term` owns the
/// deploy-admission discard as of 2026-07-28: it parses and hands the term to
/// `par_children::dismantle`, and both `admit_deploy` and `admit_deploy_cosigned`
/// call it. Measured on the same 2 MiB worker: **0.0 B/level** over
/// `256 → 4,096` in both profiles, and no ceiling below a search cap of 262,144,
/// against the derived control's 21,782 (release) / 4,503 (debug) on the identical
/// fixture. The claim is executed by
/// `casper/tests/deploy_ingress_depth_ceiling.rs`'s
/// `ingress_validation_is_depth_independent`.
///
/// ⚠ **That does NOT promote this subject, and the distinction is the whole
/// point of [`TRIPWIRE_DEPTH`]'s admission rule.** What was converted is the
/// *deploy-admission instance* of the composition. This subject stands for the
/// **evaluation** instance — `Compiler::source_to_adt` handing a term to
/// `InterpreterImpl::inj_attempt`, which still releases it through the derived
/// destructor when reduction finishes — and that traversal is unchanged, still
/// reachable, and still Θ(depth). A name leaves this list when the traversal it
/// stands for is gone, never because a sibling call site was fixed.
///
/// ⚠ **The two facts are individually gated and jointly unguarded, and that is
/// the whole finding.** `normalize` is in
/// [`converted_traversals_are_depth_independent`] at 0 B/level, so the gate
/// certifies that a term of any depth can be BUILT from a deploy. `par_drop` is
/// in [`theta_depth_tripwire`] at ~470 B/level, so the gate certifies that
/// tearing one down costs native stack per level. Nothing asserted the join —
/// and the join is a `SIGSEGV`, which is neither a panic nor an `Err`, so
/// `inj_attempt`'s `Err(e) => handle_error(ParserError(..))` arm cannot see it
/// and no `catch_unwind` can contain it. It takes the node, not the deploy.
///
/// ★ Its measured slope must track [`par_drop_body`]'s, because the normalizer
/// contributes 0: if this subject ever reads materially BELOW `par_drop`, the
/// fixture has collapsed (a shallow term drops cheaply) and the reading is
/// vacuous — which is what `assert_carries` on the returned term prevents.
///
/// Its bidirectional control is `normalize` itself: same source, same
/// normalizer, same term, teardown by worklist instead of by recursion, 0
/// B/level. The ONLY difference between the two ladders is the destructor.
fn normalize_drop_body(depth: usize) {
    let src = nested_list_source(depth);
    assert_carries(
        "the normalize_drop input's SOURCE nesting",
        source_bracket_depth(&src),
        depth,
    );
    let term = Compiler::source_to_adt(&src).expect("stack_depth_gate: normalize_drop failed");
    assert_carries("the NORMALIZED term's nesting", par_depth(&term), depth);
    // ⚠ NOT `dismantle`. This is the line under test: the EVALUATION path's
    // caller of `Compiler::source_to_adt` releases the term exactly like this.
    //
    // ★ "every production caller" until 2026-07-28, when deploy admission stopped
    // being one — see this function's docs. `inj_attempt` still does.
    drop(term);
}

/// The NORMALIZER — Θ(*source* nesting), and the only member of the family that
/// runs before a term exists at all, therefore before metering
/// (`inj_attempt`'s `build-normalized-term` precedes `set-initial-cost`).
///
/// ★ Both ends carry the parameter, per Rule V: the INPUT is checked for its
/// bracket run and the OUTPUT for its `EList` nesting. A normalizer that
/// silently produced a shallow term — or one that rejected the input and was
/// never entered — would read a comfortable zero otherwise.
///
/// ⚠ It ends in `dismantle`, which no production caller does. See
/// [`normalize_drop_body`] for the same composition with the deploy path's own
/// teardown, and for why the difference is a node abort rather than a detail.
fn normalize_body(depth: usize) {
    let src = nested_list_source(depth);
    assert_carries(
        "the normalize input's SOURCE nesting",
        source_bracket_depth(&src),
        depth,
    );
    let term = Compiler::source_to_adt(&src).expect("stack_depth_gate: normalize failed");
    assert_carries("the NORMALIZED term's nesting", par_depth(&term), depth);
    dismantle(term);
}

/// The normalizer's WIDTH axis: `fold_match` iterates a collection's elements,
/// and before the conversion each element was a recursive call. Gated in DEBUG
/// as well as release, for the same reason `free_check` is: `-O2` may turn a
/// tail-position loop into a loop by itself, and a release-only gate would
/// certify the optimiser's discretion rather than the code.
fn normalize_wide_body(width: usize) {
    let src = wide_list_source(width);
    assert_carries(
        "the normalize_wide input's SOURCE sibling count",
        source_sibling_count(&src),
        width,
    );
    let term = Compiler::source_to_adt(&src).expect("stack_depth_gate: normalize_wide failed");
    assert_carries(
        "the NORMALIZED term's sibling count",
        par_width(&term),
        width,
    );
    dismantle(term);
}

/// ★★ **`inj_attempt`'s `set-initial-cost` PHASE, verbatim — the write-then-
/// read-back that used to cost TWO deep traversals where a move suffices.**
///
/// [`normalize_drop_body`] measures the composition
/// `source_to_adt` ▸ `Drop`. This subject measures what actually sits between
/// them in `InterpreterImpl::inj_attempt`: the metering handshake, which takes
/// the freshly normalized `Par` by value and then has to give it back.
///
/// **What it used to be** (`interpreter.rs`, until 2026-07-27):
///
/// ```text
///   Compiler::source_to_adt(source)             Θ(1) native stack (Stage G)
///        │  a `Par` of the source's nesting depth
///        ▼
///   SignedProcess::metered(parsed, sig, phlo)   Θ(1)  — moves the Par IN
///        ▼
///   budget.reset_from_signed_process(&sp)       Θ(1)  — reads ONLY `.token()`
///        ▼
///   sp.source_process().cloned()                Θ(depth) ★ <Par as Clone>::clone
///        ▼
///   … `sp` falls out of scope …                 Θ(depth) ★ drop_in_place::<Par>
/// ```
///
/// **Two deep traversals, not one**, eleven lines apart, on a term whose nesting
/// depth is chosen by whoever wrote the deploy — and neither bought anything.
/// `reset_from_signed_process` reads only `SignedProcess::token()`, and `token()`
/// returns `None` on the `Signed` arm, so the metering handshake never observes
/// `process` at all. The clone was a read-back of what the line above had
/// written.
///
/// **What it is now:** `SignedProcess::into_source_process` — the by-move twin of
/// `source_process`, in the pattern `par_children::take_par_child_pars` /
/// `par_child_pars` establishes and `move_and_borrow_source_process_agree`
/// enforces. The term is handed over instead of copied, so the clone is gone and
/// there is no original left to drop.
///
/// ★ **Anti-vacuity is the whole subject here.** `par_depth` is asserted on the
/// term going IN and on the term coming OUT. `into_source_process` returning the
/// wrong arm — or a `SignedProcess::metered` that stopped retaining the process —
/// yields a shallow or absent `Par`, and the reading would be a comfortable zero
/// for a handshake that was never given any depth. That check is what makes this
/// ladder mean anything, because unlike every other subject in this file the
/// traversal under test is one the code no longer performs: what is measured is
/// that it is *absent*.
///
/// ## Measured, RELEASE, by direct bisection of this subject (2026-07-27)
///
/// The BEFORE column was produced by running this exact body with
/// `into_source_process()` replaced by `source_process().cloned()` + an explicit
/// `drop(signed_process)` — i.e. the composition `interpreter.rs` performed until
/// this date — and nothing else changed:
///
/// | | B / nesting level | min stack @ 16 | min stack @ 128 | max depth on a 2 MiB worker |
/// |---|---:|---:|---:|---:|
/// | before (`.cloned()` + `drop`) | **2,852** | 57,344 B | 376,832 B | **729** |
/// | after (`into_source_process`) | **0** | 32,768 B @ 4 | 32,768 B @ 4,096 | **≥ 1,048,576** |
///
/// Depth 730 aborted with `thread 'gate' has overflowed its stack` /
/// `fatal runtime error: stack overflow, aborting`, shell status **134**
/// (`128 + SIGABRT`) — not a panic, not an `Err`, and therefore not something
/// `inj_attempt`'s `Err(e) => handle_error(ParserError(..))` arm could ever see.
///
/// ⚠ 729, not 735. The figure that circulated before this bisection was
/// arithmetic — `2 MiB / 2,852 B` — which ignores the subject's own ~57 KiB
/// intercept and is therefore an UPPER bound. The bisected value agrees with the
/// standalone `clone` subject's bisected maximum depth (**729**) to the level,
/// which is the cross-check that the composition's ceiling really was
/// `<Par as Clone>::clone`'s and not something else in the phase.
///
/// ⚠⚠ **This was never the deploy path's binding constraint, and removing it did
/// not lift the deploy path's ceiling.** Measured end to end through the real
/// runtime on a 2 MiB tokio worker
/// (`rholang/tests/deploy_depth_ceiling.rs`), a deploy still stopped at depth
/// **286** after this conversion — because `Substitute::substitute_and_charge`
/// took its term by reference and opened with `term.clone()`, a second
/// `<Par as Clone>::clone` that fires on the ORDINARY SEND path, needs no binder
/// and no COMM, and measured **7,253 B/level**.
///
/// ★ That wrapper now takes its term by value (2026-07-28), and `plain_deploy`'s
/// end-to-end ceiling moved 286 → **6,831**. `env_get_deploy` did not move: it
/// is bounded by `Env::get`, not by either of these two clones. See
/// `subst_and_charge_body` and that file's own module documentation.
fn inj_attempt_clone_body(depth: usize) {
    let src = nested_list_source(depth);
    assert_carries(
        "the inj_attempt_clone input's SOURCE nesting",
        source_bracket_depth(&src),
        depth,
    );
    let parsed = Compiler::source_to_adt(&src)
        .expect("stack_depth_gate: inj_attempt_clone normalize failed");
    assert_carries("the NORMALIZED term's nesting", par_depth(&parsed), depth);

    // ── `inj_attempt`'s `set-initial-cost` block, line for line. ──
    let budget = RuntimeBudget::new(Cost::create(
        INJ_ATTEMPT_GATE_PHLO,
        "stack_depth_gate: inj_attempt_clone",
    ));
    let signed_process = SignedProcess::metered(
        parsed,
        budget.signature(),
        u64::try_from(INJ_ATTEMPT_GATE_PHLO).unwrap_or(0),
    );
    budget.reset_from_signed_process(&signed_process);
    let out = signed_process
        .into_source_process()
        .expect("metered deploy must retain source process");

    // ⚠ THE STRONGEST CHECK IN THIS SUBJECT — see the note above.
    assert_carries("the term handed to `reducer.inj`", par_depth(&out), depth);
    dismantle(out);
}

/// The budget this gate's `inj_attempt` subject is initialised with. Only its
/// magnitude matters: `reset_from_signed_process` stores a token count and never
/// walks the term.
const INJ_ATTEMPT_GATE_PHLO: i64 = 1_000_000;

/// `<Par as Clone>::clone` — GENERATED over `drive::drive_with` since stage F-4.
///
/// ★ The ladder `nested_list` builds already nests **through two `Vec` fields**:
/// `Par.exprs: Vec<Expr>` and `EList.ps: Vec<Par>`. That matters, because the
/// defect class this subject guards against is not "recursion" in general but
/// **collection-element delegation** — a `Vec<T>` field routed to a whole-value
/// `<Vec<T> as Clone>::clone`, which re-enters the element type's `Clone` and
/// recurses. Eight of the nine "iterative" drivers the sibling `mettail-rust`
/// generator emits are Θ(depth) for exactly that reason, at 254–10,592 B/level.
///
/// ⚠ It is a `Vec`-nested ladder, but it exercises only TWO of `Par`'s nine
/// recursive collections. [`clone_send_chain_body`] is the second ladder, through
/// a different one, for that reason.
fn clone_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the clone input's nesting", par_depth(&term), depth);
    let c = term.clone();
    assert_carries("the CLONE's nesting", par_depth(&c), depth);
    dismantle(c);
    dismantle(term);
}

/// ★★ The **`Vec`-nested** clone ladder, through `Par.sends[0].chan`.
///
/// ⚠ **Why a second clone ladder is not redundant.** The generated driver has one
/// `clone_push_children_*` / `clone_child_count_*` / `clone_rebuild_*` triple per
/// entered type, and a ladder that nests through one collection field proves
/// nothing about the other eight. `nested_list` descends
/// `Par.exprs → Expr → EList.ps → Par`; this one descends
/// `Par.sends → Send.chan → Par`, which is a **different** `Vec` field reached
/// through an `Option<Par>` rather than a `Vec<Par>`.
///
/// ★ It is also the ladder that would go RED if the emitter regressed to
/// `self.sends.clone()`: `<Vec<Send> as Clone>::clone` re-enters `Send::clone`,
/// which clones `chan: Option<Par>`, which re-enters the DRIVEN `Par::clone` —
/// one native frame per level, i.e. Θ(depth), with the `clone` subject above
/// still flat because it never touches `sends`. A single-collection ladder is
/// exactly how the sibling repo's defect hid.
fn clone_send_chain_body(depth: usize) {
    let term = nested_send_chain(depth);
    assert_carries(
        "the clone_send_chain input's nesting",
        send_chain_depth(&term),
        depth,
    );
    let c = term.clone();
    assert_carries("the CLONE's nesting", send_chain_depth(&c), depth);
    dismantle(c);
    dismantle(term);
}

/// ★ The **EXTERN obligation**, measured rather than asserted.
///
/// The clone driver treats `EPathMap` as a **bounded** field — one whole-value
/// `Clone` call — because an extern type contributes no descriptor-derived
/// children. That is correct only because `EPathMap::clone` is O(1) **at the
/// node**: `ps` is an `EntryTrie` whose clone is a refcount bump on the trie root
/// plus an `Arc` bump on the memoized projection, and the shadow cell is an
/// `OnceLock<Arc<_>>` clone (`models/src/rust/rhoapi_ext.rs`).
///
/// ⚠ If that ever stops being true — if `EPathMap::clone` starts deep-copying its
/// `Vec<Par>` — this ladder goes sloped while every other clone subject stays
/// flat, because `EPathMap` is the ONE recursive edge the driver does not own.
/// Nesting is `Par → Expr → EPathmapBody(EPathMap) → ps[0] → Par`.
///
/// ## ⚠★ Why this subject LEAKS instead of dismantling — measured, not assumed
///
/// Its first reading was **1,865 B/level** (debug, 16 → 128) while `clone` and
/// `clone_send_chain` were both flat, which looked exactly like the emitter
/// having routed one field to a whole-value clone. It was not. The control
/// [`pathmap_chain_drop_body`] builds the same ladder and releases it **with no
/// clone at all**, and it measures the SAME two numbers to the byte:
///
/// ```text
///   clone_pathmap_chain   69,632 -> 278,528   1,865 B/level
///   pathmap_chain_drop    69,632 -> 278,528   1,865 B/level   ◀── no clone
/// ```
///
/// So the clone contributes **zero** and the slope is the FIXTURE's teardown:
/// `par_children::dismantle` cannot tear down through an `EPathMap`. Its
/// `EPathmapBody` arm does `out.extend(x.ps().iter().cloned())` — it clones the
/// entries out of the trie and leaves the ORIGINAL trie to drop through the
/// derived recursive destructor. (`par_children.rs` already calls that exclusion
/// *"a real gap, not a formality"*, for the matcher; this ladder is the first
/// thing to measure it.)
///
/// A subject whose reading is `max(clone, drop)` reports the wrong traversal —
/// the exact trap that once made the score-tree subjects report the SORTER's
/// 78,573 B/level instead of the comparator's 1,329. So the term is **forgotten**
/// rather than dismantled, and the teardown is measured separately by
/// `pathmap_chain_drop`, which is in [`CLONE_LADDERS_CAPPED_BY_THEIR_FIXTURE`] —
/// **neither** register list, per its own doc at `:898-921`. ⛔ This line previously read
/// `TRIPWIRE_DEPTH`, and `:2515` previously read `CONVERTED_DEPTH`; the file asserted
/// three mutually exclusive memberships for one subject. Corrected 2026-07-30 against
/// the list itself rather than against either claim.
///
/// ⚠ Leaking is correct HERE and nowhere else: every subject runs in its own
/// child process, which exits immediately afterwards. `models/benches/bincode_encoder_bench.rs`
/// uses `std::mem::forget` for the same reason.
fn clone_pathmap_chain_body(depth: usize) {
    let term = nested_pathmap_chain(depth);
    assert_carries(
        "the clone_pathmap_chain input's nesting",
        pathmap_chain_depth(&term),
        depth,
    );
    let c = term.clone();
    assert_carries("the CLONE's nesting", pathmap_chain_depth(&c), depth);
    dismantle(c);
    dismantle(term);
}

/// ★★ **The DERIVED CONTROL for [`clone_body`]** — `term_ops::oracle_clone_par`,
/// which is the `#[derive(Clone)]` expansion `models/build.rs` stripped, re-emitted
/// by the same generator from the same resolved-field vector.
///
/// ## Why this exists rather than a manual revert
///
/// The "before" leg of a conversion's bisection evidence is normally taken by
/// reverting the change and re-measuring. For a converted TRAIT IMPL that is both
/// awkward and dangerous: the revert has to be un-reverted, and this workspace has
/// already frozen a deliberately-broken probe form into history once by
/// committing mid-measurement.
///
/// `models/codegen/schema.rs` §F therefore retains the derive's own body as a
/// free function, and `models/tests/clone_equivalence_corpus.rs` proves it
/// byte-identical to the driven form on eight axes over 67 enumerated shapes. So
/// the control and the subject live in the SAME binary, run on the SAME ladder, in
/// the SAME invocation — and the "before" figure stops being a number somebody
/// has to reproduce and becomes a number this file measures.
///
/// ⚠ It is deliberately in NEITHER [`CONVERTED_DEPTH`] nor [`TRIPWIRE_DEPTH`],
/// exactly as `bincode_ser_derived` is: it is a Θ(depth) control, not a claim.
/// Adding it to the tripwire list would require an [`assert_slope_below`] call
/// pinning a ceiling on code that exists only to be sloped.
fn clone_oracle_body(depth: usize) {
    use models::rust::rholang::term_ops::oracle_clone_par;
    let term = nested_list(depth);
    assert_carries("the clone_oracle input's nesting", par_depth(&term), depth);
    let c = oracle_clone_par(&term);
    assert_carries("the ORACLE CLONE's nesting", par_depth(&c), depth);
    dismantle(c);
    dismantle(term);
}

/// ★ The **TEARDOWN control** for [`clone_pathmap_chain_body`]: build the same
/// ladder and release it, with **no clone at all**.
///
/// ⚠ This exists because the first release measurement of `clone_pathmap_chain`
/// came out SLOPED at 1,865 B/level (debug) while `clone` and `clone_send_chain`
/// were both flat, and there are two candidate causes with opposite conclusions:
///
/// 1. the driven clone recurses through `EPathMap` — a defect in the emitter;
/// 2. the FIXTURE's teardown recurses through `EPathMap` — a defect in
///    `par_children::dismantle`, which the clone subject would then be reporting
///    as its own.
///
/// This subject decides it. If it is sloped, cause (2) holds and the clone is
/// innocent; the gate's own history contains the same trap — "a cloning fixture
/// would report `max(clone, eq)`", which once made the score-tree subjects report
/// the SORTER's 78,573 B/level instead of the comparator's 1,329.
///
/// ★ It stays in the file whatever the answer is, because a subject whose reading
/// is a maximum over two traversals needs the other one measured beside it.
///
/// ## ★★★ THE ANSWER: NEITHER. It was a THIRD cause, and the first "confirmation"
/// was a COMMON-CAUSE ARTIFACT.
///
/// The measurement sequence, recorded in full because the middle step is exactly
/// the mistake this note exists to stop somebody repeating:
///
/// | step | `clone_pathmap_chain` | `pathmap_chain_drop` | read as |
/// |---|---|---|---|
/// | 1. both dismantle their fixture | 69,632 → 278,528 (1,865) | 69,632 → 278,528 (1,865) | "identical ⇒ the TEARDOWN is the cause" |
/// | 2. clone subject leaks instead | 49,152 → 278,528 (2,048) | — | ⚠ still sloped, and the DEEP END DID NOT MOVE |
/// | 3. fixture built on a big stack | **32,768 → 32,768 (0)** | **69,632 → 69,632 (0)** | ★ the BUILDER was the cause |
///
/// Step 1's identical numbers were **not** evidence that the teardown was the
/// cause. Two bodies that share one sloped component read identically whatever
/// else they do — and both of these shared the FIXTURE BUILDER. The step-1
/// reading is a true statement ("the clone contributes zero") that supports a
/// false conclusion, and step 2's unchanged deep end is what gave it away.
///
/// **The actual cause**: `EPathMap::new` keys every entry by its canonical bytes,
/// so building an `EPathMap` around a depth-`d` `Par` *encodes* that `Par`, and
/// the prost encoder is audit row 7 — still Θ(depth), still in
/// [`TRIPWIRE_DEPTH`] as `encode`. The ladder was measuring the ENCODER through a
/// constructor.
///
/// ⛔ **CORRECTED 2026-07-30. The paragraph that stood here carried TWO falsehoods,
/// and the flat measurement it explained is true for a different reason that does
/// NOT generalise.** It read: *"`par_children::dismantle` handles the `EPathMap`
/// shape FLAT. Its `EPathmapBody` arm clones the entries out of the trie and pushes
/// them onto the worklist; the residual trie holds `Par`s that were moved out, so its
/// drop is O(1) per node. … which is why it is in `CONVERTED_DEPTH` rather than in the
/// tripwire list."*
///
/// **Falsehood 1 — nothing is moved out.** `par_children.rs:395` is
/// `out.extend(x.ps().iter().cloned())`, a **copy**. Read it beside its own siblings,
/// which is where it becomes unmistakable: `EListBody`, `ETupleBody`, `ESetBody` and
/// `EMapBody` all write `out.extend(x.ps)`, *consuming*; only `EPathmapBody` and
/// `EZipperBody` clone. The arm **owns** the `EPathMap`, so the originals are still in
/// it when the arm ends and they fall to the recursive derived destructor.
///
/// ⚠ Worse than one stray copy: `x.ps()` is `EntryTrie::view()`, a **memoised** `Vec`
/// built by `entries_in_trie_order`, which clones every value — and a freshly built map's
/// memo is always cold. So the teardown path *materialises a full second copy in order to
/// destroy the first*: up to **2N** deep clones, and **2N** `Par`s left to the recursive
/// destructor. (`EPathMap` in fact retains the entries **three** ways — `trie`, this
/// memo, and `intern`, whose `map` is a third clone of the same trie.)
///
/// **Falsehood 2 — the list membership.** `pathmap_chain_drop` is in **neither**
/// register: it is in [`CLONE_LADDERS_CAPPED_BY_THEIR_FIXTURE`], documented at `:898-921`
/// as *"deliberately in NEITHER register list"*. ⚠ `:2404` states the opposite of this
/// sentence (`TRIPWIRE_DEPTH`), so the file contradicted itself twice over.
///
/// ⇒ ★ **Why the ladder nonetheless reads flat, which is the part worth keeping.**
/// `EPathMap::clone` is O(1) — a refcount bump on the trie root plus an `Arc` bump on the
/// memo. `nested_pathmap_chain` nests a pathmap **at every level**, so the entry the arm
/// clones is a *shallow alias* sharing the level-below trie; when the original drops, that
/// refcount is still `≥ 1` and the recursion **stops one level down**. **The sharing breaks
/// the chain — the dismantling does not.**
///
/// ⇒ ⛔ **This fixture is precisely the one shape that cannot see the defect.** Any payload
/// that is not immediately re-entrant into another `EPathMap` — an `EList`, `Send` or `ESet`
/// chain — is deep-copied by `Par::clone` while the original drops at **full depth**.
/// `pathmap_entry_drop` (one `EPathMap`, one deep non-pathmap entry) is the discriminating
/// subject and is expected RED. ⇒ **A flat reading here must never again be read as
/// coverage.**
///
/// ⚠ `move_and_borrow_tables_agree` (`par_children.rs:1034-1051`) cannot catch this either:
/// it compares `locally_free[0]` **tag bytes**, so a clone passes identically to a move —
/// and the by-*reference* table (`:181`) reports the same memo projection, so **both tables
/// are wrong identically**. The guard meant to keep them in step is blind to the very class
/// that separates them.
///
/// ⇒ Tracked as stack-safety **§3d-0**, which must land **before** any integration of
/// `dismantle`: integrating it as-is would ship a false claim of coverage.
///
/// ⚠ The `EPathmapBody` exclusion `par_children.rs` calls *"a real gap, not a
/// formality"* is about **matching and substitution descent**, not about
/// teardown. Nothing here bears on it.
fn pathmap_chain_drop_body(depth: usize) {
    let term = nested_pathmap_chain(depth);
    assert_carries(
        "the pathmap_chain_drop input's nesting",
        pathmap_chain_depth(&term),
        depth,
    );
    // The ONE thing under test: releasing this shape through the iterative
    // teardown the clone subjects use.
    dismantle(term);
}

/// `Par { sends: [Send { chan: <inner> }] }`, nested `depth` times.
fn nested_send_chain(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for level in 0..depth {
        p = models::par_from_default! {
            sends: vec![models::rhoapi::Send {
                chan: Some(p),
                data: vec![],
                persistent: false,
                locally_free: vec![(level % 251) as u8],
                connective_used: false,
            }],
            ..Default::default()
        };
    }
    p
}

/// The measured nesting of a [`nested_send_chain`] term.
///
/// ⚠ Iterative, like [`par_depth`]: a recursive depth probe would consume the
/// very stack the subject is measuring and the reading would be `max(subject,
/// probe)`.
fn send_chain_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        match cur.sends.first().and_then(|s| s.chan.as_ref()) {
            Some(inner) => {
                n += 1;
                cur = inner;
            }
            None => return n,
        }
    }
}

/// `Par { exprs: [EPathmapBody({| key: <inner> |})] }`, nested `depth` times.
///
/// The recursive edge is a borrowed `PathMap<Par>` value. Constant-size keys
/// keep construction linear and exercise the map specialization directly;
/// using each inner term as a set key would repeatedly encode every suffix.
fn nested_pathmap_chain(depth: usize) -> Par { nested_pathmap_chain_leaf(depth, 0) }

/// [`nested_pathmap_chain`] with a selectable deepest map value.
fn nested_pathmap_chain_leaf(depth: usize, leaf: i64) -> Par {
    let mut p = new_gint_par(leaf, vec![], false);
    for level in 0..depth {
        let key = new_gint_par(level as i64, vec![], false);
        p = models::par_from_default! {
            exprs: vec![models::rhoapi::Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(
                    models::rust::rhoapi_ext::EPathMap::new_map(
                        [(key, p)],
                        vec![(level % 251) as u8],
                        false,
                        None,
                    ),
                )),
            }],
            ..Default::default()
        };
    }
    p
}

/// The measured nesting of a [`nested_pathmap_chain`] term.
fn pathmap_chain_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        let next = match cur.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EPathmapBody(pm)) if !pm.is_empty() => pm
                .entry_trie()
                .map_trie()
                .and_then(|map| map.iter().next().map(|(_, value)| value)),
            _ => None,
        };
        let Some(next) = next else {
            return n;
        };
        n += 1;
        cur = next;
    }
}

/// `drop_in_place::<Par>` — `prost`'s DERIVED recursive destructor.
///
/// ⚠ Named `drop` until 2026-07-27; see the rename note in [`subject`].
fn par_drop_body(depth: usize) {
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
// ★ THE FOUR-QUADRANT S0 SUBJECTS — the five traversals absent at S0
//
// ⚠ Their absence from `CONVERTED_DEPTH`, `TRIPWIRE_DEPTH` and every
// `assert_slope_below` call meant they were UNMEASURED, not FLAT. That
// distinction is the whole reason these exist: a hand-picked list of "the
// traversals that matter" had missed `Hash` entirely, and nobody had named
// `Ord`/`PartialOrd` at all.
//
// Each one differs from its neighbours in exactly the way its anti-vacuity
// assertion has to account for:
//
//   eq     short-circuits at the FIRST unequal field  → the twins must be EQUAL
//   ord    short-circuits at the first UNEQUAL field  → they must differ at the
//                                                        LEAF and nowhere else
//   hash   never short-circuits, but is invisible     → two leaves, two digests
//   debug  produces a string, so length is the guard
//   protobuf_de is a generated, unbounded protobuf-decoder PDA
// ---------------------------------------------------------------------------

/// `<Par as PartialEq>::eq` — the **hand-written** structural comparison
/// (`models/src/lib.rs`), not a derive.
///
/// ⚠ The twins are built ITERATIVELY, twice — never `a.clone()`. `<Par as
/// Clone>::clone` is itself Θ(depth) at 2,852 B/level release, so a cloning
/// fixture would report `max(clone, eq)` and attribute the clone's frame to the
/// comparator. That is the exact defect that once made the score-tree subjects
/// report the SORTER's 78,573 B/level instead of the comparator's 1,329.
fn eq_body(depth: usize) {
    let a = nested_list(depth);
    let b = nested_list(depth);
    assert_carries("the eq input's nesting", par_depth(&a), depth);
    assert_carries("the eq TWIN's nesting", par_depth(&b), depth);
    // ★ EQUAL, so the comparison cannot stop before the leaf. An `assert_ne`
    // here would be satisfied by one frame.
    assert!(
        a == b,
        "VACUOUS PROBE: two independently built depth-{depth} `nested_list` terms compared \
         UNEQUAL, so `eq` returned before descending. The reading would be one frame's."
    );
    dismantle(a);
    dismantle(b);
}

/// `<Par as Hash>::hash` — also **hand-written** (`models/src/lib.rs`), and the
/// surface a hand-picked driver list had missed entirely.
///
/// `Hash` cannot short-circuit, so the risk is not truncation but INVISIBILITY:
/// a digest is produced whatever the walk did. Two terms differing only at the
/// leaf must therefore hash differently, which is only possible if both walks
/// reached the leaf.
fn hash_body(depth: usize) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let zero = nested_list_leaf(depth, 0);
    let one = nested_list_leaf(depth, 1);
    assert_carries("the hash input's nesting", par_depth(&zero), depth);
    assert_carries("the hash TWIN's nesting", par_depth(&one), depth);

    let digest = |p: &Par| {
        let mut h = DefaultHasher::new();
        p.hash(&mut h);
        h.finish()
    };
    assert_ne!(
        digest(&zero),
        digest(&one),
        "VACUOUS PROBE: two depth-{depth} terms differing ONLY at the leaf hashed \
         identically, so the walk did not reach the leaf and the reading is not about \
         depth {depth}.",
    );
    dismantle(zero);
    dismantle(one);
}

/// `<Par as Hash>::hash` through the legacy `ESet.ps: Vec<Par>` edge.
///
/// This is the `hash_nested_set` control named by `d0279621`. Its original
/// purpose was to attribute a residual in `sort_nested_set`; that residual no
/// longer exists because Hash, Eq, and the sorter are all generated/converted.
/// The ladder remains valuable as a non-list collection edge that would catch
/// a generator which accidentally delegated `Vec<Par>::hash` recursively.
fn hash_nested_set_body(depth: usize) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let zero = nested_sets_leaf(depth, 0);
    let one = nested_sets_leaf(depth, 1);
    assert_carries(
        "the hash_nested_set input's nesting",
        eset_depth(&zero),
        depth,
    );
    assert_carries(
        "the hash_nested_set TWIN's nesting",
        eset_depth(&one),
        depth,
    );

    let digest = |p: &Par| {
        let mut h = DefaultHasher::new();
        p.hash(&mut h);
        h.finish()
    };
    assert_ne!(
        digest(&zero),
        digest(&one),
        "VACUOUS PROBE: two nested ESet terms differing only at the leaf hashed identically"
    );
    dismantle(zero);
    dismantle(one);
}

/// Hash one set-mode EPathMap containing a deep key.
///
/// Set mode is a prefix-compressed `PathMap<()>`: hashing walks its canonical
/// byte paths with PathMap zippers and never materializes a `Vec<Par>`.
fn hash_pathmap_set_body(depth: usize) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let make = |leaf| {
        let key = nested_list_leaf(depth, leaf);
        assert_carries("the PathMap<()> set key's nesting", par_depth(&key), depth);
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(
                    models::rust::rhoapi_ext::EPathMap::new(vec![key], vec![], false, None),
                )),
            }],
            ..Default::default()
        }
    };
    let zero = make(0);
    let one = make(1);
    let digest = |p: &Par| {
        let mut h = DefaultHasher::new();
        p.hash(&mut h);
        h.finish()
    };
    assert_ne!(
        digest(&zero),
        digest(&one),
        "VACUOUS PROBE: PathMap<()> keys differing only at their deep leaf hashed identically"
    );
    dismantle(zero);
    dismantle(one);
}

/// Hash nested map-mode EPathMaps whose deepest `PathMap<Par>` value differs.
///
/// Constant-size keys prevent fixture construction from repeatedly encoding
/// the suffix. The generated Hash PDA borrows values directly from PathMap's
/// zipper and schedules them on its own worklist.
fn hash_pathmap_map_body(depth: usize) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let zero = nested_pathmap_chain_leaf(depth, 0);
    let one = nested_pathmap_chain_leaf(depth, 1);
    assert_carries(
        "the PathMap<Par> input's nesting",
        pathmap_chain_depth(&zero),
        depth,
    );
    assert_carries(
        "the PathMap<Par> TWIN's nesting",
        pathmap_chain_depth(&one),
        depth,
    );
    let digest = |p: &Par| {
        let mut h = DefaultHasher::new();
        p.hash(&mut h);
        h.finish()
    };
    assert_ne!(
        digest(&zero),
        digest(&one),
        "VACUOUS PROBE: PathMap<Par> values differing only at their deep leaf hashed identically"
    );
    dismantle(zero);
    dismantle(one);
}

/// The generated `prost::Message::clear` PDA, independently of `Drop`.
fn message_clear_body(depth: usize) {
    use prost::Message;

    let mut term = nested_list(depth);
    assert_carries(
        "the Message::clear input's nesting",
        par_depth(&term),
        depth,
    );
    Message::clear(&mut term);
    assert_eq!(
        term,
        Par::default(),
        "Message::clear did not restore Par::default()"
    );
    assert!(term.encode_to_vec().is_empty());
}

/// `<Par as Ord>::cmp` — generated from the schema as an explicit-worklist
/// lexicographic comparator at the descriptor-derived feedback vertex set.
///
/// `cmp` returns at the first field that differs, so the twins differ
/// at the LEAF and only there — every level above must compare `Equal` for the
/// descent to continue. `Less` is therefore a verdict only a full descent can
/// reach.
fn ord_body(depth: usize) {
    let smaller = nested_list_leaf(depth, 0);
    let larger = nested_list_leaf(depth, 1);
    assert_carries("the ord input's nesting", par_depth(&smaller), depth);
    assert_carries("the ord TWIN's nesting", par_depth(&larger), depth);
    assert_eq!(
        smaller.cmp(&larger),
        std::cmp::Ordering::Less,
        "VACUOUS PROBE: the depth-{depth} twins did not compare `Less`. They differ only at \
         the leaf, so any other verdict means `cmp` decided before descending."
    );
    dismantle(smaller);
    dismantle(larger);
}

/// `<Par as Debug>::fmt` — now generated as a declaration-order explicit-
/// worklist formatter at the schema feedback vertex set. The declaration order
/// remains prost-derive compatible; the generated shallow differential checks
/// the output, while this subject measures the deep walk.
fn debug_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the debug input's nesting", par_depth(&term), depth);
    let rendered = format!("{:?}", term);
    // Every `EList` level contributes at least its `EListBody(EList { ps: [` —
    // far more than one byte — so a collapsed fixture cannot reach this bound.
    assert!(
        rendered.len() >= depth,
        "VACUOUS PROBE: `Debug` of a depth-{} term rendered only {} characters",
        depth,
        rendered.len()
    );
    dismantle(term);
}

/// `<Par as prost::Message>::merge` — the generated protobuf decoder PDA.
fn protobuf_de_body(depth: usize) {
    use prost::Message;
    let term = nested_list(depth);
    let bytes = term.encode_to_vec();
    dismantle(term);
    let decoded = Par::decode(&bytes[..]).expect("stack_depth_gate: protobuf_de failed");
    assert_carries(
        "the prost-DECODED term's nesting",
        par_depth(&decoded),
        depth,
    );
    dismantle(decoded);
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
fn printed_bracket_depth(s: &str) -> usize { s.chars().take_while(|c| *c == '[').count() }

/// ⚠ THE FAMILY'S BINDING MEMBER, AND UNTIL NOW IT HAD NO GATE COVERAGE AT ALL.
///
/// `models/build.rs` attaches `serde::Serialize`/`Deserialize` to every
/// `.rhoapi` message, and RSpace serialises datums and continuations with
/// **bincode 1.3.3** (`rspace++/src/rspace/serializers/serializers.rs`). Before
/// conversion, stock prost rejected this shape from depth 34 while the derived
/// bincode reader remained unbounded and recursively consumed native stack.
/// The production protobuf and bincode readers are now generated, unbounded
/// explicit-state PDAs; this paragraph records the defect that selected the
/// subject, not the current implementation.
///
/// In the pre-conversion implementation the encode side was ~9x (debug) to
/// ~39x (release) cheaper per level than the decode side, so a term could be
/// *written* on a stack that could not *read it back*. The paired generated
/// PDAs close that persistence hazard.
///
/// Measured 2026-07-26: encode 3,052 / 329 B/level (debug / release), decode
/// 28,362 / 12,894.
fn bincode_ser_body(depth: usize) {
    use models::rust::rholang::bincode_encoder::ColdStoreEncode;
    let term = nested_list(depth);
    assert_carries("the bincode_ser input's nesting", par_depth(&term), depth);
    let bytes = term.cold_encode();
    assert!(
        bytes.len() >= depth,
        "VACUOUS PROBE: encoding a depth-{} term produced only {} bytes",
        depth,
        bytes.len()
    );
    dismantle(term);
}

/// The PRE-CONVERSION control: the derived `Serialize`, still compiled.
///
/// Retained for exactly the reason `bincode_de_derived` is — so the
/// before/after comparison is available in ONE run instead of requiring a
/// checkout of an older commit — and because the derive is the ENCODE ORACLE
/// the differential is measured against (`models/tests/
/// bincode_encoder_differential.rs`). It is no longer what the node runs.
fn bincode_ser_derived_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the control's nesting", par_depth(&term), depth);
    let bytes = bincode::serialize(&term).expect("stack_depth_gate: the derived control failed");
    assert!(bytes.len() >= depth, "VACUOUS CONTROL");
    dismantle(term);
}

/// The DECODE side. See [`bincode_ser_body`] for why this member matters.
///
/// ★ CONVERTED. This subject now exercises `Par::cold_decode`
/// (`models/src/rust/rholang/bincode_decoder.rs`), the explicit-worklist decoder that
/// replaced the derived `Deserialize` on the cold-store read path. The derived
/// impl is retained as a `#[cfg(test)]` oracle and is differentially compared
/// against the machine over ~1.9M inputs, including every truncation of every
/// corpus encoding (`models/tests/bincode_decoder_malformed.rs`) — but it is no
/// longer what the node runs, so it is no longer what this gate measures.
///
/// ★ `bincode_ser` has since been converted too (Stage H,
/// `models/src/rust/rholang/bincode_encoder.rs`), so BOTH halves of the RSpace
/// codec are now depth-independent and the isolation below is belt-and-braces
/// rather than load-bearing. It is kept because the point of isolating the
/// decoder is to measure the DECODER: a probe that silently became
/// `max(encode, decode)` would still be wrong even when both are flat.
fn bincode_de_body(depth: usize) {
    use models::rust::rholang::bincode_encoder::ColdStoreEncode;
    let term = nested_list(depth);
    let bytes = term.cold_encode();
    dismantle(term);
    let decoded: Par = Par::cold_decode(&bytes).expect("stack_depth_gate: bincode_de failed");
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

/// Binding-path matcher subject. A concrete pattern would bypass the semantic
/// matcher through `match_pars`, so the free leaf is load-bearing.
fn spatial_binding_body(depth: usize) {
    let target = nested_list(depth);
    let pattern = nested_list_pattern(depth);
    assert_carries("the spatial target's nesting", par_depth(&target), depth);
    assert_carries("the spatial pattern's nesting", par_depth(&pattern), depth);
    let mut context = SpatialMatcherContext::new();
    let matched = context.spatial_match_result(target, pattern);
    assert!(matched.is_some(), "the binding matcher refused its witness");
    assert!(
        context.free_map.contains_key(&0),
        "the binding witness completed without binding level zero"
    );
    std::mem::forget(context);
}

/// Concrete fast-path subject. Alternating New/Receive/ReceiveBind is the
/// schema shape on which `match_pars` formerly called itself.
fn spatial_concrete_binders_body(depth: usize) {
    let target = nested_binders(depth);
    let pattern = nested_binders(depth);
    assert_carries(
        "the concrete spatial target's binder nesting",
        binder_depth(&target),
        depth,
    );
    assert_carries(
        "the concrete spatial pattern's binder nesting",
        binder_depth(&pattern),
        depth,
    );
    let mut context = SpatialMatcherContext::new();
    assert!(
        context.spatial_match_result(target, pattern).is_some(),
        "the concrete binder witness did not match"
    );
    std::mem::forget(context);
}

/// ⚠ `rho-pure-eval::eval_with`'s OWN Θ(depth) SCC — a separate crate, and one
/// the audit's enumeration reached only by measurement (§11.2).
///
/// The shape matters: a nested `EList` chain does NOT drive this recursion at
/// all, because the `EListBody` arm returns `par_with_expr(expr.clone())`
/// without descending. On that shape the probe measures `<Par as Clone>::clone`
/// and nothing else. `!(!(…(!true)…))` is the shape that actually recurses;
/// measured 21,584 B/level debug / 3,359 release before the conversion.
fn nested_nots(depth: usize) -> Par {
    let mut p = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(true)),
        }],
        ..Default::default()
    };
    for _ in 0..depth {
        p = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(models::rhoapi::ENot { p: Some(p) })),
            }],
            ..Default::default()
        };
    }
    p
}

fn eval_with_nots_body(depth: usize) {
    use rho_pure_eval::{eval_with, NoSpatialMatch};
    let p = nested_nots(depth);
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
// Phase 7 resource-measurement controls. These are addressable through
// `gate_child` but deliberately absent from the converted/tripwire registers.
// ---------------------------------------------------------------------------

/// Cachegrind's invariant arm: no term construction, traversal, or teardown.
fn phase7_invariant_body(parameter: usize) { std::hint::black_box(parameter); }

/// Fixture-only control shared by substitution and sorting.
fn heap_fixture_par_body(depth: usize) {
    let term = nested_list(depth);
    assert_carries("the heap Par fixture's nesting", par_depth(&term), depth);
    std::hint::black_box(&term);
    // One process per Massif cell: retaining the fixture makes its live bytes
    // the control high-water and prevents teardown from entering the profile.
    std::mem::forget(term);
}

/// Fixture-only control for normalization: source text exists, no parse runs.
fn heap_fixture_normalize_body(depth: usize) {
    let source = nested_list_source(depth);
    assert_carries(
        "the heap normalizer fixture's source nesting",
        source_bracket_depth(&source),
        depth,
    );
    std::hint::black_box(&source);
    std::mem::forget(source);
}

/// Fixture-only control for evaluation: nested ENot input, no evaluator.
fn heap_fixture_eval_body(depth: usize) {
    let term = nested_nots(depth);
    assert_carries(
        "the heap evaluator fixture's nesting",
        enot_depth(&term),
        depth,
    );
    std::hint::black_box(&term);
    std::mem::forget(term);
}

// ---------------------------------------------------------------------------
// THE GATE
// ---------------------------------------------------------------------------

/// Traversals that have been converted to a heap-bounded (explicit worklist)
/// form. Membership of this list is the deliverable; it only ever grows.
///
/// See `theta_depth_tripwire` below and
/// `docs/design/audits/theta-depth-traversals-2026-07-26.md` § "Disposition"
/// for what is still outstanding. A traversal is added here only once its
/// bisected minimum stack is flat across the whole ladder in **both** profiles;
/// adding one on the strength of a fixed-stack ladder alone would be a false
/// claim, because a large intercept reads as a zero slope on a short ladder.
///
/// **Done-for-the-family** is this list containing every hand-written member —
/// `substitute`, `substitute_no_sort`, `substitute_binders`, `sort`,
/// `score_cmp`, `tree_drop`, `tree_clone`, `pretty` — on the depth axis, and
/// `substitute_wide`, `sort_wide`, `score_cmp_wide`, `pretty_wide` on the width
/// axis, **in both profiles**.
///
/// ★ With Stage D (`pretty` / `pretty_wide`) that enumeration is now COMPLETE:
/// every hand-written member named above is present on both axes.
#[test]
fn converted_traversals_are_depth_independent() {
    // ★ The lists live in [`CONVERTED_DEPTH`] / [`CONVERTED_WIDTH`] so that one
    // register serves both this test and `the_audit_agrees_with_the_gate`.
    let converted_depth: &[&str] = CONVERTED_DEPTH;
    let converted_width: &[&str] = CONVERTED_WIDTH;

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

/// ★ **The executed reddening leg for the depth axis.**
///
/// Runs the synthetic control past all three depth-axis checkers and asserts
/// each one puts `synthetic_sloped` and `synthetic_flat` on OPPOSITE sides:
///
/// | checker                       | `synthetic_sloped` | `synthetic_flat` |
/// |-------------------------------|--------------------|------------------|
/// | fixed-stack half (1 MiB)      | ACCEPTS ⚠          | accepts          |
/// | [`zero_slope_verdict`]        | **rejects** ✓      | accepts ✓        |
/// | [`slope_below_verdict`]       | **rejects** ✓      | accepts ✓        |
///
/// The first row is not a defect — it is the measurement that
/// [`assert_depth_independent`] describes in prose and has never run. A subject
/// that is Θ(depth) by construction *passes* the fixed-stack half at depths 4
/// through 256, because 256 levels of a small frame fit inside 1 MiB however
/// non-zero the slope is. That is precisely why half (2) exists, and it is now
/// demonstrated rather than derived from `compare_score`'s numbers.
///
/// The last two rows are the N1 obligation: each judge is run on data in the
/// class it excludes, returns a rejection, and the test asserts WHICH clause
/// produced it. The `synthetic_flat` column is the bidirectionality — a checker
/// that rejected everything would fail here, so "rejects" means "discriminates".
fn the_depth_checkers_reject_a_known_theta_depth_subject() {
    // Two bisections per subject, over the SAME ladder the real bar uses. The
    // verdicts are pure functions of these observations, so one measurement
    // feeds every checker below.
    let sloped = measure_ladder("synthetic_sloped", 4, 4096);
    let flat = measure_ladder("synthetic_flat", 4, 4096);

    // ── N2: the observations came from the real subject, not from a stub. ──
    // A frame costs at least its own ballast, and the toy is a toy: anything far
    // outside this band means the bisection measured something else (a build
    // that inlined the recursion away, a subject-table mix-up) and the rejections
    // below would be attributable to the wrong thing.
    assert!(
        sloped.per_step_of_control("sloped") >= SYNTHETIC_FRAME_BYTES,
        "the synthetic control must actually cost its ballast per level: measured \
         {} B/level against {SYNTHETIC_FRAME_BYTES} B of ballast ({} KiB at depth 4, \
         {} KiB at depth 4,096). Below the ballast means the recursion was elided, \
         and this leg would be certifying nothing.",
        sloped.per_step_of_control("sloped"),
        sloped.lo_stack,
        sloped.hi_stack
    );
    assert!(
        sloped.per_step_of_control("sloped") <= 16 * SYNTHETIC_FRAME_BYTES,
        "the synthetic control measured {} B/level, more than 16× its \
         {SYNTHETIC_FRAME_BYTES} B ballast — that is not the toy this leg thinks it \
         is driving",
        sloped.per_step_of_control("sloped")
    );

    // The sharpest statement of the difference, and it needs no checker at all:
    // on the stack that suffices for the FLAT control at parameter 4,096, the
    // SLOPED one does not survive.
    assert!(
        !runs_within(
            flat.hi_stack.bytes_upper_bound("flat"),
            4096,
            "synthetic_sloped"
        ),
        "the two synthetic subjects must be separable at all: `synthetic_flat` needs \
         {} KiB at 4,096 and `synthetic_sloped` survived the same stack, so the \
         control has no slope and every rejection below is vacuous",
        flat.hi_stack
    );

    // ── The fixed-stack half, run on a subject that is Θ(depth) by construction.
    // It ACCEPTS — which is the point. `assert_depth_independent` says so in
    // prose; this executes it.
    for depth in [4usize, 16, 64, 256] {
        assert!(
            runs_within(1024 * 1024, depth, "synthetic_sloped"),
            "the fixed-stack half was expected to ACCEPT the Θ(depth) control at depth \
             {depth} — that is the measurement justifying half (2). If it rejects, the \
             control's per-level cost has grown past the point where this argument \
             holds and the ballast should be reduced, not the claim."
        );
    }

    // ── Checker 1: the zero-slope half. The class it excludes is exactly what
    // `synthetic_sloped` is.
    let why = zero_slope_verdict("synthetic_sloped", "depth", sloped)
        .expect_err("ZERO-SLOPE must reject a subject that is Θ(depth) by construction");
    assert!(
        why.contains("ZERO-SLOPE GATE FAILED"),
        "the rejection must come from the zero-slope clause, not from something else \
         that happened to fail; got: {why}"
    );
    assert!(
        why.contains("depth axis"),
        "the rejection must name the axis it was asked about; got: {why}"
    );
    // …and it rejects with room to spare, so the verdict is not riding on the
    // bisection's 4 KiB resolution.
    assert!(
        clears_tolerance_by_an_order_of_magnitude(sloped),
        "the control's growth ({} KiB) must clear ZERO_SLOPE_TOLERANCE ({} KiB) by an \
         order of magnitude, or this leg is a coin-flip on bisection noise",
        sloped.growth_of_control("sloped") / 1024,
        ZERO_SLOPE_TOLERANCE / 1024
    );

    // ── Checker 2: the tripwire's own ceiling comparison, BOTH directions.
    // Ceilings are derived from the control's measured slope rather than
    // hardcoded, so this leg is profile-independent exactly like the rest of the
    // file — no `cfg!(debug_assertions)` constant to drift.
    let why = slope_below_verdict(
        "synthetic_sloped",
        sloped.per_step_of_control("sloped") / 2,
        sloped,
    )
    .expect_err("the tripwire must reject a slope twice its ceiling");
    assert!(
        why.contains("Θ(DEPTH) TRIPWIRE"),
        "the rejection must come from the tripwire clause; got: {why}"
    );
    slope_below_verdict(
        "synthetic_sloped",
        sloped.per_step_of_control("sloped") * 2,
        sloped,
    )
    .expect(
        "the tripwire must ACCEPT a slope at half its ceiling — a checker that rejects \
         unconditionally is no checker",
    );

    // ── The control column: every checker accepts the Θ(1) subject. ──
    zero_slope_verdict("synthetic_flat", "depth", flat)
        .expect("ZERO-SLOPE must accept a subject that is Θ(1) by construction");
    // The tripwire, at the tightest ceiling this file's own arithmetic admits: the per-step
    // equivalent of [`ZERO_SLOPE_TOLERANCE`], i.e. the gate's own definition of "no growth".
    // Over 4 -> 4,096 that is 4 B/level, and the sloped control sits ~40x above it, so the two
    // synthetics are still separated by a wide margin.
    //
    // ⚠ NOT a literal 0. `per_step()` is a quotient of two bisections quantised to
    // `RESOLUTION`, so a ceiling of 0 demands that two independent measurements land in the
    // *same* 4 KiB bucket — the one zero-margin comparison in a file whose every other
    // assertion is explicitly tolerance-based. Measured 25/25 identical here, but a guard
    // whose margin is one bucket is a guard waiting to flake for a reason that is not a
    // regression, and a flaky gate gets disabled.
    let flat_ceiling = ZERO_SLOPE_TOLERANCE / (flat.hi_param - flat.lo_param);
    slope_below_verdict("synthetic_flat", flat_ceiling, flat).expect(
        "the tripwire must accept a Θ(1) subject at the gate's own no-growth ceiling — its \
         minimum stack does not move",
    );
    assert!(
        flat.growth_of_control("flat") <= ZERO_SLOPE_TOLERANCE,
        "the Θ(1) control's minimum stack must not move at all across the ladder; grew {} KiB",
        flat.growth_of_control("flat") / 1024
    );

    println!(
        "  synthetic control (depth): sloped {} B/level ({} KiB -> {} KiB over 4 -> 4,096), \
         flat {} B/level — checkers separate them",
        sloped.per_step_of_control("sloped"),
        sloped.lo_stack,
        sloped.hi_stack,
        flat.per_step_of_control("flat")
    );
}

/// ★ **The executed reddening leg for a DESTRUCTOR.**
///
/// [`the_depth_checkers_reject_a_known_theta_depth_subject`] drives the checkers
/// with a recursive *function*. `par_drop` and `normalize_drop` are recursive
/// *drop glue* — code no call site names, emitted by the compiler from the type
/// and run when a value falls out of scope. This leg supplies the missing
/// datum: a destructor that is Θ(depth) BY CONSTRUCTION
/// ([`synthetic_drop_body`]), and its iterative twin over the identical
/// structure ([`synthetic_drop_flat_body`]), on opposite sides of every
/// depth-axis checker.
///
/// Without it, `assert_slope_below("par_drop", …)` would be a ceiling that had
/// only ever been compared against subjects that cleared it — the thirteenth
/// check that cannot fail.
///
/// ## ⚠ Why this leg does NOT share the function control's ladder
///
/// It did, until 2026-07-27, and in RELEASE that made it RED — not because the
/// destructor stopped being Θ(depth), but because a ladder calibrated for a
/// 96 B function frame does not produce enough *evidence* from a 32 B one. See
/// [`DESTRUCTOR_CONTROL_HI`] for the measured derivation.
fn the_depth_checkers_reject_a_recursive_destructor() {
    let recursive = measure_ladder("synthetic_drop", 4, DESTRUCTOR_CONTROL_HI);
    let iterative = measure_ladder("synthetic_drop_flat", 4, DESTRUCTOR_CONTROL_HI);

    // ── N2: the reading came from the real destructor. ──
    //
    // ⚠ **The floor here is NOT the ballast, and the difference is instructive.**
    // [`synthetic_recurse`] observes its ballast with `black_box` *after* the
    // recursive call, so the array is live across it and must occupy a frame
    // slot — which is why that control's floor can be `SYNTHETIC_FRAME_BYTES`.
    // Drop glue does no such thing: `drop_in_place::<DropChain>` drops fields in
    // declaration order, and `[u8; N]` has no destructor, so the ballast is
    // never read and nothing obliges any profile to keep it in the frame. It
    // costs 95 B/level in debug because `-O0` materialises the whole struct;
    // `-O2` is free to spill nothing but the tail pointer.
    //
    // The first draft of this leg asserted `>= SYNTHETIC_FRAME_BYTES` and went
    // RED at 95 against 96 — a one-byte margin on a quotient of two bisections
    // quantised to `RESOLUTION`, which is precisely the flake this file warns
    // against in [`the_depth_checkers_reject_a_known_theta_depth_subject`]'s
    // `flat_ceiling` note. So the floor is derived from the ABI instead: a
    // recursive call cannot cost less than the return address it pushes. That
    // bound holds in every profile and on every target, and it still catches the
    // only failure this check exists for — an ELIDED recursion reads ~0, not 8.
    const RETURN_ADDRESS_BYTES: usize = std::mem::size_of::<usize>();
    assert!(
        recursive.per_step_of_control("recursive") >= RETURN_ADDRESS_BYTES,
        "the synthetic DESTRUCTOR must cost at least one return address \
         ({RETURN_ADDRESS_BYTES} B) per level: measured {} B/level ({} KiB at {}, {} KiB at \
         {}). Below that means `drop_in_place::<DropChain>` was not recursive in this \
         build — the glue was flattened — and this leg would certify nothing.",
        recursive.per_step_of_control("recursive"),
        recursive.lo_stack,
        recursive.lo_param,
        recursive.hi_stack,
        recursive.hi_param
    );
    assert!(
        recursive.per_step_of_control("recursive") <= 16 * SYNTHETIC_FRAME_BYTES,
        "the synthetic destructor measured {} B/level, more than 16x the \
         {SYNTHETIC_FRAME_BYTES} B chain link it is tearing down — that is not the toy \
         this leg thinks it is driving",
        recursive.per_step_of_control("recursive")
    );

    // The sharpest statement, needing no checker: on the stack that suffices to
    // tear the chain down ITERATIVELY at the ladder's deep end, tearing the SAME
    // chain down recursively does not survive. The two subjects differ in nothing
    // else.
    //
    // ⚠ The link count comes from the ladder rather than from a literal, so that
    // moving [`DESTRUCTOR_CONTROL_HI`] cannot leave this comparison probing a
    // depth neither subject was measured at.
    assert!(
        !runs_within(
            iterative.hi_stack.bytes_upper_bound("iterative"),
            iterative.hi_param,
            "synthetic_drop"
        ),
        "the two destructors must be separable at all: `synthetic_drop_flat` needs {} KiB \
         at {} links and the recursive destructor survived the same stack, so the \
         control has no slope and every rejection below is vacuous",
        iterative.hi_stack,
        iterative.hi_param
    );

    // ── Checker 1: the zero-slope half rejects the recursive destructor. ──
    let why = zero_slope_verdict("synthetic_drop", "depth", recursive)
        .expect_err("ZERO-SLOPE must reject a destructor that is Θ(depth) by construction");
    assert!(
        why.contains("ZERO-SLOPE GATE FAILED"),
        "the rejection must come from the zero-slope clause; got: {why}"
    );
    assert!(
        clears_tolerance_by_an_order_of_magnitude(recursive),
        "the destructor control's growth ({} KiB over {} -> {}) must clear \
         ZERO_SLOPE_TOLERANCE ({} KiB) by an order of magnitude, or this leg is a \
         coin-flip on bisection noise. See DESTRUCTOR_CONTROL_HI: at 32 B/level in \
         release this needs a longer ladder than the 96 B function control does, and \
         the answer is a longer ladder — never a smaller multiple.",
        recursive.growth_of_control("recursive") / 1024,
        recursive.lo_param,
        recursive.hi_param,
        ZERO_SLOPE_TOLERANCE / 1024
    );
    // ★★ **The executed reddening for the clause immediately above** — the one
    // 2026-07-27 changed, by moving this leg's ladder from 4 → 4,096 to
    // 4 → [`DESTRUCTOR_CONTROL_HI`].
    //
    // Lengthening a ladder raises every growth reading, so the hazard that change
    // introduces is precisely that the clause becomes satisfiable by ANY subject.
    // Handing it the ITERATIVE twin — same structure, same link count, same
    // ladder, no slope — and requiring a `false` is the direct refutation. The
    // flat destructor's minimum stack is 12,288 B at both ends, so its growth is
    // 0 and the bar rejects it by the whole 128 KiB.
    //
    // ⚠ Yes, this is IMPLIED by `zero_slope_verdict` accepting `iterative` below
    // (accepting means growth <= 16,384, and 16,384 < 131,072). Implication is
    // exactly what this file does not accept as evidence: `assert_depth_
    // independent`'s zero-slope claim was sound arithmetic on recorded numbers
    // for months and still described a checker nobody had watched refuse
    // anything. The clause is run, on data in the class it must exclude.
    assert!(
        !clears_tolerance_by_an_order_of_magnitude(iterative),
        "the evidence-margin clause must still REJECT a genuinely flat subject: the \
         ITERATIVE destructor grew {} KiB over {} -> {} and cleared the {} KiB bar \
         anyway. That means the ladder is now long enough to manufacture the margin \
         from nothing, and the recursive control's clearance above certifies the \
         ladder rather than the slope.",
        iterative.growth_of_control("iterative") / 1024,
        iterative.lo_param,
        iterative.hi_param,
        (8 * ZERO_SLOPE_TOLERANCE) / 1024
    );

    // ── Checker 2: the tripwire — the checker `par_drop` and `normalize_drop`
    // are actually asserted with — BOTH directions. Ceilings derived from the
    // control's own slope, so this leg carries no profile-dependent constant.
    let why = slope_below_verdict(
        "synthetic_drop",
        recursive.per_step_of_control("recursive") / 2,
        recursive,
    )
    .expect_err("the tripwire must reject a destructor whose slope is twice its ceiling");
    assert!(
        why.contains("Θ(DEPTH) TRIPWIRE"),
        "the rejection must come from the tripwire clause; got: {why}"
    );
    slope_below_verdict(
        "synthetic_drop",
        recursive.per_step_of_control("recursive") * 2,
        recursive,
    )
    .expect(
        "the tripwire must ACCEPT a destructor at half its ceiling — a checker that rejects \
         unconditionally is no checker",
    );

    // ── The control column: the ITERATIVE destructor passes everything. ──
    zero_slope_verdict("synthetic_drop_flat", "depth", iterative)
        .expect("ZERO-SLOPE must accept a destructor that is Θ(1) by construction");
    let flat_ceiling = ZERO_SLOPE_TOLERANCE / (iterative.hi_param - iterative.lo_param);
    slope_below_verdict("synthetic_drop_flat", flat_ceiling, iterative).expect(
        "the tripwire must accept an iterative teardown at the gate's own no-growth ceiling",
    );

    println!(
        "  synthetic DESTRUCTOR (depth): recursive {} B/level ({} KiB -> {} KiB over {} -> \
         {}), iterative {} B/level — checkers separate them",
        recursive.per_step_of_control("recursive"),
        recursive.lo_stack,
        recursive.hi_stack,
        recursive.lo_param,
        recursive.hi_param,
        iterative.per_step_of_control("iterative")
    );
}

/// Tripwire over every traversal still known to be Θ(depth). Ceilings are ~1.5×
/// the values measured on 2026-07-26 (recorded in the audit document), so
/// ordinary codegen drift will not flake while an order-of-magnitude regression
/// still trips.
///
/// A traversal LEAVES this list only by moving to `converted_traversals_are_
/// depth_independent`, never by having its ceiling raised.
///
/// ★ **Its first phase is the executed proof that the depth-axis checkers can
/// REJECT.** Everything below this line is a rejecter, and until 2026-07-27 none
/// of them had ever been seen to refuse a Θ(depth) subject: the tripwire's
/// ceilings were only ever compared against subjects that cleared them, and
/// `assert_depth_independent`'s claim that its zero-slope half "cannot be passed
/// by anything with a non-zero slope" was arithmetic on `compare_score`'s
/// recorded numbers, not an execution. A checker that accepted everything would
/// have satisfied both, which is to say the guard could not fail.
///
/// The phase feeds [`synthetic_sloped_body`] — Θ(parameter) BY CONSTRUCTION, not
/// by measurement — to all three depth-axis checkers and asserts each one
/// rejects it, then feeds [`synthetic_flat_body`] to the same checkers and
/// asserts each one accepts. Both directions, as in
/// `normalize_oracle_provenance.rs::the_provenance_check_can_go_red`.
#[test]
fn theta_depth_tripwire() {
    the_depth_checkers_reject_a_known_theta_depth_subject();
    the_depth_checkers_reject_a_recursive_destructor();

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
    //
    // ⚠ `pretty` has LEFT this list — it is in
    // `converted_traversals_are_depth_independent` (Stage D: `PrettyPrinter`
    // became an explicit pushdown driver, `pretty_printer.rs` `mod drive`).
    // Measured immediately before the conversion by direct bisection of this
    // very subject: **41,984 B/level debug** (724,992 B at depth 16, 2,740,224 B
    // at depth 64) and **4,266 B/level release** (81,920 and 286,720) — i.e. a
    // maximum nesting depth of ~49 / ~490 on a 2 MiB tokio worker. Measured
    // immediately after: a FLAT 45,056 B (debug) and 12,288 B (release) at
    // depths 4, 16, 64 and 4,096 alike, and the width subject `pretty_wide` a
    // flat 49,152 / 12,288 at widths 4 through 65,536. As always, a traversal
    // leaves this list only by being converted, never by having its ceiling
    // raised.
    // ★★ THE METERED WRAPPER — see `subst_and_charge_body`.
    //
    // The ceiling was `ceiling(25_000, 12_000)`, matching `substitute_deep_binding`
    // below it, because both subjects were then the SAME traversal:
    // `<Par as Clone>::clone` over a depth-`d` `Par`, reached through two call
    // sites. The wrapper's copy is gone (it takes its term by value), so they are
    // no longer the same traversal and no longer share a ceiling:
    //
    //   subst_and_charge   15,872 -> 1,462 B/level debug   2,852 -> 146 release
    //
    // That was an INTERMEDIATE state, not the final disposition. The generated
    // `prost::Message` implementation now routes `encoded_len` through the
    // memoized bottom-up protobuf PDA, and generated `Clone` breaks the recursive
    // schema cycle. `subst_and_charge` therefore moved to `CONVERTED_DEPTH` and
    // is held to the full 4 → 4,096 zero-slope ladder. No ceiling remains.
    //
    // ★ **Shown RED at the value it exists to refuse.** With the subject body
    // reverted to a copying call — `substitute_and_charge(t.clone(), …)`, which
    // reproduces the defect at the call site rather than inside the wrapper —
    // this line failed with `2852 B/level exceeds the 700 B/level ceiling
    // (56 KiB @ depth 16 -> 368 KiB @ depth 128)`. 2,852 is precisely the
    // pre-repair reading, so the lowered ceiling refuses the exact regression it
    // was lowered to refuse, and not merely something.
    // ⚠★ `assert_slope_below("substitute_deep_binding", ceiling(25_000, 12_000),
    // 16, 128)` USED TO BE HERE, and it is deleted rather than relaxed.
    //
    // The subject is in [`CONVERTED_DEPTH`], driven by
    // `converted_traversals_are_depth_independent` over 4 → 4,096. A ceiling
    // here would be a WEAKER statement about the same traversal and would put
    // the name in two registers at once.
    //
    // ★ And the ceiling it carried was never certifying anything: on 16 → 128
    // this subject read **0 B/level in BOTH profiles** at the commit that
    // converted `Par::clone` AND at the commit before it, because both probe
    // points sat inside the intercept. The tripwire could not have gone red on
    // the regression it existed to catch. The 4 → 4,096 bar can.
    //
    // ⚠ **The clause "because `slope_below_verdict` consults only `per_step`" is
    // struck from that sentence, and the correction is not cosmetic.** It named the
    // wrong mechanism: the readings here were genuine measurements far above the
    // instrument floor, and their difference was genuinely small because the LADDER
    // was too short — not because the verdict mis-read a floor. Attributing an
    // intercept artefact to the verdict's arithmetic pointed the repair at the
    // instrument when the fix was a longer ladder, which is exactly the prescription
    // that then sat here as load-bearing documentation.
    // ⇒ Both defects are now closed, separately: this ladder moved to 4 → 4,096, and
    // `slope_below_verdict` refuses an unreadable ladder rather than scoring it `0`.
    // ⚠★ `assert_slope_below("clone", ceiling(25_000, 5_000), 16, 128)` USED TO BE
    // HERE, and it is deleted rather than relaxed.
    //
    // Stage F-4 converted `<Par as Clone>::clone`: `models/build.rs` strips the
    // `Clone` derive from the 55 non-`Copy` `rhoapi` items and
    // `models/codegen/schema.rs` generates the impls, `Par`'s as an
    // explicit-worklist traversal over `drive::drive_with`. The subject is in
    // [`CONVERTED_DEPTH`] and `converted_traversals_are_depth_independent` now
    // drives it, so a ceiling here would be a weaker statement about the same
    // traversal AND would put the name in two registers at once.
    //
    // ★ **It LEFT the tripwire list by being CONVERTED, never by having its
    // ceiling raised** — the same rule and the same words as `bincode_ser`'s
    // departure. Pre-conversion baselines, by direct bisection of this subject:
    // 16,493 B/level debug and 3,254 release.
    // ★★ THE DEPLOY-REACHABLE COMPOSITION — see `normalize_drop_body`.
    //
    // `normalize` is converted (0 B/level) and `par_drop` is not (~470), so a
    // deploy can BUILD a term the runtime cannot DESTROY. Ceiling is `par_drop`'s
    // own, because the normalizer contributes nothing: this subject is here to
    // keep the composition measured, and to fail if the teardown gets worse or
    // if the normalizer stops being flat.
    //
    // ⚠ **It stays here even though one of its two production instances was
    // converted on 2026-07-28.** Deploy ADMISSION now routes through
    // `interpreter_util::validate_deploy_term` (parse ▸ `dismantle`, 0 B/level,
    // no ceiling below 262,144 — gated in `casper/tests/`). The EVALUATION
    // instance, `inj_attempt` releasing the reduced term through the derived
    // destructor, is untouched, and that is what this subject stands for. A name
    // leaves this list when its traversal is gone, never when a sibling call site
    // is fixed.
    // ⚠ THE RSpace CODEC HAS LEFT THIS LIST ENTIRELY — both halves.
    //
    // `bincode_de` left first (Stage F: `models/src/rust/rholang/bincode_decoder.rs`,
    // an explicit obligation-stack decoder). `bincode_ser` has now followed
    // (Stage H: `models/src/rust/rholang/bincode_encoder.rs`, the single-walk
    // trampolined encoder driven by the same generated table). Both are in
    // `converted_traversals_are_depth_independent`.
    //
    // The old note here said the encoder stays Θ(depth) "on purpose", because
    // leaving `Serialize` derived was what made the cold-store leaf bytes
    // byte-identical by construction. That trade no longer has to be made: byte
    // identity is now established by DIFFERENTIAL against the derive — which
    // stays compiled as the oracle, and as the `bincode_ser_derived` control in
    // this very gate — over an exhaustive structural corpus plus proptest, with
    // an executed mutation proof that the differential can go RED.
    //
    // Pre-conversion baselines, by direct bisection of this subject:
    // ENCODE 3,052 B/level debug and 329 B/level release; DECODE 28,331 and
    // 12,971. As always, a traversal leaves this list only by being converted.
    // `sort_nested_set` and `sort_nested_map` left this tripwire by conversion.
    // Stage C-2b puts their members on the same PDA worklist and combines
    // already-scored children without nested sorter calls. They now run through
    // `converted_traversals_are_depth_independent` over the full 4 → 4,096
    // ladder, so retaining a slope ceiling here would be a weaker duplicate.

    // ── ★ Close the register loop. ──
    // `TRIPWIRE_DEPTH` is what `the_audit_agrees_with_the_gate` publishes; this
    // asserts it is what actually ran. Adding an `assert_slope_below` above
    // without listing the subject — or listing one without asserting it — fails
    // HERE, at the commit that does it, instead of drifting until the next audit.
    //
    // ★ **SHOWN RED, not merely asserted.** Adding `"eq"` to [`TRIPWIRE_DEPTH`]
    // with no matching `assert_slope_below` — `eq` is a real subject in
    // [`subject`], so the failure is the register's and not an unknown name —
    // fails this assertion with:
    //
    //     THE SUBJECT REGISTER IS OUT OF DATE. `theta_depth_tripwire` drove
    //     [.., "encode"] but `TRIPWIRE_DEPTH` lists [.., "eq"].
    //
    // ⚠ Note the direction, because it is the opposite of the one usually
    // quoted: a name in a CONVERTED list must have NO `assert_slope_below` — one
    // would put the name in the driven set while `TRIPWIRE_DEPTH` omits it, and
    // this same assertion would reject it. Membership in exactly one register,
    // enforced from both sides.
    let driven = SLOPE_SUBJECTS_DRIVEN.with(|s| s.borrow().clone());
    let mut driven_sorted = driven.clone();
    driven_sorted.sort();
    let mut expected: Vec<String> = TRIPWIRE_DEPTH.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(
        driven_sorted, expected,
        "THE SUBJECT REGISTER IS OUT OF DATE. `theta_depth_tripwire` drove {:?} but \
         `TRIPWIRE_DEPTH` lists {:?}. That constant is the one the audit document is \
         checked against, so a mismatch here is exactly the drift \
         `the_audit_agrees_with_the_gate` exists to prevent — fix the constant in the \
         same commit as the assertion.",
        driven, TRIPWIRE_DEPTH
    );
}

// ---------------------------------------------------------------------------
// ★ THE FOUR-QUADRANT S0 BASELINE
// ---------------------------------------------------------------------------

/// One S0 row: a subject, the label the four-quadrant design knows it by, and
/// the ladder to measure it on.
///
/// `hi_max` is what makes the ladder **self-selecting** rather than hand-picked;
/// see [`S0_MIN_GROWTH`].
struct S0Ladder {
    /// The [`subject`] name — the traversal's ONE name in this gate.
    subject: &'static str,
    /// The four-quadrant design's label for the same traversal. Equal to
    /// `subject` except where the design and this gate inherited different
    /// vocabularies for one thing.
    label: &'static str,
    lo: usize,
    /// Where the deep end STARTS. It is widened until the ladder binds.
    hi: usize,
    /// The deepest the deep end may be widened to. Reaching it without binding
    /// is a loud failure, never a shrug.
    hi_max: usize,
}

/// The growth a ladder must produce before its derived slope is evidence.
///
/// ★★ **The release run of the first version of this test failed here, and the
/// failure was correct.** `par_drop` was given the common `16 → 128` ladder; in
/// release it costs ~470 B/level, so BOTH ends bisected into
/// [`min_stack_for`]'s initial 16 KiB probe window and the derived slope was
/// `(16384 - 12288) / 112 = 36 B/level` — quantisation, not measurement. The
/// gate's own [`assert_slope_below`] states the requirement in prose ("the CHEAP
/// traversals must clear the minimum viable thread stack at the deeper point —
/// otherwise both probes bottom out on the same floor and the derived slope is a
/// meaningless 0"); this constant makes it executable.
///
/// Eight [`RESOLUTION`] buckets. [`Ladder::growth`] is a difference of two
/// bisections each quantised to `RESOLUTION`, so it carries `±2 * RESOLUTION` of
/// uncertainty; requiring eight buckets bounds that at ±25%. Hand-picking a
/// per-subject, per-profile ladder instead would put the analyst in the
/// measurement loop, which is how the inherited constants this stage exists to
/// replace got there.
const S0_MIN_GROWTH: usize = 8 * RESOLUTION;

/// No production traversal remains in the historical sloped baseline. The
/// before measurements remain in the audit; current implementations are all
/// exercised by the full depth-independence register above.
const S0_LADDERS: &[S0Ladder] = &[];

/// Measure `rung`, widening its deep end until the ladder BINDS.
///
/// "Binds" means [`Ladder::growth`] clears [`S0_MIN_GROWTH`] — i.e. the reading
/// is a measurement rather than the difference of two numbers sitting in
/// [`min_stack_for`]'s initial probe window.
///
/// ⚠ It **fails loudly** at `hi_max` rather than returning the unbound ladder,
/// and the message distinguishes the two things that can produce it, because
/// they call for opposite responses:
///
/// * the traversal is genuinely **depth-independent**, in which case it does not
///   belong in a slope row at all — [`assert_no_slope`] is its checker;
/// * the ladder is **too short**, in which case `hi_max` is what needs raising.
///
/// Returning a `0 B/level` row for either would put a claim in the baseline that
/// no measurement supports, which is the whole failure mode S0 exists to end.
fn s0_binding_ladder(rung: &S0Ladder) -> Ladder {
    let mut hi = rung.hi;
    loop {
        let l = measure_ladder(rung.subject, rung.lo, hi);
        // ★ An UNRESOLVED ladder is exactly "not bound yet" — the state this loop already
        // exists to escape by lengthening the ladder. It falls through to the expansion
        // below rather than needing a new branch: this harness's SHAPE was always right, it
        // was the `saturating_sub` underneath it that reported a floor pair as a growth of
        // 0 rather than as an unreadable ladder.
        if l.growth().is_ok_and(|g| g >= S0_MIN_GROWTH) {
            return l;
        }
        let growth_note = match l.growth() {
            Ok(g) => format!("{g} B"),
            Err(why) => format!("an unreadable amount — {why}"),
        };
        let per_step_note = match l.per_step() {
            Ok(p) => format!("{p}"),
            Err(_) => "an unreadable".to_string(),
        };
        assert!(
            hi < rung.hi_max,
            "S0 LADDER DID NOT BIND for `{}` ({}): at depth {} → {} the minimum stack grew \
             only {}, below the {} B this harness requires before a derived slope is \
             evidence rather than bisection quantisation (±{} B). The deep end is already \
             at its maximum {}.\n\
             \n\
             Two things produce this, and they need opposite responses:\n\
             \x20 (a) the traversal is DEPTH-INDEPENDENT — then it belongs in \
             `converted_traversals_are_depth_independent`, checked by `assert_no_slope`, \
             not in an S0 slope row;\n\
             \x20 (b) the ladder is too SHORT — then raise `hi_max` for this row.\n\
             \n\
             Reporting `{} B/level` instead would put a number in the baseline that no \
             measurement supports.",
            rung.subject,
            rung.label,
            rung.lo,
            hi,
            growth_note,
            S0_MIN_GROWTH,
            2 * RESOLUTION,
            rung.hi_max,
            per_step_note
        );
        // Double, but never past the declared measurement limit.
        hi = (hi * 2).min(rung.hi_max);
    }
}

/// ★★★ **Stage F-4's bisection evidence, as a TEST rather than as a transcript.**
///
/// A conversion's evidence is a before/after pair at both ends of a ladder, in
/// both profiles. The "before" is normally taken by reverting the change — which
/// for a converted TRAIT IMPL means un-deriving and re-deriving `Clone`, and this
/// workspace has already frozen a deliberately-broken probe form into history
/// once by committing mid-measurement.
///
/// So the "before" is a **live control**. `models/codegen/schema.rs` §F retains
/// the derive's own body as `term_ops::oracle_clone_par`, and
/// `models/tests/clone_equivalence_corpus.rs` proves it byte-identical to the
/// driven form on eight axes over 67 enumerated shapes. Subject and control are in
/// the same binary, on the same ladder, in the same invocation.
///
/// ## The four legs
///
/// | leg | what it establishes |
/// |---|---|
/// | the control is SLOPED, by an order of magnitude | the ladder can see a Θ(depth) clone at all, so the subject's flatness is a finding and not a short ladder |
/// | the subject is FLAT | the conversion worked |
/// | the control does **not** survive the stack the subject needs at the deep rung | the sharpest form of the difference, and it needs no checker |
/// | the two `Vec`-nested ladders are flat too | ★ the collection-element boundary, which a single-collection ladder cannot cover |
///
/// ⚠ **Leg 3 is the guard watched RED at the value it refuses.** `clone_oracle` IS
/// the pre-conversion `<Par as Clone>::clone` semantically; asserting that it
/// fails on the subject's stack is the same statement as "reverting the emitter
/// reproduces a Θ(depth) clone", taken by measurement instead of by revert.
///
/// ## ⚠★ How faithfully the oracle reproduces the derive — MEASURED, and not
/// equally in both profiles
///
/// ```text
///                       oracle, measured here      derive, on record
///   debug                    16,128 B/level          16,493 B/level   (−2.2%)
///   release                   7,021 B/level           3,254 B/level   (+116%)
/// ```
///
/// The oracle is a **semantic** reproduction, and that much is proved rather than
/// asserted: `models/tests/clone_equivalence_corpus.rs` shows it byte-identical to
/// the driven clone on eight axes over 67 enumerated shapes. It is **not** a
/// frame-layout reproduction. In debug it lands within 2.2% of the derive; under
/// `-O` a family of free functions inlines differently from a single
/// `<Par as Clone>::clone`, and it costs 2.16× more per level.
///
/// ⇒ **Do not read 7,021 as the derive's release figure.** The derive's release
/// figure is **3,254 B/level**, bisected directly from this subject at HEAD before
/// the conversion and recorded in
/// `docs/design/audits/four-quadrant-s0-baseline-2026-07-28.md` and in
/// `models/codegen/schema.rs`'s disposition table.
///
/// ★ The divergence is CONSERVATIVE for every leg that uses the oracle: legs 1 and
/// 3 both want the control to be *expensive*, so an over-costly control makes them
/// easier to pass and cannot manufacture a false green for leg 2 — which measures
/// the subject alone.
///
/// `#[ignore]`d because it is eight bisections, each an exponential probe plus
/// ~15 child processes. Drive it per profile:
///
/// ```text
///   cargo test -p rholang --test stack_depth_gate -- --ignored --exact \
///       the_clone_conversion_is_visible_against_its_own_derived_control --nocapture
///   cargo test --release -p rholang --test stack_depth_gate -- --ignored --exact \
///       the_clone_conversion_is_visible_against_its_own_derived_control --nocapture
/// ```
#[test]
#[ignore = "eight bisections; drive explicitly per profile"]
fn the_clone_conversion_is_visible_against_its_own_derived_control() {
    // The ladder `clone` was tripwired on before the conversion, so the numbers
    // are comparable to the ones on record.
    const LO: usize = 16;
    const HI: usize = 128;

    let control = measure_ladder("clone_oracle", LO, HI);
    let subject = measure_ladder("clone", LO, HI);

    println!(
        "  clone_oracle (the DERIVE): {} KiB @ {LO} -> {} KiB @ {HI} = {} B/level",
        control.lo_stack,
        control.hi_stack,
        control.per_step_of_control("control")
    );
    println!(
        "  clone        (the DRIVER): {} KiB @ {LO} -> {} KiB @ {HI} = {} B/level",
        subject.lo_stack,
        subject.hi_stack,
        show(subject.per_step())
    );

    // ── LEG 1: the control is sloped, by an order of magnitude. ──
    assert!(
        clears_tolerance_by_an_order_of_magnitude(control),
        "the DERIVED control `clone_oracle` grew only {} B across depth {LO} -> {HI} ({} \
         B/level). It is `term_ops::oracle_clone_par`, the `#[derive(Clone)]` body this stage \
         replaced, and it was measured at 16,493 B/level debug / 3,254 release before the \
         conversion. A flat control means the oracle is not the traversal it claims to be — most \
         likely it has started routing through the DRIVEN `<Par as Clone>::clone` instead of \
         recursing into its own family — and every comparison below would be vacuous.",
        control.growth_of_control("control"),
        control.per_step_of_control("control")
    );

    // ── LEG 2: the subject is flat. ──
    if let Err(why) = zero_slope_verdict("clone", "depth", subject) {
        panic!("{why}");
    }

    // ── LEG 3: the control does not survive the subject's stack. ──
    //
    // The sharpest statement of the difference, and it needs no verdict function:
    // on the stack that suffices for the DRIVEN clone at depth `HI`, the DERIVED
    // one does not run.
    assert!(
        !runs_within(
            subject.hi_stack.bytes_upper_bound("subject"),
            HI,
            "clone_oracle"
        ),
        "★ the DERIVED control survived the {} KiB the DRIVEN clone needs at depth {HI}, so the \
         two are not separable on this ladder and the conversion is not visible. Either the \
         ladder is too short (raise HI) or the oracle is no longer the derive.",
        subject.hi_stack
    );

    // ── LEG 4: the Vec-nested ladders, which `clone` alone cannot cover. ──
    //
    // ★★ `clone` descends `Par.exprs -> Expr -> EList.ps -> Par`: two of `Par`'s
    // nine recursive collections. The sibling `mettail-rust` generator's eight
    // Θ(depth) "iterative" drivers all escaped through the SAME boundary —
    // `Category -> Vec<Elem> -> Elem` — and a single-collection ladder is exactly
    // how that hid. These two nest through different fields.
    for (name, lo, hi) in [("clone_send_chain", LO, HI)] {
        let l = measure_ladder(name, lo, hi);
        println!(
            "  {name}: {} KiB @ {lo} -> {} KiB @ {hi} = {} B/level",
            l.lo_stack,
            l.hi_stack,
            show(l.per_step())
        );
        if let Err(why) = zero_slope_verdict(name, "depth", l) {
            panic!(
                "{why}\n\
                 \n\
                 ★ This ladder nests through a DIFFERENT collection field from `clone`'s, and it \
                 is sloped while `clone` is flat. That is the exact signature of \
                 COLLECTION-ELEMENT DELEGATION: the emitter routed one `Vec<T>` field to a \
                 whole-value `<Vec<T> as Clone>::clone`, which re-enters the element type's \
                 `Clone` and recurses. Check `clone_push_children_*` in \
                 `OUT_DIR/rhoapi_term_ops.rs` for a `.clone()` where a `for` loop belongs."
            );
        }
    }
}

/// ★★ **The S0 baseline: measure first, claim nothing.**
///
/// This test asserts **no ceiling**. It bisects each subject's minimum stack at
/// both ends of its ladder and prints the derived bytes-per-level, and that is
/// the whole deliverable — the baseline every later stage's win is measured
/// against. A ceiling is a claim; S0 exists precisely because the claims that
/// were in circulation had been inherited rather than measured.
///
/// ⚠ **The audit's §12.6 constants (2,852 / 144 / 310 / 1,244) are NOT
/// inherited here.** `9082d12c` removed a *call* to `<Par as Clone>::clone` at
/// `inj_attempt`'s set-initial-cost phase and entered the COMPOSITION in
/// [`CONVERTED_DEPTH`] as `inj_attempt_clone`. And `tree_clone` / `tree_drop` are
/// `score_tree::Tree<T>`, not `Par`. Every number this test prints is re-measured
/// at HEAD.
///
/// ⚠★ **CORRECTED 2026-07-29.** This paragraph used to end *"`<Par as
/// Clone>::clone` itself is untouched and still in `TRIPWIRE_DEPTH`"*. That was
/// true when it was written and it is not true now: **stage F-4 converted it**
/// (`models/build.rs` strips the `Clone` derive from the 55 non-`Copy` `rhoapi`
/// items; `models/codegen/schema.rs` generates the impls, `Par`'s over
/// `drive::drive_with`). `clone` is in [`CONVERTED_DEPTH`], its
/// [`assert_slope_below`] call is deleted, and its [`S0_LADDERS`] row is gone —
/// see the note on that constant. The reading the old sentence stood behind,
/// **16,493 B/level debug and 3,254 release**, is now the pre-conversion baseline
/// rather than a current measurement.
///
/// ## Running it
///
/// It is `#[ignore]`d because a full run is ~40 bisections, each an exponential
/// probe plus ~15 child processes, and CI runs the whole `models`/`rholang`
/// suites on every push. Drive it explicitly, per profile:
///
/// ```text
///   cargo test -p rholang --test stack_depth_gate -- --ignored --exact \
///       four_quadrant_s0_baseline --nocapture
///   cargo test --release -p rholang --test stack_depth_gate -- --ignored --exact \
///       four_quadrant_s0_baseline --nocapture
/// ```
///
/// The recorded results live in
/// `docs/design/audits/four-quadrant-s0-baseline-2026-07-28.md`. They are
/// recorded THERE and not restated here, for the reason
/// [`the_audit_agrees_with_the_gate`] exists: two copies of one truth do not
/// stay equal.
///
/// ## Why it prints CSV
///
/// So the record is transcribed by `tee`, not by hand. Every recorded number in
/// this campaign that was typed into prose has drifted from the code that
/// produced it.
#[test]
#[ignore = "S0 baseline measurement: ~40 bisections; drive explicitly, see the doc comment"]
fn four_quadrant_s0_baseline() {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("S0-BEGIN profile={profile}");
    println!("subject,label,profile,lo_depth,lo_stack_b,hi_depth,hi_stack_b,growth_b,b_per_level");

    let mut measured = 0usize;
    for rung in S0_LADDERS {
        let l = s0_binding_ladder(rung);
        println!(
            "{},{},{profile},{},{},{},{},{},{}",
            rung.subject,
            rung.label,
            l.lo_param,
            l.lo_stack,
            l.hi_param,
            l.hi_stack,
            show(l.growth()),
            show(l.per_step())
        );
        measured += 1;
    }

    assert_eq!(
        measured,
        S0_LADDERS.len(),
        "the S0 table must carry one row per ladder; a short table is a baseline with a \
         silent hole in it"
    );
    println!("S0-END profile={profile} rows={measured}");
}

/// ★★ **The deploy path's stack requirement is bounded BELOW by its
/// destructor's — stated as a structural invariant, not as a pinned number.**
///
/// `normalize_drop` runs `Compiler::source_to_adt` and then releases the result,
/// SEQUENTIALLY. Two traversals that do not nest need
/// $`\max(S_{\text{normalize}}, S_{\text{drop}})`$ of native stack, not their
/// sum, which puts the composition inside a band both of whose ends are
/// structural:
///
/// ```math
/// S_{\text{par\_drop}} \;\le\; S_{\text{normalize\_drop}} \;\le\;
///   S_{\text{par\_drop}} + S_{\text{normalize}}
/// ```
///
/// ⚠ **Neither end inverts when the defect is fixed**, which is why the band is
/// the assertion and the equality is not. $`\max(a,b) \ge b`$ holds however
/// small $`b`$ becomes, so converting the destructor leaves this test green —
/// whereas "the composition costs exactly what the destructor costs" is TRUE
/// TODAY and would have to be deleted the day somebody fixed it. A guard that
/// must be deleted to record success is a guard that discourages success.
///
/// **What each end catches.**
///
/// * The lower bound catches a **collapsed fixture**: if `normalize_drop` ever
///   needed less than `par_drop`, its term would not be carrying the depth, and
///   its tripwire reading would be a comfortable, meaningless number. This is
///   the same failure the audit records three times (§4.3, §11.3, and the
///   score-tree subjects), approached from the composition side.
/// * The upper bound catches the composition becoming **nested** rather than
///   sequential — a future `source_to_adt` that held a Θ(depth) frame chain
///   alive across the teardown would exceed the sum and trip here, and nothing
///   else in this file would notice.
///
/// **Measured 2026-07-27, debug, over 256 → 4,096** (the arithmetic is recorded
/// because the band deliberately does not pin it):
///
/// | subject | min stack @ 256 | min stack @ 4,096 | B/level |
/// |---|---:|---:|---:|
/// | `normalize` | 196 KiB | 196 KiB | **0** |
/// | `par_drop` | 124 KiB | **1,864 KiB** | 464 |
/// | `normalize_drop` | 196 KiB | **1,864 KiB** | 444 |
///
/// The deep ends are IDENTICAL to the byte, and the two slopes differ only
/// because the shallow end of `normalize_drop` is clamped at the normalizer's
/// 200,704 B floor rather than the destructor's 126,976 B one:
/// $`(1{,}908{,}736 - 200{,}704)/3{,}840 = 444.8`$ against
/// $`(1{,}908{,}736 - 126{,}976)/3{,}840 = 464.0`$. So at depth the deploy
/// path's cost *is* the destructor's cost, and the normalizer's conversion
/// bought a flat term-builder in front of a Θ(depth) term-releaser.
#[test]
fn the_deploy_composition_is_bounded_below_by_its_destructor() {
    let compose = measure_ladder("normalize_drop", 256, 4096);
    let destruct = measure_ladder("par_drop", 256, 4096);
    let build = measure_ladder("normalize", 256, 4096);

    // ── Lower bound: sequential composition cannot cost LESS than either part.
    assert!(
        compose.hi_stack.bytes_upper_bound("compose") + ZERO_SLOPE_TOLERANCE
            >= destruct.hi_stack.bytes_upper_bound("destruct"),
        "VACUOUS COMPOSITION: `normalize_drop` needed only {} KiB at depth 4,096 while \
         `par_drop` — the destructor it runs — needed {} KiB. A composition cannot cost \
         less than a traversal it performs, so the normalized term is not carrying the \
         depth the subject asked for and its tripwire reading means nothing.",
        compose.hi_stack.bytes_upper_bound("compose"),
        destruct.hi_stack.bytes_upper_bound("destruct")
    );

    // ── Upper bound: the two traversals must not NEST.
    assert!(
        compose.hi_stack.bytes_upper_bound("compose")
            <= destruct.hi_stack.bytes_upper_bound("destruct")
                + build.hi_stack.bytes_upper_bound("build")
                + ZERO_SLOPE_TOLERANCE,
        "`normalize_drop` needed {} KiB at depth 4,096, more than `par_drop` ({} KiB) plus \
         `normalize` ({} KiB). Those two run one after the other, so the composition should \
         need the MAXIMUM and not the SUM — exceeding the sum means the normalizer is now \
         holding a frame chain alive across the teardown, which is a new Θ(depth) member \
         that no other assertion in this file would see.",
        compose.hi_stack.bytes_upper_bound("compose"),
        destruct.hi_stack.bytes_upper_bound("destruct"),
        build.hi_stack.bytes_upper_bound("build")
    );

    // ── The normalizer's own contribution to the composition's SLOPE is nil,
    // and that is the point: the deploy path is flat to build and sloped to
    // release. Asserted as an inequality so a conversion of the destructor
    // leaves it green.
    // ★ A FLOOR reading passes, and it is the same asymmetry `zero_slope_verdict` carries:
    // this leg asserts `normalize` is FLAT, and a subject still fitting under the instrument
    // floor at depth 4,096 has answered that — the floor itself bounds its per-level cost.
    // ⚠ A NEGATIVE difference does NOT pass: that is the probe disagreeing with itself, and
    // the attribution below would rest on an unstable measurement.
    let build_is_flat = match build.growth() {
        Ok(g) => g <= ZERO_SLOPE_TOLERANCE,
        Err(Unresolved::NegativeDifference { .. }) => false,
    };
    assert!(
        build_is_flat,
        "`normalize` grew {} across 256 -> 4,096; it is in the converted list and must \
         be flat, or the attribution of `normalize_drop`'s slope to the destructor is unsound",
        show(build.growth())
    );

    println!(
        // ★ The two stack readings print through `MinStack`'s `Display`, which supplies its
        // own `KiB` and renders a floor reading as a floor — so the literal must NOT repeat
        // the unit. It did, and the line read `196 KiB KiB -> 1908736 KiB` (the second a raw
        // byte count). A printed line nobody can parse is a measurement nobody can check.
        "  deploy composition: normalize {} B/level, par_drop {} B/level, \
         normalize_drop {} B/level ({} -> {}); band [{}, {}] KiB at 4,096",
        show(build.per_step()),
        show(destruct.per_step()),
        show(compose.per_step()),
        compose.lo_stack,
        compose.hi_stack,
        destruct.hi_stack.bytes_upper_bound("destruct") / 1024,
        (destruct.hi_stack.bytes_upper_bound("destruct")
            + build.hi_stack.bytes_upper_bound("build"))
            / 1024
    );
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
/// ★ **This test used to execute nothing.** Its whole body was a `println!`
/// reporting that no un-converted width-axis subject was in scope — green,
/// permanently, whatever the width-axis checker did. A `println!` cannot fail,
/// so "retained, named and wired" was a claim about the infrastructure that the
/// infrastructure never demonstrated.
///
/// It now carries a **synthetic control**: [`synthetic_sloped_body`], driven on
/// the width ladder, is Θ(width) by construction, and
/// [`assert_width_independent`]'s own checker must reject it. The real subjects
/// follow, so a checker that rejected everything fails too.
///
/// Why synthetic rather than a real subject: every width-axis member found so
/// far HAS been converted, and `FoldMatch::free_check` — the one Θ(width)
/// traversal left in the family — is in the matcher, measured by
/// `stack_depth_probe.rs`. "The defect is fixed, so it cannot be reproduced"
/// would retire this obligation permanently, and would retire it for every guard
/// in this file, since every one of them guards a fixed defect. Where the
/// excluded-class datum comes from is an implementation detail; that it is in
/// the class is the requirement.
#[test]
fn theta_width_tripwire() {
    // ── The control, on the SAME ladder `assert_width_independent` uses. ──
    let sloped = measure_ladder("synthetic_sloped", 4, 65_536);
    let flat = measure_ladder("synthetic_flat", 4, 65_536);

    // N2: the observations came from the real recursion.
    //
    // ⚠ This assertion is load-bearing on the width axis in a way it is not on
    // the depth axis. The whole reason this test exists is that `-O2` turns
    // `compare_score_nodes`' tail call into a loop and the measured slope goes to
    // 0 — "a codegen accident, not a property". If LLVM did the same to
    // `synthetic_recurse` the control would silently become a second flat
    // subject and every rejection below would be vacuous. `synthetic_recurse`
    // is written so it cannot (see its doc comment); this is the check that says
    // so out loud, in whichever profile the gate is run.
    assert!(
        sloped.per_step_of_control("sloped") >= SYNTHETIC_FRAME_BYTES,
        "the width control measured {} B/sibling against {SYNTHETIC_FRAME_BYTES} B of \
         ballast ({} KiB at width 4, {} KiB at width 65,536). Below the ballast means \
         the recursion became a loop — the exact codegen accident this axis exists to \
         distrust — and the rejections below would certify nothing.",
        sloped.per_step_of_control("sloped"),
        sloped.lo_stack,
        sloped.hi_stack
    );
    assert!(
        !runs_within(
            flat.hi_stack.bytes_upper_bound("flat"),
            65_536,
            "synthetic_sloped"
        ),
        "the two synthetic subjects must be separable on the width axis: \
         `synthetic_flat` needs {} KiB at width 65,536 and `synthetic_sloped` survived \
         the same stack",
        flat.hi_stack
    );

    // The fixed-stack half ACCEPTS it — 256 siblings of a small frame fit in
    // 1 MiB whatever the slope — which is why `assert_width_independent` also
    // runs `assert_no_slope`.
    for width in [4usize, 16, 64, 256] {
        assert!(
            runs_within(1024 * 1024, width, "synthetic_sloped"),
            "the fixed-stack half was expected to ACCEPT the Θ(width) control at width \
             {width}; that acceptance is the measurement justifying the zero-slope half"
        );
    }

    // The checker under test, run on data in the class it excludes.
    let why = zero_slope_verdict("synthetic_sloped", "width", sloped)
        .expect_err("the width checker must reject a subject that is Θ(width) by construction");
    assert!(
        why.contains("ZERO-SLOPE GATE FAILED"),
        "the rejection must come from the zero-slope clause; got: {why}"
    );
    assert!(
        why.contains("width axis"),
        "the rejection must name the WIDTH axis — a checker hardwired to `depth` would \
         pass every other assertion here; got: {why}"
    );
    assert!(
        clears_tolerance_by_an_order_of_magnitude(sloped),
        "the control's growth ({} KiB) must clear ZERO_SLOPE_TOLERANCE ({} KiB) by an \
         order of magnitude",
        sloped.growth_of_control("sloped") / 1024,
        ZERO_SLOPE_TOLERANCE / 1024
    );

    // Bidirectional: the same checker accepts the Θ(1) synthetic…
    zero_slope_verdict("synthetic_flat", "width", flat)
        .expect("the width checker must accept a subject that is Θ(1) by construction");

    // …and it accepts a REAL converted subject. `score_cmp_wide` is the
    // traversal the width axis was discovered on — `compare_score_nodes`'
    // sibling walk, Stage C-1 — so this is the checker that just rejected a
    // sloped subject accepting the very implementation it guards.
    assert_no_slope("score_cmp_wide", 4, 65_536, "width");

    // ── The real un-converted subjects. ──
    // Every WIDTH-axis member found so far has been converted (Stage C-1: the
    // score-tree sibling walk). The remaining Θ(width) traversal in the family
    // is `FoldMatch::free_check` (483 B per sibling, debug), which is in the
    // MATCHER rather than in the sorter or the substitution SCC; it is measured
    // by `stack_depth_probe.rs` (subject `free_check`) and is not on this
    // conversion's path.
    //
    // The list is empty and that is now an EXECUTED claim rather than a printed
    // one: adding a subject is one line, and the checker above it has been shown
    // to be able to refuse.
    const UNCONVERTED_WIDTH_SUBJECTS: &[(&str, usize, usize, usize)] = &[];
    for &(name, ceiling_bytes, lo, hi) in UNCONVERTED_WIDTH_SUBJECTS {
        assert_slope_below(name, ceiling_bytes, lo, hi);
    }

    // ── ★ Close the register loop on the width axis too; see the depth
    // tripwire's tail for why the register is checked against what RAN.
    let driven = SLOPE_SUBJECTS_DRIVEN.with(|s| s.borrow().clone());
    let mut driven_sorted = driven.clone();
    driven_sorted.sort();
    let mut expected: Vec<String> = TRIPWIRE_WIDTH.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(
        driven_sorted, expected,
        "THE SUBJECT REGISTER IS OUT OF DATE on the WIDTH axis. `theta_width_tripwire` \
         drove {:?} but `TRIPWIRE_WIDTH` lists {:?}.",
        driven, TRIPWIRE_WIDTH
    );

    println!(
        "  width axis: {} un-converted subject(s); control separates at {} B/sibling",
        UNCONVERTED_WIDTH_SUBJECTS.len(),
        sloped.per_step_of_control("sloped")
    );
}

// ---------------------------------------------------------------------------
// ★★ THE AUDIT IS DERIVED FROM THE GATE, AND THE DERIVATION IS CHECKED
// ---------------------------------------------------------------------------

/// Path of the audit, relative to this crate's manifest directory.
const AUDIT_PATH: &str = "../docs/design/audits/theta-depth-traversals-2026-07-26.md";

/// The fence that delimits the audit's ONE live status block.
const AUDIT_BLOCK_BEGIN: &str = "<!-- GATE-SUBJECTS:BEGIN";
const AUDIT_BLOCK_END: &str = "<!-- GATE-SUBJECTS:END -->";

/// Every key the block must carry. Needed as a set because a field runs until
/// the NEXT key, not until the next newline.
const AUDIT_KEYS: &[&str] = &[
    "converted-depth",
    "converted-width",
    "tripwire-depth",
    "tripwire-width",
    "totals",
];

/// Pull the comma-separated names of the audit block's `key:` field.
///
/// ⚠ **A field may WRAP.** The first draft of this function read one `line` and
/// stopped, and its first RED run under-reported a 9-name field as 5 — a
/// silent short read, indistinguishable from the document genuinely omitting
/// four names. A checker whose failure message misstates the evidence is worse
/// than no checker, and in the other direction (a continuation carrying an
/// EXTRA name) the short read would have let a disagreeing block PASS.
///
/// So a field runs from its own `key:` to the start of the next key in
/// [`AUDIT_KEYS`], or to the end of the block — line breaks inside it are
/// whitespace, exactly as a reader would take them.
fn audit_field(block: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key}:");
    let start = block.find(&prefix).unwrap_or_else(|| {
        panic!(
            "the audit's GATE-SUBJECTS block has no `{key}:` field. The block is derived from \
             the gate's subject register and must carry every key the gate publishes; see \
             `the_audit_agrees_with_the_gate`."
        )
    }) + prefix.len();
    let end = AUDIT_KEYS
        .iter()
        .filter(|k| **k != key)
        .filter_map(|k| block[start..].find(&format!("{k}:")).map(|i| start + i))
        .min()
        .unwrap_or(block.len());
    // ⚠ Backticks are stripped BEFORE whitespace is normalised, not after. The
    // last field runs to the end of the block, which includes the closing ```
    // fence, and trimming backticks afterwards left `"tripwired=9 "` — a value
    // that differs from `"tripwired=9"` only in a character no reader can see.
    block[start..end]
        .split(',')
        .map(|s| s.replace('`', " "))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Compare one key, with a message that says what to do about a mismatch.
fn assert_audit_key_agrees(block: &str, key: &str, gate: &[&str]) {
    let mut stated = audit_field(block, key);
    stated.sort();
    let mut expected: Vec<String> = gate.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    if stated == expected {
        return;
    }
    let missing: Vec<&String> = expected.iter().filter(|n| !stated.contains(n)).collect();
    let extra: Vec<&String> = stated.iter().filter(|n| !expected.contains(n)).collect();
    panic!(
        "★ THE AUDIT AND THE GATE DISAGREE about `{key}`.\n\
         \n\
         gate  ({} subjects): {:?}\n\
         audit ({} subjects): {:?}\n\
         \n\
         in the gate but NOT in the audit: {:?}\n\
         in the audit but NOT in the gate: {:?}\n\
         \n\
         The gate is the source of truth — it is executable, the prose is not. Update the\n\
         GATE-SUBJECTS block in {AUDIT_PATH} to match, in THIS commit. Do not adjust the\n\
         constants to match the document: a subject listed there is a test that runs here.",
        expected.len(),
        expected,
        stated.len(),
        stated,
        missing,
        extra
    );
}

/// ★★ **The audit's status lists are DERIVED from this gate, and this is the
/// derivation check.**
///
/// **The defect this replaces.** The audit transcribed the converted and
/// tripwired sets as prose, in four separate places, and every copy drifted:
/// §11.4 named **7** converted subjects, §12.6 and §12.9 named **13**, and the
/// gate carried **17**. §12.6 additionally listed `pretty` as "the next
/// conversion", tripwired — it had been converted in Stage D — and §12.9
/// asserted `pretty` / `pretty_wide` were "present but commented out", which
/// they were not. Both prose copies went stale within the hour of being
/// reconciled, twice. Reconciling by hand a third time buys another hour; the
/// only stable fix is to remove the second copy's authority.
///
/// **The mechanism.** The gate publishes [`CONVERTED_DEPTH`],
/// [`CONVERTED_WIDTH`], [`TRIPWIRE_DEPTH`] and [`TRIPWIRE_WIDTH`]; the audit
/// carries ONE generated block; this test asserts they are equal as sets. It
/// fails at the commit that separates them, which is the campaign's discipline
/// everywhere else — make the invariant fail where it is broken, not where it is
/// next inspected.
///
/// **What the audit keeps.** Method, dispositions, derivations, per-subject
/// numbers, and its commit-ANCHORED historical snapshots. Those snapshots
/// ("the family at `b9aaa3d4`") are evidence, not status: they are true of the
/// commit they name and rewriting them would falsify the record. This test
/// therefore checks exactly one block — the one that claims to describe *now* —
/// and the historical sections are explicitly out of its scope.
///
/// ★ **Shown RED.** Adding `par_drop` and `normalize_drop` to the gate without
/// touching the document made this test fail with both names under "in the gate
/// but NOT in the audit"; the block was then regenerated and it went green. That
/// is the whole point: the next subject somebody adds gets the same treatment
/// automatically.
#[test]
fn the_audit_agrees_with_the_gate() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(AUDIT_PATH);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read the audit at {}: {e}. The gate publishes the subject register that \
             document is generated from, so the check cannot be skipped when the file moves \
             — update AUDIT_PATH.",
            path.display()
        )
    });

    let start = text.find(AUDIT_BLOCK_BEGIN).unwrap_or_else(|| {
        panic!(
            "the audit at {} has no `{AUDIT_BLOCK_BEGIN}` fence. That block is the ONLY \
             place the document is allowed to state the current subject sets; prose \
             enumerations elsewhere must be anchored to a commit and read as history.",
            path.display()
        )
    });
    let end = text[start..]
        .find(AUDIT_BLOCK_END)
        .map(|i| start + i)
        .unwrap_or_else(|| panic!("the audit's GATE-SUBJECTS block is not terminated"));
    let block = &text[start..end];

    assert_audit_key_agrees(block, "converted-depth", CONVERTED_DEPTH);
    assert_audit_key_agrees(block, "converted-width", CONVERTED_WIDTH);
    assert_audit_key_agrees(block, "tripwire-depth", TRIPWIRE_DEPTH);
    assert_audit_key_agrees(block, "tripwire-width", TRIPWIRE_WIDTH);

    // ── The counts the audit states in prose must follow from the same lists,
    // because "13 subjects" in a heading is the form the drift actually took.
    let stated_total = audit_field(block, "totals");
    assert_eq!(
        stated_total,
        vec![
            format!(
                "converted={}",
                CONVERTED_DEPTH.len() + CONVERTED_WIDTH.len()
            ),
            format!("tripwired={}", TRIPWIRE_DEPTH.len() + TRIPWIRE_WIDTH.len()),
        ],
        "the audit's `totals:` line must be derived from the same register as its lists — \
         the drift this test replaces was as much in the COUNTS (\"13 subjects\") as in the \
         names"
    );

    println!(
        "  audit agrees with the gate: {} converted ({} depth + {} width), {} tripwired",
        CONVERTED_DEPTH.len() + CONVERTED_WIDTH.len(),
        CONVERTED_DEPTH.len(),
        CONVERTED_WIDTH.len(),
        TRIPWIRE_DEPTH.len() + TRIPWIRE_WIDTH.len()
    );
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
/// ★ All four of those have since been converted — `ParSortMatcher` and
/// `compare_score` in Stage C-2/C-1, `compare_score_nodes` in Stage C-1, and
/// `PrettyPrinter` in Stage D — and all four are now in
/// `converted_traversals_are_depth_independent`. The note is kept because the
/// *reasoning* is what matters: this assertion going green is a statement about
/// one reproducer, not about the family.
///
/// The definition of done for the FAMILY is
/// `converted_traversals_are_depth_independent` carrying every member at depth
/// **and width** 256, in **both** profiles — not this test.
/// ★★ **The 577-byte deploy that killed a release node.**
///
/// `[`×288 · `0` · `]`×288 — no `new`, no send, no user-defined process, one
/// nested list literal, and it fits in a single TCP segment. On the 2 MiB stack
/// a tokio worker gets when `RUST_MIN_STACK` is unset it aborted a **release**
/// node with `fatal runtime error: stack overflow`, and it was categorically
/// worse than every other member of its family:
///
/// * it fired in `InterpreterImpl::inj_attempt`'s FIRST phase
///   (`build-normalized-term`), **before** `set-initial-cost` establishes a
///   budget — so cost accounting could not bound it, not because the charge was
///   too small but because no charge existed yet;
/// * a stack overflow is a `SIGSEGV` on the guard page, not an `Err`, so the
///   call site's `Err(e) => handle_error(ParserError(..))` arm — which exists
///   precisely to turn a bad deploy into a *failed deploy* — never ran;
/// * `ReplayRuntimeOps::run_user_deploy → evaluate → inj_attempt` puts it on the
///   **validator** path, on source that arrived from the network, and producing
///   it requires no privilege and no stake.
///
/// Bisected max surviving source depth before the conversion: **287 release,
/// 45 debug**.
///
/// ★ **The depths below are NOT profile-dependent, and that is the whole
/// point.** Its neighbour `reported_reproducer_depth_survives_a_default_worker_stack`
/// asserts depth 10 in debug and 70 in release, because the traversal it guards
/// is still profile-sensitive at the margin. Here the profile split is exactly
/// the defect: 288 aborted release while debug died at 46, so a *depth guard*
/// could not have been given a single constant that was neither inert in
/// release nor newly restrictive in debug — which is why the traversal was
/// converted instead. A regression test that re-introduced a
/// `cfg!(debug_assertions)` branch would re-import that asymmetry into the very
/// artefact meant to exclude it. One list, both profiles, or the claim is not
/// the claim.
///
/// It asserts the deploy is *processed*, not merely that the process survives:
/// `normalize_body` checks that the input carries its bracket run **and** that
/// the normalized term carries the same `EList` nesting, so a normalizer that
/// rejected the input — which would also "not abort" — fails here rather than
/// passing quietly.
///
/// ⚠ This is a named regression, not the family's definition of done. That is
/// `converted_traversals_are_depth_independent`, which carries `normalize` at
/// parameter 4,096 with a flat minimum stack.
///
/// ⚠⚠ **AND ITS SCOPE IS NARROWER THAN ITS NAME.** It drives `normalize`, whose
/// body ends in `par_children::dismantle`. When this was written **no production
/// caller did that** — `Compiler::source_to_adt` returns the sorted `Par` by
/// value and every caller, `InterpreterImpl::inj_attempt` included, let it fall
/// out of scope through the derived recursive `Drop`. Measured 2026-07-27 on this
/// very fixture with `dismantle` replaced by `drop` (subject `normalize_drop`):
/// the same 2 MiB worker carries depth **4,414**, not 100,000, and depth 4,415
/// aborts with `fatal runtime error: stack overflow`.
///
/// ★ **One production caller does it now.** Deploy admission's
/// `interpreter_util::validate_deploy_term` (2026-07-28) is `source_to_adt` ▸
/// `dismantle` — this fixture's exact shape, on the gRPC intake path — so for
/// that caller the 100,000 above is no longer an artefact of the fixture. It
/// remains one for `inj_attempt`, which is why `normalize_drop` is still
/// tripwired and why the paragraph below still stands.
///
/// So this test certifies "the *normalizer* no longer aborts the node", which is
/// exactly what Stage G set out to do and exactly what it achieved. It does not
/// certify "the deploy no longer aborts the node", and the 100,000 in its own
/// loop is the distance between those two claims. The join is measured by
/// [`the_deploy_composition_is_bounded_below_by_its_destructor`] and tripwired
/// as `normalize_drop`; see [`normalize_drop_body`] for why the residue is a
/// destructor and not a traversal anybody wrote.
#[test]
fn the_577_byte_reproducer_is_a_deploy_and_not_a_node_abort() {
    const DEFAULT_SPAWNED_THREAD_STACK: usize = 2 * 1024 * 1024;
    // 288 is the FIRST depth that aborted a release node; 287 was the last that
    // survived. 1,152 is 4× that, and 100,000 is a 200 kB source — two orders of
    // magnitude past any ceiling either profile ever had.
    for depth in [288usize, 1_152, 100_000] {
        assert!(
            runs_within(DEFAULT_SPAWNED_THREAD_STACK, depth, "normalize"),
            "★ REGRESSION — THE REPRODUCER IS BACK. `[`×{depth} `0` `]`×{depth} \
             ({} bytes of source) no longer normalizes on the {} MiB stack a tokio \
             worker gets. Before the conversion this aborted the NODE — before \
             metering existed, through an error arm a SIGSEGV cannot reach, on the \
             validator path.",
            2 * depth + 1,
            DEFAULT_SPAWNED_THREAD_STACK / (1024 * 1024)
        );
    }
}

/// ★★ **The metering handshake no longer has a depth ceiling — stated as an
/// ABSOLUTE depth on the stack a tokio worker actually gets.**
///
/// [`converted_traversals_are_depth_independent`] proves `inj_attempt_clone`'s
/// minimum stack does not GROW across 4 → 4,096. That is the structural claim,
/// and it is the right one, but it is measured by bisecting a stack the gate
/// chooses. This test fixes the stack at the one value production runs on and
/// asserts the depths directly, so the claim can be read without reconstructing
/// a bisection: **288 / 1,152 / 100,000 all survive 2 MiB.**
///
/// ★ The explicit `stack_size` is what makes the number mean anything. An
/// ambient `RUST_MIN_STACK` or a future harness-default change must not move the
/// measurement. The repository intentionally carries no `RUST_MIN_STACK`
/// override; `runs_within` → `gate_child` still pins the production worker's
/// 2 MiB stack explicitly so the assertion remains meaningful.
///
/// **Why these three depths.** 288 is the first depth at which the 577-byte
/// reproducer aborted a release node. 1,152 is 4× that. 100,000 is a 200 kB
/// source, two orders of magnitude past every ceiling this composition ever had
/// — and it is far past the **729** that the pre-2026-07-27 form managed on this
/// exact stack (see [`inj_attempt_clone_body`] for the before/after bisection),
/// so a regression to the `.cloned()` form fails here at the very first depth.
///
/// ★ **Shown RED.** With the subject body reverted to
/// `source_process().cloned()` + `drop(signed_process)`, depth 288 still passed
/// (729 > 288) but depth 1,152 aborted the child with
/// `fatal runtime error: stack overflow` / status 134, and this test failed
/// naming `1152`. The 288 rung is deliberately kept anyway: it is the reproducer's
/// own depth, and a future regression cheaper than 7 KiB/level would be caught by
/// the 100,000 rung regardless.
///
/// ⚠ It asserts the handshake is **entered and completed**, not merely that the
/// process survives: `inj_attempt_clone_body` checks the source's bracket run,
/// the normalized term's nesting, AND the nesting of the term
/// `into_source_process` hands back. A handshake that returned the wrong arm
/// would also "not abort", and would fail here rather than pass quietly.
#[test]
fn the_metering_handshake_carries_any_depth_on_a_default_worker_stack() {
    const DEFAULT_SPAWNED_THREAD_STACK: usize = 2 * 1024 * 1024;
    for depth in [288usize, 1_152, 100_000] {
        assert!(
            runs_within(DEFAULT_SPAWNED_THREAD_STACK, depth, "inj_attempt_clone"),
            "★ REGRESSION — THE `set-initial-cost` HANDSHAKE HAS A DEPTH CEILING AGAIN. \
             A deploy of nesting depth {depth} no longer survives the metering handshake on \
             the {} MiB stack a tokio worker gets. The phase must hand the normalized term \
             to `reducer.inj` BY MOVE (`SignedProcess::into_source_process`); the \
             `source_process().cloned()` form it replaced cost a `<Par as Clone>::clone` \
             AND a recursive `drop_in_place::<Par>` of the original, and stopped at depth \
             729 on this very stack. A stack overflow is a SIGSEGV, so the call site's \
             `Err(e) => handle_error(ParserError(..))` arm cannot see it: it takes the \
             node, not the deploy.",
            DEFAULT_SPAWNED_THREAD_STACK / (1024 * 1024)
        );
    }
}

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
