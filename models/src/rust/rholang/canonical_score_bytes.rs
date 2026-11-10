//! Arena-backed canonical score bytes for byte-native protobuf admission.
//!
//! A score is a DAG of atom and node handles. Parents retain handles rather
//! than flattening child bytes, so a depth-`D` protobuf term owns `O(D)` score
//! nodes instead of copying every descendant score into every ancestor. The
//! lending byte cursor is itself an explicit heap worklist and therefore has
//! no native-stack depth dependency.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

#[cfg(test)]
use prost::bytes::Bytes;
pub(crate) use verified_score_store::ScoreHandle as CanonicalScoreHandle;
use verified_score_store::{
    BorrowedIndexedWindow, ScoreEntry as VerifiedScoreEntry, ScoreNode as VerifiedScoreNode,
    ScoreStore,
};

use super::canonical_semantics::{CanonicalScoreAlgebra, PreScoredBytes, PreScoredString};
use super::indexed_protobuf::{IndexedByteCursor, IndexedByteRegion};
#[cfg(test)]
use super::protobuf_cursor::ProtobufRegion;

const INTEGER_PREFIX: &[u8] = &[1, 1];
const STRING_PREFIX: &[u8] = &[1, 2];
const BYTES_PREFIX: &[u8] = &[1, 3];
const TERMINATOR: &[u8] = &[0, 0];
const NODE_START: u8 = 2;
const NODE_END: u8 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalScoreArenaError {
    LengthOverflow,
}

impl fmt::Display for CanonicalScoreArenaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthOverflow => {
                formatter.write_str("canonical score exceeds addressable memory")
            }
        }
    }
}

impl std::error::Error for CanonicalScoreArenaError {}

enum ScoreBytes<'backing> {
    #[cfg(test)]
    Owned(Box<[u8]>),
    #[cfg(test)]
    Shared(Bytes),
    #[cfg(test)]
    Region(ProtobufRegion),
    Indexed(IndexedByteRegion<'backing>),
}

impl ScoreBytes<'_> {
    fn as_slice(&self) -> Option<&[u8]> {
        match self {
            #[cfg(test)]
            Self::Owned(bytes) => Some(bytes),
            #[cfg(test)]
            Self::Shared(bytes) => Some(bytes),
            #[cfg(test)]
            Self::Region(region) => Some(region.as_bytes()),
            Self::Indexed(_) => None,
        }
    }
}

type ScoreNode<'backing> = VerifiedScoreNode<'backing, ScoreBytes<'backing>>;
type ScoreEntry<'backing> = VerifiedScoreEntry<'backing, ScoreBytes<'backing>>;

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalScoreSourceKind {
    Integer(i64),
    StringOwned(Vec<u8>),
    StringShared(Vec<u8>),
    StringRegion(Vec<u8>),
    StringIndexed(Vec<u8>),
    BytesOwned(Vec<u8>),
    BytesShared(Vec<u8>),
    BytesRegion(Vec<u8>),
    BytesIndexed(Vec<u8>),
    Node(Vec<usize>),
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalScoreSourceEntry {
    pub(crate) kind: CanonicalScoreSourceKind,
    pub(crate) encoded_len: usize,
}

// Only the arena's prepare paths can construct this entry. Its length is
// computed from source bytes or previously certified child entries.
struct PreparedScoreEntry<'backing>(ScoreEntry<'backing>);

#[derive(Clone)]
pub(crate) struct CanonicalScoreAppendCheckpoint {
    prefix_len: usize,
    arena_identity: Arc<()>,
}

impl CanonicalScoreAppendCheckpoint {
    pub(crate) fn source_len(&self) -> usize { self.prefix_len }
}

/// Read-only score source. All payloads and child lists remain borrowed from
/// the arena; an indexed atom retains its exact indexed backing view.
#[derive(Clone, Copy)]
pub(crate) enum CanonicalScoreBytesView<'entry, 'backing> {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production admission in protobuf_admission.rs reads only integer, node and indexed string score views, so owned and byte-node payloads are read only by the test score-source audits"
        )
    )]
    Owned(&'entry [u8]),
    #[cfg(test)]
    Shared(&'entry Bytes),
    #[cfg(test)]
    Region(&'entry ProtobufRegion),
    Indexed(&'entry IndexedByteRegion<'backing>),
    BorrowedIndexed(&'entry BorrowedIndexedWindow<'backing>),
}

#[derive(Clone, Copy)]
pub(crate) enum CanonicalScoreNodeView<'entry, 'backing> {
    Integer(i64),
    String(CanonicalScoreBytesView<'entry, 'backing>),
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production admission in protobuf_admission.rs reads only integer, node and indexed string score views, so owned and byte-node payloads are read only by the test score-source audits"
        )
    )]
    Bytes(CanonicalScoreBytesView<'entry, 'backing>),
    Node(&'entry [CanonicalScoreHandle]),
}

#[derive(Clone, Copy)]
pub(crate) struct CanonicalScoreEntryView<'entry, 'backing> {
    pub(crate) kind: CanonicalScoreNodeView<'entry, 'backing>,
    #[cfg(test)]
    pub(crate) encoded_len: usize,
}

pub(crate) struct CanonicalScoreArena<'backing> {
    store: ScoreStore<'backing, ScoreBytes<'backing>>,
    certified_prefix_len: usize,
    identity: Arc<()>,
    #[cfg(test)]
    backing: Option<Bytes>,
    #[cfg(test)]
    zero_prefix: Vec<usize>,
}

impl<'backing> CanonicalScoreArena<'backing> {
    #[cfg(test)]
    const ZERO_BLOCK_BYTES: usize = 256;

    pub(crate) fn new() -> Self {
        Self {
            store: ScoreStore::new(),
            certified_prefix_len: 0,
            identity: Arc::new(()),
            #[cfg(test)]
            backing: None,
            #[cfg(test)]
            zero_prefix: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_region(root: &ProtobufRegion) -> Result<Self, CanonicalScoreArenaError> {
        let backing = root.backing_bytes();
        let blocks = backing
            .len()
            .checked_add(Self::ZERO_BLOCK_BYTES - 1)
            .ok_or(CanonicalScoreArenaError::LengthOverflow)?
            / Self::ZERO_BLOCK_BYTES;
        let mut zero_prefix = Vec::new();
        zero_prefix
            .try_reserve_exact(blocks + 1)
            .map_err(|_| CanonicalScoreArenaError::LengthOverflow)?;
        zero_prefix.push(0);
        let mut total = 0usize;
        for block in backing.chunks(Self::ZERO_BLOCK_BYTES) {
            total = total
                .checked_add(block.iter().filter(|&&byte| byte == 0).count())
                .ok_or(CanonicalScoreArenaError::LengthOverflow)?;
            zero_prefix.push(total);
        }
        Ok(Self {
            store: ScoreStore::new(),
            certified_prefix_len: 0,
            identity: Arc::new(()),
            backing: Some(backing),
            zero_prefix,
        })
    }

    fn prepare(
        &self,
        node: ScoreNode<'backing>,
    ) -> Result<PreparedScoreEntry<'backing>, CanonicalScoreArenaError> {
        let encoded_len = match &node {
            ScoreNode::Integer(_) => 10,
            ScoreNode::StringOwned(bytes) | ScoreNode::BytesOwned(bytes) => {
                Self::terminated_len(bytes)?
            }
            ScoreNode::String(bytes) | ScoreNode::Bytes(bytes) => match bytes {
                #[cfg(test)]
                ScoreBytes::Owned(value) => Self::terminated_len(value)?,
                #[cfg(test)]
                ScoreBytes::Shared(value) => Self::terminated_len(value)?,
                #[cfg(test)]
                ScoreBytes::Region(value) => {
                    Self::terminated_len_from_counts(value.len(), self.region_zero_count(value)?)?
                }
                ScoreBytes::Indexed(value) => Self::indexed_terminated_len(value)?,
            },
            ScoreNode::StringBorrowed(bytes) | ScoreNode::BytesBorrowed(bytes) => {
                Self::terminated_len(bytes.as_slice())?
            }
            ScoreNode::Node(children) => children.iter().try_fold(2usize, |length, child| {
                let entry = self
                    .store
                    .source_entry_at(child.index())
                    .ok_or(CanonicalScoreArenaError::LengthOverflow)?;
                length
                    .checked_add(entry.encoded_len)
                    .ok_or(CanonicalScoreArenaError::LengthOverflow)
            })?,
        };
        Ok(PreparedScoreEntry(ScoreEntry { node, encoded_len }))
    }

    fn push(
        &mut self,
        node: ScoreNode<'backing>,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        self.push_with_reservation(node, || Ok(()))
    }

    fn push_with_reservation(
        &mut self,
        node: ScoreNode<'backing>,
        reservation_granted: impl FnOnce() -> Result<(), CanonicalScoreArenaError>,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        let prepared = self.prepare(node)?;
        self.push_prepared_with_reservation(prepared, reservation_granted)
    }

    fn push_prepared_with_reservation(
        &mut self,
        prepared: PreparedScoreEntry<'backing>,
        reservation_granted: impl FnOnce() -> Result<(), CanonicalScoreArenaError>,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        if self.certified_prefix_len != self.store.len() {
            return Err(CanonicalScoreArenaError::LengthOverflow);
        }
        let PreparedScoreEntry(entry) = prepared;
        reservation_granted()?;
        let handle = match entry.node {
            ScoreNode::Integer(value) => self.store.append_integer(value),
            ScoreNode::StringOwned(bytes) => self.store.append_owned_string(bytes),
            ScoreNode::BytesOwned(bytes) => self.store.append_owned_bytes(bytes),
            ScoreNode::String(bytes) => self.store.append_string(bytes, entry.encoded_len),
            ScoreNode::Bytes(bytes) => self.store.append_bytes(bytes, entry.encoded_len),
            ScoreNode::StringBorrowed(bytes) => self.store.append_borrowed_string(bytes),
            ScoreNode::BytesBorrowed(bytes) => self.store.append_borrowed_bytes(bytes),
            ScoreNode::Node(children) => self.store.append_node(children),
        }
        .map_err(|_| CanonicalScoreArenaError::LengthOverflow)?;
        self.certified_prefix_len = self.store.len();
        Ok(handle)
    }

    fn append_owned_string(
        &mut self,
        value: &str,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        if self.certified_prefix_len != self.store.len() {
            return Err(CanonicalScoreArenaError::LengthOverflow);
        }
        let handle = self
            .store
            .append_owned_string(value.as_bytes().to_vec())
            .map_err(|_| CanonicalScoreArenaError::LengthOverflow)?;
        self.certified_prefix_len = self.store.len();
        Ok(handle)
    }

    fn append_owned_bytes(
        &mut self,
        value: &[u8],
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        if self.certified_prefix_len != self.store.len() {
            return Err(CanonicalScoreArenaError::LengthOverflow);
        }
        let handle = self
            .store
            .append_owned_bytes(value.to_vec())
            .map_err(|_| CanonicalScoreArenaError::LengthOverflow)?;
        self.certified_prefix_len = self.store.len();
        Ok(handle)
    }

    fn terminated_len(bytes: &[u8]) -> Result<usize, CanonicalScoreArenaError> {
        let escapes = bytes.iter().filter(|&&byte| byte == 0).count();
        Self::terminated_len_from_counts(bytes.len(), escapes)
    }

    fn terminated_len_from_counts(
        len: usize,
        escapes: usize,
    ) -> Result<usize, CanonicalScoreArenaError> {
        4usize
            .checked_add(len)
            .and_then(|length| length.checked_add(escapes))
            .ok_or(CanonicalScoreArenaError::LengthOverflow)
    }

    #[cfg(test)]
    fn node_len(
        child_lengths: impl IntoIterator<Item = usize>,
    ) -> Result<usize, CanonicalScoreArenaError> {
        child_lengths
            .into_iter()
            .try_fold(2usize, |length, child_length| {
                length
                    .checked_add(child_length)
                    .ok_or(CanonicalScoreArenaError::LengthOverflow)
            })
    }

    fn indexed_terminated_len(
        bytes: &IndexedByteRegion<'_>,
    ) -> Result<usize, CanonicalScoreArenaError> {
        let escapes = bytes.bytes().try_fold(0usize, |count, byte| {
            count
                .checked_add(usize::from(byte == 0))
                .ok_or(CanonicalScoreArenaError::LengthOverflow)
        })?;
        Self::terminated_len_from_counts(bytes.len(), escapes)
    }

    /// Retain an already-owned byte region as one canonical byte-array score
    /// atom. `Bytes::slice` makes this O(1), including an EPM1 snapshot cut from
    /// the owning protobuf image.
    #[cfg(test)]
    pub(crate) fn shared_bytes(
        &mut self,
        bytes: Bytes,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        self.push(ScoreNode::Bytes(ScoreBytes::Shared(bytes)))
    }

    #[cfg(test)]
    fn region_zero_count(
        &self,
        region: &ProtobufRegion,
    ) -> Result<usize, CanonicalScoreArenaError> {
        let backing = self
            .backing
            .as_ref()
            .ok_or(CanonicalScoreArenaError::LengthOverflow)?;
        if !region.shares_backing_with(backing) {
            return Err(CanonicalScoreArenaError::LengthOverflow);
        }
        let range = region.absolute_range();
        if range.start > range.end || range.end > backing.len() {
            return Err(CanonicalScoreArenaError::LengthOverflow);
        }
        if range.is_empty() {
            return Ok(0);
        }

        let first_full = range
            .start
            .checked_add(Self::ZERO_BLOCK_BYTES - 1)
            .ok_or(CanonicalScoreArenaError::LengthOverflow)?
            / Self::ZERO_BLOCK_BYTES;
        let last_full = range.end / Self::ZERO_BLOCK_BYTES;
        if first_full > last_full {
            return Ok(backing[range].iter().filter(|&&byte| byte == 0).count());
        }

        let prefix_end = first_full * Self::ZERO_BLOCK_BYTES;
        let suffix_start = last_full * Self::ZERO_BLOCK_BYTES;
        let prefix = backing[range.start..prefix_end]
            .iter()
            .filter(|&&byte| byte == 0)
            .count();
        let middle = self.zero_prefix[last_full] - self.zero_prefix[first_full];
        let suffix = backing[suffix_start..range.end]
            .iter()
            .filter(|&&byte| byte == 0)
            .count();
        prefix
            .checked_add(middle)
            .and_then(|count| count.checked_add(suffix))
            .ok_or(CanonicalScoreArenaError::LengthOverflow)
    }

    /// Reference one subregion of the admission root without copying it or
    /// rescanning the complete region to derive its escaped score length. The
    /// rank index scans at most two fixed 256-byte boundary fragments.
    #[cfg(test)]
    pub(crate) fn shared_region(
        &mut self,
        region: ProtobufRegion,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        self.push(ScoreNode::Bytes(ScoreBytes::Region(region)))
    }

    /// Retain an indexed protobuf string without materializing its bytes.
    /// UTF-8 validity remains the admission cursor's responsibility; this
    /// method only selects the canonical string-atom score prefix.
    pub(crate) fn indexed_string(
        &mut self,
        bytes: IndexedByteRegion<'backing>,
    ) -> Result<PreScoredString<CanonicalScoreHandle>, CanonicalScoreArenaError> {
        let score = if let Some(window) = bytes.borrowed_window() {
            if self.certified_prefix_len != self.store.len() {
                return Err(CanonicalScoreArenaError::LengthOverflow);
            }
            let score = self
                .store
                .append_borrowed_string(window)
                .map_err(|_| CanonicalScoreArenaError::LengthOverflow)?;
            self.certified_prefix_len = self.store.len();
            score
        } else {
            self.push(ScoreNode::String(ScoreBytes::Indexed(bytes)))?
        };
        Ok(PreScoredString::new(score))
    }

    /// Retain an indexed protobuf byte string without materializing its bytes.
    pub(crate) fn indexed_bytes(
        &mut self,
        bytes: IndexedByteRegion<'backing>,
    ) -> Result<PreScoredBytes<CanonicalScoreHandle>, CanonicalScoreArenaError> {
        let score = if let Some(window) = bytes.borrowed_window() {
            if self.certified_prefix_len != self.store.len() {
                return Err(CanonicalScoreArenaError::LengthOverflow);
            }
            let score = self
                .store
                .append_borrowed_bytes(window)
                .map_err(|_| CanonicalScoreArenaError::LengthOverflow)?;
            self.certified_prefix_len = self.store.len();
            score
        } else {
            self.push(ScoreNode::Bytes(ScoreBytes::Indexed(bytes)))?
        };
        Ok(PreScoredBytes::new(score))
    }

    #[cfg(test)]
    pub(crate) fn encoded_len(&self, score: CanonicalScoreHandle) -> usize {
        self.store
            .source_entry_at(score.index())
            .unwrap()
            .encoded_len
    }

    pub(crate) fn cursor(
        &self,
        score: CanonicalScoreHandle,
    ) -> CanonicalScoreByteCursor<'_, 'backing> {
        CanonicalScoreByteCursor::new(self, score)
    }

    pub(crate) fn compare(
        &self,
        left: CanonicalScoreHandle,
        right: CanonicalScoreHandle,
    ) -> Ordering {
        self.cursor(left).cmp(self.cursor(right))
    }

    #[cfg(test)]
    pub(crate) fn equals_bytes(&self, score: CanonicalScoreHandle, expected: &[u8]) -> bool {
        self.cursor(score).eq(expected.iter().copied())
    }

    #[cfg(test)]
    fn to_vec(&self, score: CanonicalScoreHandle) -> Vec<u8> { self.cursor(score).collect() }

    fn bytes_view<'entry>(
        bytes: &'entry ScoreBytes<'backing>,
    ) -> CanonicalScoreBytesView<'entry, 'backing> {
        match bytes {
            #[cfg(test)]
            ScoreBytes::Owned(value) => CanonicalScoreBytesView::Owned(value),
            #[cfg(test)]
            ScoreBytes::Shared(value) => CanonicalScoreBytesView::Shared(value),
            #[cfg(test)]
            ScoreBytes::Region(value) => CanonicalScoreBytesView::Region(value),
            ScoreBytes::Indexed(value) => CanonicalScoreBytesView::Indexed(value),
        }
    }

    fn entry_view<'entry>(
        entry: &'entry ScoreEntry<'backing>,
    ) -> CanonicalScoreEntryView<'entry, 'backing> {
        let kind = match &entry.node {
            ScoreNode::Integer(value) => CanonicalScoreNodeView::Integer(*value),
            ScoreNode::StringOwned(bytes) => {
                CanonicalScoreNodeView::String(CanonicalScoreBytesView::Owned(bytes))
            }
            ScoreNode::BytesOwned(bytes) => {
                CanonicalScoreNodeView::Bytes(CanonicalScoreBytesView::Owned(bytes))
            }
            ScoreNode::String(bytes) => CanonicalScoreNodeView::String(Self::bytes_view(bytes)),
            ScoreNode::Bytes(bytes) => CanonicalScoreNodeView::Bytes(Self::bytes_view(bytes)),
            ScoreNode::StringBorrowed(bytes) => {
                CanonicalScoreNodeView::String(CanonicalScoreBytesView::BorrowedIndexed(bytes))
            }
            ScoreNode::BytesBorrowed(bytes) => {
                CanonicalScoreNodeView::Bytes(CanonicalScoreBytesView::BorrowedIndexed(bytes))
            }
            ScoreNode::Node(children) => CanonicalScoreNodeView::Node(children),
        };
        CanonicalScoreEntryView {
            kind,
            #[cfg(test)]
            encoded_len: entry.encoded_len,
        }
    }

    /// Obtain one exact source entry without flattening its score transcript.
    pub(crate) fn source_entry_at(
        &self,
        index: usize,
    ) -> Option<CanonicalScoreEntryView<'_, 'backing>> {
        self.store.source_entry_at(index).map(Self::entry_view)
    }

    /// Read only entries appended since `start`. This never traverses the old
    /// prefix and never retains references across a mutable arena operation.
    #[cfg(test)]
    pub(crate) fn source_suffix_from(
        &self,
        start: usize,
    ) -> Option<impl Iterator<Item = (usize, CanonicalScoreEntryView<'_, 'backing>)> + '_> {
        let suffix = self.store.entries().get(start..)?;
        Some(
            suffix
                .iter()
                .enumerate()
                .map(move |(offset, entry)| (start + offset, Self::entry_view(entry))),
        )
    }

    /// Independently rescan a newly appended suffix. Production uses the
    /// insertion-time certificate; this remains the test oracle for it.
    #[cfg(test)]
    pub(crate) fn source_suffix_valid(&self, start: usize) -> bool {
        let Some(mut suffix) = self.source_suffix_from(start) else {
            return false;
        };
        suffix.all(|(index, entry)| match entry.kind {
            CanonicalScoreNodeView::Integer(_) => entry.encoded_len == 10,
            CanonicalScoreNodeView::String(bytes) | CanonicalScoreNodeView::Bytes(bytes) => {
                let length = match bytes {
                    CanonicalScoreBytesView::Owned(value) => Self::terminated_len(value),
                    #[cfg(test)]
                    CanonicalScoreBytesView::Shared(value) => Self::terminated_len(value),
                    #[cfg(test)]
                    CanonicalScoreBytesView::Region(value) => {
                        Self::terminated_len(value.as_bytes())
                    }
                    CanonicalScoreBytesView::Indexed(value) => Self::indexed_terminated_len(value),
                    CanonicalScoreBytesView::BorrowedIndexed(value) => {
                        Self::terminated_len(value.as_slice())
                    }
                };
                length == Ok(entry.encoded_len)
            }
            CanonicalScoreNodeView::Node(children) => {
                children.iter().all(|child| child.index() < index)
                    && Self::node_len(children.iter().map(|child| {
                        self.store
                            .source_entry_at(child.index())
                            .unwrap()
                            .encoded_len
                    })) == Ok(entry.encoded_len)
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn source_image(&self) -> Vec<CanonicalScoreSourceEntry> {
        self.store
            .entries()
            .iter()
            .map(|entry| {
                let kind = match &entry.node {
                    ScoreNode::Integer(value) => CanonicalScoreSourceKind::Integer(*value),
                    ScoreNode::StringOwned(bytes) => {
                        CanonicalScoreSourceKind::StringOwned(bytes.clone())
                    }
                    ScoreNode::BytesOwned(bytes) => {
                        CanonicalScoreSourceKind::BytesOwned(bytes.clone())
                    }
                    ScoreNode::String(bytes) => match bytes {
                        ScoreBytes::Owned(value) => {
                            CanonicalScoreSourceKind::StringOwned(value.to_vec())
                        }
                        ScoreBytes::Shared(value) => {
                            CanonicalScoreSourceKind::StringShared(value.to_vec())
                        }
                        ScoreBytes::Region(value) => {
                            CanonicalScoreSourceKind::StringRegion(value.as_bytes().to_vec())
                        }
                        ScoreBytes::Indexed(value) => {
                            CanonicalScoreSourceKind::StringIndexed(value.bytes().collect())
                        }
                    },
                    ScoreNode::StringBorrowed(value) => {
                        CanonicalScoreSourceKind::StringIndexed(value.as_slice().to_vec())
                    }
                    ScoreNode::Bytes(bytes) => match bytes {
                        ScoreBytes::Owned(value) => {
                            CanonicalScoreSourceKind::BytesOwned(value.to_vec())
                        }
                        ScoreBytes::Shared(value) => {
                            CanonicalScoreSourceKind::BytesShared(value.to_vec())
                        }
                        ScoreBytes::Region(value) => {
                            CanonicalScoreSourceKind::BytesRegion(value.as_bytes().to_vec())
                        }
                        ScoreBytes::Indexed(value) => {
                            CanonicalScoreSourceKind::BytesIndexed(value.bytes().collect())
                        }
                    },
                    ScoreNode::BytesBorrowed(value) => {
                        CanonicalScoreSourceKind::BytesIndexed(value.as_slice().to_vec())
                    }
                    ScoreNode::Node(children) => CanonicalScoreSourceKind::Node(
                        children.iter().map(|handle| handle.index()).collect(),
                    ),
                };
                CanonicalScoreSourceEntry {
                    kind,
                    encoded_len: entry.encoded_len,
                }
            })
            .collect()
    }

    pub(crate) fn source_len(&self) -> usize { self.store.len() }

    pub(crate) fn append_checkpoint(&self) -> CanonicalScoreAppendCheckpoint {
        CanonicalScoreAppendCheckpoint {
            prefix_len: self.certified_prefix_len,
            arena_identity: Arc::clone(&self.identity),
        }
    }

    /// Every entry up to `certified_prefix_len` was prepared from its retained
    /// source before the sole append path published it. This check visits no
    /// payload bytes at Message EOF, including for shared indexed regions.
    pub(crate) fn certified_append_since(
        &self,
        checkpoint: &CanonicalScoreAppendCheckpoint,
    ) -> bool {
        Arc::ptr_eq(&checkpoint.arena_identity, &self.identity)
            && checkpoint.prefix_len <= self.certified_prefix_len
            && self.certified_prefix_len == self.store.len()
    }
}

impl<'backing> Default for CanonicalScoreArena<'backing> {
    fn default() -> Self { Self::new() }
}

impl<'backing> CanonicalScoreAlgebra for CanonicalScoreArena<'backing> {
    type Score = CanonicalScoreHandle;
    type Error = CanonicalScoreArenaError;

    fn integer(&mut self, value: i64) -> Result<Self::Score, Self::Error> {
        self.push(ScoreNode::Integer(value))
    }

    fn string(&mut self, value: &str) -> Result<Self::Score, Self::Error> {
        self.append_owned_string(value)
    }

    fn bytes(&mut self, value: &[u8]) -> Result<Self::Score, Self::Error> {
        self.append_owned_bytes(value)
    }

    fn node(&mut self, children: Vec<Self::Score>) -> Result<Self::Score, Self::Error> {
        self.push(ScoreNode::Node(children))
    }
}

enum CursorOp<'arena, 'backing> {
    Score(CanonicalScoreHandle),
    Slice {
        bytes: &'arena [u8],
        offset: usize,
    },
    Integer {
        bytes: [u8; 8],
        offset: usize,
    },
    Escaped {
        bytes: &'arena [u8],
        offset: usize,
        pending_ff: bool,
    },
    IndexedEscaped {
        bytes: IndexedByteCursor<'backing>,
        pending_ff: bool,
    },
    Byte(u8),
}

pub(crate) struct CanonicalScoreByteCursor<'arena, 'backing> {
    arena: &'arena CanonicalScoreArena<'backing>,
    work: Vec<CursorOp<'arena, 'backing>>,
}

impl<'arena, 'backing> CanonicalScoreByteCursor<'arena, 'backing> {
    fn new(arena: &'arena CanonicalScoreArena<'backing>, root: CanonicalScoreHandle) -> Self {
        Self {
            arena,
            work: vec![CursorOp::Score(root)],
        }
    }

    fn push_slice(&mut self, bytes: &'arena [u8]) {
        if !bytes.is_empty() {
            self.work.push(CursorOp::Slice { bytes, offset: 0 });
        }
    }
}

impl Iterator for CanonicalScoreByteCursor<'_, '_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                CursorOp::Score(handle) => match &self
                    .arena
                    .store
                    .source_entry_at(handle.index())
                    .unwrap()
                    .node
                {
                    ScoreNode::Integer(value) => {
                        let bytes = ((*value as u64) ^ (1u64 << 63)).to_be_bytes();
                        self.work.push(CursorOp::Integer { bytes, offset: 0 });
                        self.push_slice(INTEGER_PREFIX);
                    }
                    ScoreNode::StringOwned(bytes) => {
                        self.push_slice(TERMINATOR);
                        self.work.push(CursorOp::Escaped {
                            bytes,
                            offset: 0,
                            pending_ff: false,
                        });
                        self.push_slice(STRING_PREFIX);
                    }
                    ScoreNode::BytesOwned(bytes) => {
                        self.push_slice(TERMINATOR);
                        self.work.push(CursorOp::Escaped {
                            bytes,
                            offset: 0,
                            pending_ff: false,
                        });
                        self.push_slice(BYTES_PREFIX);
                    }
                    ScoreNode::String(bytes) => {
                        self.push_slice(TERMINATOR);
                        match bytes.as_slice() {
                            Some(bytes) => self.work.push(CursorOp::Escaped {
                                bytes,
                                offset: 0,
                                pending_ff: false,
                            }),
                            None => match bytes {
                                ScoreBytes::Indexed(bytes) => {
                                    self.work.push(CursorOp::IndexedEscaped {
                                        bytes: bytes.bytes(),
                                        pending_ff: false,
                                    });
                                }
                                #[cfg(test)]
                                _ => unreachable!("only indexed score bytes lack a slice"),
                            },
                        }
                        self.push_slice(STRING_PREFIX);
                    }
                    ScoreNode::StringBorrowed(bytes) => {
                        self.push_slice(TERMINATOR);
                        self.work.push(CursorOp::Escaped {
                            bytes: bytes.as_slice(),
                            offset: 0,
                            pending_ff: false,
                        });
                        self.push_slice(STRING_PREFIX);
                    }
                    ScoreNode::Bytes(bytes) => {
                        self.push_slice(TERMINATOR);
                        match bytes.as_slice() {
                            Some(bytes) => self.work.push(CursorOp::Escaped {
                                bytes,
                                offset: 0,
                                pending_ff: false,
                            }),
                            None => match bytes {
                                ScoreBytes::Indexed(bytes) => {
                                    self.work.push(CursorOp::IndexedEscaped {
                                        bytes: bytes.bytes(),
                                        pending_ff: false,
                                    });
                                }
                                #[cfg(test)]
                                _ => unreachable!("only indexed score bytes lack a slice"),
                            },
                        }
                        self.push_slice(BYTES_PREFIX);
                    }
                    ScoreNode::BytesBorrowed(bytes) => {
                        self.push_slice(TERMINATOR);
                        self.work.push(CursorOp::Escaped {
                            bytes: bytes.as_slice(),
                            offset: 0,
                            pending_ff: false,
                        });
                        self.push_slice(BYTES_PREFIX);
                    }
                    ScoreNode::Node(children) => {
                        self.work.push(CursorOp::Byte(NODE_END));
                        self.work
                            .extend(children.iter().rev().copied().map(CursorOp::Score));
                        self.work.push(CursorOp::Byte(NODE_START));
                    }
                },
                CursorOp::Slice { bytes, offset } => {
                    let byte = bytes[offset];
                    if offset + 1 < bytes.len() {
                        self.work.push(CursorOp::Slice {
                            bytes,
                            offset: offset + 1,
                        });
                    }
                    return Some(byte);
                }
                CursorOp::Integer { bytes, offset } => {
                    let byte = bytes[offset];
                    if offset + 1 < bytes.len() {
                        self.work.push(CursorOp::Integer {
                            bytes,
                            offset: offset + 1,
                        });
                    }
                    return Some(byte);
                }
                CursorOp::Escaped {
                    bytes,
                    offset,
                    pending_ff,
                } => {
                    if pending_ff {
                        self.work.push(CursorOp::Escaped {
                            bytes,
                            offset,
                            pending_ff: false,
                        });
                        return Some(0xff);
                    }
                    let Some(&byte) = bytes.get(offset) else {
                        continue;
                    };
                    self.work.push(CursorOp::Escaped {
                        bytes,
                        offset: offset + 1,
                        pending_ff: byte == 0,
                    });
                    return Some(byte);
                }
                CursorOp::IndexedEscaped {
                    mut bytes,
                    pending_ff,
                } => {
                    if pending_ff {
                        self.work.push(CursorOp::IndexedEscaped {
                            bytes,
                            pending_ff: false,
                        });
                        return Some(0xff);
                    }
                    let Some(byte) = bytes.next() else {
                        continue;
                    };
                    self.work.push(CursorOp::IndexedEscaped {
                        bytes,
                        pending_ff: byte == 0,
                    });
                    return Some(byte);
                }
                CursorOp::Byte(byte) => return Some(byte),
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) { (0, None) }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::rust::rholang::canonical_bytes_arena::{
        CanonicalByteProvenance, CanonicalBytesArena, CanonicalLocatedByte,
    };
    use crate::rust::rholang::canonical_semantics::{
        ground_score, ground_score_pre_scored, GroundScore, PreScoredGroundScore,
    };
    use crate::rust::rholang::protobuf_cursor::CanonicalByteRange;

    const SOURCE: u32 = 91;

    fn append_indexed_bytes(
        arena: &mut CanonicalBytesArena,
        bytes: &[u8],
    ) -> super::super::canonical_bytes_arena::CanonicalBytesHandle {
        let mut value = arena.empty();
        for (offset, byte) in bytes.iter().copied().enumerate() {
            value = arena
                .append(
                    value,
                    CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(SOURCE, offset)),
                )
                .unwrap();
        }
        value
    }

    fn independently_encoded_atom(prefix: [u8; 2], bytes: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(4 + bytes.len() * 2);
        encoded.extend(prefix);
        for byte in bytes {
            encoded.push(*byte);
            if *byte == 0 {
                encoded.push(0xff);
            }
        }
        encoded.extend([0, 0]);
        encoded
    }

    fn borrowed_source_entry(entry: CanonicalScoreEntryView<'_, '_>) -> CanonicalScoreSourceEntry {
        let kind = match entry.kind {
            CanonicalScoreNodeView::Integer(value) => CanonicalScoreSourceKind::Integer(value),
            CanonicalScoreNodeView::Node(children) => {
                CanonicalScoreSourceKind::Node(children.iter().map(|child| child.index()).collect())
            }
            CanonicalScoreNodeView::String(bytes) => match bytes {
                CanonicalScoreBytesView::Owned(value) => {
                    CanonicalScoreSourceKind::StringOwned(value.to_vec())
                }
                CanonicalScoreBytesView::Shared(value) => {
                    CanonicalScoreSourceKind::StringShared(value.to_vec())
                }
                CanonicalScoreBytesView::Region(value) => {
                    CanonicalScoreSourceKind::StringRegion(value.as_bytes().to_vec())
                }
                CanonicalScoreBytesView::Indexed(value) => {
                    CanonicalScoreSourceKind::StringIndexed(value.bytes().collect())
                }
                CanonicalScoreBytesView::BorrowedIndexed(value) => {
                    CanonicalScoreSourceKind::StringIndexed(value.as_slice().to_vec())
                }
            },
            CanonicalScoreNodeView::Bytes(bytes) => match bytes {
                CanonicalScoreBytesView::Owned(value) => {
                    CanonicalScoreSourceKind::BytesOwned(value.to_vec())
                }
                CanonicalScoreBytesView::Shared(value) => {
                    CanonicalScoreSourceKind::BytesShared(value.to_vec())
                }
                CanonicalScoreBytesView::Region(value) => {
                    CanonicalScoreSourceKind::BytesRegion(value.as_bytes().to_vec())
                }
                CanonicalScoreBytesView::Indexed(value) => {
                    CanonicalScoreSourceKind::BytesIndexed(value.bytes().collect())
                }
                CanonicalScoreBytesView::BorrowedIndexed(value) => {
                    CanonicalScoreSourceKind::BytesIndexed(value.as_slice().to_vec())
                }
            },
        };
        CanonicalScoreSourceEntry {
            kind,
            encoded_len: entry.encoded_len,
        }
    }

    #[test]
    fn append_certificate_rejects_future_child_before_publishing() {
        let mut scores = CanonicalScoreArena::new();
        let child = scores.integer(7).unwrap();
        let checkpoint = scores.append_checkpoint();
        let next = scores.source_len();
        assert_eq!(
            scores.node(vec![CanonicalScoreHandle(next)]),
            Err(CanonicalScoreArenaError::LengthOverflow)
        );
        assert_eq!(scores.source_len(), next);
        assert_eq!(scores.append_checkpoint().source_len(), next);
        assert!(scores.certified_append_since(&checkpoint));
        assert!(scores.source_suffix_valid(next));
        let parent = scores.node(vec![child]).unwrap();
        assert_eq!(parent.index(), next);
        assert!(scores.certified_append_since(&checkpoint));
        assert!(scores.source_suffix_valid(next));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_score_appends_match_independent_source_entries_and_bytes(
            operations in prop::collection::vec(
                (0u8..7, any::<i64>(),
                    prop::collection::vec(any::<u8>(), 0..12),
                    prop::collection::vec(any::<u8>(), 0..3)),
                1..12,
            ),
        ) {
            let mut scores = CanonicalScoreArena::new();
            let mut handles = Vec::new();
            let mut expected_entries = Vec::new();
            let mut expected_bytes: Vec<Vec<u8>> = Vec::new();
            for (kind, integer, payload, children) in &operations {
                let next = expected_entries.len();
                let checkpoint = scores.append_checkpoint();
                prop_assert_eq!(checkpoint.source_len(), next);
                let (handle, source_kind, encoded) = match kind {
                    0 => {
                        let mut encoded = vec![1, 1];
                        encoded.extend(((*integer as u64) ^ (1u64 << 63)).to_be_bytes());
                        (scores.integer(*integer).unwrap(),
                            CanonicalScoreSourceKind::Integer(*integer), encoded)
                    }
                    1 => (
                        scores.bytes(payload).unwrap(),
                        CanonicalScoreSourceKind::BytesOwned(payload.clone()),
                        independently_encoded_atom([1, 3], payload),
                    ),
                    2 => {
                        let string: String = payload.iter().map(|byte| char::from(*byte % 128)).collect();
                        let bytes = string.as_bytes().to_vec();
                        (
                            scores.string(&string).unwrap(),
                            CanonicalScoreSourceKind::StringOwned(bytes.clone()),
                            independently_encoded_atom([1, 2], &bytes),
                        )
                    }
                    3 => {
                        let child_indices: Vec<_> = children.iter()
                            .map(|index| usize::from(*index) % next.max(1))
                            .filter(|_| next > 0)
                            .collect();
                        let mut encoded = vec![2];
                        for index in &child_indices {
                            encoded.extend_from_slice(&expected_bytes[*index]);
                        }
                        encoded.push(0);
                        let node_children = child_indices.iter().map(|index| handles[*index]).collect();
                        (
                            scores.node(node_children).unwrap(),
                            CanonicalScoreSourceKind::Node(child_indices),
                            encoded,
                        )
                    }
                    4 => {
                        let region = IndexedByteRegion::from_borrowed(payload, SOURCE, next);
                        (
                            scores.indexed_bytes(region).unwrap().into_score(),
                            CanonicalScoreSourceKind::BytesIndexed(payload.clone()),
                            independently_encoded_atom([1, 3], payload),
                        )
                    }
                    5 => {
                        let region = IndexedByteRegion::from_borrowed(payload, SOURCE, next);
                        (
                            scores.indexed_string(region).unwrap().into_score(),
                            CanonicalScoreSourceKind::StringIndexed(payload.clone()),
                            independently_encoded_atom([1, 2], payload),
                        )
                    }
                    6 => (
                        scores.shared_bytes(Bytes::copy_from_slice(payload)).unwrap(),
                        CanonicalScoreSourceKind::BytesShared(payload.clone()),
                        independently_encoded_atom([1, 3], payload),
                    ),
                    _ => unreachable!(),
                };
                assert_eq!(handle.index(), next);
                assert_eq!(scores.encoded_len(handle), encoded.len());
                expected_entries.push(CanonicalScoreSourceEntry {
                    kind: source_kind,
                    encoded_len: encoded.len(),
                });
                assert!(scores.source_suffix_valid(next));
                prop_assert!(scores.certified_append_since(&checkpoint));
                let new_entries: Vec<_> = scores.source_suffix_from(next).unwrap().collect();
                assert_eq!(new_entries.len(), 1);
                assert_eq!(new_entries[0].0, next);
                assert_eq!(borrowed_source_entry(new_entries[0].1), expected_entries[next]);
                assert_eq!(borrowed_source_entry(scores.source_entry_at(next).unwrap()), expected_entries[next]);
                handles.push(handle);
                expected_bytes.push(encoded);
                assert_eq!(scores.source_image(), expected_entries);
                for (index, previous) in handles.iter().copied().enumerate() {
                    assert_eq!(scores.to_vec(previous), expected_bytes[index]);
                }
            }
        }

        #[test]
        fn generated_reservation_failures_preserve_entries_bytes_and_next_handle(
            operations in prop::collection::vec(
                (0u8..7, any::<i64>(), prop::collection::vec(any::<u8>(), 0..12)),
                1..20,
            ),
        ) {
            let mut scores = CanonicalScoreArena::new();
            let mut handles = Vec::new();
            for (tag, value, payload) in operations {
                let children: Vec<_> = handles.iter().rev().copied().take(3).collect();
                let string = String::from_utf8_lossy(&payload).into_owned();
                let (node, source_kind, expected_bytes) = match tag {
                    0 => {
                        let mut expected = vec![1, 1];
                        expected.extend(((value as u64) ^ (1u64 << 63)).to_be_bytes());
                        (ScoreNode::Integer(value), CanonicalScoreSourceKind::Integer(value), expected)
                    }
                    1 => {
                        let bytes = string.as_bytes();
                        (
                            ScoreNode::String(ScoreBytes::Owned(Box::from(bytes))),
                            CanonicalScoreSourceKind::StringOwned(bytes.to_vec()),
                            independently_encoded_atom([1, 2], bytes),
                        )
                    }
                    2 => (
                        ScoreNode::Bytes(ScoreBytes::Owned(Box::from(payload.as_slice()))),
                        CanonicalScoreSourceKind::BytesOwned(payload.clone()),
                        independently_encoded_atom([1, 3], &payload),
                    ),
                    3 => {
                        let mut expected = vec![2];
                        for child in &children {
                            expected.extend(scores.to_vec(*child));
                        }
                        expected.push(0);
                        (
                            ScoreNode::Node(children.clone()),
                            CanonicalScoreSourceKind::Node(children.iter().map(|child| child.index()).collect()),
                            expected,
                        )
                    }
                    4 => (
                        ScoreNode::String(ScoreBytes::Indexed(IndexedByteRegion::from_protobuf_region(
                            ProtobufRegion::copy_from_slice(string.as_bytes()), SOURCE,
                        ))),
                        CanonicalScoreSourceKind::StringIndexed(string.as_bytes().to_vec()),
                        independently_encoded_atom([1, 2], string.as_bytes()),
                    ),
                    5 => (
                        ScoreNode::Bytes(ScoreBytes::Indexed(IndexedByteRegion::from_protobuf_region(
                            ProtobufRegion::copy_from_slice(&payload), SOURCE,
                        ))),
                        CanonicalScoreSourceKind::BytesIndexed(payload.clone()),
                        independently_encoded_atom([1, 3], &payload),
                    ),
                    6 => (
                        ScoreNode::Bytes(ScoreBytes::Shared(Bytes::copy_from_slice(&payload))),
                        CanonicalScoreSourceKind::BytesShared(payload.clone()),
                        independently_encoded_atom([1, 3], &payload),
                    ),
                    _ => unreachable!(),
                };
                let encoded_len = expected_bytes.len();
                let before = scores.source_image();
                let checkpoint = scores.append_checkpoint();
                let before_bytes: Vec<_> = handles.iter().map(|handle| scores.to_vec(*handle)).collect();
                let denied = scores.push_with_reservation(node, || {
                    Err(CanonicalScoreArenaError::LengthOverflow)
                });
                assert_eq!(denied, Err(CanonicalScoreArenaError::LengthOverflow));
                assert_eq!(scores.source_image(), before);
                prop_assert_eq!(scores.append_checkpoint().source_len(), checkpoint.source_len());
                prop_assert!(scores.certified_append_since(&checkpoint));
                for (handle, bytes) in handles.iter().zip(before_bytes.iter()) {
                    assert_eq!(scores.to_vec(*handle), *bytes);
                }
                let handle = match tag {
                    0 => scores.integer(value).unwrap(),
                    1 => scores.string(&string).unwrap(),
                    2 => scores.bytes(&payload).unwrap(),
                    3 => scores.node(children).unwrap(),
                    4 => scores.indexed_string(IndexedByteRegion::from_protobuf_region(
                        ProtobufRegion::copy_from_slice(string.as_bytes()), SOURCE,
                    )).unwrap().into_score(),
                    5 => scores.indexed_bytes(IndexedByteRegion::from_protobuf_region(
                        ProtobufRegion::copy_from_slice(&payload), SOURCE,
                    )).unwrap().into_score(),
                    6 => scores.shared_bytes(Bytes::copy_from_slice(&payload)).unwrap(),
                    _ => unreachable!(),
                };
                assert_eq!(handle.index(), before.len());
                assert_eq!(scores.encoded_len(handle), encoded_len);
                assert_eq!(scores.to_vec(handle), expected_bytes);
                assert_eq!(scores.source_image().last(), Some(&CanonicalScoreSourceEntry {
                    kind: source_kind,
                    encoded_len,
                }));
                handles.push(handle);
            }
        }

        #[test]
        fn generated_region_reservation_denial_and_retry_preserve_source_and_bytes(
            root_bytes in prop::collection::vec(any::<u8>(), 0..1024),
            raw_start in any::<usize>(),
            raw_span in any::<usize>(),
            seed in any::<i64>(),
        ) {
            let root = ProtobufRegion::copy_from_slice(&root_bytes);
            let start = if root_bytes.is_empty() { 0 } else { raw_start % root_bytes.len() };
            let end = if root_bytes.is_empty() {
                0
            } else {
                start + 1 + raw_span % (root_bytes.len() - start)
            };
            let mut scores = CanonicalScoreArena::for_region(&root).unwrap();
            let mut handles = vec![scores.integer(seed).unwrap()];
            for (range_start, range_end) in [(start, start), (start, end)] {
                let region = root.subregion(CanonicalByteRange {
                    start: range_start,
                    end: range_end,
                }).unwrap();
                let payload = &root_bytes[range_start..range_end];
                let expected_bytes = independently_encoded_atom([1, 3], payload);
                let prior_source = scores.source_image();
                let checkpoint = scores.append_checkpoint();
                let prior_bytes: Vec<_> = handles.iter().map(|handle| scores.to_vec(*handle)).collect();
                let denied = scores.push_with_reservation(
                    ScoreNode::Bytes(ScoreBytes::Region(region.clone())),
                    || Err(CanonicalScoreArenaError::LengthOverflow),
                );
                assert_eq!(denied, Err(CanonicalScoreArenaError::LengthOverflow));
                assert_eq!(scores.source_image(), prior_source);
                prop_assert_eq!(scores.append_checkpoint().source_len(), checkpoint.source_len());
                prop_assert!(scores.certified_append_since(&checkpoint));
                for (handle, bytes) in handles.iter().zip(prior_bytes.iter()) {
                    assert_eq!(scores.to_vec(*handle), *bytes);
                }
                let handle = scores.shared_region(region).unwrap();
                assert_eq!(handle.index(), prior_source.len());
                assert_eq!(scores.to_vec(handle), expected_bytes);
                assert_eq!(scores.source_image().last(), Some(&CanonicalScoreSourceEntry {
                    kind: CanonicalScoreSourceKind::BytesRegion(payload.to_vec()),
                    encoded_len: expected_bytes.len(),
                }));
                handles.push(handle);
            }
        }

        #[test]
        fn generated_atom_length_overflow_matches_wide_arithmetic(
            length in prop_oneof![0usize..=64, (usize::MAX - 64)..=usize::MAX],
            escapes in prop_oneof![0usize..=64, (usize::MAX - 64)..=usize::MAX],
        ) {
            let exact = 4u128 + length as u128 + escapes as u128;
            let expected = usize::try_from(exact)
                .map_err(|_| CanonicalScoreArenaError::LengthOverflow);
            assert_eq!(CanonicalScoreArena::terminated_len_from_counts(length, escapes), expected);
        }

        #[test]
        fn generated_node_length_overflow_matches_wide_arithmetic(
            child_lengths in prop::collection::vec(
                prop_oneof![0usize..=64, (usize::MAX - 64)..=usize::MAX],
                0..16,
            ),
        ) {
            let exact = child_lengths.iter().fold(2u128, |length, child| length + *child as u128);
            let expected = usize::try_from(exact)
                .map_err(|_| CanonicalScoreArenaError::LengthOverflow);
            assert_eq!(CanonicalScoreArena::node_len(child_lengths), expected);
        }
    }

    #[test]
    fn arena_emits_prefix_free_atoms_and_nodes_exactly() {
        let mut arena = CanonicalScoreArena::new();
        let integer = arena.integer(-1).unwrap();
        let string = arena.string("a\0b").unwrap();
        let bytes = arena.shared_bytes(Bytes::from_static(&[0, 1])).unwrap();
        let root = arena.node(vec![integer, string, bytes]).unwrap();

        let mut expected = vec![2, 1, 1];
        expected.extend_from_slice(&(((-1i64 as u64) ^ (1u64 << 63)).to_be_bytes()));
        expected.extend_from_slice(&[1, 2, b'a', 0, 0xff, b'b', 0, 0]);
        expected.extend_from_slice(&[1, 3, 0, 0xff, 1, 0, 0, 0]);
        assert_eq!(arena.to_vec(root), expected);
        assert_eq!(arena.encoded_len(root), expected.len());
        assert!(arena.equals_bytes(root, &expected));
    }

    #[test]
    fn deep_score_cursor_uses_the_heap_on_a_small_native_stack() {
        std::thread::Builder::new()
            .name("canonical-score-arena-depth".to_string())
            .stack_size(128 * 1024)
            .spawn(|| {
                let mut arena = CanonicalScoreArena::new();
                let mut score = arena.integer(0).unwrap();
                for _ in 0..20_000 {
                    score = arena.node(vec![score]).unwrap();
                }
                assert_eq!(arena.cursor(score).count(), arena.encoded_len(score));
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn shared_region_lengths_use_the_root_zero_rank_index() {
        let mut bytes = vec![7u8; 1_024];
        for offset in [0, 3, 255, 256, 511, 700, 1_023] {
            bytes[offset] = 0;
        }
        let root = ProtobufRegion::from(bytes);
        let region = root
            .subregion(CanonicalByteRange { start: 3, end: 701 })
            .unwrap();
        let expected_zeroes = region.as_bytes().iter().filter(|&&byte| byte == 0).count();
        let mut arena = CanonicalScoreArena::for_region(&root).unwrap();
        let score = arena.shared_region(region.clone()).unwrap();
        assert_eq!(arena.encoded_len(score), 4 + region.len() + expected_zeroes);
        let emitted = arena.to_vec(score);
        assert_eq!(&emitted[..2], BYTES_PREFIX);
        assert_eq!(&emitted[emitted.len() - 2..], TERMINATOR);
    }

    #[test]
    fn contiguous_and_persistent_indexed_atoms_match_owned_scores_and_order() {
        let string_bytes = b"a\0z";
        let byte_bytes = b"\0\x01\xff";

        let mut backing = CanonicalBytesArena::new();
        let string_handle = append_indexed_bytes(&mut backing, string_bytes);
        let byte_handle = append_indexed_bytes(&mut backing, byte_bytes);
        let persistent_string = IndexedByteRegion::from_arena(
            &backing,
            string_handle,
            CanonicalByteProvenance::new(SOURCE, string_bytes.len()),
        );
        let persistent_bytes = IndexedByteRegion::from_arena(
            &backing,
            byte_handle,
            CanonicalByteProvenance::new(SOURCE, byte_bytes.len()),
        );
        let contiguous_string = IndexedByteRegion::from_protobuf_region(
            ProtobufRegion::copy_from_slice(string_bytes),
            SOURCE + 1,
        );
        let contiguous_bytes = IndexedByteRegion::from_protobuf_region(
            ProtobufRegion::copy_from_slice(byte_bytes),
            SOURCE + 1,
        );

        let mut scores = CanonicalScoreArena::new();
        let owned_string = scores.string("a\0z").unwrap();
        let indexed_contiguous_string = scores
            .indexed_string(contiguous_string)
            .unwrap()
            .into_score();
        let indexed_persistent_string = scores
            .indexed_string(persistent_string)
            .unwrap()
            .into_score();
        let owned_bytes = scores.bytes(byte_bytes).unwrap();
        let indexed_contiguous_bytes = scores.indexed_bytes(contiguous_bytes).unwrap().into_score();
        let indexed_persistent_bytes = scores.indexed_bytes(persistent_bytes).unwrap().into_score();

        for indexed in [indexed_contiguous_string, indexed_persistent_string] {
            assert_eq!(scores.compare(owned_string, indexed), Ordering::Equal);
            assert_eq!(scores.to_vec(owned_string), scores.to_vec(indexed));
        }
        for indexed in [indexed_contiguous_bytes, indexed_persistent_bytes] {
            assert_eq!(scores.compare(owned_bytes, indexed), Ordering::Equal);
            assert_eq!(scores.to_vec(owned_bytes), scores.to_vec(indexed));
        }

        let later = scores.string("b").unwrap();
        assert_eq!(scores.compare(owned_string, later), Ordering::Less);
        assert_eq!(
            scores.compare(later, indexed_persistent_string),
            Ordering::Greater
        );
    }

    #[test]
    fn pre_scored_ground_helpers_preserve_the_ast_score_grammar() {
        let bytes = b"big\0integer";
        let indexed =
            IndexedByteRegion::from_protobuf_region(ProtobufRegion::copy_from_slice(bytes), SOURCE);
        let mut scores = CanonicalScoreArena::new();
        let expected = ground_score(&mut scores, GroundScore::BigInt(bytes)).unwrap();
        let atom = scores.indexed_bytes(indexed).unwrap();
        let actual =
            ground_score_pre_scored(&mut scores, PreScoredGroundScore::BigInt(atom)).unwrap();
        assert_eq!(scores.compare(expected, actual), Ordering::Equal);
        assert_eq!(scores.to_vec(expected), scores.to_vec(actual));
    }

    #[test]
    fn deeply_persistent_indexed_atom_and_score_cursor_are_stack_safe() {
        std::thread::Builder::new()
            .name("canonical-indexed-score-depth".to_string())
            .stack_size(128 * 1024)
            .spawn(|| {
                let mut backing = CanonicalBytesArena::new();
                let bytes = vec![b'x'; 20_000];
                let handle = append_indexed_bytes(&mut backing, &bytes);
                let indexed = IndexedByteRegion::from_arena(
                    &backing,
                    handle,
                    CanonicalByteProvenance::new(SOURCE, bytes.len()),
                );
                let mut scores = CanonicalScoreArena::new();
                let mut score = scores.indexed_bytes(indexed).unwrap().into_score();
                for _ in 0..20_000 {
                    score = scores.node(vec![score]).unwrap();
                }
                assert_eq!(scores.cursor(score).count(), scores.encoded_len(score));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
