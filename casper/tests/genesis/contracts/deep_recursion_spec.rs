use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use casper::rust::genesis::genesis::Genesis;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, TaggedContinuation};
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
use rspace_plus_plus::rspace::r#match::Match;

use crate::genesis::contracts::test_util::TestUtil;
use crate::util::rholang::resources::{generate_scope_id, mk_test_rnode_store_manager_shared};

// ═══════════════════════════════════════════════════════════════════════════
// ★ TWO CLAIMS, TWO CLOCKS — why this file no longer asserts on wall time
//
// Both tests below used to carry a single 180 s `tokio::time::timeout`, and it
// was doing two jobs at once:
//
//   (1) LIVENESS  — "the driver terminates"; a hang must fail, not block the
//                   suite forever;
//   (2) WORK      — "the reduction driver is heap-bound"; the pre-conversion
//                   parked-parent chain needed a 300 s budget, and tightening to
//                   180 s is what codified the improvement.
//
// Claim (2) is about how much COMPUTATION the driver performs. Wall-clock time
// is that computation *divided by the share of a CPU the operating system
// happened to give this process*, and the denominator is set by every other
// process on the machine. That made the assertion fail for reasons having
// nothing to do with the code, and it has already caused one false attribution:
// a full-suite run under a load average of ~125 recorded these two as RED, they
// were investigated as a regression, and they pass in isolation on the same
// tree.
//
// ⚠ MEASURED 2026-07-27, debug, on 32 cores — the wall/CPU split is not an
// argument, it is a reading:
//
//   | condition            | load | wall     | thread CPU | %CPU |
//   |----------------------|-----:|---------:|-----------:|-----:|
//   | idle                 |  ~6  |  95.96 s |    93.66 s |  97% |
//   | 32 spinners (2x)     |  ~40 | 121.91 s |    93.73 s |  77% |
//   | 96 spinners (4x)     | ~108 | TIMEOUT  |          — |    — |
//
// Wall time grew 27 % between the first two rows; CPU time moved **0.07 s**.
// At 4x both tests hit the old 180 s budget and failed — the reported flake,
// reproduced under a controlled parameter rather than inferred.
//
// ★ And no larger wall budget fixes it. A budget `B` is safe iff
// `B >= T_idle * C`, where `C` is the contention factor imposed by OTHER
// processes. `C` is unbounded, so no finite `B` is safe; raising it only moves
// the load at which it flakes. That is why the number was not simply widened.
//
// ★ A nextest `threads-required` annotation was considered and REJECTED for the
// same reason: it serialises a test against the other tests in its own run, and
// the contention that caused the recorded failure was EXTERNAL (concurrent
// build jobs). It would remove one term of `C` and leave `C` unbounded. It also
// could not help CI, which runs `cargo test --release -p <crate>`, not nextest.
//
// ★ Making the test genuinely faster was also rejected: the 32,768 iterations
// ARE the regression (f1r3node issues #305 and #306). Reducing the count would
// weaken the property the test exists to hold.
//
// So the two claims are now measured by the two clocks that suit them:
// (1) by a generous wall-clock timeout, and (2) by thread CPU time, which the
// table above shows is invariant to contention.
// ═══════════════════════════════════════════════════════════════════════════

/// **Liveness bound.** Its only job is to stop a genuine hang from blocking the
/// suite; it is deliberately far too loose to express a performance claim.
///
/// Derived: the work is ~93 s of CPU, so at `K`x oversubscription the wall time
/// is ≈ `93·K` seconds. 900 s tolerates `K ≈ 9.7`, against the ~3.9x
/// (load ~125 on 32 cores) that produced the recorded false failure — a margin
/// of about 2.5x beyond the worst contention ever observed here — while still
/// bounding a deadlock to fifteen minutes.
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(900);

/// **Work bound**, in CPU time, and the claim the old 180 s wall budget was
/// really trying to make.
///
/// Derived from the two numbers it must separate:
///
/// * the measured cost of the current heap-bound driver — 93.7 s of thread CPU,
///   reproduced to 0.07 % across a 27 % change in wall time, with a worst
///   observed excursion of 107.4 s under heavy memory-subsystem contention;
/// * the pre-conversion parked-parent chain, which required the 300 s budget
///   this file used to carry.
///
/// 180 s is the geometric mean of 107.4 and 300 (`sqrt(107.4 * 300) = 179.5`),
/// so it sits 1.68x above the worst measurement and 1.67x below the behaviour it
/// must reject — the widest symmetric separation the two numbers admit.
///
/// ⚠ It is numerically the same as the wall budget it replaces, and that is a
/// coincidence of the arithmetic, not a carry-over. The old 180 was a wall-clock
/// figure chosen for ~2x headroom on an idle machine; this one is a CPU-time
/// figure chosen to lie between two measured costs.
const CPU_WORK_BUDGET: Duration = Duration::from_secs(180);

/// CPU time consumed by the CALLING THREAD.
///
/// ★ Per-thread rather than per-process, and that is load-bearing. Under
/// `cargo test` — which is what CI runs — up to `nproc` tests share one process,
/// so a process-wide reading would include every sibling test's CPU and could
/// fail this test for another test's work. `/proc/thread-self` (Linux 3.17+)
/// names the calling thread only.
///
/// ★ It captures the whole cost, which was measured rather than assumed:
/// sampling `/proc/<pid>/task/*/stat` through a complete run found **one** of
/// the process's 34 threads holding 107.40 s of a 107.40 s total, the other 33
/// at zero. `#[tokio::test]` builds a `current_thread` runtime and drives it
/// with `block_on` on the test's own thread, so every spawned task runs here.
///
/// Fields 14 (`utime`) and 15 (`stime`) of `stat`, in clock ticks. The `comm`
/// field may itself contain `)`, so parsing starts after the LAST one — the
/// standard defence, and the reason this is not a `split_whitespace().nth(13)`.
#[cfg(target_os = "linux")]
fn thread_cpu_time() -> Duration {
    const TICKS_PER_SEC: u64 = 100; // _SC_CLK_TCK; constant at 100 on Linux/x86-64
    let stat = std::fs::read_to_string("/proc/thread-self/stat")
        .expect("deep_recursion_spec: cannot read /proc/thread-self/stat on Linux");
    let tail = &stat[stat
        .rfind(')')
        .expect("deep_recursion_spec: /proc/thread-self/stat has no `)` terminating comm")
        + 1..];
    let fields: Vec<&str> = tail.split_whitespace().collect();
    // After the `)` the first field is `state`, so `utime` and `stime` — overall
    // fields 14 and 15 — are indices 11 and 12 here.
    let ticks: u64 = ["utime", "stime"]
        .iter()
        .zip([11usize, 12])
        .map(|(name, i)| {
            fields
                .get(i)
                .unwrap_or_else(|| panic!("deep_recursion_spec: {name} missing from stat"))
                .parse::<u64>()
                .unwrap_or_else(|e| panic!("deep_recursion_spec: {name} is not an integer: {e}"))
        })
        .sum();
    Duration::from_secs_f64(ticks as f64 / TICKS_PER_SEC as f64)
}

/// Run `body`, asserting the liveness bound by wall clock and the work bound by
/// this thread's CPU time. One place, so the two tests cannot drift apart.
async fn assert_bounded_work(what: &str, code: &str) {
    #[cfg(target_os = "linux")]
    let cpu_before = thread_cpu_time();

    let result = eval_rholang_code(code, LIVENESS_TIMEOUT).await;

    // Liveness first: a timeout here is a hang, not slowness, because
    // LIVENESS_TIMEOUT is ~9.7x the work at full CPU.
    assert!(
        result.is_ok(),
        "{what} deep recursion failed: {:?}\n\
         (the {} s bound is a LIVENESS guard — at ~93 s of CPU it cannot be reached by \
         contention below ~9.7x oversubscription, so reaching it means the driver did not \
         terminate)",
        result.err(),
        LIVENESS_TIMEOUT.as_secs()
    );

    #[cfg(target_os = "linux")]
    {
        let spent = thread_cpu_time().saturating_sub(cpu_before);
        assert!(
            spent <= CPU_WORK_BUDGET,
            "★ {what}: the driver consumed {:.1} s of CPU against a {} s budget.\n\
             \n\
             This is a WORK regression, not a slow machine: the budget is thread CPU time, \
             which was measured to move 0.07 s across a 27 % change in wall time. The \
             number separates the heap-bound driver (93.7 s measured, 107.4 s worst \
             observed) from the parked-parent chain that preceded it (which needed a 300 s \
             budget), so exceeding it means the O(N) chain — or another traversal of its \
             cost — is back.",
            spent.as_secs_f64(),
            CPU_WORK_BUDGET.as_secs()
        );
        println!(
            "  {what}: {:.1} s CPU (budget {} s)",
            spent.as_secs_f64(),
            CPU_WORK_BUDGET.as_secs()
        );
    }
}

async fn eval_rholang_code(code: &str, timeout: Duration) -> Result<(), String> {
    let scope_id = generate_scope_id();
    let mut kvs_manager = mk_test_rnode_store_manager_shared(scope_id);
    let r_store = kvs_manager
        .r_space_stores()
        .await
        .map_err(|e| format!("Failed to create RSpaceStore: {}", e))?;

    let matcher = Arc::new(Box::new(Matcher::default())
        as Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>);

    let runtime = create_runtime_from_kv_store(
        r_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        true,
        &mut vec![],
        matcher,
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    )
    .await;

    let rand = Blake2b512Random::create_from_length(128);

    match tokio::time::timeout(
        timeout,
        TestUtil::eval(code, &runtime, HashMap::new(), rand),
    )
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("Interpreter error: {:?}", e)),
        Err(_) => Err(format!("Timeout of {:?} expired", timeout)),
    }
}

/// Regression test for https://github.com/F1R3FLY-io/f1r3node/issues/305
///
/// shortslow.rho: direct recursive contract that calls itself 32768 times.
/// Resolved structurally by the detached-spawn + atomic-counter reduction driver: the
/// eval->produce/consume->dispatch->eval recursion runs as independent O(1)-native-stack
/// tokio tasks (no StackGrowingFuture / dynamic stack growth), so there is no stack
/// overflow AND no O(N) parked-parent chain.
///
/// ⚠ The budget that codifies the heap-bound is now `CPU_WORK_BUDGET`, in CPU
/// time, and the wall clock carries only `LIVENESS_TIMEOUT`. See the header for
/// the measurements and for why widening the old 180 s wall budget could not
/// have worked. Observed cost: ~90 s wall, 93.7 s CPU, debug.
#[tokio::test]
async fn deep_recursion_shortslow_should_not_stackoverflow() {
    let code = crate::util::rholang::test_rho_loader::load_test_rho("shortslow.rho")
        .expect("Failed to load shortslow.rho");

    assert_bounded_work("shortslow", &code).await;
}

/// Regression test for https://github.com/F1R3FLY-io/f1r3node/issues/306
///
/// longslow.rho: sends a 32768-char string to a channel, reads its length,
/// then recurses that many times. This exercises produce/consume + string ops
/// in addition to deep recursion, matching the exact integration test scenario.
///
/// Same heap-bound as shortslow (detached-spawn driver), and the same two-clock
/// treatment — see the header. Observed cost: ~91 s wall, 93.7 s CPU, debug.
#[tokio::test]
async fn deep_recursion_longslow_should_not_stackoverflow() {
    let code = crate::util::rholang::test_rho_loader::load_test_rho("longslow.rho")
        .expect("Failed to load longslow.rho");

    assert_bounded_work("longslow", &code).await;
}
