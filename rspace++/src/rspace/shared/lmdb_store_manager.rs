#![allow(clippy::new_ret_no_self)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use async_trait::async_trait;
use heed::types::SerdeBincode;
use heed::{Database, Env, EnvOpenOptions};
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;
use tokio::sync::Mutex;

use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

// Process-global cache of opened LMDB environments, keyed by directory path.
//
// Lifecycle: entries are inserted on first open for a given path and are NOT
// evicted on shutdown(). This is intentional. heed 0.22 rejects duplicate env
// opens of the same directory with EnvAlreadyOpened, so any code that reopens
// the same path (the casper shared-LMDB test pattern, or a manager recreated
// after shutdown) must observe the same Env handle. Evicting on shutdown
// would be unsafe whenever an LmdbKeyValueStore still holds an Env clone:
// the next open would attempt env_builder.open() and trip EnvAlreadyOpened.
//
// Bounded growth: one entry per unique LMDB directory path. In production
// this is a small fixed set (history, cold, channels). In tests, per-test
// tempdirs accumulate for the process lifetime, but test processes are
// short-lived, so the bound is acceptable.
//
// Locking: a single StdMutex guards the HashMap. The critical section on the
// hot path is HashMap::get + Env::clone (an Arc bump). The slow path
// (env_builder.open) only runs once per unique path for the process
// lifetime, so per-path lock granularity would add complexity without
// measurable benefit.
//
// heed's Env is internally refcounted; clones are cheap and safe to share
// across threads.
static ENV_CACHE: OnceLock<StdMutex<HashMap<PathBuf, Env>>> = OnceLock::new();

fn get_or_open_env(dir_path: &Path, max_env_size: usize, max_dbs: u32) -> Result<Env, heed::Error> {
    let cache = ENV_CACHE.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut cache_lock = cache.lock().expect("LMDB env cache mutex poisoned");
    if let Some(env) = cache_lock.get(dir_path) {
        return Ok(env.clone());
    }

    fs::create_dir_all(dir_path)?;
    let mut env_builder = EnvOpenOptions::new();
    env_builder.map_size(max_env_size);
    env_builder.max_dbs(max_dbs);
    env_builder.max_readers(2048);
    // SAFETY: heed::EnvOpenOptions::open is unsafe because LMDB mmaps the
    // data file. Callers must guarantee that:
    //   1. No other process modifies the file while this Env is alive.
    //   2. The path is on a local filesystem (LMDB does not support NFS or similar
    //      networked storage without explicit configuration).
    //   3. The same Env is not opened more than once in this process.
    // We uphold (3) via ENV_CACHE: this call only runs after a cache miss
    // under the cache mutex, so no concurrent open of the same path is
    // possible. (1) and (2) are responsibilities of the caller's deployment
    // configuration and are documented at the workspace level.
    let env = unsafe { env_builder.open(dir_path)? };
    cache_lock.insert(dir_path.to_path_buf(), env.clone());
    Ok(env)
}

// See shared/src/main/scala/coop/rchain/store/LmdbStoreManager.scala
pub struct LmdbStoreManager {
    dir_path: PathBuf,
    max_env_size: usize,
    max_dbs: u32,
    env: Arc<Mutex<Option<Env>>>,
    dbs: Arc<Mutex<HashMap<String, DbEnv>>>,
}

#[derive(Clone)]
struct DbEnv {
    env: Env,
    db: Database<SerdeBincode<Vec<u8>>, SerdeBincode<Vec<u8>>>,
}

impl LmdbStoreManager {
    pub fn new(
        dir_path: PathBuf,
        max_env_size: usize,
        max_dbs: u32,
    ) -> Box<dyn KeyValueStoreManager> {
        Box::new(LmdbStoreManager {
            dir_path,
            max_env_size,
            max_dbs,
            env: Arc::new(Mutex::new(None)),
            dbs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn get_current_env(&mut self, db_name: &str) -> Result<DbEnv, heed::Error> {
        {
            let dbs = self.dbs.lock().await;
            if let Some(db_env) = dbs.get(db_name) {
                return Ok(db_env.clone());
            }
        }

        // Obtain a handle to the shared env for this path. Multiple
        // LmdbStoreManager instances targeting the same dir_path (e.g. the
        // casper shared-LMDB test pattern) all reuse the same Env clone.
        let env = {
            let mut env_slot = self.env.lock().await;
            if env_slot.is_none() {
                *env_slot = Some(get_or_open_env(&self.dir_path, self.max_env_size, self.max_dbs)?);
            }
            env_slot.as_ref().unwrap().clone()
        };

        // Open or create the named database within a write transaction.
        let mut wtxn = env.write_txn()?;
        let db = env.create_database(&mut wtxn, Some(db_name))?;
        wtxn.commit()?;

        let mut dbs = self.dbs.lock().await;
        let db_env = DbEnv { env, db };
        dbs.insert(db_name.to_string(), db_env.clone());
        Ok(db_env)
    }
}

#[async_trait]
impl KeyValueStoreManager for LmdbStoreManager {
    async fn store(&mut self, name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
        let db_env = self.get_current_env(&name).await?;
        Ok(Arc::new(LmdbKeyValueStore::new(db_env.env, db_env.db)))
    }

    async fn shutdown(&mut self) -> Result<(), heed::Error> {
        // Drop all cached DbEnv handles and the underlying env. heed's Env
        // Drop closes the LMDB file handles when the last clone is released.
        let mut dbs = self.dbs.lock().await;
        dbs.clear();
        let mut env_slot = self.env.lock().await;
        *env_slot = None;
        Ok(())
    }
}

// Ensures LMDB environment is closed when the manager is dropped.
impl Drop for LmdbStoreManager {
    fn drop(&mut self) {
        if let Ok(mut dbs) = self.dbs.try_lock() {
            dbs.clear();
        }
        if let Ok(mut env_slot) = self.env.try_lock() {
            *env_slot = None;
        }
    }
}
