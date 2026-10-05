use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::history::roots_store::{RootsStore, RootsStoreInstances};
use rspace_plus_plus::rspace::hot_store_action::{HotStoreAction, InsertAction, InsertData};
use rspace_plus_plus::rspace::shared::rspace_store_manager::get_or_create_rspace_store;

use crate::history::history_repository_tests::datum;

type Repo = Box<dyn HistoryRepository<String, String, String, String> + Send + Sync>;

const SITES: [(&str, &str); 8] = [
    ("roots_repository", "reset"),
    ("roots_repository", "checkpoint"),
    ("roots_repository", "record_root"),
    ("roots_repository", "contains_root"),
    ("current_history", "reset"),
    ("current_history", "checkpoint"),
    ("current_history", "history_reader"),
    ("current_history", "root"),
];

fn env_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn batch(seed: u64, size: u64) -> Vec<HotStoreAction<String, String, String, String>> {
    (0..size)
        .map(|index| {
            HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                channel: format!("channel-{seed}-{index}"),
                data: vec![datum(index as i32)],
            }))
        })
        .collect()
}

struct Phase {
    resets: u64,
    checkpoints: u64,
    counters: Vec<(String, u64)>,
    histograms: Vec<(String, Vec<f64>)>,
}

impl Phase {
    fn counter(&self, name: &str) -> u64 {
        self.counters
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| *value)
            .sum()
    }

    fn histogram_mean(&self, name: &str) -> f64 {
        let values: Vec<f64> = self
            .histograms
            .iter()
            .filter(|(key, _)| key == name)
            .flat_map(|(_, values)| values.iter().copied())
            .collect();
        if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        }
    }

    fn report(&self, label: &str) {
        println!("== {label}: resets={} checkpoints={}", self.resets, self.checkpoints);
        println!(
            "{:<18} {:<15} {:>8} {:>14} {:>14} {:>14}",
            "lock", "site", "calls", "wait_mean_us", "hold_mean_us", "wait_total_ms"
        );
        for (lock, site) in SITES {
            let prefix = format!("history.repository.{lock}.{site}");
            let calls = self.counter(&format!("{prefix}.calls"));
            if calls == 0 {
                continue;
            }
            let wait = self.counter(&format!("{prefix}.wait_ns"));
            let hold = self.counter(&format!("{prefix}.hold_ns"));
            println!(
                "{lock:<18} {site:<15} {calls:>8} {:>14.1} {:>14.1} {:>14.1}",
                wait as f64 / calls as f64 / 1e3,
                hold as f64 / calls as f64 / 1e3,
                wait as f64 / 1e6
            );
        }
        for kind in ["read", "write"] {
            let calls = self.counter(&format!("history.roots_store.{kind}s"));
            let total = self.counter(&format!("history.roots_store.{kind}_ns"));
            if calls > 0 {
                println!(
                    "roots_store {kind}: calls={calls} mean_us={:.1} total_ms={:.1}",
                    total as f64 / calls as f64 / 1e3,
                    total as f64 / 1e6
                );
            }
        }
        if self.checkpoints > 0 {
            println!(
                "checkpoint mean_ms={:.1} history_process_mean_ms={:.1} root_commit_mean_ms={:.1}",
                self.histogram_mean("history.checkpoint.time") * 1e3,
                self.histogram_mean("history.checkpoint.history-process.time") * 1e3,
                self.histogram_mean("history.checkpoint.root-commit.time") * 1e3
            );
        }
    }
}

fn run_phase(
    repo: &Repo,
    roots: &[Blake2b256Hash],
    resetters: u64,
    with_checkpoints: bool,
    seconds: u64,
    batch_size: u64,
) -> Phase {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let stop = AtomicBool::new(false);
    let resets = AtomicU64::new(0);
    let checkpoints = AtomicU64::new(0);
    std::thread::scope(|scope| {
        if with_checkpoints {
            let writer = repo.reset(&roots[0]).unwrap();
            let (stop, checkpoints, recorder) = (&stop, &checkpoints, &recorder);
            scope.spawn(move || {
                metrics::with_local_recorder(recorder, || {
                    let mut seed = 1_000_000;
                    while !stop.load(Ordering::Relaxed) {
                        writer.checkpoint(batch(seed, batch_size));
                        seed += 1;
                        checkpoints.fetch_add(1, Ordering::Relaxed);
                    }
                })
            });
        }
        for thread in 0..resetters {
            let space = repo.reset(&roots[0]).unwrap();
            let (stop, resets, recorder) = (&stop, &resets, &recorder);
            scope.spawn(move || {
                metrics::with_local_recorder(recorder, || {
                    let mut index = thread as usize;
                    while !stop.load(Ordering::Relaxed) {
                        space.reset(&roots[index % roots.len()]).unwrap();
                        index += 1;
                        resets.fetch_add(1, Ordering::Relaxed);
                    }
                })
            });
        }
        std::thread::sleep(Duration::from_secs(seconds));
        stop.store(true, Ordering::Relaxed);
    });
    let mut counters = Vec::new();
    let mut histograms = Vec::new();
    for (key, _, _, value) in snapshotter.snapshot().into_vec() {
        let name = key.key().name().to_string();
        match value {
            DebugValue::Counter(value) => counters.push((name, value)),
            DebugValue::Histogram(values) => {
                histograms.push((name, values.iter().map(|v| v.into_inner()).collect()))
            }
            DebugValue::Gauge(_) => {}
        }
    }
    Phase {
        resets: resets.into_inner(),
        checkpoints: checkpoints.into_inner(),
        counters,
        histograms,
    }
}

fn roots_store_counters(f: impl FnOnce()) -> (u64, u64) {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, f);
    let mut reads = 0;
    let mut writes = 0;
    for (key, _, _, value) in snapshotter.snapshot().into_vec() {
        if let DebugValue::Counter(value) = value {
            match key.key().name() {
                "history.roots_store.reads" => reads += value,
                "history.roots_store.writes" => writes += value,
                _ => {}
            }
        }
    }
    (reads, writes)
}

#[test]
fn lmdb_roots_store_reset_reads_once_and_never_writes() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 28).unwrap();
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let first = repo.checkpoint(batch(1, 4));
    let second = first.checkpoint(batch(2, 4));
    let roots = [repo.root(), first.root(), second.root()];

    let (reads, writes) = roots_store_counters(|| {
        for root in &roots {
            repo.reset(root).unwrap();
        }
    });

    assert_eq!((reads, writes), (3, 0));
    assert!(repo.reset(&Blake2b256Hash::new(b"absent root")).is_err());
}

#[test]
fn lmdb_roots_store_record_root_commits_both_keys_in_one_write() {
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 28).unwrap();
    let roots_kv = store.roots.clone();
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let root = Blake2b256Hash::new(b"recorded root");

    let (_, writes) = roots_store_counters(|| repo.record_root(&root).unwrap());

    assert_eq!(writes, 1);
    assert!(repo.contains_root(&root).unwrap());
    assert_eq!(
        RootsStoreInstances::roots_store(roots_kv)
            .current_root()
            .unwrap(),
        Some(root)
    );
}

#[test]
fn lmdb_repository_reopens_at_the_last_committed_root_after_resets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store");
    let store = get_or_create_rspace_store(path.to_str().unwrap(), 1 << 28).unwrap();
    let (history, roots, cold) = (store.history.clone(), store.roots.clone(), store.cold.clone());
    let repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let first = repo.checkpoint(batch(1, 4));
    let second = first.checkpoint(batch(2, 4));
    second.reset(&first.root()).unwrap();

    let reopened: Repo = HistoryRepositoryInstances::lmdb_repository(history, roots, cold).unwrap();

    assert_eq!(reopened.root(), second.root());
}

#[test]
#[ignore = "contention probe: cargo test -p rspace_plus_plus --release roots_lock_contention_probe \
            -- --ignored --nocapture"]
fn roots_lock_contention_probe() {
    let seconds = env_or("PROBE_SECONDS", 10);
    let resetters = env_or("PROBE_RESETTERS", 4);
    let batch_size = env_or("PROBE_BATCH", 2000);
    let dir = tempfile::tempdir().unwrap();
    let store =
        get_or_create_rspace_store(dir.path().join("store").to_str().unwrap(), 1 << 32).unwrap();
    let mut repo: Repo =
        HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)
            .unwrap();
    let mut roots = vec![repo.root()];
    for seed in 0..8 {
        repo = repo.checkpoint(batch(seed, 64));
        roots.push(repo.root());
    }
    println!(
        "probe: seconds={seconds} resetters={resetters} batch={batch_size} roots={}",
        roots.len()
    );
    let idle = run_phase(&repo, &roots, resetters, false, seconds, batch_size);
    idle.report("resets only");
    let loaded = run_phase(&repo, &roots, resetters, true, seconds, batch_size);
    loaded.report("resets with checkpoints");
    assert!(idle.resets > 0);
    assert!(loaded.resets > 0 && loaded.checkpoints > 0);
}
