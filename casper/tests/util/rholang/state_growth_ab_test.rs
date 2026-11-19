// A/B harness: how much does rspace hot-store state grow per vault
// transfer, with and without the scope-trapped unforgeable sweep?
//
// Addresses the question in issue #923: ~99% of `rspace/history` is dead
// trie nodes that only old roots reach. H1's sweep retires data rows on
// trapped unforgeables at each checkpoint, and this test measures the
// resulting hot-store footprint after N transfers in both modes.
//
// Measurement scope: `space.to_map()` gives the hot-store snapshot
// (what the next `create_checkpoint` would flow into history). That is a
// proxy for the per-block cold-state delta — it is NOT the on-disk LMDB
// size, which depends on history retention and compaction. The delta the
// sweep can plausibly reduce shows up here.
//
// Not consensus-observable: this test is ignored by default so it does
// not contribute to CI time. Run manually with:
//
//   cargo test -p casper --test mod --release \
//     -- --nocapture --ignored state_growth_ab
//
// Optional `AB_TRANSFERS` env var sets the number of transfers per run
// (default 10). `AB_WARMUP` (default 0) runs `n` transfers before the
// measurements start, so the reported growth excludes the initial
// genesis-amortisation spike. Use low values locally — each transfer
// deploys a full block through `TestNode::add_block_from_deploys`.
//
// ⚠ `F1R3FLY_SCOPE_TRAPPED_GC` is a process-global env var: this test
// sets and unsets it between the two phases. The test is not safe to
// run in parallel with any other test that reads the same variable.

use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_test_rnode_store_manager_shared;

/// One self-contained transfer deploy. `seed_tag` makes the deployer-derived
/// target address unique across the loop so each transfer reaches a
/// freshly-registered target vault (matching the issue-923 pattern where
/// every transfer leaves temporary Mint purse / NonNegativeNumber / AuthKey
/// state behind).
fn transfer_term(seed_tag: u64) -> String {
    format!(
        r#"
new return, rl(`rho:registry:lookup`), deployerId(`rho:system:deployerId`),
    VaultAddress(`rho:vault:address`), seed, svCh, targetCh, meCh, myVaultCh,
    targetVaultCh, keyCh in {{
  rl!(`rho:vault:system`, *svCh) |
  // The seed is a tuple of (deployer, loop iteration) so each transfer
  // creates a distinct target address even though the deployer is fixed.
  VaultAddress!("fromUnforgeable", (*seed, {seed_tag}), *targetCh) |
  VaultAddress!("fromDeployerId", *deployerId, *meCh) |
  for (@(_, SystemVault) <- svCh & @target <- targetCh & @me <- meCh) {{
    @SystemVault!("findOrCreate", me, *myVaultCh) |
    @SystemVault!("findOrCreate", target, *targetVaultCh) |
    @SystemVault!("deployerAuthKey", *deployerId, *keyCh) |
    for (@(true, myVault) <- myVaultCh & key <- keyCh & @(true, _) <- targetVaultCh) {{
      @myVault!("transfer", target, 1, *key, *return)
    }}
  }}
}}
"#
    )
}

#[derive(Clone, Debug, Default)]
struct StateMetrics {
    /// rspace-history LMDB store size in bytes (radix trie — the trie-node
    /// store that `rspace/history` on real nodes is).
    history_bytes: usize,
    /// rspace-cold LMDB store size in bytes (the datum + continuation
    /// payload store, keyed by content hash).
    cold_bytes: usize,
    /// rspace-roots store size in bytes.
    roots_bytes: usize,
    /// Total of the three above.
    total_bytes: usize,
}

async fn measure(scope_id: &str) -> StateMetrics {
    let mut kvm = mk_test_rnode_store_manager_shared(scope_id.to_string());
    let history = store_bytes(&mut *kvm, "rspace-history").await;
    let cold = store_bytes(&mut *kvm, "rspace-cold").await;
    let roots = store_bytes(&mut *kvm, "rspace-roots").await;
    StateMetrics {
        history_bytes: history,
        cold_bytes: cold,
        roots_bytes: roots,
        total_bytes: history + cold + roots,
    }
}

async fn store_bytes(kvm: &mut dyn KeyValueStoreManager, name: &str) -> usize {
    // LmdbKeyValueStore does not implement `size_bytes` (it's `todo!()`), so
    // use `to_map()` which LMDB supports and sum key+value lengths. Only a
    // rough proxy for actual on-disk LMDB-page usage, but it moves the same
    // way under sweep (deleted rows vanish from the map).
    match kvm.store(name.to_string()).await {
        Ok(store) => match store.to_map() {
            Ok(m) => m.iter().map(|(k, v)| k.len() + v.len()).sum(),
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

async fn run_workload(
    label: &str,
    n_transfers: usize,
    n_warmup: usize,
    gc_enabled: bool,
) -> Vec<StateMetrics> {
    // Process-global env var. The caller guarantees no parallel test uses
    // F1R3FLY_SCOPE_TRAPPED_GC while this runs.
    // SAFETY: writing an environment variable; tests here are serial.
    unsafe {
        if gc_enabled {
            std::env::set_var("F1R3FLY_SCOPE_TRAPPED_GC", "1");
        } else {
            std::env::remove_var("F1R3FLY_SCOPE_TRAPPED_GC");
        }
    }

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis");
    let scope_id = genesis.rspace_scope_id.clone();
    let mut node = TestNode::standalone(genesis).await.expect("standalone");
    let shard_id = node.genesis.shard_id.clone();
    let sk: PrivateKey = construct_deploy::DEFAULT_SEC.clone();

    // Make each deploy's signature unique by giving it a distinct timestamp
    // (instead of calling `_now_full` which collides under rapid succession
    // and trips the DAG's "already in scope" filter).
    let base_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis() as i64;

    // Warm-up: run n_warmup transfers so the measurement baseline is after
    // genesis state is amortised over at least one real deploy. These are
    // dropped from the output.
    for i in 0..n_warmup {
        let d = construct_deploy::source_deploy(
            transfer_term(u64::MAX - i as u64),
            base_ts - 1_000_000 - i as i64,
            Some(5_000_000),
            None,
            Some(sk.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("warmup deploy");
        node.add_block_from_deploys(&[d])
            .await
            .expect("warmup block");
    }

    let mut measurements = Vec::with_capacity(n_transfers + 1);
    let baseline = measure(&scope_id).await;
    eprintln!(
        "[{}] baseline  history={:>10} cold={:>10} roots={:>7} total={:>10} B",
        label,
        baseline.history_bytes,
        baseline.cold_bytes,
        baseline.roots_bytes,
        baseline.total_bytes,
    );
    measurements.push(baseline);

    for i in 0..n_transfers {
        let d = construct_deploy::source_deploy(
            transfer_term(i as u64),
            base_ts + i as i64 + 1,
            Some(5_000_000),
            None,
            Some(sk.clone()),
            None,
            Some(shard_id.clone()),
        )
        .expect("transfer deploy");
        let block = node.add_block_from_deploys(&[d]).await.expect("add block");
        assert!(
            block.body.deploys.iter().all(|dp| !dp.is_failed),
            "transfer #{} failed: {:?}",
            i,
            block.body.deploys.iter().find(|dp| dp.is_failed)
        );
        let m = measure(&scope_id).await;
        eprintln!(
            "[{}] after t{:<3} history={:>10} cold={:>10} roots={:>7} total={:>10} B",
            label,
            i + 1,
            m.history_bytes,
            m.cold_bytes,
            m.roots_bytes,
            m.total_bytes,
        );
        measurements.push(m);
    }

    // Clean up — leave the env in off state so subsequent tests are not
    // affected by lingering GC mode.
    unsafe { std::env::remove_var("F1R3FLY_SCOPE_TRAPPED_GC") };

    measurements
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn state_growth_ab_transfers() {
    let n_transfers: usize = std::env::var("AB_TRANSFERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let n_warmup: usize = std::env::var("AB_WARMUP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    eprintln!(
        "\n=== State-growth A/B: {} warmup + {} measured transfers ===\n",
        n_warmup, n_transfers
    );

    // Run the OFF phase first. If run as a separate `cargo test` invocation
    // the second run reuses the LMDB index pages warmed by the first, so
    // the two halves share the same system-contract spine.
    let off = run_workload("no-gc", n_transfers, n_warmup, false).await;
    let on = run_workload("gc-on", n_transfers, n_warmup, true).await;

    let fmt_delta = |a: usize, b: usize| -> String {
        let a = a as i64;
        let b = b as i64;
        let d = b - a;
        let pct = if a == 0 {
            0.0
        } else {
            (d as f64 / a as f64) * 100.0
        };
        format!("{:+10} ({:+.2}%)", d, pct)
    };

    println!(
        "\n### Per-transfer state size (N={}, warmup={}) ###",
        n_transfers, n_warmup
    );
    println!(
        "{:>5}  {:>12} {:>12}  {:>12} {:>12}  {:>10} {:>10}  {:>12} {:>12}",
        "step",
        "off.history",
        "on.history",
        "off.cold",
        "on.cold",
        "off.roots",
        "on.roots",
        "off.total",
        "on.total",
    );
    for (i, (o, n)) in off.iter().zip(on.iter()).enumerate() {
        let label = if i == 0 {
            "base".to_string()
        } else {
            format!("t{}", i)
        };
        println!(
            "{:>5}  {:>12} {:>12}  {:>12} {:>12}  {:>10} {:>10}  {:>12} {:>12}",
            label,
            o.history_bytes,
            n.history_bytes,
            o.cold_bytes,
            n.cold_bytes,
            o.roots_bytes,
            n.roots_bytes,
            o.total_bytes,
            n.total_bytes,
        );
    }

    let off_final = off.last().unwrap();
    let on_final = on.last().unwrap();
    let off_base = off.first().unwrap();
    let on_base = on.first().unwrap();

    let off_grow_history = off_final
        .history_bytes
        .saturating_sub(off_base.history_bytes);
    let on_grow_history = on_final.history_bytes.saturating_sub(on_base.history_bytes);
    let off_grow_cold = off_final.cold_bytes.saturating_sub(off_base.cold_bytes);
    let on_grow_cold = on_final.cold_bytes.saturating_sub(on_base.cold_bytes);
    let off_grow_total = off_final.total_bytes.saturating_sub(off_base.total_bytes);
    let on_grow_total = on_final.total_bytes.saturating_sub(on_base.total_bytes);

    println!(
        "\n### Total growth over {} measured transfers (bytes) ###",
        n_transfers
    );
    println!("                     off               on       sweep delta");
    println!(
        "history : {:16} {:16}  {}",
        off_grow_history,
        on_grow_history,
        fmt_delta(off_grow_history, on_grow_history)
    );
    println!(
        "cold    : {:16} {:16}  {}",
        off_grow_cold,
        on_grow_cold,
        fmt_delta(off_grow_cold, on_grow_cold)
    );
    println!(
        "total   : {:16} {:16}  {}",
        off_grow_total,
        on_grow_total,
        fmt_delta(off_grow_total, on_grow_total)
    );

    if n_transfers > 0 {
        println!(
            "\nper-transfer (sweep OFF): history={:.0}B cold={:.0}B total={:.0}B",
            off_grow_history as f64 / n_transfers as f64,
            off_grow_cold as f64 / n_transfers as f64,
            off_grow_total as f64 / n_transfers as f64,
        );
        println!(
            "per-transfer (sweep  ON): history={:.0}B cold={:.0}B total={:.0}B",
            on_grow_history as f64 / n_transfers as f64,
            on_grow_cold as f64 / n_transfers as f64,
            on_grow_total as f64 / n_transfers as f64,
        );
    }
}
