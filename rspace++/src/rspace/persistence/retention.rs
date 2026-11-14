//! Canonical retained-root authority for bounded durable compaction.
//!
//! The manifest names semantic state roots, not pack offsets. Compaction can
//! therefore rewrite their shared Merkle closure into a new pack generation
//! without changing logical state identities. Decoding is borrowed and
//! streaming; no root projection is required.

#[cfg(unix)]
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::sync::{OnceLock, Weak};

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_len, checked_sum,
    parse_envelope, write_envelope_header, write_u64,
};
use super::durable::CatalogSlotV1;
use super::mutation::StateVersionV1;

pub(crate) mod v2_format;
pub(crate) mod v2_reader;
pub(crate) mod v2_writer;

pub(crate) const RETENTION_MANIFEST_DOMAIN: &[u8; OBJECT_DOMAIN_LEN] = b"RSpaceDAG.Retn\0\0";
const STATE_ENTRY_LEN: u64 = 8 + 32 + 8 + 8;

/// One semantic state root and the deterministic size of its reachable,
/// deduplicated canonical closure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetainedStateV1 {
    state: StateVersionV1,
    canonical_bytes: u64,
}

impl RetainedStateV1 {
    pub fn new(state: StateVersionV1, canonical_bytes: u64) -> Result<Self, CanonicalFormatError> {
        if canonical_bytes < state.manifest().encoded_len() {
            return Err(CanonicalFormatError::RetentionStateBytes {
                generation: state.generation(),
                minimum: state.manifest().encoded_len(),
                actual: canonical_bytes,
            });
        }
        Ok(Self {
            state,
            canonical_bytes,
        })
    }

    pub const fn state(self) -> StateVersionV1 { self.state }

    pub const fn canonical_bytes(self) -> u64 { self.canonical_bytes }

    fn write<S: CanonicalSink>(self, sink: &mut S) -> Result<(), S::Error> {
        write_u64(sink, self.state.generation())?;
        self.state.manifest().write(sink)?;
        write_u64(sink, self.canonical_bytes)
    }

    fn parse(cursor: &mut ReadCursor<'_>) -> Result<Self, CanonicalFormatError> {
        let state = StateVersionV1::new(cursor.read_u64()?, ObjectRefV1::parse(cursor)?);
        Self::new(state, cursor.read_u64()?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionManifestV1 {
    current: RetainedStateV1,
    historical: Box<[RetainedStateV1]>,
    body_len: u64,
    canonical_len: u64,
}

impl RetentionManifestV1 {
    pub fn new(
        current: RetainedStateV1,
        historical: impl Into<Box<[RetainedStateV1]>>,
    ) -> Result<Self, CanonicalFormatError> {
        let historical = historical.into();
        validate_historical(current, &historical)?;
        let count = checked_len(historical.len(), "retained historical states")?;
        let entries =
            count
                .checked_mul(STATE_ENTRY_LEN)
                .ok_or(CanonicalFormatError::LengthOverflow {
                    field: "retained historical states",
                })?;
        let body_len = checked_sum(STATE_ENTRY_LEN + 8, entries, "retention manifest body")?;
        let canonical_len =
            checked_sum(OBJECT_ENVELOPE_HEADER_LEN, body_len, "retention manifest envelope")?;
        Ok(Self {
            current,
            historical,
            body_len,
            canonical_len,
        })
    }

    pub const fn current(&self) -> RetainedStateV1 { self.current }

    pub fn historical(&self) -> &[RetainedStateV1] { &self.historical }

    pub fn object_ref(&self) -> ObjectRefV1 {
        ObjectRefV1::new(self.canonical_digest(), self.canonical_len)
            .expect("validated retention-manifest length includes its envelope")
    }
}

impl CanonicalEncode for RetentionManifestV1 {
    fn canonical_len(&self) -> u64 { self.canonical_len }

    fn write_canonical<S: CanonicalSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        write_envelope_header(sink, RETENTION_MANIFEST_DOMAIN, self.body_len)?;
        self.current.write(sink)?;
        write_u64(sink, self.historical.len() as u64)?;
        for retained in &self.historical {
            retained.write(sink)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RetentionManifestView<'a> {
    bytes: &'a [u8],
    current: RetainedStateV1,
    historical: &'a [u8],
    historical_count: usize,
}

impl<'a> RetentionManifestView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, CanonicalFormatError> {
        let body = parse_envelope(bytes, RETENTION_MANIFEST_DOMAIN, "RSpace retention manifest")?;
        let mut cursor = ReadCursor::new(body);
        let current = RetainedStateV1::parse(&mut cursor)?;
        let count_u64 = cursor.read_u64()?;
        let count =
            usize::try_from(count_u64).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                field: "retained historical states",
                value: count_u64,
            })?;
        let byte_len = count.checked_mul(STATE_ENTRY_LEN as usize).ok_or(
            CanonicalFormatError::LengthOverflow {
                field: "retained historical states",
            },
        )?;
        let historical = cursor.read_exact(byte_len)?;
        cursor.finish()?;

        let mut parsed = RetainedStateIter {
            cursor: ReadCursor::new(historical),
            remaining: count,
        };
        let mut previous = None;
        for retained in &mut parsed {
            let retained = retained?;
            if retained.state() == current.state() {
                return Err(CanonicalFormatError::RetentionContainsCurrent);
            }
            if previous.is_some_and(|previous| !state_precedes(previous, retained.state())) {
                return Err(CanonicalFormatError::RetainedStatesNotStrict);
            }
            previous = Some(retained.state());
        }

        Ok(Self {
            bytes,
            current,
            historical,
            historical_count: count,
        })
    }

    pub const fn current(self) -> RetainedStateV1 { self.current }

    pub fn historical(self) -> RetainedStateIter<'a> {
        RetainedStateIter {
            cursor: ReadCursor::new(self.historical),
            remaining: self.historical_count,
        }
    }

    pub const fn canonical_bytes(self) -> &'a [u8] { self.bytes }

    pub fn object_ref(self) -> ObjectRefV1 {
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(self.bytes), self.bytes.len() as u64)
            .expect("parsed retention-manifest length includes its envelope")
    }
}

pub struct RetainedStateIter<'a> {
    cursor: ReadCursor<'a>,
    remaining: usize,
}

impl Iterator for RetainedStateIter<'_> {
    type Item = Result<RetainedStateV1, CanonicalFormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            debug_assert_eq!(self.cursor.remaining(), 0);
            return None;
        }
        self.remaining -= 1;
        Some(RetainedStateV1::parse(&mut self.cursor))
    }

    fn size_hint(&self) -> (usize, Option<usize>) { (self.remaining, Some(self.remaining)) }
}

impl ExactSizeIterator for RetainedStateIter<'_> {}

fn validate_historical(
    current: RetainedStateV1,
    historical: &[RetainedStateV1],
) -> Result<(), CanonicalFormatError> {
    let mut previous = None;
    for retained in historical {
        if retained.state() == current.state() {
            return Err(CanonicalFormatError::RetentionContainsCurrent);
        }
        if previous.is_some_and(|previous| !state_precedes(previous, retained.state())) {
            return Err(CanonicalFormatError::RetainedStatesNotStrict);
        }
        previous = Some(retained.state());
    }
    Ok(())
}

fn state_precedes(left: StateVersionV1, right: StateVersionV1) -> bool {
    (left.generation(), left.manifest()) < (right.generation(), right.manifest())
}

const _: () = assert!(STATE_ENTRY_LEN == 56);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RetainedRootKind {
    Current,
    Historical,
    Known,
    Import,
    Export,
    Reader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RetainedRootV1 {
    root: ObjectRefV1,
    kind: RetainedRootKind,
    lease_id: u64,
}

impl RetainedRootV1 {
    pub const fn root(self) -> ObjectRefV1 { self.root }
    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this accessor yet"
    )]
    pub const fn kind(self) -> RetainedRootKind { self.kind }
    pub const fn lease_id(self) -> u64 { self.lease_id }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedRootSnapshotV1 {
    revision: u64,
    roots: Box<[RetainedRootV1]>,
}

impl RetainedRootSnapshotV1 {
    pub const fn revision(&self) -> u64 { self.revision }
    pub fn roots(&self) -> &[RetainedRootV1] { &self.roots }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this method yet"
    )]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = RetainedRootV1> + '_ {
        self.roots.iter().copied()
    }

    /// Borrow the exact object references needed to seed a closure walk. This
    /// is a zero-allocation projection over the immutable snapshot; callers
    /// remain responsible for digest-level deduplication in their PDA.
    pub fn object_refs(&self) -> impl ExactSizeIterator<Item = ObjectRefV1> + '_ {
        self.roots.iter().map(|retained| retained.root)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionRegistryError {
    Poisoned,
    CounterOverflow,
    UnknownLease(u64),
    Sealed,
}

impl Display for RetentionRegistryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => f.write_str("retained-root registry lock is poisoned"),
            Self::CounterOverflow => f.write_str("retained-root registry counter overflow"),
            Self::UnknownLease(id) => write!(f, "unknown retained-root lease {id}"),
            Self::Sealed => {
                f.write_str("retained-root registry is sealed while a generation is published")
            }
        }
    }
}

impl std::error::Error for RetentionRegistryError {}

#[derive(Default)]
struct RegistryState {
    revision: u64,
    next_lease_id: u64,
    roots: std::collections::BTreeMap<u64, RetainedRootV1>,
    sealed: bool,
}

#[derive(Clone, Default)]
pub struct RetainedRootRegistryV1 {
    state: Arc<Mutex<RegistryState>>,
}

impl std::fmt::Debug for RetainedRootRegistryV1 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedRootRegistryV1")
            .finish_non_exhaustive()
    }
}

impl RetainedRootRegistryV1 {
    pub fn register(
        &self,
        root: ObjectRefV1,
        kind: RetainedRootKind,
    ) -> Result<RootLeaseV1, RetentionRegistryError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| RetentionRegistryError::Poisoned)?;
        if state.sealed {
            return Err(RetentionRegistryError::Sealed);
        }
        let id = state
            .next_lease_id
            .checked_add(1)
            .ok_or(RetentionRegistryError::CounterOverflow)?;
        let revision = state
            .revision
            .checked_add(1)
            .ok_or(RetentionRegistryError::CounterOverflow)?;
        state.next_lease_id = id;
        state.revision = revision;
        state.roots.insert(id, RetainedRootV1 {
            root,
            kind,
            lease_id: id,
        });
        Ok(RootLeaseV1 {
            lease_id: id,
            registry: Arc::clone(&self.state),
            active: true,
        })
    }

    pub fn snapshot(&self) -> Result<RetainedRootSnapshotV1, RetentionRegistryError> {
        let state = self
            .state
            .lock()
            .map_err(|_| RetentionRegistryError::Poisoned)?;
        let mut roots: Vec<_> = state.roots.values().copied().collect();
        roots.sort_unstable();
        Ok(RetainedRootSnapshotV1 {
            revision: state.revision,
            roots: roots.into_boxed_slice(),
        })
    }

    /// Snapshot dynamic leases together with the roots authenticated by the
    /// supplied retention manifest. Manifest roots use lease id zero and are
    /// never releasable through this registry; callers may therefore retain
    /// the returned immutable root set for the complete mark/copy operation.
    pub fn snapshot_with_manifest(
        &self,
        manifest: &RetentionManifestV1,
    ) -> Result<RetainedRootSnapshotV1, RetentionRegistryError> {
        let mut snapshot = self.snapshot()?;
        let mut roots = snapshot.roots.into_vec();
        roots.push(RetainedRootV1 {
            root: manifest.current().state().manifest(),
            kind: RetainedRootKind::Current,
            lease_id: 0,
        });
        roots.extend(manifest.historical().iter().map(|retained| RetainedRootV1 {
            root: retained.state().manifest(),
            kind: RetainedRootKind::Historical,
            lease_id: 0,
        }));
        roots.sort_unstable();
        roots.dedup();
        snapshot.roots = roots.into_boxed_slice();
        Ok(snapshot)
    }

    /// Check whether the dynamic lease portion of a snapshot is still the
    /// registry state. A compactor must perform this check immediately before
    /// reclaiming superseded storage; a changed result requires a fresh mark
    /// pass rather than deletion based on stale lease observations.
    pub fn is_snapshot_current(
        &self,
        snapshot: &RetainedRootSnapshotV1,
    ) -> Result<bool, RetentionRegistryError> {
        let state = self
            .state
            .lock()
            .map_err(|_| RetentionRegistryError::Poisoned)?;
        if state.revision != snapshot.revision {
            return Ok(false);
        }
        let dynamic_count = state.roots.len();
        let snapshot_dynamic_count = snapshot
            .roots
            .iter()
            .filter(|root| root.lease_id != 0)
            .count();
        if dynamic_count != snapshot_dynamic_count {
            return Ok(false);
        }
        Ok(snapshot
            .roots
            .iter()
            .filter(|candidate| candidate.lease_id != 0)
            .all(|candidate| state.roots.get(&candidate.lease_id) == Some(candidate)))
    }

    pub(crate) fn seal(&self) -> Result<RetainedRootSealV1, RetentionRegistryError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| RetentionRegistryError::Poisoned)?;
        if state.sealed {
            return Err(RetentionRegistryError::Sealed);
        }
        state.sealed = true;
        Ok(RetainedRootSealV1 {
            registry: Arc::clone(&self.state),
        })
    }

    #[expect(
        dead_code,
        reason = "no code in the crate, unit tests included, calls this method yet"
    )]
    pub fn release(&self, lease: &mut RootLeaseV1) -> Result<(), RetentionRegistryError> {
        lease.release()
    }
}

pub(crate) struct RetainedRootSealV1 {
    registry: Arc<Mutex<RegistryState>>,
}

impl Drop for RetainedRootSealV1 {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.lock() {
            state.sealed = false;
        }
    }
}

pub struct RootLeaseV1 {
    lease_id: u64,
    registry: Arc<Mutex<RegistryState>>,
    active: bool,
}

impl RootLeaseV1 {
    pub const fn lease_id(&self) -> u64 { self.lease_id }
    pub fn release(&mut self) -> Result<(), RetentionRegistryError> {
        if !self.active {
            return Err(RetentionRegistryError::UnknownLease(self.lease_id));
        }
        let mut state = self
            .registry
            .lock()
            .map_err(|_| RetentionRegistryError::Poisoned)?;
        if !state.roots.contains_key(&self.lease_id) {
            return Err(RetentionRegistryError::UnknownLease(self.lease_id));
        }
        let revision = state
            .revision
            .checked_add(1)
            .ok_or(RetentionRegistryError::CounterOverflow)?;
        state.roots.remove(&self.lease_id);
        state.revision = revision;
        self.active = false;
        Ok(())
    }
}

impl Drop for RootLeaseV1 {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut state) = self.registry.lock() {
            if state.roots.remove(&self.lease_id).is_some() {
                state.revision = state.revision.saturating_add(1);
            }
        }
    }
}

/// Immutable identity of the physical generation observed by a reader.
///
/// The key intentionally excludes filesystem paths: a path can be replaced
/// while an already-open reader remains valid. Catalog epochs can share pack
/// ranges, so reclamation checks physical overlap across keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenerationLeaseKeyV1 {
    epoch: u64,
    first_live_pack: u64,
    active_pack: u64,
    index_generation: u64,
}

impl GenerationLeaseKeyV1 {
    pub(crate) const fn new(
        epoch: u64,
        first_live_pack: u64,
        active_pack: u64,
        index_generation: u64,
    ) -> Self {
        Self {
            epoch,
            first_live_pack,
            active_pack,
            index_generation,
        }
    }

    pub const fn from_catalog(catalog: CatalogSlotV1) -> Self {
        let pack = catalog.pack();
        Self {
            epoch: catalog.epoch(),
            first_live_pack: pack.first_live_pack(),
            active_pack: pack.active_pack(),
            index_generation: pack.index_generation(),
        }
    }

    pub const fn epoch(self) -> u64 { self.epoch }
    pub const fn first_live_pack(self) -> u64 { self.first_live_pack }
    pub const fn active_pack(self) -> u64 { self.active_pack }
    pub const fn index_generation(self) -> u64 { self.index_generation }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationLeaseError {
    Poisoned,
    CounterOverflow,
    Retired(GenerationLeaseKeyV1),
    UnknownLease(u64),
}

impl Display for GenerationLeaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => f.write_str("generation lease registry lock is poisoned"),
            Self::CounterOverflow => f.write_str("generation lease counter overflow"),
            Self::Retired(key) => write!(f, "physical generation {key:?} is being reclaimed"),
            Self::UnknownLease(id) => write!(f, "unknown generation lease {id}"),
        }
    }
}

impl std::error::Error for GenerationLeaseError {}

#[derive(Debug, Default)]
struct GenerationLeaseState {
    next_lease_id: u64,
    leases: std::collections::BTreeMap<u64, GenerationLeaseKeyV1>,
    retired_pack_through: u64,
    retained_roots: RetainedRootRegistryV1,
    #[cfg(unix)]
    directory_identity: Option<(u64, u64)>,
    _directory_guard: Option<std::fs::File>,
}

#[cfg(unix)]
#[derive(Default)]
struct DirectoryLeaseRegistries {
    live: BTreeMap<(u64, u64), Weak<Mutex<GenerationLeaseState>>>,
    retired: BTreeMap<(u64, u64), Arc<Mutex<GenerationLeaseState>>>,
}

#[cfg(unix)]
fn directory_lease_registries() -> &'static Mutex<DirectoryLeaseRegistries> {
    static REGISTRIES: OnceLock<Mutex<DirectoryLeaseRegistries>> = OnceLock::new();
    REGISTRIES.get_or_init(Mutex::default)
}

/// Process-local generation lease authority.  The registry is deliberately
/// separate from retained semantic roots: a reader may retain a physical
/// generation even when it does not introduce a new semantic root.
#[derive(Clone, Debug, Default)]
pub struct GenerationLeaseRegistryV1 {
    state: Arc<Mutex<GenerationLeaseState>>,
}

impl GenerationLeaseRegistryV1 {
    #[cfg(unix)]
    pub(crate) fn for_directory_identity(
        identity: (u64, u64),
        directory_guard: std::fs::File,
    ) -> Result<Self, GenerationLeaseError> {
        let mut registries = directory_lease_registries()
            .lock()
            .map_err(|_| GenerationLeaseError::Poisoned)?;
        if let Some(state) = registries.retired.get(&identity) {
            return Ok(Self {
                state: Arc::clone(state),
            });
        }
        if let Some(state) = registries.live.get(&identity).and_then(Weak::upgrade) {
            return Ok(Self { state });
        }
        registries.live.retain(|_, state| state.strong_count() > 0);
        let state = Arc::new(Mutex::new(GenerationLeaseState {
            directory_identity: Some(identity),
            _directory_guard: Some(directory_guard),
            ..GenerationLeaseState::default()
        }));
        registries.live.insert(identity, Arc::downgrade(&state));
        Ok(Self { state })
    }

    #[cfg(test)]
    pub(crate) fn replace_lease_counter_for_test(&self, counter: u64) -> u64 {
        std::mem::replace(&mut self.state.lock().unwrap().next_lease_id, counter)
    }

    #[cfg(test)]
    pub(crate) fn active_lease_count_for_test(&self) -> usize {
        self.state
            .lock()
            .expect("generation lease registry lock")
            .leases
            .len()
    }

    pub(crate) fn shares_registry(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub fn acquire(
        &self,
        key: GenerationLeaseKeyV1,
    ) -> Result<GenerationLeaseV1, GenerationLeaseError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GenerationLeaseError::Poisoned)?;
        if key.first_live_pack <= state.retired_pack_through {
            return Err(GenerationLeaseError::Retired(key));
        }
        let lease_id = state
            .next_lease_id
            .checked_add(1)
            .ok_or(GenerationLeaseError::CounterOverflow)?;
        state.next_lease_id = lease_id;
        state.leases.insert(lease_id, key);
        Ok(GenerationLeaseV1 {
            lease_id,
            key,
            registry: Arc::clone(&self.state),
            active: true,
        })
    }

    /// Test exact catalog-key activity. Reclamation must use
    /// `retire_pack_range_if_unleased` to account for overlapping epochs.
    pub fn has_active(&self, key: GenerationLeaseKeyV1) -> Result<bool, GenerationLeaseError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GenerationLeaseError::Poisoned)?;
        Ok(state.leases.values().any(|candidate| *candidate == key))
    }

    /// Atomically exclude new leases for every pack that reclamation will
    /// remove. Incremental catalog epochs share a physical pack range, so an
    /// exact catalog-key check cannot establish that no reader needs it.
    /// The monotonic pack frontier remains excluded after partial deletion;
    /// it does not accumulate one tombstone per catalog epoch.
    pub(crate) fn retire_pack_range_if_unleased(
        &self,
        key: GenerationLeaseKeyV1,
    ) -> Result<bool, GenerationLeaseError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GenerationLeaseError::Poisoned)?;
        if state.leases.values().any(|candidate| {
            candidate.first_live_pack <= key.active_pack &&
                key.first_live_pack <= candidate.active_pack
        }) {
            return Ok(false);
        }
        state.retired_pack_through = state.retired_pack_through.max(key.active_pack);
        #[cfg(unix)]
        if let Some(identity) = state.directory_identity {
            let mut registries = directory_lease_registries()
                .lock()
                .map_err(|_| GenerationLeaseError::Poisoned)?;
            match registries.retired.get(&identity) {
                Some(existing) if !Arc::ptr_eq(existing, &self.state) => {
                    return Err(GenerationLeaseError::Poisoned);
                }
                Some(_) => {}
                None => {
                    registries.retired.insert(identity, Arc::clone(&self.state));
                }
            }
        }
        Ok(true)
    }

    pub fn release(&self, lease: &mut GenerationLeaseV1) -> Result<(), GenerationLeaseError> {
        lease.release()
    }

    pub(crate) fn retained_roots(&self) -> Result<RetainedRootRegistryV1, RetentionRegistryError> {
        let state = self
            .state
            .lock()
            .map_err(|_| RetentionRegistryError::Poisoned)?;
        Ok(state.retained_roots.clone())
    }

    pub(crate) fn register_known_root(
        &self,
        root: ObjectRefV1,
        generation: GenerationLeaseKeyV1,
    ) -> Result<KnownRootLeaseV1, KnownRootLeaseError> {
        let generation_lease = self.acquire(generation)?;
        let root_lease = self
            .retained_roots()?
            .register(root, RetainedRootKind::Known)?;
        Ok(KnownRootLeaseV1 {
            root: root_lease,
            generation: generation_lease,
        })
    }
}

pub struct GenerationLeaseV1 {
    lease_id: u64,
    key: GenerationLeaseKeyV1,
    registry: Arc<Mutex<GenerationLeaseState>>,
    active: bool,
}

impl GenerationLeaseV1 {
    pub const fn lease_id(&self) -> u64 { self.lease_id }
    pub const fn key(&self) -> GenerationLeaseKeyV1 { self.key }

    pub fn release(&mut self) -> Result<(), GenerationLeaseError> {
        if !self.active {
            return Err(GenerationLeaseError::UnknownLease(self.lease_id));
        }
        let mut state = self
            .registry
            .lock()
            .map_err(|_| GenerationLeaseError::Poisoned)?;
        if state.leases.remove(&self.lease_id) != Some(self.key) {
            return Err(GenerationLeaseError::UnknownLease(self.lease_id));
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for GenerationLeaseV1 {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut state) = self.registry.lock() {
            state.leases.remove(&self.lease_id);
        }
    }
}

pub(crate) struct KnownRootLeaseV1 {
    root: RootLeaseV1,
    generation: GenerationLeaseV1,
}

impl KnownRootLeaseV1 {
    pub(crate) const fn lease_id(&self) -> u64 { self.root.lease_id() }

    pub(crate) const fn generation(&self) -> GenerationLeaseKeyV1 { self.generation.key() }

    pub(crate) fn release(&mut self) -> Result<(), KnownRootLeaseError> {
        self.root.release()?;
        self.generation.release()?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum KnownRootLeaseError {
    Generation(GenerationLeaseError),
    Registry(RetentionRegistryError),
    Absent {
        root: ObjectRefV1,
        generation: GenerationLeaseKeyV1,
    },
    Read(Box<super::durable::DurableObjectReadErrorV1>),
}

impl Display for KnownRootLeaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generation(error) => Display::fmt(error, f),
            Self::Registry(error) => Display::fmt(error, f),
            Self::Absent { root, generation } => write!(
                f,
                "known root {root:?} is not an object of physical generation {generation:?}"
            ),
            Self::Read(error) => Display::fmt(error, f),
        }
    }
}

impl std::error::Error for KnownRootLeaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Generation(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::Read(error) => Some(error.as_ref()),
            Self::Absent { .. } => None,
        }
    }
}

impl From<GenerationLeaseError> for KnownRootLeaseError {
    fn from(error: GenerationLeaseError) -> Self { Self::Generation(error) }
}

impl From<RetentionRegistryError> for KnownRootLeaseError {
    fn from(error: RetentionRegistryError) -> Self { Self::Registry(error) }
}

impl From<super::durable::DurableObjectReadErrorV1> for KnownRootLeaseError {
    fn from(error: super::durable::DurableObjectReadErrorV1) -> Self { Self::Read(Box::new(error)) }
}

#[cfg(test)]
mod registry_tests {
    use proptest::prelude::*;

    use super::*;

    fn reference(byte: u8) -> ObjectRefV1 {
        ObjectRefV1::new(ObjectDigest::from_array([byte; 32]), OBJECT_ENVELOPE_HEADER_LEN).unwrap()
    }

    #[test]
    fn snapshot_is_sorted_and_drop_releases_lease() {
        let registry = RetainedRootRegistryV1::default();
        let first = registry
            .register(reference(2), RetainedRootKind::Reader)
            .unwrap();
        let mut second = registry
            .register(reference(1), RetainedRootKind::Import)
            .unwrap();
        let snapshot = registry.snapshot().unwrap();
        assert_eq!(snapshot.revision(), 2);
        assert_eq!(snapshot.roots()[0].root().digest(), reference(1).digest());
        assert_eq!(snapshot.roots()[1].root().digest(), reference(2).digest());
        drop(first);
        second.release().unwrap();
        assert!(registry.snapshot().unwrap().roots().is_empty());
    }

    #[test]
    fn explicit_release_is_transactional_on_unknown_lease() {
        let registry = RetainedRootRegistryV1::default();
        let mut lease = registry
            .register(reference(3), RetainedRootKind::Known)
            .unwrap();
        let id = lease.lease_id();
        lease.release().unwrap();
        assert_eq!(registry.snapshot().unwrap().revision(), 2);
        let mut fake = RootLeaseV1 {
            lease_id: id,
            registry: Arc::clone(&registry.state),
            active: true,
        };
        assert_eq!(fake.release(), Err(RetentionRegistryError::UnknownLease(id)));
        assert_eq!(registry.snapshot().unwrap().revision(), 2);
    }

    #[test]
    fn release_overflow_preserves_root_and_lease_for_retry() {
        let registry = RetainedRootRegistryV1::default();
        let mut lease = registry
            .register(reference(4), RetainedRootKind::Reader)
            .unwrap();
        {
            let mut state = registry.state.lock().unwrap();
            state.revision = u64::MAX;
        }

        assert_eq!(lease.release(), Err(RetentionRegistryError::CounterOverflow));
        let snapshot = registry.snapshot().unwrap();
        assert_eq!(snapshot.revision(), u64::MAX);
        assert_eq!(snapshot.roots(), &[RetainedRootV1 {
            root: reference(4),
            kind: RetainedRootKind::Reader,
            lease_id: lease.lease_id(),
        }]);
    }

    #[test]
    fn manifest_roots_are_added_to_dynamic_snapshot_deterministically() {
        let registry = RetainedRootRegistryV1::default();
        let current_manifest = reference(8);
        let historical_manifest = reference(7);
        let current = RetainedStateV1::new(
            StateVersionV1::new(2, current_manifest),
            current_manifest.encoded_len(),
        )
        .unwrap();
        let historical = RetainedStateV1::new(
            StateVersionV1::new(1, historical_manifest),
            historical_manifest.encoded_len(),
        )
        .unwrap();
        let manifest = RetentionManifestV1::new(current, [historical]).unwrap();
        let lease = registry
            .register(reference(9), RetainedRootKind::Export)
            .unwrap();
        let snapshot = registry.snapshot_with_manifest(&manifest).unwrap();

        assert_eq!(snapshot.roots().len(), 3);
        assert_eq!(snapshot.roots()[0].root(), historical_manifest);
        assert_eq!(snapshot.roots()[1].root(), current_manifest);
        assert_eq!(snapshot.roots()[2].root(), reference(9));
        assert_eq!(snapshot.roots()[0].lease_id(), 0);
        assert_eq!(snapshot.roots()[1].lease_id(), 0);
        assert_eq!(snapshot.object_refs().collect::<Vec<_>>(), vec![
            historical_manifest,
            current_manifest,
            reference(9),
        ]);
        drop(lease);
    }

    #[test]
    fn snapshot_current_rejects_lease_changes() {
        let registry = RetainedRootRegistryV1::default();
        let mut lease = registry
            .register(reference(10), RetainedRootKind::Reader)
            .unwrap();
        let snapshot = registry.snapshot().unwrap();
        assert!(registry.is_snapshot_current(&snapshot).unwrap());
        lease.release().unwrap();
        assert!(!registry.is_snapshot_current(&snapshot).unwrap());
    }

    #[test]
    fn generation_registry_blocks_reclamation_until_last_reader_releases() {
        let registry = GenerationLeaseRegistryV1::default();
        let key = GenerationLeaseKeyV1 {
            epoch: 7,
            first_live_pack: 3,
            active_pack: 5,
            index_generation: 9,
        };
        let mut first = registry.acquire(key).unwrap();
        let second = registry.acquire(key).unwrap();
        assert!(registry.has_active(key).unwrap());
        first.release().unwrap();
        assert!(registry.has_active(key).unwrap());
        drop(second);
        assert!(!registry.has_active(key).unwrap());
        assert!(registry.retire_pack_range_if_unleased(key).unwrap());
        assert!(matches!(
            registry.acquire(key),
            Err(GenerationLeaseError::Retired(retired)) if retired == key
        ));
        assert!(registry.retire_pack_range_if_unleased(key).unwrap());
        let successor = GenerationLeaseKeyV1::new(8, 6, 7, 10);
        let successor_reader = registry.acquire(successor).unwrap();
        assert!(!registry.retire_pack_range_if_unleased(successor).unwrap());
        drop(successor_reader);
        assert!(registry.retire_pack_range_if_unleased(successor).unwrap());
        assert!(matches!(
            registry.acquire(successor),
            Err(GenerationLeaseError::Retired(retired)) if retired == successor
        ));
        assert!(
            registry
                .acquire(GenerationLeaseKeyV1::new(9, 8, 9, 11))
                .is_ok()
        );
    }

    #[test]
    fn reclamation_waits_for_overlapping_physical_packs_across_catalog_epochs() {
        let registry = GenerationLeaseRegistryV1::default();
        let old = GenerationLeaseKeyV1::new(7, 3, 5, 9);
        let same_packs_older_epoch = GenerationLeaseKeyV1::new(6, 3, 5, 9);
        let overlapping_packs = GenerationLeaseKeyV1::new(8, 5, 7, 10);
        let disjoint_packs = GenerationLeaseKeyV1::new(9, 6, 7, 11);

        let older_reader = registry.acquire(same_packs_older_epoch).unwrap();
        let overlapping_reader = registry.acquire(overlapping_packs).unwrap();
        let successor_reader = registry.acquire(disjoint_packs).unwrap();
        assert!(!registry.retire_pack_range_if_unleased(old).unwrap());
        drop(older_reader);
        assert!(!registry.retire_pack_range_if_unleased(old).unwrap());
        drop(overlapping_reader);
        assert!(registry.retire_pack_range_if_unleased(old).unwrap());
        assert!(matches!(
            registry.acquire(same_packs_older_epoch),
            Err(GenerationLeaseError::Retired(key)) if key == same_packs_older_epoch
        ));
        assert!(matches!(
            registry.acquire(overlapping_packs),
            Err(GenerationLeaseError::Retired(key)) if key == overlapping_packs
        ));
        assert_eq!(successor_reader.key(), disjoint_packs);
        assert!(registry.acquire(disjoint_packs).is_ok());
    }

    #[test]
    fn generation_lease_acquisition_and_retirement_have_one_winner() {
        use std::sync::{Arc, Barrier};

        for epoch in 1..=16 {
            let registry = GenerationLeaseRegistryV1::default();
            let key = GenerationLeaseKeyV1::new(epoch, 3, 5, 9);
            let start = Arc::new(Barrier::new(3));
            let acquire_registry = registry.clone();
            let acquire_start = Arc::clone(&start);
            let acquire = std::thread::spawn(move || {
                acquire_start.wait();
                acquire_registry.acquire(key)
            });
            let retire_registry = registry.clone();
            let retire_start = Arc::clone(&start);
            let retire = std::thread::spawn(move || {
                retire_start.wait();
                retire_registry.retire_pack_range_if_unleased(key)
            });
            start.wait();
            let acquired = acquire.join().unwrap();
            let retired = retire.join().unwrap().unwrap();
            match acquired {
                Ok(lease) => {
                    assert!(!retired);
                    assert_eq!(lease.key(), key);
                    drop(lease);
                    assert!(registry.retire_pack_range_if_unleased(key).unwrap());
                }
                Err(GenerationLeaseError::Retired(retired_key)) => {
                    assert!(retired);
                    assert_eq!(retired_key, key);
                }
                Err(error) => panic!("unexpected lease acquisition error: {error}"),
            }
            assert!(matches!(
                registry.acquire(key),
                Err(GenerationLeaseError::Retired(retired_key)) if retired_key == key
            ));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 128,
            rng_seed: proptest::test_runner::RngSeed::Fixed(92435926),
            ..ProptestConfig::default()
        })]

        #[test]
        fn generated_pack_range_retirement_preserves_overlap_and_frontier(
            first in 1u64..128,
            old_len in 1u64..16,
            overlap_extension in 1u64..16,
            successor_len in 1u64..16,
            epoch in 1u64..100_000,
            index in 1u64..100_000,
        ) {
            let last = first + old_len - 1;
            let old = GenerationLeaseKeyV1::new(epoch, first, last, index);
            let older = GenerationLeaseKeyV1::new(epoch - 1, first, last, index);
            let partial = GenerationLeaseKeyV1::new(
                epoch + 1,
                last,
                last + overlap_extension,
                index + 1,
            );
            let successor = GenerationLeaseKeyV1::new(
                epoch + 2,
                last + 1,
                last + successor_len,
                index + 2,
            );
            let registry = GenerationLeaseRegistryV1::default();
            let older_pin = registry.acquire(older).unwrap();
            let partial_pin = registry.acquire(partial).unwrap();
            let successor_pin = registry.acquire(successor).unwrap();

            prop_assert!(!registry.retire_pack_range_if_unleased(old).unwrap());
            drop(older_pin);
            prop_assert!(!registry.retire_pack_range_if_unleased(old).unwrap());
            drop(partial_pin);
            prop_assert!(registry.retire_pack_range_if_unleased(old).unwrap());
            prop_assert_eq!(registry.state.lock().unwrap().retired_pack_through, last);
            prop_assert!(matches!(registry.acquire(old), Err(GenerationLeaseError::Retired(key)) if key == old));
            prop_assert!(matches!(registry.acquire(older), Err(GenerationLeaseError::Retired(key)) if key == older));
            prop_assert!(matches!(registry.acquire(partial), Err(GenerationLeaseError::Retired(key)) if key == partial));
            prop_assert!(registry.retire_pack_range_if_unleased(old).unwrap());
            prop_assert_eq!(registry.state.lock().unwrap().retired_pack_through, last);
            prop_assert!(!registry.retire_pack_range_if_unleased(successor).unwrap());

            drop(successor_pin);
            prop_assert!(registry.acquire(successor).is_ok());
            prop_assert_eq!(registry.state.lock().unwrap().retired_pack_through, last);
            prop_assert!(registry.retire_pack_range_if_unleased(successor).unwrap());
            prop_assert_eq!(registry.state.lock().unwrap().retired_pack_through, successor.active_pack());
            prop_assert!(matches!(registry.acquire(successor), Err(GenerationLeaseError::Retired(key)) if key == successor));
            prop_assert!(registry.acquire(GenerationLeaseKeyV1::new(
                epoch + 3,
                successor.active_pack() + 1,
                successor.active_pack() + 2,
                index + 3,
            )).is_ok());
        }
    }
}
