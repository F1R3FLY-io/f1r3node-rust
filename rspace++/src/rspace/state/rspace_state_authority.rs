//! Exclusive authority for RSpace's immutable content-addressed state.
//!
//! History nodes, cold leaves, and the verified-root catalog are deliberately
//! owned together.  Callers receive read/content-addressed-write facets, never
//! the mutation-capable backing databases.  Root publication consumes an
//! unforgeable capability produced either by a completed local checkpoint or
//! by a sealed import; arbitrary digests cannot acquire publication authority.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use heed::types::SerdeBincode;
use heed::{Database, Env};
use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{
    KeyValueStore, KeyValueStoreMutation, KvStoreError, validate_atomic_batch,
};

use crate::rspace::errors::{AuthorityBackendMismatch, HistoryError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{ColdLeafAddress, validate_cold_leaf};
use crate::rspace::history::instances::radix_history::{PreparedHistoryCheckpoint, RadixHistory};
use crate::rspace::history::roots_store::CurrentRoot;
use crate::rspace::shared::env_cache;
use crate::rspace::shared::trie_exporter::NodePath;
use crate::rspace::state::closure_verifier::{RecordValidator, verify_state_closure};
use crate::rspace::state::import_session::{
    StateImportActivation, StateImportError, StateImportPublication, StateImportPublicationMode,
    StateImportPublicationWitness, StateImportSpec,
};
use crate::rspace::state::legacy_state_migration::{complete_legacy_state, migrate_legacy_state};

type BytesDb = Database<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>>;

const VERIFIED_ROOT_KEY_PREFIX: &[u8] = b"rspace-root-catalog/v1/verified/";
const VERIFIED_ROOT_VALUE_PREFIX: &[u8] = b"rspace-verified-root\0\x01";
const CURRENT_ROOT_KEY: &[u8] = b"rspace-root-catalog/v1/current";
const CURRENT_ROOT_VALUE_PREFIX: &[u8] = b"rspace-current-root\0\x01";
const LEGACY_CURRENT_ROOT_KEY: &[u8] = b"current-root";
const ROOT_HASH_LENGTH: usize = 32;

pub(crate) struct LmdbAuthorityLayout {
    names: [String; 3],
    namespace: Arc<[u8]>,
    legacy_compatible: bool,
}

impl LmdbAuthorityLayout {
    pub(crate) fn for_prefix(prefix: &str) -> Result<Self, heed::Error> {
        Self::new(prefix, [
            format!("{prefix}-history"),
            format!("{prefix}-roots"),
            format!("{prefix}-cold"),
        ])
    }

    pub(crate) fn new(prefix: &str, names: [String; 3]) -> Result<Self, heed::Error> {
        let invalid = |message| {
            heed::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, message))
        };
        if names.iter().any(|name| name.contains('\0')) {
            return Err(invalid("authority database names cannot contain NUL"));
        }
        if names[0] == names[1] || names[0] == names[2] || names[1] == names[2] {
            return Err(invalid("authority database roles must have distinct physical names"));
        }
        let legacy_compatible = ["history", "roots", "cold"]
            .into_iter()
            .zip(&names)
            .all(|(role, name)| *name == format!("{prefix}-{role}"));
        let namespace = if legacy_compatible {
            u32::try_from(prefix.len())
                .map_err(|_| invalid("migration namespace exceeds its manifest length field"))?;
            prefix.as_bytes().to_vec()
        } else {
            const TAG: &[u8] = b"\0rspace-authority-layout\0\x01";
            let fields = [prefix, names[0].as_str(), names[1].as_str(), names[2].as_str()];
            let mut encoded_len = TAG.len();
            for field in fields {
                u64::try_from(field.len())
                    .map_err(|_| invalid("authority namespace field exceeds u64"))?;
                encoded_len = encoded_len
                    .checked_add(8)
                    .and_then(|length| length.checked_add(field.len()))
                    .ok_or_else(|| invalid("authority namespace length overflow"))?;
            }
            u32::try_from(encoded_len)
                .map_err(|_| invalid("migration namespace exceeds its manifest length field"))?;
            let mut encoded = Vec::new();
            encoded
                .try_reserve_exact(encoded_len)
                .map_err(|_| invalid("cannot allocate authority namespace"))?;
            encoded.extend_from_slice(TAG);
            for field in fields {
                let length = u64::try_from(field.len())
                    .map_err(|_| invalid("authority namespace field exceeds u64"))?;
                encoded.extend_from_slice(&length.to_be_bytes());
                encoded.extend_from_slice(field.as_bytes());
            }
            encoded
        };
        Ok(Self {
            names,
            namespace: namespace.into(),
            legacy_compatible,
        })
    }

    pub(crate) fn names(&self) -> &[String; 3] { &self.names }

    pub(crate) fn namespace(&self) -> &[u8] { &self.namespace }

    pub(crate) fn legacy_compatible(&self) -> bool { self.legacy_compatible }

    pub(crate) fn cold_source_names(&self) -> [&str; 2] { [&self.names[2], "db"] }
}

struct AuthorityIdentity;

#[derive(Clone, Copy)]
enum ObjectKind {
    History,
    Cold,
    Roots,
}

struct MemoryState {
    history: BTreeMap<ByteBuffer, ByteBuffer>,
    cold: BTreeMap<ByteBuffer, ByteBuffer>,
    roots: BTreeMap<ByteBuffer, ByteBuffer>,
}

struct LmdbState {
    namespace: Arc<[u8]>,
    env: Arc<Env>,
    history: BytesDb,
    cold: BytesDb,
    roots: BytesDb,
}

enum AuthorityBackend {
    Memory(Mutex<MemoryState>),
    Lmdb(LmdbState),
}

/// The sole mutation authority for the three persistent RSpace state domains.
pub struct RSpaceStateAuthority {
    identity: Arc<AuthorityIdentity>,
    backend: AuthorityBackend,
}

/// Affine import staging authority.  It is consumed by `seal_import`.
pub struct StagedImport {
    identity: Arc<AuthorityIdentity>,
    spec: StateImportSpec,
    expected_generation: Option<u64>,
    pending_paths: BTreeSet<NodePath>,
}

/// Proof that every requested imported root had a complete verified closure.
/// It is consumed by `publish_import`.
pub struct SealedImportedRoots {
    identity: Arc<AuthorityIdentity>,
    spec: StateImportSpec,
    expected_generation: Option<u64>,
}

/// Proof that a root is already a member of this authority's verified catalog.
pub struct KnownVerifiedRoot {
    identity: Arc<AuthorityIdentity>,
    root: Blake2b256Hash,
}

impl StagedImport {
    pub fn spec(&self) -> &StateImportSpec { &self.spec }
}

impl SealedImportedRoots {
    pub fn spec(&self) -> &StateImportSpec { &self.spec }
}

impl KnownVerifiedRoot {
    pub fn root(&self) -> &Blake2b256Hash { &self.root }
}

impl RSpaceStateAuthority {
    pub fn in_memory() -> Arc<Self> {
        let authority = Arc::new(Self {
            identity: Arc::new(AuthorityIdentity),
            backend: AuthorityBackend::Memory(Mutex::new(MemoryState {
                history: BTreeMap::new(),
                cold: BTreeMap::new(),
                roots: BTreeMap::new(),
            })),
        });
        authority
            .initialize_empty_root()
            .expect("new in-memory authority must initialize its empty root");
        authority
    }

    /// Opens all live state databases in one LMDB environment and migrates an
    /// optional legacy cold environment without modifying it.
    pub(crate) fn open_lmdb(
        authority_dir: &Path,
        db_prefix: &str,
        split_cold_dir: Option<&Path>,
        legacy_v2_dir: Option<&Path>,
        map_size: usize,
        max_dbs: u32,
    ) -> Result<Arc<Self>, heed::Error> {
        Self::open_resolved_lmdb(
            authority_dir,
            &LmdbAuthorityLayout::for_prefix(db_prefix)?,
            split_cold_dir,
            legacy_v2_dir,
            map_size,
            max_dbs,
        )
    }

    pub(crate) fn open_resolved_lmdb(
        authority_dir: &Path,
        layout: &LmdbAuthorityLayout,
        split_cold_dir: Option<&Path>,
        legacy_v2_dir: Option<&Path>,
        map_size: usize,
        max_dbs: u32,
    ) -> Result<Arc<Self>, heed::Error> {
        if legacy_v2_dir.is_some() &&
            (!layout.legacy_compatible() || layout.namespace() != b"rspace")
        {
            return Err(heed::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "legacy v2 migration requires the unscoped RSpace layout",
            )));
        }
        let resolved =
            crate::rspace::shared::lmdb_dir_store_manager::resolve_lmdb_environment(authority_dir)?;
        let env = env_cache::get_or_open_env(&resolved, map_size, max_dbs)?;
        let mut txn = env.write_txn()?;
        let history = env.create_database(&mut txn, Some(&layout.names[0]))?;
        let cold = env.create_database(&mut txn, Some(&layout.names[2]))?;
        let roots = env.create_database(&mut txn, Some(&layout.names[1]))?;
        txn.commit()?;

        let state = LmdbState {
            namespace: Arc::clone(&layout.namespace),
            env,
            history,
            cold,
            roots,
        };
        migrate_legacy_state(
            Arc::clone(&state.env),
            state.history,
            state.cold,
            state.roots,
            layout,
            split_cold_dir,
            legacy_v2_dir,
            map_size,
            max_dbs,
        )?;
        let authority = Arc::new(Self {
            identity: Arc::new(AuthorityIdentity),
            backend: AuthorityBackend::Lmdb(state),
        });
        Ok(authority)
    }

    /// Finish any structurally staged legacy migration with the concrete
    /// RSpace record decoder, then initialize an actually fresh catalog.
    pub(crate) fn initialize_for_records(
        &self,
        validator: &RecordValidator,
    ) -> Result<(), RootError> {
        if let AuthorityBackend::Lmdb(state) = &self.backend {
            complete_legacy_state(
                &state.env,
                state.history,
                state.cold,
                state.roots,
                &state.namespace,
                validator,
            )
            .map_err(root_heed)?;
        }
        self.initialize_empty_root()
    }

    pub(crate) fn history_store(
        self: &Arc<Self>,
    ) -> Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch> {
        self.legacy_facet(ObjectKind::History)
    }

    pub(crate) fn cold_store(
        self: &Arc<Self>,
    ) -> Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch> {
        self.legacy_facet(ObjectKind::Cold)
    }

    pub(crate) fn roots_store_for_inspection(
        self: &Arc<Self>,
    ) -> Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch> {
        self.legacy_facet(ObjectKind::Roots)
    }

    fn legacy_facet(
        self: &Arc<Self>,
        kind: ObjectKind,
    ) -> Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch> {
        match &self.backend {
            AuthorityBackend::Memory(_) | AuthorityBackend::Lmdb(_) => {
                Ok(Arc::new(ContentAddressedStore {
                    authority: Arc::clone(self),
                    kind,
                }))
            }
        }
    }

    pub fn current_verified_root(&self) -> Result<Option<CurrentRoot>, RootError> {
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let state = lock_memory(state)?;
                decode_current_from_map(&state.roots)
            }
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn().map_err(root_heed)?;
                decode_current_from_db(state.roots, &txn)
            }
        }
    }

    pub fn current_root(&self) -> Result<Blake2b256Hash, RootError> {
        if let Some(current) = self.current_verified_root()? {
            return Ok(current.root().clone());
        }
        Err(RootError::CorruptRootCatalog("authority has no initialized current root".to_string()))
    }

    fn initialize_empty_root(&self) -> Result<(), RootError> {
        if self.current_verified_root()?.is_some() {
            return Ok(());
        }
        let empty = RadixHistory::empty_root_node_hash();
        match self.publish_roots(std::slice::from_ref(&empty), Some(&empty), None) {
            Ok(_) | Err(RootError::RootCatalogConflict { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Atomically publish a locally prepared checkpoint.
    ///
    /// The target is derived from an encoded root object produced by
    /// `RadixHistory`; no digest-taking local publication API exists. Cold
    /// leaves, history nodes, verified membership, and current-root generation
    /// become visible under one memory lock or one LMDB write transaction.
    pub(crate) fn commit_local_checkpoint(
        self: &Arc<Self>,
        prepared: &PreparedHistoryCheckpoint,
        cold_leaves: Vec<(ColdLeafAddress, ByteBuffer)>,
        record_validator: &RecordValidator,
    ) -> Result<CurrentRoot, HistoryError> {
        if !Arc::ptr_eq(self, prepared.authority()) {
            return Err(HistoryError::ActionError(
                "prepared checkpoint belongs to a different RSpace authority".to_string(),
            ));
        }

        let history_objects = Arc::new(validate_content_addressed_batch(
            ObjectKind::History,
            prepared
                .history_objects()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )?);
        let cold_objects = Arc::new(validate_content_addressed_batch(
            ObjectKind::Cold,
            cold_leaves
                .into_iter()
                .map(|(address, value)| (address.encode(), value))
                .collect(),
        )?);

        if !prepared.changed() {
            if !self.contains_verified_root(prepared.base())? {
                return Err(unknown_verified_root(prepared.base()).into());
            }
            return self
                .current_verified_root()?
                .ok_or_else(uninitialized_current_root)
                .map_err(HistoryError::from);
        }

        let root_encoding = prepared.root_encoding().ok_or_else(|| {
            HistoryError::ActionError("changed checkpoint has no encoded root object".to_string())
        })?;
        let derived_root = Blake2b256Hash::new(root_encoding);
        if &derived_root != prepared.target_root() {
            return Err(HistoryError::ActionError(format!(
                "prepared target root {} differs from encoded root digest {}",
                prepared.target_root(),
                derived_root
            )));
        }
        match history_objects.get(&prepared.target_root().0) {
            Some(bytes) if bytes.as_slice() == root_encoding => {}
            _ => {
                return Err(HistoryError::ActionError(
                    "prepared history objects do not contain the exact derived root".to_string(),
                ));
            }
        }

        let history_overlay =
            OverlayStore::new(self.history_store()?, Arc::clone(&history_objects));
        let cold_overlay = OverlayStore::new(self.cold_store()?, Arc::clone(&cold_objects));
        verify_state_closure(
            &history_overlay,
            &cold_overlay,
            std::slice::from_ref(prepared.target_root()),
            record_validator,
        )
        .map_err(|error| HistoryError::ActionError(error.to_string()))?;

        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let mut state = lock_memory(state)?;
                require_verified_base(
                    state
                        .roots
                        .get(&verified_root_key(prepared.base()))
                        .map(Vec::as_slice),
                    prepared.base(),
                )?;
                let current = decode_current_from_map(&state.roots)?
                    .ok_or_else(uninitialized_current_root)?;
                ensure_immutable_map(&state.history, history_objects.as_ref())?;
                ensure_immutable_map(&state.cold, cold_objects.as_ref())?;
                let target_roots = std::iter::once(prepared.target_root().clone()).collect();
                validate_memberships_map(
                    &state.roots,
                    &target_roots,
                    Some(prepared.target_root()),
                )?;
                let next = next_current(Some(prepared.target_root()), Some(current.generation()))?
                    .ok_or_else(|| {
                        RootError::CorruptRootCatalog(
                            "local checkpoint did not derive a current root".to_string(),
                        )
                    })?;

                state.history.extend(
                    history_objects
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
                state.cold.extend(
                    cold_objects
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
                state.roots.insert(
                    verified_root_key(prepared.target_root()),
                    verified_root_value(prepared.target_root()),
                );
                state
                    .roots
                    .insert(CURRENT_ROOT_KEY.to_vec(), encode_current_root(&next));
                Ok(next)
            }
            AuthorityBackend::Lmdb(state) => {
                let mut txn = state.env.write_txn().map_err(root_heed)?;
                require_verified_base(
                    state
                        .roots
                        .get(&txn, &verified_root_key(prepared.base()))
                        .map_err(root_heed)?
                        .as_deref(),
                    prepared.base(),
                )?;
                let current = decode_current_pair(
                    state
                        .roots
                        .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                        .map_err(root_heed)?
                        .as_deref(),
                    state
                        .roots
                        .get(&txn, &LEGACY_CURRENT_ROOT_KEY.to_vec())
                        .map_err(root_heed)?
                        .as_deref(),
                )?
                .ok_or_else(uninitialized_current_root)?;
                ensure_immutable_db(state.history, &txn, history_objects.as_ref())?;
                ensure_immutable_db(state.cold, &txn, cold_objects.as_ref())?;
                for (key, value) in history_objects.iter() {
                    if state.history.get(&txn, key).map_err(root_heed)?.is_none() {
                        state.history.put(&mut txn, key, value).map_err(root_heed)?;
                    }
                }
                for (key, value) in cold_objects.iter() {
                    if state.cold.get(&txn, key).map_err(root_heed)?.is_none() {
                        state.cold.put(&mut txn, key, value).map_err(root_heed)?;
                    }
                }
                let target_roots = std::iter::once(prepared.target_root().clone()).collect();
                let next = publish_in_db(
                    state.roots,
                    &mut txn,
                    &target_roots,
                    Some(prepared.target_root()),
                    Some(current.generation()),
                )?
                .ok_or_else(|| {
                    RootError::CorruptRootCatalog(
                        "local checkpoint did not derive a current root".to_string(),
                    )
                })?;
                txn.commit().map_err(root_heed)?;
                Ok(next)
            }
        }
    }

    pub fn begin_import(&self, spec: StateImportSpec) -> Result<StagedImport, StateImportError> {
        let expected_generation = self
            .current_verified_root()?
            .map(|current| current.generation());
        Ok(StagedImport {
            identity: Arc::clone(&self.identity),
            pending_paths: spec
                .roots()
                .iter()
                .cloned()
                .map(|root| vec![(root, None)])
                .collect(),
            spec,
            expected_generation,
        })
    }

    pub(crate) fn accept_import_page(
        &self,
        staged: &mut StagedImport,
        start_path: &NodePath,
        next_path: NodePath,
        terminal: bool,
    ) -> Result<(), StateImportError> {
        self.ensure_authority(&staged.identity)?;
        if !staged.pending_paths.remove(start_path) {
            return Err(StateImportError::UnexpectedStartPath(start_path.clone()));
        }
        if !terminal {
            staged.pending_paths.insert(next_path);
        }
        Ok(())
    }

    pub fn seal_import(
        self: &Arc<Self>,
        staged: StagedImport,
        record_validator: &RecordValidator,
    ) -> Result<SealedImportedRoots, StateImportError> {
        self.ensure_authority(&staged.identity)?;
        if !staged.pending_paths.is_empty() {
            return Err(StateImportError::IncompletePagination(
                staged.pending_paths.iter().cloned().collect(),
            ));
        }
        let history = self.history_store()?;
        let cold = self.cold_store()?;
        verify_state_closure(
            history.as_ref(),
            cold.as_ref(),
            staged.spec.roots(),
            record_validator,
        )?;
        Ok(SealedImportedRoots {
            identity: staged.identity,
            spec: staged.spec,
            expected_generation: staged.expected_generation,
        })
    }

    pub fn publish_import(
        &self,
        sealed: SealedImportedRoots,
        witness: &StateImportPublicationWitness,
    ) -> Result<StateImportPublication, StateImportError> {
        self.ensure_authority(&sealed.identity)?;
        let activation = match sealed.spec.publication_mode() {
            StateImportPublicationMode::PreserveCurrent => {
                self.publish_roots(sealed.spec.roots(), None, sealed.expected_generation)?;
                witness.record_authoritative_publication();
                None
            }
            StateImportPublicationMode::Activate(target) => {
                let current = self.publish_roots(
                    sealed.spec.roots(),
                    Some(target),
                    sealed.expected_generation,
                )?;
                witness.record_authoritative_publication();
                let current = current.ok_or_else(|| {
                    StateImportError::InternalInvariant(
                        "import activation did not produce a current root".to_string(),
                    )
                })?;
                Some(StateImportActivation {
                    root: current.root().clone(),
                    generation: current.generation(),
                })
            }
        };
        Ok(StateImportPublication {
            verified_roots: sealed.spec.roots().to_vec(),
            activation,
        })
    }

    pub fn known_verified_root(
        &self,
        root: &Blake2b256Hash,
    ) -> Result<KnownVerifiedRoot, RootError> {
        if !self.contains_verified_root(root)? {
            return Err(RootError::UnknownRootError(format!("unknown verified root: {root}")));
        }
        Ok(KnownVerifiedRoot {
            identity: Arc::clone(&self.identity),
            root: root.clone(),
        })
    }

    pub fn activate_known(&self, known: KnownVerifiedRoot) -> Result<CurrentRoot, RootError> {
        self.ensure_root_authority(&known.identity)?;
        self.publish_roots(
            &[],
            Some(&known.root),
            self.current_verified_root()?
                .map(|current| current.generation()),
        )?
        .ok_or_else(|| RootError::CorruptRootCatalog("known activation was not produced".into()))
    }

    pub fn contains_verified_root(&self, root: &Blake2b256Hash) -> Result<bool, RootError> {
        validate_root_hash(root)?;
        let key = verified_root_key(root);
        let expected = verified_root_value(root);
        match &self.backend {
            AuthorityBackend::Memory(state) => match lock_memory(state)?.roots.get(&key) {
                None => Ok(false),
                Some(value) if value == &expected => Ok(true),
                Some(_) => Err(corrupt_membership(root)),
            },
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn().map_err(root_heed)?;
                match state.roots.get(&txn, &key).map_err(root_heed)? {
                    None => Ok(false),
                    Some(value) if value == expected => Ok(true),
                    Some(_) => Err(corrupt_membership(root)),
                }
            }
        }
    }

    fn publish_roots(
        &self,
        roots: &[Blake2b256Hash],
        target: Option<&Blake2b256Hash>,
        expected_generation: Option<u64>,
    ) -> Result<Option<CurrentRoot>, RootError> {
        let roots = unique_validated_roots(roots)?;
        if let Some(target) = target {
            validate_root_hash(target)?;
        }
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let mut state = lock_memory(state)?;
                publish_in_map(&mut state.roots, &roots, target, expected_generation)
            }
            AuthorityBackend::Lmdb(state) => {
                // One authority transaction owns history, cold, catalog membership, and
                // current-root publication.  No per-database transaction can interleave.
                let mut txn = state.env.write_txn().map_err(root_heed)?;
                let result =
                    publish_in_db(state.roots, &mut txn, &roots, target, expected_generation)?;
                txn.commit().map_err(root_heed)?;
                Ok(result)
            }
        }
    }

    fn ensure_authority(&self, observed: &Arc<AuthorityIdentity>) -> Result<(), StateImportError> {
        if Arc::ptr_eq(observed, &self.identity) {
            Ok(())
        } else {
            Err(StateImportError::InternalInvariant(
                "state-import capability belongs to a different RSpace authority".to_string(),
            ))
        }
    }

    fn ensure_root_authority(&self, observed: &Arc<AuthorityIdentity>) -> Result<(), RootError> {
        if Arc::ptr_eq(observed, &self.identity) {
            Ok(())
        } else {
            Err(RootError::UnknownRootError(
                "root capability belongs to a different RSpace authority".to_string(),
            ))
        }
    }

    fn get_objects(
        &self,
        kind: ObjectKind,
        keys: &[ByteBuffer],
    ) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let state = lock_memory_kv(state)?;
                let map = memory_map(&state, kind);
                Ok(keys.iter().map(|key| map.get(key).cloned()).collect())
            }
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn()?;
                let db = lmdb_db(state, kind);
                keys.iter()
                    .map(|key| db.get(&txn, key).map_err(Into::into))
                    .collect()
            }
        }
    }

    fn put_objects(
        &self,
        kind: ObjectKind,
        pairs: Vec<(ByteBuffer, ByteBuffer)>,
    ) -> Result<(), KvStoreError> {
        let pairs = validate_content_addressed_batch(kind, pairs)?;
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let mut state = lock_memory_kv(state)?;
                let map = memory_map_mut(&mut state, kind);
                put_if_absent_equal_map(map, pairs)
            }
            AuthorityBackend::Lmdb(state) => {
                let mut txn = state.env.write_txn()?;
                let db = lmdb_db(state, kind);
                for (key, value) in pairs {
                    match db.get(&txn, &key)? {
                        Some(existing) if existing != value => {
                            return Err(KvStoreError::InvalidArgument(format!(
                                "immutable content-addressed key {} already has different bytes",
                                hex::encode(key)
                            )));
                        }
                        Some(_) => {}
                        None => db.put(&mut txn, &key, &value)?,
                    }
                }
                txn.commit()?;
                Ok(())
            }
        }
    }

    fn compare_and_swap_objects(
        &self,
        kind: ObjectKind,
        conditions: &[(ByteBuffer, Option<ByteBuffer>)],
        mutations: &[KeyValueStoreMutation],
    ) -> Result<bool, KvStoreError> {
        validate_atomic_batch(conditions, mutations)?;
        if mutations
            .iter()
            .any(|mutation| matches!(mutation, KeyValueStoreMutation::Delete(_)))
        {
            return Err(KvStoreError::InvalidArgument(
                "content-addressed RSpace objects cannot be deleted".to_string(),
            ));
        }
        let requested = validate_content_addressed_batch(
            kind,
            mutations
                .iter()
                .map(|mutation| match mutation {
                    KeyValueStoreMutation::Put(key, value) => (key.clone(), value.clone()),
                    KeyValueStoreMutation::Delete(_) => unreachable!(),
                })
                .collect(),
        )?;
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let mut state = lock_memory_kv(state)?;
                let map = memory_map_mut(&mut state, kind);
                if conditions
                    .iter()
                    .any(|(key, expected)| map.get(key) != expected.as_ref())
                {
                    return Ok(false);
                }
                put_if_absent_equal_map(map, requested)?;
                Ok(true)
            }
            AuthorityBackend::Lmdb(state) => {
                let mut txn = state.env.write_txn()?;
                let db = lmdb_db(state, kind);
                for (key, expected) in conditions {
                    if db.get(&txn, key)?.as_ref() != expected.as_ref() {
                        return Ok(false);
                    }
                }
                for (key, value) in requested {
                    match db.get(&txn, &key)? {
                        Some(existing) if existing != value => {
                            return Err(KvStoreError::InvalidArgument(format!(
                                "immutable content-addressed key {} already has different bytes",
                                hex::encode(key)
                            )));
                        }
                        Some(_) => {}
                        None => db.put(&mut txn, &key, &value)?,
                    }
                }
                txn.commit()?;
                Ok(true)
            }
        }
    }

    fn object_map(
        &self,
        kind: ObjectKind,
    ) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let state = lock_memory_kv(state)?;
                Ok(memory_map(&state, kind).clone())
            }
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn()?;
                let mut result = BTreeMap::new();
                for item in lmdb_db(state, kind).iter(&txn)? {
                    let (key, value) = item?;
                    result.insert(key, value);
                }
                Ok(result)
            }
        }
    }

    fn iterate_objects(
        &self,
        kind: ObjectKind,
        f: fn(ByteBuffer, ByteBuffer),
    ) -> Result<(), KvStoreError> {
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let state = lock_memory_kv(state)?;
                for (key, value) in memory_map(&state, kind) {
                    f(key.clone(), value.clone());
                }
                Ok(())
            }
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn()?;
                for item in lmdb_db(state, kind).iter(&txn)? {
                    let (key, value) = item?;
                    f(key, value);
                }
                Ok(())
            }
        }
    }

    fn iterate_objects_while(
        &self,
        kind: ObjectKind,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let state = lock_memory_kv(state)?;
                for (key, value) in memory_map(&state, kind) {
                    if !f(key.clone(), value.clone())? {
                        break;
                    }
                }
                Ok(())
            }
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn()?;
                for item in lmdb_db(state, kind).iter(&txn)? {
                    let (key, value) = item?;
                    if !f(key, value)? {
                        break;
                    }
                }
                Ok(())
            }
        }
    }

    fn objects_non_empty(&self, kind: ObjectKind) -> Result<bool, KvStoreError> {
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let state = lock_memory_kv(state)?;
                Ok(!memory_map(&state, kind).is_empty())
            }
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn()?;
                let has_first = {
                    let mut objects = lmdb_db(state, kind).iter(&txn)?;
                    objects.next().transpose()?.is_some()
                };
                Ok(has_first)
            }
        }
    }

    fn objects_size_bytes(&self, kind: ObjectKind) -> Result<usize, KvStoreError> {
        let add = |total: usize, key: &[u8], value: &[u8]| {
            total
                .checked_add(key.len())
                .and_then(|total| total.checked_add(value.len()))
                .ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "content-addressed store byte size exceeds usize".to_string(),
                    )
                })
        };
        match &self.backend {
            AuthorityBackend::Memory(state) => {
                let state = lock_memory_kv(state)?;
                memory_map(&state, kind)
                    .iter()
                    .try_fold(0usize, |total, (key, value)| add(total, key, value))
            }
            AuthorityBackend::Lmdb(state) => {
                let txn = state.env.read_txn()?;
                let mut total = 0usize;
                for item in lmdb_db(state, kind).iter(&txn)? {
                    let (key, value) = item?;
                    total = add(total, &key, &value)?;
                }
                Ok(total)
            }
        }
    }
}

#[derive(Clone)]
struct OverlayStore {
    base: Arc<dyn KeyValueStore>,
    prepared: Arc<BTreeMap<ByteBuffer, ByteBuffer>>,
}

impl OverlayStore {
    fn new(base: Arc<dyn KeyValueStore>, prepared: Arc<BTreeMap<ByteBuffer, ByteBuffer>>) -> Self {
        Self { base, prepared }
    }

    fn read_only_error() -> KvStoreError {
        KvStoreError::InvalidArgument("prepared checkpoint overlay is read-only".to_string())
    }
}

impl KeyValueStore for OverlayStore {
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        let mut result = vec![None; keys.len()];
        let mut missing_positions = Vec::new();
        let mut missing_keys = Vec::new();
        for (position, key) in keys.iter().enumerate() {
            if let Some(value) = self.prepared.get(key) {
                result[position] = Some(value.clone());
            } else {
                missing_positions.push(position);
                missing_keys.push(key.clone());
            }
        }
        let loaded = self.base.get(&missing_keys)?;
        if loaded.len() != missing_positions.len() {
            return Err(KvStoreError::IoError(format!(
                "checkpoint overlay base returned {} values for {} keys",
                loaded.len(),
                missing_positions.len()
            )));
        }
        for (position, value) in missing_positions.into_iter().zip(loaded) {
            result[position] = value;
        }
        Ok(result)
    }

    fn put(&self, _pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        Err(Self::read_only_error())
    }

    fn compare_and_swap_batch(
        &self,
        _conditions: &[(ByteBuffer, Option<ByteBuffer>)],
        _mutations: &[KeyValueStoreMutation],
    ) -> Result<bool, KvStoreError> {
        Err(Self::read_only_error())
    }

    fn delete(&self, _keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        Err(Self::read_only_error())
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        for (key, value) in self.to_map()? {
            f(key, value);
        }
        Ok(())
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        for (key, value) in self.to_map()? {
            if !f(key, value)? {
                break;
            }
        }
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        let mut all = self.base.to_map()?;
        all.extend(
            self.prepared
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        Ok(all)
    }

    fn print_store(&self) -> Result<(), KvStoreError> {
        println!("{:?}", self.to_map()?);
        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> {
        Ok(!self.prepared.is_empty() || self.base.non_empty()?)
    }

    fn size_bytes(&self) -> Result<usize, KvStoreError> {
        self.to_map()?
            .iter()
            .try_fold(0usize, |total, (key, value)| {
                total
                    .checked_add(key.len())
                    .and_then(|total| total.checked_add(value.len()))
                    .ok_or_else(|| KvStoreError::IoError("checkpoint overlay size overflow".into()))
            })
    }
}

#[derive(Clone)]
struct ContentAddressedStore {
    authority: Arc<RSpaceStateAuthority>,
    kind: ObjectKind,
}

impl KeyValueStore for ContentAddressedStore {
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        self.authority.get_objects(self.kind, keys)
    }

    fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        if matches!(self.kind, ObjectKind::Roots) {
            return Err(KvStoreError::InvalidArgument(
                "verified root catalog mutations require RSpaceStateAuthority".to_string(),
            ));
        }
        self.authority.put_objects(self.kind, pairs)
    }

    fn compare_and_swap_batch(
        &self,
        conditions: &[(ByteBuffer, Option<ByteBuffer>)],
        mutations: &[KeyValueStoreMutation],
    ) -> Result<bool, KvStoreError> {
        if matches!(self.kind, ObjectKind::Roots) {
            return Err(KvStoreError::InvalidArgument(
                "verified root catalog mutations require RSpaceStateAuthority".to_string(),
            ));
        }
        self.authority
            .compare_and_swap_objects(self.kind, conditions, mutations)
    }

    fn delete(&self, _keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        Err(KvStoreError::InvalidArgument(
            "content-addressed RSpace objects cannot be deleted".to_string(),
        ))
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.authority.iterate_objects(self.kind, f)
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.authority.iterate_objects_while(self.kind, f)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.authority.object_map(self.kind)
    }

    fn print_store(&self) -> Result<(), KvStoreError> {
        println!("{:?}", self.to_map()?);
        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> {
        self.authority.objects_non_empty(self.kind)
    }

    fn size_bytes(&self) -> Result<usize, KvStoreError> {
        self.authority.objects_size_bytes(self.kind)
    }
}

fn validate_content_addressed_batch(
    kind: ObjectKind,
    pairs: Vec<(ByteBuffer, ByteBuffer)>,
) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
    let mut result = BTreeMap::new();
    for (key, value) in pairs {
        match kind {
            ObjectKind::History => {
                let actual = Blake2b256Hash::new(&value);
                if actual.0 != key {
                    return Err(KvStoreError::InvalidArgument(format!(
                        "history object key {} does not match content digest {}",
                        hex::encode(key),
                        actual
                    )));
                }
            }
            ObjectKind::Cold => {
                let address = ColdLeafAddress::decode(&key)
                    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
                validate_cold_leaf(&address, &value)
                    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?;
            }
            ObjectKind::Roots => {
                return Err(KvStoreError::InvalidArgument(
                    "verified root catalog values are not content-addressed objects".to_string(),
                ));
            }
        }
        if let Some(existing) = result.insert(key.clone(), value.clone()) {
            if existing != value {
                return Err(KvStoreError::InvalidArgument(format!(
                    "content-addressed batch assigns conflicting bytes to {}",
                    hex::encode(key)
                )));
            }
        }
    }
    Ok(result)
}

fn put_if_absent_equal_map(
    map: &mut BTreeMap<ByteBuffer, ByteBuffer>,
    pairs: BTreeMap<ByteBuffer, ByteBuffer>,
) -> Result<(), KvStoreError> {
    for (key, value) in &pairs {
        if let Some(existing) = map.get(key) {
            if existing != value {
                return Err(KvStoreError::InvalidArgument(format!(
                    "immutable content-addressed key {} already has different bytes",
                    hex::encode(key)
                )));
            }
        }
    }
    map.extend(pairs);
    Ok(())
}

fn ensure_immutable_map(
    existing: &BTreeMap<ByteBuffer, ByteBuffer>,
    prepared: &BTreeMap<ByteBuffer, ByteBuffer>,
) -> Result<(), KvStoreError> {
    for (key, value) in prepared {
        if existing.get(key).is_some_and(|old| old != value) {
            return Err(KvStoreError::InvalidArgument(format!(
                "immutable content-addressed key {} already has different bytes",
                hex::encode(key)
            )));
        }
    }
    Ok(())
}

fn ensure_immutable_db(
    db: BytesDb,
    txn: &heed::RwTxn<'_>,
    prepared: &BTreeMap<ByteBuffer, ByteBuffer>,
) -> Result<(), HistoryError> {
    for (key, value) in prepared {
        if db
            .get(txn, key)
            .map_err(KvStoreError::from)?
            .is_some_and(|old| old != *value)
        {
            return Err(HistoryError::KvStoreError(KvStoreError::InvalidArgument(format!(
                "immutable content-addressed key {} already has different bytes",
                hex::encode(key)
            ))));
        }
    }
    Ok(())
}

fn memory_map(state: &MemoryState, kind: ObjectKind) -> &BTreeMap<ByteBuffer, ByteBuffer> {
    match kind {
        ObjectKind::History => &state.history,
        ObjectKind::Cold => &state.cold,
        ObjectKind::Roots => &state.roots,
    }
}

fn memory_map_mut(
    state: &mut MemoryState,
    kind: ObjectKind,
) -> &mut BTreeMap<ByteBuffer, ByteBuffer> {
    match kind {
        ObjectKind::History => &mut state.history,
        ObjectKind::Cold => &mut state.cold,
        ObjectKind::Roots => &mut state.roots,
    }
}

fn lmdb_db(state: &LmdbState, kind: ObjectKind) -> BytesDb {
    match kind {
        ObjectKind::History => state.history,
        ObjectKind::Cold => state.cold,
        ObjectKind::Roots => state.roots,
    }
}

fn lock_memory(
    state: &Mutex<MemoryState>,
) -> Result<std::sync::MutexGuard<'_, MemoryState>, RootError> {
    state
        .lock()
        .map_err(|_| RootError::KvStoreError("RSpace authority lock poisoned".into()))
}

fn lock_memory_kv(
    state: &Mutex<MemoryState>,
) -> Result<std::sync::MutexGuard<'_, MemoryState>, KvStoreError> {
    state
        .lock()
        .map_err(|_| KvStoreError::LockError("RSpace authority lock poisoned".into()))
}

fn root_heed(error: heed::Error) -> RootError { RootError::from(KvStoreError::from(error)) }

fn validate_root_hash(root: &Blake2b256Hash) -> Result<(), RootError> {
    if root.bytes().len() == ROOT_HASH_LENGTH {
        Ok(())
    } else {
        Err(RootError::InvalidRootHash(format!(
            "expected {ROOT_HASH_LENGTH} bytes, found {}",
            root.bytes().len()
        )))
    }
}

fn unique_validated_roots(roots: &[Blake2b256Hash]) -> Result<BTreeSet<Blake2b256Hash>, RootError> {
    roots.iter().try_fold(BTreeSet::new(), |mut result, root| {
        validate_root_hash(root)?;
        result.insert(root.clone());
        Ok(result)
    })
}

fn verified_root_key(root: &Blake2b256Hash) -> Vec<u8> {
    [VERIFIED_ROOT_KEY_PREFIX, root.bytes().as_slice()].concat()
}

fn verified_root_value(root: &Blake2b256Hash) -> Vec<u8> {
    [VERIFIED_ROOT_VALUE_PREFIX, root.bytes().as_slice()].concat()
}

fn corrupt_membership(root: &Blake2b256Hash) -> RootError {
    RootError::CorruptRootCatalog(format!("verified membership does not bind root {root}"))
}

fn encode_current_root(current: &CurrentRoot) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CURRENT_ROOT_VALUE_PREFIX.len() + 8 + ROOT_HASH_LENGTH);
    bytes.extend_from_slice(CURRENT_ROOT_VALUE_PREFIX);
    bytes.extend_from_slice(&current.generation().to_be_bytes());
    bytes.extend_from_slice(&current.root().bytes());
    bytes
}

fn decode_current_root(bytes: &[u8]) -> Result<CurrentRoot, RootError> {
    let expected = CURRENT_ROOT_VALUE_PREFIX.len() + 8 + ROOT_HASH_LENGTH;
    if bytes.len() != expected || !bytes.starts_with(CURRENT_ROOT_VALUE_PREFIX) {
        return Err(RootError::CorruptRootCatalog(
            "current-root value has an unknown version or invalid length".to_string(),
        ));
    }
    let generation_offset = CURRENT_ROOT_VALUE_PREFIX.len();
    let root_offset = generation_offset + 8;
    let generation = u64::from_be_bytes(bytes[generation_offset..root_offset].try_into().unwrap());
    Ok(CurrentRoot::new(Blake2b256Hash::from_bytes(bytes[root_offset..].to_vec()), generation))
}

fn decode_current_from_map(
    roots: &BTreeMap<ByteBuffer, ByteBuffer>,
) -> Result<Option<CurrentRoot>, RootError> {
    let current = roots.get(CURRENT_ROOT_KEY);
    let legacy = roots.get(LEGACY_CURRENT_ROOT_KEY);
    decode_current_pair(current.map(Vec::as_slice), legacy.map(Vec::as_slice))
}

fn decode_current_from_db(
    roots: BytesDb,
    txn: &heed::RoTxn<'_>,
) -> Result<Option<CurrentRoot>, RootError> {
    decode_current_pair(
        roots
            .get(txn, &CURRENT_ROOT_KEY.to_vec())
            .map_err(root_heed)?
            .as_deref(),
        roots
            .get(txn, &LEGACY_CURRENT_ROOT_KEY.to_vec())
            .map_err(root_heed)?
            .as_deref(),
    )
}

fn decode_current_pair(
    current: Option<&[u8]>,
    legacy: Option<&[u8]>,
) -> Result<Option<CurrentRoot>, RootError> {
    if legacy.is_some() {
        return Err(RootError::LegacyRootCatalog(
            "unversioned current-root cannot coexist with verified authority".to_string(),
        ));
    }
    current.map(decode_current_root).transpose()
}

fn publish_in_map(
    catalog: &mut BTreeMap<ByteBuffer, ByteBuffer>,
    roots: &BTreeSet<Blake2b256Hash>,
    target: Option<&Blake2b256Hash>,
    expected_generation: Option<u64>,
) -> Result<Option<CurrentRoot>, RootError> {
    let current = decode_current_from_map(catalog)?;
    check_generation(current.as_ref(), expected_generation)?;
    validate_memberships_map(catalog, roots, target)?;
    for root in roots {
        catalog.insert(verified_root_key(root), verified_root_value(root));
    }
    let next = next_current(target, expected_generation)?;
    if let Some(next) = &next {
        catalog.insert(CURRENT_ROOT_KEY.to_vec(), encode_current_root(next));
    }
    Ok(next)
}

fn publish_in_db(
    catalog: BytesDb,
    txn: &mut heed::RwTxn<'_>,
    roots: &BTreeSet<Blake2b256Hash>,
    target: Option<&Blake2b256Hash>,
    expected_generation: Option<u64>,
) -> Result<Option<CurrentRoot>, RootError> {
    let current = decode_current_pair(
        catalog
            .get(txn, &CURRENT_ROOT_KEY.to_vec())
            .map_err(root_heed)?
            .as_deref(),
        catalog
            .get(txn, &LEGACY_CURRENT_ROOT_KEY.to_vec())
            .map_err(root_heed)?
            .as_deref(),
    )?;
    check_generation(current.as_ref(), expected_generation)?;
    for root in roots {
        let key = verified_root_key(root);
        let value = verified_root_value(root);
        match catalog.get(txn, &key).map_err(root_heed)? {
            Some(existing) if existing != value => return Err(corrupt_membership(root)),
            _ => {}
        }
    }
    if let Some(target) = target {
        let published_now = roots.contains(target);
        match catalog
            .get(txn, &verified_root_key(target))
            .map_err(root_heed)?
        {
            Some(value) if value != verified_root_value(target) => {
                return Err(corrupt_membership(target));
            }
            None if !published_now => {
                return Err(RootError::UnknownRootError(format!(
                    "unknown verified root: {target}"
                )));
            }
            _ => {}
        }
    }
    for root in roots {
        catalog
            .put(txn, &verified_root_key(root), &verified_root_value(root))
            .map_err(root_heed)?;
    }
    let next = next_current(target, expected_generation)?;
    if let Some(next) = &next {
        catalog
            .put(txn, &CURRENT_ROOT_KEY.to_vec(), &encode_current_root(next))
            .map_err(root_heed)?;
    }
    Ok(next)
}

fn validate_memberships_map(
    catalog: &BTreeMap<ByteBuffer, ByteBuffer>,
    roots: &BTreeSet<Blake2b256Hash>,
    target: Option<&Blake2b256Hash>,
) -> Result<(), RootError> {
    for root in roots {
        if let Some(existing) = catalog.get(&verified_root_key(root)) {
            if existing != &verified_root_value(root) {
                return Err(corrupt_membership(root));
            }
        }
    }
    if let Some(target) = target {
        if !roots.contains(target) {
            match catalog.get(&verified_root_key(target)) {
                Some(value) if value == &verified_root_value(target) => {}
                Some(_) => return Err(corrupt_membership(target)),
                None => {
                    return Err(RootError::UnknownRootError(format!(
                        "unknown verified root: {target}"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn check_generation(
    current: Option<&CurrentRoot>,
    expected_generation: Option<u64>,
) -> Result<(), RootError> {
    let actual_generation = current.map(CurrentRoot::generation);
    if actual_generation == expected_generation {
        Ok(())
    } else {
        Err(RootError::RootCatalogConflict {
            expected_generation,
            actual_generation,
        })
    }
}

fn unknown_verified_root(root: &Blake2b256Hash) -> RootError {
    RootError::UnknownRootError(format!("unknown verified root: {root}"))
}

fn uninitialized_current_root() -> RootError {
    RootError::CorruptRootCatalog("authority has no initialized current root".to_string())
}

fn require_verified_base(
    membership: Option<&[u8]>,
    base: &Blake2b256Hash,
) -> Result<(), RootError> {
    match membership {
        Some(value) if value == verified_root_value(base).as_slice() => Ok(()),
        Some(_) => Err(corrupt_membership(base)),
        None => Err(unknown_verified_root(base)),
    }
}

fn next_current(
    target: Option<&Blake2b256Hash>,
    generation: Option<u64>,
) -> Result<Option<CurrentRoot>, RootError> {
    target
        .map(|root| {
            let generation = generation
                .map_or(Some(0), |value| value.checked_add(1))
                .ok_or(RootError::RootCatalogGenerationOverflow)?;
            Ok(CurrentRoot::new(root.clone(), generation))
        })
        .transpose()
}

#[cfg(test)]
#[path = "../../../tests/state/atomic_state_authority.rs"]
mod atomic_state_authority_tests;
