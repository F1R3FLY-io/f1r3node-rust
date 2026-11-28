use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::{RngCore, SeedableRng};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::cold_store::{
    ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData,
};
use rspace_plus_plus::rspace::history::cold_store_codec::ColdStoreCodec;
use serde::Serialize;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::memory_key_value_store::MemoryKeyValueStore;

const MAX_ROWS: usize = 256;
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const ITERATIONS: usize = 8;

#[derive(Default, Serialize)]
struct Distribution {
    p50_ns: u64,
    p95_ns: u64,
    p99_ns: u64,
}

fn distribution(mut values: Vec<u64>) -> Distribution {
    values.sort_unstable();
    let percentile = |p: usize| values[(values.len() * p).div_ceil(100).saturating_sub(1)];
    Distribution {
        p50_ns: percentile(50),
        p95_ns: percentile(95),
        p99_ns: percentile(99),
    }
}

#[derive(Serialize)]
struct Measurement {
    logical_row_bytes: u64,
    physical_row_bytes: u64,
    put: Distribution,
    get: Distribution,
}

#[derive(Serialize)]
struct Row {
    workload: String,
    legacy: Measurement,
    raw: Measurement,
    compression: Measurement,
}

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    backend: &'static str,
    build: &'static str,
    iterations: usize,
    elapsed_ms: u128,
    rows: Vec<Row>,
}

fn measure(
    key: &[u8],
    value: &[u8],
    enabled: Option<bool>,
    deadline: Instant,
) -> Result<Measurement, Box<dyn Error>> {
    let inner = Arc::new(MemoryKeyValueStore::new());
    let store: Arc<dyn KeyValueStore> = match enabled {
        None => inner.clone(),
        Some(enabled) => Arc::new(ColdStoreCodec::new(inner.clone(), enabled)),
    };
    let mut put = Vec::with_capacity(ITERATIONS);
    let mut get = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        if Instant::now() >= deadline {
            return Err("benchmark time budget exceeded".into());
        }
        let owned_key = key.to_vec();
        let owned_value = value.to_vec();
        let start = Instant::now();
        store.put_one(owned_key, owned_value)?;
        put.push(start.elapsed().as_nanos().try_into()?);
        let owned_key = key.to_vec();
        let start = Instant::now();
        let decoded = store
            .get_one(&owned_key)?
            .ok_or("missing benchmark value")?;
        get.push(start.elapsed().as_nanos().try_into()?);
        if decoded != value {
            return Err("canonical round trip mismatch".into());
        }
    }
    let physical = inner
        .get_one(&key.to_vec())?
        .ok_or("missing physical value")?;
    Ok(Measurement {
        logical_row_bytes: bincode::serialized_size(key)? + bincode::serialized_size(value)?,
        physical_row_bytes: bincode::serialized_size(key)? + bincode::serialized_size(&physical)?,
        put: distribution(put),
        get: distribution(get),
    })
}

fn add_row(
    rows: &mut Vec<Row>,
    label: String,
    key: &[u8],
    value: &[u8],
    deadline: Instant,
) -> Result<(), Box<dyn Error>> {
    if rows.len() >= MAX_ROWS {
        return Err("benchmark row budget exceeded".into());
    }
    let legacy = measure(key, value, None, deadline)?;
    let (raw, compression) = if rows.len().is_multiple_of(2) {
        (measure(key, value, Some(false), deadline)?, measure(key, value, Some(true), deadline)?)
    } else {
        let compression = measure(key, value, Some(true), deadline)?;
        (measure(key, value, Some(false), deadline)?, compression)
    };
    rows.push(Row {
        workload: label,
        legacy,
        raw,
        compression,
    });
    Ok(())
}

fn synthetic(rows: &mut Vec<Row>, deadline: Instant) -> Result<(), Box<dyn Error>> {
    for size in [0, 128, 2048, 65536, 1024 * 1024] {
        for random in [false, true] {
            let mut bytes = vec![0; size];
            if random {
                rand::rngs::StdRng::seed_from_u64(29).fill_bytes(&mut bytes);
            }
            for kind in ["joins", "data", "continuations"] {
                let value = match kind {
                    "joins" => PersistedData::Joins(JoinsLeaf {
                        bytes: bytes.clone(),
                    }),
                    "data" => PersistedData::Data(DataLeaf {
                        bytes: bytes.clone(),
                    }),
                    _ => PersistedData::Continuations(ContinuationsLeaf {
                        bytes: bytes.clone(),
                    }),
                };
                let leaf = bincode::serialize(&DataLeaf {
                    bytes: bytes.clone(),
                })?;
                let key = Blake2b256Hash::new(&leaf).bytes();
                let canonical = bincode::serialize(&value)?;
                add_row(
                    rows,
                    format!("{kind}/bytes={size}/random={random}"),
                    &key,
                    &canonical,
                    deadline,
                )?;
            }
        }
    }
    Ok(())
}

fn corpus(directory: &Path, rows: &mut Vec<Row>, deadline: Instant) -> Result<(), Box<dyn Error>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|ext| ext != "bin")
        {
            return Err("corpus must contain only regular .bin files".into());
        }
        if files.len() >= MAX_ROWS {
            return Err("corpus row budget exceeded".into());
        }
        files.push(entry.path());
    }
    files.sort();
    if files.is_empty() {
        return Err("corpus is empty".into());
    }
    for path in files {
        if Instant::now() >= deadline {
            return Err("benchmark time budget exceeded".into());
        }
        let key = hex::decode(
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or("invalid corpus key")?,
        )?;
        if key.len() != 32 && key.len() != 40 {
            return Err("invalid corpus key length".into());
        }
        let file = File::open(&path)?;
        if file.metadata()?.len() > MAX_INPUT_BYTES as u64 {
            return Err("corpus record budget exceeded".into());
        }
        let mut value = Vec::new();
        file.take(MAX_INPUT_BYTES as u64 + 1)
            .read_to_end(&mut value)?;
        if value.len() > MAX_INPUT_BYTES {
            return Err("corpus record budget exceeded".into());
        }
        let index = rows.len();
        add_row(rows, format!("corpus-row-{index}"), &key, &value, deadline)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(120);
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let mut rows = Vec::new();
    match arguments.as_slice() {
        [] => synthetic(&mut rows, deadline)?,
        [flag] if flag == "--synthetic" => synthetic(&mut rows, deadline)?,
        [flag, path] if flag == "--corpus" => corpus(Path::new(path), &mut rows, deadline)?,
        _ => return Err("usage: cold_store_compression [--synthetic | --corpus DIRECTORY]".into()),
    }
    let report = Report {
        schema: "cold-store-compression-benchmark/v1",
        backend: "memory; byte totals include LMDB bincode key/value wrappers, not database pages",
        build: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        iterations: ITERATIONS,
        elapsed_ms: started.elapsed().as_millis(),
        rows,
    };
    serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_corpus_reports_exact_sizes_and_savings() {
        for serialized_key in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let leaf = ContinuationsLeaf {
                bytes: vec![7; 8192],
            };
            let hash = Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap());
            let key = if serialized_key {
                bincode::serialize(&hash).unwrap()
            } else {
                hash.bytes()
            };
            let canonical = bincode::serialize(&PersistedData::Continuations(leaf)).unwrap();
            let path = directory.path().join(format!("{}.bin", hex::encode(&key)));
            std::fs::write(&path, &canonical).unwrap();
            let mut rows = Vec::new();
            corpus(directory.path(), &mut rows, Instant::now() + Duration::from_secs(10)).unwrap();
            assert_eq!(rows.len(), 1);
            let row = &rows[0];
            let expected = (key.len() + canonical.len() + 16) as u64;
            assert_eq!(row.legacy.physical_row_bytes, expected);
            assert_eq!(row.raw.physical_row_bytes, expected);
            assert_eq!(row.compression.logical_row_bytes, expected);
            assert!(row.compression.physical_row_bytes < expected);
            std::fs::write(path, [0; 12]).unwrap();
            assert!(
                corpus(directory.path(), &mut Vec::new(), Instant::now() + Duration::from_secs(10))
                    .is_err()
            );
        }
    }

    #[test]
    fn invalid_corpus_and_expired_time_budget_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        assert!(corpus(directory.path(), &mut Vec::new(), deadline).is_err());
        let path = directory.path().join("not-a-key.bin");
        std::fs::write(&path, []).unwrap();
        assert!(corpus(directory.path(), &mut Vec::new(), deadline).is_err());
        assert!(corpus(directory.path(), &mut Vec::new(), Instant::now()).is_err());
        std::fs::write(directory.path().join("unexpected.txt"), []).unwrap();
        assert!(corpus(directory.path(), &mut Vec::new(), deadline).is_err());
    }

    #[test]
    fn corpus_row_budget_is_enforced_before_loading_values() {
        let directory = tempfile::tempdir().unwrap();
        for i in 0..=MAX_ROWS {
            std::fs::write(directory.path().join(format!("{i:064x}.bin")), []).unwrap();
        }
        let error =
            corpus(directory.path(), &mut Vec::new(), Instant::now() + Duration::from_secs(10))
                .err()
                .unwrap();
        assert_eq!(error.to_string(), "corpus row budget exceeded");
    }
}
