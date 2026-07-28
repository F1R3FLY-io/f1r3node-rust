//! # The END-TO-END deploy depth ceiling, measured on a production-sized worker
//!
//! `rholang/tests/stack_depth_gate.rs` measures ONE traversal at a time, each on
//! a hand-built fixture, and asks *how much native stack does it need per nesting
//! level?* That is the right question for attributing a cost to a call site, and
//! the wrong question for the only number an operator cares about:
//!
//! > **How deep can a deploy nest before it kills the node?**
//!
//! A deploy's ceiling is not any one traversal's ceiling. It is the ceiling of
//! whichever Θ(depth) traversal on its path is *worst*, and which traversals are
//! on the path depends on what the deploy DOES — a bare literal never enters
//! substitution, and a literal that arrives through a `for` binder enters it with
//! a deep bound value. So the ceiling has to be measured from SOURCE, through the
//! real runtime, at a real worker's stack size.
//!
//! Full analysis: `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
//!
//! ## What "a real worker's stack" means here
//!
//! `DebruijnInterpreter::eval_inner` detaches each parallel branch with
//! `tokio::spawn` (`reduce.rs`, `spawn_detached`), so reduction runs on tokio
//! **worker** threads, not on the thread that called `evaluate`. A tokio worker
//! gets Rust's default spawned-thread stack — **2 MiB** — whenever
//! `thread_stack_size` is unset and `RUST_MIN_STACK` is absent from the node's
//! environment.
//!
//! ⚠ This repository's `.cargo/config.toml` sets `RUST_MIN_STACK = 8388608`, so a
//! probe that simply spawned a runtime would measure an **8 MiB** worker and
//! report a ceiling four times the production one. Every probe below therefore
//! sets `thread_stack_size` **explicitly** on the tokio builder, exactly as
//! `stack_depth_gate.rs`'s `gate_child` sets `stack_size` explicitly on its
//! thread, and for exactly the same reason: a measurement that an environment
//! variable can move is not a measurement of the code.
//!
//! ## Why a child process per probe point
//!
//! Native-stack exhaustion is a `SIGSEGV` on the guard page, which Rust's handler
//! turns into `fatal runtime error: stack overflow` + `abort()` — **signal 6,
//! `SIGABRT`, shell status 134**. It is neither a panic nor an `Err`: it cannot be
//! caught, `catch_unwind` cannot contain it, and it takes down every other
//! assertion in the binary with it. So the parent re-execs this binary once per
//! (subject, depth, stack) and reads the exit status. 0 = the deploy ran;
//! anything else = it did not. Same discipline as `stack_depth_gate.rs` and
//! `stack_depth_probe.rs`.
//!
//! ## Invocation
//!
//! ```text
//! DEPLOY_SUBJECT=env_get_deploy DEPLOY_DEPTH=289 DEPLOY_STACK=2097152 \
//!   cargo test --release -p rholang --test deploy_depth_ceiling \
//!   -- --ignored --exact deploy_child
//! ```

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

// ---------------------------------------------------------------------------
// The production worker stack, and the one knob that can hide it
// ---------------------------------------------------------------------------

/// Rust's default stack for a spawned thread on Linux, which is what a tokio
/// worker gets when the node does not override it. Every headline number in this
/// file is measured at exactly this size.
const PRODUCTION_WORKER_STACK: usize = 2 * 1024 * 1024;

// ---------------------------------------------------------------------------
// sources — built ITERATIVELY, so the builder is never the constraint
// ---------------------------------------------------------------------------

/// `[[[…[0]…]]]` — `depth` opening brackets, a `0`, `depth` closing brackets.
/// The 577-byte reproducer at `depth = 288`.
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

/// A deploy that only ever BUILDS and RELEASES a deep term: no COMM, no binder,
/// nothing enters substitution with a deep environment. The control for
/// [`env_get_deploy_source`].
fn plain_deploy_source(depth: usize) -> String {
    format!("@\"out\"!({})", nested_list_source(depth))
}

/// ★ The shape that puts a deep term into the **environment**.
///
/// `dispatch.rs`'s `build_env` (`:16-25`) folds every `Par` a COMM delivered into
/// an `Env<Par>`; substituting the body's `BoundVar` then splices that value back
/// in, and `Env::get` returns it **cloned** — `<Par as Clone>::clone`, a derived
/// impl over 39 `prost` messages that is Θ(depth) in native stack. That call site
/// is documented as un-removable (`stack_depth_gate.rs`: *"the copy IS the
/// meaning of substitution"*), so whatever it costs is a floor on this shape.
fn env_get_deploy_source(depth: usize) -> String {
    format!(
        "for(@x <- @\"c\"){{ @\"out\"!(x) }} | @\"c\"!({})",
        nested_list_source(depth)
    )
}

/// Count a source's leading bracket run — the parameter the subject claims to
/// carry. Iterative by construction.
fn source_bracket_depth(src: &str) -> usize {
    src.bytes()
        .skip_while(|b| *b != b'[')
        .take_while(|b| *b == b'[')
        .count()
}

/// `[[…[x]…]]` nesting of a normalized term, counted ITERATIVELY — a recursive
/// checker would be the thing that overflows.
fn par_depth(p: &Par) -> usize {
    let mut n = 0usize;
    let mut cur = p;
    loop {
        let next = cur.exprs.first().and_then(|e| match &e.expr_instance {
            Some(ExprInstance::EListBody(l)) => l.ps.first(),
            _ => None,
        });
        match next {
            Some(child) => {
                n += 1;
                cur = child;
            }
            None => return n,
        }
    }
}

fn gstring_channel(name: &str) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }])
}

// ---------------------------------------------------------------------------
// ⚠ ANTI-VACUITY
//
// A probe whose deploy silently failed — a parse error, a rejected term, a COMM
// that never fired — would exit 0 and be recorded as "survived", and the ceiling
// would be reported for a computation that never ran. That is the same failure
// the audit records three times (§4.3, §11.3, the score-tree subjects), and it is
// the ONLY way a bisection on exit status can lie.
//
// So every subject proves, on the child side:
//   * the SOURCE carries the bracket run it was asked for;
//   * the deploy raised no interpreter errors; and
//   * the term that ended up at `@"out"` carries the FULL nesting depth.
//
// The third is what distinguishes `env_get_deploy` from a deploy whose `for`
// never COMM'd: without a COMM there is no datum at `@"out"` at all.
// ---------------------------------------------------------------------------

fn assert_carries(what: &str, actual: usize, claimed: usize) {
    assert_eq!(
        actual, claimed,
        "VACUOUS PROBE: {} is {} but the subject was asked for {}. A deploy that \
         collapsed runs in O(1) stack and reports a comfortable ceiling for a \
         computation that never happened.",
        what, actual, claimed
    );
}

/// Run `source` through the real runtime and assert a depth-`depth` term landed
/// at `@"out"`.
async fn run_deploy_and_check(source: String, depth: usize) {
    assert_carries(
        "the deploy SOURCE's bracket run",
        source_bracket_depth(&source),
        depth,
    );
    with_runtime(
        "deploy-depth-ceiling-",
        |mut runtime: RhoRuntimeImpl| async move {
            let result = runtime
                .evaluate_with_term(&source)
                .await
                .expect("deploy_depth_ceiling: evaluate failed");
            assert!(
                result.errors.is_empty(),
                "VACUOUS PROBE: the deploy raised {:?}. A failed deploy exits 0 and would be \
             recorded as 'survived' at a depth the runtime never actually carried.",
                result.errors
            );
            let data = runtime.get_data(&gstring_channel("out")).await;
            assert_eq!(
                data.len(),
                1,
                "VACUOUS PROBE: expected exactly one datum at @\"out\", found {}. For \
             `env_get_deploy` an empty channel means the COMM never fired, so `Env::get` \
             never cloned anything and the reading is meaningless.",
                data.len()
            );
            let landed = data[0]
                .a
                .pars
                .first()
                .expect("deploy_depth_ceiling: the datum carries no Par");
            assert_carries("the term that landed at @\"out\"", par_depth(landed), depth);
            // Visible under `--nocapture`, so a single probe point can be re-run by
            // hand and SEEN to have executed. The parent nulls the child's stdout.
            println!(
                "deploy_child: depth {} ran; cost {}; {} datum at @\"out\" carrying depth {}",
                depth,
                result.cost.value,
                data.len(),
                par_depth(landed)
            );
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// the child
// ---------------------------------------------------------------------------

/// Names the subject a child process should run. One place, so parent and child
/// cannot drift.
fn subject_source(name: &str) -> fn(usize) -> String {
    match name {
        "plain_deploy" => plain_deploy_source,
        "env_get_deploy" => env_get_deploy_source,
        other => panic!("deploy_depth_ceiling: unknown DEPLOY_SUBJECT={:?}", other),
    }
}

/// The child entry point.
///
/// ⚠ A NO-OP when its environment is absent, so `--run-ignored all` does not fail
/// the suite for a reason unrelated to the property. The assertions live in its
/// callers; this is a mechanism.
#[test]
#[ignore = "child process of the deploy depth-ceiling probe; driven via DEPLOY_SUBJECT"]
fn deploy_child() {
    let Ok(name) = std::env::var("DEPLOY_SUBJECT") else {
        println!("deploy_child: no DEPLOY_SUBJECT — not a child invocation, nothing to do");
        return;
    };
    let depth: usize = std::env::var("DEPLOY_DEPTH")
        .expect("DEPLOY_DEPTH must accompany DEPLOY_SUBJECT")
        .parse()
        .expect("DEPLOY_DEPTH must be an integer");
    let stack: usize = std::env::var("DEPLOY_STACK")
        .expect("DEPLOY_STACK must accompany DEPLOY_SUBJECT")
        .parse()
        .expect("DEPLOY_STACK must be an integer");

    let source = subject_source(&name)(depth);

    // ★ `thread_stack_size` EXPLICITLY — see the module docs. This sizes both the
    // worker threads (where `spawn_detached` puts every parallel branch) and the
    // blocking pool, so nothing in the reduction runs on a stack this probe did
    // not choose.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_stack_size(stack)
        .enable_all()
        .build()
        .expect("deploy_depth_ceiling: failed to build the runtime");

    rt.block_on(async move {
        // ⚠ SPAWNED, not merely awaited. `block_on` drives a future on the calling
        // thread, which is this test's own thread and NOT one of the sized
        // workers; spawning puts `evaluate` — normalizer, metering handshake and
        // all — on a worker, which is where `ReplayRuntimeOps::run_user_deploy`
        // puts it in production.
        tokio::spawn(run_deploy_and_check(source, depth))
            .await
            .expect("deploy_depth_ceiling: the deploy task panicked")
    });
}

// ---------------------------------------------------------------------------
// the parent: bisect DEPTH at a FIXED stack
// ---------------------------------------------------------------------------

/// Run one probe point in a child process. `true` iff the deploy completed.
fn deploy_survives(subject: &str, depth: usize, stack: usize) -> bool {
    let exe = std::env::current_exe().expect("deploy_depth_ceiling: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "deploy_child"])
        .env("DEPLOY_SUBJECT", subject)
        .env("DEPLOY_DEPTH", depth.to_string())
        .env("DEPLOY_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("deploy_depth_ceiling: failed to run child")
        .success()
}

/// The exit status of one probe point, for reporting the ABORT SIGNATURE rather
/// than merely the fact of failure.
fn deploy_exit(subject: &str, depth: usize, stack: usize) -> std::process::ExitStatus {
    let exe = std::env::current_exe().expect("deploy_depth_ceiling: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "deploy_child"])
        .env("DEPLOY_SUBJECT", subject)
        .env("DEPLOY_DEPTH", depth.to_string())
        .env("DEPLOY_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("deploy_depth_ceiling: failed to run child")
}

/// The greatest source nesting depth at which `subject` still completes on a
/// `stack`-byte worker. Exponential probe upward, then bisect.
///
/// `hi_cap` bounds the search: a subject that survives it is reported as
/// "≥ hi_cap" rather than searched forever.
fn max_surviving_depth(subject: &str, stack: usize, hi_cap: usize) -> usize {
    // Depth 1 must work, or the harness is broken rather than the subject deep.
    assert!(
        deploy_survives(subject, 1, stack),
        "HARNESS FAILURE: `{subject}` does not even run at depth 1 on a {} KiB worker. \
         That is not a depth ceiling — re-run the child directly to see the error.",
        stack / 1024
    );
    let mut lo = 1usize; // known good
    let mut hi = 2usize; // candidate bad
    while hi <= hi_cap && deploy_survives(subject, hi, stack) {
        lo = hi;
        hi *= 2;
    }
    if hi > hi_cap {
        return lo;
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if deploy_survives(subject, mid, stack) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

// ---------------------------------------------------------------------------
// the measurements
// ---------------------------------------------------------------------------

/// ★★ **The number an operator would ask for: how deep can a deploy nest?**
///
/// Reported for both shapes at the production 2 MiB worker stack, with the
/// bracketing evidence (`d` survives, `d + 1` does not) printed so the reading
/// can be reproduced by hand from the invocation in the module docs.
///
/// This is a MEASUREMENT, and the only thing it asserts is that the two shapes
/// are ordered — a deploy whose deep term enters the environment cannot survive
/// deeper than one that never leaves the heap, because `env_get_deploy` performs
/// a strict superset of `plain_deploy`'s traversals. Pinning either number would
/// pin a constant that a legitimate improvement must then delete, which is the
/// failure mode `the_deploy_composition_is_bounded_below_by_its_destructor`
/// documents at length.
#[test]
fn the_deploy_depth_ceiling_at_a_production_worker_stack() {
    const HI_CAP: usize = 1 << 17; // 131,072 — two orders past any measured ceiling

    let plain = max_surviving_depth("plain_deploy", PRODUCTION_WORKER_STACK, HI_CAP);
    let env_get = max_surviving_depth("env_get_deploy", PRODUCTION_WORKER_STACK, HI_CAP);

    println!(
        "  deploy depth ceiling on a {} MiB worker: plain_deploy {}, env_get_deploy {}",
        PRODUCTION_WORKER_STACK / (1024 * 1024),
        plain,
        env_get
    );
    if plain < HI_CAP {
        println!(
            "    plain_deploy    : depth {} runs, depth {} exits {:?}",
            plain,
            plain + 1,
            deploy_exit("plain_deploy", plain + 1, PRODUCTION_WORKER_STACK)
        );
    }
    if env_get < HI_CAP {
        println!(
            "    env_get_deploy  : depth {} runs, depth {} exits {:?}",
            env_get,
            env_get + 1,
            deploy_exit("env_get_deploy", env_get + 1, PRODUCTION_WORKER_STACK)
        );
    }

    assert!(
        env_get <= plain,
        "`env_get_deploy` survived to depth {env_get} while `plain_deploy` — whose \
         traversals are a strict SUBSET of it — stopped at {plain}. A superset of \
         traversals cannot have the higher ceiling, so one of the two fixtures is not \
         carrying the depth it claims."
    );
}
