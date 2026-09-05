//! Bounded ACT-versus-LMDB persistence comparison.
//!
//! Both arms use identical canonical byte keys and fixed-size byte payloads.
//! ACT stores the trie topology plus a u64 payload ordinal; LMDB stores the
//! same key and payload bytes through the production `KeyValueStore` API.
//! Construction/durable-write and lookup are measured separately so batching
//! and mmap warm-up cannot hide the actual trade-off.

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use heed::EnvOpenOptions;
use heed::types::SerdeBincode;
use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

const CARDINALITIES: [usize; 3] = [256, 4_096, 16_384];
const PAYLOAD_BYTES: usize = 128;
static DIRECTORY_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

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
        let replaced = map.insert(key.clone(), ordinal as u64);
        assert!(replaced.is_none(), "fixture key unexpectedly duplicated");
        keys.push(key);
        values.push(value);
    }
    Fixture { keys, values, map }
}

fn unique_path(label: &str, cardinality: usize) -> std::path::PathBuf {
    let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!("rspace-act-lmdb-{label}-{}-{cardinality}-{sequence}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create benchmark directory");
    path
}

fn write_act_values(fixture: &Fixture, path: &std::path::Path) {
    let mut bytes = Vec::with_capacity(fixture.values.len() * PAYLOAD_BYTES);
    for value in &fixture.values {
        bytes.extend_from_slice(value);
    }
    std::fs::write(path.join("state.values"), bytes).expect("write ACT values");
}

fn directory_bytes(path: &std::path::Path) -> u64 {
    std::fs::read_dir(path)
        .expect("read benchmark directory")
        .map(|entry| {
            let entry = entry.expect("read benchmark directory entry");
            let metadata = entry.metadata().expect("stat benchmark entry");
            if metadata.is_dir() {
                directory_bytes(&entry.path())
            } else {
                metadata.len()
            }
        })
        .sum()
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
                        .unwrap_or_else(|error| {
                            panic!(
                                "dump ACT at {} (parent exists: {}): {error}",
                                path.join("state.act").display(),
                                path.exists()
                            )
                        });
                        write_act_values(fixture, &path);
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
        .unwrap_or_else(|error| {
            panic!(
                "dump ACT at {} (parent exists: {}): {error}",
                act_dir.join("state.act").display(),
                act_dir.exists()
            )
        });
        write_act_values(&fixture, &act_dir);
        let mut act_values = Vec::with_capacity(fixture.values.len() * PAYLOAD_BYTES);
        for value in &fixture.values {
            act_values.extend_from_slice(value);
        }
        let lmdb_dir = unique_path("lookup-lmdb", cardinality);
        let store = build_lmdb(&fixture, &lmdb_dir);
        let lookup_keys = fixture.keys.clone();
        eprintln!(
            "act_lmdb_disk_bytes cardinality={cardinality} act={} lmdb={}",
            directory_bytes(&act_dir),
            directory_bytes(&lmdb_dir)
        );
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_with_input(
            BenchmarkId::new("act_mmap_get", cardinality),
            &fixture,
            |b, _fixture| {
                b.iter(|| {
                    let sum = lookup_keys
                        .iter()
                        .map(|key| {
                            let ordinal =
                                act.get_val_at(black_box(key)).unwrap_or_default() as usize;
                            let start = ordinal * PAYLOAD_BYTES;
                            black_box(&act_values[start..start + PAYLOAD_BYTES]).len()
                        })
                        .sum::<usize>();
                    black_box(sum)
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("lmdb_get_batch", cardinality),
            &fixture,
            |b, _fixture| {
                b.iter(|| {
                    let values = store.get(black_box(&lookup_keys)).expect("LMDB lookup");
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
