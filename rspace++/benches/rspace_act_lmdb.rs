//! Bounded ACT-versus-LMDB persistence comparison.
//!
//! Both arms use identical canonical byte keys and fixed-size byte payloads.
//! ACT stores the trie topology plus a u64 payload ordinal; LMDB stores the
//! same key and payload bytes through the production `KeyValueStore` API.
//! Construction/durable-write and lookup are measured separately so batching
//! and mmap warm-up cannot hide the actual trade-off.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use heed::EnvOpenOptions;
use heed::types::SerdeBincode;
use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

const CARDINALITIES: [usize; 3] = [256, 4_096, 16_384];
const PAYLOAD_BYTES: usize = 128;

struct Fixture {
    keys: Vec<Vec<u8>>,
    values: Vec<Vec<u8>>,
    map: PathMap<u64>,
}

fn fixture(cardinality: usize) -> Fixture {
    let mut keys = Vec::with_capacity(cardinality);
    let mut values = Vec::with_capacity(cardinality);
    let mut map = PathMap::new();
    for ordinal in 0..cardinality {
        let mut key = b"rho/act-lmdb/shared-prefix/".to_vec();
        key.extend_from_slice(&(ordinal as u64).to_be_bytes());
        let value = vec![(ordinal as u8).wrapping_mul(31); PAYLOAD_BYTES];
        map.insert(key.clone(), ordinal as u64)
            .expect("fixture insert");
        keys.push(key);
        values.push(value);
    }
    Fixture { keys, values, map }
}

fn unique_path(label: &str, cardinality: usize) -> std::path::PathBuf {
    let path = std::env::temp_dir()
        .join(format!("rspace-act-lmdb-{label}-{}-{cardinality}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create benchmark directory");
    path
}

fn build_lmdb(fixture: &Fixture, path: &std::path::Path) -> LmdbKeyValueStore {
    let mut options = EnvOpenOptions::new();
    options.map_size(256 * 1024 * 1024).max_dbs(2);
    let env = Arc::new(unsafe { options.open(path).expect("open LMDB") });
    let mut transaction = env.write_txn().expect("LMDB write transaction");
    let database = env
        .create_database::<SerdeBincode<Vec<u8>>, SerdeBincode<Vec<u8>>>(
            &mut transaction,
            Some("state"),
        )
        .expect("create LMDB database");
    transaction.commit().expect("commit LMDB schema");
    let store = LmdbKeyValueStore::new(env, database);
    let pairs = fixture
        .keys
        .iter()
        .cloned()
        .zip(fixture.values.iter().cloned())
        .collect();
    store.put(pairs).expect("populate LMDB");
    store
}

fn bench_persistence(c: &mut Criterion) {
    let mut group = c.benchmark_group("rspace_act_lmdb_persistence");
    for cardinality in CARDINALITIES {
        let fixture = fixture(cardinality);
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_with_input(
            BenchmarkId::new("act_dump", cardinality),
            &fixture,
            |b, fixture| {
                b.iter_batched(
                    || unique_path("act", cardinality),
                    |path| {
                        let act = ArenaCompactTree::dump_from_zipper(
                            black_box(fixture).map.read_zipper(),
                            |value| *value,
                            path.join("state.act"),
                        )
                        .expect("dump ACT");
                        black_box(act.get_data().len());
                        std::fs::remove_dir_all(path).expect("remove ACT benchmark directory");
                    },
                    criterion::BatchSize::SmallInput,
                )
            },
        );
        group.bench_with_input(
            BenchmarkId::new("lmdb_batch_put", cardinality),
            &fixture,
            |b, fixture| {
                b.iter_batched(
                    || unique_path("lmdb", cardinality),
                    |path| {
                        let _store = build_lmdb(black_box(fixture), &path);
                        std::fs::remove_dir_all(path).expect("remove LMDB benchmark directory");
                    },
                    criterion::BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

fn bench_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("rspace_act_lmdb_lookup");
    for cardinality in CARDINALITIES {
        let fixture = fixture(cardinality);
        let act_dir = unique_path("lookup-act", cardinality);
        let act = ArenaCompactTree::dump_from_zipper(
            fixture.map.read_zipper(),
            |value| *value,
            act_dir.join("state.act"),
        )
        .expect("dump ACT");
        let lmdb_dir = unique_path("lookup-lmdb", cardinality);
        let store = build_lmdb(&fixture, &lmdb_dir);
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_with_input(
            BenchmarkId::new("act_mmap_get", cardinality),
            &fixture,
            |b, fixture| {
                b.iter(|| {
                    let sum = fixture
                        .keys
                        .iter()
                        .map(|key| act.get_val_at(black_box(key)).unwrap_or_default())
                        .sum::<u64>();
                    black_box(sum)
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("lmdb_get_batch", cardinality),
            &fixture,
            |b, fixture| {
                b.iter(|| {
                    let keys = black_box(fixture.keys.clone());
                    let values = store.get(&keys).expect("LMDB lookup");
                    black_box(values.iter().flatten().map(Vec::len).sum::<usize>())
                })
            },
        );
        std::fs::remove_dir_all(act_dir).expect("remove ACT lookup directory");
        std::fs::remove_dir_all(lmdb_dir).expect("remove LMDB lookup directory");
    }
    group.finish();
}

criterion_group!(benches, bench_persistence, bench_lookup);
criterion_main!(benches);
