//! Canonical retained-root authority for bounded durable compaction.
//!
//! The manifest names semantic state roots, not pack offsets. Compaction can
//! therefore rewrite their shared Merkle closure into a new pack generation
//! without changing logical state identities. Decoding is borrowed and
//! streaming; no root projection is required.

use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};

use super::canonical::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, OBJECT_DOMAIN_LEN,
    OBJECT_ENVELOPE_HEADER_LEN, ObjectDigest, ObjectRefV1, ReadCursor, checked_len, checked_sum,
    parse_envelope, write_envelope_header, write_u64,
};
use super::mutation::StateVersionV1;

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
}

impl Display for RetentionRegistryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => f.write_str("retained-root registry lock is poisoned"),
            Self::CounterOverflow => f.write_str("retained-root registry counter overflow"),
            Self::UnknownLease(id) => write!(f, "unknown retained-root lease {id}"),
        }
    }
}

impl std::error::Error for RetentionRegistryError {}

#[derive(Default)]
struct RegistryState {
    revision: u64,
    next_lease_id: u64,
    roots: std::collections::BTreeMap<u64, RetainedRootV1>,
}

#[derive(Clone, Default)]
pub struct RetainedRootRegistryV1 {
    state: Arc<Mutex<RegistryState>>,
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
        let mut dynamic = state.roots.values().copied().collect::<Vec<_>>();
        dynamic.sort_unstable();
        Ok(snapshot
            .roots
            .iter()
            .filter(|root| root.lease_id != 0)
            .copied()
            .eq(dynamic))
    }

    pub fn release(&self, lease: &mut RootLeaseV1) -> Result<(), RetentionRegistryError> {
        lease.release()
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

#[cfg(test)]
mod registry_tests {
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
}
