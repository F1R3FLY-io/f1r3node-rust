// Propose-path benchmark: like bench_par_reducer but exercises the two layers
// the in-memory evaluate-only bench skips and that dominate real propose time:
//   1. an LMDB-backed rspace store (real history-store reads/writes), and
//   2. create_checkpoint (history-trie commit) measured separately.
//
// The testbed rholang-par propose_core stayed flat (~377s at forks=32) even
// with the matcher/Arc fixes that made bench_par_reducer's evaluate parallel
// (0.52x). That implicates a layer above evaluate. This bench splits
// evaluate vs checkpoint on a real LMDB store to locate it.
//
// Run:
//   cargo run --release --bin bench_par_propose -p rholang -- 32 5000
//
// Profile (EPYC):
//   RUSTFLAGS="-C force-frame-pointers=yes -C target-feature=+aes,+sse2" \
//   CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --bin bench_par_propose -p rholang
//   ./target/release/bench_par_propose 32 5000 &
//   PID=$!; sudo perf record --call-graph fp -F 99 -p $PID; wait $PID

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_rho_runtime, RhoRuntime};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::GB;
use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;

// Same ParTerm/BusyTerm body the testbed uses.
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

// Returns (evaluate_ms, checkpoint_ms).
async fn run_once(forks: usize, iters: usize, lmdb_dir: std::path::PathBuf) -> (u128, u128) {
    let mut kvm = mk_rspace_store_manager(lmdb_dir, GB);
    let store = kvm.r_space_stores().await.unwrap();
    let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> =
        RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();

    let mut runtime = create_rho_runtime(
        space,
        Arc::new(HashMap::new()),
        true,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await;

    let term = par_contract(forks, iters);
    let rand = Blake2b512Random::create_from_length(128);

    let t_eval = Instant::now();
    let result = runtime
        .evaluate(&term, Cost::unsafe_max(), HashMap::new(), rand)
        .await
        .expect("evaluate failed");
    let eval_ms = t_eval.elapsed().as_millis();
    if !result.errors.is_empty() {
        eprintln!("  WARNING: evaluate errors: {:?}", result.errors);
    }

    // create_checkpoint commits the accumulated hot-store changes to the
    // history trie / LMDB — the step the in-memory evaluate-only bench skipped.
    let t_ckpt = Instant::now();
    let _checkpoint = runtime.create_checkpoint().await;
    let ckpt_ms = t_ckpt.elapsed().as_millis();

    (eval_ms, ckpt_ms)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let forks: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(32);
    let iters: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(5000);

    println!("bench_par_propose: forks={forks}  iters={iters}  (LMDB-backed, evaluate + checkpoint)");

    let base = std::env::temp_dir().join(format!("bench_par_propose_{}", std::process::id()));

    let (e1, c1) = run_once(1, iters, base.join("f1")).await;
    println!("forks=1   evaluate={e1} ms   checkpoint={c1} ms   total={} ms", e1 + c1);

    let (e32, c32) = run_once(forks, iters, base.join("f32")).await;
    let total32 = e32 + c32;
    println!("forks={forks}  evaluate={e32} ms   checkpoint={c32} ms   total={total32} ms");

    let eval_ratio = e32 as f64 / e1.max(1) as f64 / forks as f64;
    let ckpt_ratio = c32 as f64 / c1.max(1) as f64 / forks as f64;
    let total_ratio = total32 as f64 / (e1 + c1).max(1) as f64 / forks as f64;
    println!(
        "s/fork ratios — evaluate: {eval_ratio:.2}x   checkpoint: {ckpt_ratio:.2}x   total: {total_ratio:.2}x  \
         (1.0 = perfect parallelism, {forks}.0 = full serialization)"
    );

    let _ = std::fs::remove_dir_all(&base);
}
