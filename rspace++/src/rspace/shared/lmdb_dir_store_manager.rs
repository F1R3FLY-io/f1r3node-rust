#![allow(clippy::new_ret_no_self)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{KeyValueStore, KeyValueStoreMutation, KvStoreError};
use tokio::sync::Mutex;

use super::key_value_store_manager::{KeyValueStoreManager, RSpaceBackendSelection};
use super::lmdb_store_manager::LmdbStoreManager;
use crate::rspace::persistence::{
    DagTransactionLimits, DurableAdmissionLimitsV1, DurableLimits,
    DurableRSpacePublicationLimitsV1, LogPackDecodeLimits, StateDifferenceLimits,
    TrustedSuccessorPolicyV1, ViewCheckpointLimitsV1,
};
use crate::rspace::state::rspace_state_authority::{LmdbAuthorityLayout, RSpaceStateAuthority};

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

pub const DURABLE_RSPACE_DIRECTORY_NAME: &str = "durable";

const DURABLE_DIFFERENCE_WORKLIST_ENTRIES: u64 = 1 << 20;
const DURABLE_DIFFERENCE_PATH_BYTES: u64 = 1 << 26;

const DURABLE_VIEW_CHECKPOINT_LIMITS: ViewCheckpointLimitsV1 = ViewCheckpointLimitsV1 {
    publication: DurableRSpacePublicationLimitsV1 {
        transaction: DagTransactionLimits::UNBOUNDED,
        recovery: DurableLimits::UNBOUNDED,
        wal: LogPackDecodeLimits::UNBOUNDED,
        admission: DurableAdmissionLimitsV1::UNBOUNDED,
        trusted: TrustedSuccessorPolicyV1::UNBOUNDED,
    },
    difference: StateDifferenceLimits {
        max_object_reads: u64::MAX,
        max_object_read_bytes: u64::MAX,
        max_worklist: DURABLE_DIFFERENCE_WORKLIST_ENTRIES,
        max_path_bytes: DURABLE_DIFFERENCE_PATH_BYTES,
    },
};

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
    envs: HashMap<PathBuf, Arc<Mutex<LmdbStoreManager>>>,
    authorities: HashMap<String, Arc<RSpaceStateAuthority>>,
    authority_prefixes: BTreeSet<String>,
    ordinary_opened: BTreeSet<PhysicalSlot>,
    backend_selections: HashMap<String, RSpaceBackendSelection>,
}

type PhysicalSlot = (PathBuf, String);

struct ResolvedDatabase {
    name: String,
    environment: PathBuf,
    config: LmdbEnvConfig,
}

struct ResolvedAuthority {
    layout: LmdbAuthorityLayout,
    environment: PathBuf,
    cold_source: PathBuf,
    legacy_v2: Option<PathBuf>,
    config: LmdbEnvConfig,
    writes: BTreeSet<PhysicalSlot>,
    reads: BTreeSet<PhysicalSlot>,
}

fn mapping_error(message: impl Into<String>) -> heed::Error {
    heed::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into()))
}

fn require_durable_prefix(db_prefix: &str) -> Result<(), heed::Error> {
    let mut components = Path::new(db_prefix).components();
    match (db_prefix, components.next(), components.next()) {
        ("eval", _, _) => {
            Err(mapping_error("the durable RSpace backend cannot be selected for the eval prefix"))
        }
        (_, Some(Component::Normal(name)), None) if name == db_prefix => Ok(()),
        _ => Err(mapping_error(format!(
            "durable RSpace prefix {db_prefix:?} is not exactly one normal path component"
        ))),
    }
}

pub(crate) fn resolve_lmdb_environment(path: &Path) -> Result<PathBuf, heed::Error> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);
                match std::fs::symlink_metadata(&resolved) {
                    Ok(_) => {
                        resolved = std::fs::canonicalize(&resolved)?;
                        if !std::fs::metadata(&resolved)?.is_dir() {
                            return Err(mapping_error(
                                "LMDB environment component is not a directory",
                            ));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(resolved)
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
        let authority_role = if let Some((prefix, domain)) = db_name.rsplit_once('-') {
            (matches!(domain, "history" | "roots" | "cold") &&
                (matches!(prefix, "rspace" | "eval") ||
                    self.managers_state
                        .lock()
                        .await
                        .authority_prefixes
                        .contains(prefix)))
            .then_some((prefix, domain))
        } else {
            None
        };
        let authority_role = match (authority_role, db_name.rsplit_once('-')) {
            (None, Some((prefix, domain @ ("history" | "roots" | "cold")))) => {
                let state = self.managers_state.lock().await;
                match state.backend_selections.get(prefix) {
                    Some(RSpaceBackendSelection::Durable) => Some((prefix, domain)),
                    Some(RSpaceBackendSelection::Legacy) | None => None,
                }
            }
            (role, _) => role,
        };
        if let Some((prefix, domain)) = authority_role {
            let authority = self.state_authority(prefix).await?;
            let inner = match domain {
                "history" => authority.history_store()?,
                "cold" => authority.cold_store()?,
                "roots" => authority.roots_store_for_inspection()?,
                _ => unreachable!("authority_owned_store admitted an unavailable domain"),
            };
            return Ok(Arc::new(ReadOnlyStore::new(inner)));
        }
        let (manager_arc, database_name) = {
            let mut state = self.managers_state.lock().await;
            let (mut databases, _) = self.resolve_mappings(
                state.authority_prefixes.iter().cloned(),
                &state.ordinary_opened,
            )?;
            let database = databases
                .remove(&db_name)
                .ok_or_else(|| mapping_error(format!("missing database mapping for {db_name}")))?;
            let manager = state
                .envs
                .entry(database.environment.clone())
                .or_insert_with(|| {
                    let manager = LmdbStoreManager::new(
                        database.environment.clone(),
                        database.config.max_env_size,
                        database.config.max_dbs,
                    );
                    Arc::new(Mutex::new(manager))
                })
                .clone();
            state
                .ordinary_opened
                .insert((database.environment, database.name.clone()));
            (manager, database.name)
        };

        let mut manager = manager_arc.lock().await;
        manager.store_literal(&database_name).await
    }

    async fn shutdown(&mut self) -> Result<(), heed::Error> {
        let managers: Vec<Arc<Mutex<LmdbStoreManager>>> = {
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
        if let Some(RSpaceBackendSelection::Durable) =
            state.backend_selections.get(db_prefix).copied()
        {
            self.resolve_mappings(
                state.authority_prefixes.iter().cloned(),
                &state.ordinary_opened,
            )?;
            let directory = self
                .dir_path
                .join(db_prefix)
                .join(DURABLE_RSPACE_DIRECTORY_NAME);
            std::fs::create_dir_all(&directory)?;
            let authority =
                RSpaceStateAuthority::durable(directory, DURABLE_VIEW_CHECKPOINT_LIMITS);
            state
                .authorities
                .insert(db_prefix.to_owned(), Arc::clone(&authority));
            return Ok(authority);
        }
        let prefixes = state
            .authority_prefixes
            .iter()
            .cloned()
            .chain(std::iter::once(db_prefix.to_owned()));
        let (_, mut layouts) = self.resolve_mappings(prefixes, &state.ordinary_opened)?;
        let resolved = layouts
            .remove(db_prefix)
            .ok_or_else(|| mapping_error("authority layout was not resolved"))?;
        let authority = RSpaceStateAuthority::open_resolved_lmdb(
            &resolved.environment,
            &resolved.layout,
            Some(&resolved.cold_source),
            resolved.legacy_v2.as_deref(),
            resolved.config.max_env_size,
            resolved.config.max_dbs,
        )?;
        state
            .authorities
            .insert(db_prefix.to_string(), Arc::clone(&authority));
        state.authority_prefixes.insert(db_prefix.to_owned());
        state
            .backend_selections
            .entry(db_prefix.to_owned())
            .or_insert(RSpaceBackendSelection::Legacy);
        Ok(authority)
    }

    async fn select_rspace_backend(
        &mut self,
        db_prefix: &str,
        selection: RSpaceBackendSelection,
    ) -> Result<(), heed::Error> {
        if let RSpaceBackendSelection::Durable = selection {
            require_durable_prefix(db_prefix)?;
        }
        let mut state = self.managers_state.lock().await;
        match state.backend_selections.get(db_prefix).copied() {
            Some(recorded) if recorded == selection => Ok(()),
            Some(recorded) => Err(mapping_error(format!(
                "RSpace backend of prefix {db_prefix} is fixed as {recorded:?} and cannot become \
                 {selection:?}"
            ))),
            None => {
                state
                    .backend_selections
                    .insert(db_prefix.to_owned(), selection);
                Ok(())
            }
        }
    }
}

impl LmdbDirStoreManager {
    fn resolve_mappings(
        &self,
        requested_prefixes: impl Iterator<Item = String>,
        ordinary_opened: &BTreeSet<PhysicalSlot>,
    ) -> Result<
        (BTreeMap<String, ResolvedDatabase>, BTreeMap<String, ResolvedAuthority>),
        heed::Error,
    > {
        let mut prefixes: BTreeSet<_> = requested_prefixes.collect();
        let mut databases = BTreeMap::new();
        let mut environments = BTreeMap::new();
        for (db, config) in &self.db_mapping {
            if let Some((prefix, _)) = authority_store_parts(db.id()) {
                prefixes.insert(prefix.to_owned());
            }
            let name = db.name_override.as_ref().unwrap_or(&db.id).clone();
            if name.contains('\0') {
                return Err(mapping_error("database name cannot contain NUL"));
            }
            let environment = resolve_lmdb_environment(&self.dir_path.join(&config.name))?;
            let limits = (config.max_env_size, config.max_dbs);
            if environments
                .insert(environment.clone(), limits)
                .is_some_and(|previous| previous != limits)
            {
                return Err(mapping_error(
                    "incompatible configurations resolve to the same LMDB environment",
                ));
            }
            if databases
                .insert(db.id.clone(), ResolvedDatabase {
                    name,
                    environment,
                    config: config.clone(),
                })
                .is_some()
            {
                return Err(mapping_error(format!(
                    "duplicate logical database mapping for {}",
                    db.id
                )));
            }
        }

        let mut layouts = BTreeMap::new();
        let mut authority_ids = BTreeSet::new();
        for prefix in prefixes {
            let ids =
                [format!("{prefix}-history"), format!("{prefix}-roots"), format!("{prefix}-cold")];
            let mut selected = Vec::with_capacity(3);
            for id in &ids {
                selected.push(databases.get(id).ok_or_else(|| {
                    mapping_error(format!("missing authority database mapping for {id}"))
                })?);
            }
            let [history, roots, cold] = selected.as_slice() else {
                unreachable!()
            };
            if history.environment != roots.environment {
                return Err(mapping_error(
                    "authority history and roots must share one LMDB environment",
                ));
            }
            let layout = LmdbAuthorityLayout::new(&prefix, [
                history.name.clone(),
                roots.name.clone(),
                cold.name.clone(),
            ])?;
            let writes: BTreeSet<_> = layout
                .names()
                .iter()
                .map(|name| (history.environment.clone(), name.clone()))
                .collect();
            let mut reads = BTreeSet::from([(cold.environment.clone(), cold.name.clone())]);
            if layout.legacy_compatible() {
                reads.insert((cold.environment.clone(), "db".to_owned()));
            }
            let legacy_v2 = if layout.legacy_compatible() && prefix == "rspace" {
                let directory = resolve_lmdb_environment(&self.dir_path.join("rspace/casper/v2"))?;
                for domain in ["history", "roots", "cold"] {
                    reads.insert((
                        resolve_lmdb_environment(&directory.join(domain))?,
                        "db".to_owned(),
                    ));
                }
                Some(directory)
            } else {
                None
            };
            authority_ids.extend(ids);
            layouts.insert(prefix, ResolvedAuthority {
                layout,
                environment: history.environment.clone(),
                cold_source: cold.environment.clone(),
                legacy_v2,
                config: history.config.clone(),
                writes,
                reads,
            });
        }
        for (prefix, layout) in &layouts {
            for (other_prefix, other) in &layouts {
                if prefix != other_prefix &&
                    (!layout.writes.is_disjoint(&other.writes) ||
                        !layout.writes.is_disjoint(&other.reads))
                {
                    return Err(mapping_error(
                        "authority layouts overlap another authority's physical databases",
                    ));
                }
            }
            if !ordinary_opened.is_disjoint(&layout.writes) ||
                !ordinary_opened.is_disjoint(&layout.reads)
            {
                return Err(mapping_error(
                    "authority layout aliases an already exposed writable database",
                ));
            }
            for (id, database) in &databases {
                if !authority_ids.contains(id) {
                    let slot = (database.environment.clone(), database.name.clone());
                    if layout.writes.contains(&slot) || layout.reads.contains(&slot) {
                        return Err(mapping_error(format!(
                            "writable mapping {id} aliases an authority database or migration \
                             source"
                        )));
                    }
                }
            }
        }
        Ok((databases, layouts))
    }

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
                authority_prefixes: BTreeSet::new(),
                ordinary_opened: BTreeSet::new(),
                backend_selections: HashMap::new(),
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
