//! Bounded ACT-versus-LMDB persistence comparison.
//!
//! Both arms use identical canonical byte keys and fixed-size byte payloads.
//! ACT stores the trie topology plus a u64 payload ordinal; LMDB stores the
//! same key and payload bytes through the production `KeyValueStore` API.
//! Construction/durable-write and lookup are measured separately so batching
//! and mmap warm-up cannot hide the actual trade-off.

use std::fs::File;
use std::hint::black_box;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use criterion::{
    BatchSize, BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use heed::EnvOpenOptions;
use heed::types::SerdeBincode;
use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

const CARDINALITIES: [usize; 3] = [256, 4_096, 16_384];
const PAYLOAD_BYTES: usize = 128;
const ORDINAL_BYTES: usize = 8;
const WORD_BYTES: usize = 8;
const KEY_PREFIX: &[u8] = b"rho/act-lmdb/shared-prefix/";
const VISIT_MULTIPLIER: u64 = 0x9E37_79B1;
const CHECKSUM_SEED: u64 = 0xCBF2_9CE4_8422_2325;
const CHECKSUM_PRIME: u64 = 0x0000_0100_0000_01B3;
const FOOTPRINT_REPEATS: usize = 2;
const IMAGE_FILE: &str = "state.act";
const SIDECAR_FILE: &str = "state.values";
static DIRECTORY_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    keys: Vec<Vec<u8>>,
    values: Vec<Vec<u8>>,
    map: PathMap<u64>,
}

fn fixture_key(ordinal: usize) -> Vec<u8> {
    let mut key = Vec::with_capacity(KEY_PREFIX.len() + ORDINAL_BYTES);
    key.extend_from_slice(KEY_PREFIX);
    key.extend_from_slice(&(ordinal as u64).to_be_bytes());
    key
}

fn fixture_payload(ordinal: usize) -> Vec<u8> {
    let mut value = vec![(ordinal as u8).wrapping_mul(31); PAYLOAD_BYTES];
    value[..ORDINAL_BYTES].copy_from_slice(&(ordinal as u64).to_be_bytes());
    value
}

fn fixture(cardinality: usize) -> Fixture {
    let mut keys = Vec::with_capacity(cardinality);
    let mut values = Vec::with_capacity(cardinality);
    let mut map = PathMap::new();
    for ordinal in 0..cardinality {
        let key = fixture_key(ordinal);
        let replaced = map.insert(key.clone(), ordinal as u64);
        assert!(replaced.is_none(), "fixture key unexpectedly duplicated");
        keys.push(key);
        values.push(fixture_payload(ordinal));
    }
    Fixture { keys, values, map }
}

fn visit_order(cardinality: usize) -> Vec<usize> {
    let order: Vec<usize> = (0..cardinality as u64)
        .map(|index| (index * VISIT_MULTIPLIER % cardinality as u64) as usize)
        .collect();
    let mut visited = vec![false; cardinality];
    for &index in &order {
        assert!(!visited[index], "the visit order repeats key {index} of {cardinality}");
        visited[index] = true;
    }
    order
}

fn lookup_keys(fixture: &Fixture, order: &[usize]) -> Vec<Vec<u8>> {
    order
        .iter()
        .map(|&index| fixture.keys[index].clone())
        .collect()
}

fn fold_payload(state: u64, payload: &[u8]) -> u64 {
    let mut words = payload.chunks_exact(WORD_BYTES);
    let mut folded = state;
    for word in &mut words {
        let value = u64::from_le_bytes(word.try_into().expect("an eight-byte word"));
        folded = (folded ^ value).wrapping_mul(CHECKSUM_PRIME);
    }
    for byte in words.remainder() {
        folded = (folded ^ u64::from(*byte)).wrapping_mul(CHECKSUM_PRIME);
    }
    folded
}

fn fixture_checksum(fixture: &Fixture, order: &[usize]) -> u64 {
    order
        .iter()
        .fold(CHECKSUM_SEED, |state, &index| fold_payload(state, &fixture.values[index]))
}

fn act_lookup_checksum<S: AsRef<[u8]>>(
    image: &ArenaCompactTree<S>,
    sidecar: &[u8],
    keys: &[Vec<u8>],
) -> u64 {
    keys.iter().fold(CHECKSUM_SEED, |state, key| {
        let ordinal = image.get_val_at(key).expect("ACT lookup key missing") as usize;
        let start = ordinal * PAYLOAD_BYTES;
        fold_payload(state, &sidecar[start..start + PAYLOAD_BYTES])
    })
}

fn lmdb_lookup_checksum(store: &LmdbKeyValueStore, keys: &Vec<Vec<u8>>) -> u64 {
    store
        .get(keys)
        .expect("LMDB lookup")
        .iter()
        .fold(CHECKSUM_SEED, |state, value| {
            fold_payload(state, value.as_deref().expect("LMDB lookup key missing"))
        })
}

struct ScratchDirectory {
    path: PathBuf,
}

impl ScratchDirectory {
    fn new(label: &str, cardinality: usize) -> Self {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rspace-act-lmdb-{label}-{}-{cardinality}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create benchmark directory");
        Self { path }
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if !std::thread::panicking() {
                panic!("remove benchmark directory {}: {error}", self.path.display());
            }
        }
    }
}

fn sync_path(path: &Path) {
    File::open(path)
        .and_then(|handle| handle.sync_all())
        .unwrap_or_else(|error| panic!("sync {}: {error}", path.display()));
}

fn sidecar_bytes(fixture: &Fixture) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(fixture.values.len() * PAYLOAD_BYTES);
    for value in &fixture.values {
        bytes.extend_from_slice(value);
    }
    bytes
}

fn write_act_durably(fixture: &Fixture, directory: &Path, sidecar: &[u8]) -> impl Sized {
    let image_path = directory.join(IMAGE_FILE);
    let image =
        ArenaCompactTree::dump_from_zipper(fixture.map.read_zipper(), |value| *value, &image_path)
            .unwrap_or_else(|error| {
                panic!(
                    "dump ACT at {} (parent exists: {}): {error}",
                    image_path.display(),
                    directory.exists()
                )
            });
    sync_path(&image_path);
    let sidecar_path = directory.join(SIDECAR_FILE);
    let mut file = File::create(&sidecar_path)
        .unwrap_or_else(|error| panic!("create {}: {error}", sidecar_path.display()));
    file.write_all(sidecar)
        .unwrap_or_else(|error| panic!("write {}: {error}", sidecar_path.display()));
    file.sync_all()
        .unwrap_or_else(|error| panic!("sync {}: {error}", sidecar_path.display()));
    sync_path(directory);
    image
}

fn open_lmdb(directory: &Path) -> LmdbKeyValueStore {
    let mut options = EnvOpenOptions::new();
    options.map_size(256 * 1024 * 1024).max_dbs(2);
    let env = Arc::new(unsafe { options.open(directory).expect("open LMDB") });
    let mut transaction = env.write_txn().expect("LMDB write transaction");
    let database = env
        .create_database::<SerdeBincode<Vec<u8>>, SerdeBincode<Vec<u8>>>(
            &mut transaction,
            Some("state"),
        )
        .expect("create LMDB database");
    transaction.commit().expect("commit LMDB schema");
    sync_path(directory);
    LmdbKeyValueStore::new(env, database)
}

fn owned_pairs(fixture: &Fixture) -> Vec<(Vec<u8>, Vec<u8>)> {
    fixture
        .keys
        .iter()
        .cloned()
        .zip(fixture.values.iter().cloned())
        .collect()
}

struct ActInput {
    directory: ScratchDirectory,
    sidecar: Vec<u8>,
}

struct LmdbInput {
    store: LmdbKeyValueStore,
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    directory: ScratchDirectory,
}

fn preflight_failure(cardinality: usize, detail: String) -> ! {
    panic!("act_lmdb_preflight size={cardinality} failed: {detail}")
}

fn first_difference(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .position(|(a, b)| a != b)
        .unwrap_or(left.len().min(right.len()))
}

fn preflight_size(cardinality: usize) {
    let fixture = fixture(cardinality);
    let order = visit_order(cardinality);
    let keys = lookup_keys(&fixture, &order);
    let expected = fixture_checksum(&fixture, &order);
    let act_directory = ScratchDirectory::new("preflight-act", cardinality);
    drop(write_act_durably(&fixture, &act_directory.path, &sidecar_bytes(&fixture)));
    let image = ArenaCompactTree::open_mmap(act_directory.path.join(IMAGE_FILE))
        .unwrap_or_else(|error| preflight_failure(cardinality, format!("open_mmap: {error}")));
    let sidecar = std::fs::read(act_directory.path.join(SIDECAR_FILE)).unwrap_or_else(|error| {
        preflight_failure(cardinality, format!("read the sidecar: {error}"))
    });
    let lmdb_directory = ScratchDirectory::new("preflight-lmdb", cardinality);
    let store = open_lmdb(&lmdb_directory.path);
    store.put(owned_pairs(&fixture)).expect("LMDB batch put");
    if sidecar.len() != cardinality * PAYLOAD_BYTES {
        preflight_failure(
            cardinality,
            format!(
                "the sidecar holds {} bytes, not {}",
                sidecar.len(),
                cardinality * PAYLOAD_BYTES
            ),
        );
    }
    for (ordinal, key) in fixture.keys.iter().enumerate() {
        let value = image.get_val_at(key);
        if value != Some(ordinal as u64) {
            preflight_failure(
                cardinality,
                format!("the ACT value of key {ordinal} is {value:?}, not {ordinal}"),
            );
        }
        let start = ordinal * PAYLOAD_BYTES;
        let read = &sidecar[start..start + PAYLOAD_BYTES];
        if read != fixture.values[ordinal].as_slice() {
            preflight_failure(
                cardinality,
                format!(
                    "the payload of key {ordinal} read through open_mmap and the sidecar differs \
                     from the fixture payload at byte {}",
                    first_difference(read, &fixture.values[ordinal])
                ),
            );
        }
    }
    let stored = store.get(&fixture.keys).expect("LMDB get");
    for (ordinal, value) in stored.iter().enumerate() {
        if value.as_deref() != Some(fixture.values[ordinal].as_slice()) {
            preflight_failure(
                cardinality,
                format!("LMDB get does not return the fixture payload of key {ordinal}"),
            );
        }
    }
    let act_checksum = act_lookup_checksum(&image, &sidecar, &keys);
    let lmdb_checksum = lmdb_lookup_checksum(&store, &keys);
    if act_checksum != expected || lmdb_checksum != expected {
        preflight_failure(
            cardinality,
            format!(
                "the lookup checksums {act_checksum:#018x} (ACT) and {lmdb_checksum:#018x} (LMDB) \
                 are not the fixture checksum {expected:#018x}"
            ),
        );
    }
    let absent = fixture_key(cardinality);
    if image.get_val_at(&absent).is_some() {
        preflight_failure(cardinality, "the absent key has an ACT value".to_string());
    }
    let absent_lookup = store
        .get(&vec![absent])
        .expect("LMDB get of the absent key");
    if absent_lookup.len() != 1 || absent_lookup[0].is_some() {
        preflight_failure(cardinality, "LMDB get of the absent key is not None".to_string());
    }
    let flipped_key = order[0];
    let flipped_byte = PAYLOAD_BYTES - 1;
    let mut flipped_payload = fixture.values[flipped_key].clone();
    flipped_payload[flipped_byte] ^= 0x01;
    let flipped = order.iter().fold(CHECKSUM_SEED, |state, &index| {
        if index == flipped_key {
            fold_payload(state, &flipped_payload)
        } else {
            fold_payload(state, &fixture.values[index])
        }
    });
    if flipped == expected {
        preflight_failure(
            cardinality,
            format!(
                "flipping byte {flipped_byte} of the payload of key {flipped_key} leaves the \
                 checksum unchanged"
            ),
        );
    }
    println!(
        "act_lmdb_preflight size={cardinality} passed: {cardinality} ACT values equal their \
         indices; every payload read through open_mmap and the sidecar and through LMDB get \
         equals the fixture; both lookup checksums equal the fixture checksum {expected:#018x}; \
         an absent key is None in both; flipping byte {flipped_byte} of the payload of key \
         {flipped_key} changes the checksum to {flipped:#018x}"
    );
}

fn preflight(_criterion: &mut Criterion) {
    for cardinality in CARDINALITIES {
        preflight_size(cardinality);
    }
}

fn configured() -> Criterion {
    Criterion::default()
        .sample_size(51)
        .confidence_level(0.99)
        .significance_level(0.01)
        .warm_up_time(Duration::from_secs(2))
}

fn bench_persistence(c: &mut Criterion) {
    let mut group = c.benchmark_group("rspace_act_lmdb_persistence");
    group.sampling_mode(SamplingMode::Flat);
    group.measurement_time(Duration::from_secs(10));
    for cardinality in CARDINALITIES {
        let fixture = fixture(cardinality);
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_with_input(
            BenchmarkId::new("act_dump", cardinality),
            &fixture,
            |b, fixture| {
                b.iter_batched(
                    || ActInput {
                        directory: ScratchDirectory::new("act", cardinality),
                        sidecar: sidecar_bytes(fixture),
                    },
                    |input| {
                        let image = write_act_durably(
                            black_box(fixture),
                            &input.directory.path,
                            &input.sidecar,
                        );
                        (image, input)
                    },
                    BatchSize::PerIteration,
                )
            },
        );
        group.bench_with_input(
            BenchmarkId::new("lmdb_batch_put", cardinality),
            &fixture,
            |b, fixture| {
                b.iter_batched(
                    || {
                        let directory = ScratchDirectory::new("lmdb", cardinality);
                        let store = open_lmdb(&directory.path);
                        LmdbInput {
                            store,
                            pairs: owned_pairs(fixture),
                            directory,
                        }
                    },
                    |input| {
                        let LmdbInput {
                            store,
                            pairs,
                            directory,
                        } = input;
                        store.put(black_box(pairs)).expect("LMDB batch put");
                        (store, directory)
                    },
                    BatchSize::PerIteration,
                )
            },
        );
    }
    group.finish();
}

fn bench_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("rspace_act_lmdb_lookup");
    group.measurement_time(Duration::from_secs(5));
    for cardinality in CARDINALITIES {
        let fixture = fixture(cardinality);
        let order = visit_order(cardinality);
        let lookup_keys = lookup_keys(&fixture, &order);
        let expected = fixture_checksum(&fixture, &order);
        let act_directory = ScratchDirectory::new("lookup-act", cardinality);
        drop(write_act_durably(&fixture, &act_directory.path, &sidecar_bytes(&fixture)));
        let image = ArenaCompactTree::open_mmap(act_directory.path.join(IMAGE_FILE))
            .expect("reopen the ACT image");
        let sidecar =
            std::fs::read(act_directory.path.join(SIDECAR_FILE)).expect("read the ACT sidecar");
        let lmdb_directory = ScratchDirectory::new("lookup-lmdb", cardinality);
        let store = open_lmdb(&lmdb_directory.path);
        store.put(owned_pairs(&fixture)).expect("populate LMDB");
        assert_eq!(
            act_lookup_checksum(&image, &sidecar, &lookup_keys),
            expected,
            "the ACT lookup checksum of {cardinality} keys"
        );
        assert_eq!(
            lmdb_lookup_checksum(&store, &lookup_keys),
            expected,
            "the LMDB lookup checksum of {cardinality} keys"
        );
        group.throughput(Throughput::Elements(cardinality as u64));
        group.bench_with_input(
            BenchmarkId::new("act_mmap_get", cardinality),
            &fixture,
            |b, _fixture| {
                b.iter(|| black_box(act_lookup_checksum(&image, &sidecar, black_box(&lookup_keys))))
            },
        );
        group.bench_with_input(
            BenchmarkId::new("lmdb_get_batch", cardinality),
            &fixture,
            |b, _fixture| {
                b.iter(|| black_box(lmdb_lookup_checksum(&store, black_box(&lookup_keys))))
            },
        );
    }
    group.finish();
}

fn print_footprint(repeat: usize, cardinality: usize, arm: &str, directory: &Path) {
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .expect("read footprint directory")
        .map(|entry| entry.expect("read footprint directory entry"))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let metadata = entry.metadata().expect("stat footprint entry");
        assert!(metadata.is_file(), "footprint entry {} is not a file", entry.path().display());
        println!(
            "act_lmdb_footprint repeat={repeat} size={cardinality} arm={arm} file={} apparent={} \
             allocated={}",
            entry.file_name().to_string_lossy(),
            metadata.len(),
            metadata.blocks() * 512
        );
    }
}

fn footprint(_criterion: &mut Criterion) {
    for repeat in 1..=FOOTPRINT_REPEATS {
        for cardinality in CARDINALITIES {
            let fixture = fixture(cardinality);
            let act_directory = ScratchDirectory::new("footprint-act", cardinality);
            drop(write_act_durably(&fixture, &act_directory.path, &sidecar_bytes(&fixture)));
            print_footprint(repeat, cardinality, "act", &act_directory.path);
            let lmdb_directory = ScratchDirectory::new("footprint-lmdb", cardinality);
            let store = open_lmdb(&lmdb_directory.path);
            store.put(owned_pairs(&fixture)).expect("LMDB batch put");
            drop(store);
            print_footprint(repeat, cardinality, "lmdb", &lmdb_directory.path);
        }
    }
}

criterion_group! {
    name = benches;
    config = configured();
    targets = preflight, bench_persistence, bench_lookup, footprint
}
criterion_main!(benches);
