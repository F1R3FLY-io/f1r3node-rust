use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock, RwLock};

use pathmap::utils::{ByteMask, ByteMaskIter};
use serde::Serialize;

use super::bag::{Bag, BagDelta, BagError, BagReadCursor, Multiplicity};
use super::guarded_pathmap::{DisposeValue, GuardedPathMap, GuardedReadCursor, TeardownBarrier};
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::history::cold_store::{PERSISTED_DATA_HEADER_LEN, PersistedDataKind};
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
    LoadedChannelCountOverflow,
    AggregateDistinctCountOverflow,
    AggregateCardinalityOverflow { current: u128, added: u128 },
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
            Self::LoadedChannelCountOverflow => {
                f.write_str("loaded channel count exceeds usize::MAX")
            }
            Self::AggregateDistinctCountOverflow => {
                f.write_str("RSpace distinct datum count exceeds usize::MAX")
            }
            Self::AggregateCardinalityOverflow { current, added } => {
                write!(f, "RSpace aggregate cardinality overflow: {current} + {added}")
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
    pub encoded_persisted_data: Vec<u8>,
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

#[derive(Debug, Clone, Copy, Default)]
struct CandidatePositionDisposer;

impl DisposeValue<CandidatePosition> for CandidatePositionDisposer {
    #[inline]
    fn dispose(&self, _value: CandidatePosition) {}
}

#[derive(Debug, Clone, Copy, Default)]
struct CandidatePositionBarrier;

impl TeardownBarrier<CandidatePosition> for CandidatePositionBarrier {
    #[inline]
    fn make_barrier() -> CandidatePosition { CandidatePosition(0) }
}

type CandidatePositionReadCursor<'map, 'path> = GuardedReadCursor<
    'map,
    'path,
    CandidatePosition,
    CandidatePositionDisposer,
    CandidatePositionBarrier,
>;

/// Stack-safely owned, copy-on-write trie from canonical candidate suffixes
/// to their fixed-width catalog positions.
///
/// This facade deliberately exposes only borrowed cursors. Candidate matching
/// therefore cannot detach an owned subtree while suspending a continuation.
#[derive(Clone)]
struct CandidatePositionMap {
    entries: GuardedPathMap<CandidatePosition, CandidatePositionDisposer, CandidatePositionBarrier>,
}

impl Default for CandidatePositionMap {
    fn default() -> Self {
        Self {
            entries: GuardedPathMap::new_with_barrier(CandidatePositionDisposer),
        }
    }
}

impl CandidatePositionMap {
    fn new() -> Self { Self::default() }

    /// Insert a position, returning whether an existing endpoint was replaced.
    fn insert(&mut self, path: &[u8], position: CandidatePosition) -> bool {
        self.entries.set_val_at(path, position)
    }

    fn contains(&self, path: &[u8]) -> bool { self.entries.contains(path) }

    fn with_read_cursor<R>(
        &self,
        operation: impl for<'map> FnOnce(CandidatePositionReadCursor<'map, 'static>) -> R,
    ) -> R {
        self.entries.with_read_cursor(operation)
    }

    fn with_read_cursor_at<'path, R>(
        &self,
        path: &'path [u8],
        operation: impl for<'map> FnOnce(CandidatePositionReadCursor<'map, 'path>) -> R,
    ) -> R {
        self.entries.with_read_cursor_at(path, operation)
    }
}

struct DebugCandidatePositions<'a>(&'a CandidatePositionMap);

impl fmt::Debug for DebugCandidatePositions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_map()
            .entries(self.0.entries.iter())
            .finish()
    }
}

impl fmt::Debug for CandidatePositionMap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("CandidatePositionMap")
            .field(&DebugCandidatePositions(self))
            .finish()
    }
}

#[derive(Debug, Clone)]
struct CandidateCatalog<A: Clone> {
    /// Copy-on-write clone of the selected channel's authoritative Bag.  A
    /// catalog build and every incremental catalog transition preserve the
    /// inner guarded root, so candidate matching never rebuilds a second
    /// channel-relative trie merely to obtain an owned lifetime.
    availability: Bag,
    ordered: Arc<[CandidatePoolEntry<A>]>,
    positions: CandidatePositionMap,
    active_distinct_len: usize,
    cardinality: u128,
}

impl<A: Clone> CandidateCatalog<A> {
    fn from_encoded_entries(
        availability: Bag,
        prefix_len: usize,
        entries: Vec<EncodedIndexedDatum>,
    ) -> Self
    where
        A: ColdStoreDecode,
    {
        let active_distinct_len = entries.len();
        let cardinality = entries
            .iter()
            .map(|entry| u128::from(entry.multiplicity.get()))
            .sum();
        let mut positions = CandidatePositionMap::new();
        let ordered = entries
            .into_iter()
            .enumerate()
            .map(|(position, entry)| {
                let relative_path = &entry.path[prefix_len..];
                let replaced =
                    positions.insert(relative_path, CandidatePosition::from_index(position));
                debug_assert!(!replaced, "candidate paths are distinct");
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
            availability,
            ordered,
            positions,
            active_distinct_len,
            cardinality,
        }
    }

    fn needs_refresh(&self) -> bool { self.active_distinct_len != self.ordered.len() }

    fn record_transition_metadata(
        &mut self,
        relative_path: &[u8],
        previous: Option<Multiplicity>,
        current: Option<Multiplicity>,
    ) -> bool {
        if previous.is_none() && current.is_some() && !self.positions.contains(relative_path) {
            return false;
        }
        let previous_count = previous.map_or(0, Multiplicity::get);
        let current_count = current.map_or(0, Multiplicity::get);
        self.cardinality =
            self.cardinality - u128::from(previous_count) + u128::from(current_count);
        match (previous, current) {
            (None, Some(_)) => {
                self.active_distinct_len += 1;
            }
            (Some(_), None) => self.active_distinct_len -= 1,
            _ => {}
        }
        true
    }

    fn record_insert_metadata(&mut self, relative_path: &[u8], multiplicity: Multiplicity) -> bool {
        let previous = (multiplicity != Multiplicity::ONE)
            .then(|| Multiplicity::new(multiplicity.get() - 1).expect("positive predecessor"));
        self.record_transition_metadata(relative_path, previous, Some(multiplicity))
    }

    fn record_remove_metadata(&mut self, relative_path: &[u8], previous: Multiplicity) {
        let current = (previous != Multiplicity::ONE)
            .then(|| Multiplicity::new(previous.get() - 1).expect("positive remainder"));
        let retained = self.record_transition_metadata(relative_path, Some(previous), current);
        debug_assert!(retained, "removal cannot introduce an unknown catalog identity");
    }
}

#[derive(Debug, Clone)]
pub struct CandidatePool<A: Clone> {
    /// Immutable channel-relative multiplicities owned by the candidate
    /// catalog. Candidate search never mutates this PathMap-backed Bag.
    authority: Bag,
    /// Replay exclusions are a pool-local overlay.  The immutable catalog
    /// authority is never rewritten merely to filter one speculative search.
    excluded: Bag,
    /// Speculative, non-persistent demand placed against `authority`.
    /// Repeated-channel joins share one pool, so multiplicity is reserved once
    /// per selected occurrence without copying or rewriting the authority trie.
    reserved: Bag,
    ordered: Arc<[CandidatePoolEntry<A>]>,
    positions: CandidatePositionMap,
    fresh: Option<Datum<A>>,
    fresh_available: bool,
    decode: fn(&[u8]) -> Datum<A>,
}

#[cfg_attr(any(test, feature = "candidate-continuation-experiment"), derive(Clone))]
struct CandidateProductCursor<'availability, 'positions> {
    availability: BagReadCursor<'availability, 'static>,
    positions: CandidatePositionReadCursor<'positions, 'static>,
}

impl<'availability, 'positions> CandidateProductCursor<'availability, 'positions> {
    fn new(
        availability: BagReadCursor<'availability, 'static>,
        positions: CandidatePositionReadCursor<'positions, 'static>,
    ) -> Self {
        Self {
            availability,
            positions,
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
}

/// Where one continuation node obtains the frame appended to its parent.
/// Exact query bytes and repeated-capture bytes remain borrowed; only a frame
/// discovered by a wildcard/capture scanner is owned, exactly once.
enum CandidateFrameSource {
    Root,
    ExactAtom(usize),
    Owned(Box<[u8]>),
    Captured(usize),
}

/// Flat, append-only continuation node. Parent and capture links are integer
/// indices, so neither clone nor drop traverses a recursively owned chain.
struct CandidateContinuationNode {
    parent: Option<usize>,
    frame_depth: u32,
    byte_depth: usize,
    source: CandidateFrameSource,
    capture_slot: Option<u32>,
}

struct CandidateContinuationArena {
    nodes: Vec<CandidateContinuationNode>,
}

impl CandidateContinuationArena {
    fn at_structure_root(expected_depth: usize) -> Self {
        let mut nodes = Vec::with_capacity(expected_depth.saturating_add(1));
        nodes.push(CandidateContinuationNode {
            parent: None,
            frame_depth: 0,
            byte_depth: 1,
            source: CandidateFrameSource::Root,
            capture_slot: None,
        });
        Self { nodes }
    }

    fn node(&self, index: usize) -> &CandidateContinuationNode { &self.nodes[index] }

    fn frame<'arena, 'query>(
        &'arena self,
        index: usize,
        query: &'query RhoPathQuery,
    ) -> &'arena [u8]
    where
        'query: 'arena,
    {
        match &self.node(index).source {
            CandidateFrameSource::Root => &[],
            CandidateFrameSource::ExactAtom(atom_index) => match &query.atoms()[*atom_index] {
                CompiledRhoPathQueryAtom::Exact { stored_frame, .. } => stored_frame,
                _ => unreachable!("an exact continuation references an exact query atom"),
            },
            CandidateFrameSource::Owned(frame) => frame,
            CandidateFrameSource::Captured(binding) => match &self.node(*binding).source {
                CandidateFrameSource::Owned(frame) => frame,
                _ => unreachable!("a capture slot references its first owned frame"),
            },
        }
    }

    fn push(
        &mut self,
        parent: usize,
        frame_len: usize,
        source: CandidateFrameSource,
        capture_slot: Option<u32>,
    ) -> usize {
        let parent_node = self.node(parent);
        let frame_depth = parent_node
            .frame_depth
            .checked_add(1)
            .expect("query frame depth fits in u32");
        let byte_depth = parent_node
            .byte_depth
            .checked_add(frame_len)
            .expect("candidate path depth fits in usize");
        let index = self.nodes.len();
        self.nodes.push(CandidateContinuationNode {
            parent: Some(parent),
            frame_depth,
            byte_depth,
            source,
            capture_slot,
        });
        #[cfg(test)]
        candidate_product_test_metrics::record_continuation();
        index
    }

    fn push_exact(&mut self, parent: usize, atom_index: usize, frame_len: usize) -> usize {
        self.push(parent, frame_len, CandidateFrameSource::ExactAtom(atom_index), None)
    }

    fn push_owned(&mut self, parent: usize, frame: &[u8], capture_slot: Option<u32>) -> usize {
        self.push(parent, frame.len(), CandidateFrameSource::Owned(frame.into()), capture_slot)
    }

    fn push_captured(&mut self, parent: usize, binding: usize) -> usize {
        let frame_len = match &self.node(binding).source {
            CandidateFrameSource::Owned(frame) => frame.len(),
            _ => unreachable!("a capture slot references its first owned frame"),
        };
        self.push(parent, frame_len, CandidateFrameSource::Captured(binding), None)
    }
}

#[derive(Debug, Clone, Copy)]
struct CandidateQueryState {
    continuation: usize,
    atom_index: usize,
}

#[derive(Default)]
struct CandidateFrameScanScratch {
    levels: Vec<(ByteMaskIter, StoredSequenceFrameScanner)>,
    frame: Vec<u8>,
    product_levels: Vec<ByteMaskIter>,
}

fn visit_sequence_frames(
    cursor: &mut CandidateProductCursor<'_, '_>,
    continuation: usize,
    arena: &mut CandidateContinuationArena,
    capture_slot: Option<u32>,
    scratch: &mut CandidateFrameScanScratch,
    mut visitor: impl FnMut(usize, &[u8]),
) {
    visit_sequence_frames_with_prefix(
        cursor,
        continuation,
        arena,
        capture_slot,
        scratch,
        &[],
        |_| true,
        &mut visitor,
    );
}

fn visit_sequence_frames_with_prefix(
    cursor: &mut CandidateProductCursor<'_, '_>,
    continuation: usize,
    arena: &mut CandidateContinuationArena,
    capture_slot: Option<u32>,
    scratch: &mut CandidateFrameScanScratch,
    prefix: &[u8],
    mut accepts: impl FnMut(&[u8]) -> bool,
    mut visitor: impl FnMut(usize, &[u8]),
) {
    if !cursor.descend_to_existing(prefix) {
        return;
    }
    scratch.levels.clear();
    scratch.frame.clear();
    scratch.frame.extend_from_slice(prefix);
    let mut scanner = StoredSequenceFrameScanner::default();
    for byte in prefix {
        if scanner.feed(*byte) != StoredSequenceFrameStep::NeedMore {
            cursor.ascend_bytes(prefix.len());
            scratch.frame.clear();
            return;
        }
    }
    scratch.levels.push((cursor.child_mask().iter(), scanner));
    let base_len = scratch.frame.len();
    while let Some((children, scanner_base)) = scratch.levels.last_mut() {
        match children.next() {
            Some(byte) => {
                let mut scanner = *scanner_base;
                match scanner.feed(byte) {
                    StoredSequenceFrameStep::NeedMore => {
                        cursor.descend_to_byte(byte);
                        scratch.frame.push(byte);
                        scratch.levels.push((cursor.child_mask().iter(), scanner));
                    }
                    StoredSequenceFrameStep::Complete => {
                        cursor.descend_to_byte(byte);
                        scratch.frame.push(byte);
                        #[cfg(test)]
                        candidate_product_test_metrics::record_frame();
                        if accepts(&scratch.frame) {
                            let next = arena.push_owned(continuation, &scratch.frame, capture_slot);
                            visitor(next, &scratch.frame);
                        }
                        scratch.frame.pop();
                        cursor.ascend_byte();
                    }
                    StoredSequenceFrameStep::Invalid => {}
                }
            }
            None => {
                scratch.levels.pop();
                if scratch.frame.len() > base_len {
                    scratch.frame.pop();
                    cursor.ascend_byte();
                }
            }
        }
    }
    cursor.ascend_bytes(prefix.len());
    scratch.frame.clear();
}

#[cfg(any(test, feature = "candidate-continuation-experiment"))]
fn visit_product_values(
    cursor: &mut CandidateProductCursor<'_, '_>,
    mut visitor: impl FnMut(CandidatePosition),
) {
    let mut levels = Vec::new();
    visit_product_values_with_scratch(cursor, &mut levels, &mut visitor);
}

fn visit_product_values_with_scratch(
    cursor: &mut CandidateProductCursor<'_, '_>,
    levels: &mut Vec<ByteMaskIter>,
    mut visitor: impl FnMut(CandidatePosition),
) {
    if let Some(position) = cursor.position() {
        visitor(position);
    }
    levels.clear();
    levels.push(cursor.child_mask().iter());
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

fn undo_continuation_node(
    cursor: &mut CandidateProductCursor<'_, '_>,
    current: usize,
    arena: &CandidateContinuationArena,
    capture_slots: &mut [Option<usize>],
) -> usize {
    let node = arena.node(current);
    let parent = node
        .parent
        .expect("the structure-root continuation has no parent");
    if let Some(slot) = node.capture_slot {
        let binding = &mut capture_slots[slot as usize];
        debug_assert_eq!(*binding, Some(current));
        *binding = None;
        #[cfg(test)]
        candidate_product_test_metrics::record_capture_edit();
    }
    let bytes = node.byte_depth - arena.node(parent).byte_depth;
    cursor.ascend_bytes(bytes);
    #[cfg(test)]
    candidate_product_test_metrics::record_navigation(bytes);
    parent
}

fn redo_continuation_node(
    cursor: &mut CandidateProductCursor<'_, '_>,
    target: usize,
    arena: &CandidateContinuationArena,
    query: &RhoPathQuery,
    capture_slots: &mut [Option<usize>],
) -> bool {
    let frame = arena.frame(target, query);
    if !cursor.descend_to_existing(frame) {
        return false;
    }
    #[cfg(test)]
    candidate_product_test_metrics::record_navigation(frame.len());
    if let Some(slot) = arena.node(target).capture_slot {
        let binding = &mut capture_slots[slot as usize];
        debug_assert!(binding.is_none());
        *binding = Some(target);
        #[cfg(test)]
        candidate_product_test_metrics::record_capture_edit();
    }
    true
}

/// Move the sole live product-cursor pair between two arena continuations by
/// their integer-index LCA. Source capture edits are undone before target
/// edits are redone, and no shared prefix bytes are replayed.
fn rebase_candidate_cursor(
    cursor: &mut CandidateProductCursor<'_, '_>,
    current: &mut usize,
    target: usize,
    arena: &CandidateContinuationArena,
    query: &RhoPathQuery,
    capture_slots: &mut [Option<usize>],
    target_suffix: &mut Vec<usize>,
) -> bool {
    target_suffix.clear();
    let mut source = *current;
    let mut destination = target;

    while arena.node(source).frame_depth > arena.node(destination).frame_depth {
        source = undo_continuation_node(cursor, source, arena, capture_slots);
    }
    while arena.node(destination).frame_depth > arena.node(source).frame_depth {
        target_suffix.push(destination);
        destination = arena
            .node(destination)
            .parent
            .expect("a continuation above the root has a parent");
    }
    while source != destination {
        source = undo_continuation_node(cursor, source, arena, capture_slots);
        target_suffix.push(destination);
        destination = arena
            .node(destination)
            .parent
            .expect("a continuation above the root has a parent");
    }

    *current = source;
    for next in target_suffix.iter().rev().copied() {
        if !redo_continuation_node(cursor, next, arena, query, capture_slots) {
            return false;
        }
        *current = next;
    }
    true
}

fn drive_candidate_query(
    cursor: &mut CandidateProductCursor<'_, '_>,
    continuation: &mut usize,
    mut atom_index: usize,
    query: &RhoPathQuery,
    stored_offset: usize,
    arena: &mut CandidateContinuationArena,
    capture_slots: &[Option<usize>],
    scanner_scratch: &mut CandidateFrameScanScratch,
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
                *continuation = arena.push_exact(*continuation, atom_index, stored_frame.len());
                atom_index += 1;
            }
            CompiledRhoPathQueryAtom::Capture { slot } => {
                if let Some(binding) = capture_slots[*slot as usize] {
                    let frame = arena.frame(binding, query);
                    if !cursor.descend_to_existing(frame) {
                        return;
                    }
                    *continuation = arena.push_captured(*continuation, binding);
                    atom_index += 1;
                    continue;
                }
                visit_sequence_frames(
                    cursor,
                    *continuation,
                    arena,
                    Some(*slot),
                    scanner_scratch,
                    |next, _| {
                        work.push(CandidateQueryState {
                            continuation: next,
                            atom_index: atom_index + 1,
                        });
                    },
                );
                return;
            }
            CompiledRhoPathQueryAtom::Wildcard => {
                visit_sequence_frames(
                    cursor,
                    *continuation,
                    arena,
                    None,
                    scanner_scratch,
                    |next, _| {
                        work.push(CandidateQueryState {
                            continuation: next,
                            atom_index: atom_index + 1,
                        });
                    },
                );
                return;
            }
            CompiledRhoPathQueryAtom::Match(nested_query) => {
                visit_sequence_frames_with_prefix(
                    cursor,
                    *continuation,
                    arena,
                    None,
                    scanner_scratch,
                    nested_query.product_prefix(),
                    |frame| nested_query.matches_product_frame(frame),
                    |next, _| {
                        work.push(CandidateQueryState {
                            continuation: next,
                            atom_index: atom_index + 1,
                        });
                    },
                );
                return;
            }
        }
    }

    if cursor.descend_to_existing(&[0, 0]) {
        visit_product_values_with_scratch(
            cursor,
            &mut scanner_scratch.product_levels,
            |position| {
                result.push(position.index() + stored_offset);
            },
        );
        cursor.ascend_bytes(2);
    }
    if query.accepts_remainder() {
        visit_sequence_frames(cursor, *continuation, arena, None, scanner_scratch, |next, _| {
            work.push(CandidateQueryState {
                continuation: next,
                atom_index,
            });
        });
    }
}

#[cfg(test)]
mod candidate_product_test_metrics {
    use std::cell::Cell;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(super) struct Metrics {
        pub continuations: usize,
        pub visited_frames: usize,
        pub navigated_bytes: usize,
        pub capture_edits: usize,
    }

    thread_local! {
        static CONTINUATIONS: Cell<usize> = const { Cell::new(0) };
        static VISITED_FRAMES: Cell<usize> = const { Cell::new(0) };
        static NAVIGATED_BYTES: Cell<usize> = const { Cell::new(0) };
        static CAPTURE_EDITS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn reset() {
        CONTINUATIONS.set(0);
        VISITED_FRAMES.set(0);
        NAVIGATED_BYTES.set(0);
        CAPTURE_EDITS.set(0);
    }

    pub(super) fn record_continuation() { CONTINUATIONS.set(CONTINUATIONS.get() + 1); }

    pub(super) fn record_frame() { VISITED_FRAMES.set(VISITED_FRAMES.get() + 1); }

    pub(super) fn record_navigation(bytes: usize) {
        NAVIGATED_BYTES.set(NAVIGATED_BYTES.get() + bytes);
    }

    pub(super) fn record_capture_edit() { CAPTURE_EDITS.set(CAPTURE_EDITS.get() + 1); }

    pub(super) fn take() -> Metrics {
        let result = Metrics {
            continuations: CONTINUATIONS.get(),
            visited_frames: VISITED_FRAMES.get(),
            navigated_bytes: NAVIGATED_BYTES.get(),
            capture_edits: CAPTURE_EDITS.get(),
        };
        CONTINUATIONS.set(0);
        VISITED_FRAMES.set(0);
        NAVIGATED_BYTES.set(0);
        CAPTURE_EDITS.set(0);
        result
    }
}

impl<A: Clone + PartialEq> PartialEq for CandidatePool<A> {
    fn eq(&self, other: &Self) -> bool {
        self.authority == other.authority &&
            self.excluded == other.excluded &&
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
            excluded: Bag::new(),
            reserved: Bag::new(),
            ordered: Arc::clone(&catalog.ordered),
            positions: catalog.positions.clone(),
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
            self.authority.exact_cardinality() ==
                self.reserved.exact_cardinality() + self.excluded.exact_cardinality()
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
        self.positions
            .with_read_cursor_at(&encoded_prefix, |mut cursor| {
                if let Some(position) = cursor.get_val() {
                    result.push(position.index() + stored_offset);
                }
                while let Some(position) = cursor.to_next_get_val() {
                    result.push(position.index() + stored_offset);
                }
            });
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
        self.authority.with_read_cursor(|availability| {
            self.positions.with_read_cursor(|positions| {
                let mut cursor = CandidateProductCursor::new(availability, positions);
                if !cursor.descend_to_existing(&[1]) {
                    return;
                }

                let expected_depth = query.atoms().len();
                let mut arena = CandidateContinuationArena::at_structure_root(expected_depth);
                let mut capture_slots = vec![None; query.capture_slot_count() as usize];
                let mut work = Vec::with_capacity(expected_depth.saturating_add(1));
                work.push(CandidateQueryState {
                    continuation: 0,
                    atom_index: 0,
                });
                let mut current = 0usize;
                let mut target_suffix = Vec::with_capacity(expected_depth);
                let mut scanner_scratch = CandidateFrameScanScratch::default();

                while let Some(state) = work.pop() {
                    if !rebase_candidate_cursor(
                        &mut cursor,
                        &mut current,
                        state.continuation,
                        &arena,
                        query,
                        &mut capture_slots,
                        &mut target_suffix,
                    ) {
                        continue;
                    }
                    drive_candidate_query(
                        &mut cursor,
                        &mut current,
                        state.atom_index,
                        query,
                        stored_offset,
                        &mut arena,
                        &capture_slots,
                        &mut scanner_scratch,
                        &mut work,
                        &mut result,
                    );
                }
            });
        });
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
                &self.excluded,
                &mut self.reserved,
                entry.relative_path(),
            );
        }
        let Some(entry) = self.ordered.get(position) else {
            return false;
        };
        Self::reserve_stored(
            &self.authority,
            &self.excluded,
            &mut self.reserved,
            entry.relative_path(),
        )
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
                let multiplicity = self.authority.count(entry.relative_path());
                let multiplicity = Multiplicity::new(multiplicity)
                    .expect("every catalog entry has positive authoritative multiplicity");
                self.excluded
                    .set_count(entry.relative_path(), Some(multiplicity));
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
            .checked_sub(self.excluded.count(entry.relative_path()))
            .expect("candidate exclusions must remain a subset of authority")
            .checked_sub(self.reserved.count(entry.relative_path()))
            .expect("candidate reservations must remain a subset of authority")
    }

    fn reserve_stored(
        authority: &Bag,
        exclusions: &Bag,
        reservations: &mut Bag,
        path: &[u8],
    ) -> bool {
        let authoritative = authority
            .count(path)
            .checked_sub(exclusions.count(path))
            .expect("candidate exclusions must remain a subset of authority");
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

#[derive(Clone)]
enum ChannelSlot {
    Real(Bag),
    SyntheticBarrier,
}

impl ChannelSlot {
    fn real(&self) -> &Bag {
        match self {
            Self::Real(bag) => bag,
            Self::SyntheticBarrier => {
                unreachable!("a synthetic outer barrier is never observable")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ChannelSlotDisposer;

impl DisposeValue<ChannelSlot> for ChannelSlotDisposer {
    fn dispose(&self, value: ChannelSlot) {
        match value {
            ChannelSlot::Real(bag) => drop(bag),
            ChannelSlot::SyntheticBarrier => std::process::abort(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ChannelSlotBarrier;

impl TeardownBarrier<ChannelSlot> for ChannelSlotBarrier {
    fn make_barrier() -> ChannelSlot { ChannelSlot::SyntheticBarrier }

    fn discard_barrier(barrier: ChannelSlot) {
        if !matches!(barrier, ChannelSlot::SyntheticBarrier) {
            std::process::abort();
        }
    }
}

type GuardedChannelMap = GuardedPathMap<ChannelSlot, ChannelSlotDisposer, ChannelSlotBarrier>;

#[derive(Clone)]
pub struct RSpaceDataIndex {
    channels: GuardedChannelMap,
    loaded_channel_count: usize,
    distinct_len: usize,
    exact_cardinality: u128,
}

impl Default for RSpaceDataIndex {
    fn default() -> Self {
        Self {
            channels: GuardedPathMap::new_with_barrier(ChannelSlotDisposer),
            loaded_channel_count: 0,
            distinct_len: 0,
            exact_cardinality: 0,
        }
    }
}

impl RSpaceDataIndex {
    pub fn new() -> Self { Self::default() }

    pub const fn loaded_channel_count(&self) -> usize { self.loaded_channel_count }

    pub const fn distinct_len(&self) -> usize { self.distinct_len }

    pub const fn cardinality(&self) -> u64 {
        if self.exact_cardinality > u64::MAX as u128 {
            u64::MAX
        } else {
            self.exact_cardinality as u64
        }
    }

    pub fn is_empty(&self) -> bool { self.distinct_len == 0 }

    fn channel_bag_at_marker(&self, marker: &[u8]) -> Option<&Bag> {
        self.channels.get(marker).map(ChannelSlot::real)
    }

    fn is_loaded_marker(&self, marker: &[u8]) -> bool { self.channels.contains(marker) }

    fn preflight_new_channel(&self, marker: &[u8]) -> Result<(), DataIndexError> {
        if !self.is_loaded_marker(marker) && self.loaded_channel_count == usize::MAX {
            return Err(DataIndexError::LoadedChannelCountOverflow);
        }
        Ok(())
    }

    fn replace_channel_at_marker(
        &mut self,
        marker: &[u8],
        prepared: Bag,
    ) -> Result<bool, DataIndexError> {
        let previous = self.channel_bag_at_marker(marker);
        let previous_distinct = previous.map_or(0, Bag::distinct_len);
        let previous_exact = previous.map_or(0, Bag::exact_cardinality);
        let was_loaded = previous.is_some();
        let retained_distinct = self
            .distinct_len
            .checked_sub(previous_distinct)
            .expect("one channel contributes its distinct endpoints to the aggregate");
        let retained_exact = self
            .exact_cardinality
            .checked_sub(previous_exact)
            .expect("one channel contributes its cardinality to the aggregate");
        let next_distinct = retained_distinct
            .checked_add(prepared.distinct_len())
            .ok_or(DataIndexError::AggregateDistinctCountOverflow)?;
        let next_exact = retained_exact
            .checked_add(prepared.exact_cardinality())
            .ok_or(DataIndexError::AggregateCardinalityOverflow {
                current: retained_exact,
                added: prepared.exact_cardinality(),
            })?;
        let next_loaded = if was_loaded {
            self.loaded_channel_count
        } else {
            self.loaded_channel_count
                .checked_add(1)
                .ok_or(DataIndexError::LoadedChannelCountOverflow)?
        };

        let replaced = self
            .channels
            .set_val_at(marker, ChannelSlot::Real(prepared));
        debug_assert_eq!(replaced, was_loaded);
        self.loaded_channel_count = next_loaded;
        self.distinct_len = next_distinct;
        self.exact_cardinality = next_exact;
        Ok(was_loaded)
    }

    /// Aggregate metadata is preflighted over the final state before the first
    /// outer-trie mutation.  This avoids both partial publication and false
    /// overflow caused by an arbitrary order of growing and shrinking
    /// channels in one logical delta.
    fn replace_channels_at_markers(
        &mut self,
        replacements: &BTreeMap<Vec<u8>, Bag>,
    ) -> Result<(), DataIndexError> {
        if replacements.is_empty() {
            return Ok(());
        }

        let mut retained_distinct = self.distinct_len;
        let mut retained_exact = self.exact_cardinality;
        let mut added_loaded = 0usize;
        for marker in replacements.keys() {
            match self.channel_bag_at_marker(marker) {
                Some(previous) => {
                    retained_distinct = retained_distinct
                        .checked_sub(previous.distinct_len())
                        .expect("a resident channel contributes its distinct endpoints");
                    retained_exact = retained_exact
                        .checked_sub(previous.exact_cardinality())
                        .expect("a resident channel contributes its exact cardinality");
                }
                None => {
                    added_loaded = added_loaded
                        .checked_add(1)
                        .ok_or(DataIndexError::LoadedChannelCountOverflow)?;
                }
            }
        }

        let mut next_distinct = retained_distinct;
        let mut next_exact = retained_exact;
        for prepared in replacements.values() {
            next_distinct = next_distinct
                .checked_add(prepared.distinct_len())
                .ok_or(DataIndexError::AggregateDistinctCountOverflow)?;
            next_exact = next_exact.checked_add(prepared.exact_cardinality()).ok_or(
                DataIndexError::AggregateCardinalityOverflow {
                    current: next_exact,
                    added: prepared.exact_cardinality(),
                },
            )?;
        }
        let next_loaded = self
            .loaded_channel_count
            .checked_add(added_loaded)
            .ok_or(DataIndexError::LoadedChannelCountOverflow)?;

        for (marker, prepared) in replacements {
            self.channels
                .set_val_at(marker, ChannelSlot::Real(prepared.clone()));
        }
        self.loaded_channel_count = next_loaded;
        self.distinct_len = next_distinct;
        self.exact_cardinality = next_exact;
        Ok(())
    }

    pub fn clear_channel<C: RhoPathEncode>(&mut self, channel: &C) -> (usize, u64) {
        let marker = RhoPathV1::datum_prefix(channel);
        let Some(previous) = self.channel_bag_at_marker(&marker) else {
            return (0, 0);
        };
        let removed_distinct = previous.distinct_len();
        let removed_cardinality = previous.cardinality();
        self.replace_channel_at_marker(&marker, Bag::new())
            .expect("replacing one channel with the empty Bag reduces aggregate metrics");
        (removed_distinct, removed_cardinality)
    }

    pub fn datum_path<C, A>(channel: &C, datum: &Datum<A>) -> Vec<u8>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let mut path = RhoPathV1::datum_prefix(channel);
        path.extend_from_slice(&Self::relative_datum_path(datum));
        path
    }

    fn relative_datum_path<A>(datum: &Datum<A>) -> Vec<u8>
    where A: Clone + Serialize + RhoPathEncode {
        RhoPathV1::relative_datum_key(&datum.a.rho_path_structure(), &encode_datum(datum))
    }

    fn prepare_channel_replacement<A>(
        datums: impl IntoIterator<Item = Datum<A>>,
    ) -> Result<Bag, DataIndexError>
    where A: Clone + Serialize + RhoPathEncode {
        let mut prepared = Bag::new();
        for datum in datums {
            prepared.insert(&Self::relative_datum_path(&datum))?;
        }
        Ok(prepared)
    }

    /// Prepare a channel-local Bag from authenticated endpoint multiplicities
    /// without expanding repeated occurrences into a temporary `Vec`.
    pub(crate) fn prepare_channel_replacement_counts<A>(
        entries: impl IntoIterator<Item = (Datum<A>, Multiplicity)>,
    ) -> Result<Bag, DataIndexError>
    where A: Clone + Serialize + RhoPathEncode {
        let mut prepared = Bag::new();
        for (datum, multiplicity) in entries {
            prepared.insert_many(&Self::relative_datum_path(&datum), multiplicity)?;
        }
        Ok(prepared)
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
        let marker = RhoPathV1::datum_prefix(channel);
        let relative = Self::relative_datum_path(datum);
        let mut path = Vec::with_capacity(marker.len() + relative.len());
        path.extend_from_slice(&marker);
        path.extend_from_slice(&relative);
        let mut bag = self
            .channel_bag_at_marker(&marker)
            .cloned()
            .unwrap_or_default();
        let multiplicity = bag.insert(&relative)?;
        self.replace_channel_at_marker(&marker, bag)?;
        Ok((path, multiplicity))
    }

    pub fn count<C, A>(&self, channel: &C, datum: &Datum<A>) -> u64
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let marker = RhoPathV1::datum_prefix(channel);
        self.channel_bag_at_marker(&marker)
            .map_or(0, |bag| bag.count(&Self::relative_datum_path(datum)))
    }

    pub fn count_path(&self, path: &[u8]) -> u64 {
        let Ok(view) = RhoPathV1::parse_datum(path) else {
            return 0;
        };
        let (marker, relative) = path.split_at(view.channel_prefix_len());
        self.channel_bag_at_marker(marker)
            .map_or(0, |bag| bag.count(relative))
    }

    pub fn remove_path(&mut self, path: &[u8]) -> Option<Multiplicity> {
        let view = RhoPathV1::parse_datum(path).ok()?;
        let (marker, relative) = path.split_at(view.channel_prefix_len());
        let mut bag = self.channel_bag_at_marker(marker)?.clone();
        let next = bag.remove(relative);
        self.replace_channel_at_marker(marker, bag)
            .expect("removing one resident occurrence reduces aggregate metrics");
        next
    }

    pub fn channel_entries<C, A>(&self, channel: &C) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        let marker = RhoPathV1::datum_prefix(channel);
        let Some(bag) = self.channel_bag_at_marker(&marker) else {
            return Ok(Vec::new());
        };
        self.entries_at_relative_prefix(&marker, bag, b"")
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
        let marker = RhoPathV1::datum_prefix(channel);
        let Some(bag) = self.channel_bag_at_marker(&marker) else {
            return Ok(Vec::new());
        };
        let relative_prefix = RhoPathV1::relative_datum_structure_prefix(structure_prefix);
        self.entries_at_relative_prefix(&marker, bag, &relative_prefix)
    }

    pub fn ordered_channel_entries<C, A>(
        &self,
        channel: &C,
    ) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        let marker = RhoPathV1::datum_prefix(channel);
        self.ordered_encoded_channel_entries(&marker)?
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
        let marker = RhoPathV1::datum_prefix(channel);
        let catalog = self.candidate_catalog::<A>(&marker)?;
        self.candidate_pool_from_catalog(&catalog)
    }

    fn candidate_catalog<A>(&self, prefix: &[u8]) -> Result<CandidateCatalog<A>, DataIndexError>
    where A: Clone + ColdStoreDecode {
        let empty = Bag::new();
        let availability = self.channel_bag_at_marker(prefix).unwrap_or(&empty);
        let mut decorated = Vec::new();
        availability.try_visit_relative_prefix(b"", |relative, multiplicity| {
            let identity = RhoPathV1::relative_datum_identity(relative)?;
            let mut path = Vec::with_capacity(prefix.len() + relative.len());
            path.extend_from_slice(prefix);
            path.extend_from_slice(relative);
            let identity_offset = path.len() - identity.len();
            decorated.push((datum_identity_digest(identity), EncodedIndexedDatum {
                path,
                identity_offset,
                multiplicity,
            }));
            Ok::<(), DataIndexError>(())
        })?;
        sort_encoded_candidates(&mut decorated);
        validate_unique_candidate_identities(&decorated)?;
        let ordered = decorated.into_iter().map(|(_, entry)| entry).collect();
        Ok(CandidateCatalog::from_encoded_entries(availability.clone(), prefix.len(), ordered))
    }

    fn candidate_pool_from_catalog<A>(
        &self,
        catalog: &CandidateCatalog<A>,
    ) -> Result<CandidatePool<A>, DataIndexError>
    where
        A: Clone + ColdStoreDecode,
    {
        let availability = catalog.availability.clone();
        Ok(CandidatePool::from_catalog(availability, catalog))
    }

    fn ordered_encoded_channel_entries(
        &self,
        prefix: &[u8],
    ) -> Result<Vec<EncodedIndexedDatum>, DataIndexError> {
        let mut decorated = Vec::new();
        let Some(bag) = self.channel_bag_at_marker(prefix) else {
            return Ok(Vec::new());
        };
        bag.try_visit_relative_prefix(b"", |relative, multiplicity| {
            let identity = RhoPathV1::relative_datum_identity(relative)?;
            let mut path = Vec::with_capacity(prefix.len() + relative.len());
            path.extend_from_slice(prefix);
            path.extend_from_slice(relative);
            let identity_offset = path.len() - identity.len();
            decorated.push((datum_identity_digest(identity), EncodedIndexedDatum {
                path,
                identity_offset,
                multiplicity,
            }));
            Ok::<(), DataIndexError>(())
        })?;
        sort_encoded_candidates(&mut decorated);
        validate_unique_candidate_identities(&decorated)?;
        Ok(decorated.into_iter().map(|(_, entry)| entry).collect())
    }

    fn entries_at_relative_prefix<A>(
        &self,
        marker: &[u8],
        bag: &Bag,
        relative_prefix: &[u8],
    ) -> Result<Vec<IndexedDatum<A>>, DataIndexError>
    where
        A: Clone + ColdStoreDecode,
    {
        let mut entries = Vec::new();
        bag.try_visit_relative_prefix(relative_prefix, |relative_suffix, multiplicity| {
            let mut relative = Vec::with_capacity(relative_prefix.len() + relative_suffix.len());
            relative.extend_from_slice(relative_prefix);
            relative.extend_from_slice(relative_suffix);
            let identity = RhoPathV1::relative_datum_identity(&relative)?;
            let datum = decode_datum(identity)?;
            let mut path = Vec::with_capacity(marker.len() + relative.len());
            path.extend_from_slice(marker);
            path.extend_from_slice(&relative);
            entries.push(IndexedDatum {
                path,
                datum,
                multiplicity,
            });
            Ok::<(), DataIndexError>(())
        })?;
        Ok(entries)
    }

    /// Encode the final legacy `PersistedData::Data` value directly from the
    /// canonical datum identities resident in the PathMap.
    ///
    /// The RhoPath structural dimension is intentionally not the cold-store
    /// ordering authority: legacy leaves sort by each complete datum identity.
    /// This method therefore retains one lightweight identity descriptor per
    /// distinct endpoint, sorts those exact bytes, and expands multiplicity
    /// directly into the final bincode image. It never constructs `Datum<A>`
    /// values or a `Vec<Vec<u8>>` shadow.
    fn encode_canonical_persisted_data(&self, bag: &Bag) -> Result<(Vec<u8>, u64), DataIndexError> {
        let mut identities = Vec::new();
        let mut descriptors = Vec::new();
        bag.try_visit_relative_prefix(b"", |path, multiplicity| {
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
        let payload_capacity = usize::try_from(encoded_len)
            .map_err(|_| DataIndexError::EncodedLengthOverflow(encoded_len))?;
        let capacity = payload_capacity
            .checked_add(PERSISTED_DATA_HEADER_LEN)
            .ok_or(DataIndexError::EncodedLengthOverflow(u128::MAX))?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(capacity)
            .map_err(|_| DataIndexError::AllocationFailure(capacity))?;
        PersistedDataKind::Data.write_header(
            u64::try_from(payload_capacity)
                .map_err(|_| DataIndexError::EncodedLengthOverflow(encoded_len))?,
            &mut output,
        );
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

    fn try_visit_channels<E>(
        &self,
        mut visitor: impl FnMut(&[u8], &Bag) -> Result<(), E>,
    ) -> Result<(), E> {
        self.channels.with_read_cursor(|mut cursor| {
            if let Some(slot) = cursor.val() {
                visitor(cursor.path(), slot.real())?;
            }
            while cursor.to_next_val() {
                let slot = cursor
                    .val()
                    .expect("a successful value step stops at an outer endpoint");
                visitor(cursor.path(), slot.real())?;
            }
            Ok(())
        })
    }

    #[cfg(test)]
    fn outer_anchor_identity(&self) -> usize { self.channels.anchor_identity() }

    #[cfg(test)]
    fn channel_bag_anchor_identity(&self, marker: &[u8]) -> Option<usize> {
        self.channel_bag_at_marker(marker).map(Bag::anchor_identity)
    }

    #[cfg(test)]
    fn from_channel_bag_for_test(marker: &[u8], bag: Bag) -> Self {
        let mut index = Self::new();
        index
            .replace_channel_at_marker(marker, bag)
            .expect("a test channel Bag fits aggregate metrics");
        index
    }
}

impl fmt::Debug for RSpaceDataIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RSpaceDataIndex")
            .field("loaded_channel_count", &self.loaded_channel_count)
            .field("distinct_len", &self.distinct_len)
            .field("cardinality", &self.cardinality())
            .finish()
    }
}

impl PartialEq for RSpaceDataIndex {
    fn eq(&self, other: &Self) -> bool {
        if self.loaded_channel_count != other.loaded_channel_count ||
            self.distinct_len != other.distinct_len ||
            self.exact_cardinality != other.exact_cardinality
        {
            return false;
        }
        let mut equal = true;
        let result = self.try_visit_channels(|marker, bag| {
            if other.channel_bag_at_marker(marker) != Some(bag) {
                equal = false;
            }
            Ok::<(), std::convert::Infallible>(())
        });
        match result {
            Ok(()) => equal,
            Err(error) => match error {},
        }
    }
}

impl Eq for RSpaceDataIndex {}

pub(crate) fn record_rebased_counts(
    journal: &mut BagDelta,
    prefix: &[u8],
    base: &Bag,
    overlay: &Bag,
) -> Result<(), BagError> {
    let mut path = prefix.to_vec();
    overlay
        .subtract(base)
        .try_visit_relative_prefix(b"", |relative, amount| {
            path.truncate(prefix.len());
            path.extend_from_slice(relative);
            journal.record_addition(&path, amount)
        })?;
    base.subtract(overlay)
        .try_visit_relative_prefix(b"", |relative, amount| {
            path.truncate(prefix.len());
            path.extend_from_slice(relative);
            journal.record_removal(&path, amount)
        })
}

pub struct RSpaceDataStore<C, A: Clone> {
    index: RSpaceDataIndex,
    candidate_catalogs: RwLock<HashMap<Vec<u8>, Arc<CandidateCatalog<A>>>>,
    /// Net path-level changes since the last checkpoint boundary. Repeated
    /// mutations cancel through the same PathMap-backed Bag algebra as the
    /// authoritative data index.
    pending_delta: BagDelta,
    marker: PhantomData<fn() -> (C, A)>,
}

impl<C, A: Clone> Clone for RSpaceDataStore<C, A> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            candidate_catalogs: RwLock::new(
                self.candidate_catalogs
                    .read()
                    .expect("candidate catalog read lock")
                    .clone(),
            ),
            pending_delta: self.pending_delta.clone(),
            marker: PhantomData,
        }
    }
}

impl<C, A: Clone> Default for RSpaceDataStore<C, A> {
    fn default() -> Self {
        Self {
            index: RSpaceDataIndex::new(),
            candidate_catalogs: RwLock::new(HashMap::new()),
            pending_delta: BagDelta::new(),
            marker: PhantomData,
        }
    }
}

impl<C, A: Clone> RSpaceDataStore<C, A> {
    pub fn new() -> Self { Self::default() }

    pub fn index(&self) -> &RSpaceDataIndex { &self.index }

    pub fn loaded_channel_count(&self) -> usize { self.index.loaded_channel_count() }

    pub fn distinct_len(&self) -> usize { self.index.distinct_len() }

    pub fn cardinality(&self) -> u64 { self.index.cardinality() }

    pub fn is_empty(&self) -> bool { self.index.loaded_channel_count() == 0 }

    pub fn clear(&mut self) {
        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        let empty = RSpaceDataIndex::new();
        self.index = empty;
        catalogs.clear();
        self.pending_delta = BagDelta::new();
    }

    /// Consume normalized path-level changes since the last checkpoint.
    /// Callers must hold the enclosing hot-store transition boundary.
    pub fn take_pending_delta(&mut self) -> BagDelta { std::mem::take(&mut self.pending_delta) }

    pub fn pending_delta(&self) -> &BagDelta { &self.pending_delta }

    pub(crate) fn count_path(&self, path: &[u8]) -> u64 { self.index.count_path(path) }

    /// Record a datum inserted while hydrating a previously cold channel.
    /// Channel replacement is intentionally journal-free because it is a
    /// history projection; the caller must record the one new datum after the
    /// projection has been installed.
    pub(crate) fn record_pending_datum_addition(
        &mut self,
        path: &[u8],
    ) -> Result<(), DataIndexError> {
        self.pending_delta
            .record_addition(path, Multiplicity::ONE)
            .map_err(Into::into)
    }

    pub(crate) fn restore_pending_delta(&mut self, delta: BagDelta) { self.pending_delta = delta; }

    pub(crate) fn loaded_channels(&self) -> Result<Vec<(Vec<u8>, C)>, DataIndexError>
    where C: ColdStoreDecode {
        let mut channels = Vec::new();
        channels
            .try_reserve_exact(self.loaded_channel_count())
            .map_err(|_| DataIndexError::AllocationFailure(self.loaded_channel_count()))?;
        self.index.try_visit_channels(|marker, _| {
            let view = RhoPathV1::parse_datum_prefix(marker)?;
            channels.push((marker.to_vec(), C::cold_decode(&view.channel_identity())?));
            Ok::<(), DataIndexError>(())
        })?;
        Ok(channels)
    }

    pub(crate) fn record_rebased_channel(
        &self,
        marker: &[u8],
        base: impl IntoIterator<Item = Datum<A>>,
        journal: &mut BagDelta,
    ) -> Result<(), DataIndexError>
    where
        A: Clone + Serialize + RhoPathEncode,
    {
        let Some(overlay) = self.index.channel_bag_at_marker(marker) else {
            return Ok(());
        };
        let base = RSpaceDataIndex::prepare_channel_replacement(base)?;
        record_rebased_counts(journal, marker, &base, overlay).map_err(Into::into)
    }

    pub fn is_loaded(&self, channel: &C) -> bool
    where C: RhoPathEncode {
        self.index
            .is_loaded_marker(&RhoPathV1::datum_prefix(channel))
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
        let marker = RhoPathV1::datum_prefix(&channel);
        self.index.preflight_new_channel(&marker)?;
        let prepared = RSpaceDataIndex::prepare_channel_replacement(datums)?;
        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        self.index.replace_channel_at_marker(&marker, prepared)?;
        catalogs.remove(&marker);
        Ok(())
    }

    /// Replace one channel from exact authenticated multiplicities. Counts
    /// stay in the Bag/PathMap representation throughout construction.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only the test-only eager durable hot-store bootstrap loads channels from \
                      authenticated counts"
        )
    )]
    pub(crate) fn set_channel_counts(
        &mut self,
        channel: C,
        entries: impl IntoIterator<Item = (Datum<A>, Multiplicity)>,
    ) -> Result<(), DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let marker = RhoPathV1::datum_prefix(&channel);
        self.index.preflight_new_channel(&marker)?;
        let prepared = RSpaceDataIndex::prepare_channel_replacement_counts(entries)?;
        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        self.index.replace_channel_at_marker(&marker, prepared)?;
        catalogs.remove(&marker);
        Ok(())
    }

    pub fn insert(&mut self, channel: &C, datum: &Datum<A>) -> Result<Multiplicity, DataIndexError>
    where
        C: RhoPathEncode,
        A: Clone + Serialize + RhoPathEncode,
    {
        let marker = RhoPathV1::datum_prefix(channel);
        self.index.preflight_new_channel(&marker)?;
        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        let (path, multiplicity) = self.index.insert(channel, datum)?;
        self.pending_delta
            .record_addition(&path, Multiplicity::new(1).expect("one is nonzero"))?;
        let relative_path = path
            .strip_prefix(marker.as_slice())
            .expect("an inserted datum path must begin with its channel prefix");
        if let Some(catalog) = catalogs.get_mut(&marker) {
            let catalog = Arc::make_mut(catalog);
            if catalog.record_insert_metadata(relative_path, multiplicity) {
                catalog.availability = self
                    .index
                    .channel_bag_at_marker(&marker)
                    .expect("an inserted channel is loaded")
                    .clone();
            } else {
                catalogs.remove(&marker);
            }
        }
        #[cfg(feature = "accepted-match-transition-instrumentation")]
        crate::rspace::hot_store::accepted_match_transition_experiment::record_pathmap_root_publication();
        Ok(multiplicity)
    }

    /// Atomically apply exact datum-identity multiplicity changes.
    pub fn apply_delta(&mut self, delta: &BagDelta) -> Result<(), DataIndexError> {
        if delta.is_empty() {
            return Ok(());
        }

        let mut channel_deltas = BTreeMap::<Vec<u8>, BagDelta>::new();
        for (is_addition, changes) in [(true, delta.additions()), (false, delta.removals())] {
            changes.try_visit_prefix(b"", |path, amount| {
                let view = RhoPathV1::parse_datum(path)?;
                let (marker, relative) = path.split_at(view.channel_prefix_len());
                let channel_delta = channel_deltas.entry(marker.to_vec()).or_default();
                if is_addition {
                    channel_delta.record_addition(relative, amount)?;
                } else {
                    channel_delta.record_removal(relative, amount)?;
                }
                Ok::<(), DataIndexError>(())
            })?;
        }

        let mut replacements = BTreeMap::new();
        for (marker, channel_delta) in &channel_deltas {
            let mut prepared = self
                .index
                .channel_bag_at_marker(marker)
                .cloned()
                .unwrap_or_default();
            channel_delta.apply_to(&mut prepared)?;
            replacements.insert(marker.clone(), prepared);
        }

        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        let mut candidate_catalogs = channel_deltas
            .keys()
            .map(|marker| (marker.clone(), catalogs.get(marker).cloned()))
            .collect::<BTreeMap<_, _>>();
        for (is_addition, changes) in [(true, delta.additions()), (false, delta.removals())] {
            changes.visit_prefix(b"", |path, amount| {
                let view = RhoPathV1::parse_datum(path)
                    .expect("a BagDelta datum path was validated before mutation");
                let marker = &path[..view.channel_prefix_len()];
                let relative_path = &path[view.channel_prefix_len()..];
                let current_count = replacements
                    .get(marker)
                    .expect("every validated delta path has a prepared channel")
                    .count(relative_path);
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

                let invalidate = candidate_catalogs
                    .get_mut(marker)
                    .and_then(Option::as_mut)
                    .is_some_and(|catalog| {
                        !Arc::make_mut(catalog).record_transition_metadata(
                            relative_path,
                            previous,
                            current,
                        )
                    });
                if invalidate {
                    *candidate_catalogs
                        .get_mut(marker)
                        .expect("every validated delta path has a staged channel") = None;
                }
            });
        }
        for marker in channel_deltas.keys() {
            if let Some(catalog) = candidate_catalogs
                .get_mut(marker.as_slice())
                .and_then(Option::as_mut)
            {
                Arc::make_mut(catalog).availability = replacements
                    .get(marker)
                    .expect("a successfully changed channel is prepared")
                    .clone();
            }
        }

        self.index.replace_channels_at_markers(&replacements)?;
        for (is_addition, changes) in [(true, delta.additions()), (false, delta.removals())] {
            changes.visit_prefix(b"", |path, amount| {
                let result = if is_addition {
                    self.pending_delta.record_addition(path, amount)
                } else {
                    self.pending_delta.record_removal(path, amount)
                };
                result.expect("validated BagDelta must remain representable");
            });
        }
        for (marker, catalog) in candidate_catalogs {
            match catalog {
                Some(catalog) => {
                    catalogs.insert(marker, catalog);
                }
                None => {
                    catalogs.remove(&marker);
                }
            }
        }
        #[cfg(feature = "accepted-match-transition-instrumentation")]
        crate::rspace::hot_store::accepted_match_transition_experiment::record_pathmap_root_publication();
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
        if self.index.is_loaded_marker(&prefix) {
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
                .candidate_pool_from_catalog(catalog.as_ref())
                .map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn remove_path(&mut self, path: &[u8]) -> bool {
        let Some(previous) = Multiplicity::new(self.index.count_path(path)).ok() else {
            return false;
        };
        let Ok(view) = RhoPathV1::parse_datum(path) else {
            return false;
        };
        let marker = &path[..view.channel_prefix_len()];
        let relative_path = &path[view.channel_prefix_len()..];
        let catalogs = self
            .candidate_catalogs
            .get_mut()
            .expect("candidate catalog write lock");
        self.index.remove_path(path);
        self.pending_delta
            .record_removal(path, Multiplicity::new(1).expect("one is nonzero"))
            .expect("a removed positive multiplicity is representable");
        if let Some(catalog) = catalogs.get_mut(marker) {
            let catalog = Arc::make_mut(catalog);
            catalog.record_remove_metadata(relative_path, previous);
            catalog.availability = self
                .index
                .channel_bag_at_marker(marker)
                .expect("removal preserves the loaded channel endpoint")
                .clone();
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
            .try_reserve_exact(self.loaded_channel_count())
            .map_err(|_| DataIndexError::AllocationFailure(self.loaded_channel_count()))?;
        self.index.try_visit_channels(|path, _| {
            let view = RhoPathV1::parse_datum_prefix(path)?;
            let channel = C::cold_decode(&view.channel_identity())?;
            let datums = self.channel_data(&channel)?.unwrap_or_default();
            records.push((channel, datums));
            Ok::<(), DataIndexError>(())
        })?;
        Ok(records)
    }

    pub(crate) fn try_visit_loaded_records(
        &self,
        mut visitor: impl FnMut(C, Vec<Datum<A>>) -> Result<(), DataIndexError>,
    ) -> Result<(), DataIndexError>
    where
        C: Clone + ColdStoreDecode + RhoPathEncode,
        A: Clone + ColdStoreDecode,
    {
        self.index.try_visit_channels(|path, _| {
            let view = RhoPathV1::parse_datum_prefix(path)?;
            let channel = C::cold_decode(&view.channel_identity())?;
            let datums = self.channel_data(&channel)?.unwrap_or_default();
            visitor(channel, datums)?;
            Ok::<(), DataIndexError>(())
        })
    }

    pub(crate) fn has_nonempty_loaded_channel(&self) -> bool { self.index.distinct_len() != 0 }

    /// Return checkpoint-ready DATA records without decoding resident datum
    /// identities back into typed values.
    pub(crate) fn try_visit_loaded_canonical_records(
        &self,
        mut visitor: impl FnMut(CanonicalDataRecord) -> Result<(), DataIndexError>,
    ) -> Result<(), DataIndexError> {
        self.index.try_visit_channels(|prefix, bag| {
            let view = RhoPathV1::parse_datum_prefix(prefix)?;
            let channel_hash = Blake2b256Hash::new(&view.channel_identity());
            let (encoded_persisted_data, datum_count) =
                self.index.encode_canonical_persisted_data(bag)?;
            visitor(CanonicalDataRecord {
                channel_hash,
                encoded_persisted_data,
                datum_count,
            })?;
            Ok::<(), DataIndexError>(())
        })
    }
}

impl<C, A: Clone> fmt::Debug for RSpaceDataStore<C, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RSpaceDataStore")
            .field("loaded_channel_count", &self.loaded_channel_count())
            .field("distinct_len", &self.distinct_len())
            .field("cardinality", &self.cardinality())
            .finish()
    }
}

impl<C, A: Clone> PartialEq for RSpaceDataStore<C, A> {
    fn eq(&self, other: &Self) -> bool { self.index == other.index }
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

    #[test]
    fn malformed_two_level_index_refuses_one_identity_under_two_structural_paths() {
        let channel = "channel".to_owned();
        let marker = RhoPathV1::datum_prefix(&channel);
        let value = Datum {
            a: Arc::new("same datum".to_owned()),
            persist: false,
            source: crate::rspace::trace::event::Produce::new(
                Blake2b256Hash::new(b"produce"),
                Blake2b256Hash::new(b"produce-sequence"),
                false,
            ),
        };
        let identity = encode_datum(&value);
        let left = RhoPathV1::relative_datum_key(b"a-derived-structure", &identity);
        let right = RhoPathV1::relative_datum_key(b"z-derived-structure", &identity);
        assert_ne!(left, right, "the malformed fixture needs two exact trie paths");

        let mut bag = Bag::new();
        bag.insert(&left).unwrap();
        bag.insert(&right).unwrap();
        let index = RSpaceDataIndex::from_channel_bag_for_test(&marker, bag);
        assert_eq!(index.distinct_len(), 2);
        assert_eq!(
            index.ordered_channel_entries::<_, String>(&channel),
            Err(DataIndexError::DuplicateDatumIdentity),
        );
        assert!(matches!(
            index.candidate_pool::<_, String>(&channel),
            Err(DataIndexError::DuplicateDatumIdentity)
        ));
    }

    #[test]
    fn malformed_relative_index_keys_fail_at_the_selected_channel() {
        let channel = "channel".to_owned();
        let marker = RhoPathV1::datum_prefix(&channel);
        let mut bag = Bag::new();
        bag.insert(&[0, 2]).unwrap();
        let index = RSpaceDataIndex::from_channel_bag_for_test(&marker, bag);

        assert!(matches!(
            index.channel_entries::<_, String>(&channel),
            Err(DataIndexError::Path(_))
        ));
    }

    #[test]
    fn set_channel_reuses_the_unique_outer_authority_anchor() {
        let mut store = RSpaceDataStore::<String, String>::new();
        let before = store.index.outer_anchor_identity();
        store
            .set_channel("one".to_owned(), [Datum {
                a: Arc::new("payload".to_owned()),
                persist: false,
                source: crate::rspace::trace::event::Produce::new(
                    Blake2b256Hash::new(b"produce"),
                    Blake2b256Hash::new(b"produce-seq"),
                    false,
                ),
            }])
            .expect("first channel replacement");
        store
            .set_channel("empty".to_owned(), [])
            .expect("loaded-empty replacement");
        assert_eq!(
            store.index.outer_anchor_identity(),
            before,
            "set_channel must not manufacture a full-store COW version"
        );
    }

    #[test]
    fn set_channel_counts_preserves_authenticated_multiplicity_without_expansion() {
        let mut store = RSpaceDataStore::<String, String>::new();
        let channel = "counted-channel".to_owned();
        let datum = Datum {
            a: Arc::new("counted-payload".to_owned()),
            persist: false,
            source: crate::rspace::trace::event::Produce::new(
                Blake2b256Hash::new(b"counted-produce"),
                Blake2b256Hash::new(b"counted-sequence"),
                false,
            ),
        };
        store
            .set_channel_counts(channel.clone(), [(
                datum.clone(),
                Multiplicity::new(7).expect("positive count"),
            )])
            .expect("counted channel replacement");
        assert_eq!(store.index().count(&channel, &datum), 7);
        assert_eq!(store.cardinality(), 7);
        assert_eq!(store.distinct_len(), 1);
    }

    #[test]
    fn set_channel_source_has_no_whole_store_clone_transaction() {
        let source = include_str!("data_index.rs");
        let start = source
            .find("    pub fn set_channel(")
            .expect("set_channel source boundary");
        let tail = &source[start..];
        let end = tail
            .find("\n    pub fn insert(")
            .expect("next data-store method boundary");
        assert!(
            !tail[..end].contains("self.clone()"),
            "set_channel must prepare a channel-local Bag, not clone the whole store"
        );
    }

    #[test]
    fn apply_delta_reuses_the_unique_outer_authority_anchor() {
        let datum = |label: &str| Datum {
            a: Arc::new(format!("delta-payload-{label}")),
            persist: false,
            source: crate::rspace::trace::event::Produce::new(
                Blake2b256Hash::new(label.as_bytes()),
                Blake2b256Hash::new(b"delta-produce-seq"),
                false,
            ),
        };
        let loaded = "loaded".to_owned();
        let unloaded = "unloaded".to_owned();
        let mut store = RSpaceDataStore::<String, String>::new();
        store
            .set_channel(loaded.clone(), [datum("resident")])
            .expect("a resident channel loads");
        let before = store.index.outer_anchor_identity();
        let mut delta = BagDelta::new();
        delta
            .record_addition(
                &RSpaceDataIndex::datum_path(&loaded, &datum("added")),
                Multiplicity::ONE,
            )
            .expect("an addition to a loaded channel records");
        delta
            .record_removal(
                &RSpaceDataIndex::datum_path(&loaded, &datum("resident")),
                Multiplicity::ONE,
            )
            .expect("a removal from a loaded channel records");
        delta
            .record_addition(
                &RSpaceDataIndex::datum_path(&unloaded, &datum("first")),
                Multiplicity::ONE,
            )
            .expect("an addition to an unloaded channel records");

        store
            .apply_delta(&delta)
            .expect("a valid two-channel delta applies");

        assert_eq!(store.index().count(&loaded, &datum("added")), 1);
        assert_eq!(store.index().count(&loaded, &datum("resident")), 0);
        assert_eq!(store.index().count(&unloaded, &datum("first")), 1);
        assert_eq!(store.loaded_channel_count(), 2);
        assert_eq!(
            store.index.outer_anchor_identity(),
            before,
            "apply_delta must update the unique outer authority in place"
        );
    }

    #[test]
    fn apply_delta_source_has_no_whole_index_clone_transaction() {
        let source = include_str!("data_index.rs");
        let start = source
            .find("    pub fn apply_delta(")
            .expect("apply_delta source boundary");
        let tail = &source[start..];
        let end = tail
            .find("\n    pub fn channel_entries(")
            .expect("next data-store method boundary");
        let body = &tail[..end];
        assert!(
            !body.contains("self.index.clone()"),
            "apply_delta must write the outer authority in place, not clone the whole index"
        );
        assert!(!body.contains("self.index = "), "apply_delta must not replace the whole index");
    }

    #[test]
    fn pending_delta_tracks_and_cancels_path_mutations_without_projection() {
        let mut store = RSpaceDataStore::<String, String>::new();
        let channel = "journal-channel".to_owned();
        store
            .set_channel(channel.clone(), [])
            .expect("load empty channel");
        let datum = Datum {
            a: Arc::new("journal-payload".to_owned()),
            persist: false,
            source: crate::rspace::trace::event::Produce::new(
                Blake2b256Hash::new(b"journal-produce"),
                Blake2b256Hash::new(b"journal-sequence"),
                false,
            ),
        };

        store.insert(&channel, &datum).expect("insert datum");
        assert_eq!(store.pending_delta().additions().distinct_len(), 1);
        assert!(store.pending_delta().removals().is_empty());

        store.remove_index(&channel, 0).expect("remove datum");
        assert!(store.pending_delta().is_empty());
    }
}

#[cfg(test)]
#[path = "../../tests/data_index/candidate_product_continuation.rs"]
mod candidate_product_continuation_tests;

#[cfg(test)]
#[path = "../../tests/data_index/candidate_pool_reservation.rs"]
mod candidate_pool_reservation_tests;

#[cfg(test)]
#[path = "../../tests/data_index/channel_prefix_replacement_properties.rs"]
mod channel_prefix_replacement_properties;

#[cfg(test)]
#[path = "../../tests/data_index/two_level_channel_authority.rs"]
mod two_level_channel_authority_tests;

#[cfg(test)]
#[path = "../../tests/data_index/flat_delta_refinement.rs"]
mod flat_delta_refinement_tests;

#[cfg(test)]
#[path = "../../tests/data_index/digest_collision_replay.rs"]
mod digest_collision_replay_tests;

#[cfg(feature = "candidate-continuation-experiment")]
#[doc(hidden)]
#[path = "../../tests/data_index/candidate_continuation_experiment.rs"]
pub mod candidate_continuation_experiment;

#[cfg(feature = "canonical-egress-experiment")]
#[doc(hidden)]
#[path = "../../tests/data_index/canonical_egress_experiment.rs"]
pub mod canonical_egress_experiment;
