//! PathMap-authoritative continuation and join collections.
//!
//! The public RSpace API still projects ordinary vectors and hash maps for
//! compatibility, but those projections are never retained as authority.
//! Exact channel-group prefixes select a compressed subtrie; complete frozen
//! record bytes select endpoints inside it. Consequently zipper order is the
//! cold-store's byte order by construction. Continuations additionally keep a
//! source-free identity lookup for historical first-representative
//! deduplication; that lookup shares record values and never carries a second
//! copy of the complete record. Live newest-first order is explicit fixed-width
//! metadata because trie order and scheduler order are different contracts.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use pathmap::PathMap;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperReadOnlyIteration, ZipperValues};

use super::bag::{BagDelta, Multiplicity};
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::hashing::stable_hash_provider::{StableHashSerialize, hash, hash_from_slice};
use super::history::cold_store::{CanonicalCollectionKind, PERSISTED_DATA_HEADER_LEN};
use super::internal::WaitingContinuation;
use super::rho_path::{RhoPathEncode, RhoPathV1};
use super::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into, encode_join_record,
};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalEncode, ContentTransition, ObjectRefV1,
};

fn continuation_source(identity: &[u8], canonical: &[u8]) -> (ObjectRefV1, Arc<[u8]>) {
    let source = canonical
        .get(identity.len()..)
        .expect("a continuation record contains its complete identity prefix");
    let blob = CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, source.to_vec())
        .expect("a continuation source payload is a valid canonical blob");
    let reference = blob.object_ref();
    let bytes = blob
        .try_canonical_bytes()
        .expect("a continuation source blob is canonically encodable");
    (reference, bytes.into())
}

fn record_content_transition(
    journal: &mut PathMap<ContentTransition>,
    path: &[u8],
    before: Option<ObjectRefV1>,
    after: Option<ObjectRefV1>,
) {
    if let Some(previous) = journal.get(path).copied() {
        assert_eq!(previous.after(), before, "continuation journal transition is discontinuous");
        if previous.before() == after {
            journal.remove_val_at(path, true);
        } else {
            journal.insert(
                path,
                ContentTransition::new(previous.before(), after)
                    .expect("a non-cancelled continuation transition is non-noop"),
            );
        }
    } else if before != after {
        journal.insert(
            path,
            ContentTransition::new(before, after)
                .expect("a new continuation transition is non-noop"),
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecordIndexError {
    EncodedCardinalityOverflow(u128),
    EncodedLengthOverflow(u128),
    AllocationFailure(usize),
    InvariantViolation(&'static str),
}

impl fmt::Display for RecordIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncodedCardinalityOverflow(cardinality) => write!(
                formatter,
                "record cardinality {cardinality} does not fit the frozen bincode u64 length"
            ),
            Self::EncodedLengthOverflow(length) => {
                write!(formatter, "encoded record sequence length {length} does not fit in memory")
            }
            Self::AllocationFailure(requested) => {
                write!(formatter, "unable to reserve {requested} bytes for canonical record egress")
            }
            Self::InvariantViolation(message) => {
                write!(formatter, "canonical record index invariant failed: {message}")
            }
        }
    }
}

impl std::error::Error for RecordIndexError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalRecordReplayError<E> {
    Plan(RecordIndexError),
    Sink(E),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalRecordGroup {
    pub kind: CanonicalCollectionKind,
    pub history_key_hash: Blake2b256Hash,
    pub encoded_persisted_data: Vec<u8>,
    pub record_count: u64,
}

/// A validated, immutable view of one canonical continuation/join collection.
///
/// The plan owns a CoW clone of the record trie root rather than a projected
/// record list. Mutations published through the live store therefore cannot
/// alter either the endpoints or bytes observed by a later replay. Callers
/// replay into private staging and publish only after `try_replay` succeeds.
#[derive(Clone)]
pub(crate) struct CanonicalRecordPlan<R: Clone + Send + Sync> {
    records: PathMap<Arc<StoredRecord<R>>>,
    prefix: Arc<[u8]>,
    header: [u8; PERSISTED_DATA_HEADER_LEN],
    next_order: u64,
    record_count: u64,
    encoded_entries_len: u128,
    encoded_len: usize,
}

impl<R: Clone + Send + Sync> CanonicalRecordPlan<R> {
    pub(crate) fn record_count(&self) -> u64 { self.record_count }

    pub(crate) fn encoded_len(&self) -> usize { self.encoded_len }

    /// Replay the preflighted frozen bytes directly from the PathMap cursor.
    ///
    /// The sink is intentionally fallible. It must be private staging: a
    /// caller may expose/commit its staged bytes only after this method
    /// returns `Ok(())`.
    pub(crate) fn try_replay<E>(
        &self,
        mut sink: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), CanonicalRecordReplayError<E>> {
        sink(&self.header).map_err(CanonicalRecordReplayError::Sink)?;
        sink(&self.record_count.to_le_bytes()).map_err(CanonicalRecordReplayError::Sink)?;

        let mut emitted_count = 0u128;
        let mut emitted_entries_len = 0u128;
        let mut cursor = self
            .records
            .read_zipper_at_borrowed_path(self.prefix.as_ref());
        if cursor.path_exists() {
            if let Some(record) = cursor.val() {
                self.replay_record(
                    cursor.path(),
                    record,
                    &mut emitted_count,
                    &mut emitted_entries_len,
                    &mut sink,
                )?;
            }
            while let Some(record) = cursor.to_next_get_val() {
                self.replay_record(
                    cursor.path(),
                    record,
                    &mut emitted_count,
                    &mut emitted_entries_len,
                    &mut sink,
                )?;
            }
        }
        if emitted_count != self.record_count as u128 {
            return Err(CanonicalRecordReplayError::Plan(RecordIndexError::InvariantViolation(
                "replayed multiplicities differ from the preflighted cardinality",
            )));
        }
        if emitted_entries_len != self.encoded_entries_len {
            return Err(CanonicalRecordReplayError::Plan(RecordIndexError::InvariantViolation(
                "replayed record bytes differ from the preflighted encoded length",
            )));
        }
        Ok(())
    }

    fn replay_record<E>(
        &self,
        canonical_bytes: &[u8],
        record: &Arc<StoredRecord<R>>,
        emitted_count: &mut u128,
        emitted_entries_len: &mut u128,
        sink: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), CanonicalRecordReplayError<E>> {
        let (multiplicity, framed_len, record_len) = Self::validated_record_metrics(
            self.prefix.as_ref(),
            canonical_bytes,
            record,
            self.next_order,
        )
        .map_err(CanonicalRecordReplayError::Plan)?;
        *emitted_count =
            emitted_count
                .checked_add(multiplicity)
                .ok_or(CanonicalRecordReplayError::Plan(
                    RecordIndexError::EncodedCardinalityOverflow(u128::MAX),
                ))?;
        *emitted_entries_len =
            emitted_entries_len
                .checked_add(framed_len)
                .ok_or(CanonicalRecordReplayError::Plan(
                    RecordIndexError::EncodedLengthOverflow(u128::MAX),
                ))?;
        let record_len = record_len.to_le_bytes();
        for _ in record.orders.iter() {
            sink(&record_len).map_err(CanonicalRecordReplayError::Sink)?;
            sink(canonical_bytes).map_err(CanonicalRecordReplayError::Sink)?;
        }
        Ok(())
    }

    fn validated_record_metrics(
        prefix: &[u8],
        canonical_bytes: &[u8],
        record: &Arc<StoredRecord<R>>,
        next_order: u64,
    ) -> Result<(u128, u128, u64), RecordIndexError> {
        if record.prefix_len != prefix.len() || !record.path.starts_with(prefix) {
            return Err(RecordIndexError::InvariantViolation(
                "record endpoint is not owned by its group prefix",
            ));
        }
        let expected_path_len = prefix
            .len()
            .checked_add(canonical_bytes.len())
            .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
        if record.path.len() != expected_path_len || &record.path[prefix.len()..] != canonical_bytes
        {
            return Err(RecordIndexError::InvariantViolation(
                "record endpoint path differs from its PathMap cursor",
            ));
        }
        if canonical_bytes.len() != record.canonical_len {
            return Err(RecordIndexError::InvariantViolation(
                "record endpoint length differs from its metadata",
            ));
        }
        if record.orders.is_empty() {
            return Err(RecordIndexError::InvariantViolation(
                "record endpoint has zero multiplicity",
            ));
        }
        let mut previous_order = next_order;
        for order in record.orders.iter().copied() {
            if order == 0 || order >= previous_order {
                return Err(RecordIndexError::InvariantViolation(
                    "record endpoint orders are outside their strict live-order range",
                ));
            }
            previous_order = order;
        }
        let record_len = u64::try_from(canonical_bytes.len())
            .map_err(|_| RecordIndexError::EncodedLengthOverflow(canonical_bytes.len() as u128))?;
        let multiplicity = record.orders.len() as u128;
        let framed_len = ((u64::BITS / 8) as u128)
            .checked_add(record_len as u128)
            .and_then(|len| len.checked_mul(multiplicity))
            .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
        Ok((multiplicity, framed_len, record_len))
    }

    /// Test-only convenience for inspecting one record plan. Production
    /// compatibility materialization is owned by `CanonicalRecordGroupPlan`.
    #[cfg(any(test, feature = "record-order-index-experiment"))]
    fn encode_vec(&self) -> Result<Vec<u8>, RecordIndexError> {
        let mut output = Vec::new();
        output
            .try_reserve_exact(self.encoded_len())
            .map_err(|_| RecordIndexError::AllocationFailure(self.encoded_len()))?;
        let replay = self.try_replay(|bytes| {
            output.extend_from_slice(bytes);
            Ok::<(), std::convert::Infallible>(())
        });
        match replay {
            Ok(()) => {}
            Err(CanonicalRecordReplayError::Plan(error)) => return Err(error),
            Err(CanonicalRecordReplayError::Sink(error)) => match error {},
        }
        if output.len() != self.encoded_len() {
            return Err(RecordIndexError::InvariantViolation(
                "replayed bytes differ from the preflighted encoded length",
            ));
        }
        Ok(output)
    }
}

/// Collection metadata paired with a reusable canonical cursor plan.
#[derive(Clone)]
pub(crate) struct CanonicalRecordGroupPlan<R: Clone + Send + Sync> {
    kind: CanonicalCollectionKind,
    history_key_hash: Blake2b256Hash,
    records: CanonicalRecordPlan<R>,
}

impl<R: Clone + Send + Sync> CanonicalRecordGroupPlan<R> {
    pub(crate) fn kind(&self) -> CanonicalCollectionKind { self.kind }

    pub(crate) fn history_key_hash(&self) -> &Blake2b256Hash { &self.history_key_hash }

    pub(crate) fn record_count(&self) -> u64 { self.records.record_count() }

    pub(crate) fn encoded_len(&self) -> usize { self.records.encoded_len() }

    pub(crate) fn try_replay<E>(
        &self,
        sink: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), CanonicalRecordReplayError<E>> {
        self.records.try_replay(sink)
    }

    /// Compatibility materialization for the existing cold-history boundary.
    /// The exact reserve happens before replay, so no partially encoded value
    /// can escape and successful replay performs no capacity growth. Future
    /// WAL, hashing, and page sinks use `try_replay` directly.
    pub(crate) fn into_record_group(self) -> Result<CanonicalRecordGroup, RecordIndexError> {
        let mut encoded_persisted_data = Vec::new();
        encoded_persisted_data
            .try_reserve_exact(self.encoded_len())
            .map_err(|_| RecordIndexError::AllocationFailure(self.encoded_len()))?;
        let replay = self.try_replay(|bytes| {
            encoded_persisted_data.extend_from_slice(bytes);
            Ok::<(), std::convert::Infallible>(())
        });
        match replay {
            Ok(()) => {}
            Err(CanonicalRecordReplayError::Plan(error)) => return Err(error),
            Err(CanonicalRecordReplayError::Sink(error)) => match error {},
        }
        if encoded_persisted_data.len() != self.encoded_len() {
            return Err(RecordIndexError::InvariantViolation(
                "replayed group bytes differ from the preflighted encoded length",
            ));
        }
        Ok(CanonicalRecordGroup {
            kind: self.kind,
            history_key_hash: self.history_key_hash,
            encoded_persisted_data,
            record_count: self.records.record_count(),
        })
    }
}

#[derive(Clone)]
struct GroupState<G> {
    key: Arc<[G]>,
    len: usize,
    next_order: u64,
    /// Exact frozen `Vec<Vec<u8>>` entry bytes, excluding its outer count.
    encoded_entries_len: u128,
}

#[derive(Clone)]
pub(crate) struct StoredRecord<R> {
    path: Arc<[u8]>,
    identity_path: Option<Arc<[u8]>>,
    prefix_len: usize,
    value: R,
    canonical_len: usize,
    orders: Arc<[u64]>,
    source_ref: Option<ObjectRefV1>,
}

struct PendingRecord<R> {
    path: Arc<[u8]>,
    identity_path: Option<Arc<[u8]>>,
    value: R,
    canonical_len: usize,
    orders: Vec<u64>,
    source_ref: Option<ObjectRefV1>,
}

impl<R> StoredRecord<R> {
    pub(crate) fn path(&self) -> &Arc<[u8]> { &self.path }

    pub(crate) fn value(&self) -> &R { &self.value }

    /// Return the complete canonical record bytes embedded in this endpoint.
    /// The prefix and suffix share the authoritative PathMap allocation; the
    /// caller can keep the returned record alive while handing this borrowed
    /// slice to a durable staging sink, avoiding decode/re-encode work.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only unit tests call this until task 7931 wires persistence into RSpace"
        )
    )]
    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        &self.path[self.prefix_len..self.prefix_len + self.canonical_len]
    }
}

#[derive(Clone)]
struct OrderedGroupStore<
    G: Clone + Send + Sync,
    R: Clone + Send + Sync,
    const SEPARATE_IDENTITY: bool,
> {
    groups: PathMap<Arc<GroupState<G>>>,
    /// Complete canonical records, ordered exactly as the frozen cold store.
    records: PathMap<Arc<StoredRecord<R>>>,
    /// Continuation-only source-free identity lookup. Values are shared with
    /// `records`; joins use their complete record as identity and leave this
    /// trie empty.
    identities: PathMap<Arc<StoredRecord<R>>>,
    group_count: usize,
    record_count: usize,
}

impl<G: Clone + Send + Sync, R: Clone + Send + Sync, const SEPARATE_IDENTITY: bool> Default
    for OrderedGroupStore<G, R, SEPARATE_IDENTITY>
{
    fn default() -> Self {
        Self {
            groups: PathMap::new(),
            records: PathMap::new(),
            identities: PathMap::new(),
            group_count: 0,
            record_count: 0,
        }
    }
}

impl<G: Clone + Send + Sync, R: Clone + Send + Sync, const SEPARATE_IDENTITY: bool>
    OrderedGroupStore<G, R, SEPARATE_IDENTITY>
{
    fn is_loaded(&self, prefix: &[u8]) -> bool { self.groups.contains(prefix) }

    fn group_count(&self) -> usize { self.group_count }

    fn record_count(&self) -> usize { self.record_count }

    fn clear(&mut self) {
        *self = Self::default();
        #[cfg(test)]
        self.assert_coherent();
    }

    fn record_path(prefix: &[u8], canonical: &[u8]) -> Arc<[u8]> {
        let mut path = Vec::with_capacity(prefix.len().saturating_add(canonical.len()));
        path.extend_from_slice(prefix);
        path.extend_from_slice(canonical);
        path.into()
    }

    fn identity_path(prefix: &[u8], identity: &[u8]) -> Arc<[u8]> {
        RhoPathV1::collection_record_path(prefix, identity).into()
    }

    fn publish_record(&mut self, record: Arc<StoredRecord<R>>) {
        self.records
            .insert(record.path.as_ref(), Arc::clone(&record));
        if let Some(identity_path) = &record.identity_path {
            self.identities
                .insert(identity_path.as_ref(), Arc::clone(&record));
        }
    }

    fn remove_group_records(&mut self, prefix: &[u8], len: usize) {
        if len == 0 {
            return;
        }
        self.records.remove_branches_at(prefix, true);
        self.records.remove_val_at(prefix, true);
        if SEPARATE_IDENTITY {
            self.identities.remove_branches_at(prefix, true);
            self.identities.remove_val_at(prefix, true);
        }
        self.record_count -= len;
    }

    /// Replace one loaded group atomically through PathMap's CoW roots.
    ///
    /// `records` is in the historical visible order. The first representative
    /// of a repeated identity wins, matching hot-store deduplication.
    fn set_group(
        &mut self,
        prefix: &[u8],
        key: Arc<[G]>,
        records: Vec<(Vec<u8>, Vec<u8>, R)>,
        preserve_duplicates: bool,
    ) -> Result<(), RecordIndexError> {
        self.set_group_with_refs(
            prefix,
            key,
            records
                .into_iter()
                .map(|(identity, canonical, value)| (identity, canonical, value, None))
                .collect(),
            preserve_duplicates,
        )
    }

    fn set_group_with_refs(
        &mut self,
        prefix: &[u8],
        key: Arc<[G]>,
        records: Vec<(Vec<u8>, Vec<u8>, R, Option<ObjectRefV1>)>,
        preserve_duplicates: bool,
    ) -> Result<(), RecordIndexError> {
        let mut replacement = self.clone();
        if let Some(previous) = replacement.groups.remove(prefix) {
            replacement.remove_group_records(prefix, previous.len);
            replacement.group_count -= 1;
        }

        let total = records.len();
        let next_order = u64::try_from(total)
            .map_err(|_| RecordIndexError::EncodedCardinalityOverflow(total as u128))?
            .checked_add(1)
            .ok_or(RecordIndexError::EncodedCardinalityOverflow(total as u128))?;
        let mut stored = 0usize;
        let mut encoded_entries_len = 0u128;
        let mut pending = Vec::<PendingRecord<R>>::new();
        pending
            .try_reserve_exact(total)
            .map_err(|_| RecordIndexError::AllocationFailure(total))?;
        let mut pending_records = PathMap::<usize>::new();
        let mut pending_identities = PathMap::<usize>::new();
        for (index, (identity, canonical_bytes, value, source_ref)) in
            records.into_iter().enumerate()
        {
            let framed_len = (u64::BITS / 8) as u128 + canonical_bytes.len() as u128;
            let order = u64::try_from(total - index).map_err(|_| {
                RecordIndexError::EncodedCardinalityOverflow((total - index) as u128)
            })?;

            let identity_path = SEPARATE_IDENTITY.then(|| Self::identity_path(prefix, &identity));
            if let Some(identity_path) = &identity_path {
                if pending_identities.contains(identity_path.as_ref()) {
                    continue;
                }
            } else {
                debug_assert_eq!(identity, canonical_bytes);
            }

            let path = Self::record_path(prefix, &canonical_bytes);
            if let Some(existing_index) = pending_records.get(path.as_ref()).copied() {
                if SEPARATE_IDENTITY {
                    return Err(RecordIndexError::InvariantViolation(
                        "distinct continuation identities resolve to one canonical record path",
                    ));
                }
                if preserve_duplicates {
                    pending[existing_index].orders.push(order);
                    stored = stored
                        .checked_add(1)
                        .ok_or(RecordIndexError::EncodedCardinalityOverflow(u128::MAX))?;
                    encoded_entries_len = encoded_entries_len
                        .checked_add(framed_len)
                        .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
                }
                continue;
            }

            let pending_index = pending.len();
            pending_records.insert(path.as_ref(), pending_index);
            if let Some(identity_path) = &identity_path {
                pending_identities.insert(identity_path.as_ref(), pending_index);
            }
            pending.push(PendingRecord {
                path,
                identity_path,
                value,
                canonical_len: canonical_bytes.len(),
                orders: vec![order],
                source_ref,
            });
            stored = stored
                .checked_add(1)
                .ok_or(RecordIndexError::EncodedCardinalityOverflow(u128::MAX))?;
            encoded_entries_len = encoded_entries_len
                .checked_add(framed_len)
                .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
        }
        for record in pending {
            replacement.publish_record(Arc::new(StoredRecord {
                path: record.path,
                identity_path: record.identity_path,
                prefix_len: prefix.len(),
                value: record.value,
                canonical_len: record.canonical_len,
                orders: record.orders.into(),
                source_ref: record.source_ref,
            }));
        }
        replacement.groups.insert(
            prefix,
            Arc::new(GroupState {
                key,
                len: stored,
                next_order,
                encoded_entries_len,
            }),
        );
        replacement.group_count += 1;
        replacement.record_count += stored;
        *self = replacement;
        #[cfg(test)]
        self.assert_coherent();
        Ok(())
    }

    fn try_visit_group_records<E>(
        &self,
        prefix: &[u8],
        mut visitor: impl FnMut(&[u8], &Arc<StoredRecord<R>>) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut cursor = self.records.read_zipper_at_borrowed_path(prefix);
        if !cursor.path_exists() {
            return Ok(());
        }
        if let Some(record) = cursor.val() {
            visitor(cursor.path(), record)?;
        }
        while let Some(record) = cursor.to_next_get_val() {
            visitor(cursor.path(), record)?;
        }
        Ok(())
    }

    fn visit_group_records(
        &self,
        prefix: &[u8],
        mut visitor: impl FnMut(&[u8], &Arc<StoredRecord<R>>),
    ) {
        let result = self.try_visit_group_records(prefix, |path, record| {
            visitor(path, record);
            Ok::<(), std::convert::Infallible>(())
        });
        match result {
            Ok(()) => {}
            Err(error) => match error {},
        }
    }

    fn try_visit_groups<E>(
        &self,
        mut visitor: impl FnMut(&[u8], &Arc<GroupState<G>>) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut cursor = self.groups.read_zipper();
        if let Some(group) = cursor.val() {
            visitor(cursor.path(), group)?;
        }
        while let Some(group) = cursor.to_next_get_val() {
            visitor(cursor.path(), group)?;
        }
        Ok(())
    }

    fn visit_groups(&self, mut visitor: impl FnMut(&[u8], &Arc<GroupState<G>>)) {
        let result = self.try_visit_groups(|path, group| {
            visitor(path, group);
            Ok::<(), std::convert::Infallible>(())
        });
        match result {
            Ok(()) => {}
            Err(error) => match error {},
        }
    }

    fn ordered_records(&self, prefix: &[u8]) -> Vec<Arc<StoredRecord<R>>> {
        let Some(group) = self.groups.get(prefix) else {
            return Vec::new();
        };
        let mut records = Vec::with_capacity(group.len);
        self.visit_group_records(prefix, |_, record| {
            records.extend(
                record
                    .orders
                    .iter()
                    .map(|order| (*order, Arc::clone(record))),
            );
        });
        records.sort_unstable_by(|(left_order, left), (right_order, right)| {
            right_order
                .cmp(left_order)
                .then_with(|| left.path.cmp(&right.path))
        });
        records.into_iter().map(|(_, record)| record).collect()
    }

    fn group_values(&self, prefix: &[u8]) -> Option<Vec<R>> {
        self.groups.get(prefix)?;
        Some(
            self.ordered_records(prefix)
                .into_iter()
                .map(|record| record.value.clone())
                .collect(),
        )
    }

    fn renumber(&mut self, prefix: &[u8]) {
        let ordered = self.ordered_records(prefix);
        let len = ordered.len();
        let mut grouped: BTreeMap<Arc<[u8]>, (Arc<StoredRecord<R>>, Vec<u64>)> = BTreeMap::new();
        for (index, record) in ordered.into_iter().enumerate() {
            let order = u64::try_from(len - index)
                .expect("an in-memory record position must fit the u64 ordering domain");
            grouped
                .entry(Arc::clone(&record.path))
                .or_insert_with(|| (Arc::clone(&record), Vec::new()))
                .1
                .push(order);
        }
        for (path, (record, orders)) in grouped {
            let mut replacement = (*record).clone();
            replacement.orders = orders.into();
            debug_assert_eq!(replacement.path.as_ref(), path.as_ref());
            self.publish_record(Arc::new(replacement));
        }
        if let Some(group) = self.groups.get(prefix).cloned() {
            let mut replacement = (*group).clone();
            replacement.next_order = u64::try_from(len)
                .expect("an in-memory record count must fit the u64 ordering domain")
                .saturating_add(1);
            self.groups.insert(prefix, Arc::new(replacement));
        }
    }

    fn insert(
        &mut self,
        prefix: &[u8],
        key: Arc<[G]>,
        identity: &[u8],
        canonical_bytes: Vec<u8>,
        value: R,
        source_ref: Option<ObjectRefV1>,
    ) -> Result<bool, RecordIndexError> {
        let mut replacement = self.clone();
        if !replacement.insert_in_place(
            prefix,
            key,
            identity,
            canonical_bytes,
            value,
            source_ref,
        )? {
            return Ok(false);
        }
        *self = replacement;
        #[cfg(test)]
        self.assert_coherent();
        Ok(true)
    }

    fn insert_in_place(
        &mut self,
        prefix: &[u8],
        key: Arc<[G]>,
        identity: &[u8],
        canonical_bytes: Vec<u8>,
        value: R,
        source_ref: Option<ObjectRefV1>,
    ) -> Result<bool, RecordIndexError> {
        if !self.is_loaded(prefix) {
            self.groups.insert(
                prefix,
                Arc::new(GroupState {
                    key,
                    len: 0,
                    next_order: 1,
                    encoded_entries_len: 0,
                }),
            );
            self.group_count += 1;
        }

        let identity_path = SEPARATE_IDENTITY.then(|| Self::identity_path(prefix, identity));
        if let Some(identity_path) = &identity_path {
            if self.identities.contains(identity_path.as_ref()) {
                return Ok(false);
            }
        } else {
            debug_assert_eq!(identity, canonical_bytes);
        }
        let path = Self::record_path(prefix, &canonical_bytes);
        if self.records.contains(path.as_ref()) {
            if SEPARATE_IDENTITY {
                return Err(RecordIndexError::InvariantViolation(
                    "distinct continuation identities resolve to one canonical record path",
                ));
            }
            return Ok(false);
        }
        if self
            .groups
            .get(prefix)
            .is_some_and(|group| group.next_order == u64::MAX)
        {
            self.renumber(prefix);
        }
        let group = self
            .groups
            .get(prefix)
            .cloned()
            .expect("an inserted record must have a loaded group marker");
        let framed_len = (u64::BITS / 8) as u128 + canonical_bytes.len() as u128;
        let record = StoredRecord {
            path: Arc::clone(&path),
            identity_path,
            prefix_len: prefix.len(),
            value,
            canonical_len: canonical_bytes.len(),
            orders: Arc::from([group.next_order]),
            source_ref,
        };
        self.publish_record(Arc::new(record));

        let mut next_group = (*group).clone();
        next_group.len += 1;
        next_group.next_order += 1;
        next_group.encoded_entries_len = next_group
            .encoded_entries_len
            .checked_add(framed_len)
            .expect("an in-memory record group cannot exceed the u128 byte domain");
        self.groups.insert(prefix, Arc::new(next_group));
        self.record_count += 1;
        Ok(true)
    }

    fn get_path(&self, prefix: &[u8], path: &[u8]) -> Option<Arc<StoredRecord<R>>> {
        let record = self.records.get(path)?;
        (record.prefix_len == prefix.len() && path.starts_with(prefix)).then(|| Arc::clone(record))
    }

    fn contains_path(&self, path: &[u8]) -> bool { self.records.contains(path) }

    fn get_identity_path(&self, prefix: &[u8], path: &[u8]) -> Option<Arc<StoredRecord<R>>> {
        let record = self.identities.get(path)?;
        (record.prefix_len == prefix.len() && path.starts_with(prefix)).then(|| Arc::clone(record))
    }

    fn remove_path(&mut self, prefix: &[u8], path: &[u8]) -> Option<R> {
        let mut replacement = self.clone();
        let removed = replacement.remove_path_in_place(prefix, path)?;
        *self = replacement;
        #[cfg(test)]
        self.assert_coherent();
        Some(removed)
    }

    fn remove_path_in_place(&mut self, prefix: &[u8], path: &[u8]) -> Option<R> {
        let current = self.records.get(path)?;
        if current.prefix_len != prefix.len() || !path.starts_with(prefix) {
            return None;
        }
        let removed = self.records.remove(path)?;
        if let Some(identity_path) = &removed.identity_path {
            self.identities.remove(identity_path.as_ref());
        }
        let group = self
            .groups
            .get(prefix)
            .cloned()
            .expect("a stored record must have a loaded group marker");
        let mut next_group = (*group).clone();
        next_group.len -= removed.orders.len();
        let removed_len = ((u64::BITS / 8) as u128 + removed.canonical_len as u128)
            .checked_mul(removed.orders.len() as u128)
            .expect("an in-memory record group cannot exceed the u128 byte domain");
        next_group.encoded_entries_len = next_group
            .encoded_entries_len
            .checked_sub(removed_len)
            .expect("stored record byte accounting must not underflow");
        self.groups.insert(prefix, Arc::new(next_group));
        self.record_count -= removed.orders.len();
        Some(removed.value.clone())
    }

    fn remove_identity(&mut self, prefix: &[u8], identity: &[u8]) -> Option<R> {
        let mut replacement = self.clone();
        let record = if SEPARATE_IDENTITY {
            let identity_path = Self::identity_path(prefix, identity);
            replacement
                .identities
                .get(identity_path.as_ref())
                .cloned()?
        } else {
            let path = Self::record_path(prefix, identity);
            replacement.records.get(path.as_ref()).cloned()?
        };
        if record.orders.len() == 1 {
            let removed = replacement.remove_path_in_place(prefix, record.path.as_ref())?;
            *self = replacement;
            #[cfg(test)]
            self.assert_coherent();
            return Some(removed);
        }
        let mut next = (*record).clone();
        let remove_index = next
            .orders
            .iter()
            .enumerate()
            .max_by_key(|(_, order)| *order)
            .map(|(index, _)| index)
            .expect("a duplicated record has at least two orders");
        let mut orders = next.orders.to_vec();
        orders.remove(remove_index);
        next.orders = orders.into();
        replacement.publish_record(Arc::new(next));
        let group = replacement
            .groups
            .get(prefix)
            .cloned()
            .expect("a stored record must have a loaded group marker");
        let mut next_group = (*group).clone();
        next_group.len -= 1;
        next_group.encoded_entries_len = next_group
            .encoded_entries_len
            .checked_sub((u64::BITS / 8) as u128 + record.canonical_len as u128)
            .expect("stored record byte accounting must not underflow");
        replacement.groups.insert(prefix, Arc::new(next_group));
        replacement.record_count -= 1;
        let value = record.value.clone();
        *self = replacement;
        #[cfg(test)]
        self.assert_coherent();
        Some(value)
    }

    fn loaded_records(&self) -> Vec<(Arc<[G]>, Vec<R>)> {
        let mut records = Vec::with_capacity(self.group_count);
        self.visit_loaded_records(|key, values| records.push((key, values)));
        records
    }

    fn visit_loaded_records(&self, mut visitor: impl FnMut(Arc<[G]>, Vec<R>)) {
        self.visit_groups(|prefix, group| {
            visitor(
                Arc::clone(&group.key),
                self.group_values(prefix)
                    .expect("an iterated group marker is loaded"),
            );
        });
    }

    fn has_nonempty_loaded_group(&self) -> bool {
        let mut found = false;
        self.visit_groups(|_, group| found |= group.len != 0);
        found
    }

    /// Preflight one canonical envelope against an immutable CoW record root.
    /// The key suffix is the complete canonical record, so PathMap order equals
    /// frozen record-byte order definitionally; ordinal metadata is used only
    /// to expand equal join occurrences.
    fn canonical_record_plan(
        &self,
        prefix: &[u8],
        kind: CanonicalCollectionKind,
    ) -> Result<CanonicalRecordPlan<R>, RecordIndexError> {
        let group = self
            .groups
            .get(prefix)
            .ok_or(RecordIndexError::InvariantViolation("loaded group marker is absent"))?;
        let records = self.records.clone();
        let record_count = u64::try_from(group.len)
            .map_err(|_| RecordIndexError::EncodedCardinalityOverflow(group.len as u128))?;
        let mut actual_count = 0u128;
        let mut actual_entries_len = 0u128;
        let mut cursor = records.read_zipper_at_borrowed_path(prefix);
        if cursor.path_exists() {
            if let Some(record) = cursor.val() {
                let (multiplicity, framed_len, _) =
                    CanonicalRecordPlan::<R>::validated_record_metrics(
                        prefix,
                        cursor.path(),
                        record,
                        group.next_order,
                    )?;
                actual_count = actual_count
                    .checked_add(multiplicity)
                    .ok_or(RecordIndexError::EncodedCardinalityOverflow(u128::MAX))?;
                actual_entries_len = actual_entries_len
                    .checked_add(framed_len)
                    .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
            }
            while let Some(record) = cursor.to_next_get_val() {
                let (multiplicity, framed_len, _) =
                    CanonicalRecordPlan::<R>::validated_record_metrics(
                        prefix,
                        cursor.path(),
                        record,
                        group.next_order,
                    )?;
                actual_count = actual_count
                    .checked_add(multiplicity)
                    .ok_or(RecordIndexError::EncodedCardinalityOverflow(u128::MAX))?;
                actual_entries_len = actual_entries_len
                    .checked_add(framed_len)
                    .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
            }
        }
        drop(cursor);
        if actual_count != group.len as u128 {
            return Err(RecordIndexError::InvariantViolation(
                "endpoint multiplicities differ from the group cardinality",
            ));
        }
        if actual_entries_len != group.encoded_entries_len {
            return Err(RecordIndexError::InvariantViolation(
                "endpoint bytes differ from the transactionally maintained length",
            ));
        }

        let payload_len = ((u64::BITS / 8) as u128)
            .checked_add(actual_entries_len)
            .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
        let payload_len_u64 = u64::try_from(payload_len)
            .map_err(|_| RecordIndexError::EncodedLengthOverflow(payload_len))?;
        let encoded_len_u128 = (PERSISTED_DATA_HEADER_LEN as u128)
            .checked_add(payload_len)
            .ok_or(RecordIndexError::EncodedLengthOverflow(u128::MAX))?;
        let encoded_len = usize::try_from(encoded_len_u128)
            .map_err(|_| RecordIndexError::EncodedLengthOverflow(encoded_len_u128))?;

        Ok(CanonicalRecordPlan {
            records,
            prefix: Arc::from(prefix),
            header: kind.persisted_kind().header(payload_len_u64),
            next_order: group.next_order,
            record_count,
            encoded_entries_len: actual_entries_len,
            encoded_len,
        })
    }

    /// Materialize the compatibility `Vec<u8>` only after immutable preflight.
    #[cfg(any(test, feature = "record-order-index-experiment"))]
    fn encode_canonical_group(
        &self,
        prefix: &[u8],
        kind: CanonicalCollectionKind,
    ) -> Result<(Vec<u8>, u64), RecordIndexError> {
        let plan = self.canonical_record_plan(prefix, kind)?;
        let record_count = plan.record_count();
        let output = plan.encode_vec()?;
        Ok((output, record_count))
    }

    /// Assert the complete multi-root authority relation after every test
    /// mutation. This intentionally checks semantic accounting and shared
    /// endpoint ownership, not merely that each PathMap can be traversed.
    #[cfg(test)]
    fn assert_coherent(&self) {
        use std::collections::BTreeSet;

        let group_entries = self.groups.iter().collect::<Vec<_>>();
        assert_eq!(self.group_count, group_entries.len());

        let mut logical_records = 0usize;
        let mut primary_endpoints = 0usize;
        for (prefix, group) in group_entries {
            let mut group_records = 0usize;
            let mut encoded_entries_len = 0u128;
            let mut orders = BTreeSet::new();
            self.visit_group_records(&prefix, |suffix, record| {
                primary_endpoints += 1;
                assert_eq!(record.prefix_len, prefix.len());
                assert_eq!(record.canonical_len, suffix.len());
                assert_eq!(record.path.len(), prefix.len() + suffix.len());
                assert!(record.path.starts_with(&prefix));
                assert_eq!(&record.path[prefix.len()..], suffix);
                assert!(!record.orders.is_empty());
                for order in record.orders.iter().copied() {
                    assert!(order > 0 && order < group.next_order);
                    assert!(orders.insert(order), "record order must be unique within a group");
                }

                group_records += record.orders.len();
                encoded_entries_len +=
                    ((u64::BITS / 8) as u128 + suffix.len() as u128) * record.orders.len() as u128;
                if SEPARATE_IDENTITY {
                    let identity_path = record
                        .identity_path
                        .as_ref()
                        .expect("continuation primary endpoint must name its identity endpoint");
                    assert!(identity_path.starts_with(&prefix));
                    let identity_record = self
                        .identities
                        .get(identity_path.as_ref())
                        .expect("continuation identity endpoint must be present");
                    assert!(Arc::ptr_eq(identity_record, record));
                } else {
                    assert!(record.identity_path.is_none());
                }
            });
            assert_eq!(group.len, group_records);
            assert_eq!(group.encoded_entries_len, encoded_entries_len);
            logical_records += group_records;
        }

        let all_primary = self.records.iter().collect::<Vec<_>>();
        assert_eq!(primary_endpoints, all_primary.len());
        for (path, record) in all_primary {
            assert_eq!(path.as_slice(), record.path.as_ref());
            assert!(record.prefix_len <= path.len());
            assert!(self.groups.contains(&path[..record.prefix_len]));
        }

        let identity_entries = self.identities.iter().collect::<Vec<_>>();
        if SEPARATE_IDENTITY {
            assert_eq!(identity_entries.len(), primary_endpoints);
            for (identity_path, identity_record) in identity_entries {
                assert_eq!(
                    identity_record.identity_path.as_deref(),
                    Some(identity_path.as_slice())
                );
                let primary = self
                    .records
                    .get(identity_record.path.as_ref())
                    .expect("identity endpoint must name a primary endpoint");
                assert!(Arc::ptr_eq(primary, identity_record));
            }
        } else {
            assert!(identity_entries.is_empty());
        }
        assert_eq!(self.record_count, logical_records);
    }
}

/// Authoritative resting and installed continuation tries.
#[derive(Clone)]
pub(crate) struct RSpaceContinuationStore<
    C: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
> {
    resting: OrderedGroupStore<C, WaitingContinuation<P, K>, true>,
    installed: PathMap<Arc<(Arc<[C]>, WaitingContinuation<P, K>)>>,
    pending_delta: BagDelta,
    pending_content: PathMap<ContentTransition>,
    pending_source_bytes: PathMap<Arc<[u8]>>,
}

impl<C, P, K> Default for RSpaceContinuationStore<C, P, K>
where
    C: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    fn default() -> Self {
        Self {
            resting: OrderedGroupStore::default(),
            installed: PathMap::new(),
            pending_delta: BagDelta::new(),
            pending_content: PathMap::new(),
            pending_source_bytes: PathMap::new(),
        }
    }
}

impl<C, P, K> RSpaceContinuationStore<C, P, K>
where
    C: Clone + RhoPathEncode + Send + Sync,
    P: Clone + StableHashSerialize + Send + Sync,
    K: Clone + StableHashSerialize + Send + Sync,
{
    pub(crate) fn new() -> Self { Self::default() }

    pub(crate) fn is_loaded(&self, channels: &[C]) -> bool {
        self.resting
            .is_loaded(&RhoPathV1::continuation_prefix(channels))
    }

    pub(crate) fn set_group(
        &mut self,
        channels: Vec<C>,
        continuations: Vec<WaitingContinuation<P, K>>,
    ) {
        let prefix = RhoPathV1::continuation_prefix(&channels);
        let records = continuations
            .into_iter()
            .map(|continuation| {
                let identity = encode_continuation_identity(&continuation);
                let mut canonical = identity.clone();
                encode_continuation_source_into(&continuation, &mut canonical);
                let (source_ref, _) = continuation_source(&identity, &canonical);
                (identity, canonical, continuation, Some(source_ref))
            })
            .collect();
        self.resting
            .set_group_with_refs(&prefix, Arc::from(channels), records, false)
            .expect("typed continuation records must have an injective frozen encoding");
    }

    pub(crate) fn put_prepared(
        &mut self,
        channels: &[C],
        continuation: WaitingContinuation<P, K>,
        identity: Vec<u8>,
        canonical: Vec<u8>,
    ) -> bool {
        let prefix = RhoPathV1::continuation_prefix(channels);
        // The live primary trie and its multiplicity delta are keyed by the
        // complete canonical record bytes. The semantic ContinuationMap
        // content journal uses the separate framed, source-free identity path.
        let mut primary_record_path = prefix.clone();
        primary_record_path.extend_from_slice(&canonical);
        let identity_path = RhoPathV1::collection_record_path(&prefix, &identity);
        let (source_ref, source_bytes) = continuation_source(&identity, &canonical);
        let inserted = self
            .resting
            .insert(
                &prefix,
                Arc::from(channels),
                &identity,
                canonical,
                continuation,
                Some(source_ref),
            )
            .expect("typed continuation records must have an injective frozen encoding");
        if inserted {
            self.pending_delta
                .record_addition(
                    &primary_record_path,
                    Multiplicity::new(1).expect("one is nonzero"),
                )
                .expect("a successful continuation insertion is representable");
            record_content_transition(
                &mut self.pending_content,
                &identity_path,
                None,
                Some(source_ref),
            );
            self.pending_source_bytes
                .insert(&identity_path, source_bytes);
        }
        inserted
    }

    pub(crate) fn resting_records(
        &self,
        channels: &[C],
    ) -> Option<Vec<Arc<StoredRecord<WaitingContinuation<P, K>>>>> {
        let prefix = RhoPathV1::continuation_prefix(channels);
        self.resting
            .is_loaded(&prefix)
            .then(|| self.resting.ordered_records(&prefix))
    }

    pub(crate) fn resting_values(&self, channels: &[C]) -> Option<Vec<WaitingContinuation<P, K>>> {
        self.resting
            .group_values(&RhoPathV1::continuation_prefix(channels))
    }

    pub(crate) fn resting_len(&self, channels: &[C]) -> Option<usize> {
        let prefix = RhoPathV1::continuation_prefix(channels);
        self.resting.groups.get(&prefix).map(|group| group.len)
    }

    pub(crate) fn get_path(
        &self,
        channels: &[C],
        path: &[u8],
    ) -> Option<Arc<StoredRecord<WaitingContinuation<P, K>>>> {
        self.resting
            .get_path(&RhoPathV1::continuation_prefix(channels), path)
    }

    /// Look up an authoritative resting continuation endpoint by its complete
    /// PathMap path. The returned Arc keeps the canonical suffix allocation
    /// alive while a persistence adapter stages the bytes; no decode or
    /// re-encoding is required.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only unit tests call this until task 7931 wires persistence into RSpace"
        )
    )]
    pub(crate) fn source_record(
        &self,
        channels: &[C],
        path: &[u8],
    ) -> Option<Arc<StoredRecord<WaitingContinuation<P, K>>>> {
        let prefix = RhoPathV1::continuation_prefix(channels);
        self.resting.get_identity_path(&prefix, path)
    }

    pub(crate) fn remove_path(
        &mut self,
        channels: &[C],
        path: &[u8],
    ) -> Option<WaitingContinuation<P, K>> {
        let record = self
            .resting
            .get_path(&RhoPathV1::continuation_prefix(channels), path);
        let identity_path = record
            .as_ref()
            .and_then(|record| record.identity_path.clone());
        let removed = self
            .resting
            .remove_path(&RhoPathV1::continuation_prefix(channels), path);
        if removed.is_some() {
            self.pending_delta
                .record_removal(path, Multiplicity::new(1).expect("one is nonzero"))
                .expect("a successful continuation removal is representable");
            let source_ref = record
                .and_then(|record| record.source_ref)
                .expect("a continuation record carries its source reference");
            let identity_path = identity_path
                .as_deref()
                .expect("a continuation record carries its identity path");
            record_content_transition(
                &mut self.pending_content,
                identity_path,
                Some(source_ref),
                None,
            );
            self.pending_source_bytes.remove_val_at(identity_path, true);
        }
        removed
    }

    pub(crate) fn install(&mut self, channels: &[C], continuation: WaitingContinuation<P, K>) {
        self.installed.insert(
            RhoPathV1::continuation_prefix(channels),
            Arc::new((Arc::from(channels), continuation)),
        );
    }

    pub(crate) fn installed(&self, channels: &[C]) -> Option<WaitingContinuation<P, K>> {
        self.installed
            .get(RhoPathV1::continuation_prefix(channels))
            .map(|entry| entry.1.clone())
    }

    pub(crate) fn contains_installed(&self, channels: &[C]) -> bool {
        self.installed
            .contains(RhoPathV1::continuation_prefix(channels))
    }

    pub(crate) fn loaded_records(&self) -> Vec<(Arc<[C]>, Vec<WaitingContinuation<P, K>>)> {
        self.resting.loaded_records()
    }

    pub(crate) fn visit_loaded_records(
        &self,
        visitor: impl FnMut(Arc<[C]>, Vec<WaitingContinuation<P, K>>),
    ) {
        self.resting.visit_loaded_records(visitor);
    }

    pub(crate) fn has_nonempty_loaded_group(&self) -> bool {
        self.resting.has_nonempty_loaded_group()
    }

    pub(crate) fn try_visit_loaded_canonical_plans(
        &self,
        mut visitor: impl FnMut(
            CanonicalRecordGroupPlan<WaitingContinuation<P, K>>,
        ) -> Result<(), RecordIndexError>,
    ) -> Result<(), RecordIndexError>
    where
        C: StableHashSerialize,
    {
        self.resting.try_visit_groups(|prefix, group| {
            visitor(CanonicalRecordGroupPlan {
                kind: CanonicalCollectionKind::Continuations,
                history_key_hash: hash_from_slice(&group.key),
                records: self
                    .resting
                    .canonical_record_plan(prefix, CanonicalCollectionKind::Continuations)?,
            })
        })
    }

    #[cfg(test)]
    pub(crate) fn try_visit_loaded_canonical_records(
        &self,
        mut visitor: impl FnMut(CanonicalRecordGroup) -> Result<(), RecordIndexError>,
    ) -> Result<(), RecordIndexError>
    where
        C: StableHashSerialize,
    {
        self.try_visit_loaded_canonical_plans(|plan| visitor(plan.into_record_group()?))
    }

    pub(crate) fn installed_records(&self) -> Vec<(Arc<[C]>, WaitingContinuation<P, K>)> {
        let mut records = Vec::with_capacity(self.installed_count());
        let mut cursor = self.installed.read_zipper();
        if let Some(entry) = cursor.val() {
            records.push((Arc::clone(&entry.0), entry.1.clone()));
        }
        while let Some(entry) = cursor.to_next_get_val() {
            records.push((Arc::clone(&entry.0), entry.1.clone()));
        }
        records
    }

    pub(crate) fn loaded_group_count(&self) -> usize { self.resting.group_count() }

    pub(crate) fn resting_record_count(&self) -> usize { self.resting.record_count() }

    pub(crate) fn installed_count(&self) -> usize { self.installed.val_count() }

    pub(crate) fn pending_delta(&self) -> &BagDelta { &self.pending_delta }

    pub(crate) fn pending_content(&self) -> &PathMap<ContentTransition> { &self.pending_content }

    pub(crate) fn pending_source_bytes(&self) -> &PathMap<Arc<[u8]>> { &self.pending_source_bytes }

    pub(crate) fn restore_pending_content(&mut self, content: PathMap<ContentTransition>) {
        self.pending_content = content;
    }

    pub(crate) fn restore_pending_source_bytes(&mut self, bytes: PathMap<Arc<[u8]>>) {
        self.pending_source_bytes = bytes;
    }

    pub(crate) fn restore_pending_delta(&mut self, delta: BagDelta) { self.pending_delta = delta; }

    pub(crate) fn clear(&mut self) {
        self.resting.clear();
        self.installed = PathMap::new();
        self.pending_delta = BagDelta::new();
        self.pending_content = PathMap::new();
        self.pending_source_bytes = PathMap::new();
    }
}

impl<C, P, K> fmt::Debug for RSpaceContinuationStore<C, P, K>
where
    C: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RSpaceContinuationStore")
            .field("loaded_groups", &self.resting.group_count)
            .field("resting_records", &self.resting.record_count)
            .finish_non_exhaustive()
    }
}

/// Authoritative resting and installed join tries.
#[derive(Clone)]
pub(crate) struct RSpaceJoinStore<C: Clone + Send + Sync> {
    resting: OrderedGroupStore<C, Vec<C>, false>,
    installed: OrderedGroupStore<C, Vec<C>, false>,
    pending_delta: BagDelta,
}

impl<C: Clone + Send + Sync> Default for RSpaceJoinStore<C> {
    fn default() -> Self {
        Self {
            resting: OrderedGroupStore::default(),
            installed: OrderedGroupStore::default(),
            pending_delta: BagDelta::new(),
        }
    }
}

impl<C> RSpaceJoinStore<C>
where C: Clone + RhoPathEncode + Send + Sync
{
    pub(crate) fn new() -> Self { Self::default() }

    pub(crate) fn is_loaded(&self, channel: &C) -> bool {
        self.resting.is_loaded(&RhoPathV1::join_prefix(channel))
    }

    /// Native endpoint lookup for the semantic JoinBag path. Join endpoints
    /// are unit occurrences in the resident index; multiplicity deltas record
    /// each successful endpoint insertion/removal separately.
    pub(crate) fn count_path(&self, path: &[u8]) -> u64 {
        u64::from(self.resting.contains_path(path))
    }

    pub(crate) fn set_channel(&mut self, channel: C, joins: Vec<Vec<C>>) {
        let prefix = RhoPathV1::join_prefix(&channel);
        let records = joins
            .into_iter()
            .map(|join| {
                let identity = encode_join_record(&join);
                (identity.clone(), identity, join)
            })
            .collect();
        self.resting
            .set_group(&prefix, Arc::from([channel]), records, true)
            .expect("typed join records must have an injective frozen encoding");
    }

    pub(crate) fn put(&mut self, channel: &C, join: &[C]) -> bool {
        let prefix = RhoPathV1::join_prefix(channel);
        let identity = encode_join_record(join);
        let inserted = self
            .resting
            .insert(
                &prefix,
                Arc::from([channel.clone()]),
                &identity,
                identity.clone(),
                join.to_vec(),
                None,
            )
            .expect("typed join records must have an injective frozen encoding");
        if inserted {
            let path = OrderedGroupStore::<C, Vec<C>, false>::record_path(&prefix, &identity);
            self.pending_delta
                .record_addition(&path, Multiplicity::new(1).expect("one is nonzero"))
                .expect("a successful join insertion is representable");
        }
        inserted
    }

    pub(crate) fn install(&mut self, channel: &C, join: &[C]) -> bool {
        let prefix = RhoPathV1::join_prefix(channel);
        let identity = encode_join_record(join);
        self.installed
            .insert(
                &prefix,
                Arc::from([channel.clone()]),
                &identity,
                identity.clone(),
                join.to_vec(),
                None,
            )
            .expect("typed join records must have an injective frozen encoding")
    }

    pub(crate) fn resting_values(&self, channel: &C) -> Option<Vec<Vec<C>>> {
        self.resting.group_values(&RhoPathV1::join_prefix(channel))
    }

    pub(crate) fn installed_values(&self, channel: &C) -> Vec<Vec<C>> {
        self.installed
            .group_values(&RhoPathV1::join_prefix(channel))
            .unwrap_or_default()
    }

    pub(crate) fn remove(&mut self, channel: &C, join: &[C]) -> Option<Vec<C>> {
        let prefix = RhoPathV1::join_prefix(channel);
        let identity = encode_join_record(join);
        let removed = self.resting.remove_identity(&prefix, &identity);
        if removed.is_some() {
            let path = OrderedGroupStore::<C, Vec<C>, false>::record_path(&prefix, &identity);
            self.pending_delta
                .record_removal(&path, Multiplicity::new(1).expect("one is nonzero"))
                .expect("a successful join removal is representable");
        }
        removed
    }

    pub(crate) fn pending_delta(&self) -> &BagDelta { &self.pending_delta }

    pub(crate) fn restore_pending_delta(&mut self, delta: BagDelta) { self.pending_delta = delta; }

    pub(crate) fn loaded_records(&self) -> Vec<(C, Vec<Vec<C>>)> {
        self.resting
            .loaded_records()
            .into_iter()
            .map(|(channel, joins)| (channel[0].clone(), joins))
            .collect()
    }

    pub(crate) fn visit_loaded_records(&self, mut visitor: impl FnMut(C, Vec<Vec<C>>)) {
        self.resting.visit_loaded_records(|channel, joins| {
            visitor(channel[0].clone(), joins);
        });
    }

    pub(crate) fn has_nonempty_loaded_group(&self) -> bool {
        self.resting.has_nonempty_loaded_group()
    }

    pub(crate) fn try_visit_loaded_canonical_plans(
        &self,
        mut visitor: impl FnMut(CanonicalRecordGroupPlan<Vec<C>>) -> Result<(), RecordIndexError>,
    ) -> Result<(), RecordIndexError>
    where
        C: StableHashSerialize,
    {
        self.resting.try_visit_groups(|prefix, group| {
            let channel = group
                .key
                .first()
                .ok_or(RecordIndexError::InvariantViolation("join group has no channel key"))?;
            visitor(CanonicalRecordGroupPlan {
                kind: CanonicalCollectionKind::Joins,
                history_key_hash: hash(channel),
                records: self
                    .resting
                    .canonical_record_plan(prefix, CanonicalCollectionKind::Joins)?,
            })
        })
    }

    #[cfg(test)]
    pub(crate) fn try_visit_loaded_canonical_records(
        &self,
        mut visitor: impl FnMut(CanonicalRecordGroup) -> Result<(), RecordIndexError>,
    ) -> Result<(), RecordIndexError>
    where
        C: StableHashSerialize,
    {
        self.try_visit_loaded_canonical_plans(|plan| visitor(plan.into_record_group()?))
    }

    pub(crate) fn installed_records(&self) -> Vec<(C, Vec<Vec<C>>)> {
        self.installed
            .loaded_records()
            .into_iter()
            .map(|(channel, joins)| (channel[0].clone(), joins))
            .collect()
    }

    pub(crate) fn loaded_channel_count(&self) -> usize { self.resting.group_count() }

    pub(crate) fn resting_record_count(&self) -> usize { self.resting.record_count() }

    pub(crate) fn installed_channel_count(&self) -> usize { self.installed.group_count() }

    pub(crate) fn installed_record_count(&self) -> usize { self.installed.record_count() }

    pub(crate) fn clear(&mut self) {
        self.resting.clear();
        self.installed.clear();
    }
}

impl<C: Clone + Send + Sync> fmt::Debug for RSpaceJoinStore<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RSpaceJoinStore")
            .field("loaded_channels", &self.resting.group_count)
            .field("resting_records", &self.resting.record_count)
            .field("installed_channels", &self.installed.group_count)
            .field("installed_records", &self.installed.record_count)
            .finish()
    }
}

#[cfg(feature = "record-order-index-experiment")]
#[doc(hidden)]
#[path = "../../tests/record_index/order_index_experiment.rs"]
pub mod order_index_experiment;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::{fmt, thread};

    use proptest::prelude::*;
    use serde::Serialize;

    use super::*;
    use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use crate::rspace::history::cold_store::{ContinuationsLeaf, JoinsLeaf, PersistedData};
    use crate::rspace::serializers::serializers::{encode_continuations, encode_joins};
    use crate::rspace::trace::event::Consume;

    fn continuation(source: u8, name: &str) -> WaitingContinuation<String, String> {
        WaitingContinuation {
            patterns: Arc::new(vec![name.to_string()]),
            continuation: Arc::new(format!("k-{name}")),
            persist: false,
            peeks: BTreeSet::new(),
            source: Consume {
                channel_hashes: Vec::new(),
                hash: Blake2b256Hash::from_bytes(vec![source; 32]),
                persistent: false,
            },
        }
    }

    #[derive(Clone, PartialEq, Eq, Serialize)]
    struct CollidingDebugContinuation(u8);

    impl fmt::Debug for CollidingDebugContinuation {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("same-debug-image")
        }
    }

    impl StableHashSerialize for CollidingDebugContinuation {}

    fn colliding_debug_continuation(
        value: u8,
    ) -> WaitingContinuation<String, CollidingDebugContinuation> {
        WaitingContinuation {
            patterns: Arc::new(vec!["pattern".to_owned()]),
            continuation: Arc::new(CollidingDebugContinuation(value)),
            persist: false,
            peeks: BTreeSet::new(),
            source: Consume {
                channel_hashes: Vec::new(),
                hash: Blake2b256Hash::from_bytes(vec![0x41; 32]),
                persistent: false,
            },
        }
    }

    #[test]
    fn continuation_identity_lookup_retains_first_representative_and_loaded_empty() {
        let channels = vec!["a".to_string(), "b".to_string()];
        let first = continuation(1, "same");
        let alias = continuation(9, "same");
        let other = continuation(2, "other");
        let mut store = RSpaceContinuationStore::new();

        store.set_group(channels.clone(), vec![first.clone(), alias, other.clone()]);
        let values = store.resting_values(&channels).expect("loaded");
        assert_eq!(values, vec![first, other]);
        assert_eq!(store.resting_record_count(), 2);

        store.set_group(channels.clone(), Vec::new());
        assert!(store.is_loaded(&channels));
        assert_eq!(store.resting_values(&channels), Some(Vec::new()));
    }

    #[test]
    fn continuation_journal_uses_full_canonical_record_path_and_cancels() {
        let channels = vec!["channel".to_owned()];
        let value = continuation(7, "continuation");
        let identity = encode_continuation_identity(&value);
        let mut canonical = identity.clone();
        encode_continuation_source_into(&value, &mut canonical);
        let expected_canonical = canonical.clone();
        let prefix = RhoPathV1::continuation_prefix(&channels);
        let path = RhoPathV1::collection_record_path(&prefix, &identity);
        let mut primary_path = prefix.clone();
        primary_path.extend_from_slice(&expected_canonical);
        let mut store = RSpaceContinuationStore::new();

        assert!(store.put_prepared(&channels, value, identity, canonical));
        assert_eq!(store.pending_delta().additions().count(&primary_path), 1);
        assert!(store.pending_delta().removals().is_empty());
        let transition = store
            .pending_content()
            .get(&path)
            .copied()
            .expect("continuation content transition");
        assert!(transition.before().is_none());
        assert!(transition.after().is_some());
        let source_record = store
            .source_record(&channels, &path)
            .expect("inserted continuation source record");
        assert_eq!(source_record.canonical_bytes(), expected_canonical.as_slice());
        let source_bytes = store
            .pending_source_bytes()
            .get(&path)
            .cloned()
            .expect("revision-local canonical source bytes");
        let mut restored = RSpaceContinuationStore::<String, String, String>::new();
        restored.restore_pending_source_bytes(store.pending_source_bytes().clone());
        assert_eq!(restored.pending_source_bytes().get(&path), Some(&source_bytes));

        assert!(store.remove_path(&channels, &primary_path).is_some());
        assert!(store.pending_delta().is_empty());
        assert!(store.pending_content().is_empty());
    }

    #[test]
    fn hydrated_continuation_replacement_retains_exact_source_transition() {
        let channels = vec!["channel".to_owned()];
        let first = continuation(1, "same");
        let replacement = continuation(9, "same");
        let identity = encode_continuation_identity(&first);
        assert_eq!(identity, encode_continuation_identity(&replacement));
        let prefix = RhoPathV1::continuation_prefix(&channels);
        let identity_path = RhoPathV1::collection_record_path(&prefix, &identity);
        let mut first_canonical = identity.clone();
        encode_continuation_source_into(&first, &mut first_canonical);
        let mut replacement_canonical = identity.clone();
        encode_continuation_source_into(&replacement, &mut replacement_canonical);
        let (first_ref, _) = continuation_source(&identity, &first_canonical);
        let (replacement_ref, replacement_bytes) =
            continuation_source(&identity, &replacement_canonical);
        assert_ne!(first_ref, replacement_ref);

        let mut store = RSpaceContinuationStore::new();
        store.set_group(channels.clone(), vec![first.clone()]);
        assert!(!store.put_prepared(
            &channels,
            replacement.clone(),
            identity.clone(),
            replacement_canonical.clone(),
        ));
        assert!(store.pending_content().is_empty());
        assert!(store.pending_source_bytes().is_empty());
        assert_eq!(
            store
                .source_record(&channels, &identity_path)
                .unwrap()
                .source_ref,
            Some(first_ref),
        );

        let mut first_path = prefix.clone();
        first_path.extend_from_slice(&first_canonical);
        assert_eq!(store.remove_path(&channels, &first_path), Some(first));
        let removed = store
            .pending_content()
            .get(&identity_path)
            .copied()
            .unwrap();
        assert_eq!(removed.before(), Some(first_ref));
        assert_eq!(removed.after(), None);
        assert!(store.pending_source_bytes().is_empty());

        assert!(store.put_prepared(
            &channels,
            replacement,
            identity.clone(),
            replacement_canonical,
        ));
        let replaced = store
            .pending_content()
            .get(&identity_path)
            .copied()
            .unwrap();
        assert_eq!(replaced.before(), Some(first_ref));
        assert_eq!(replaced.after(), Some(replacement_ref));
        assert_eq!(store.pending_source_bytes().get(&identity_path), Some(&replacement_bytes),);

        let mut restored = RSpaceContinuationStore::<String, String, String>::new();
        restored.restore_pending_content(store.pending_content().clone());
        restored.restore_pending_source_bytes(store.pending_source_bytes().clone());
        assert_eq!(restored.pending_content().get(&identity_path), Some(&replaced));
        assert_eq!(restored.pending_source_bytes().get(&identity_path), Some(&replacement_bytes),);
    }

    #[test]
    fn frozen_continuation_identity_repairs_legacy_debug_aliasing() {
        let channels = vec!["channel".to_owned()];
        let first = colliding_debug_continuation(1);
        let second = colliding_debug_continuation(2);
        let legacy_first = format!(
            "{:?}|{:?}|{}|{:?}",
            first.patterns, first.continuation, first.persist, first.peeks
        );
        let legacy_second = format!(
            "{:?}|{:?}|{}|{:?}",
            second.patterns, second.continuation, second.persist, second.peeks
        );

        assert_eq!(legacy_first, legacy_second, "the retired Debug identity aliases this pair");
        assert_ne!(
            encode_continuation_identity(&first),
            encode_continuation_identity(&second),
            "the frozen bincode identity must distinguish unequal continuations"
        );

        let mut store = RSpaceContinuationStore::new();
        store.set_group(channels.clone(), vec![first.clone(), second.clone()]);
        assert_eq!(store.resting_values(&channels), Some(vec![first, second]));
    }

    #[test]
    fn join_membership_is_exact_and_live_order_is_newest_first() {
        let channel = "a".to_string();
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "c".to_string()];
        let mut store = RSpaceJoinStore::new();

        store.set_channel(channel.clone(), vec![old.clone()]);
        assert!(store.put(&channel, &new));
        assert!(!store.put(&channel, &old));
        assert_eq!(store.resting_values(&channel), Some(vec![new.clone(), old.clone()]));
        assert_eq!(store.remove(&channel, &new), Some(new));
        assert_eq!(store.resting_values(&channel), Some(vec![old]));
    }

    #[test]
    fn installed_and_resting_join_domains_remain_independent() {
        let channel = "a".to_string();
        let installed = vec!["installed".to_string()];
        let resting = vec!["resting".to_string()];
        let mut store = RSpaceJoinStore::new();

        assert!(store.install(&channel, &installed));
        store.set_channel(channel.clone(), vec![resting.clone()]);
        assert_eq!(store.installed_values(&channel), vec![installed]);
        assert_eq!(store.resting_values(&channel), Some(vec![resting]));
    }

    #[test]
    fn imported_duplicate_joins_share_one_endpoint_but_preserve_visible_order_and_count() {
        let channel = "a".to_string();
        let repeated = vec!["b".to_string()];
        let other = vec!["a".to_string()];
        let mut store = RSpaceJoinStore::new();

        store.set_channel(channel.clone(), vec![
            repeated.clone(),
            other.clone(),
            repeated.clone(),
            repeated.clone(),
        ]);
        assert_eq!(
            store.resting_values(&channel),
            Some(vec![repeated.clone(), other.clone(), repeated.clone(), repeated.clone(),])
        );
        assert_eq!(store.resting_record_count(), 4);

        let mut canonical = Vec::new();
        store
            .try_visit_loaded_canonical_records(|record| {
                canonical.push(record);
                Ok(())
            })
            .expect("canonical joins");
        let expected = PersistedData::Joins(JoinsLeaf {
            bytes: encode_joins(&vec![
                repeated.clone(),
                other.clone(),
                repeated.clone(),
                repeated.clone(),
            ]),
        })
        .into_legacy_bincode();
        let prefix = RhoPathV1::join_prefix(&channel);
        let plan = store
            .resting
            .canonical_record_plan(&prefix, CanonicalCollectionKind::Joins)
            .expect("duplicate join plan");
        let mut replayed = Vec::with_capacity(plan.encoded_len());
        plan.try_replay(|bytes| {
            replayed.extend_from_slice(bytes);
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
        assert_eq!(plan.record_count(), 4);
        assert_eq!(replayed, expected);
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].encoded_persisted_data, expected);
        assert_eq!(canonical[0].record_count, 4);

        assert_eq!(store.remove(&channel, &repeated), Some(repeated.clone()));
        assert_eq!(
            store.resting_values(&channel),
            Some(vec![other.clone(), repeated.clone(), repeated.clone()])
        );
        assert_eq!(store.resting_record_count(), 3);

        assert_eq!(store.remove(&channel, &repeated), Some(repeated.clone()));
        assert_eq!(store.remove(&channel, &repeated), Some(repeated));
        assert_eq!(store.resting_values(&channel), Some(vec![other]));
        assert_eq!(store.resting_record_count(), 1);
    }

    #[test]
    fn canonical_continuation_egress_equals_the_legacy_sorted_leaf() {
        let channels = vec!["a".to_string(), "b".to_string()];
        let first = continuation(9, "z");
        let second = continuation(1, "a");
        let alias = continuation(7, "z");
        let mut store = RSpaceContinuationStore::new();
        store.set_group(channels.clone(), vec![first.clone(), second.clone(), alias]);

        let visible = vec![first, second];
        assert_eq!(store.resting_values(&channels), Some(visible.clone()));
        let expected = PersistedData::Continuations(ContinuationsLeaf {
            bytes: encode_continuations(&visible),
        })
        .into_legacy_bincode();
        let prefix = RhoPathV1::continuation_prefix(&channels);
        let plan = store
            .resting
            .canonical_record_plan(&prefix, CanonicalCollectionKind::Continuations)
            .expect("continuation plan");
        let mut replayed = Vec::with_capacity(plan.encoded_len());
        plan.try_replay(|bytes| {
            replayed.extend_from_slice(bytes);
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
        assert_eq!(plan.record_count(), 2);
        assert_eq!(replayed, expected);
        let mut canonical = Vec::new();
        store
            .try_visit_loaded_canonical_records(|record| {
                canonical.push(record);
                Ok(())
            })
            .expect("canonical continuations");
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].encoded_persisted_data, expected);
        assert_eq!(canonical[0].record_count, 2);
    }

    #[test]
    fn framed_identity_paths_are_prefix_free_and_preserve_identity_order() {
        let prefix = RhoPathV1::continuation_prefix(&["channel".to_string()]);
        let corpus = [
            Vec::new(),
            vec![0],
            vec![0, 0],
            vec![0, 1],
            vec![1],
            vec![1, 0],
            vec![1, 1],
            vec![0xff],
            vec![0xff, 0],
        ];

        for left in &corpus {
            for right in &corpus {
                let left_path = RhoPathV1::collection_record_path(&prefix, left);
                let right_path = RhoPathV1::collection_record_path(&prefix, right);
                assert_eq!(left.cmp(right), left_path.cmp(&right_path));
                if left != right {
                    assert!(!left_path.starts_with(&right_path));
                    assert!(!right_path.starts_with(&left_path));
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 128,
            failure_persistence: None,
            .. ProptestConfig::default()
        })]

        #[test]
        fn framed_identity_lookup_is_an_order_embedding(
            left in proptest::collection::vec(any::<u8>(), 0..128),
            right in proptest::collection::vec(any::<u8>(), 0..128),
        ) {
            let prefix = RhoPathV1::continuation_prefix(&["channel".to_string()]);
            let left_path = RhoPathV1::collection_record_path(&prefix, &left);
            let right_path = RhoPathV1::collection_record_path(&prefix, &right);
            prop_assert_eq!(left.cmp(&right), left_path.cmp(&right_path));
            if left != right {
                prop_assert!(!left_path.starts_with(&right_path));
                prop_assert!(!right_path.starts_with(&left_path));
            }
        }

        #[test]
        fn primary_record_zipper_order_equals_sorted_raw_wire_bytes(
            generated in proptest::collection::vec(
                proptest::collection::vec(any::<u8>(), 0..32),
                0..64,
            ),
        ) {
            let prefix = RhoPathV1::join_prefix(&"channel".to_string());
            let expected_records = generated
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mut insertion_order = expected_records.clone();
            insertion_order.reverse();
            let records = insertion_order
                .into_iter()
                .enumerate()
                .map(|(index, canonical)| {
                    (canonical.clone(), canonical, u64::try_from(index).unwrap())
                })
                .collect();
            let mut store: OrderedGroupStore<String, u64, false> =
                OrderedGroupStore::default();
            store.set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                records,
                false,
            ).unwrap();

            let (encoded, count) = store
                .encode_canonical_group(&prefix, CanonicalCollectionKind::Joins)
                .expect("raw primary records must have canonical egress");
            let expected = PersistedData::Joins(JoinsLeaf {
                bytes: bincode::serialize(&expected_records).unwrap(),
            })
            .into_legacy_bincode();
            prop_assert_eq!(count, expected_records.len() as u64);
            prop_assert_eq!(encoded, expected);
        }
    }

    #[test]
    fn full_record_key_order_handles_prefix_related_continuation_identities() {
        let prefix = RhoPathV1::continuation_prefix(&["channel".to_string()]);
        let short_identity = vec![0x10];
        let long_identity = vec![0x10, 0x20];
        let short_record = vec![0x10, 0xff];
        let long_record = vec![0x10, 0x20];
        let discarded_source_alias = vec![0x10, 0x00];
        assert!(long_identity.starts_with(&short_identity));
        assert!(short_identity < long_identity);
        assert!(short_record > long_record);

        let mut store: OrderedGroupStore<String, u8, true> = OrderedGroupStore::default();
        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![
                    (short_identity.clone(), short_record.clone(), 1),
                    (long_identity, long_record.clone(), 2),
                    (short_identity, discarded_source_alias, 3),
                ],
                false,
            )
            .unwrap();

        // Source-free identity lookup retains the first representative in
        // live order, while canonical egress follows complete record bytes.
        assert_eq!(store.group_values(&prefix), Some(vec![1, 2]));
        let (encoded, count) = store
            .encode_canonical_group(&prefix, CanonicalCollectionKind::Continuations)
            .expect("prefix-related identities must have canonical egress");
        let expected_records = vec![long_record, short_record];
        let expected = PersistedData::Continuations(ContinuationsLeaf {
            bytes: bincode::serialize(&expected_records).unwrap(),
        })
        .into_legacy_bincode();
        assert_eq!(count, 2);
        assert_eq!(encoded, expected);

        let short_identity_path =
            OrderedGroupStore::<String, u8, true>::identity_path(&prefix, &[0x10]);
        let short_record_path =
            OrderedGroupStore::<String, u8, true>::record_path(&prefix, &[0x10, 0xff]);
        assert!(store.identities.contains(short_identity_path.as_ref()));
        assert!(store.records.contains(short_record_path.as_ref()));
        assert_eq!(store.remove_path(&prefix, short_record_path.as_ref()), Some(1));
        assert!(!store.identities.contains(short_identity_path.as_ref()));
        assert!(!store.records.contains(short_record_path.as_ref()));
        assert_eq!(store.group_values(&prefix), Some(vec![2]));
    }

    #[test]
    fn distinct_identities_cannot_overwrite_one_canonical_primary_record() {
        let prefix = RhoPathV1::continuation_prefix(&["channel".to_string()]);
        let mut store: OrderedGroupStore<String, u8, true> = OrderedGroupStore::default();
        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![(vec![0x01], vec![0x01, 0xaa], 7)],
                false,
            )
            .unwrap();
        let record_path =
            OrderedGroupStore::<String, u8, true>::record_path(&prefix, &[0x01, 0xaa]);
        let identity_path = OrderedGroupStore::<String, u8, true>::identity_path(&prefix, &[0x01]);
        let before_group = Arc::clone(store.groups.get(&prefix).unwrap());
        let before_record = Arc::clone(store.records.get(record_path.as_ref()).unwrap());
        let before_identity = Arc::clone(store.identities.get(identity_path.as_ref()).unwrap());
        let before_group_count = store.group_count();

        let error = store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![(vec![0x10], vec![0x10, 0x20], 1), (vec![0x10, 0x20], vec![0x10, 0x20], 2)],
                false,
            )
            .unwrap_err();
        assert_eq!(
            error,
            RecordIndexError::InvariantViolation(
                "distinct continuation identities resolve to one canonical record path"
            )
        );
        assert!(Arc::ptr_eq(&before_group, store.groups.get(&prefix).unwrap()));
        assert!(Arc::ptr_eq(&before_record, store.records.get(record_path.as_ref()).unwrap()));
        assert!(Arc::ptr_eq(
            &before_identity,
            store.identities.get(identity_path.as_ref()).unwrap()
        ));
        assert_eq!(store.group_count(), before_group_count);
        assert_eq!(
            store.group_values(&prefix),
            Some(vec![7]),
            "a rejected private replacement must not change any public PathMap root"
        );
        assert_eq!(store.record_count(), 1);
        assert!(!store.records.contains(
            OrderedGroupStore::<String, u8, true>::record_path(&prefix, &[0x10, 0x20]).as_ref()
        ));
        assert!(!store.identities.contains(
            OrderedGroupStore::<String, u8, true>::identity_path(&prefix, &[0x10]).as_ref()
        ));
    }

    #[test]
    fn record_paths_cannot_be_removed_through_an_unrelated_group() {
        let left = RhoPathV1::continuation_prefix(&["left".to_string()]);
        let right = RhoPathV1::continuation_prefix(&["right".to_string()]);
        let mut store: OrderedGroupStore<String, u8, true> = OrderedGroupStore::default();
        store
            .set_group(
                &left,
                Arc::from(["left".to_string()]),
                vec![(vec![0x01], vec![0x01, 0x11], 1)],
                false,
            )
            .unwrap();
        store
            .set_group(
                &right,
                Arc::from(["right".to_string()]),
                vec![(vec![0x02], vec![0x02, 0x22], 2)],
                false,
            )
            .unwrap();
        let right_path = Arc::clone(&store.ordered_records(&right)[0].path);

        assert!(store.get_path(&left, right_path.as_ref()).is_none());
        assert!(store.get_path(&right, right_path.as_ref()).is_some());
        assert_eq!(store.remove_path(&left, right_path.as_ref()), None);
        assert_eq!(store.group_values(&left), Some(vec![1]));
        assert_eq!(store.group_values(&right), Some(vec![2]));
        assert_eq!(store.record_count(), 2);
    }

    #[test]
    fn duplicate_join_import_aggregates_one_primary_endpoint_with_exact_orders() {
        let prefix = RhoPathV1::join_prefix(&"channel".to_string());
        let repeated = vec![0x42; 32];
        let logical_count = 4_096usize;
        let mut store: OrderedGroupStore<String, u8, false> = OrderedGroupStore::default();
        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                (0..logical_count)
                    .map(|_| (repeated.clone(), repeated.clone(), 9))
                    .collect(),
                true,
            )
            .unwrap();

        let endpoints = store
            .records
            .read_zipper_at_borrowed_path(&prefix)
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].1.orders.len(), logical_count);
        assert_eq!(store.record_count(), logical_count);
        assert_eq!(store.group_values(&prefix), Some(vec![9; logical_count]));
    }

    #[test]
    fn duplicate_join_removal_debits_exactly_one_canonical_frame() {
        let prefix = RhoPathV1::join_prefix(&"channel".to_string());
        let repeated = vec![0x42; 32];
        let mut store: OrderedGroupStore<String, u8, false> = OrderedGroupStore::default();
        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![
                    (repeated.clone(), repeated.clone(), 9),
                    (repeated.clone(), repeated.clone(), 9),
                    (repeated.clone(), repeated.clone(), 9),
                ],
                true,
            )
            .unwrap();
        let snapshot = store.clone();

        for expected_count in [2u64, 1, 0] {
            assert_eq!(store.remove_identity(&prefix, &repeated), Some(9));
            let (encoded, count) = store
                .encode_canonical_group(&prefix, CanonicalCollectionKind::Joins)
                .expect("remaining duplicate join group must encode exactly");
            assert_eq!(count, expected_count);
            let expected_len = PERSISTED_DATA_HEADER_LEN +
                (u64::BITS / 8) as usize +
                expected_count as usize * ((u64::BITS / 8) as usize + repeated.len());
            assert_eq!(encoded.len(), expected_len);
        }
        assert_eq!(store.remove_identity(&prefix, &repeated), None);

        let (_, snapshot_count) = snapshot
            .encode_canonical_group(&prefix, CanonicalCollectionKind::Joins)
            .expect("CoW snapshot retains all duplicate joins");
        assert_eq!(snapshot_count, 3);
    }

    #[test]
    fn canonical_plan_is_a_stable_cow_snapshot_and_failed_replay_is_not_published() {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        struct InjectedFailure;

        let prefix = RhoPathV1::join_prefix(&"channel".to_string());
        let repeated = vec![0x10, 0x20];
        let child = vec![0x10, 0x20, 0x30];
        let mut store: OrderedGroupStore<String, u8, false> = OrderedGroupStore::default();
        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![
                    (repeated.clone(), repeated.clone(), 1),
                    (child.clone(), child.clone(), 2),
                    (repeated.clone(), repeated.clone(), 1),
                ],
                true,
            )
            .unwrap();
        let plan = store
            .canonical_record_plan(&prefix, CanonicalCollectionKind::Joins)
            .expect("preflighted snapshot");
        let expected = plan.encode_vec().expect("snapshot bytes");

        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![(vec![0xff], vec![0xff], 9)],
                true,
            )
            .unwrap();
        let current = store
            .encode_canonical_group(&prefix, CanonicalCollectionKind::Joins)
            .unwrap()
            .0;
        assert_ne!(current, expected);
        assert_eq!(plan.encode_vec().unwrap(), expected);

        let published = vec![0xa5];
        let mut staging = Vec::new();
        let mut writes = 0usize;
        let failure = plan.try_replay(|bytes| {
            writes += 1;
            if writes == 4 {
                return Err(InjectedFailure);
            }
            staging.extend_from_slice(bytes);
            Ok(())
        });
        assert_eq!(failure, Err(CanonicalRecordReplayError::Sink(InjectedFailure)));
        assert!(!staging.is_empty(), "the injected failure exercises mid-stream replay");
        assert_ne!(staging, expected, "failed private staging is incomplete");
        assert_eq!(published, vec![0xa5], "failed staging was never published");
    }

    #[test]
    fn canonical_preflight_rejects_length_and_zero_multiplicity_invariants() {
        let prefix = RhoPathV1::join_prefix(&"channel".to_string());
        let canonical = vec![0x42, 0x43];
        let mut store: OrderedGroupStore<String, u8, false> = OrderedGroupStore::default();
        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![(canonical.clone(), canonical.clone(), 7)],
                true,
            )
            .unwrap();

        let group = store.groups.get(&prefix).cloned().unwrap();
        let mut corrupt_group = (*group).clone();
        corrupt_group.encoded_entries_len += 1;
        store.groups.insert(&prefix, Arc::new(corrupt_group));
        assert_eq!(
            store
                .canonical_record_plan(&prefix, CanonicalCollectionKind::Joins)
                .err(),
            Some(RecordIndexError::InvariantViolation(
                "endpoint bytes differ from the transactionally maintained length"
            ))
        );

        store.groups.insert(&prefix, group);
        let path = OrderedGroupStore::<String, u8, false>::record_path(&prefix, &canonical);
        let record = store.records.get(path.as_ref()).cloned().unwrap();
        let mut corrupt_record = (*record).clone();
        corrupt_record.orders = Arc::from([]);
        store
            .records
            .insert(path.as_ref(), Arc::new(corrupt_record));
        assert_eq!(
            store
                .canonical_record_plan(&prefix, CanonicalCollectionKind::Joins)
                .err(),
            Some(RecordIndexError::InvariantViolation("record endpoint has zero multiplicity"))
        );
    }

    #[test]
    fn direct_zipper_visits_a_record_at_the_group_prefix() {
        let prefix = RhoPathV1::join_prefix(&"channel".to_string());
        let mut store: OrderedGroupStore<String, u8, false> = OrderedGroupStore::default();
        store
            .set_group(
                &prefix,
                Arc::from(["channel".to_string()]),
                vec![(Vec::new(), Vec::new(), 5), (vec![0x01], vec![0x01], 7)],
                true,
            )
            .unwrap();

        assert_eq!(store.group_values(&prefix), Some(vec![5, 7]));
        let (encoded, count) = store
            .encode_canonical_group(&prefix, CanonicalCollectionKind::Joins)
            .expect("prefix-value and child-value records must both encode");
        assert_eq!(count, 2);
        assert_eq!(encoded[PERSISTED_DATA_HEADER_LEN..], [
            2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1,
        ]);
    }

    #[test]
    fn depth_20000_record_lifecycle_fits_a_256_kib_native_stack() {
        thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let prefix = RhoPathV1::join_prefix(&"channel".to_owned());
                let canonical = vec![0x5a; 20_000];
                let mut store: OrderedGroupStore<String, u32, false> = OrderedGroupStore::default();
                store
                    .set_group(
                        &prefix,
                        Arc::from(["channel".to_owned()]),
                        vec![(canonical.clone(), canonical.clone(), 7)],
                        true,
                    )
                    .expect("deep record insertion");

                let snapshot = store.clone();
                assert_eq!(snapshot.group_values(&prefix), Some(vec![7]));
                let (encoded, count) = snapshot
                    .encode_canonical_group(&prefix, CanonicalCollectionKind::Joins)
                    .expect("deep record egress");
                assert_eq!(count, 1);
                assert!(encoded.ends_with(&canonical));

                assert_eq!(store.remove_identity(&prefix, &canonical), Some(7));
                assert_eq!(store.group_values(&prefix), Some(Vec::new()));
                drop(store);
                drop(snapshot);
            })
            .expect("small-stack record-index thread")
            .join()
            .expect("deep record lifecycle must remain stack-safe");
    }
}
