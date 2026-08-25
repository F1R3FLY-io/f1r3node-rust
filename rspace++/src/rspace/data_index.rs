use std::cmp::Ordering;
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
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::internal::{Datum, DatumAddress};
use super::rho_path::{
    CompiledRhoPathQueryAtom, RhoPathEncode, RhoPathError, RhoPathQuery, RhoPathV1,
    StoredSequenceFrameScanner, StoredSequenceFrameStep,
};
use super::serializers::cold_store_decode::{ColdStoreDecode, ColdStoreDecodeError};
use super::serializers::serializers::{decode_datum, encode_datum};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataIndexError {
    Bag(BagError),
    Path(RhoPathError),
    Decode(ColdStoreDecodeError),
    CardinalityOverflow(u128),
    EncodedCardinalityOverflow(u128),
    EncodedLengthOverflow(u128),
    AllocationFailure(usize),
    DuplicateDatumIdentity,
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
            Self::EncodedCardinalityOverflow(cardinality) => write!(
                f,
                "datum cardinality {cardinality} does not fit the legacy bincode Vec length"
            ),
            Self::EncodedLengthOverflow(length) => {
                write!(f, "encoded datum sequence length {length} does not fit in usize")
            }
            Self::AllocationFailure(requested) => {
                write!(f, "unable to reserve requested capacity {requested}")
            }
            Self::DuplicateDatumIdentity => f.write_str(
                "one canonical datum identity is stored under multiple derived RhoPaths",
            ),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalDataRecord {
    pub channel_hash: Blake2b256Hash,
    pub encoded_datums: Vec<u8>,
    pub datum_count: u64,
}

#[derive(Debug)]
struct DatumIdentityDescriptor {
    start: usize,
    len: usize,
    multiplicity: Multiplicity,
}

impl DatumIdentityDescriptor {
    fn identity<'a>(&self, arena: &'a [u8]) -> &'a [u8] {
        &arena[self.start..self.start + self.len]
    }
}

/// Compare datum candidates without consulting their derived structural path.
///
/// The 256-bit hash is the ordinary fast ordering key. Canonical datum bytes
/// make its collision case total and representation-independent: changing an
/// in-memory RhoMatchPath version can never change which COMM is considered
/// first. The identity is already the collision-free suffix of every stored
/// datum path, so no additional trie-path tie-break is required.
fn compare_candidate_identity(
    left_hash: &Blake2b256Hash,
    left_identity: &[u8],
    right_hash: &Blake2b256Hash,
    right_identity: &[u8],
) -> Ordering {
    left_hash
        .cmp(right_hash)
        .then_with(|| left_identity.cmp(right_identity))
}

#[cfg(not(test))]
#[inline(always)]
fn datum_identity_digest(identity: &[u8]) -> Blake2b256Hash { Blake2b256Hash::new(identity) }

#[cfg(test)]
fn datum_identity_digest(identity: &[u8]) -> Blake2b256Hash {
    super::candidate_digest_test_hook::digest(identity)
}

fn sort_encoded_candidates(entries: &mut [(Blake2b256Hash, EncodedIndexedDatum)]) {
    entries.sort_unstable_by(|(left_hash, left), (right_hash, right)| {
        compare_candidate_identity(
            left_hash,
            &left.path[left.identity_offset..],
            right_hash,
            &right.path[right.identity_offset..],
        )
    });
}

fn validate_unique_candidate_identities(
    entries: &[(Blake2b256Hash, EncodedIndexedDatum)],
) -> Result<(), DataIndexError> {
    if entries.windows(2).any(|pair| {
        if pair[0].0 != pair[1].0 {
            return false;
        }
        let left = &pair[0].1;
        let right = &pair[1].1;
        left.path[left.identity_offset..] == right.path[right.identity_offset..]
    }) {
        return Err(DataIndexError::DuplicateDatumIdentity);
    }
    Ok(())
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
    /// Authoritative stored multiplicities after any replay preselection
    /// filter. Candidate search never mutates this PathMap-backed Bag.
    authority: Bag,
    /// Speculative, non-persistent demand placed against `authority`.
    /// Repeated-channel joins share one pool, so multiplicity is reserved once
    /// per selected occurrence without copying or rewriting the authority trie.
    reserved: Bag,
    ordered: Arc<[CandidatePoolEntry<A>]>,
    positions: Arc<PathMap<CandidatePosition>>,
    fresh: Option<Datum<A>>,
    fresh_available: bool,
    decode: fn(&[u8]) -> Datum<A>,
}

#[cfg_attr(any(test, feature = "candidate-checkpoint-experiment"), derive(Clone))]
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
        let availability_depth = self.availability.descend_to_existing(path);
        let positions_depth = self.positions.descend_to_existing(path);
        if availability_depth == path.len() && positions_depth == path.len() {
            return true;
        }
        for _ in 0..availability_depth {
            self.availability.ascend_byte();
        }
        for _ in 0..positions_depth {
            self.positions.ascend_byte();
        }
        false
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

    fn ascend_bytes(&mut self, count: usize) {
        for _ in 0..count {
            self.ascend_byte();
        }
    }

    fn child_mask(&self) -> ByteMask {
        self.availability.child_mask() & self.positions.child_mask()
    }

    fn position(&self) -> Option<CandidatePosition> {
        self.availability.get_val()?;
        self.positions.get_val().copied()
    }

    fn checkpoint(&self) -> Option<CandidateProductCheckpoint> {
        let availability_root = self.availability.get_val().copied();
        let positions_root = self.positions.get_val().copied();
        let mut availability = ZipperSubtries::make_map(&self.availability).unwrap_or_default();
        let mut positions = ZipperSubtries::make_map(&self.positions).unwrap_or_default();
        if let Some(value) = availability_root {
            availability.insert([], value);
        }
        if let Some(value) = positions_root {
            positions.insert([], value);
        }
        if availability.is_empty() || positions.is_empty() {
            return None;
        }
        #[cfg(test)]
        {
            candidate_product_test_metrics::record_checkpoint();
            if availability_root.is_some() || positions_root.is_some() {
                candidate_product_test_metrics::record_root_endpoint_restoration();
            }
        }
        Some(CandidateProductCheckpoint {
            availability,
            positions,
        })
    }
}

/// A depth-independent candidate-product continuation.
///
/// Current RhoPath framing places stored candidate values only after the outer
/// structure terminator and datum-identity suffix, never at the boundary after
/// one complete component. Checkpoint extraction nevertheless preserves a
/// focus value defensively, so correctness does not depend on that framing
/// invariant, PathMap's optional `graft_root_vals` feature, or a future
/// encoding continuing to omit endpoints at component boundaries.
#[derive(Clone)]
struct CandidateProductCheckpoint {
    availability: PathMap<Multiplicity>,
    positions: PathMap<CandidatePosition>,
}

impl CandidateProductCheckpoint {
    fn cursor(&self) -> CandidateProductCursor<'_> {
        CandidateProductCursor::new(&self.availability, &self.positions)
    }
}

struct CandidateQueryState {
    checkpoint: CandidateProductCheckpoint,
    atom_index: usize,
    captures: PathMap<Arc<[u8]>>,
}

fn visit_sequence_frames(
    cursor: CandidateProductCursor<'_>,
    mut visitor: impl FnMut(CandidateProductCheckpoint, &[u8]),
) {
    visit_sequence_frames_with_prefix(cursor, &[], |_| true, &mut visitor);
}

fn visit_sequence_frames_with_prefix(
    mut cursor: CandidateProductCursor<'_>,
    prefix: &[u8],
    mut accepts: impl FnMut(&[u8]) -> bool,
    mut visitor: impl FnMut(CandidateProductCheckpoint, &[u8]),
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
                        #[cfg(test)]
                        candidate_product_test_metrics::record_frame();
                        if accepts(&frame) {
                            if let Some(checkpoint) = cursor.checkpoint() {
                                visitor(checkpoint, &frame);
                            }
                        }
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
    cursor: &mut CandidateProductCursor<'_>,
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

fn drive_candidate_query(
    mut cursor: CandidateProductCursor<'_>,
    mut atom_index: usize,
    captures: PathMap<Arc<[u8]>>,
    query: &RhoPathQuery,
    stored_offset: usize,
    work: &mut Vec<CandidateQueryState>,
    result: &mut Vec<usize>,
) {
    loop {
        let Some(atom) = query.atoms().get(atom_index) else {
            break;
        };
        match atom {
            CompiledRhoPathQueryAtom::Exact { stored_frame, .. } => {
                if !cursor.descend_to_existing(stored_frame) {
                    return;
                }
                atom_index += 1;
            }
            CompiledRhoPathQueryAtom::Capture(id) => {
                let capture_path = id.path();
                if let Some(frame) = captures.get(capture_path) {
                    if !cursor.descend_to_existing(frame.as_ref()) {
                        return;
                    }
                    atom_index += 1;
                    continue;
                }
                visit_sequence_frames(cursor, |checkpoint, frame| {
                    let mut captures = captures.clone();
                    captures.insert(capture_path, Arc::from(frame));
                    work.push(CandidateQueryState {
                        checkpoint,
                        atom_index: atom_index + 1,
                        captures,
                    });
                });
                return;
            }
            CompiledRhoPathQueryAtom::Wildcard => {
                visit_sequence_frames(cursor, |checkpoint, _| {
                    work.push(CandidateQueryState {
                        checkpoint,
                        atom_index: atom_index + 1,
                        captures: captures.clone(),
                    });
                });
                return;
            }
            CompiledRhoPathQueryAtom::Match(nested_query) => {
                visit_sequence_frames_with_prefix(
                    cursor,
                    nested_query.product_prefix(),
                    |frame| nested_query.matches_product_frame(frame),
                    |checkpoint, _| {
                        work.push(CandidateQueryState {
                            checkpoint,
                            atom_index: atom_index + 1,
                            captures: captures.clone(),
                        });
                    },
                );
                return;
            }
        }
    }

    if cursor.descend_to_existing(&[0, 0]) {
        visit_product_values(&mut cursor, |position| {
            result.push(position.index() + stored_offset);
        });
        cursor.ascend_bytes(2);
    }
    if query.accepts_remainder() {
        visit_sequence_frames(cursor, |checkpoint, _| {
            work.push(CandidateQueryState {
                checkpoint,
                atom_index,
                captures: captures.clone(),
            });
        });
    }
}

#[cfg(test)]
mod candidate_product_test_metrics {
    use std::cell::Cell;

    thread_local! {
        static CHECKPOINTS: Cell<usize> = const { Cell::new(0) };
        static VISITED_FRAMES: Cell<usize> = const { Cell::new(0) };
        static ROOT_ENDPOINT_RESTORATIONS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn reset() {
        CHECKPOINTS.set(0);
        VISITED_FRAMES.set(0);
        ROOT_ENDPOINT_RESTORATIONS.set(0);
    }

    pub(super) fn record_checkpoint() { CHECKPOINTS.set(CHECKPOINTS.get() + 1); }

    pub(super) fn record_frame() { VISITED_FRAMES.set(VISITED_FRAMES.get() + 1); }

    pub(super) fn record_root_endpoint_restoration() {
        ROOT_ENDPOINT_RESTORATIONS.set(ROOT_ENDPOINT_RESTORATIONS.get() + 1);
    }

    pub(super) fn root_endpoint_restorations() -> usize { ROOT_ENDPOINT_RESTORATIONS.get() }

    pub(super) fn take() -> (usize, usize) {
        let result = (CHECKPOINTS.get(), VISITED_FRAMES.get());
        CHECKPOINTS.set(0);
        VISITED_FRAMES.set(0);
        result
    }
}

impl<A: Clone + PartialEq> PartialEq for CandidatePool<A> {
    fn eq(&self, other: &Self) -> bool {
        self.authority == other.authority &&
            self.reserved == other.reserved &&
            self.ordered == other.ordered &&
            self.fresh == other.fresh &&
            self.fresh_available == other.fresh_available
    }
}

impl<A: Clone + Eq> Eq for CandidatePool<A> {}

impl<A: Clone> CandidatePool<A> {
    fn from_catalog(authority: Bag, catalog: &CandidateCatalog<A>) -> Self
    where A: ColdStoreDecode {
        Self {
            authority,
            reserved: Bag::new(),
            ordered: Arc::clone(&catalog.ordered),
            positions: Arc::clone(&catalog.positions),
            fresh: None,
            fresh_available: false,
            decode: decode_candidate::<A>,
        }
    }

    pub fn prepend_fresh(&mut self, datum: Datum<A>) -> bool {
        if !self.reserved.is_empty() || self.fresh.as_ref().is_some_and(|_| !self.fresh_available) {
            return false;
        }
        self.fresh = Some(datum);
        self.fresh_available = true;
        true
    }

    pub fn len(&self) -> usize { self.ordered.len() + usize::from(self.fresh.is_some()) }

    pub fn is_empty(&self) -> bool {
        !self.fresh_available &&
            self.authority.exact_cardinality() == self.reserved.exact_cardinality()
    }

    /// Authoritative multiplicities for the stored (non-fresh) candidates.
    ///
    /// Retained receive automata use this borrowed root for a read-only zipper
    /// product. The Bag remains owned by the pool; callers cannot mutate it or
    /// flatten it into a positional collection.
    pub(crate) fn stored_availability(&self) -> &Bag { &self.authority }

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
        let mut root = CandidateProductCursor::new(self.authority.as_pathmap(), &self.positions);
        if !root.descend_to_existing(&[1]) {
            return result;
        }
        let mut work = Vec::new();
        drive_candidate_query(
            root,
            0,
            PathMap::new(),
            query,
            stored_offset,
            &mut work,
            &mut result,
        );
        while let Some(state) = work.pop() {
            let cursor = state.checkpoint.cursor();
            drive_candidate_query(
                cursor,
                state.atom_index,
                state.captures,
                query,
                stored_offset,
                &mut work,
                &mut result,
            );
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
            return Self::reserve_stored(
                &self.authority,
                &mut self.reserved,
                entry.relative_path(),
            );
        }
        let Some(entry) = self.ordered.get(position) else {
            return false;
        };
        Self::reserve_stored(&self.authority, &mut self.reserved, entry.relative_path())
    }

    /// Release one successful speculative reservation.
    ///
    /// `false` identifies an invalid position or a double release instead of
    /// silently manufacturing multiplicity that is absent from the authority.
    pub fn release(&mut self, position: usize) -> bool {
        if self.fresh.is_some() {
            if position == 0 {
                if self.fresh_available {
                    return false;
                }
                self.fresh_available = true;
                return true;
            }
            let Some(entry) = self.ordered.get(position - 1) else {
                return false;
            };
            return Self::release_stored(&mut self.reserved, entry.relative_path());
        }
        let Some(entry) = self.ordered.get(position) else {
            return false;
        };
        Self::release_stored(&mut self.reserved, entry.relative_path())
    }

    /// Apply replay's COMM filter before candidate selection begins.
    ///
    /// Filtering an active selection would invalidate the subset relation
    /// between `reserved` and `authority`, so make that misuse impossible to
    /// overlook rather than trying to repair it after the fact.
    pub fn retain_before_selection(
        &mut self,
        mut predicate: impl FnMut(&Datum<A>) -> bool,
    ) -> bool {
        if !self.reserved.is_empty() || self.fresh.as_ref().is_some_and(|_| !self.fresh_available) {
            return false;
        }
        if self.fresh.as_ref().is_some_and(|datum| !predicate(datum)) {
            self.fresh = None;
            self.fresh_available = false;
        }
        for entry in self.ordered.iter() {
            if !predicate(entry.datum(self.decode)) {
                self.authority.set_count(entry.relative_path(), None);
            }
        }
        true
    }

    pub(crate) fn is_available(&self, position: usize) -> bool {
        if self.fresh.is_some() {
            if position == 0 {
                return self.fresh_available;
            }
            return self
                .ordered
                .get(position - 1)
                .is_some_and(|entry| self.unreserved_count(entry) != 0);
        }
        self.ordered
            .get(position)
            .is_some_and(|entry| self.unreserved_count(entry) != 0)
    }

    fn unreserved_count(&self, entry: &CandidatePoolEntry<A>) -> u64 {
        self.authority
            .count(entry.relative_path())
            .checked_sub(self.reserved.count(entry.relative_path()))
            .expect("candidate reservations must remain a subset of authority")
    }

    fn reserve_stored(authority: &Bag, reservations: &mut Bag, path: &[u8]) -> bool {
        let authoritative = authority.count(path);
        let reserved = reservations.count(path);
        if reserved >= authoritative {
            return false;
        }
        let next = Multiplicity::new(reserved + 1)
            .expect("a reservation bounded by authoritative multiplicity cannot overflow");
        reservations.set_count(path, Some(next));
        true
    }

    fn release_stored(reservations: &mut Bag, path: &[u8]) -> bool {
        let reserved = reservations.count(path);
        if reserved == 0 {
            return false;
        }
        let next = (reserved > 1).then(|| {
            Multiplicity::new(reserved - 1)
                .expect("a positive reservation predecessor must be representable")
        });
        reservations.set_count(path, next);
        true
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

    pub fn cardinality(&self) -> u64 { self.bag.cardinality() }

    pub fn is_empty(&self) -> bool { self.bag.is_empty() }

    pub fn clear_channel<C: RhoPathEncode>(&mut self, channel: &C) -> (usize, u64) {
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
        A: Clone + ColdStoreDecode,
    {
        let prefix = RhoPathV1::datum_prefix(channel);
        self.ordered_encoded_channel_entries(&prefix)?
            .into_iter()
            .map(|entry| {
                let datum = decode_datum(&entry.path[entry.identity_offset..])?;
                Ok(IndexedDatum {
                    path: entry.path,
                    datum,
                    multiplicity: entry.multiplicity,
                })
            })
            .collect()
    }

    pub fn candidate_pool<C, A>(&self, channel: &C) -> Result<CandidatePool<A>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
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
            decorated.push((datum_identity_digest(view.datum_identity()), EncodedIndexedDatum {
                path: path.to_vec(),
                identity_offset,
                multiplicity,
            }));
            Ok::<(), DataIndexError>(())
        })?;
        sort_encoded_candidates(&mut decorated);
        validate_unique_candidate_identities(&decorated)?;
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

    /// Encode the legacy cold-store `Vec<Vec<u8>>` body directly from the
    /// canonical datum identities resident in the PathMap.
    ///
    /// The RhoPath structural dimension is intentionally not the cold-store
    /// ordering authority: legacy leaves sort by each complete datum identity.
    /// This method therefore retains one lightweight identity descriptor per
    /// distinct endpoint, sorts those exact bytes, and expands multiplicity
    /// directly into the final bincode image. It never constructs `Datum<A>`
    /// values or a `Vec<Vec<u8>>` shadow.
    fn encode_canonical_datums_at_prefix(
        &self,
        prefix: &[u8],
    ) -> Result<(Vec<u8>, u64), DataIndexError> {
        let mut identities = Vec::new();
        let mut descriptors = Vec::new();
        self.bag
            .try_visit_relative_prefix(prefix, |path, multiplicity| {
                let identity = RhoPathV1::relative_datum_identity(path)?;
                let start = identities.len();
                let identity_capacity = start
                    .checked_add(identity.len())
                    .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))?;
                identities
                    .try_reserve(identity.len())
                    .map_err(|_| DataIndexError::AllocationFailure(identity_capacity))?;
                let descriptor_capacity = descriptors
                    .len()
                    .checked_add(1)
                    .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))?;
                descriptors
                    .try_reserve(1)
                    .map_err(|_| DataIndexError::AllocationFailure(descriptor_capacity))?;
                identities.extend_from_slice(identity);
                descriptors.push(DatumIdentityDescriptor {
                    start,
                    len: identity.len(),
                    multiplicity,
                });
                Ok::<(), DataIndexError>(())
            })?;
        descriptors.sort_unstable_by(|left, right| {
            left.identity(&identities).cmp(right.identity(&identities))
        });

        let mut datum_count = 0_u128;
        let mut encoded_len = u128::from(u64::BITS / 8);
        for descriptor in &descriptors {
            let multiplicity = u128::from(descriptor.multiplicity.get());
            datum_count = datum_count
                .checked_add(multiplicity)
                .ok_or(DataIndexError::EncodedCardinalityOverflow(u128::MAX))?;
            let framed_identity_len = u128::from(u64::BITS / 8)
                .checked_add(descriptor.len as u128)
                .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))?;
            encoded_len = encoded_len
                .checked_add(
                    multiplicity
                        .checked_mul(framed_identity_len)
                        .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))?,
                )
                .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))?;
        }

        let datum_count_u64 = u64::try_from(datum_count)
            .map_err(|_| DataIndexError::EncodedCardinalityOverflow(datum_count))?;
        let capacity = usize::try_from(encoded_len)
            .map_err(|_| DataIndexError::EncodedLengthOverflow(encoded_len))?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(capacity)
            .map_err(|_| DataIndexError::AllocationFailure(capacity))?;
        output.extend_from_slice(&datum_count_u64.to_le_bytes());
        for descriptor in descriptors {
            let identity = descriptor.identity(&identities);
            let identity_len = u64::try_from(identity.len())
                .map_err(|_| DataIndexError::EncodedLengthOverflow(identity.len() as u128))?;
            for _ in 0..descriptor.multiplicity.get() {
                output.extend_from_slice(&identity_len.to_le_bytes());
                output.extend_from_slice(identity);
            }
        }
        debug_assert_eq!(output.len(), capacity);
        Ok((output, datum_count_u64))
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

    pub fn cardinality(&self) -> u64 { self.index.cardinality() }

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
        #[cfg(feature = "accepted-match-transition-instrumentation")]
        crate::rspace::hot_store::accepted_match_transition_experiment::record_pathmap_root_publication();
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
        #[cfg(feature = "accepted-match-transition-instrumentation")]
        crate::rspace::hot_store::accepted_match_transition_experiment::record_pathmap_root_publication();
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
        A: Clone + ColdStoreDecode,
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
        A: Clone + ColdStoreDecode,
    {
        self.channel_entries(channel)?
            .map(expand_entries)
            .transpose()
    }

    pub fn channel_pool(&self, channel: &C) -> Result<Option<CandidatePool<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
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
        A: Clone + ColdStoreDecode,
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
        A: Clone + ColdStoreDecode,
    {
        let mut records = Vec::new();
        records
            .try_reserve_exact(self.loaded_channel_count)
            .map_err(|_| DataIndexError::AllocationFailure(self.loaded_channel_count))?;
        for (path, _) in self.loaded_channels.iter() {
            let view = RhoPathV1::parse_datum_prefix(&path)?;
            let channel = C::cold_decode(&view.channel_identity())?;
            let datums = self.channel_data(&channel)?.unwrap_or_default();
            records.push((channel, datums));
        }
        Ok(records)
    }

    /// Return checkpoint-ready DATA records without decoding resident datum
    /// identities back into typed values.
    pub(crate) fn loaded_canonical_records(
        &self,
    ) -> Result<Vec<CanonicalDataRecord>, DataIndexError> {
        let mut records = Vec::new();
        records
            .try_reserve_exact(self.loaded_channel_count)
            .map_err(|_| DataIndexError::AllocationFailure(self.loaded_channel_count))?;
        for (prefix, _) in self.loaded_channels.iter() {
            let view = RhoPathV1::parse_datum_prefix(&prefix)?;
            let channel_hash = Blake2b256Hash::new(&view.channel_identity());
            let (encoded_datums, datum_count) =
                self.index.encode_canonical_datums_at_prefix(&prefix)?;
            records.push(CanonicalDataRecord {
                channel_hash,
                encoded_datums,
                datum_count,
            });
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

#[cfg(test)]
mod candidate_identity_order_tests {
    use super::*;

    /// Compile-time witness that byte-native catalog/read operations require
    /// decoding only. Reintroducing the retired decoded-hash route would make
    /// this generic body demand `A: Serialize` and fail to type-check.
    #[allow(dead_code)]
    fn decode_only_catalog_api_witness<C, A>(
        index: &RSpaceDataIndex,
        store: &mut RSpaceDataStore<C, A>,
        channel: &C,
    ) where
        C: Clone + ColdStoreDecode + RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        let _: Result<Vec<IndexedDatum<A>>, DataIndexError> =
            index.ordered_channel_entries(channel);
        let _: Result<CandidatePool<A>, DataIndexError> = index.candidate_pool(channel);
        let _: Result<Option<Vec<IndexedDatum<A>>>, DataIndexError> =
            store.channel_entries(channel);
        let _: Result<Option<Vec<Datum<A>>>, DataIndexError> = store.channel_data(channel);
        let _: Result<Option<CandidatePool<A>>, DataIndexError> = store.channel_pool(channel);
        let _: Result<(), DataIndexError> = store.remove_index(channel, 0);
        let _: Result<Vec<(C, Vec<Datum<A>>)>, DataIndexError> = store.loaded_records();
    }

    fn encoded(path_prefix: &[u8], identity: &[u8]) -> EncodedIndexedDatum {
        let mut path = path_prefix.to_vec();
        let identity_offset = path.len();
        path.extend_from_slice(identity);
        EncodedIndexedDatum {
            path,
            identity_offset,
            multiplicity: Multiplicity::ONE,
        }
    }

    #[test]
    fn a_hash_collision_is_ordered_by_identity_not_the_derived_rho_path() {
        let collision = Blake2b256Hash::from_bytes(vec![0xAB; 32]);
        let mut entries = vec![
            // Full-path order says `a...z` first; identity order says `a`
            // first. This deliberately opposes the removed RMP tie-break.
            (collision.clone(), encoded(b"a-rmp", b"z-identity")),
            (collision, encoded(b"z-rmp", b"a-identity")),
        ];

        assert!(
            entries[0].1.path < entries[1].1.path,
            "legacy full-path order must oppose identity order"
        );
        sort_encoded_candidates(&mut entries);

        assert_eq!(&entries[0].1.path[entries[0].1.identity_offset..], b"a-identity");
        assert_eq!(&entries[1].1.path[entries[1].1.identity_offset..], b"z-identity");
    }

    #[test]
    fn one_identity_under_two_derived_paths_is_rejected() {
        let identity = b"same-identity";
        let hash = Blake2b256Hash::new(identity);
        let mut entries =
            vec![(hash.clone(), encoded(b"a-rmp", identity)), (hash, encoded(b"z-rmp", identity))];

        sort_encoded_candidates(&mut entries);

        assert_eq!(
            validate_unique_candidate_identities(&entries),
            Err(DataIndexError::DuplicateDatumIdentity)
        );
    }
}

#[cfg(test)]
#[path = "../../tests/data_index/candidate_product_checkpoint.rs"]
mod candidate_product_checkpoint_tests;

#[cfg(test)]
#[path = "../../tests/data_index/candidate_pool_reservation.rs"]
mod candidate_pool_reservation_tests;

#[cfg(test)]
#[path = "../../tests/data_index/digest_collision_replay.rs"]
mod digest_collision_replay_tests;

#[cfg(feature = "candidate-checkpoint-experiment")]
#[doc(hidden)]
#[path = "../../tests/data_index/candidate_checkpoint_experiment.rs"]
pub mod candidate_checkpoint_experiment;

#[cfg(feature = "canonical-egress-experiment")]
#[doc(hidden)]
#[path = "../../tests/data_index/canonical_egress_experiment.rs"]
pub mod canonical_egress_experiment;
