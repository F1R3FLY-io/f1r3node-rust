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

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::utils::{new_eplus_par, new_elist_par, new_gint_par};
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;

fn build_plus(depth: usize) -> Par {
    let mut t = new_gint_par(0, Vec::new(), false);
    for _ in 0..depth {
        t = new_eplus_par(t, new_gint_par(1, Vec::new(), false));
    }
    t
}

fn build_list(depth: usize) -> Par {
    let mut u = new_gint_par(0, Vec::new(), false);
    for _ in 0..depth {
        u = new_elist_par(vec![u], Vec::new(), false, None, Vec::new(), false);
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
    let par = match kind.as_str() {
        "list" => build_list(depth),
        _ => build_plus(depth),
    };

    // Evaluate on a fresh thread with the DEFAULT 8 MB stack — this is the point.
    let handle = std::thread::Builder::new()
        .stack_size(stack_mib * 1024 * 1024)
        .spawn(move || {
            let env: Env<Par> = Env::new();
            reducer.eval_expr(&par, &env)
        })
        .expect("spawn eval thread");

    match handle.join() {
        Ok(Ok(_result)) => {
            println!("OK kind={} depth={}", kind_for_thread, depth);
            std::process::exit(0);
        }
        Ok(Err(e)) => {
            println!("ERR kind={} depth={} err={:?}", kind_for_thread, depth, e);
            std::process::exit(2);
        }
        Err(_) => {
            println!("PANIC kind={} depth={}", kind_for_thread, depth);
            std::process::exit(3);
        }
    }
}
