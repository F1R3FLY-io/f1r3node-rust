use std::error::Error;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use heed::types::Bytes;
use heed::{EnvFlags, EnvOpenOptions};
use serde_json::{json, Value};

const SAMPLE_RECORDS: usize = 256;
const SAMPLE_TIME: Duration = Duration::from_secs(2);

fn inspect(path: &Path, names: &[&str]) -> Result<Value, Box<dyn Error>> {
    match fs::metadata(path.join("data.mdb")) {
        Ok(m) if m.is_file() => {}
        Ok(_) => return Err("data.mdb is not a regular file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"path": path, "status": "environment absent", "tables": []}));
        }
        Err(e) => return Err(e.into()),
    }
    let started = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let mut options = EnvOpenOptions::new();
    options.max_dbs(64);
    let env = unsafe { options.flags(EnvFlags::READ_ONLY).open(path)? };
    let txn = env.read_txn()?;
    let txn_id = txn.id();
    let deadline = Instant::now() + SAMPLE_TIME;
    let mut tables = Vec::new();
    for name in names {
        let Some(db) = env.open_database::<Bytes, Bytes>(&txn, Some(name))? else {
            tables.push(json!({"name": name, "status": "database absent"}));
            continue;
        };
        let stat = db.stat(&txn)?;
        let page_bytes = (stat.branch_pages + stat.leaf_pages + stat.overflow_pages) as u64
            * u64::from(stat.page_size);
        let mut n = 0usize;
        let mut total = 0u64;
        let mut min = u64::MAX;
        let mut max = 0u64;
        let mut key_min = usize::MAX;
        let mut key_max = 0usize;
        let mut value_min = usize::MAX;
        let mut value_max = 0usize;
        if Instant::now() < deadline {
            let mut rows = db.iter(&txn)?;
            while n < SAMPLE_RECORDS && Instant::now() < deadline {
                let Some(row) = rows.next() else { break };
                let (key, value) = row?;
                let bytes = key.len() as u64 + value.len() as u64;
                n += 1;
                total += bytes;
                min = min.min(bytes);
                max = max.max(bytes);
                key_min = key_min.min(key.len());
                key_max = key_max.max(key.len());
                value_min = value_min.min(value.len());
                value_max = value_max.max(value.len());
            }
        }
        tables.push(json!({
            "name": name, "status": "open", "records": stat.entries,
            "page_bytes": page_bytes, "page_size": stat.page_size,
            "sample": {
                "method": "first keys in LMDB order, not a random sample", "records": n,
                "complete": n == stat.entries, "key_value_bytes": total,
                "record_min_bytes": (n > 0).then_some(min),
                "record_max_bytes": (n > 0).then_some(max),
                "record_mean_bytes": (n > 0).then(|| total as f64 / n as f64),
                "key_min_bytes": (n > 0).then_some(key_min),
                "key_max_bytes": (n > 0).then_some(key_max),
                "value_min_bytes": (n > 0).then_some(value_min),
                "value_max_bytes": (n > 0).then_some(value_max),
            }
        }));
    }
    drop(txn);
    let metadata = fs::metadata(path.join("data.mdb"))?;
    Ok(json!({
        "path": path, "status": "read-only snapshot", "sample_started_unix_seconds": started,
        "transaction_id": txn_id, "file_length_bytes": metadata.len(),
        "file_allocated_bytes": metadata.blocks() * 512,
        "map_size_bytes": env.info().map_size, "tables": tables,
    }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err(
            "usage: erd-store-stats ENVIRONMENT_PATH DATABASE_NAMES_COMMA_SEPARATED".into(),
        );
    }
    let names: Vec<_> = args[2].split(',').collect();
    if names.iter().any(|n| {
        n.is_empty()
            || !n
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }) {
        return Err(
            "database names must contain only ASCII letters, digits, hyphens or underscores".into(),
        );
    }
    println!(
        "{}",
        serde_json::to_string(&inspect(Path::new(&args[1]), &names)?)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_counts_match_raw_lengths_and_do_not_change_data_file() {
        let dir = tempfile::tempdir().unwrap();
        {
            let env = unsafe { EnvOpenOptions::new().max_dbs(2).open(dir.path()).unwrap() };
            let mut txn = env.write_txn().unwrap();
            let db = env
                .create_database::<Bytes, Bytes>(&mut txn, Some("records"))
                .unwrap();
            db.put(&mut txn, b"a", b"123").unwrap();
            db.put(&mut txn, b"bb", b"12345").unwrap();
            txn.commit().unwrap();
        }
        let before = fs::read(dir.path().join("data.mdb")).unwrap();
        let result = inspect(dir.path(), &["records", "missing"]).unwrap();
        let table = &result["tables"][0];
        assert_eq!(table["records"], 2);
        assert_eq!(table["sample"]["key_value_bytes"], 11);
        assert_eq!(table["sample"]["complete"], true);
        assert_eq!(table["sample"]["record_min_bytes"], 4);
        assert_eq!(table["sample"]["record_max_bytes"], 7);
        assert_eq!(result["tables"][1]["status"], "database absent");
        assert_eq!(fs::read(dir.path().join("data.mdb")).unwrap(), before);
        assert!(!result.to_string().contains("12345"));
    }

    #[test]
    fn missing_environment_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing");
        assert_eq!(
            inspect(&path, &["records"]).unwrap()["status"],
            "environment absent"
        );
        assert!(!path.exists());
    }

    #[test]
    fn scan_is_bounded_and_empty_samples_have_no_false_minimum() {
        let dir = tempfile::tempdir().unwrap();
        {
            let env = unsafe { EnvOpenOptions::new().max_dbs(2).open(dir.path()).unwrap() };
            let mut txn = env.write_txn().unwrap();
            let db = env
                .create_database::<Bytes, Bytes>(&mut txn, Some("records"))
                .unwrap();
            env.create_database::<Bytes, Bytes>(&mut txn, Some("empty"))
                .unwrap();
            for i in 0..(SAMPLE_RECORDS + 10) {
                db.put(&mut txn, &i.to_be_bytes(), b"v").unwrap();
            }
            txn.commit().unwrap();
        }
        let result = inspect(dir.path(), &["records", "empty"]).unwrap();
        assert_eq!(result["tables"][0]["sample"]["records"], SAMPLE_RECORDS);
        assert_eq!(result["tables"][0]["sample"]["complete"], false);
        assert!(result["tables"][1]["sample"]["record_min_bytes"].is_null());
    }

    #[test]
    fn bincode_wrappers_and_fixed_record_formulas() {
        fn encoded<T: serde::Serialize>(value: &T) -> Vec<u8> { bincode::serialize(value).unwrap() }
        let hash = vec![0u8; 32];
        assert_eq!(encoded(&hash).len(), 40);
        assert_eq!(encoded(&encoded(&hash)).len() * 2, 96);
        let membership = encoded(&(0u64, 0i64, false));
        assert_eq!(encoded(&hash).len() + encoded(&membership).len(), 65);
        assert_eq!(
            encoded(&vec![0u8; 40]).len() + encoded(&encoded(&0u64)).len(),
            64
        );
        assert_eq!(encoded(&hash).len() + encoded(&b"tag".to_vec()).len(), 51);
        assert_eq!(
            encoded(&b"current-root".to_vec()).len() + encoded(&hash).len(),
            60
        );
        assert_eq!(encoded(&None::<i64>).len(), 1);
        assert_eq!(encoded(&Some(0i64)).len(), 9);
    }

    #[test]
    fn metadata_lifecycle_and_queue_formula_components() {
        #[derive(serde::Serialize)]
        struct Metadata {
            hash: Vec<u8>,
            parents: Vec<Vec<u8>>,
            sender: Vec<u8>,
            justifications: Vec<(Vec<u8>, Vec<u8>)>,
            bonds: Vec<(Vec<u8>, String)>,
            height: i64,
            sequence: i32,
            equivocation: Option<Vec<u8>>,
            directly_finalized: bool,
            finalized: bool,
            tolerance: f32,
            base: Vec<u8>,
        }
        let hash = vec![0u8; 32];
        let sender = vec![0u8; 65];
        let metadata = Metadata {
            hash: hash.clone(),
            parents: vec![hash.clone(); 3],
            sender: sender.clone(),
            justifications: vec![(sender.clone(), hash.clone()); 3],
            bonds: vec![(sender, "1000".into()); 3],
            height: 1,
            sequence: 1,
            equivocation: None,
            directly_finalized: false,
            finalized: false,
            tolerance: 0.0,
            base: hash.clone(),
        };
        assert_eq!(bincode::serialized_size(&metadata).unwrap(), 910);
        assert_eq!(
            bincode::serialized_size(&(0u32, 0u32, 0i64, hash.clone())).unwrap(),
            56
        );
        assert_eq!(
            bincode::serialized_size(&(0i64, hash, (0u32, false))).unwrap(),
            53
        );
        let queue_base = (
            Vec::<u8>::new(),
            0u64,
            0i64,
            0u64,
            0i64,
            0u32,
            None::<i64>,
            None::<i64>,
            0u32,
            0u32,
            None::<Vec<u8>>,
            None::<i64>,
            None::<String>,
        );
        assert_eq!(bincode::serialized_size(&queue_base).unwrap(), 57);
    }
}
