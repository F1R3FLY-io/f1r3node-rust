#![allow(clippy::new_ret_no_self)]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{KeyValueStore, KeyValueStoreMutation, KvStoreError};
use tokio::sync::Mutex;

use super::key_value_store_manager::KeyValueStoreManager;
use super::lmdb_store_manager::LmdbStoreManager;
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

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
    authorities: HashMap<String, Arc<RSpaceStateAuthority>>,
}

#[derive(Clone)]
pub(super) struct ReadOnlyStore {
    inner: Arc<dyn KeyValueStore>,
}

impl ReadOnlyStore {
    pub(super) fn new(inner: Arc<dyn KeyValueStore>) -> Self { Self { inner } }
}

impl KeyValueStore for ReadOnlyStore {
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        self.inner.get(keys)
    }

    fn put(&self, _pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        Err(authority_mutation_error())
    }

    fn compare_and_swap_batch(
        &self,
        _conditions: &[(ByteBuffer, Option<ByteBuffer>)],
        _mutations: &[KeyValueStoreMutation],
    ) -> Result<bool, KvStoreError> {
        Err(authority_mutation_error())
    }

    fn delete(&self, _keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        Err(authority_mutation_error())
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.inner.iterate(f)
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.inner.to_map()
    }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> Result<usize, KvStoreError> { self.inner.size_bytes() }
}

fn authority_mutation_error() -> KvStoreError {
    KvStoreError::InvalidArgument(
        "RSpace authority databases are exposed read-only; mutate through RSpaceStateAuthority"
            .to_string(),
    )
}

pub(super) fn authority_store_parts(name: &str) -> Option<(&str, &str)> {
    let (prefix, domain) = name.rsplit_once('-')?;
    (matches!(prefix, "rspace" | "eval") && matches!(domain, "history" | "roots" | "cold"))
        .then_some((prefix, domain))
}

#[async_trait]
impl KeyValueStoreManager for LmdbDirStoreManager {
    async fn store(&mut self, db_name: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
        if let Some((prefix, domain)) = authority_store_parts(&db_name) {
            let authority = self.state_authority(prefix).await?;
            let inner = match domain {
                "history" => authority.history_store(),
                "cold" => authority.cold_store(),
                "roots" => authority.roots_store_for_inspection(),
                _ => unreachable!("authority_owned_store admitted an unavailable domain"),
            };
            return Ok(Arc::new(ReadOnlyStore::new(inner)));
        }
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

            (db.name_override.clone().unwrap_or(db.id.clone()), (*cfg).clone())
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
            state.authorities.clear();
            state.envs.drain().map(|(_, m)| m).collect()
        };

        for manager_arc in managers {
            let mut manager = manager_arc.lock().await;
            let _ = manager.shutdown().await;
        }

        Ok(())
    }

    async fn state_authority(
        &mut self,
        db_prefix: &str,
    ) -> Result<Arc<RSpaceStateAuthority>, heed::Error> {
        let mut state = self.managers_state.lock().await;
        if let Some(authority) = state.authorities.get(db_prefix) {
            return Ok(Arc::clone(authority));
        }
        let history_name = format!("{db_prefix}-history");
        let cold_name = format!("{db_prefix}-cold");
        let history_cfg = self
            .db_mapping
            .iter()
            .find(|(db, _)| db.id() == history_name)
            .map(|(_, cfg)| cfg.clone())
            .ok_or_else(|| {
                heed::Error::Io(std::io::Error::other(format!(
                    "missing authority history mapping for {db_prefix}"
                )))
            })?;
        let legacy_cold = self
            .db_mapping
            .iter()
            .find(|(db, _)| db.id() == cold_name)
            .map(|(_, cfg)| self.dir_path.join(&cfg.name));
        let authority = RSpaceStateAuthority::open_lmdb(
            &self.dir_path.join(&history_cfg.name),
            db_prefix,
            legacy_cold.as_deref(),
            (db_prefix == "rspace")
                .then(|| self.dir_path.join("rspace/casper/v2"))
                .as_deref(),
            history_cfg.max_env_size,
            history_cfg.max_dbs,
        )?;
        state
            .authorities
            .insert(db_prefix.to_string(), Arc::clone(&authority));
        Ok(authority)
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
                authorities: HashMap::new(),
            })),
        }
    }
}

// Ensure cached managers (and their LMDB envs) are dropped when this manager
// is dropped.
impl Drop for LmdbDirStoreManager {
    fn drop(&mut self) {
        if let Ok(mut state) = self.managers_state.try_lock() {
            state.authorities.clear();
            state.envs.clear();
        }
    }
}
