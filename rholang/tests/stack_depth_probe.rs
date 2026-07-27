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

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
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
            use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
            use models::rust::rholang::sorter::sortable::Sortable;
            let t = nested_list(depth);
            let out = ParSortMatcher::sort_match(&t);
            std::mem::forget(out);
            std::mem::forget(t);
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
