// Reducer-level benchmark: runs the SAME ParTerm/BusyTerm contract the
// rholang-par testbed uses, through the full RhoRuntime::evaluate path.
//
// Unlike rspace++/bench_par_branches (which calls rspace.produce directly and
// bypasses cost accounting + the interpreter), this goes through:
//   evaluate -> reduce eval_par -> tokio::spawn per branch -> charge() per step
// so it exercises the CostManager mutex that the testbed result implicates.
//
// Run:
//   cargo run --release --bin bench_par_reducer -p rholang -- 32 5000
//
// Profile (M1):
//   RUSTFLAGS="-C force-frame-pointers=yes" CARGO_PROFILE_RELEASE_DEBUG=1 \
//     cargo build --release --bin bench_par_reducer -p rholang
//   samply record ./target/release/bench_par_reducer 32 5000

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_rho_runtime, RhoRuntime};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;

// Generates the ParTerm contract: N par-composed busy branches, each running
// `iters` rounds of a recursive countdown, fork-joined by an N-receive `for`.
// Identical structure to the testbed's rholang-par body.
fn par_contract(forks: usize, iters: usize) -> String {
    let busy_calls: Vec<String> = (0..forks)
        .map(|i| format!("busy!({}, {}, *done)", iters, i))
        .collect();
    let receives: Vec<String> = (0..forks).map(|_| "@_ <- done".to_string()).collect();

    format!(
        r#"new done, stdout(`rho:io:stdout`) in {{
  new busy in {{
    contract busy(@iters, @id, done) = {{
      new loop in {{
        contract loop(@n) = {{
          if (n <= 0) {{ done!(id) }} else {{ loop!(n - 1) }}
        }} |
        loop!(iters)
      }}
    }} |
    {busy}
  }} |
  for ({recv}) {{
    stdout!(("ParTerm done", {forks}))
  }}
}}"#,
        busy = busy_calls.join(" | "),
        recv = receives.join("; "),
        forks = forks,
    )
}

async fn run_once(forks: usize, iters: usize) -> u128 {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> =
        RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();

    let runtime = create_rho_runtime(
        space,
        Arc::new(HashMap::new()),
        true,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await;

    let term = par_contract(forks, iters);
    let rand = Blake2b512Random::create_from_length(128);

    let t = Instant::now();
    let result = runtime
        .evaluate(&term, Cost::unsafe_max(), HashMap::new(), rand)
        .await
        .expect("evaluate failed");
    let elapsed = t.elapsed().as_millis();

    if !result.errors.is_empty() {
        eprintln!("  WARNING: evaluate returned errors: {:?}", result.errors);
    }
    elapsed
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let forks: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(32);
    let iters: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(5000);

    println!("bench_par_reducer: forks={forks}  iters={iters}");

    // Baseline: 1 fork. s/fork at higher forks should stay flat if the reducer
    // parallelizes; it grows if branches serialize (on CostManager or rspace).
    let single = run_once(1, iters).await;
    println!("forks=1   {single} ms   ({single} ms/fork)");

    let multi = run_once(forks, iters).await;
    let per_fork = multi as f64 / forks as f64;
    let baseline_per_fork = single as f64;
    let slowdown = per_fork / baseline_per_fork;

    println!("forks={forks}  {multi} ms   ({per_fork:.0} ms/fork)");
    println!(
        "s/fork ratio: {slowdown:.2}x baseline  (1.0 = perfect parallelism, {forks}.0 = full serialization)"
    );
}
