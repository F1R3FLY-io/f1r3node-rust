use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock, RwLock};

use pathmap::PathMap;
use pathmap::utils::{ByteMask, ByteMaskIter};
use pathmap::zipper::{
    ReadZipperUntracked, Zipper, ZipperMoving, ZipperReadOnlyValues, ZipperSubtries,
};
use serde::Serialize;

use super::bag::{Bag, BagDelta, BagError, Multiplicity};
use super::candidate_order::deterministic_candidate_hash;
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::internal::{Datum, DatumAddress};
use super::rho_path::{
    CompiledRhoPathQueryAtom, RhoPathEncode, RhoPathError, RhoPathQuery, RhoPathV1,
    StoredSequenceFrameScanner, StoredSequenceFrameStep,
};
use super::serializers::cold_store_decode::{ColdStoreDecode, ColdStoreDecodeError};
use super::serializers::serializers::{CandidateOrderingBytes, decode_datum, encode_datum};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataIndexError {
    Bag(BagError),
    Path(RhoPathError),
    Decode(ColdStoreDecodeError),
    CardinalityOverflow(u128),
    AllocationFailure(usize),
    IndexOutOfBounds { index: i32, cardinality: u128 },
}

impl fmt::Display for DataIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bag(error) => error.fmt(f),
            Self::Path(error) => error.fmt(f),
            Self::Decode(error) => error.fmt(f),
            Self::CardinalityOverflow(cardinality) => {
                write!(f, "bag cardinality {cardinality} does not fit in usize")
            }
            Self::AllocationFailure(requested) => {
                write!(f, "unable to reserve space for {requested} bag elements")
            }
            Self::IndexOutOfBounds { index, cardinality } => {
                write!(f, "datum index {index} is out of bounds for cardinality {cardinality}")
            }
        }
    }
}

impl std::error::Error for DataIndexError {}

impl From<BagError> for DataIndexError {
    fn from(error: BagError) -> Self { Self::Bag(error) }
}

impl From<RhoPathError> for DataIndexError {
    fn from(error: RhoPathError) -> Self { Self::Path(error) }
}

impl From<ColdStoreDecodeError> for DataIndexError {
    fn from(error: ColdStoreDecodeError) -> Self { Self::Decode(error) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedDatum<A: Clone> {
    pub path: Vec<u8>,
    pub datum: Datum<A>,
    pub multiplicity: Multiplicity,
}

#[derive(Debug)]
struct CandidatePoolEntry<A: Clone> {
    storage_path: Arc<[u8]>,
    relative_offset: usize,
    identity_offset: usize,
    datum: OnceLock<Datum<A>>,
}

impl<A: Clone> PartialEq for CandidatePoolEntry<A> {
    fn eq(&self, other: &Self) -> bool {
        self.storage_path == other.storage_path &&
            self.relative_offset == other.relative_offset &&
            self.identity_offset == other.identity_offset
    }
}

impl<A: Clone> Eq for CandidatePoolEntry<A> {}

impl<A: Clone> CandidatePoolEntry<A> {
    fn relative_path(&self) -> &[u8] { &self.storage_path[self.relative_offset..] }

    fn datum(&self, decode: fn(&[u8]) -> Datum<A>) -> &Datum<A> {
        self.datum
            .get_or_init(|| decode(&self.storage_path[self.identity_offset..]))
    }
}

fn decode_candidate<A: Clone + ColdStoreDecode>(bytes: &[u8]) -> Datum<A> {
    decode_datum(bytes).expect("PathMap candidate identity must decode after insertion")
}

struct EncodedIndexedDatum {
    path: Vec<u8>,
    identity_offset: usize,
    multiplicity: Multiplicity,
}

/// Fixed-width position in a candidate catalog's canonical order.
///
/// The identifier lives only in a derived in-memory PathMap. It is not a wire
/// value and deliberately does not use platform-width `usize` as trie data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidatePosition(u64);

impl CandidatePosition {
    fn from_index(index: usize) -> Self {
        Self(u64::try_from(index).expect("candidate catalog length must fit in u64"))
    }

    fn index(self) -> usize {
        usize::try_from(self.0).expect("candidate position must fit on the current platform")
    }
}

#[derive(Debug, Clone)]
struct CandidateCatalog<A: Clone> {
    ordered: Arc<[CandidatePoolEntry<A>]>,
    positions: Arc<PathMap<CandidatePosition>>,
    active_distinct_len: usize,
    cardinality: u128,
}

impl<A: Clone> CandidateCatalog<A> {
    fn from_encoded_entries(prefix_len: usize, entries: Vec<EncodedIndexedDatum>) -> Self
    where A: ColdStoreDecode {
        let active_distinct_len = entries.len();
        let cardinality = entries
            .iter()
            .map(|entry| u128::from(entry.multiplicity.get()))
            .sum();
        let mut positions = PathMap::new();
        let ordered = entries
            .into_iter()
            .enumerate()
            .map(|(position, entry)| {
                let previous = positions
                    .insert(&entry.path[prefix_len..], CandidatePosition::from_index(position));
                debug_assert!(previous.is_none(), "candidate paths are distinct");
                CandidatePoolEntry {
                    storage_path: Arc::from(entry.path),
                    relative_offset: prefix_len,
                    identity_offset: entry.identity_offset,
                    datum: OnceLock::new(),
                }
            })
            .collect::<Vec<_>>()
            .into();
        Self {
            ordered,
            positions: Arc::new(positions),
            active_distinct_len,
            cardinality,
        }
    }

    fn needs_refresh(&self) -> bool { self.active_distinct_len != self.ordered.len() }

    fn record_transition(
        &mut self,
        relative_path: &[u8],
        previous: Option<Multiplicity>,
        current: Option<Multiplicity>,
    ) -> bool {
        let previous_count = previous.map_or(0, Multiplicity::get);
        let current_count = current.map_or(0, Multiplicity::get);
        self.cardinality =
            self.cardinality - u128::from(previous_count) + u128::from(current_count);
        match (previous, current) {
            (None, Some(_)) => {
                if !self.positions.contains(relative_path) {
                    return false;
                }
                self.active_distinct_len += 1;
            }
            (Some(_), None) => self.active_distinct_len -= 1,
            _ => {}
        }
        true
    }

    fn record_insert(&mut self, relative_path: &[u8], multiplicity: Multiplicity) -> bool {
        let previous = (multiplicity != Multiplicity::ONE)
            .then(|| Multiplicity::new(multiplicity.get() - 1).expect("positive predecessor"));
        self.record_transition(relative_path, previous, Some(multiplicity))
    }

    fn record_remove(&mut self, relative_path: &[u8], previous: Multiplicity) {
        let current = (previous != Multiplicity::ONE)
            .then(|| Multiplicity::new(previous.get() - 1).expect("positive remainder"));
        let retained = self.record_transition(relative_path, Some(previous), current);
        debug_assert!(retained, "removal cannot introduce an unknown catalog identity");
    }
}

#[derive(Debug, Clone)]
pub struct CandidatePool<A: Clone> {
    availability: Bag,
    ordered: Arc<[CandidatePoolEntry<A>]>,
    positions: Arc<PathMap<CandidatePosition>>,
    fresh: Option<Datum<A>>,
    fresh_available: bool,
    decode: fn(&[u8]) -> Datum<A>,
}

#[derive(Clone)]
struct CandidateProductCursor<'a> {
    availability: ReadZipperUntracked<'a, 'static, Multiplicity>,
    positions: ReadZipperUntracked<'a, 'static, CandidatePosition>,
}

impl<'a> CandidateProductCursor<'a> {
    fn new(
        availability: &'a PathMap<Multiplicity>,
        positions: &'a PathMap<CandidatePosition>,
    ) -> Self {
        Self {
            availability: availability.read_zipper(),
            positions: positions.read_zipper(),
        }
    }

    fn descend_to_existing(&mut self, path: &[u8]) -> bool {
        self.availability.descend_to_existing(path) == path.len() &&
            self.positions.descend_to_existing(path) == path.len()
    }

    fn descend_to_byte(&mut self, byte: u8) {
        self.availability.descend_to_byte(byte);
        self.positions.descend_to_byte(byte);
    }

    fn ascend_byte(&mut self) {
        let availability_ascended = self.availability.ascend_byte();
        let positions_ascended = self.positions.ascend_byte();
        debug_assert_eq!(availability_ascended, positions_ascended);
    }

    fn child_mask(&self) -> ByteMask {
        self.availability.child_mask() & self.positions.child_mask()
    }

    fn position(&self) -> Option<CandidatePosition> {
        self.availability.get_val()?;
        self.positions.get_val().copied()
    }
}

struct CandidateQueryState<'a> {
    cursor: CandidateProductCursor<'a>,
    atom_index: usize,
    captures: PathMap<Arc<[u8]>>,
}

fn visit_sequence_frames<'a>(
    cursor: CandidateProductCursor<'a>,
    mut visitor: impl FnMut(CandidateProductCursor<'a>, &[u8]),
) {
    visit_sequence_frames_with_prefix(cursor, &[], &mut visitor);
}

fn visit_sequence_frames_with_prefix<'a>(
    mut cursor: CandidateProductCursor<'a>,
    prefix: &[u8],
    mut visitor: impl FnMut(CandidateProductCursor<'a>, &[u8]),
) {
    if !cursor.descend_to_existing(prefix) {
        return;
    }
    let mut scanner = StoredSequenceFrameScanner::default();
    for byte in prefix {
        if scanner.feed(*byte) != StoredSequenceFrameStep::NeedMore {
            return;
        }
    }
    let mut levels: Vec<(ByteMaskIter, StoredSequenceFrameScanner)> =
        vec![(cursor.child_mask().iter(), scanner)];
    let mut frame = prefix.to_vec();
    let base_len = frame.len();
    while let Some((children, scanner_base)) = levels.last_mut() {
        match children.next() {
            Some(byte) => {
                let mut scanner = *scanner_base;
                match scanner.feed(byte) {
                    StoredSequenceFrameStep::NeedMore => {
                        cursor.descend_to_byte(byte);
                        frame.push(byte);
                        levels.push((cursor.child_mask().iter(), scanner));
                    }
                    StoredSequenceFrameStep::Complete => {
                        cursor.descend_to_byte(byte);
                        frame.push(byte);
                        visitor(cursor.clone(), &frame);
                        frame.pop();
                        cursor.ascend_byte();
                    }
                    StoredSequenceFrameStep::Invalid => {}
                }
            }
            None => {
                levels.pop();
                if frame.len() > base_len {
                    frame.pop();
                    cursor.ascend_byte();
                }
            }
        }
    }
}

fn visit_product_values(
    mut cursor: CandidateProductCursor<'_>,
    mut visitor: impl FnMut(CandidatePosition),
) {
    if let Some(position) = cursor.position() {
        visitor(position);
    }
    let mut levels = vec![cursor.child_mask().iter()];
    while let Some(children) = levels.last_mut() {
        match children.next() {
            Some(byte) => {
                cursor.descend_to_byte(byte);
                if let Some(position) = cursor.position() {
                    visitor(position);
                }
                levels.push(cursor.child_mask().iter());
            }
            None => {
                levels.pop();
                if !levels.is_empty() {
                    cursor.ascend_byte();
                }
            }
        }
    }
}

impl<A: Clone + PartialEq> PartialEq for CandidatePool<A> {
    fn eq(&self, other: &Self) -> bool {
        self.availability == other.availability &&
            self.ordered == other.ordered &&
            self.fresh == other.fresh &&
            self.fresh_available == other.fresh_available
    }
}

impl<A: Clone + Eq> Eq for CandidatePool<A> {}

impl<A: Clone> CandidatePool<A> {
    fn from_catalog(availability: Bag, catalog: &CandidateCatalog<A>) -> Self
    where A: ColdStoreDecode {
        Self {
            availability,
            ordered: Arc::clone(&catalog.ordered),
            positions: Arc::clone(&catalog.positions),
            fresh: None,
            fresh_available: false,
            decode: decode_candidate::<A>,
        }
    }

    pub fn prepend_fresh(&mut self, datum: Datum<A>) {
        self.fresh = Some(datum);
        self.fresh_available = true;
    }

    pub fn len(&self) -> usize { self.ordered.len() + usize::from(self.fresh.is_some()) }

    pub fn is_empty(&self) -> bool { !self.fresh_available && self.availability.is_empty() }

    pub fn next_available(&self, start: usize) -> Option<usize> {
        (start..self.len()).find(|position| self.is_available(*position))
    }

    /// Canonical candidate positions whose RhoPath structure has `prefix`.
    ///
    /// PathMap performs the prefix cut before any stored datum is decoded. The
    /// zipper yields trie order, whereas RSpace consensus uses the catalog's
    /// deterministic hash order, so the fixed-width position values are sorted
    /// before they leave this derived index. Availability remains authoritative
    /// and is checked by the caller as reservations change during backtracking.
    pub(crate) fn positions_with_structure_prefix(
        &self,
        prefix: &[u8],
        fresh_matches: bool,
    ) -> Vec<usize> {
        let mut result = Vec::new();
        if self.fresh.is_some() && fresh_matches {
            result.push(0);
        }
        let stored_offset = usize::from(self.fresh.is_some());
        let encoded_prefix = RhoPathV1::relative_datum_structure_prefix(prefix);
        for (_, position) in self.positions.read_zipper_at_borrowed_path(&encoded_prefix) {
            result.push(position.index() + stored_offset);
        }
        result.sort_unstable();
        result
    }

    pub(crate) fn positions_matching_query(
        &self,
        query: &RhoPathQuery,
        fresh_matches: bool,
    ) -> Vec<usize> {
        let mut result = Vec::new();
        if self.fresh.is_some() && fresh_matches {
            result.push(0);
        }
        let stored_offset = usize::from(self.fresh.is_some());
        let mut root = CandidateProductCursor::new(self.availability.as_pathmap(), &self.positions);
        if !root.descend_to_existing(&[1]) {
            return result;
        }
        let mut work = vec![CandidateQueryState {
            cursor: root,
            atom_index: 0,
            captures: PathMap::new(),
        }];
        while let Some(state) = work.pop() {
            if let Some(atom) = query.atoms().get(state.atom_index) {
                match atom {
                    CompiledRhoPathQueryAtom::Exact { stored_frame, .. } => {
                        let mut cursor = state.cursor;
                        if cursor.descend_to_existing(stored_frame) {
                            work.push(CandidateQueryState {
                                cursor,
                                atom_index: state.atom_index + 1,
                                captures: state.captures,
                            });
                        }
                    }
                    CompiledRhoPathQueryAtom::Capture(id) => {
                        let capture_path = id.path();
                        if let Some(frame) = state.captures.get(capture_path) {
                            let mut cursor = state.cursor;
                            if cursor.descend_to_existing(frame.as_ref()) {
                                work.push(CandidateQueryState {
                                    cursor,
                                    atom_index: state.atom_index + 1,
                                    captures: state.captures,
                                });
                            }
                        } else {
                            visit_sequence_frames(state.cursor, |cursor, frame| {
                                let mut captures = state.captures.clone();
                                captures.insert(capture_path, Arc::from(frame));
                                work.push(CandidateQueryState {
                                    cursor,
                                    atom_index: state.atom_index + 1,
                                    captures,
                                });
                            });
                        }
                    }
                    CompiledRhoPathQueryAtom::Wildcard => {
                        visit_sequence_frames(state.cursor, |cursor, _| {
                            work.push(CandidateQueryState {
                                cursor,
                                atom_index: state.atom_index + 1,
                                captures: state.captures.clone(),
                            });
                        });
                    }
                    CompiledRhoPathQueryAtom::Match(query) => {
                        visit_sequence_frames_with_prefix(
                            state.cursor,
                            query.product_prefix(),
                            |cursor, frame| {
                                if query.matches_product_frame(frame) {
                                    work.push(CandidateQueryState {
                                        cursor,
                                        atom_index: state.atom_index + 1,
                                        captures: state.captures.clone(),
                                    });
                                }
                            },
                        );
                    }
                }
                continue;
            }

            let mut end = state.cursor.clone();
            if end.descend_to_existing(&[0, 0]) {
                visit_product_values(end, |position| {
                    result.push(position.index() + stored_offset);
                });
            }
            if query.accepts_remainder() {
                visit_sequence_frames(state.cursor, |cursor, _| {
                    work.push(CandidateQueryState {
                        cursor,
                        atom_index: state.atom_index,
                        captures: state.captures.clone(),
                    });
                });
            }
        }
        result.sort_unstable();
        result.dedup();
        result
    }

    pub(crate) fn fresh_datum(&self) -> Option<&Datum<A>> { self.fresh.as_ref() }

    pub fn candidate(&self, position: usize) -> Option<(&Datum<A>, DatumAddress)> {
        if !self.is_available(position) {
            return None;
        }
        if self.fresh.is_some() {
            if position == 0 {
                return self
                    .fresh
                    .as_ref()
                    .map(|datum| (datum, DatumAddress::Fresh));
            }
            let entry = self.ordered.get(position - 1)?;
            return Some((
                entry.datum(self.decode),
                DatumAddress::Stored(Arc::clone(&entry.storage_path)),
            ));
        }
        let entry = self.ordered.get(position)?;
        Some((entry.datum(self.decode), DatumAddress::Stored(Arc::clone(&entry.storage_path))))
    }

    pub fn reserve(&mut self, position: usize) -> bool {
        if self.fresh.is_some() {
            if position == 0 {
                if self.fresh_available {
                    self.fresh_available = false;
                    return true;
                }
                return false;
            }
            let Some(entry) = self.ordered.get(position - 1) else {
                return false;
            };
            if self.availability.count(entry.relative_path()) == 0 {
                return false;
            }
            self.availability.remove(entry.relative_path());
            return true;
        }
        let Some(entry) = self.ordered.get(position) else {
            return false;
        };
        if self.availability.count(entry.relative_path()) == 0 {
            return false;
        }
        self.availability.remove(entry.relative_path());
        true
    }

    pub fn release(&mut self, position: usize) {
        if self.fresh.is_some() {
            if position == 0 {
                self.fresh_available = true;
                return;
            }
            let entry = &self.ordered[position - 1];
            self.availability
                .insert(entry.relative_path())
                .expect("candidate reservation release cannot overflow");
            return;
        }
        let entry = &self.ordered[position];
        self.availability
            .insert(entry.relative_path())
            .expect("candidate reservation release cannot overflow");
    }

    pub fn retain(&mut self, mut predicate: impl FnMut(&Datum<A>) -> bool) {
        if self.fresh.as_ref().is_some_and(|datum| !predicate(datum)) {
            self.fresh = None;
            self.fresh_available = false;
        }
        for entry in self.ordered.iter() {
            if !predicate(entry.datum(self.decode)) {
                self.availability.set_count(entry.relative_path(), None);
            }
        }
    }

    pub(crate) fn is_available(&self, position: usize) -> bool {
        if self.fresh.is_some() {
            if position == 0 {
                return self.fresh_available;
            }
            return self
                .ordered
                .get(position - 1)
                .is_some_and(|entry| self.availability.contains(entry.relative_path()));
        }
        self.ordered
            .get(position)
            .is_some_and(|entry| self.availability.contains(entry.relative_path()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RSpaceDataIndex {
    bag: Bag,
}

impl RSpaceDataIndex {
    pub fn new() -> Self { Self::default() }

    pub fn from_bag(bag: Bag) -> Self { Self { bag } }

    pub fn as_bag(&self) -> &Bag { &self.bag }

    pub fn into_bag(self) -> Bag { self.bag }

    pub fn distinct_len(&self) -> usize { self.bag.distinct_len() }

    pub fn cardinality(&self) -> u128 { self.bag.cardinality() }

    pub fn is_empty(&self) -> bool { self.bag.is_empty() }

    pub fn clear_channel<C: RhoPathEncode>(&mut self, channel: &C) -> (usize, u128) {
        self.bag.remove_prefix(&RhoPathV1::datum_prefix(channel))
    }

    pub fn datum_path<C, A>(channel: &C, datum: &Datum<A>) -> Vec<u8>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        RhoPathV1::datum_key(channel, &datum.a.rho_path_structure(), &encode_datum(datum))
    }

    pub fn insert<C, A>(
        &mut self,
        channel: &C,
        datum: &Datum<A>,
    ) -> Result<(Vec<u8>, Multiplicity), DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let path = Self::datum_path(channel, datum);
        let multiplicity = self.bag.insert(&path)?;
        Ok((path, multiplicity))
    }

    pub fn count<C, A>(&self, channel: &C, datum: &Datum<A>) -> u64
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        self.bag.count(&Self::datum_path(channel, datum))
    }

    pub fn count_path(&self, path: &[u8]) -> u64 { self.bag.count(path) }

    pub fn remove_path(&mut self, path: &[u8]) -> Option<Multiplicity> { self.bag.remove(path) }

    pub fn channel_entries<C, A>(&self, channel: &C) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        let prefix = RhoPathV1::datum_prefix(channel);
        self.entries_at_prefix(&prefix)
    }

    pub fn structure_prefix_entries<C, A>(
        &self,
        channel: &C,
        structure_prefix: &[u8],
    ) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        let prefix = RhoPathV1::datum_structure_prefix(channel, structure_prefix);
        self.entries_at_prefix(&prefix)
    }

    pub fn ordered_channel_entries<C, A>(
        &self,
        channel: &C,
    ) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let entries = self.channel_entries(channel)?;
        let mut decorated = entries
            .into_iter()
            .map(|entry| (deterministic_candidate_hash(&entry.datum), entry))
            .collect::<Vec<_>>();
        decorated.sort_unstable_by(|(left_hash, left), (right_hash, right)| {
            left_hash
                .cmp(right_hash)
                .then_with(|| left.path.cmp(&right.path))
        });
        Ok(decorated.into_iter().map(|(_, entry)| entry).collect())
    }

    pub fn candidate_pool<C, A>(&self, channel: &C) -> Result<CandidatePool<A>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let prefix = RhoPathV1::datum_prefix(channel);
        let catalog = self.candidate_catalog::<A>(&prefix)?;
        self.candidate_pool_from_catalog(&prefix, &catalog)
    }

    fn candidate_catalog<A>(&self, prefix: &[u8]) -> Result<CandidateCatalog<A>, DataIndexError>
    where A: Clone + ColdStoreDecode {
        self.ordered_encoded_channel_entries(prefix)
            .map(|entries| CandidateCatalog::from_encoded_entries(prefix.len(), entries))
    }

    fn candidate_pool_from_catalog<A>(
        &self,
        prefix: &[u8],
        catalog: &CandidateCatalog<A>,
    ) -> Result<CandidatePool<A>, DataIndexError>
    where
        A: Clone + ColdStoreDecode,
    {
        let availability = Bag::from_pathmap_with_metrics(
            self.bag
                .as_pathmap()
                .read_zipper_at_borrowed_path(prefix)
                .make_map()
                .unwrap_or_default(),
            catalog.active_distinct_len,
            catalog.cardinality,
        );
        Ok(CandidatePool::from_catalog(availability, catalog))
    }

    fn ordered_encoded_channel_entries(
        &self,
        prefix: &[u8],
    ) -> Result<Vec<EncodedIndexedDatum>, DataIndexError> {
        let mut decorated = Vec::new();
        self.bag.try_visit_prefix(prefix, |path, multiplicity| {
            let view = RhoPathV1::parse_datum(path)?;
            let identity_offset = path.len() - view.datum_identity().len();
            decorated.push((Blake2b256Hash::new(view.datum_identity()), EncodedIndexedDatum {
                path: path.to_vec(),
                identity_offset,
                multiplicity,
            }));
            Ok::<(), DataIndexError>(())
        })?;
        decorated.sort_unstable_by(|(left_hash, left), (right_hash, right)| {
            left_hash
                .cmp(right_hash)
                .then_with(|| left.path.cmp(&right.path))
        });
        Ok(decorated.into_iter().map(|(_, entry)| entry).collect())
    }

    fn entries_at_prefix<A>(&self, prefix: &[u8]) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where A: Clone + ColdStoreDecode {
        let mut entries = Vec::new();
        self.bag.try_visit_prefix(prefix, |path, multiplicity| {
            let datum = RhoPathV1::parse_datum(path)
                .map_err(DataIndexError::from)
                .and_then(|view| decode_datum(view.datum_identity()).map_err(DataIndexError::from));
            entries.push(IndexedDatum {
                path: path.to_vec(),
                datum: datum?,
                multiplicity,
            });
            Ok::<(), DataIndexError>(())
        })?;
        Ok(entries)
    }
}

pub struct RSpaceDataStore<C, A: Clone> {
    index: RSpaceDataIndex,
    loaded_channels: PathMap<()>,
    loaded_channel_count: usize,
    candidate_catalogs: RwLock<HashMap<Vec<u8>, Arc<CandidateCatalog<A>>>>,
    marker: PhantomData<fn() -> (C, A)>,
}

impl<C, A: Clone> Clone for RSpaceDataStore<C, A> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            loaded_channels: self.loaded_channels.clone(),
            loaded_channel_count: self.loaded_channel_count,
            candidate_catalogs: RwLock::new(
                self.candidate_catalogs
                    .read()
                    .expect("candidate catalog read lock")
                    .clone(),
            ),
            marker: PhantomData,
        }
    }
}

impl<C, A: Clone> Default for RSpaceDataStore<C, A> {
    fn default() -> Self {
        Self {
            index: RSpaceDataIndex::new(),
            loaded_channels: PathMap::new(),
            loaded_channel_count: 0,
            candidate_catalogs: RwLock::new(HashMap::new()),
            marker: PhantomData,
        }
    }
}

impl<C, A: Clone> RSpaceDataStore<C, A> {
    pub fn new() -> Self { Self::default() }

    pub fn index(&self) -> &RSpaceDataIndex { &self.index }

    pub fn loaded_channel_count(&self) -> usize { self.loaded_channel_count }

    pub fn distinct_len(&self) -> usize { self.index.distinct_len() }

    pub fn cardinality(&self) -> u128 { self.index.cardinality() }

    pub fn is_empty(&self) -> bool { self.loaded_channel_count == 0 }

    pub fn clear(&mut self) {
        self.index = RSpaceDataIndex::new();
        self.loaded_channels = PathMap::new();
        self.loaded_channel_count = 0;
        self.candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock")
            .clear();
    }

    pub fn is_loaded(&self, channel: &C) -> bool
    where C: RhoPathEncode {
        self.loaded_channels
            .contains(RhoPathV1::datum_prefix(channel))
    }

    pub fn set_channel(
        &mut self,
        channel: C,
        datums: impl IntoIterator<Item = Datum<A>>,
    ) -> Result<(), DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let mut replacement = self.clone();
        replacement.index.clear_channel(&channel);
        let marker = RhoPathV1::datum_prefix(&channel);
        replacement
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock")
            .remove(&marker);
        if replacement.loaded_channels.insert(&marker, ()).is_none() {
            replacement.loaded_channel_count += 1;
        }
        for datum in datums {
            replacement.index.insert(&channel, &datum)?;
        }
        *self = replacement;
        Ok(())
    }

    pub fn insert(&mut self, channel: &C, datum: &Datum<A>) -> Result<Multiplicity, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let (path, multiplicity) = self.index.insert(channel, datum)?;
        let marker = RhoPathV1::datum_prefix(channel);
        let relative_path = path
            .strip_prefix(marker.as_slice())
            .expect("an inserted datum path must begin with its channel prefix");
        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        if catalogs.get_mut(&marker).is_some_and(|catalog| {
            !Arc::make_mut(catalog).record_insert(relative_path, multiplicity)
        }) {
            catalogs.remove(&marker);
        }
        if self.loaded_channels.insert(&marker, ()).is_none() {
            self.loaded_channel_count += 1;
        }
        Ok(multiplicity)
    }

    /// Atomically apply exact datum-identity multiplicity changes.
    ///
    /// Every changed path is validated before one BagDelta-owned PathMap COW
    /// root is published under the hot-store write lock. The data store does
    /// not wrap that atomic replacement in a redundant second root clone, and
    /// already-built candidate catalogs are advanced from the same exact
    /// before/after multiplicities rather than rebuilt speculatively.
    pub fn apply_delta(&mut self, delta: &BagDelta) -> Result<(), DataIndexError> {
        if delta.is_empty() {
            return Ok(());
        }

        for changes in [delta.additions(), delta.removals()] {
            changes.try_visit_prefix(b"", |path, _| {
                RhoPathV1::parse_datum(path)?;
                Ok::<(), DataIndexError>(())
            })?;
        }

        delta.apply_to(&mut self.index.bag)?;
        let bag = &self.index.bag;
        let loaded_channels = &mut self.loaded_channels;
        let loaded_channel_count = &mut self.loaded_channel_count;
        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        for (is_addition, changes) in [(true, delta.additions()), (false, delta.removals())] {
            changes.visit_prefix(b"", |path, amount| {
                let view = RhoPathV1::parse_datum(path)
                    .expect("a BagDelta datum path was validated before mutation");
                let marker = &path[..view.channel_prefix_len()];
                let relative_path = &path[view.channel_prefix_len()..];
                let current_count = bag.count(path);
                let previous_count = if is_addition {
                    current_count
                        .checked_sub(amount.get())
                        .expect("a committed addition contains its exact amount")
                } else {
                    current_count
                        .checked_add(amount.get())
                        .expect("a committed removal had a valid predecessor")
                };
                let previous = Multiplicity::new(previous_count).ok();
                let current = Multiplicity::new(current_count).ok();

                if is_addition && loaded_channels.insert(marker, ()).is_none() {
                    *loaded_channel_count += 1;
                }
                let invalidate = catalogs.get_mut(marker).is_some_and(|catalog| {
                    !Arc::make_mut(catalog).record_transition(relative_path, previous, current)
                });
                if invalidate {
                    catalogs.remove(marker);
                }
            });
        }
        Ok(())
    }

    pub fn channel_entries(
        &self,
        channel: &C,
    ) -> Result<Option<Vec<IndexedDatum<A>>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        if self.is_loaded(channel) {
            self.index.ordered_channel_entries(channel).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn channel_data(&self, channel: &C) -> Result<Option<Vec<Datum<A>>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        self.channel_entries(channel)?
            .map(expand_entries)
            .transpose()
    }

    pub fn channel_pool(&self, channel: &C) -> Result<Option<CandidatePool<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let prefix = RhoPathV1::datum_prefix(channel);
        if self.loaded_channels.contains(&prefix) {
            let cached = self
                .candidate_catalogs
                .read()
                .expect("candidate catalog read lock")
                .get(&prefix)
                .filter(|catalog| !catalog.needs_refresh())
                .cloned();
            let catalog = match cached {
                Some(catalog) => catalog,
                None => {
                    let mut catalogs = self
                        .candidate_catalogs
                        .write()
                        .expect("candidate catalog write lock");
                    if catalogs
                        .get(&prefix)
                        .is_none_or(|catalog| catalog.needs_refresh())
                    {
                        catalogs.insert(
                            prefix.clone(),
                            Arc::new(self.index.candidate_catalog(&prefix)?),
                        );
                    }
                    Arc::clone(
                        catalogs
                            .get(&prefix)
                            .expect("candidate catalog must exist after insertion"),
                    )
                }
            };
            self.index
                .candidate_pool_from_catalog(&prefix, catalog.as_ref())
                .map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn remove_path(&mut self, path: &[u8]) -> bool {
        let Some(previous) = Multiplicity::new(self.index.count_path(path)).ok() else {
            return false;
        };
        self.index.remove_path(path);
        if let Ok(view) = RhoPathV1::parse_datum(path) {
            let relative_path = &path[view.channel_prefix_len()..];
            if let Some(catalog) = self
                .candidate_catalogs
                .get_mut()
                .expect("candidate catalog write lock")
                .get_mut(&path[..view.channel_prefix_len()])
            {
                Arc::make_mut(catalog).record_remove(relative_path, previous);
            }
        }
        true
    }

    pub fn remove_index(&mut self, channel: &C, index: i32) -> Result<(), DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let entries = self.channel_entries(channel)?.unwrap_or_default();
        let cardinality = channel_cardinality(&entries);
        let mut remaining = u64::try_from(index)
            .map_err(|_| DataIndexError::IndexOutOfBounds { index, cardinality })?;
        for entry in entries {
            if remaining < entry.multiplicity.get() {
                self.remove_path(&entry.path);
                return Ok(());
            }
            remaining -= entry.multiplicity.get();
        }
        Err(DataIndexError::IndexOutOfBounds { index, cardinality })
    }

    pub fn loaded_records(&self) -> Result<Vec<(C, Vec<Datum<A>>)>, DataIndexError>
    where
        C: Clone + ColdStoreDecode + RhoPathEncode,
        A: Clone + ColdStoreDecode + Serialize,
        Datum<A>: CandidateOrderingBytes,
    {
        let mut records = Vec::with_capacity(self.loaded_channel_count);
        for (path, _) in self.loaded_channels.iter() {
            let view = RhoPathV1::parse_datum_prefix(&path)?;
            let channel = C::cold_decode(&view.channel_identity())?;
            let datums = self.channel_data(&channel)?.unwrap_or_default();
            records.push((channel, datums));
        }
        Ok(records)
    }
}

impl<C, A: Clone> fmt::Debug for RSpaceDataStore<C, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RSpaceDataStore")
            .field("loaded_channel_count", &self.loaded_channel_count)
            .field("distinct_len", &self.distinct_len())
            .field("cardinality", &self.cardinality())
            .finish()
    }
}

impl<C, A: Clone> PartialEq for RSpaceDataStore<C, A> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index &&
            self.loaded_channel_count == other.loaded_channel_count &&
            self.loaded_channels
                .iter()
                .all(|(path, _)| other.loaded_channels.contains(&path))
    }
}

impl<C, A: Clone> Eq for RSpaceDataStore<C, A> {}

fn channel_cardinality<A: Clone>(entries: &[IndexedDatum<A>]) -> u128 {
    entries
        .iter()
        .map(|entry| u128::from(entry.multiplicity.get()))
        .sum()
}

fn expand_entries<A: Clone>(
    entries: Vec<IndexedDatum<A>>,
) -> Result<Vec<Datum<A>>, DataIndexError> {
    let cardinality = channel_cardinality(&entries);
    let capacity = usize::try_from(cardinality)
        .map_err(|_| DataIndexError::CardinalityOverflow(cardinality))?;
    let mut datums = Vec::new();
    datums
        .try_reserve_exact(capacity)
        .map_err(|_| DataIndexError::AllocationFailure(capacity))?;
    for entry in entries {
        for _ in 0..entry.multiplicity.get() {
            datums.push(entry.datum.clone());
        }
    }
    Ok(datums)
}
