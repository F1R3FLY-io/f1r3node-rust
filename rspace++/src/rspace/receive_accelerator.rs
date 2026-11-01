use std::sync::Arc;

use pathmap::PathMap;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperReadOnlyIteration, ZipperValues};

use super::hashing::stable_hash_provider::StableHashSerialize;
use super::hot_store::ReceiveRevision;
use super::internal::WaitingContinuation;
use super::record_index::{RSpaceContinuationStore, RSpaceJoinStore, StoredRecord};
use super::rho_path::{RhoPathEncode, RhoPathError, RhoPathReceivePosition, RhoPathV1};

#[derive(Clone)]
pub(crate) struct ReceiveOccurrenceIndex {
    keys: PathMap<()>,
}

impl ReceiveOccurrenceIndex {
    pub(crate) fn new() -> Self {
        Self {
            keys: PathMap::new(),
        }
    }

    pub(crate) fn insert_group(&mut self, group_prefix: &[u8]) -> Result<(), RhoPathError> {
        let group = RhoPathV1::parse_continuation_group(group_prefix)?;
        for frame in group.frames() {
            self.keys.insert(occurrence_key(frame, group_prefix), ());
        }
        Ok(())
    }

    pub(crate) fn remove_group(&mut self, group_prefix: &[u8]) -> Result<(), RhoPathError> {
        let group = RhoPathV1::parse_continuation_group(group_prefix)?;
        for frame in group.frames() {
            self.keys.remove(occurrence_key(frame, group_prefix));
        }
        Ok(())
    }

    pub(crate) fn try_visit_groups_of<E>(
        &self,
        channel_frame: &[u8],
        mut visitor: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut cursor = self.keys.read_zipper_at_borrowed_path(channel_frame);
        if !cursor.path_exists() {
            return Ok(());
        }
        if cursor.val().is_some() {
            visitor(cursor.path())?;
        }
        while cursor.to_next_get_val().is_some() {
            visitor(cursor.path())?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn same_keys(&self, other: &Self) -> bool {
        self.keys
            .iter()
            .map(|(key, _)| key)
            .eq(other.keys.iter().map(|(key, _)| key))
    }

    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<Vec<u8>> {
        let mut keys = Vec::with_capacity(self.keys.val_count());
        keys.extend(self.keys.iter().map(|(key, _)| key));
        keys
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveAcceleratorClass {
    Absent,
    Building,
    Foreign,
    Stale,
    Incomplete,
    Coherent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveKind {
    Installed,
    Resting,
}

#[derive(Clone)]
pub struct ReceiveAcceleratorHandle<C, P, K>
where
    C: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    revision: ReceiveRevision,
    snapshot: RSpaceContinuationStore<C, P, K>,
}

impl<C, P, K> ReceiveAcceleratorHandle<C, P, K>
where
    C: Clone + RhoPathEncode + Send + Sync,
    P: Clone + StableHashSerialize + Send + Sync,
    K: Clone + StableHashSerialize + Send + Sync,
{
    pub(crate) fn new(
        revision: ReceiveRevision,
        snapshot: RSpaceContinuationStore<C, P, K>,
    ) -> Option<Self> {
        let indexed = snapshot.receive_index().is_some();
        indexed.then_some(Self { revision, snapshot })
    }

    pub fn revision(&self) -> &ReceiveRevision { &self.revision }

    pub(crate) fn scope(&self) -> &RSpaceContinuationStore<C, P, K> { &self.snapshot }

    pub(crate) fn token_class(&self, current: &ReceiveRevision) -> Option<ReceiveAcceleratorClass> {
        match (self.revision.shares_generation(current), self.revision == *current) {
            (false, _) => Some(ReceiveAcceleratorClass::Foreign),
            (true, false) => Some(ReceiveAcceleratorClass::Stale),
            (true, true) => None,
        }
    }

    pub(crate) fn materialize(&self, channel: &C) -> ReceiveAnswer<C, P, K> {
        let index = self
            .snapshot
            .receive_index()
            .expect("an accelerator handle holds a published occurrence index");
        let frame = channel_frame(channel);
        let mut groups = Vec::new();
        let visited = index.try_visit_groups_of(&frame, |prefix| {
            match self.snapshot.loaded_group_channels(prefix) {
                Some(channels) => {
                    groups.push(self.occurrence_group(Arc::clone(channels), prefix, &frame));
                    Ok(())
                }
                None => Err(ReceiveAcceleratorClass::Incomplete),
            }
        });
        match visited {
            Ok(()) => ReceiveAnswer::Exact(ReceiveOccurrences {
                revision: self.revision.clone(),
                groups,
            }),
            Err(class) => ReceiveAnswer::Fallback(class),
        }
    }

    fn occurrence_group(
        &self,
        channels: Arc<[C]>,
        prefix: &[u8],
        frame: &[u8],
    ) -> ReceiveOccurrenceGroup<C, P, K> {
        let shared_prefix: Arc<[u8]> = Arc::from(prefix);
        let installed = self.snapshot.installed_entry(prefix);
        let resting = self.snapshot.ordered_resting_records(prefix);
        let mut targets = Vec::with_capacity(
            resting
                .len()
                .saturating_add(usize::from(installed.is_some())),
        );
        targets.extend(installed.map(|entry| ReceiveTarget {
            prefix: Arc::clone(&shared_prefix),
            record: ReceiveRecord::Installed(Arc::clone(entry)),
        }));
        targets.extend(resting.into_iter().map(|record| ReceiveTarget {
            prefix: Arc::clone(&shared_prefix),
            record: ReceiveRecord::Resting(record),
        }));
        ReceiveOccurrenceGroup {
            channels,
            positions: positions_of(prefix, frame),
            prefix: shared_prefix,
            targets,
        }
    }
}

pub struct ReceiveTarget<C, P: Clone, K: Clone> {
    prefix: Arc<[u8]>,
    record: ReceiveRecord<C, P, K>,
}

enum ReceiveRecord<C, P: Clone, K: Clone> {
    Resting(Arc<StoredRecord<WaitingContinuation<P, K>>>),
    Installed(Arc<(Arc<[C]>, WaitingContinuation<P, K>)>),
}

impl<C, P: Clone, K: Clone> ReceiveTarget<C, P, K> {
    pub fn kind(&self) -> ReceiveKind {
        match self.record {
            ReceiveRecord::Resting(_) => ReceiveKind::Resting,
            ReceiveRecord::Installed(_) => ReceiveKind::Installed,
        }
    }

    pub fn group_prefix(&self) -> &[u8] { &self.prefix }

    pub fn continuation(&self) -> &WaitingContinuation<P, K> {
        match &self.record {
            ReceiveRecord::Resting(record) => record.value(),
            ReceiveRecord::Installed(entry) => &entry.1,
        }
    }
}

pub struct ReceiveOccurrenceGroup<C, P: Clone, K: Clone> {
    channels: Arc<[C]>,
    prefix: Arc<[u8]>,
    positions: Vec<RhoPathReceivePosition>,
    targets: Vec<ReceiveTarget<C, P, K>>,
}

impl<C, P: Clone, K: Clone> ReceiveOccurrenceGroup<C, P, K> {
    pub fn channels(&self) -> &[C] { &self.channels }

    pub fn prefix(&self) -> &[u8] { &self.prefix }

    pub fn positions(&self) -> &[RhoPathReceivePosition] { &self.positions }

    pub fn targets(&self) -> &[ReceiveTarget<C, P, K>] { &self.targets }
}

pub struct ReceiveOccurrences<C, P: Clone, K: Clone> {
    revision: ReceiveRevision,
    groups: Vec<ReceiveOccurrenceGroup<C, P, K>>,
}

impl<C, P: Clone, K: Clone> ReceiveOccurrences<C, P, K> {
    pub fn revision(&self) -> &ReceiveRevision { &self.revision }

    pub fn groups(&self) -> &[ReceiveOccurrenceGroup<C, P, K>] { &self.groups }
}

pub enum ReceiveAnswer<C, P: Clone, K: Clone> {
    Exact(ReceiveOccurrences<C, P, K>),
    Fallback(ReceiveAcceleratorClass),
}

pub(crate) fn join_scope_class<C, P, K>(
    scope: &RSpaceContinuationStore<C, P, K>,
    joins: &RSpaceJoinStore<C>,
    channel: &C,
) -> Option<ReceiveAcceleratorClass>
where
    C: Clone + RhoPathEncode + Send + Sync,
    P: Clone + StableHashSerialize + Send + Sync,
    K: Clone + StableHashSerialize + Send + Sync,
{
    let covered = joins.try_visit_join_groups(channel, |group| {
        scope
            .is_loaded_prefix(&RhoPathV1::continuation_prefix(group))
            .then_some(())
            .ok_or(())
    });
    match covered {
        Some(Ok(())) => None,
        Some(Err(())) | None => Some(ReceiveAcceleratorClass::Incomplete),
    }
}

fn occurrence_key(frame: &[u8], group_prefix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(frame.len().saturating_add(group_prefix.len()));
    key.extend_from_slice(frame);
    key.extend_from_slice(group_prefix);
    key
}

fn positions_of(group_prefix: &[u8], frame: &[u8]) -> Vec<RhoPathReceivePosition> {
    let group = RhoPathV1::parse_continuation_group(group_prefix)
        .expect("an indexed occurrence group prefix parses");
    let count = group
        .frames()
        .filter(|candidate| *candidate == frame)
        .count();
    let mut positions = Vec::with_capacity(count);
    positions.extend(
        group
            .frames()
            .enumerate()
            .filter(|(_, candidate)| *candidate == frame)
            .map(|(index, _)| RhoPathReceivePosition::at_index(index)),
    );
    positions
}

pub(crate) fn channel_frame<C: RhoPathEncode>(channel: &C) -> Vec<u8> {
    let prefix = RhoPathV1::continuation_prefix(std::slice::from_ref(channel));
    RhoPathV1::parse_continuation_group(&prefix)
        .ok()
        .and_then(|group| group.frames().next())
        .expect("a one-channel continuation prefix carries exactly one channel frame")
        .to_vec()
}

#[cfg(test)]
thread_local! {
    static MAINTENANCE_FAULT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn arm_maintenance_fault() { MAINTENANCE_FAULT.with(|armed| armed.set(true)); }

#[cfg(test)]
pub(crate) fn injected_maintenance_fault(before: bool, after: bool) -> bool {
    before != after && MAINTENANCE_FAULT.with(|armed| armed.replace(false))
}

#[cfg(test)]
#[path = "../../tests/receive_accelerator/occurrence_index.rs"]
mod occurrence_index_tests;
