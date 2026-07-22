//! Stack-overflow crossover probe for the expression evaluator.
//!
//! Builds a DEEPLY NESTED term and evaluates it via the SYNC `eval_expr` on a
//! dedicated OS thread with the DEFAULT 8 MB stack (no RUST_MIN_STACK override).
//! One depth per process, so a stack overflow (SIGABRT via the guard-page
//! handler) only kills THIS process — a driver loop bisects the crossover depth.
//!
//! Two term families (each exercises a different clone site in reduce.rs):
//!   * `plus`  : t_{k+1} = EPlus(t_k, 1)   — per-operand `p1.clone()` per level.
//!   * `list`  : u_{k+1} = [u_k]           — `expr_instance.clone()`@eval_expr_to_par per level.
//!
//! Usage:  so_probe <plus|list> <depth>
//! Exit:   0 = evaluated OK, 2 = InterpreterError, 3 = ordinary panic.
//!         A stack overflow aborts the process (exit 134 = 128 + SIGABRT).
//! Both terms are built ITERATIVELY (no build-time / drop-time deep recursion),
//! so the ONLY deep recursion measured is the evaluator's own call stack.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, EList, EPlus, Expr, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;

// Direct, CLONE-FREE constructors. `models::rust::utils::new_eplus_par` /
// `new_elist_par` route through `Par::with_locally_free`, which does `..self.clone()`
// — so accumulating with them deep-clones the whole term every iteration (O(n^2) work,
// O(n) recursive-clone stack). That would overflow in the BUILDER before eval runs and
// confound the measurement. Here every step MOVES the accumulator (no clone), so build
// is genuinely O(1) stack per level and the only deep recursion measured is the evaluator.
fn gint_par(v: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }],
        ..Default::default()
    }
}

fn build_plus(depth: usize) -> Par {
    let mut t = gint_par(0);
    for _ in 0..depth {
        let e = Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(t),
                p2: Some(gint_par(1)),
            })),
        };
        t = Par {
            exprs: vec![e],
            ..Default::default()
        };
    }
    t
}

fn build_list(depth: usize) -> Par {
    let mut u = gint_par(0);
    for _ in 0..depth {
        let e = Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![u],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        u = Par {
            exprs: vec![e],
            ..Default::default()
        };
    }
    u
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.get(1).map(|s| s.as_str()).unwrap_or("plus").to_string();
    let depth: usize = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    // Optional arg 3: eval-thread stack size in MiB (default 8 = the process default).
    // Used to demonstrate that the overflow ceiling is a pure linear function of
    // available stack — i.e. a heap-bounded evaluator (whose recursion "stack"
    // lives in the heap) has no fixed depth ceiling.
    let stack_mib: usize = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);

    // Reducer setup only (async — needs the store). eval runs off-runtime below.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let reducer = rt.block_on(async {
        let (_space, reducer) = create_test_space::<
            RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        >()
        .await;
        reducer
    });

    let kind_for_thread = kind.clone();

    // Build the term, evaluate it, and DROP both the input term and the (possibly
    // deeply-nested) result ALL on the worker thread — main only ever sees a shallow
    // Result<(), String>. This isolates the measurement to the evaluator's recursion:
    // the term build is iterative (no deep stack) and the deep Drop happens here, on a
    // thread whose base stack `stack_mib` we control, so neither confounds the eval depth.
    let handle = std::thread::Builder::new()
        .stack_size(stack_mib * 1024 * 1024)
        .spawn(move || -> Result<(), String> {
            let par = match kind_for_thread.as_str() {
                "list" => build_list(depth),
                _ => build_plus(depth),
            };
            let env: Env<Par> = Env::new();
            let result = reducer.eval_expr(&par, &env).map_err(|e| format!("{:?}", e));
            // `result` (deep for `list`) and `par` drop here, on the worker thread.
            result.map(|_p| ())
        })
        .expect("spawn eval thread");

    match handle.join() {
        Ok(Ok(())) => {
            println!("OK kind={} depth={}", kind, depth);
            std::process::exit(0);
        }
        Ok(Err(e)) => {
            println!("ERR kind={} depth={} err={}", kind, depth, e);
            std::process::exit(2);
        }
        Err(_) => {
            println!("PANIC kind={} depth={}", kind, depth);
            std::process::exit(3);
        }
    }
}
