#![allow(clippy::new_ret_no_self)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::SerdeBincode;
use heed::{Database, Env};
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;
use tokio::sync::Mutex;

use crate::rspace::shared::env_cache;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

// See shared/src/main/scala/coop/rchain/store/LmdbStoreManager.scala
pub struct LmdbStoreManager {
    dir_path: PathBuf,
    max_env_size: usize,
    max_dbs: u32,
    env: Arc<Mutex<Option<Arc<Env>>>>,
    dbs: Arc<Mutex<HashMap<String, DbEnv>>>,
}

#[derive(Clone)]
struct DbEnv {
    env: Arc<Env>,
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

        // Obtain a shared Arc<Env> for this path from the workspace-level
        // env_cache. Multiple LmdbStoreManager instances targeting the same
        // dir_path (e.g. the casper shared-LMDB test pattern) all observe the
        // same Env via the cache.
        let env = {
            let mut env_slot = self.env.lock().await;
            if env_slot.is_none() {
                *env_slot = Some(env_cache::get_or_open_env(
                    &self.dir_path,
                    self.max_env_size,
                    self.max_dbs,
                )?);
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
        // Drop our Arc<Env> clones. The env_cache holds Weak refs, so when
        // the last consumer drops, the cache entry auto-evicts on the next
        // lookup and heed closes the LMDB file handles.
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
