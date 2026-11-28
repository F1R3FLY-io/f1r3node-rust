use std::error::Error;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;

use heed::types::SerdeBincode;
use rspace_plus_plus::rspace::history::cold_store_codec::ColdStoreCodec;
use serde_json::{Value, json};
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

fn inventory(path: &Path) -> Result<Value, Box<dyn Error>> {
    let env = Arc::new(unsafe {
        heed::EnvOpenOptions::new()
            .max_dbs(8)
            .flags(heed::EnvFlags::READ_ONLY)
            .open(path)?
    });
    let txn = env.read_txn()?;
    let db = env
        .open_database::<SerdeBincode<Vec<u8>>, SerdeBincode<Vec<u8>>>(&txn, Some("rspace-cold"))?
        .ok_or("missing cold database")?;
    txn.commit()?;
    let physical = Arc::new(LmdbKeyValueStore::new(env, db));
    let logical = ColdStoreCodec::new(physical.clone(), false);
    let mut cursor = None;
    let mut rows = 0u64;
    let mut compressed = 0u64;
    let mut physical_bytes = 0u64;
    let mut canonical_bytes = 0u64;
    loop {
        let page = physical.scan_page(cursor.as_deref(), 16)?;
        for (key, value) in page.rows {
            let canonical = logical.get_one(&key)?.ok_or("cold row disappeared")?;
            rows += 1;
            compressed += u64::from(value.starts_with(b"F1CLZ4\0\0"));
            physical_bytes += (key.len() + value.len() + 16) as u64;
            canonical_bytes += (key.len() + canonical.len() + 16) as u64;
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    let metadata = std::fs::metadata(path.join("data.mdb"))?;
    Ok(
        json!({"rows":rows,"compressed_rows":compressed,"canonical_row_bytes":canonical_bytes,"physical_row_bytes":physical_bytes,"live_page_bytes":logical.try_size_bytes()?,"file_bytes":metadata.len(),"allocated_file_bytes":metadata.blocks()*512}),
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        return Err("usage: cold_store_inventory STOPPED_COLD_ENVIRONMENT".into());
    };
    println!("{}", serde_json::to_string_pretty(&inventory(Path::new(path))?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::history::cold_store::{
        ContinuationsLeaf, DataLeaf, PersistedData,
    };

    use super::*;

    #[test]
    fn read_only_reopen_counts_mixed_rows_without_changing_the_database() {
        for compressed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            {
                let env = Arc::new(unsafe {
                    heed::EnvOpenOptions::new()
                        .max_dbs(8)
                        .map_size(16 * 1024 * 1024)
                        .open(directory.path())
                        .unwrap()
                });
                let mut txn = env.write_txn().unwrap();
                let db = env.create_database(&mut txn, Some("rspace-cold")).unwrap();
                txn.commit().unwrap();
                let store =
                    ColdStoreCodec::new(Arc::new(LmdbKeyValueStore::new(env, db)), compressed);
                for variant in [0, 1] {
                    let leaf = DataLeaf {
                        bytes: vec![variant; 4096],
                    };
                    let key = Blake2b256Hash::new(&bincode::serialize(&leaf).unwrap()).bytes();
                    let value = if variant == 0 {
                        PersistedData::Data(leaf)
                    } else {
                        PersistedData::Continuations(ContinuationsLeaf { bytes: leaf.bytes })
                    };
                    store
                        .put_one(key, bincode::serialize(&value).unwrap())
                        .unwrap();
                }
            }
            let file = directory.path().join("data.mdb");
            let before = std::fs::read(&file).unwrap();
            let report = inventory(directory.path()).unwrap();
            assert_eq!(report["rows"], 2);
            assert_eq!(report["compressed_rows"], u64::from(compressed));
            if compressed {
                assert!(
                    report["canonical_row_bytes"].as_u64().unwrap() >
                        report["physical_row_bytes"].as_u64().unwrap()
                );
            } else {
                assert_eq!(report["canonical_row_bytes"], report["physical_row_bytes"]);
            }
            assert_eq!(std::fs::read(file).unwrap(), before);
        }
    }
}
