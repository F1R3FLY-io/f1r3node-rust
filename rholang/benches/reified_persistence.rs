//! The persistence benchmarks of Reified RSpaces (P10 step S8, design v2 section 2.8).
//!
//! The groups measure what a block of space operations costs beyond its evaluation:
//!
//! | Group | Measure |
//! |---|---|
//! | `commit` | One deploy of 10 sends with its checkpoint, for each collection |
//! | `checkpoint_size` | One send with its checkpoint, when the space already holds S records |
//! | `restart_attach` | The first receive after a restart of an LMDB store (cold attach) and after a reset (cached) |
//! | `merge` | The merge of two branches that both write one bag over a common base on an LMDB store |
//! | `replay` | The native replay of a chain of two blocks played on an LMDB store |
//! | `lmdb_commit` | One deploy of 10 sends with its checkpoint on an LMDB store, and the bytes that each block adds to the store |
//!
//! `checkpoint_size` spreads the S records over channels of [`RECORDS_PER_CHANNEL`] records, so
//! no collection reaches the size where one send no longer fits the v6 limits. Each channel has
//! its own fill step, so the fill of each channel starts with deploys of [`common::CHUNK`] sends.
//! The legacy group `checkpoint_operations` (soft checkpoints of the legacy in-memory store) has
//! this counterpart. The bytes that a block adds to the history are recorded with
//! `RRS_BENCH_RECORD`. The LMDB stores of the cases live in directories under the scratch root,
//! and each case removes its directory when it ends.

#[path = "../tests/support/mod.rs"]
mod support;

#[allow(dead_code)]
mod common;

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant};

use common::{
    bench_case, check, create, fill, group, matches, measure_prepared, observe, par, prepare_on,
    record, record_value, runtime, sizes, with_space, Case, Observed, Step, PHLO_LIMIT,
};
use criterion::{criterion_group, criterion_main, Criterion};
use support::merge::{index, merged_root};
use support::native::{assert_reproduces, certify, NativeChain};
use support::process::scratch_root;

/// The state sizes of the checkpoint series.
const STATE_SIZES: [u64; 4] = [1_000, 10_000, 100_000, 1_000_000];
/// The records of one channel in the checkpoint series.
const RECORDS_PER_CHANNEL: u64 = 100;

fn collections() -> Vec<(&'static str, String)> {
    vec![
        ("hashmap:bag", "\"default\"".to_string()),
        ("hashmap:set", "\"default\"".to_string()),
        ("hashmap:queue", "\"default\"".to_string()),
        ("hashmap:stack", "\"default\"".to_string()),
        ("hashmap:priorityqueue", "\"default\"".to_string()),
        (
            "hashmap:vectordb",
            "\"default\"; capacity=1000000, dimensions=4".to_string(),
        ),
    ]
}

fn short(family: &str) -> &str {
    family
        .split(':')
        .nth(1)
        .expect("a family has an inner collection")
}

/// The send of record `i` on the channel of its group of [`RECORDS_PER_CHANNEL`] records.
fn spread_send(family: &str, i: u64) -> String {
    let channel = i / RECORDS_PER_CHANNEL;
    if family.ends_with("vectordb") {
        format!("@[\"c\", {channel}]!([{}, {}, {}, 1])", i % 7, i % 5, i % 3)
    } else if family.ends_with("priorityqueue") {
        format!("@[\"c\", {channel}]!({i}; priority={})", i % 3)
    } else {
        format!("@[\"c\", {channel}]!({i})")
    }
}

/// The setup steps that store `size` records of `family` with [`spread_send`]: one fill step for
/// each channel of [`RECORDS_PER_CHANNEL`] records.
fn spread_fill(family: &str, size: u64) -> Vec<Step> {
    (0..size.div_ceil(RECORDS_PER_CHANNEL))
        .map(|channel| {
            let first = channel * RECORDS_PER_CHANNEL;
            let last = (first + RECORDS_PER_CHANNEL).min(size);
            Step::Fill((first..last).map(|i| spread_send(family, i)).collect())
        })
        .collect()
}

/// A new directory for the LMDB store of the case `name` under the scratch root. A directory of
/// an earlier run of the same process id is removed first.
fn lmdb_directory(name: &str) -> std::path::PathBuf {
    let directory = scratch_root().join(format!("bench-{name}-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("remove the old LMDB directory");
    }
    directory
}

fn bench_commit(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "commit");
    for (family, ctor) in collections() {
        let case = Case {
            id: format!("{}/10", short(family)),
            setup: vec![Step::Deploy(create(family, &ctor))],
            term: with_space(&par(10, |i| spread_send(family, i))),
            with_checkpoint: true,
        };
        bench_case(&mut g, &rt, "commit", &case);
    }
    g.finish();
}

fn bench_checkpoint_size(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "checkpoint_size");
    for size in sizes(&STATE_SIZES) {
        for (family, ctor) in collections() {
            let mut setup = vec![Step::Deploy(create(family, &ctor))];
            setup.extend(spread_fill(family, size));
            let case = Case {
                id: format!("{}/{size}", short(family)),
                setup,
                // One send to a new channel, measured with its checkpoint.
                term: with_space(&spread_send(family, size + RECORDS_PER_CHANNEL)),
                with_checkpoint: true,
            };
            bench_case(&mut g, &rt, "checkpoint_size", &case);
        }
    }
    g.finish();
}

fn bench_restart_attach(c: &mut Criterion) {
    let rt = runtime();
    let mut g = group(c, "restart_attach");
    let records = 100;
    let case = Case {
        id: format!("bag/{records}"),
        setup: vec![
            Step::Deploy(create("hashmap:bag", "\"default\"")),
            fill(records, |i| format!("@\"c\"!({i})")),
        ],
        term: with_space("for (@x <- @\"c\") { out!(x) }"),
        with_checkpoint: false,
    };
    let cold = format!("restart_attach/{}/cold", case.id);
    if !matches(&format!("restart_attach/{}", case.id)) && !matches(&cold) {
        g.finish();
        return;
    }
    let directory = lmdb_directory("restart");
    let lmdb = directory.join("lmdb");
    // The contract check and the cached case: each iteration resets the chain to the root, and
    // the runtime keeps its caches of the history.
    let setup_started = Instant::now();
    let prepared = rt.block_on(async {
        let opened = NativeChain::open(&lmdb, None).await;
        prepare_on(opened, &case, "restart_attach").await
    });
    let (chain, root) = measure_prepared(
        &mut g,
        &rt,
        "restart_attach",
        &case,
        prepared,
        setup_started.elapsed(),
    )
    .unwrap_or_else(|| panic!("restart_attach: the case {} does not fit", case.id));
    // Dropping the chain closes the LMDB store, as a node that stops.
    drop(chain);
    if matches(&cold) {
        // The cold case: each iteration opens the store at the root with a new runtime, as a
        // node that restarts, and times the first receive. The open and the close are not timed.
        let label = format!("{cold} measured");
        g.bench_function(format!("{}/cold", case.id), |b| {
            b.iter_custom(|iterations| {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    let mut restarted = rt.block_on(NativeChain::open(&lmdb, Some(&root)));
                    let start = Instant::now();
                    let evaluated = rt.block_on(restarted.evaluate(&case.term, &label, PHLO_LIMIT));
                    total += start.elapsed();
                    std::hint::black_box(&evaluated);
                    drop(restarted);
                }
                total
            })
        });
    }
    g.finish();
    std::fs::remove_dir_all(&directory).expect("remove the LMDB directory of the restart case");
}

fn bench_merge(c: &mut Criterion) {
    if !matches("merge/bag_two_branches/10") {
        return;
    }
    let rt = runtime();
    let mut g = group(c, "merge");
    let directory = lmdb_directory("merge");
    // Two branches over one base each store 10 records in one bag; a bag has no counter, so the
    // merger accepts both branches.
    let case = Case {
        id: "bag_two_branches/10".to_string(),
        setup: vec![Step::Deploy(create("hashmap:bag", "\"default\""))],
        term: String::new(),
        with_checkpoint: false,
    };
    let (mut chain, base) = rt
        .block_on(async {
            let opened = NativeChain::open(&directory.join("lmdb"), None).await;
            prepare_on(opened, &case, "merge").await
        })
        .unwrap_or_else(|reached| panic!("merge: the setup stops at {reached} records"));
    let left = with_space(&par(10, |i| format!("@\"c\"!({i})")));
    let right = with_space(&par(10, |i| format!("@\"c\"!({})", 100 + i)));
    let (left_played, right_played) = rt.block_on(async {
        let left_played = chain.play(&left, "merge left", PHLO_LIMIT).await;
        chain.reset(&base).await;
        let right_played = chain.play(&right, "merge right", PHLO_LIMIT).await;
        (left_played, right_played)
    });
    let history = chain.history();
    let left_index = index(&history, &base, &left_played.log);
    let right_index = index(&history, &base, &right_played.log);
    let merged = merged_root(&history, &base, &[
        (&left_played.root, &left_index),
        (&right_played.root, &right_index),
    ]);
    let observed = Observed {
        result: vec![],
        phlo: None,
        root: hex::encode(merged.bytes()),
    };
    check(&format!("merge/{}", case.id), &observed);
    record(
        &format!("merge/{}", case.id),
        &observed,
        Duration::ZERO,
        Duration::ZERO,
    );
    g.bench_function(case.id.as_str(), |b| {
        b.iter(|| {
            let left_index = index(&history, &base, &left_played.log);
            let right_index = index(&history, &base, &right_played.log);
            std::hint::black_box(merged_root(&history, &base, &[
                (&left_played.root, &left_index),
                (&right_played.root, &right_index),
            ]))
        })
    });
    g.finish();
    drop(history);
    drop(chain);
    std::fs::remove_dir_all(&directory).expect("remove the LMDB directory of the merge case");
}

fn bench_replay(c: &mut Criterion) {
    if !matches("replay/queue_two_blocks/10") {
        return;
    }
    let rt = runtime();
    let mut g = group(c, "replay");
    let directory = lmdb_directory("replay");
    let first = create("hashmap:queue", "\"default\"");
    let second = with_space(&par(10, |i| format!("@\"c\"!({i})")));
    let (chain, first_played, second_played) = rt.block_on(async {
        let mut chain = NativeChain::open(&directory.join("lmdb"), None).await;
        let first_played = chain.play(&first, "replay block 1", PHLO_LIMIT).await;
        let second_played = chain.play(&second, "replay block 2", PHLO_LIMIT).await;
        (chain, first_played, second_played)
    });
    // The reference replay: native replay reproduces each block of its play.
    rt.block_on(async {
        let certification = certify(&first_played, &first, PHLO_LIMIT).await;
        assert_reproduces("replay block 1", &first_played, &certification);
        let certification = certify(&second_played, &second, PHLO_LIMIT).await;
        assert_reproduces("replay block 2", &second_played, &certification);
    });
    let observed = Observed {
        result: vec!["reproduced".to_string()],
        phlo: second_played.result.native_phlo_usage,
        root: hex::encode(second_played.root.bytes()),
    };
    check("replay/queue_two_blocks/10", &observed);
    record(
        "replay/queue_two_blocks/10",
        &observed,
        Duration::ZERO,
        Duration::ZERO,
    );
    g.bench_function("queue_two_blocks/10", |b| {
        b.iter(|| {
            rt.block_on(async {
                std::hint::black_box(certify(&first_played, &first, PHLO_LIMIT).await);
                std::hint::black_box(certify(&second_played, &second, PHLO_LIMIT).await);
            })
        })
    });
    g.finish();
    drop((first_played, second_played, chain));
    std::fs::remove_dir_all(&directory).expect("remove the LMDB directory of the replay case");
}

/// The bytes of the LMDB store in `directory`: for each LMDB environment of the store (one
/// subdirectory, such as the history and the cold store), the length and the allocated bytes of
/// its data file.
fn store_bytes(directory: &std::path::Path) -> BTreeMap<String, (u64, u64)> {
    let mut environments = BTreeMap::new();
    for entry in std::fs::read_dir(directory).expect("read the LMDB directory") {
        let entry = entry.expect("an entry of the LMDB directory");
        if let Ok(metadata) = std::fs::metadata(entry.path().join("data.mdb")) {
            let name = entry.file_name().to_string_lossy().into_owned();
            environments.insert(name, (metadata.len(), metadata.blocks() * 512));
        }
    }
    assert!(!environments.is_empty(), "the LMDB store has no data file");
    environments
}

fn bench_lmdb_commit(c: &mut Criterion) {
    if !matches("lmdb_commit/bag/10") {
        return;
    }
    let rt = runtime();
    let mut g = group(c, "lmdb_commit");
    let directory = lmdb_directory("lmdb-commit");
    let lmdb = directory.join("lmdb");
    let blocks = 10;
    let (mut chain, base) = rt.block_on(async {
        let mut chain = NativeChain::open(&lmdb, None).await;
        let evaluated = chain
            .evaluate(
                &create("hashmap:bag", "\"default\""),
                "lmdb_commit setup",
                PHLO_LIMIT,
            )
            .await
            .expect("the setup deploy evaluates");
        assert!(evaluated.errors.is_empty(), "the setup deploy fails");
        let base = chain.checkpoint().await;
        (chain, base)
    });
    // The bytes that each block of 10 sends adds to the LMDB store.
    let mut before = store_bytes(&lmdb);
    for block in 0..blocks {
        let term = with_space(&par(10, |i| format!("@\"c\"!({})", block * 10 + i)));
        let label = format!("lmdb_commit block {block}");
        let evaluated = rt
            .block_on(chain.evaluate(&term, &label, PHLO_LIMIT))
            .expect("a block evaluates");
        assert!(evaluated.errors.is_empty(), "{label} fails");
        rt.block_on(chain.checkpoint());
        let after = store_bytes(&lmdb);
        let environments: serde_json::Map<String, serde_json::Value> = after
            .iter()
            .map(|(name, (file, allocated))| {
                let (file_before, allocated_before) = before.get(name).copied().unwrap_or((0, 0));
                let entry = serde_json::json!({
                    "file_bytes": file,
                    "file_bytes_added": file.saturating_sub(file_before),
                    "allocated_bytes": allocated,
                    "allocated_bytes_added": allocated.saturating_sub(allocated_before),
                });
                (name.clone(), entry)
            })
            .collect();
        let total = |bytes: &BTreeMap<String, (u64, u64)>| -> (u64, u64) {
            bytes.values().fold((0, 0), |(f, a), (file, allocated)| {
                (f + file, a + allocated)
            })
        };
        let (file, allocated) = total(&after);
        let (file_before, allocated_before) = total(&before);
        record_value(serde_json::json!({
            "lmdb_block": block,
            "file_bytes": file,
            "file_bytes_added": file.saturating_sub(file_before),
            "allocated_bytes": allocated,
            "allocated_bytes_added": allocated.saturating_sub(allocated_before),
            "environments": environments,
        }));
        before = after;
    }
    rt.block_on(chain.reset(&base));
    let term = with_space(&par(10, |i| format!("@\"c\"!({i})")));
    let label = "lmdb_commit/bag/10 measured";
    let (observed, _) = rt.block_on(observe(&mut chain, &term, label));
    check("lmdb_commit/bag/10", &observed);
    record(
        "lmdb_commit/bag/10",
        &observed,
        Duration::ZERO,
        Duration::ZERO,
    );
    g.bench_function("bag/10", |b| {
        b.iter_custom(|iterations| {
            let mut total = Duration::ZERO;
            for _ in 0..iterations {
                rt.block_on(chain.reset(&base));
                let start = Instant::now();
                rt.block_on(async {
                    std::hint::black_box(chain.evaluate(&term, label, PHLO_LIMIT).await.ok());
                    std::hint::black_box(chain.checkpoint().await);
                });
                total += start.elapsed();
            }
            total
        })
    });
    g.finish();
    drop(chain);
    std::fs::remove_dir_all(&directory).expect("remove the LMDB directory of the bench");
}

criterion_group!(
    benches,
    bench_commit,
    bench_checkpoint_size,
    bench_restart_attach,
    bench_merge,
    bench_replay,
    bench_lmdb_commit
);
criterion_main!(benches);
