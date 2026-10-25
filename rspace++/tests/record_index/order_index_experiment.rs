//! Feature-gated executable models for operational record-order projection.

use std::collections::BTreeSet;
use std::sync::Arc;

use pathmap::PathMap;
use pathmap::zipper::ZipperWriting;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderArm {
    SortedPrepared,
    SharedRecordOrderTrie,
    SplitPayloadOrderTrie,
}

impl OrderArm {
    pub const ALL: [Self; 3] =
        [Self::SortedPrepared, Self::SharedRecordOrderTrie, Self::SplitPayloadOrderTrie];

    pub const fn label(self) -> &'static str {
        match self {
            Self::SortedPrepared => "sorted_prepared",
            Self::SharedRecordOrderTrie => "shared_record_order_trie",
            Self::SplitPayloadOrderTrie => "split_payload_order_trie",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionShape {
    Continuation,
    RestingJoin,
    InstalledJoin,
}

impl CollectionShape {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Continuation => "continuation",
            Self::RestingJoin => "resting_join",
            Self::InstalledJoin => "installed_join",
        }
    }

    const fn separate_identity(self) -> bool { matches!(self, Self::Continuation) }

    const fn kind(self) -> CanonicalCollectionKind {
        match self {
            Self::Continuation => CanonicalCollectionKind::Continuations,
            Self::RestingJoin | Self::InstalledJoin => CanonicalCollectionKind::Joins,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefixShape {
    Shared,
    Unshared,
}

impl PrefixShape {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Unshared => "unshared",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderCase {
    pub collection: CollectionShape,
    pub distinct: usize,
    pub multiplicity: usize,
    pub record_len: usize,
    pub prefix_len: usize,
    pub shape: PrefixShape,
}

impl OrderCase {
    pub fn logical_cardinality(self) -> usize {
        self.distinct
            .checked_mul(self.multiplicity)
            .expect("registered order-index cardinality fits usize")
    }

    pub fn label(self) -> String {
        format!(
            "{}-{}-d{}-m{}-r{}-p{}",
            self.collection.label(),
            self.shape.label(),
            self.distinct,
            self.multiplicity,
            self.record_len,
            self.prefix_len,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationCell {
    Project,
    Construct,
    Replace,
    Insert,
    RemoveIdentity,
    RemovePath,
    CloneUpdate,
    Clear,
    Drop,
}

impl OperationCell {
    pub const ALL: [Self; 9] = [
        Self::Project,
        Self::Construct,
        Self::Replace,
        Self::Insert,
        Self::RemoveIdentity,
        Self::RemovePath,
        Self::CloneUpdate,
        Self::Clear,
        Self::Drop,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Construct => "construct",
            Self::Replace => "replace",
            Self::Insert => "insert",
            Self::RemoveIdentity => "remove_identity",
            Self::RemovePath => "remove_path",
            Self::CloneUpdate => "clone_update",
            Self::Clear => "clear",
            Self::Drop => "drop",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectedRecord {
    pub position: u64,
    pub value: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderOutcome {
    pub records: Vec<ProjectedRecord>,
}

impl OrderOutcome {
    pub fn checksum(&self) -> u64 {
        self.records
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |state, record| {
                state
                    .wrapping_mul(0x0000_0100_0000_01b3)
                    .wrapping_add(record.position.rotate_left(17) ^ record.value)
            })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetainedFootprint {
    pub primary_endpoints: u64,
    pub identity_endpoints: u64,
    pub order_endpoints: u64,
    pub owned_payload_bytes_floor: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostics {
    pub logical_records: usize,
    pub canonical_endpoints: usize,
    pub identity_endpoints: usize,
    pub order_endpoints: usize,
    pub sorted_comparisons_floor: usize,
    pub retained: RetainedFootprint,
}

#[derive(Clone)]
struct GroupInput {
    prefix: Arc<[u8]>,
    key: Arc<[Vec<u8>]>,
    records: Arc<[(Vec<u8>, Vec<u8>, u64)]>,
    preserve_duplicates: bool,
    kind: CanonicalCollectionKind,
}

impl GroupInput {
    fn production_records(&self) -> Vec<(Vec<u8>, Vec<u8>, u64)> {
        self.records.iter().cloned().collect()
    }

    fn reversed(&self) -> Self {
        let mut records = self.production_records();
        records.reverse();
        Self {
            prefix: Arc::clone(&self.prefix),
            key: Arc::clone(&self.key),
            records: records.into(),
            preserve_duplicates: self.preserve_duplicates,
            kind: self.kind,
        }
    }

    #[cfg(test)]
    fn rotated(&self, by: usize) -> Self {
        let mut records = self.production_records();
        if !records.is_empty() {
            let distance = by % records.len();
            records.rotate_left(distance);
        }
        Self {
            prefix: Arc::clone(&self.prefix),
            key: Arc::clone(&self.key),
            records: records.into(),
            preserve_duplicates: self.preserve_duplicates,
            kind: self.kind,
        }
    }

    fn inserted_record(&self) -> (Vec<u8>, Vec<u8>, u64) {
        let index = self.records.len().saturating_add(0x10_0000);
        let mut identity = vec![0x7d; 32];
        identity.extend_from_slice(&(index as u64).to_be_bytes());
        let mut canonical = identity.clone();
        if self.kind == CanonicalCollectionKind::Continuations {
            canonical.push(0xa7);
        }
        (identity, canonical, index as u64)
    }

    fn removable_identity(&self) -> Option<Vec<u8>> {
        self.records
            .first()
            .map(|(identity, _, _)| identity.clone())
    }

    fn removable_canonical(&self) -> Option<Vec<u8>> {
        self.records
            .first()
            .map(|(_, canonical, _)| canonical.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupObservation {
    prefix: Vec<u8>,
    key: Vec<Vec<u8>>,
    visible: Vec<u64>,
    primary_paths: Vec<Vec<u8>>,
    identity_paths: Vec<Vec<u8>>,
    orders: Vec<u64>,
    next_order: u64,
    encoded_entries_len: u128,
    canonical: Vec<u8>,
    canonical_hash: Blake2b256Hash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoreObservation {
    groups: Vec<GroupObservation>,
    group_count: usize,
    record_count: usize,
}

fn observe_control<const SEPARATE_IDENTITY: bool>(
    store: &OrderedGroupStore<Vec<u8>, u64, SEPARATE_IDENTITY>,
    kinds: &PathMap<CanonicalCollectionKind>,
) -> StoreObservation {
    let mut groups = Vec::with_capacity(store.group_count);
    for (prefix, group) in store.groups.iter() {
        let kind = *kinds
            .get(&prefix)
            .expect("loaded group has a registered kind");
        let visible = store
            .ordered_records(&prefix)
            .into_iter()
            .map(|record| record.value)
            .collect();
        let mut primary_paths = Vec::new();
        let mut identity_paths = Vec::new();
        let mut orders = Vec::new();
        store.visit_group_records(&prefix, |_, record| {
            primary_paths.push(record.path.to_vec());
            if let Some(identity_path) = &record.identity_path {
                identity_paths.push(identity_path.to_vec());
            }
            orders.extend(record.orders.iter().copied());
        });
        primary_paths.sort_unstable();
        identity_paths.sort_unstable();
        orders.sort_unstable();
        let (canonical, count) = store
            .encode_canonical_group(&prefix, kind)
            .expect("coherent experiment group has canonical egress");
        assert_eq!(usize::try_from(count).expect("count fits memory"), group.len);
        groups.push(GroupObservation {
            prefix,
            key: group.key.to_vec(),
            visible,
            primary_paths,
            identity_paths,
            orders,
            next_order: group.next_order,
            encoded_entries_len: group.encoded_entries_len,
            canonical_hash: Blake2b256Hash::new(&canonical),
            canonical,
        });
    }
    groups.sort_unstable_by(|left, right| left.prefix.cmp(&right.prefix));
    StoreObservation {
        groups,
        group_count: store.group_count,
        record_count: store.record_count,
    }
}

fn visit_order_index<R: Clone + Send + Sync + Unpin>(
    index: &PathMap<R>,
    prefix: &[u8],
    mut visitor: impl FnMut(u64, &R),
) {
    let mut cursor = index.read_zipper_at_borrowed_path(prefix);
    if !cursor.path_exists() {
        return;
    }
    if let Some(value) = cursor.val() {
        visitor(decode_order(cursor.path()), value);
    }
    while let Some(value) = cursor.to_next_get_val() {
        visitor(decode_order(cursor.path()), value);
    }
}

fn project_control<const SEPARATE_IDENTITY: bool>(
    store: &OrderedGroupStore<Vec<u8>, u64, SEPARATE_IDENTITY>,
    prefix: &[u8],
) -> OrderOutcome {
    let ordered = store.ordered_records(prefix);
    let len = ordered.len();
    OrderOutcome {
        records: ordered
            .into_iter()
            .enumerate()
            .map(|(index, record)| ProjectedRecord {
                position: u64::try_from(len - index).expect("visible position fits u64"),
                value: record.value,
            })
            .collect(),
    }
}

#[derive(Clone)]
struct ControlStore<const SEPARATE_IDENTITY: bool> {
    store: OrderedGroupStore<Vec<u8>, u64, SEPARATE_IDENTITY>,
    kinds: PathMap<CanonicalCollectionKind>,
}

impl<const SEPARATE_IDENTITY: bool> Default for ControlStore<SEPARATE_IDENTITY> {
    fn default() -> Self {
        Self {
            store: OrderedGroupStore::default(),
            kinds: PathMap::new(),
        }
    }
}

impl<const SEPARATE_IDENTITY: bool> ControlStore<SEPARATE_IDENTITY> {
    fn set_group(&mut self, input: &GroupInput) {
        let mut replacement = self.clone();
        replacement
            .store
            .set_group(
                &input.prefix,
                Arc::clone(&input.key),
                input.production_records(),
                input.preserve_duplicates,
            )
            .expect("registered experiment group is valid");
        replacement.kinds.insert(&input.prefix, input.kind);
        *self = replacement;
    }

    fn insert(&mut self, input: &GroupInput, record: &(Vec<u8>, Vec<u8>, u64)) -> bool {
        let inserted = self
            .store
            .insert(
                &input.prefix,
                Arc::clone(&input.key),
                &record.0,
                record.1.clone(),
                record.2,
                None,
            )
            .expect("registered experiment insertion is valid");
        self.kinds.insert(&input.prefix, input.kind);
        inserted
    }

    fn remove_identity(&mut self, input: &GroupInput, identity: &[u8]) -> Option<u64> {
        self.store.remove_identity(&input.prefix, identity)
    }

    fn remove_path(&mut self, input: &GroupInput, canonical: &[u8]) -> Option<u64> {
        let path = OrderedGroupStore::<Vec<u8>, u64, SEPARATE_IDENTITY>::record_path(
            &input.prefix,
            canonical,
        );
        self.store.remove_path(&input.prefix, path.as_ref())
    }

    fn clear(&mut self) {
        self.store.clear();
        self.kinds = PathMap::new();
    }

    fn project(&self, prefix: &[u8]) -> OrderOutcome { project_control(&self.store, prefix) }

    fn observation(&self) -> StoreObservation { observe_control(&self.store, &self.kinds) }

    fn retained(&self) -> RetainedFootprint { base_footprint(&self.store, 0, 0) }
}

#[derive(Clone)]
struct SharedOrderStore<const SEPARATE_IDENTITY: bool> {
    base: ControlStore<SEPARATE_IDENTITY>,
    order: PathMap<Arc<StoredRecord<u64>>>,
}

impl<const SEPARATE_IDENTITY: bool> Default for SharedOrderStore<SEPARATE_IDENTITY> {
    fn default() -> Self {
        Self {
            base: ControlStore::default(),
            order: PathMap::new(),
        }
    }
}

impl<const SEPARATE_IDENTITY: bool> SharedOrderStore<SEPARATE_IDENTITY> {
    fn remove_group_index(&mut self, prefix: &[u8]) {
        self.order.remove_branches_at(prefix, true);
        self.order.remove_val_at(prefix, true);
    }

    fn index_group(&mut self, prefix: &[u8]) {
        self.remove_group_index(prefix);
        let base = &self.base.store;
        let mut order = self.order.write_zipper_at_path(prefix);
        base.visit_group_records(prefix, |_, record| {
            for ordinal in record.orders.iter().copied() {
                let suffix = (u64::MAX - ordinal).to_be_bytes();
                order.descend_to(suffix);
                assert!(order.set_val(Arc::clone(record)).is_none());
                assert!(order.ascend(suffix.len()));
            }
        });
    }

    fn set_group(&mut self, input: &GroupInput) {
        let mut replacement = self.clone();
        replacement.base.set_group(input);
        replacement.index_group(&input.prefix);
        *self = replacement;
    }

    fn insert(&mut self, input: &GroupInput, record: &(Vec<u8>, Vec<u8>, u64)) -> bool {
        let mut replacement = self.clone();
        if !replacement.base.insert(input, record) {
            return false;
        }
        let path = OrderedGroupStore::<Vec<u8>, u64, SEPARATE_IDENTITY>::record_path(
            &input.prefix,
            &record.1,
        );
        let stored = replacement
            .base
            .store
            .records
            .get(path.as_ref())
            .cloned()
            .expect("inserted primary endpoint");
        let ordinal = *stored.orders.first().expect("inserted ordinal");
        assert!(set_order(&mut replacement.order, &input.prefix, ordinal, stored).is_none());
        *self = replacement;
        true
    }

    fn remove_identity(&mut self, input: &GroupInput, identity: &[u8]) -> Option<u64> {
        let mut replacement = self.clone();
        let old = find_record(&replacement.base.store, &input.prefix, identity)?;
        let removed_ordinal = *old.orders.iter().max().expect("stored record has an order");
        let value = replacement.base.remove_identity(input, identity)?;
        remove_order(&mut replacement.order, &input.prefix, removed_ordinal)
            .expect("removed occurrence has an order endpoint");
        if let Some(current) = replacement
            .base
            .store
            .records
            .get(old.path.as_ref())
            .cloned()
        {
            let mut order = replacement.order.write_zipper_at_path(&input.prefix);
            for ordinal in current.orders.iter().copied() {
                let suffix = (u64::MAX - ordinal).to_be_bytes();
                order.descend_to(suffix);
                order.set_val(Arc::clone(&current));
                assert!(order.ascend(suffix.len()));
            }
        }
        *self = replacement;
        Some(value)
    }

    fn remove_path(&mut self, input: &GroupInput, canonical: &[u8]) -> Option<u64> {
        let mut replacement = self.clone();
        let path = OrderedGroupStore::<Vec<u8>, u64, SEPARATE_IDENTITY>::record_path(
            &input.prefix,
            canonical,
        );
        let old = replacement
            .base
            .store
            .get_path(&input.prefix, path.as_ref())?;
        let ordinals = Arc::clone(&old.orders);
        let value = replacement.base.remove_path(input, canonical)?;
        {
            let mut order = replacement.order.write_zipper_at_path(&input.prefix);
            for ordinal in ordinals.iter().copied() {
                let suffix = (u64::MAX - ordinal).to_be_bytes();
                order.descend_to(suffix);
                order
                    .remove_val(true)
                    .expect("removed primary occurrence has an order endpoint");
                assert!(order.ascend(suffix.len()));
            }
        }
        *self = replacement;
        Some(value)
    }

    fn clear(&mut self) {
        self.base.clear();
        self.order = PathMap::new();
    }

    fn project(&self, prefix: &[u8]) -> OrderOutcome {
        let capacity = self
            .base
            .store
            .groups
            .get(prefix)
            .map_or(0, |group| group.len);
        let mut records = Vec::with_capacity(capacity);
        visit_order_index(&self.order, prefix, |_, record| {
            let position = u64::try_from(capacity - records.len()).expect("position fits u64");
            records.push(ProjectedRecord {
                position,
                value: record.value,
            });
        });
        OrderOutcome { records }
    }

    fn audit(&self) {
        let mut order_count = 0usize;
        for (prefix, group) in self.base.store.groups.iter() {
            let mut seen = BTreeSet::new();
            visit_order_index(&self.order, &prefix, |ordinal, indexed| {
                assert!(ordinal > 0 && ordinal < group.next_order);
                assert!(seen.insert(ordinal));
                let primary = self
                    .base
                    .store
                    .records
                    .get(indexed.path.as_ref())
                    .expect("order endpoint names a primary endpoint");
                assert!(Arc::ptr_eq(primary, indexed));
                assert!(indexed.orders.contains(&ordinal));
                order_count += 1;
            });
            assert_eq!(seen.len(), group.len);
        }
        assert_eq!(order_count, self.base.store.record_count);
        assert_eq!(self.order.iter().count(), order_count);
    }

    fn observation(&self) -> StoreObservation {
        self.audit();
        self.base.observation()
    }

    fn retained(&self) -> RetainedFootprint {
        let path_bytes = self.order.iter().map(|(path, _)| path.len() as u64).sum();
        base_footprint(&self.base.store, self.order.iter().count() as u64, path_bytes)
    }
}

#[derive(Clone)]
struct ImmutablePayload {
    canonical_path: Arc<[u8]>,
    identity_path: Option<Arc<[u8]>>,
    value: u64,
    canonical_len: usize,
}

#[derive(Clone)]
struct SplitState {
    payload: Arc<ImmutablePayload>,
    orders: Arc<[u64]>,
}

#[derive(Clone)]
struct SplitOrderStore<const SEPARATE_IDENTITY: bool> {
    base: ControlStore<SEPARATE_IDENTITY>,
    records: PathMap<Arc<SplitState>>,
    identities: PathMap<Arc<ImmutablePayload>>,
    order: PathMap<Arc<ImmutablePayload>>,
}

impl<const SEPARATE_IDENTITY: bool> Default for SplitOrderStore<SEPARATE_IDENTITY> {
    fn default() -> Self {
        Self {
            base: ControlStore::default(),
            records: PathMap::new(),
            identities: PathMap::new(),
            order: PathMap::new(),
        }
    }
}

impl<const SEPARATE_IDENTITY: bool> SplitOrderStore<SEPARATE_IDENTITY> {
    fn clear_group_maps(&mut self, prefix: &[u8]) {
        self.records.remove_branches_at(prefix, true);
        self.records.remove_val_at(prefix, true);
        self.identities.remove_branches_at(prefix, true);
        self.identities.remove_val_at(prefix, true);
        self.order.remove_branches_at(prefix, true);
        self.order.remove_val_at(prefix, true);
    }

    fn publish(&mut self, record: &Arc<StoredRecord<u64>>) {
        let payload = Arc::new(ImmutablePayload {
            canonical_path: Arc::clone(&record.path),
            identity_path: record.identity_path.as_ref().map(Arc::clone),
            value: record.value,
            canonical_len: record.canonical_len,
        });
        self.records.insert(
            record.path.as_ref(),
            Arc::new(SplitState {
                payload: Arc::clone(&payload),
                orders: Arc::clone(&record.orders),
            }),
        );
        if let Some(identity_path) = &payload.identity_path {
            self.identities
                .insert(identity_path.as_ref(), Arc::clone(&payload));
        }
        let prefix = &record.path[..record.prefix_len];
        let mut order = self.order.write_zipper_at_path(prefix);
        for ordinal in record.orders.iter().copied() {
            let suffix = (u64::MAX - ordinal).to_be_bytes();
            order.descend_to(suffix);
            assert!(order.set_val(Arc::clone(&payload)).is_none());
            assert!(order.ascend(suffix.len()));
        }
    }

    fn index_group(&mut self, prefix: &[u8]) {
        self.clear_group_maps(prefix);
        let base = &self.base.store;
        let records = &mut self.records;
        let identities = &mut self.identities;
        let mut order = self.order.write_zipper_at_path(prefix);
        base.visit_group_records(prefix, |_, record| {
            let payload = Arc::new(ImmutablePayload {
                canonical_path: Arc::clone(&record.path),
                identity_path: record.identity_path.as_ref().map(Arc::clone),
                value: record.value,
                canonical_len: record.canonical_len,
            });
            records.insert(
                record.path.as_ref(),
                Arc::new(SplitState {
                    payload: Arc::clone(&payload),
                    orders: Arc::clone(&record.orders),
                }),
            );
            if let Some(identity_path) = &payload.identity_path {
                identities.insert(identity_path.as_ref(), Arc::clone(&payload));
            }
            for ordinal in record.orders.iter().copied() {
                let suffix = (u64::MAX - ordinal).to_be_bytes();
                order.descend_to(suffix);
                assert!(order.set_val(Arc::clone(&payload)).is_none());
                assert!(order.ascend(suffix.len()));
            }
        });
    }

    fn set_group(&mut self, input: &GroupInput) {
        let mut replacement = self.clone();
        replacement.base.set_group(input);
        replacement.index_group(&input.prefix);
        *self = replacement;
    }

    fn insert(&mut self, input: &GroupInput, record: &(Vec<u8>, Vec<u8>, u64)) -> bool {
        let mut replacement = self.clone();
        if !replacement.base.insert(input, record) {
            return false;
        }
        let path = OrderedGroupStore::<Vec<u8>, u64, SEPARATE_IDENTITY>::record_path(
            &input.prefix,
            &record.1,
        );
        let stored = replacement
            .base
            .store
            .records
            .get(path.as_ref())
            .cloned()
            .expect("inserted primary endpoint");
        replacement.publish(&stored);
        *self = replacement;
        true
    }

    fn remove_identity(&mut self, input: &GroupInput, identity: &[u8]) -> Option<u64> {
        let mut replacement = self.clone();
        let old = find_record(&replacement.base.store, &input.prefix, identity)?;
        let removed_ordinal = *old.orders.iter().max().expect("stored record has an order");
        let value = replacement.base.remove_identity(input, identity)?;
        remove_order(&mut replacement.order, &input.prefix, removed_ordinal)
            .expect("removed occurrence has an order endpoint");
        if let Some(current) = replacement
            .base
            .store
            .records
            .get(old.path.as_ref())
            .cloned()
        {
            let existing = replacement
                .records
                .get(old.path.as_ref())
                .cloned()
                .expect("remaining split primary endpoint");
            replacement.records.insert(
                old.path.as_ref(),
                Arc::new(SplitState {
                    payload: Arc::clone(&existing.payload),
                    orders: Arc::clone(&current.orders),
                }),
            );
        } else {
            replacement.records.remove(old.path.as_ref());
            if let Some(identity_path) = &old.identity_path {
                replacement.identities.remove(identity_path.as_ref());
            }
        }
        *self = replacement;
        Some(value)
    }

    fn remove_path(&mut self, input: &GroupInput, canonical: &[u8]) -> Option<u64> {
        let mut replacement = self.clone();
        let path = OrderedGroupStore::<Vec<u8>, u64, SEPARATE_IDENTITY>::record_path(
            &input.prefix,
            canonical,
        );
        let old = replacement
            .base
            .store
            .get_path(&input.prefix, path.as_ref())?;
        let ordinals = Arc::clone(&old.orders);
        let identity_path = old.identity_path.as_ref().map(Arc::clone);
        let value = replacement.base.remove_path(input, canonical)?;
        replacement
            .records
            .remove(path.as_ref())
            .expect("removed production primary has a split primary");
        if let Some(identity_path) = identity_path {
            replacement.identities.remove(identity_path.as_ref());
        }
        {
            let mut order = replacement.order.write_zipper_at_path(&input.prefix);
            for ordinal in ordinals.iter().copied() {
                let suffix = (u64::MAX - ordinal).to_be_bytes();
                order.descend_to(suffix);
                order
                    .remove_val(true)
                    .expect("removed split primary occurrence has an order endpoint");
                assert!(order.ascend(suffix.len()));
            }
        }
        *self = replacement;
        Some(value)
    }

    fn clear(&mut self) {
        self.base.clear();
        self.records = PathMap::new();
        self.identities = PathMap::new();
        self.order = PathMap::new();
    }

    fn project(&self, prefix: &[u8]) -> OrderOutcome {
        let capacity = self
            .base
            .store
            .groups
            .get(prefix)
            .map_or(0, |group| group.len);
        let mut records = Vec::with_capacity(capacity);
        self.visit_order(prefix, |_, payload| {
            let position = u64::try_from(capacity - records.len()).expect("position fits u64");
            records.push(ProjectedRecord {
                position,
                value: payload.value,
            });
        });
        OrderOutcome { records }
    }

    fn visit_order(&self, prefix: &[u8], visitor: impl FnMut(u64, &Arc<ImmutablePayload>)) {
        visit_order_index(&self.order, prefix, visitor);
    }

    fn audit(&self) {
        assert_eq!(self.base.store.records.iter().count(), self.records.iter().count());
        assert_eq!(self.base.store.identities.iter().count(), self.identities.iter().count());
        let mut order_count = 0usize;
        for (prefix, group) in self.base.store.groups.iter() {
            let mut seen = BTreeSet::new();
            self.visit_order(&prefix, |ordinal, payload| {
                assert!(ordinal > 0 && ordinal < group.next_order);
                assert!(seen.insert(ordinal));
                let state = self
                    .records
                    .get(payload.canonical_path.as_ref())
                    .expect("split order endpoint names a primary endpoint");
                assert!(Arc::ptr_eq(&state.payload, payload));
                assert!(state.orders.contains(&ordinal));
                assert_eq!(payload.canonical_len, payload.canonical_path.len() - prefix.len());
                order_count += 1;
            });
            assert_eq!(seen.len(), group.len);
        }
        for (path, state) in self.records.iter() {
            let primary = self
                .base
                .store
                .records
                .get(&path)
                .expect("split primary has a production primary");
            assert_eq!(state.payload.canonical_path.as_ref(), path.as_slice());
            assert_eq!(state.payload.value, primary.value);
            assert_eq!(state.orders.as_ref(), primary.orders.as_ref());
            assert_eq!(state.payload.identity_path.as_deref(), primary.identity_path.as_deref());
            if let Some(identity_path) = &state.payload.identity_path {
                let indexed = self
                    .identities
                    .get(identity_path.as_ref())
                    .expect("split identity endpoint");
                assert!(Arc::ptr_eq(indexed, &state.payload));
            }
        }
        assert_eq!(order_count, self.base.store.record_count);
        assert_eq!(self.order.iter().count(), order_count);
    }

    fn observation(&self) -> StoreObservation {
        self.audit();
        self.base.observation()
    }

    fn retained(&self) -> RetainedFootprint {
        let secondary_path_bytes = self
            .records
            .iter()
            .map(|(path, _)| path.len() as u64)
            .chain(self.identities.iter().map(|(path, _)| path.len() as u64))
            .chain(self.order.iter().map(|(path, _)| path.len() as u64))
            .sum();
        let mut retained = base_footprint(
            &self.base.store,
            self.order.iter().count() as u64,
            secondary_path_bytes,
        );
        retained.primary_endpoints += self.records.iter().count() as u64;
        retained.identity_endpoints += self.identities.iter().count() as u64;
        retained
    }
}

fn find_record<const SEPARATE_IDENTITY: bool>(
    store: &OrderedGroupStore<Vec<u8>, u64, SEPARATE_IDENTITY>,
    prefix: &[u8],
    identity: &[u8],
) -> Option<Arc<StoredRecord<u64>>> {
    if SEPARATE_IDENTITY {
        let path =
            OrderedGroupStore::<Vec<u8>, u64, SEPARATE_IDENTITY>::identity_path(prefix, identity);
        store.identities.get(path.as_ref()).cloned()
    } else {
        let path =
            OrderedGroupStore::<Vec<u8>, u64, SEPARATE_IDENTITY>::record_path(prefix, identity);
        store.records.get(path.as_ref()).cloned()
    }
}

fn base_footprint<const SEPARATE_IDENTITY: bool>(
    store: &OrderedGroupStore<Vec<u8>, u64, SEPARATE_IDENTITY>,
    order_endpoints: u64,
    additional_bytes: u64,
) -> RetainedFootprint {
    let primary_endpoints = store.records.iter().count() as u64;
    let identity_endpoints = store.identities.iter().count() as u64;
    let owned_payload_bytes_floor = store
        .groups
        .iter()
        .map(|(path, group)| {
            path.len() as u64 + group.key.iter().map(|part| part.len() as u64).sum::<u64>()
        })
        .chain(store.records.iter().map(|(path, record)| {
            path.len() as u64 +
                record
                    .identity_path
                    .as_ref()
                    .map_or(0, |path| path.len() as u64) +
                (record.orders.len() * std::mem::size_of::<u64>()) as u64
        }))
        .sum::<u64>()
        .saturating_add(additional_bytes);
    RetainedFootprint {
        primary_endpoints,
        identity_endpoints,
        order_endpoints,
        owned_payload_bytes_floor,
    }
}

#[derive(Clone)]
enum TypedStore<const SEPARATE_IDENTITY: bool> {
    Control(ControlStore<SEPARATE_IDENTITY>),
    Shared(SharedOrderStore<SEPARATE_IDENTITY>),
    Split(SplitOrderStore<SEPARATE_IDENTITY>),
}

impl<const SEPARATE_IDENTITY: bool> TypedStore<SEPARATE_IDENTITY> {
    fn empty(arm: OrderArm) -> Self {
        match arm {
            OrderArm::SortedPrepared => Self::Control(ControlStore::default()),
            OrderArm::SharedRecordOrderTrie => Self::Shared(SharedOrderStore::default()),
            OrderArm::SplitPayloadOrderTrie => Self::Split(SplitOrderStore::default()),
        }
    }

    fn set_group(&mut self, input: &GroupInput) {
        match self {
            Self::Control(store) => store.set_group(input),
            Self::Shared(store) => store.set_group(input),
            Self::Split(store) => store.set_group(input),
        }
    }

    fn insert(&mut self, input: &GroupInput, record: &(Vec<u8>, Vec<u8>, u64)) -> bool {
        match self {
            Self::Control(store) => store.insert(input, record),
            Self::Shared(store) => store.insert(input, record),
            Self::Split(store) => store.insert(input, record),
        }
    }

    fn remove_identity(&mut self, input: &GroupInput, identity: &[u8]) -> Option<u64> {
        match self {
            Self::Control(store) => store.remove_identity(input, identity),
            Self::Shared(store) => store.remove_identity(input, identity),
            Self::Split(store) => store.remove_identity(input, identity),
        }
    }

    fn remove_path(&mut self, input: &GroupInput, canonical: &[u8]) -> Option<u64> {
        match self {
            Self::Control(store) => store.remove_path(input, canonical),
            Self::Shared(store) => store.remove_path(input, canonical),
            Self::Split(store) => store.remove_path(input, canonical),
        }
    }

    fn clear(&mut self) {
        match self {
            Self::Control(store) => store.clear(),
            Self::Shared(store) => store.clear(),
            Self::Split(store) => store.clear(),
        }
    }

    fn project(&self, prefix: &[u8]) -> OrderOutcome {
        match self {
            Self::Control(store) => store.project(prefix),
            Self::Shared(store) => store.project(prefix),
            Self::Split(store) => store.project(prefix),
        }
    }

    fn observation(&self) -> StoreObservation {
        match self {
            Self::Control(store) => store.observation(),
            Self::Shared(store) => store.observation(),
            Self::Split(store) => store.observation(),
        }
    }

    fn retained(&self) -> RetainedFootprint {
        match self {
            Self::Control(store) => store.retained(),
            Self::Shared(store) => store.retained(),
            Self::Split(store) => store.retained(),
        }
    }
}

#[derive(Clone)]
enum ArmStore {
    Continuation(TypedStore<true>),
    Join(TypedStore<false>),
}

impl ArmStore {
    fn empty(collection: CollectionShape, arm: OrderArm) -> Self {
        if collection.separate_identity() {
            Self::Continuation(TypedStore::empty(arm))
        } else {
            Self::Join(TypedStore::empty(arm))
        }
    }

    fn set_group(&mut self, input: &GroupInput) {
        match self {
            Self::Continuation(store) => store.set_group(input),
            Self::Join(store) => store.set_group(input),
        }
    }

    fn insert(&mut self, input: &GroupInput, record: &(Vec<u8>, Vec<u8>, u64)) -> bool {
        match self {
            Self::Continuation(store) => store.insert(input, record),
            Self::Join(store) => store.insert(input, record),
        }
    }

    fn remove_identity(&mut self, input: &GroupInput, identity: &[u8]) -> Option<u64> {
        match self {
            Self::Continuation(store) => store.remove_identity(input, identity),
            Self::Join(store) => store.remove_identity(input, identity),
        }
    }

    fn remove_path(&mut self, input: &GroupInput, canonical: &[u8]) -> Option<u64> {
        match self {
            Self::Continuation(store) => store.remove_path(input, canonical),
            Self::Join(store) => store.remove_path(input, canonical),
        }
    }

    fn clear(&mut self) {
        match self {
            Self::Continuation(store) => store.clear(),
            Self::Join(store) => store.clear(),
        }
    }

    fn project(&self, prefix: &[u8]) -> OrderOutcome {
        match self {
            Self::Continuation(store) => store.project(prefix),
            Self::Join(store) => store.project(prefix),
        }
    }

    fn observation(&self) -> StoreObservation {
        match self {
            Self::Continuation(store) => store.observation(),
            Self::Join(store) => store.observation(),
        }
    }

    fn retained(&self) -> RetainedFootprint {
        match self {
            Self::Continuation(store) => store.retained(),
            Self::Join(store) => store.retained(),
        }
    }
}

fn assert_matches_observation(store: &ArmStore, expected: &StoreObservation) {
    assert_eq!(store.observation(), *expected);
    for group in &expected.groups {
        let projected = store.project(&group.prefix);
        assert_eq!(
            projected
                .records
                .iter()
                .map(|record| record.value)
                .collect::<Vec<_>>(),
            group.visible
        );
    }
}

pub struct OrderFixture {
    case: OrderCase,
    input: GroupInput,
    control: ArmStore,
    shared: ArmStore,
    split: ArmStore,
}

impl OrderFixture {
    pub fn new(case: OrderCase) -> Self {
        let input = group_input(case, 0);
        let mut control = ArmStore::empty(case.collection, OrderArm::SortedPrepared);
        let mut shared = ArmStore::empty(case.collection, OrderArm::SharedRecordOrderTrie);
        let mut split = ArmStore::empty(case.collection, OrderArm::SplitPayloadOrderTrie);
        control.set_group(&input);
        shared.set_group(&input);
        split.set_group(&input);
        let fixture = Self {
            case,
            input,
            control,
            shared,
            split,
        };
        fixture.audit();
        fixture
    }

    pub const fn case(&self) -> OrderCase { self.case }

    pub fn project(&self, arm: OrderArm, reuse: usize) -> OrderOutcome {
        assert!(reuse > 0);
        let store = self.store(arm);
        let mut outcome = None;
        for _ in 0..reuse {
            let next = store.project(&self.input.prefix);
            std::hint::black_box(&next);
            outcome = Some(next);
        }
        outcome.expect("positive reuse produces an outcome")
    }

    pub fn audit_outcome(&self, outcome: &OrderOutcome) {
        assert_eq!(outcome, &self.control.project(&self.input.prefix));
    }

    pub fn diagnostics(&self, arm: OrderArm) -> Diagnostics {
        let observation = self.store(arm).observation();
        let group = observation
            .groups
            .iter()
            .find(|group| group.prefix.as_slice() == self.input.prefix.as_ref())
            .expect("fixture group is loaded");
        let retained = self.store(arm).retained();
        Diagnostics {
            logical_records: group.visible.len(),
            canonical_endpoints: group.primary_paths.len(),
            identity_endpoints: group.identity_paths.len(),
            order_endpoints: retained.order_endpoints as usize,
            sorted_comparisons_floor: group.visible.len().saturating_sub(1),
            retained,
        }
    }

    pub fn prepare(&self, arm: OrderArm, operation: OperationCell) -> PreparedOperation {
        let mut expected = self.control.clone();
        let action = match operation {
            OperationCell::Construct => PreparedAction::Construct {
                collection: self.case.collection,
                arm,
                input: self.input.clone(),
                result: None,
            },
            OperationCell::Drop => PreparedAction::Drop(Some(self.store(arm).clone())),
            OperationCell::Project => PreparedAction::Project {
                store: self.store(arm).clone(),
                prefix: Arc::clone(&self.input.prefix),
                outcome: None,
            },
            OperationCell::Replace => {
                let input = self.input.reversed();
                expected.set_group(&input);
                PreparedAction::Replace {
                    store: self.store(arm).clone(),
                    input,
                }
            }
            OperationCell::Insert => {
                let record = self.input.inserted_record();
                expected.insert(&self.input, &record);
                PreparedAction::Insert {
                    store: self.store(arm).clone(),
                    input: self.input.clone(),
                    record,
                }
            }
            OperationCell::RemoveIdentity => {
                let identity = self.input.removable_identity().unwrap_or_default();
                expected.remove_identity(&self.input, &identity);
                PreparedAction::RemoveIdentity {
                    store: self.store(arm).clone(),
                    input: self.input.clone(),
                    identity,
                }
            }
            OperationCell::RemovePath => {
                let canonical = self.input.removable_canonical().unwrap_or_default();
                expected.remove_path(&self.input, &canonical);
                PreparedAction::RemovePath {
                    store: self.store(arm).clone(),
                    input: self.input.clone(),
                    canonical,
                }
            }
            OperationCell::CloneUpdate => {
                let record = self.input.inserted_record();
                expected.insert(&self.input, &record);
                PreparedAction::CloneUpdate {
                    source: self.store(arm).clone(),
                    updated: None,
                    input: self.input.clone(),
                    record,
                }
            }
            OperationCell::Clear => {
                expected.clear();
                PreparedAction::Clear(self.store(arm).clone())
            }
        };
        PreparedOperation {
            operation,
            initial: self.control.observation(),
            expected: expected.observation(),
            action,
        }
    }

    fn store(&self, arm: OrderArm) -> &ArmStore {
        match arm {
            OrderArm::SortedPrepared => &self.control,
            OrderArm::SharedRecordOrderTrie => &self.shared,
            OrderArm::SplitPayloadOrderTrie => &self.split,
        }
    }

    fn audit(&self) {
        let expected = self.control.observation();
        assert_eq!(self.shared.observation(), expected);
        assert_eq!(self.split.observation(), expected);
        for arm in OrderArm::ALL {
            self.audit_outcome(&self.store(arm).project(&self.input.prefix));
        }
    }
}

enum PreparedAction {
    Construct {
        collection: CollectionShape,
        arm: OrderArm,
        input: GroupInput,
        result: Option<ArmStore>,
    },
    Project {
        store: ArmStore,
        prefix: Arc<[u8]>,
        outcome: Option<OrderOutcome>,
    },
    Replace {
        store: ArmStore,
        input: GroupInput,
    },
    Insert {
        store: ArmStore,
        input: GroupInput,
        record: (Vec<u8>, Vec<u8>, u64),
    },
    RemoveIdentity {
        store: ArmStore,
        input: GroupInput,
        identity: Vec<u8>,
    },
    RemovePath {
        store: ArmStore,
        input: GroupInput,
        canonical: Vec<u8>,
    },
    CloneUpdate {
        source: ArmStore,
        updated: Option<ArmStore>,
        input: GroupInput,
        record: (Vec<u8>, Vec<u8>, u64),
    },
    Clear(ArmStore),
    Drop(Option<ArmStore>),
}

pub struct PreparedOperation {
    operation: OperationCell,
    initial: StoreObservation,
    expected: StoreObservation,
    action: PreparedAction,
}

impl PreparedOperation {
    pub fn execute(&mut self) -> u64 {
        match &mut self.action {
            PreparedAction::Construct {
                collection,
                arm,
                input,
                result,
            } => {
                let mut store = ArmStore::empty(*collection, *arm);
                store.set_group(input);
                *result = Some(store);
                1
            }
            PreparedAction::Project {
                store,
                prefix,
                outcome,
            } => {
                let projected = store.project(prefix);
                let checksum = projected.checksum();
                *outcome = Some(projected);
                checksum
            }
            PreparedAction::Replace { store, input } => {
                store.set_group(input);
                1
            }
            PreparedAction::Insert {
                store,
                input,
                record,
            } => u64::from(store.insert(input, record)),
            PreparedAction::RemoveIdentity {
                store,
                input,
                identity,
            } => store.remove_identity(input, identity).unwrap_or_default(),
            PreparedAction::RemovePath {
                store,
                input,
                canonical,
            } => store.remove_path(input, canonical).unwrap_or_default(),
            PreparedAction::CloneUpdate {
                source,
                updated,
                input,
                record,
            } => {
                let mut next = source.clone();
                let inserted = next.insert(input, record);
                *updated = Some(next);
                u64::from(inserted)
            }
            PreparedAction::Clear(store) => {
                store.clear();
                1
            }
            PreparedAction::Drop(store) => {
                drop(store.take());
                1
            }
        }
    }

    pub fn verify(&self) {
        match &self.action {
            PreparedAction::Construct { result, .. } => assert_matches_observation(
                result.as_ref().expect("constructed store"),
                &self.expected,
            ),
            PreparedAction::Project { store, outcome, .. } => {
                assert_matches_observation(store, &self.expected);
                let visible = self
                    .expected
                    .groups
                    .first()
                    .map_or(&[][..], |group| group.visible.as_slice());
                let expected = visible
                    .iter()
                    .enumerate()
                    .map(|(index, value)| ProjectedRecord {
                        position: u64::try_from(visible.len() - index)
                            .expect("visible position fits u64"),
                        value: *value,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(outcome.as_ref().expect("projected outcome").records, expected);
            }
            PreparedAction::Replace { store, .. } |
            PreparedAction::Insert { store, .. } |
            PreparedAction::RemoveIdentity { store, .. } |
            PreparedAction::RemovePath { store, .. } |
            PreparedAction::Clear(store) => assert_matches_observation(store, &self.expected),
            PreparedAction::CloneUpdate {
                source, updated, ..
            } => {
                assert_matches_observation(source, &self.initial);
                assert_matches_observation(
                    updated.as_ref().expect("updated clone"),
                    &self.expected,
                );
            }
            PreparedAction::Drop(store) => assert!(store.is_none()),
        }
    }

    pub fn retained(&self) -> RetainedFootprint {
        match &self.action {
            PreparedAction::Construct { result, .. } => result
                .as_ref()
                .map_or(RetainedFootprint::default(), ArmStore::retained),
            PreparedAction::Project { store, .. } |
            PreparedAction::Replace { store, .. } |
            PreparedAction::Insert { store, .. } |
            PreparedAction::RemoveIdentity { store, .. } |
            PreparedAction::RemovePath { store, .. } |
            PreparedAction::Clear(store) => store.retained(),
            PreparedAction::CloneUpdate { updated, .. } => updated
                .as_ref()
                .map_or(RetainedFootprint::default(), ArmStore::retained),
            PreparedAction::Drop(_) => RetainedFootprint::default(),
        }
    }

    pub const fn operation(&self) -> OperationCell { self.operation }
}

fn group_input(case: OrderCase, group: u64) -> GroupInput {
    assert!(case.multiplicity > 0 || case.distinct == 0);
    assert!(!case.collection.separate_identity() || case.multiplicity == 1);
    let mut channel = vec![0x63; case.prefix_len];
    channel.extend_from_slice(&group.to_be_bytes());
    let prefix = if case.collection.separate_identity() {
        RhoPathV1::continuation_prefix(std::slice::from_ref(&channel))
    } else {
        RhoPathV1::join_prefix(&channel)
    };
    let records = (0..case.logical_cardinality())
        .map(|position| {
            let identity_index = if case.distinct == 0 {
                0
            } else {
                position % case.distinct
            };
            let (identity, canonical) = record_bytes(case, identity_index, group);
            (identity, canonical, identity_index as u64 ^ group.rotate_left(23))
        })
        .collect::<Vec<_>>();
    GroupInput {
        prefix: prefix.into(),
        key: Arc::from([channel]),
        records: records.into(),
        preserve_duplicates: !case.collection.separate_identity(),
        kind: case.collection.kind(),
    }
}

fn record_bytes(case: OrderCase, index: usize, group: u64) -> (Vec<u8>, Vec<u8>) {
    let minimum = if case.collection.separate_identity() {
        17
    } else {
        16
    };
    let len = case.record_len.max(minimum);
    let identity_len = len - usize::from(case.collection.separate_identity());
    let mut identity = match case.shape {
        PrefixShape::Shared => vec![0x61; identity_len],
        PrefixShape::Unshared => {
            let index_bytes = (index as u64).to_le_bytes();
            let group_bytes = group.to_le_bytes();
            let seed =
                Blake2b256Hash::new_from_chunks([index_bytes.as_slice(), group_bytes.as_slice()])
                    .bytes();
            (0..identity_len)
                .map(|position| seed[position % seed.len()])
                .collect()
        }
    };
    let index_bytes = (index as u64).to_be_bytes();
    let group_bytes = group.to_be_bytes();
    let identity_len = identity.len();
    identity[identity_len - 16..identity_len - 8].copy_from_slice(&group_bytes);
    identity[identity_len - 8..].copy_from_slice(&index_bytes);
    if case.collection.separate_identity() {
        let mut canonical = identity.clone();
        canonical.push((index as u8).wrapping_mul(31));
        (identity, canonical)
    } else {
        (identity.clone(), identity)
    }
}

fn set_order<V: Clone + Send + Sync + Unpin>(
    index: &mut PathMap<V>,
    prefix: &[u8],
    order: u64,
    value: V,
) -> Option<V> {
    let suffix = (u64::MAX - order).to_be_bytes();
    let mut cursor = index.write_zipper_at_path(prefix);
    cursor.descend_to(suffix);
    cursor.set_val(value)
}

fn remove_order<V: Clone + Send + Sync + Unpin>(
    index: &mut PathMap<V>,
    prefix: &[u8],
    order: u64,
) -> Option<V> {
    let suffix = (u64::MAX - order).to_be_bytes();
    let mut cursor = index.write_zipper_at_path(prefix);
    cursor.descend_to(suffix);
    cursor.remove_val(true)
}

fn decode_order(relative: &[u8]) -> u64 {
    let encoded: [u8; 8] = relative
        .try_into()
        .expect("an order-index relative path is exactly eight bytes");
    u64::MAX - u64::from_be_bytes(encoded)
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    fn apply_and_compare(stores: &mut [ArmStore; 3], operation: impl Fn(&mut ArmStore)) {
        for store in stores.iter_mut() {
            operation(store);
        }
        let expected = stores[0].observation();
        for store in stores.iter() {
            assert_matches_observation(store, &expected);
        }
    }

    #[test]
    fn every_command_preserves_full_authority_relation_across_groups() {
        for collection in [
            CollectionShape::Continuation,
            CollectionShape::RestingJoin,
            CollectionShape::InstalledJoin,
        ] {
            let case = OrderCase {
                collection,
                distinct: 16,
                multiplicity: 1,
                record_len: 64,
                prefix_len: 32,
                shape: PrefixShape::Shared,
            };
            let first = group_input(case, 1);
            let second = group_input(case, 2);
            let empty = GroupInput {
                records: Arc::from([]),
                ..group_input(case, 3)
            };
            let mut stores = OrderArm::ALL.map(|arm| ArmStore::empty(collection, arm));

            assert!(
                stores
                    .iter()
                    .all(|store| store.observation().groups.is_empty())
            );
            apply_and_compare(&mut stores, |store| store.set_group(&empty));
            assert_eq!(stores[0].observation().groups[0].visible, Vec::<u64>::new());
            apply_and_compare(&mut stores, |store| store.set_group(&first));
            apply_and_compare(&mut stores, |store| store.set_group(&second.rotated(7)));
            apply_and_compare(&mut stores, |store| store.set_group(&first.reversed()));

            let inserted = first.inserted_record();
            apply_and_compare(&mut stores, |store| {
                assert!(store.insert(&first, &inserted));
            });
            apply_and_compare(&mut stores, |store| {
                assert!(!store.insert(&first, &inserted));
            });
            apply_and_compare(&mut stores, |store| {
                assert_eq!(store.remove_identity(&first, &inserted.0), Some(inserted.2));
            });
            apply_and_compare(&mut stores, |store| {
                assert!(store.insert(&first, &inserted));
            });
            apply_and_compare(&mut stores, |store| {
                assert_eq!(store.remove_path(&first, &inserted.1), Some(inserted.2));
            });

            for store in &stores {
                let snapshot = store.clone();
                let mut updated = store.clone();
                assert!(updated.insert(&first, &inserted));
                assert_eq!(snapshot.observation(), store.observation());
                assert_ne!(updated.observation(), snapshot.observation());
                drop(updated);
                assert_eq!(snapshot.observation(), store.observation());
            }
            apply_and_compare(&mut stores, ArmStore::clear);
            assert!(
                stores
                    .iter()
                    .all(|store| store.observation().groups.is_empty())
            );
        }
    }

    #[test]
    fn join_multiplicity_domain_is_exact_for_all_registered_cardinalities() {
        for multiplicity in [1, 2, 16, 256, 4_096] {
            let case = OrderCase {
                collection: CollectionShape::RestingJoin,
                distinct: 1,
                multiplicity,
                record_len: 64,
                prefix_len: 32,
                shape: PrefixShape::Unshared,
            };
            let input = group_input(case, multiplicity as u64);
            let mut stores = OrderArm::ALL.map(|arm| ArmStore::empty(case.collection, arm));
            apply_and_compare(&mut stores, |store| store.set_group(&input));
            let observation = stores[0].observation();
            let group = &observation.groups[0];
            assert_eq!(group.visible.len(), multiplicity);
            assert_eq!(group.primary_paths.len(), 1);
            assert_eq!(group.orders, (1..=multiplicity as u64).collect::<Vec<_>>());
            apply_and_compare(&mut stores, |store| {
                assert!(store.remove_identity(&input, &input.records[0].0).is_some());
            });
            assert_eq!(stores[0].observation().groups[0].visible.len(), multiplicity - 1);
            apply_and_compare(&mut stores, |store| {
                assert_eq!(
                    store.remove_path(&input, &input.records[0].1),
                    (multiplicity > 1).then_some(input.records[0].2)
                );
            });
            assert!(stores[0].observation().groups[0].visible.is_empty());
        }
    }

    #[test]
    fn all_operation_cells_are_executable_and_self_verifying() {
        for collection in [CollectionShape::Continuation, CollectionShape::RestingJoin] {
            let fixture = OrderFixture::new(OrderCase {
                collection,
                distinct: 16,
                multiplicity: 1,
                record_len: 64,
                prefix_len: 32,
                shape: PrefixShape::Shared,
            });
            for operation in OperationCell::ALL {
                for arm in OrderArm::ALL {
                    let mut prepared = fixture.prepare(arm, operation);
                    std::hint::black_box(prepared.execute());
                    prepared.verify();
                }
            }
        }
    }

    #[test]
    fn depth_20000_full_lifecycle_fits_a_256_kib_native_stack() {
        thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let case = OrderCase {
                    collection: CollectionShape::RestingJoin,
                    distinct: 1,
                    multiplicity: 2,
                    record_len: 20_000,
                    prefix_len: 32,
                    shape: PrefixShape::Shared,
                };
                let input = group_input(case, 7);
                let mut stores = OrderArm::ALL.map(|arm| ArmStore::empty(case.collection, arm));
                apply_and_compare(&mut stores, |store| store.set_group(&input));
                let inserted = input.inserted_record();
                apply_and_compare(&mut stores, |store| {
                    assert!(store.insert(&input, &inserted));
                });
                apply_and_compare(&mut stores, |store| {
                    assert_eq!(
                        store.remove_identity(&input, &input.records[0].0),
                        Some(input.records[0].2)
                    );
                });
                for store in stores {
                    let snapshot = store.clone();
                    drop(store);
                    assert!(!snapshot.observation().groups.is_empty());
                    drop(snapshot);
                }
            })
            .expect("small-stack record-order-index thread")
            .join()
            .expect("deep record-order-index lifecycle must remain stack-safe");
    }
}
