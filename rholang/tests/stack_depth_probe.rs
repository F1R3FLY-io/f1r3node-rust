//! # Θ(depth) stack-consumption probe (the FALSIFICATION experiment)
//!
//! This is a *measurement* harness, not an assertion harness. It answers one
//! question per invocation:
//!
//!   > Does traversal `T`, applied to a term of nesting depth `N`, survive a
//!   > thread whose stack is exactly `S` bytes?
//!
//! The caller (`scripts/stack_depth_probe.sh`) bisects `S` for a ladder of `N`
//! and fits `S(N) = a + b·N`. A materially non-zero `b` proves the traversal is
//! Θ(depth) in native stack; `b ≈ 0` proves it is heap-bounded.
//!
//! ## Why a separate process per (T, N, S)
//!
//! A stack overflow is a `SIGSEGV` caught by the runtime's guard-page handler,
//! which prints and `abort()`s. It is NOT unwindable, so a bisection cannot run
//! inside one process. The driver therefore re-execs this binary once per probe
//! point and reads the exit status: 0 = survived, non-zero = overflowed.
//!
//! ## Isolation discipline (this is what makes the numbers per-traversal)
//!
//! * The term is BUILT bottom-up with an explicit loop — O(1) native stack — so
//!   construction never contributes to the measured bound.
//! * Everything the probe touches is `std::mem::forget`-ed at the end, because
//!   `Drop` for the `Par`/`Expr`/`ExprInstance` family is ITSELF a Θ(depth)
//!   recursive traversal and would otherwise contaminate every other reading.
//!   `drop` is measured as its own probe (`PROBE_WHAT=drop`).
//! * The probe body runs on a thread created with an explicit `stack_size`, so
//!   neither `RUST_MIN_STACK` nor `ulimit -s` is in play.
//!
//! ## Invocation
//!
//! ```text
//! PROBE_WHAT=subst PROBE_DEPTH=40 PROBE_STACK=8388608 \
//!   cargo test -p rholang --test stack_depth_probe -- --ignored --exact probe
//! ```

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Connective, ConnectiveBody, EList, Expr, New, Par, Receive, ReceiveBind};
use models::rust::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::score_tree::{ScoreAtom, ScoredTerm, Tree};
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gint_par;
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::matcher::has_locally_free::HasLocallyFree;
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

// ---------------------------------------------------------------------------
// ⚠ Some probe SETUPS are themselves Θ(depth)
//
// `bincode::serialize`, `sort_match` and `Env::put` all recurse with the term,
// so performing them on the bisected probe thread would make the reading
// `max(setup, subject)` rather than the subject. Running the setup on a stack
// that is never the constraint restores the isolation discipline the module
// documentation promises: ONE traversal per number.
// ---------------------------------------------------------------------------

/// Run `f` on a thread whose stack is large enough never to bind, and hand back
/// its result. 1 GiB is address space, not resident memory.
fn on_a_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(1024 * 1024 * 1024)
        .name("probe-setup".to_string())
        .spawn(f)
        .expect("stack_depth_probe: failed to spawn the setup thread")
        .join()
        .expect("stack_depth_probe: setup thread panicked")
}

// ---------------------------------------------------------------------------
// term builders — all ITERATIVE (bottom-up), so O(1) native stack.
// ---------------------------------------------------------------------------

fn expr_par(ei: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ei),
        }],
        ..Default::default()
    }
}

/// `elist` — the same helper the eval-SCC differential harness uses
/// (`reduce.rs`, `mod differential_trampoline`).
fn elist(ps: Vec<Par>) -> Par {
    expr_par(ExprInstance::EListBody(EList {
        ps,
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    }))
}

/// `[[[…[0]…]]]` with `depth` bracket levels: exactly the shape that takes the
/// reducer down at depth 10 (`@"OUT"!([[[[[[[[[[0]]]]]]]]]])`).
fn nested_list(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = elist(vec![p]);
    }
    p
}

/// The same chain, lifted out of its enclosing `Par` so two of them can be made
/// siblings of one `Par`. `leaf` distinguishes two otherwise identical chains,
/// which is what forces the score comparator to descend all the way down.
fn nested_list_expr(depth: usize, leaf: i64) -> Expr {
    let mut p = new_gint_par(leaf, vec![], false);
    for _ in 0..depth {
        p = elist(vec![p]);
    }
    p.exprs
        .pop()
        .expect("stack_depth_probe: nested_list_expr built a Par with no exprs")
}

/// A single `[0, 1, …, width-1]` — one bracket level, `width` siblings. The
/// WIDTH axis: `compare_score_nodes` recurses on the list TAIL, so its native
/// stack grows with sibling count, independently of nesting depth.
fn wide_list_expr(width: usize) -> Expr {
    let mut ps = Vec::with_capacity(width);
    for i in 0..width {
        ps.push(new_gint_par(i as i64, vec![], false));
    }
    elist(ps)
        .exprs
        .pop()
        .expect("stack_depth_probe: wide_list_expr built a Par with no exprs")
}

/// `new x0 in { for (_ <- @0) { new x1 in { for (_ <- @0) { … 0 … } } } }` with
/// `depth` alternating binder scopes.
///
/// This is the shape a pure `EList` chain cannot reach: every `New` and every
/// `Receive` level makes `substitute` build `env.shift(bind_count)`, which is
/// `Env { shift: self.shift + j, ..(*self).clone() }` — a full
/// `HashMap<i32, Par>` clone, hence a DEEP clone of every bound value, once per
/// level.
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
        let inner = Par {
            receives: vec![receive],
            ..Default::default()
        };
        let new_scope = New {
            bind_count: 1,
            p: Some(inner),
            uri: vec![],
            injections: BTreeMap::new(),
            locally_free: vec![],
        };
        p = Par {
            news: vec![new_scope],
            ..Default::default()
        };
    }
    p
}

/// `{~{~{…}}}`-style nesting through `Connective`, which is the ONLY recursion
/// path `ParCount::min_max_par` follows (it descends `par.connectives`, never
/// `par.exprs`). A nested `EList` chain leaves it at depth 1, which is why it
/// needs a shape of its own.
fn nested_conn_and(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = Par {
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![p],
                })),
            }],
            connective_used: true,
            ..Default::default()
        };
    }
    p
}

/// `[0, 1, …, width-1]` as a whole `Par` — one bracket level, `width`
/// siblings. The WIDTH counterpart of [`nested_list`].
fn wide_list(width: usize) -> Par {
    let mut ps = Vec::with_capacity(width);
    for i in 0..width {
        ps.push(new_gint_par(i as i64, vec![], false));
    }
    elist(ps)
}

/// A `Par` carrying TWO `width`-wide lists, so that the sorter has two
/// siblings to order and its comparator has `width` score-tree children to
/// walk. The width analogue of `nested_list` for the sorter family.
fn wide_pair(width: usize) -> Par {
    Par {
        exprs: vec![wide_list_expr(width), wide_list_expr(width)],
        ..Default::default()
    }
}

/// `!(!(…(!true)…))` with `depth` negations — the shape that drives
/// `rho_pure_eval::eval_with`'s OWN recursion. A nested `EList` chain does
/// not: the `EListBody` arm returns `par_with_expr(expr.clone())` without
/// descending, so on that shape `eval_with` measures `<Par as Clone>::clone`
/// and nothing else.
fn nested_nots(depth: usize) -> Par {
    let mut p = expr_par(ExprInstance::GBool(true));
    for _ in 0..depth {
        p = expr_par(ExprInstance::ENotBody(models::rhoapi::ENot { p: Some(p) }));
    }
    p
}

/// `{{{…{0}…}}}` — `depth` nested `ESet`s.
///
/// ⚠ THE NAMED RESIDUAL OF LEG-2 STAGE C-2. The `ESetBody` arm of the sorter
/// is deliberately NOT worklisted: it routes its elements through
/// `SortedParHashSet`, which re-enters `ParSortMatcher::sort_match` on OWNED
/// intermediates (the deduplicated set) rather than on sub-terms of the input.
/// Each re-entry is its own bounded drive, so a chain of `n` nested sets costs
/// `n` drive frames rather than `n` sorter frames — and, underneath that,
/// `HashSet<Par>` invokes the DERIVED `Par: Clone + Hash + Eq`, each of which
/// is Θ(depth) in its own right (audit row 5, disposition "Leg-1 only: remove
/// the call sites, not the impl"). No conversion of the SORTER can remove
/// those. This probe measures what is left instead of assuming it away.
fn nested_sets(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = expr_par(ExprInstance::ESetBody(models::rhoapi::ESet {
            ps: vec![p],
            locally_free: vec![],
            connective_used: false,
            remainder: None,
        }));
    }
    p
}

/// The `EMap` counterpart of [`nested_sets`], for the same reason.
fn nested_maps(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = expr_par(ExprInstance::EMapBody(models::rhoapi::EMap {
            kvs: vec![models::rhoapi::KeyValuePair {
                key: Some(p),
                value: Some(new_gint_par(1, vec![], false)),
            }],
            locally_free: vec![],
            connective_used: false,
            remainder: None,
        }));
    }
    p
}

/// Rholang source text `[[[…[0]…]]]` with `depth` bracket levels, built
/// iteratively. Drives the normalizer, whose recursion follows *source*
/// nesting rather than `Par` nesting.
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

/// The score tree of a depth-`depth` chain, with the terms discarded so that a
/// subsequent `drop`/`clone`/`==` measures `Tree<ScoreAtom>` and nothing else.
fn score_of(depth: usize) -> Tree<ScoreAtom> {
    let t = nested_list(depth);
    let ScoredTerm { term, score } = ParSortMatcher::sort_match(&t);
    std::mem::forget(term);
    std::mem::forget(t);
    score
}

// ---------------------------------------------------------------------------
// probes
// ---------------------------------------------------------------------------

fn substitute_instance() -> Substitute {
    // A budget large enough that no probe ever runs out — we are measuring
    // stack, not cost.
    let cost = Cost::create(i64::MAX / 4, "stack_depth_probe".to_string());
    Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(cost)),
    }
}

/// Run one probe. Everything it produces is leaked on purpose (see module doc).
fn run_probe(what: &str, depth: usize) {
    let env: Env<Par> = Env::new();
    match what {
        // ---- f1r3node substitution SCC ----
        "subst" => {
            let t = nested_list(depth);
            let s = substitute_instance();
            let out = s
                .substitute(t, 0, &env)
                .expect("stack_depth_probe: substitute failed");
            std::mem::forget(out);
        }
        "subst_no_sort" => {
            let t = nested_list(depth);
            let s = substitute_instance();
            let out = s
                .substitute_no_sort(t, 0, &env)
                .expect("stack_depth_probe: substitute_no_sort failed");
            std::mem::forget(out);
        }
        "subst_and_charge" => {
            let t = nested_list(depth);
            let s = substitute_instance();
            let out = s
                .substitute_and_charge(&t, 0, &env)
                .expect("stack_depth_probe: substitute_and_charge failed");
            std::mem::forget(out);
            std::mem::forget(t);
        }

        // ---- the sorter family ----
        "sort" => {
            let t = nested_list(depth);
            let out = ParSortMatcher::sort_match(&t);
            std::mem::forget(out);
            std::mem::forget(t);
        }

        "sort_nested_set" => {
            let t = nested_sets(depth);
            let out = ParSortMatcher::sort_match(&t);
            std::mem::forget(out);
            std::mem::forget(t);
        }
        "sort_nested_map" => {
            let t = nested_maps(depth);
            let out = ParSortMatcher::sort_match(&t);
            std::mem::forget(out);
            std::mem::forget(t);
        }
        "clone_nested_set" => {
            // The CONTROL for `sort_nested_set`: the derived `<Par as Clone>`
            // over the same shape. If the two agree, what is left in the set
            // arm is the derived-traversal class and not the sorter.
            let t = nested_sets(depth);
            let c = t.clone();
            std::mem::forget(c);
            std::mem::forget(t);
        }

        // ---- WIDTH counterparts (`PROBE_DEPTH` carries W) ----
        "subst_wide" => {
            let t = wide_list(depth);
            let s = substitute_instance();
            let out = s
                .substitute(t, 0, &env)
                .expect("stack_depth_probe: subst_wide failed");
            std::mem::forget(out);
        }
        "sort_wide" => {
            let t = wide_pair(depth);
            let out = ParSortMatcher::sort_match(&t);
            std::mem::forget(out);
            std::mem::forget(t);
        }
        "pretty_wide" => {
            let t = wide_list(depth);
            let mut pp = PrettyPrinter::new();
            let s = pp.build_string_from_message(&t);
            std::mem::forget(s);
            std::mem::forget(t);
        }

        // ---- the SCORE TREE: four traversals the proto-level enumeration
        //      could not see, because `Tree<T>` is a recursive RUST type and
        //      not a proto message. `score_tree.rs:24`.
        //
        //      ⚠ The score trees are built on a big stack — `sort_match` is
        //      itself Θ(depth), so building them here would report
        //      `max(sort_match, comparator)`.
        "score_cmp" => {
            // DEPTH axis. Two chains that differ ONLY at the leaf, so
            // `compare_score` cannot decide until it has descended every level.
            //
            // ⚠ This is the shape the GATE's own probe does not have: a linear
            // chain sorts as a ONE-element vector, and `Vec::sort_by` on one
            // element performs ZERO comparisons — so `compare_score` is never
            // entered and a gate built on `nested_list` alone would pass with
            // the comparator untouched.
            let mut scored = on_a_big_stack(move || {
                vec![
                    ExprSortMatcher::sort_match(&nested_list_expr(depth, 1)),
                    ExprSortMatcher::sort_match(&nested_list_expr(depth, 0)),
                ]
            });
            ScoredTerm::sort_vec(&mut scored);
            std::mem::forget(scored);
        }
        "score_cmp_wide" => {
            // WIDTH axis (`PROBE_DEPTH` is the sibling count W here). Two
            // IDENTICAL lists: every head pair compares `Equal`, so
            // `compare_score_nodes` must recurse over the whole tail.
            let width = depth;
            let mut scored = on_a_big_stack(move || {
                vec![
                    ExprSortMatcher::sort_match(&wide_list_expr(width)),
                    ExprSortMatcher::sort_match(&wide_list_expr(width)),
                ]
            });
            ScoredTerm::sort_vec(&mut scored);
            std::mem::forget(scored);
        }
        "tree_drop" => {
            let score = on_a_big_stack(move || score_of(depth));
            drop(score);
        }
        "tree_clone" => {
            let score = on_a_big_stack(move || score_of(depth));
            let c = score.clone();
            std::mem::forget(c);
            std::mem::forget(score);
        }
        "tree_eq" => {
            let (a, b) = on_a_big_stack(move || (score_of(depth), score_of(depth)));
            assert!(a == b, "stack_depth_probe: tree_eq built unequal scores");
            std::mem::forget(a);
            std::mem::forget(b);
        }

        // ---- derived Clone / Drop / PartialEq for the recursive prost types ----
        "clone" => {
            let t = nested_list(depth);
            let c = t.clone();
            std::mem::forget(c);
            std::mem::forget(t);
        }
        "drop" => {
            let t = nested_list(depth);
            drop(t);
        }
        "eq" => {
            let t = nested_list(depth);
            let u = nested_list(depth);
            assert!(t == u, "stack_depth_probe: eq probe built unequal terms");
            std::mem::forget(t);
            std::mem::forget(u);
        }
        "debug" => {
            let t = nested_list(depth);
            let s = format!("{:?}", t);
            std::mem::forget(s);
            std::mem::forget(t);
        }

        // ---- prost wire codec ----
        "encoded_len" => {
            let t = nested_list(depth);
            let n = t.encoded_len();
            assert!(n > 0);
            std::mem::forget(t);
        }
        "encode" => {
            let t = nested_list(depth);
            let bytes = t.encode_to_vec();
            std::mem::forget(bytes);
            std::mem::forget(t);
        }
        "decode" => {
            // `encode` is itself Θ(depth), so encoding the probe input would make
            // this measurement the MINIMUM of the two traversals.  Synthesise the
            // wire bytes ITERATIVELY instead, so the probe isolates the DECODER.
            //
            // Field numbers are from `models/src/main/protobuf/RhoTypes.proto`:
            //   Par.exprs         = 5   -> key (5<<3)|2  = 42   -> 0x2a
            //   Expr.e_list_body  = 20  -> key (20<<3)|2 = 162  -> 0xa2 0x01
            //   EList.ps          = 1   -> key (1<<3)|2  = 10   -> 0x0a
            // (A wrong key makes prost SKIP the field as unknown and the probe
            // silently measures nothing — this arm is therefore depth-checked
            // below.)
            let mut bytes = new_gint_par(0, vec![], false).encode_to_vec();
            for _ in 0..depth {
                bytes = wrap_len_delim(&[0x0a], bytes); // EList.ps         (field 1)
                bytes = wrap_len_delim(&[0xa2, 0x01], bytes); // Expr.e_list_body (field 20)
                bytes = wrap_len_delim(&[0x2a], bytes); // Par.exprs        (field 5)
            }
            let p = Par::decode(bytes.as_slice()).expect("stack_depth_probe: decode failed");
            assert_eq!(
                par_depth(&p),
                depth,
                "stack_depth_probe: decode probe did not reconstruct the nesting \
                 (wrong proto field number?) — the reading would be meaningless"
            );
            std::mem::forget(p);
            std::mem::forget(bytes);
        }

        // ---- the locally-free reader ----
        "locally_free" => {
            let t = nested_list(depth);
            let lf = t.locally_free(t.clone(), 0);
            std::mem::forget(lf);
            std::mem::forget(t);
        }
        "connective_used" => {
            let t = nested_list(depth);
            let cu = t.connective_used(t.clone());
            assert!(!cu);
            std::mem::forget(t);
        }

        // ---- the spatial matcher ----
        "spatial" => {
            let t = nested_list(depth);
            let p = nested_list(depth);
            let mut ctx = SpatialMatcherContext::new();
            let r = ctx.spatial_match_result(t, p);
            assert!(
                r.is_some(),
                "stack_depth_probe: spatial probe did not match"
            );
            std::mem::forget(ctx);
        }

        // ---- the pretty printer ----
        "pretty" => {
            let t = nested_list(depth);
            let mut pp = PrettyPrinter::new();
            let s = pp.build_string_from_message(&t);
            std::mem::forget(s);
            std::mem::forget(t);
        }

        // ---- F2: substitution through BINDER scopes, under a populated env ----
        //
        // `nested_list` never populates the environment and never shifts it, so
        // it cannot see `Env::shift`'s `..(*self).clone()`. These two arms
        // differ ONLY in how deep the bound value is, so their difference
        // isolates the environment clone from the binder recursion itself.
        "subst_binders" => {
            let env = on_a_big_stack(move || {
                let mut base: Env<Par> = Env::new();
                base.put(nested_list(depth))
            });
            let t = nested_binders(depth);
            let s = substitute_instance();
            let out = s
                .substitute(t, 0, &env)
                .expect("stack_depth_probe: subst_binders failed");
            std::mem::forget(out);
            std::mem::forget(env);
        }
        "subst_binders_no_sort" => {
            // The DRIVER alone: `substitute` ends in one `sort_match`, which is
            // still Θ(depth) until Stage C, so the sorted entry point cannot
            // attribute a residual to the driver.
            let env = on_a_big_stack(move || {
                let mut base: Env<Par> = Env::new();
                base.put(nested_list(depth))
            });
            let t = nested_binders(depth);
            let s = substitute_instance();
            let out = s
                .substitute_no_sort(t, 0, &env)
                .expect("stack_depth_probe: subst_binders_no_sort failed");
            std::mem::forget(out);
            std::mem::forget(env);
        }
        "subst_deep_binding" => {
            // ⚠ The named RESIDUAL. `Env::get` returns its value CLONED, because
            // substituting a `BoundVar` splices the bound term into the result.
            // That clone is `<Par as Clone>::clone` — a derived impl (audit row
            // 5) whose disposition is Leg-1 only — and it is Θ(depth of the
            // BOUND VALUE), not of the term being traversed. Identical in the
            // recursive form; this arm exists so the residual is measured
            // rather than assumed.
            let env = on_a_big_stack(move || {
                let mut base: Env<Par> = Env::new();
                base.put(nested_list(depth))
            });
            let t = expr_par(ExprInstance::EVarBody(models::rhoapi::EVar {
                v: Some(models::rhoapi::Var {
                    var_instance: Some(models::rhoapi::var::VarInstance::BoundVar(0)),
                }),
            }));
            let s = substitute_instance();
            let out = s
                .substitute_no_sort(t, 0, &env)
                .expect("stack_depth_probe: subst_deep_binding failed");
            std::mem::forget(out);
            std::mem::forget(env);
        }
        "subst_binders_ground_env" => {
            let mut base: Env<Par> = Env::new();
            let env = base.put(new_gint_par(7, vec![], false));
            let t = nested_binders(depth);
            let s = substitute_instance();
            let out = s
                .substitute(t, 0, &env)
                .expect("stack_depth_probe: subst_binders_ground_env failed");
            std::mem::forget(out);
            std::mem::forget(env);
        }

        // ---- F3: the OTHER codec, and the two `Par`-as-key traversals ----
        //
        // `models/build.rs` attaches `serde::Serialize`/`Deserialize` to every
        // `.rhoapi` message, and RSpace serialises datums/continuations with
        // bincode 1.3.3 — which, unlike `prost`, has NO recursion limit.
        "bincode_ser" => {
            let t = nested_list(depth);
            let bytes = bincode::serialize(&t).expect("stack_depth_probe: bincode_ser failed");
            assert!(!bytes.is_empty());
            std::mem::forget(bytes);
            std::mem::forget(t);
        }
        // ★ CONVERTED (Stage F). `bincode_de` now measures what the node
        // actually runs on the cold-store read path: `Par::cold_decode`
        // (`models/src/rust/rholang/par_codec.rs`). Leaving this arm on
        // `bincode::deserialize` would have been a quiet trap — a later
        // re-measurement would report the PRE-conversion 28,362 B/level and
        // read as "nothing changed".
        //
        // `bincode_de_derived` retains the old body as the CONTROL, so the
        // before/after comparison stays available in one run instead of
        // requiring a checkout of an older commit.
        "bincode_de" | "bincode_de_derived" => {
            use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
            // Encode on a stack that never binds, so this arm isolates the
            // DECODER (the same discipline the `decode` arm above uses). It
            // matters twice as much now: the ENCODER is still Θ(depth), so an
            // un-isolated probe would report the encoder's slope.
            let bytes = on_a_big_stack(move || {
                let t = nested_list(depth);
                let b = bincode::serialize(&t).expect("stack_depth_probe: bincode encode failed");
                std::mem::forget(t);
                b
            });
            let p: Par = if what == "bincode_de_derived" {
                bincode::deserialize(&bytes).expect("stack_depth_probe: bincode_de failed")
            } else {
                Par::cold_decode(&bytes).expect("stack_depth_probe: bincode_de failed")
            };
            assert_eq!(
                par_depth(&p),
                depth,
                "stack_depth_probe: bincode_de did not reconstruct the nesting — \
                 the reading would be meaningless"
            );
            std::mem::forget(p);
            std::mem::forget(bytes);
        }
        "par_ord" => {
            // `models/build.rs` derives `Ord`/`PartialOrd` for every `.rhoapi`
            // message; `SortedParMap`/`SortedParHashSet` order `Par` keys.
            let t = nested_list(depth);
            let u = nested_list(depth);
            assert_eq!(
                t.cmp(&u),
                std::cmp::Ordering::Equal,
                "stack_depth_probe: par_ord built unequal terms"
            );
            std::mem::forget(t);
            std::mem::forget(u);
        }
        "par_hash" => {
            // `Par` is a `HashSet`/`HashMap` key (`SortedParHashSet`,
            // `SortedParMap`, `Ordering::sort_map`).
            let t = nested_list(depth);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.hash(&mut h);
            assert!(h.finish() != 0 || depth == usize::MAX);
            std::mem::forget(t);
        }

        // ---- the audit's "not separately tabulated, same class" list ----
        "par_to_sexpr" => {
            use models::rust::par_to_sexpr::ParToSExpr;
            let t = nested_list(depth);
            let s = ParToSExpr::par_to_sexpr(&t);
            assert!(!s.is_empty());
            std::mem::forget(s);
            std::mem::forget(t);
        }
        "min_max_par" => {
            use rholang::rust::interpreter::matcher::par_count::ParCount;
            // Recursion here follows `connectives`, never `exprs`.
            let t = nested_conn_and(depth);
            let pc = ParCount::new(&Par::default());
            let (lo, hi) = pc.min_max_par(t);
            std::mem::forget(lo);
            std::mem::forget(hi);
        }
        "free_check" => {
            // WIDTH axis: `free_check` recurses on the slice TAIL, so its
            // native stack grows with the surplus-target count, not with depth.
            use rholang::rust::interpreter::matcher::fold_match::FoldMatch;
            let width = depth;
            let mut trem = Vec::with_capacity(width);
            for i in 0..width {
                trem.push(new_gint_par(i as i64, vec![], false));
            }
            let ctx = SpatialMatcherContext::new();
            let out = FoldMatch::<Par, Par>::free_check(&ctx, &trem, 0, Vec::new())
                .expect("stack_depth_probe: free_check rejected a locally-free-empty list");
            assert_eq!(out.len(), width);
            std::mem::forget(out);
            std::mem::forget(trem);
            std::mem::forget(ctx);
        }
        "sorted_par_hash_set" => {
            use models::rust::sorted_par_hash_set::SortedParHashSet;
            let t = nested_list(depth);
            let mut s = SortedParHashSet::create_from_empty();
            let out = s.insert(t);
            std::mem::forget(out);
            std::mem::forget(s);
        }
        "sorted_par_map" => {
            use models::rust::sorted_par_map::SortedParMap;
            let k = nested_list(depth);
            let v = nested_list(depth);
            let mut m = SortedParMap::create_from_empty();
            let out = m.insert((k, v));
            std::mem::forget(out);
            std::mem::forget(m);
        }
        "make_mut" => {
            // `SharedPars::make_mut` is `Arc::make_mut`: a `Vec<Par>` deep
            // clone whenever the payload is shared.
            use models::rust::rhoapi_ext::SharedPars;
            let t = nested_list(depth);
            let mut a = SharedPars::from(vec![t]);
            let b = a.clone();
            a.make_mut().push(new_gint_par(1, vec![], false));
            std::mem::forget(a);
            std::mem::forget(b);
        }
        "par_to_path" => {
            // The recursive half of `pathmap_zipper::descend_to` — the zipper
            // itself only walks the flattened byte key.
            use models::rust::pathmap_integration::par_to_path;
            let t = nested_list(depth);
            let segs = par_to_path(&t);
            std::mem::forget(segs);
            std::mem::forget(t);
        }
        "eval_with" => {
            use rho_pure_eval::{eval_with, NoSpatialMatch};
            let t = nested_list(depth);
            let e: Env<Par> = Env::new();
            let out = eval_with(&t, &e, &NoSpatialMatch)
                .expect("stack_depth_probe: eval_with failed");
            std::mem::forget(out);
            std::mem::forget(t);
        }
        "eval_with_nots" => {
            // `eval_with`'s OWN recursion, which follows EXPRESSION nesting.
            // Contrast with the `eval_with` arm, whose reading on an `EList`
            // chain is `<Par as Clone>::clone` and not a recursion at all.
            use rho_pure_eval::{eval_with, NoSpatialMatch};
            let t = nested_nots(depth);
            let e: Env<Par> = Env::new();
            let out = eval_with(&t, &e, &NoSpatialMatch)
                .expect("stack_depth_probe: eval_with_nots failed");
            std::mem::forget(out);
            std::mem::forget(t);
        }
        "normalize" => {
            // The NORMALIZER — Θ(*source* nesting), on the deploy path, and the
            // only member reachable before a term exists at all.
            use rholang::rust::interpreter::compiler::compiler::Compiler;
            let src = nested_list_source(depth);
            let out = Compiler::source_to_adt(&src).expect("stack_depth_probe: normalize failed");
            std::mem::forget(out);
            std::mem::forget(src);
        }

        other => panic!("stack_depth_probe: unknown PROBE_WHAT={:?}", other),
    }
}

/// Count `[[…[x]…]]` nesting levels ITERATIVELY. Used to prove a probe input
/// actually has the nesting the probe claims (see the `decode` arm).
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

/// Prefix `body` with `tag` + its varint length. Iterative by construction.
fn wrap_len_delim(tag: &[u8], body: Vec<u8>) -> Vec<u8> {
    let mut len_buf = Vec::with_capacity(10);
    let mut n = body.len() as u64;
    loop {
        let mut b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            b |= 0x80;
        }
        len_buf.push(b);
        if n == 0 {
            break;
        }
    }
    let mut out = Vec::with_capacity(tag.len() + len_buf.len() + body.len());
    out.extend_from_slice(tag);
    out.extend_from_slice(&len_buf);
    out.extend_from_slice(&body);
    out
}

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// Not part of CI: `#[ignore]`d, env-driven, and it deliberately crashes the
/// process when the bound is exceeded. The regression GATE that *is* part of CI
/// lives in `rholang/tests/stack_depth_gate.rs`.
#[test]
#[ignore = "measurement probe: driven by scripts/stack_depth_probe.sh"]
fn probe() {
    let what = std::env::var("PROBE_WHAT").unwrap_or_else(|_| "subst".to_string());
    let depth: usize = std::env::var("PROBE_DEPTH")
        .unwrap_or_else(|_| "10".to_string())
        .parse()
        .expect("stack_depth_probe: PROBE_DEPTH must be a non-negative integer");
    let stack: usize = std::env::var("PROBE_STACK")
        .unwrap_or_else(|_| "8388608".to_string())
        .parse()
        .expect("stack_depth_probe: PROBE_STACK must be a non-negative integer");

    let h = std::thread::Builder::new()
        .stack_size(stack)
        .name("probe".to_string())
        .spawn(move || run_probe(&what, depth))
        .expect("stack_depth_probe: failed to spawn probe thread");

    h.join().expect("stack_depth_probe: probe thread panicked");
    println!("PROBE OK depth={} stack={}", depth, stack);
}

/// Type-size census. Frame size is dominated by the by-value types a traversal
/// moves through it; this prints them so the 190 KiB/level figure can be
/// attributed rather than guessed at.
#[test]
fn type_sizes() {
    use std::mem::size_of;
    macro_rules! p {
        ($t:ty) => {
            println!("{:>10}  {}", size_of::<$t>(), stringify!($t));
        };
    }
    p!(Par);
    p!(Expr);
    p!(ExprInstance);
    p!(EList);
    p!(models::rhoapi::EMethod);
    p!(models::rhoapi::Connective);
    p!(models::rhoapi::connective::ConnectiveInstance);
    p!(models::rhoapi::Send);
    p!(models::rhoapi::Receive);
    p!(models::rhoapi::New);
    p!(models::rhoapi::Match);
    p!(models::rhoapi::Bundle);
    p!(models::rhoapi::If);
    p!(rholang::rust::interpreter::errors::InterpreterError);
    p!(Result<Par, rholang::rust::interpreter::errors::InterpreterError>);
    p!(Result<Expr, rholang::rust::interpreter::errors::InterpreterError>);
    p!(models::rust::rholang::sorter::score_tree::ScoredTerm<Par>);
    p!(
        models::rust::rholang::sorter::score_tree::Tree<
            models::rust::rholang::sorter::score_tree::ScoreAtom,
        >
    );
    p!(models::rust::rholang::sorter::score_tree::ScoreAtom);
}
