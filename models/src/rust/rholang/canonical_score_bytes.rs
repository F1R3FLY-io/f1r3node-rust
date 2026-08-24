//! Arena-backed canonical score bytes for byte-native protobuf admission.
//!
//! A score is a DAG of atom and node handles. Parents retain handles rather
//! than flattening child bytes, so a depth-`D` protobuf term owns `O(D)` score
//! nodes instead of copying every descendant score into every ancestor. The
//! lending byte cursor is itself an explicit heap worklist and therefore has
//! no native-stack depth dependency.

use std::cmp::Ordering;
use std::fmt;

use prost::bytes::Bytes;

use super::canonical_semantics::CanonicalScoreAlgebra;
use super::protobuf_cursor::ProtobufRegion;

const INTEGER_PREFIX: &[u8] = &[1, 1];
const STRING_PREFIX: &[u8] = &[1, 2];
const BYTES_PREFIX: &[u8] = &[1, 3];
const TERMINATOR: &[u8] = &[0, 0];
const NODE_START: u8 = 2;
const NODE_END: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CanonicalScoreHandle(usize);

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

enum ScoreBytes {
    Owned(Box<[u8]>),
    Shared(Bytes),
    Region(ProtobufRegion),
}

impl ScoreBytes {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Shared(bytes) => bytes,
            Self::Region(region) => region.as_bytes(),
        }
    }
}

enum ScoreNode {
    Integer(i64),
    String(ScoreBytes),
    Bytes(ScoreBytes),
    Node(Box<[CanonicalScoreHandle]>),
}

struct ScoreEntry {
    node: ScoreNode,
    encoded_len: usize,
}

pub(crate) struct CanonicalScoreArena {
    entries: Vec<ScoreEntry>,
    backing: Option<Bytes>,
    zero_prefix: Vec<usize>,
}

impl CanonicalScoreArena {
    const ZERO_BLOCK_BYTES: usize = 256;

    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            backing: None,
            zero_prefix: Vec::new(),
        }
    }

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
            entries: Vec::new(),
            backing: Some(backing),
            zero_prefix,
        })
    }

    fn push(
        &mut self,
        node: ScoreNode,
        encoded_len: usize,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        let handle = CanonicalScoreHandle(self.entries.len());
        self.entries
            .try_reserve(1)
            .map_err(|_| CanonicalScoreArenaError::LengthOverflow)?;
        self.entries.push(ScoreEntry { node, encoded_len });
        Ok(handle)
    }

    fn terminated_len(bytes: &[u8]) -> Result<usize, CanonicalScoreArenaError> {
        let escapes = bytes.iter().filter(|&&byte| byte == 0).count();
        4usize
            .checked_add(bytes.len())
            .and_then(|length| length.checked_add(escapes))
            .ok_or(CanonicalScoreArenaError::LengthOverflow)
    }

    /// Retain an already-owned byte region as one canonical byte-array score
    /// atom. `Bytes::slice` makes this O(1), including an EPM1 snapshot cut from
    /// the owning protobuf image.
    pub(crate) fn shared_bytes(
        &mut self,
        bytes: Bytes,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        let encoded_len = Self::terminated_len(&bytes)?;
        self.push(ScoreNode::Bytes(ScoreBytes::Shared(bytes)), encoded_len)
    }

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
    pub(crate) fn shared_region(
        &mut self,
        region: ProtobufRegion,
    ) -> Result<CanonicalScoreHandle, CanonicalScoreArenaError> {
        let zeroes = self.region_zero_count(&region)?;
        let encoded_len = 4usize
            .checked_add(region.len())
            .and_then(|length| length.checked_add(zeroes))
            .ok_or(CanonicalScoreArenaError::LengthOverflow)?;
        self.push(ScoreNode::Bytes(ScoreBytes::Region(region)), encoded_len)
    }

    pub(crate) fn encoded_len(&self, score: CanonicalScoreHandle) -> usize {
        self.entries[score.0].encoded_len
    }

    pub(crate) fn cursor(&self, score: CanonicalScoreHandle) -> CanonicalScoreByteCursor<'_> {
        CanonicalScoreByteCursor::new(self, score)
    }

    pub(crate) fn compare(
        &self,
        left: CanonicalScoreHandle,
        right: CanonicalScoreHandle,
    ) -> Ordering {
        self.cursor(left).cmp(self.cursor(right))
    }

    pub(crate) fn equals_bytes(&self, score: CanonicalScoreHandle, expected: &[u8]) -> bool {
        self.cursor(score).eq(expected.iter().copied())
    }

    #[cfg(test)]
    fn to_vec(&self, score: CanonicalScoreHandle) -> Vec<u8> { self.cursor(score).collect() }
}

impl Default for CanonicalScoreArena {
    fn default() -> Self { Self::new() }
}

impl CanonicalScoreAlgebra for CanonicalScoreArena {
    type Score = CanonicalScoreHandle;
    type Error = CanonicalScoreArenaError;

    fn integer(&mut self, value: i64) -> Result<Self::Score, Self::Error> {
        self.push(ScoreNode::Integer(value), 10)
    }

    fn string(&mut self, value: &str) -> Result<Self::Score, Self::Error> {
        let encoded_len = Self::terminated_len(value.as_bytes())?;
        self.push(
            ScoreNode::String(ScoreBytes::Owned(Box::from(value.as_bytes()))),
            encoded_len,
        )
    }

    fn bytes(&mut self, value: &[u8]) -> Result<Self::Score, Self::Error> {
        let encoded_len = Self::terminated_len(value)?;
        self.push(
            ScoreNode::Bytes(ScoreBytes::Owned(Box::from(value))),
            encoded_len,
        )
    }

    fn node(&mut self, children: Vec<Self::Score>) -> Result<Self::Score, Self::Error> {
        let encoded_len = children.iter().try_fold(2usize, |length, child| {
            length
                .checked_add(self.encoded_len(*child))
                .ok_or(CanonicalScoreArenaError::LengthOverflow)
        })?;
        self.push(ScoreNode::Node(children.into_boxed_slice()), encoded_len)
    }
}

enum CursorOp<'a> {
    Score(CanonicalScoreHandle),
    Slice {
        bytes: &'a [u8],
        offset: usize,
    },
    Integer {
        bytes: [u8; 8],
        offset: usize,
    },
    Escaped {
        bytes: &'a [u8],
        offset: usize,
        pending_ff: bool,
    },
    Byte(u8),
}

pub(crate) struct CanonicalScoreByteCursor<'a> {
    arena: &'a CanonicalScoreArena,
    work: Vec<CursorOp<'a>>,
}

impl<'a> CanonicalScoreByteCursor<'a> {
    fn new(arena: &'a CanonicalScoreArena, root: CanonicalScoreHandle) -> Self {
        Self {
            arena,
            work: vec![CursorOp::Score(root)],
        }
    }

    fn push_slice(&mut self, bytes: &'a [u8]) {
        if !bytes.is_empty() {
            self.work.push(CursorOp::Slice { bytes, offset: 0 });
        }
    }
}

impl Iterator for CanonicalScoreByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                CursorOp::Score(handle) => match &self.arena.entries[handle.0].node {
                    ScoreNode::Integer(value) => {
                        let bytes = ((*value as u64) ^ (1u64 << 63)).to_be_bytes();
                        self.work.push(CursorOp::Integer { bytes, offset: 0 });
                        self.push_slice(INTEGER_PREFIX);
                    }
                    ScoreNode::String(bytes) => {
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
                CursorOp::Byte(byte) => return Some(byte),
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) { (0, None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::rholang::protobuf_cursor::CanonicalByteRange;

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
}
