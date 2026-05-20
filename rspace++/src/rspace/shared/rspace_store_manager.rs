use std::collections::HashMap;
use std::fs::create_dir_all;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

use super::env_cache;
use super::lmdb_dir_store_manager::{Db, LmdbEnvConfig};
use crate::rspace::rspace::RSpaceStore;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::shared::lmdb_dir_store_manager::LmdbDirStoreManager;

// max_dbs limit for shared envs (history + roots live in the same env).
// Increased to support parallel test execution with scoped database names.
const RSPACE_MAX_DBS: u32 = 10000;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/RholangCLI.scala
pub fn mk_rspace_store_manager(dir_path: PathBuf, map_size: usize) -> impl KeyValueStoreManager {
    let rspace_history_env_config = LmdbEnvConfig::new("history".to_owned(), map_size);
    let rspace_cold_env_config = LmdbEnvConfig::new("cold".to_owned(), map_size);
    let channel_env_config = LmdbEnvConfig::new("channels".to_owned(), map_size);

    let mut db_mapping = HashMap::new();
    db_mapping
        .insert(Db::new("rspace-history".to_string(), None), rspace_history_env_config.clone());
    db_mapping.insert(Db::new("rspace-roots".to_string(), None), rspace_history_env_config);
    db_mapping.insert(Db::new("rspace-cold".to_string(), None), rspace_cold_env_config);
    db_mapping.insert(Db::new("rspace-channels".to_string(), None), channel_env_config);

    LmdbDirStoreManager::new(dir_path, db_mapping)
}

pub fn get_or_create_rspace_store(
    lmdb_path: &str,
    map_size: usize,
) -> Result<RSpaceStore, heed::Error> {
    if Path::new(lmdb_path).exists() {
        tracing::debug!("RSpace++ storage path {} already exists (reopening)", lmdb_path);

        // In Scala (and Rust rnode_db_mapping), RSpace envs are in subfolders:
        // rspace/history and rspace/cold. history and roots share the same env.
        let history_env_path = format!("{}/rspace/history", lmdb_path);
        let cold_env_path = format!("{}/rspace/cold", lmdb_path);

        let history_store = open_lmdb_store(&history_env_path, "rspace-history", map_size)?;
        let roots_store = open_lmdb_store(&history_env_path, "rspace-roots", map_size)?;
        let cold_store = open_lmdb_store(&cold_env_path, "rspace-cold", map_size)?;

        Ok(RSpaceStore {
            history: Arc::new(history_store),
            roots: Arc::new(roots_store),
            cold: Arc::new(cold_store),
        })
    } else {
        tracing::debug!("RSpace++ storage path {} does not exist, creating new", lmdb_path);
        create_dir_all(lmdb_path).expect("Failed to create RSpace++ storage directory");

        // Create subfolders consistent with rnode_db_mapping
        let history_env_path = format!("{}/rspace/history", lmdb_path);
        let cold_env_path = format!("{}/rspace/cold", lmdb_path);
        create_dir_all(&history_env_path).expect("Failed to create RSpace++ history directory");
        create_dir_all(&cold_env_path).expect("Failed to create RSpace++ cold directory");

        let history_store = create_lmdb_store(&history_env_path, "rspace-history", map_size)?;
        let roots_store = create_lmdb_store(&history_env_path, "rspace-roots", map_size)?;
        let cold_store = create_lmdb_store(&cold_env_path, "rspace-cold", map_size)?;

        Ok(RSpaceStore {
            history: Arc::new(history_store),
            roots: Arc::new(roots_store),
            cold: Arc::new(cold_store),
        })
    }
}

pub fn close_rspace_store(rspace_store: RSpaceStore) { drop(rspace_store); }

fn create_lmdb_store(
    lmdb_path: &str,
    db_name: &str,
    max_env_size: usize,
) -> Result<LmdbKeyValueStore, heed::Error> {
    // Route through env_cache so a second create_lmdb_store call for the same
    // path (e.g. rspace-history + rspace-roots both in rspace/history) reuses
    // the same Env handle instead of tripping EnvAlreadyOpened under heed 0.22.
    let env = env_cache::get_or_open_env(Path::new(lmdb_path), max_env_size, RSPACE_MAX_DBS)?;
    let mut wtxn = env.write_txn()?;
    let db = env.create_database(&mut wtxn, Some(db_name))?;
    wtxn.commit()?;
    Ok(LmdbKeyValueStore::new(env, db))
}

fn open_lmdb_store(
    lmdb_path: &str,
    db_name: &str,
    max_env_size: usize,
) -> Result<LmdbKeyValueStore, heed::Error> {
    let env = env_cache::get_or_open_env(Path::new(lmdb_path), max_env_size, RSPACE_MAX_DBS)?;
    let rtxn = env.read_txn()?;
    let db = env.open_database(&rtxn, Some(db_name))?;
    drop(rtxn);
    match db {
        Some(open_db) => Ok(LmdbKeyValueStore::new(env, open_db)),
        None => panic!("\nFailed to open database: {}", db_name),
    }
}
