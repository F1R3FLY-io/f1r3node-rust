use std::collections::HashMap;
use std::fs::create_dir_all;
use std::path::{Path, PathBuf};

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
    // Live cold objects are consolidated into the authority environment.  The
    // old `cold` environment remains a read-only migration source.
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
        let legacy_v2_path = format!("{}/rspace/casper/v2", lmdb_path);

        let authority =
            crate::rspace::state::rspace_state_authority::RSpaceStateAuthority::open_lmdb(
                Path::new(&history_env_path),
                "rspace",
                Some(Path::new(&cold_env_path)),
                Some(Path::new(&legacy_v2_path)),
                map_size,
                RSPACE_MAX_DBS,
            )?;
        Ok(RSpaceStore::new(authority))
    } else {
        tracing::debug!("RSpace++ storage path {} does not exist, creating new", lmdb_path);
        create_dir_all(lmdb_path).expect("Failed to create RSpace++ storage directory");

        // Create subfolders consistent with rnode_db_mapping
        let history_env_path = format!("{}/rspace/history", lmdb_path);
        let cold_env_path = format!("{}/rspace/cold", lmdb_path);
        let legacy_v2_path = format!("{}/rspace/casper/v2", lmdb_path);
        create_dir_all(&history_env_path).expect("Failed to create RSpace++ history directory");
        let authority =
            crate::rspace::state::rspace_state_authority::RSpaceStateAuthority::open_lmdb(
                Path::new(&history_env_path),
                "rspace",
                Some(Path::new(&cold_env_path)),
                Some(Path::new(&legacy_v2_path)),
                map_size,
                RSPACE_MAX_DBS,
            )?;
        Ok(RSpaceStore::new(authority))
    }
}

pub fn close_rspace_store(rspace_store: RSpaceStore) { drop(rspace_store); }
