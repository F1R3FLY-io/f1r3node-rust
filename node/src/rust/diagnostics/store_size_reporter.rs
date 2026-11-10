use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use casper::rust::storage::rnode_key_value_store_manager::rnode_db_mapping;

use crate::rust::diagnostics::SYSTEM_METRICS_SOURCE;

const LMDB_DATA_FILE: &str = "data.mdb";

/// LMDB environment directories of the node, relative to the data directory
/// (e.g. `rspace/history`), as opened by the store manager.
fn lmdb_environments() -> BTreeSet<String> {
    rnode_db_mapping(None)
        .into_iter()
        .map(|(_, env)| env.name)
        .collect()
}

/// Size in bytes of the file at `path`; `None` when it does not exist yet.
fn file_size(path: &Path) -> std::io::Result<Option<u64>> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Size in bytes of each LMDB data file under `data_dir`, keyed by its
/// environment directory. Environments not created yet are left out; a store
/// whose size cannot be read is logged and left out.
pub fn store_sizes(data_dir: &Path) -> Vec<(String, u64)> {
    lmdb_environments()
        .into_iter()
        .filter_map(|env| {
            match file_size(&data_dir.join(&env).join(LMDB_DATA_FILE)) {
                Ok(size) => size.map(|bytes| (env, bytes)),
                Err(e) => {
                    tracing::warn!(store = %env, error = %e, "Cannot read the size of an LMDB store");
                    None
                }
            }
        })
        .collect()
}

pub fn start_store_size_reporter(data_dir: PathBuf, interval_duration: Duration) {
    std::thread::spawn(move || loop {
        for (store, bytes) in store_sizes(&data_dir) {
            metrics::gauge!(
                "store_size_bytes",
                "source" => SYSTEM_METRICS_SOURCE,
                "store" => store
            )
            .set(bytes as f64);
        }
        std::thread::sleep(interval_duration);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_data_file(data_dir: &Path, env: &str, len: usize) {
        let dir = data_dir.join(env);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LMDB_DATA_FILE), vec![0u8; len]).unwrap();
    }

    #[test]
    fn a_missing_file_has_no_size_but_other_errors_are_reported() {
        let data_dir = tempfile::tempdir().unwrap();
        let not_a_dir = data_dir.path().join("plain-file");
        std::fs::write(&not_a_dir, b"x").unwrap();

        assert_eq!(file_size(&data_dir.path().join("missing")).unwrap(), None);
        assert!(file_size(&not_a_dir.join(LMDB_DATA_FILE)).is_err());
    }

    #[test]
    fn an_unreadable_store_is_left_out_without_hiding_the_others() {
        let data_dir = tempfile::tempdir().unwrap();
        write_data_file(data_dir.path(), "rspace/cold", 8192);
        std::fs::create_dir_all(data_dir.path().join("rspace")).unwrap();
        std::fs::write(data_dir.path().join("rspace/history"), b"x").unwrap();

        assert_eq!(store_sizes(data_dir.path()), vec![(
            "rspace/cold".to_string(),
            8192
        )]);
    }

    #[test]
    fn reports_each_existing_environment_by_its_directory() {
        let data_dir = tempfile::tempdir().unwrap();
        write_data_file(data_dir.path(), "rspace/history", 4096);
        write_data_file(data_dir.path(), "rspace/cold", 8192);
        write_data_file(data_dir.path(), "reporting", 100);

        assert_eq!(store_sizes(data_dir.path()), vec![
            ("reporting".to_string(), 100),
            ("rspace/cold".to_string(), 8192),
            ("rspace/history".to_string(), 4096),
        ]);
    }

    #[test]
    fn ignores_files_outside_the_known_environments() {
        let data_dir = tempfile::tempdir().unwrap();
        write_data_file(data_dir.path(), "not-a-store", 4096);
        write_data_file(data_dir.path(), "rspace/history/nested", 4096);

        assert!(store_sizes(data_dir.path()).is_empty());
    }

    #[test]
    fn covers_every_store_directory_of_the_node() {
        let envs = lmdb_environments();
        for env in [
            "rspace/history",
            "rspace/cold",
            "eval/history",
            "eval/cold",
            "blockstorage",
            "dagstorage",
            "deploystorage",
            "casperbuffer",
            "reporting",
            "transaction",
        ] {
            assert!(envs.contains(env), "missing {env}");
        }
    }
}
