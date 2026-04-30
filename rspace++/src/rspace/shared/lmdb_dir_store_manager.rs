use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use shared::rust::store::key_value_store::KeyValueStore;
use tokio::sync::Mutex;

use super::key_value_store_manager::KeyValueStoreManager;
use super::lmdb_store_manager::LmdbStoreManager;

/**
 * Specification for LMDB database: unique identifier and database name
 *
 * @param id unique identifier
 * @param nameOverride name to use as database name instead of [[id]]
 */
#[derive(Ord, PartialOrd, PartialEq, Eq, Hash)]
pub struct Db {
    id: String,
    name_override: Option<String>,
}

impl Db {
    pub fn new(id: String, name_override: Option<String>) -> Self { Db { id, name_override } }

    pub fn id(&self) -> &str { &self.id }
}

// Mega, giga and tera bytes
pub const MB: usize = 1024 * 1024;
pub const GB: usize = 1024 * MB;
pub const TB: usize = 1024 * GB;

#[derive(Clone)]
pub struct LmdbEnvConfig {
    pub name: String,
    pub max_env_size: usize,
    pub max_dbs: u32,
}

impl LmdbEnvConfig {
    pub fn new(name: String, max_env_size: usize) -> Self {
        LmdbEnvConfig {
            name,
            max_env_size,
            max_dbs: 20,
        }
    }

    pub fn with_max_dbs(mut self, max_dbs: u32) -> Self {
        self.max_dbs = max_dbs;
        self
    }
}

// See shared/src/main/scala/coop/rchain/store/LmdbDirStoreManager.scala
// The idea for this class is to manage multiple of key-value lmdb databases.
// For LMDB this allows control which databases are part of the same environment
// (file).
pub struct LmdbDirStoreManager {
    dir_path: PathBuf,
    db_mapping: HashMap<Db, LmdbEnvConfig>,
    managers_state: Arc<Mutex<StoreState>>,
}

struct StoreState {
    // Cached LmdbStoreManagers keyed by env config name. Each Arc<Mutex<...>>
    // is created once per env config and reused for every database that shares
    // that env. heed 0.22 rejects duplicate env opens of the same directory
    // with EnvAlreadyOpened, so the cache must survive across store() calls.
    envs: HashMap<String, Arc<Mutex<Box<dyn KeyValueStoreManager>>>>,
}

#[async_trait]
impl KeyValueStoreManager for LmdbDirStoreManager {
    async fn store(&mut self, db_name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
        let (database_name, man_cfg) = {
            let db_instance_mapping: HashMap<&String, (&Db, &LmdbEnvConfig)> = self
                .db_mapping
                .iter()
                .map(|(db, cfg)| (&db.id, (db, cfg)))
                .collect();

            let (db, cfg) = db_instance_mapping.get(&db_name).ok_or_else(|| {
                heed::Error::Io(std::io::Error::other(format!(
                    "LMDB_Dir_Store_Manager: Key {} was not found",
                    db_name
                )))
            })?;

            (
                db.name_override.clone().unwrap_or(db.id.clone()),
                (*cfg).clone(),
            )
        };

        let manager_arc = {
            let mut state = self.managers_state.lock().await;
            state
                .envs
                .entry(man_cfg.name.clone())
                .or_insert_with(|| {
                    let manager = LmdbStoreManager::new(
                        self.dir_path.join(&man_cfg.name),
                        man_cfg.max_env_size,
                        man_cfg.max_dbs,
                    );
                    Arc::new(Mutex::new(manager))
                })
                .clone()
        };

        let mut manager = manager_arc.lock().await;
        manager.store(database_name).await
    }

    async fn shutdown(&mut self) -> Result<(), heed::Error> {
        let managers: Vec<Arc<Mutex<Box<dyn KeyValueStoreManager>>>> = {
            let mut state = self.managers_state.lock().await;
            state.envs.drain().map(|(_, m)| m).collect()
        };

        for manager_arc in managers {
            let mut manager = manager_arc.lock().await;
            let _ = manager.shutdown().await;
        }

        Ok(())
    }
}

impl LmdbDirStoreManager {
    pub fn new(
        dir_path: PathBuf,
        db_instance_mapping: HashMap<Db, LmdbEnvConfig>,
    ) -> impl KeyValueStoreManager {
        LmdbDirStoreManager {
            dir_path,
            db_mapping: db_instance_mapping,
            managers_state: Arc::new(Mutex::new(StoreState {
                envs: HashMap::new(),
            })),
        }
    }
}

// Ensure cached managers (and their LMDB envs) are dropped when this manager
// is dropped.
impl Drop for LmdbDirStoreManager {
    fn drop(&mut self) {
        if let Ok(mut state) = self.managers_state.try_lock() {
            state.envs.clear();
        }
    }
}
