//! Canonical, prefix-compressed byte snapshots for `EPathMap`.
//!
//! `EPM1` keeps the PathMap trie intact across every serialization boundary:
//! the key topology is PathMap's compact `ACTree03` arena and map values are a
//! separate positional body table encoded by the generated stack-safe protobuf PDA.
//! No `Vec<Par>` entry projection participates in either direction.

use std::fmt;
use std::ops::Range;

use pathmap::arena_compact::ArenaCompactTree;
use pathmap::zipper::{
    WriteZipperOwned, ZipperIteration, ZipperMoving, ZipperReadOnlyIteration, ZipperValues,
    ZipperWriting,
};
use pathmap::PathMap;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieActiveLineWorkSource, CanonicalCompactTrieCursor,
    CanonicalCompactTrieCursorSourcePosition, CanonicalCompactTrieCursorSourceStage,
    CanonicalCompactTrieIndexedTopologyCursor, CanonicalCompactTrieLineCommit,
    CanonicalCompactTrieLineDirection, CanonicalCompactTrieLineSpan,
    CanonicalCompactTriePreparedLineSpan, CanonicalCompactTrieTopologyCursor,
    CanonicalCompactTrieTopologyEvent, CanonicalCompactTrieValueSummary, CompactTrieError,
    CompactTrieTopologyEvent,
};
#[cfg(test)]
use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieValidatedPlanSource, CompactTrie,
};
use rspace_plus_plus::rspace::pathmap_mutation::{
    remove_value_preserving_path, retire_pathmap, BarrierProvider, DisposeValue,
};

use crate::rhoapi::Par;
use crate::rust::rholang::canonical_bytes_arena::CanonicalByteProvenance;
use crate::rust::rholang::canonical_source_tree::SourceTreeWindow;
use crate::rust::rholang::indexed_protobuf::{
    IndexedByteReadError, IndexedByteReadErrorKind, IndexedByteReadOperation, IndexedByteReader,
    IndexedByteRegion,
};
use crate::rust::rholang::protobuf_admission::validate_canonical_par_protobuf_indexed;
use crate::rust::rholang::protobuf_cursor::{CanonicalByteRange, ProtobufRegion};
use crate::rust::rholang::{protobuf_decoder, protobuf_encoder};

const EPM_MAGIC: &[u8; 4] = b"EPM1";
const EPM_VERSION: u8 = 1;

/// The homogeneous storage modes admitted by an `EPathMap`.
///
/// `Empty` is deliberately mode-neutral. The first insertion selects `Set` or
/// `Map`. Deleting the last value returns to `Empty` only when no explicit
/// value-free topology remains; otherwise the selected specialization stays
/// observable. A value can never contain a mixture of set-only and
/// value-bearing entries.
#[derive(Default)]
pub enum EPathMapRepr<T: Clone + Send + Sync + Unpin + 'static> {
    #[default]
    Empty,
    Set(PathMap<()>),
    Map(PathMap<T>),
}

impl<T: Clone + Send + Sync + Unpin + 'static> Clone for EPathMapRepr<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Set(map) => Self::Set(map.clone()),
            Self::Map(map) => Self::Map(map.clone()),
        }
    }
}

impl<T: Clone + Send + Sync + Unpin + 'static> EPathMapRepr<T> {
    #[inline]
    pub fn mode(&self) -> EPathMapMode {
        match self {
            Self::Empty => EPathMapMode::Empty,
            Self::Set(_) => EPathMapMode::Set,
            Self::Map(_) => EPathMapMode::Map,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Set(map) => map.is_empty(),
            Self::Map(map) => map.is_empty(),
        }
    }

    #[inline]
    pub fn as_set(&self) -> Option<&PathMap<()>> {
        match self {
            Self::Set(map) => Some(map),
            Self::Empty | Self::Map(_) => None,
        }
    }

    #[inline]
    pub fn as_map(&self) -> Option<&PathMap<T>> {
        match self {
            Self::Map(map) => Some(map),
            Self::Empty | Self::Set(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EPathMapMode {
    Empty = 0,
    Set = 1,
    Map = 2,
}

impl TryFrom<u8> for EPathMapMode {
    type Error = TrieCodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Empty),
            1 => Ok(Self::Set),
            2 => Ok(Self::Map),
            other => Err(TrieCodecError::new(format!(
                "unknown EPathMap mode {other}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrieCodecError {
    message: String,
    reported: Option<CanonicalByteProvenance>,
    failure: Option<CanonicalByteProvenance>,
}

impl TrieCodecError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            reported: None,
            failure: None,
        }
    }

    fn located(
        message: impl Into<String>,
        reported: CanonicalByteProvenance,
        failure: CanonicalByteProvenance,
    ) -> Self {
        Self {
            message: message.into(),
            reported: Some(reported),
            failure: Some(failure),
        }
    }

    #[cfg(test)]
    pub(crate) const fn reported_provenance(&self) -> Option<CanonicalByteProvenance> {
        self.reported
    }

    #[cfg(test)]
    pub(crate) const fn failure_provenance(&self) -> Option<CanonicalByteProvenance> {
        self.failure
    }
}

impl fmt::Display for TrieCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.message) }
}

impl std::error::Error for TrieCodecError {}

impl From<CompactTrieError> for TrieCodecError {
    fn from(error: CompactTrieError) -> Self { Self::new(error.to_string()) }
}

#[allow(deprecated)]
impl From<TrieCodecError> for prost::DecodeError {
    fn from(error: TrieCodecError) -> Self {
        prost::DecodeError::new(format!("EPathMap trie_snapshot: {error}"))
    }
}

/// One canonical map key discovered directly in an ACTree03 arena.
///
/// A compressed line-only path remains a subrange of the input snapshot.  A
/// branched path is assembled only at the value boundary because its edge
/// bytes live in branch-node masks rather than in one contiguous input range.
/// This distinction is what lets a nested one-entry map chain retain one
/// backing allocation instead of copying every suffix.
pub(crate) enum CanonicalSnapshotKey {
    Relative(Range<usize>),
    Owned(Vec<u8>),
}

/// Canonical EPM1 structure needed by the enclosing canonical-path worklist.
pub(crate) struct CanonicalSnapshotInspection {
    pub(crate) mode: EPathMapMode,
    pub(crate) keys: Vec<CanonicalSnapshotKey>,
    pub(crate) value_bodies: Vec<Range<usize>>,
}

/// Canonical EPM1 framing needed by byte-native consumers that retain the
/// compact trie unchanged and descend only into map values.
///
/// `prefix` contains the magic, version, mode, ACTree03 arena, and declared
/// value count. Replacing every following value record with an empty protobuf
/// body therefore yields a valid shallow shell without materializing a key or
/// a `PathMap<Par>`.
pub(crate) struct CanonicalSnapshotValueRegions {
    pub(crate) mode: EPathMapMode,
    pub(crate) prefix: Range<usize>,
    pub(crate) value_bodies: Vec<Range<usize>>,
}

/// One event in the logical zipper walk of a canonical EPM1 trie.
///
/// This is intentionally independent of ACTree03's physical compression.  A
/// consumer sees exactly the root/value, byte-edge/value, and postorder-leave
/// sequence produced by a live `PathMap` read zipper, without reconstructing a
/// temporary map or collecting its endpoints.
pub(crate) use rspace_plus_plus::rspace::pathmap_codec::CompactTrieTopologyEvent as CanonicalSnapshotTopologyEvent;

struct CanonicalSnapshotFraming<'a> {
    mode: EPathMapMode,
    arena_start: usize,
    arena: &'a [u8],
    value_count: usize,
    value_table_start: usize,
    value_bodies: Vec<Range<usize>>,
}

/// Constant-auxiliary-space routing information from the fixed EPM1 prefix.
///
/// This is deliberately *not* a canonicality proof. It skips the compact ACT
/// arena by its declared byte length and reads the following map-value count;
/// callers may use it only to decide whether a more specialized parser is
/// worth attempting. Every accepted route must still pass the complete value
/// framing and ACT topology validation in [`inspect_canonical_snapshot`] or
/// [`inspect_canonical_snapshot_value_regions`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EPathMapSnapshotRouteHint {
    mode: EPathMapMode,
    map_value_count: usize,
}

impl EPathMapSnapshotRouteHint {
    #[inline]
    pub(crate) const fn mode(self) -> EPathMapMode { self.mode }

    #[inline]
    pub(crate) const fn map_value_count(self) -> usize { self.map_value_count }
}

struct CanonicalSnapshotPrefix<'a> {
    mode: EPathMapMode,
    arena_start: usize,
    arena: &'a [u8],
    value_count: usize,
    value_table_start: usize,
}

#[cfg(test)]
pub(crate) struct CanonicalEpm1Entry<'a> {
    key: &'a [u8],
    value_body: Option<Range<usize>>,
}

#[cfg(test)]
impl CanonicalEpm1Entry<'_> {
    pub(crate) fn key(&self) -> &[u8] { self.key }

    pub(crate) fn value_body(&self) -> Option<Range<usize>> { self.value_body.clone() }
}

#[cfg(test)]
fn validate_epm1_value_table(
    bytes: &[u8],
    value_count: usize,
    value_table_start: usize,
) -> Result<(), TrieCodecError> {
    let mut value_cursor = value_table_start;
    for _ in 0..value_count {
        let len = read_len(bytes, &mut value_cursor, "map value length")?;
        take(bytes, &mut value_cursor, len, "map value payload")?;
    }
    if value_cursor != bytes.len() {
        return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
    }
    Ok(())
}

#[cfg(test)]
fn validate_typed_epm1_trie<B>(
    mode: EPathMapMode,
    value_count: usize,
    trie: &CanonicalCompactTrieCursor<B>,
) -> Result<(), TrieCodecError>
where
    B: AsRef<[u8]>,
{
    validate_canonical_act_summary(mode, value_count, trie.is_empty(), trie.value_summary())
}

#[cfg(test)]
pub(crate) struct CanonicalEpm1Cursor<'a> {
    bytes: &'a [u8],
    mode: EPathMapMode,
    value_count: usize,
    value_cursor: usize,
    remaining_values: usize,
    trie: Option<CanonicalCompactTrieCursor<&'a [u8]>>,
}

#[cfg(test)]
impl<'a> CanonicalEpm1Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, TrieCodecError> {
        let prefix = parse_canonical_snapshot_prefix(bytes)?;
        validate_epm1_value_table(bytes, prefix.value_count, prefix.value_table_start)?;

        let trie = match prefix.mode {
            EPathMapMode::Empty => {
                if !prefix.arena.is_empty() || prefix.value_count != 0 {
                    return Err(TrieCodecError::new(
                        "neutral empty EPathMap carries trie or values",
                    ));
                }
                None
            }
            EPathMapMode::Set | EPathMapMode::Map => {
                let mut trie = CanonicalCompactTrieCursor::new(prefix.arena)?;
                validate_typed_epm1_trie(prefix.mode, prefix.value_count, &mut trie)?;
                Some(trie)
            }
        };

        Ok(Self {
            bytes,
            mode: prefix.mode,
            value_count: prefix.value_count,
            value_cursor: prefix.value_table_start,
            remaining_values: prefix.value_count,
            trie,
        })
    }

    pub(crate) const fn mode(&self) -> EPathMapMode { self.mode }

    pub(crate) const fn value_count(&self) -> usize { self.value_count }

    pub(crate) fn next_entry(&mut self) -> Option<CanonicalEpm1Entry<'_>> {
        let trie = self.trie.as_mut()?;
        let value_body = if self.mode == EPathMapMode::Map {
            if self.remaining_values == 0 {
                return None;
            }
            let len = read_len(self.bytes, &mut self.value_cursor, "map value length")
                .expect("constructor validated every map value length");
            let start = self.value_cursor;
            take(self.bytes, &mut self.value_cursor, len, "map value payload")
                .expect("constructor validated every map value body");
            self.remaining_values -= 1;
            Some(start..self.value_cursor)
        } else {
            None
        };
        let endpoint = trie.next_value_endpoint()?;
        Some(CanonicalEpm1Entry {
            key: endpoint.path,
            value_body,
        })
    }
}

#[cfg(test)]
pub(crate) struct OwnedCanonicalEpm1Entry<'a> {
    key: &'a [u8],
    value_body_range: Option<Range<usize>>,
    value_body: Option<Bytes>,
}

#[cfg(test)]
impl OwnedCanonicalEpm1Entry<'_> {
    pub(crate) fn key(&self) -> &[u8] { self.key }

    pub(crate) fn value_body_range(&self) -> Option<Range<usize>> { self.value_body_range.clone() }

    pub(crate) fn value_body(&self) -> Option<Bytes> { self.value_body.clone() }
}

#[cfg(test)]
pub(crate) struct OwnedCanonicalEpm1Cursor {
    bytes: Bytes,
    mode: EPathMapMode,
    value_count: usize,
    value_cursor: usize,
    remaining_values: usize,
    trie: Option<CanonicalCompactTrieCursor<Bytes>>,
}

#[cfg(test)]
impl OwnedCanonicalEpm1Cursor {
    pub(crate) fn new(bytes: Bytes) -> Result<Self, TrieCodecError> {
        let (mode, arena, value_count, value_table_start) = {
            let prefix = parse_canonical_snapshot_prefix(bytes.as_ref())?;
            let arena_end = prefix
                .arena_start
                .checked_add(prefix.arena.len())
                .ok_or_else(|| TrieCodecError::new("EPM1 arena range overflows usize"))?;
            (
                prefix.mode,
                prefix.arena_start..arena_end,
                prefix.value_count,
                prefix.value_table_start,
            )
        };
        validate_epm1_value_table(bytes.as_ref(), value_count, value_table_start)?;

        let trie = match mode {
            EPathMapMode::Empty => {
                if !arena.is_empty() || value_count != 0 {
                    return Err(TrieCodecError::new(
                        "neutral empty EPathMap carries trie or values",
                    ));
                }
                None
            }
            EPathMapMode::Set | EPathMapMode::Map => {
                let mut trie = CanonicalCompactTrieCursor::from_backing(bytes.slice(arena))?;
                validate_typed_epm1_trie(mode, value_count, &mut trie)?;
                Some(trie)
            }
        };

        Ok(Self {
            bytes,
            mode,
            value_count,
            value_cursor: value_table_start,
            remaining_values: value_count,
            trie,
        })
    }

    pub(crate) const fn mode(&self) -> EPathMapMode { self.mode }

    pub(crate) const fn value_count(&self) -> usize { self.value_count }

    pub(crate) fn next_entry(&mut self) -> Option<OwnedCanonicalEpm1Entry<'_>> {
        let trie = self.trie.as_mut()?;
        let value_body_range = if self.mode == EPathMapMode::Map {
            if self.remaining_values == 0 {
                return None;
            }
            let len = read_len(
                self.bytes.as_ref(),
                &mut self.value_cursor,
                "map value length",
            )
            .expect("constructor validated every map value length");
            let start = self.value_cursor;
            take(
                self.bytes.as_ref(),
                &mut self.value_cursor,
                len,
                "map value payload",
            )
            .expect("constructor validated every map value body");
            self.remaining_values -= 1;
            Some(start..self.value_cursor)
        } else {
            None
        };
        let value_body = value_body_range
            .as_ref()
            .map(|range| self.bytes.slice(range.clone()));
        let endpoint = trie.next_value_endpoint()?;
        Some(OwnedCanonicalEpm1Entry {
            key: endpoint.path,
            value_body_range,
            value_body,
        })
    }
}

fn indexed_read_error(label: &str, error: IndexedByteReadError) -> TrieCodecError {
    let message = match error.kind {
        IndexedByteReadErrorKind::UnexpectedEof { .. }
        | IndexedByteReadErrorKind::TruncatedVarint => format!("truncated {label}"),
        IndexedByteReadErrorKind::NonMinimalVarint => format!("non-canonical {label}"),
        IndexedByteReadErrorKind::VarintOverflow => format!("overflowing {label}"),
        IndexedByteReadErrorKind::InvalidUtf8 | IndexedByteReadErrorKind::TruncatedUtf8 => {
            format!("invalid {label}")
        }
    };
    TrieCodecError::located(message, error.start.physical, error.failure.physical)
}

fn read_indexed_len(
    view: &IndexedByteRegion<'_>,
    reader: &mut IndexedByteReader<'_>,
    label: &str,
) -> Result<usize, TrieCodecError> {
    let start = reader.offset();
    let value = reader
        .read_canonical_varint()
        .map_err(|error| indexed_read_error(label, error))?;
    usize::try_from(value).map_err(|_| {
        let reported = view
            .position_at(start)
            .expect("an indexed varint start belongs to its view")
            .physical;
        let failure = view
            .position_at(reader.offset())
            .expect("an indexed varint end belongs to its view")
            .physical;
        TrieCodecError::located(format!("{label} overflows usize"), reported, failure)
    })
}

fn take_indexed_region<'a>(
    view: &IndexedByteRegion<'a>,
    reader: &mut IndexedByteReader<'a>,
    len: usize,
    label: &str,
) -> Result<IndexedByteRegion<'a>, TrieCodecError> {
    let start = reader.offset();
    let Some(end) = start.checked_add(len) else {
        let provenance = view
            .position_at(start)
            .expect("an indexed region start belongs to its view")
            .physical;
        return Err(TrieCodecError::located(
            format!("{label} length overflow"),
            provenance,
            provenance,
        ));
    };
    reader
        .skip_exact(len, IndexedByteReadOperation::Byte)
        .map_err(|error| indexed_read_error(label, error))?;
    view.subview(CanonicalByteRange { start, end })
        .ok_or_else(|| {
            let provenance = view
                .position_at(start)
                .expect("an indexed region start belongs to its view")
                .physical;
            TrieCodecError::located(format!("truncated {label}"), provenance, provenance)
        })
}

fn indexed_act_error(arena: &IndexedByteRegion<'_>, error: CompactTrieError) -> TrieCodecError {
    let logical_offset = error.source_offset().unwrap_or(arena.len());
    let position = arena
        .position_at(logical_offset.min(arena.len()))
        .expect("a bounded ACT error position belongs to its indexed arena");
    TrieCodecError::located(error.to_string(), position.physical, position.physical)
}

fn validate_indexed_canonical_act_summary(
    mode: EPathMapMode,
    value_count: usize,
    value_count_provenance: CanonicalByteProvenance,
    trie: &CanonicalCompactTrieIndexedTopologyCursor<IndexedByteRegion<'_>>,
) -> Result<(), TrieCodecError> {
    let arena = trie.source();
    if trie.is_empty() {
        let provenance = arena.end_position().physical;
        return Err(TrieCodecError::located(
            "typed EPathMap mode has empty ACTree topology; use neutral empty mode",
            provenance,
            provenance,
        ));
    }
    let summary = trie.value_summary();
    match mode {
        EPathMapMode::Set => {
            if let Some((source_offset, _marker)) = summary.first_nonzero() {
                let provenance = arena
                    .position_at(source_offset.min(arena.len()))
                    .expect("a validated ACT value offset belongs to its indexed arena")
                    .physical;
                return Err(TrieCodecError::located(
                    "set ACTree value marker is not zero",
                    provenance,
                    provenance,
                ));
            }
        }
        EPathMapMode::Map => {
            if let Some((source_offset, marker)) = summary.first_nonzero() {
                let provenance = arena
                    .position_at(source_offset.min(arena.len()))
                    .expect("a validated ACT value offset belongs to its indexed arena")
                    .physical;
                return Err(TrieCodecError::located(
                    format!("map ACTree value marker {marker} is not zero"),
                    provenance,
                    provenance,
                ));
            }
            if summary.value_count() != value_count {
                return Err(TrieCodecError::located(
                    format!(
                        "map ACTree references {} values but EPM1 declares {value_count}",
                        summary.value_count()
                    ),
                    value_count_provenance,
                    value_count_provenance,
                ));
            }
        }
        EPathMapMode::Empty => unreachable!("empty mode has no indexed ACT cursor"),
    }
    Ok(())
}

struct IndexedCanonicalSnapshotPrefix<'a> {
    mode: EPathMapMode,
    mode_provenance: CanonicalByteProvenance,
    arena: IndexedByteRegion<'a>,
    value_count: usize,
    value_count_provenance: CanonicalByteProvenance,
    value_table_start: usize,
    bytes_read: usize,
    bytes_traversed: usize,
}

fn parse_indexed_canonical_snapshot_prefix<'a>(
    snapshot: &IndexedByteRegion<'a>,
) -> Result<IndexedCanonicalSnapshotPrefix<'a>, TrieCodecError> {
    let mut reader = snapshot.reader();
    for (index, expected) in EPM_MAGIC.iter().copied().enumerate() {
        let byte = reader
            .read_byte()
            .map_err(|error| indexed_read_error("EPM1 magic", error))?;
        if byte != expected {
            let provenance = snapshot
                .position_at(index)
                .expect("an EPM1 magic byte belongs to its snapshot")
                .physical;
            return Err(TrieCodecError::located(
                "invalid EPM1 magic",
                provenance,
                provenance,
            ));
        }
    }

    let version_offset = reader.offset();
    let version = reader
        .read_byte()
        .map_err(|error| indexed_read_error("EPM1 version", error))?;
    if version != EPM_VERSION {
        let provenance = snapshot
            .position_at(version_offset)
            .expect("the EPM1 version belongs to its snapshot")
            .physical;
        return Err(TrieCodecError::located(
            format!("unsupported EPM1 version {version}"),
            provenance,
            provenance,
        ));
    }

    let mode_offset = reader.offset();
    let mode_byte = reader
        .read_byte()
        .map_err(|error| indexed_read_error("EPM1 mode", error))?;
    let mode_provenance = snapshot
        .position_at(mode_offset)
        .expect("the EPM1 mode belongs to its snapshot")
        .physical;
    let mode = EPathMapMode::try_from(mode_byte).map_err(|error| {
        TrieCodecError::located(error.to_string(), mode_provenance, mode_provenance)
    })?;

    let arena_len = read_indexed_len(snapshot, &mut reader, "ACTree03 length")?;
    let arena = take_indexed_region(snapshot, &mut reader, arena_len, "ACTree03 payload")?;
    let value_count_offset = reader.offset();
    let value_count = read_indexed_len(snapshot, &mut reader, "map value count")?;
    let value_count_provenance = snapshot
        .position_at(value_count_offset)
        .expect("the EPM1 value count belongs to its snapshot")
        .physical;
    let value_table_start = reader.offset();
    let remaining = snapshot.len() - value_table_start;
    if value_count > remaining {
        return Err(TrieCodecError::located(
            format!(
                "map value count {value_count} exceeds the {remaining}-byte payload lower bound"
            ),
            value_count_provenance,
            value_count_provenance,
        ));
    }
    let bytes_read = reader.bytes_read();
    let bytes_traversed = reader.bytes_traversed();

    Ok(IndexedCanonicalSnapshotPrefix {
        mode,
        mode_provenance,
        arena,
        value_count,
        value_count_provenance,
        value_table_start,
        bytes_read,
        bytes_traversed,
    })
}

#[derive(Clone, Debug)]
pub(crate) enum IndexedCanonicalEpm1TopologyEvent<'a> {
    Value {
        has_value: bool,
        value_body_range: Option<Range<usize>>,
        value_body: Option<IndexedByteRegion<'a>>,
        source_offset: usize,
        provenance: CanonicalByteProvenance,
    },
    Edge {
        byte: u8,
        provenance: CanonicalByteProvenance,
    },
    Leave {
        provenance: CanonicalByteProvenance,
    },
}

/// One actual cursor call and its on-demand source receipt. The Map receipt
/// cannot be paired with an event or prestate supplied by the caller.
pub(crate) struct IndexedCanonicalEpm1TopologySourceEvent<'a> {
    pub(crate) event: IndexedCanonicalEpm1TopologyEvent<'a>,
    pub(crate) map_value_receipt: Option<IndexedCanonicalEpm1MapValueSourceReceipt>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IndexedCanonicalEpm1TopologyStats {
    pub(crate) prefix_bytes_read: usize,
    pub(crate) prefix_bytes_traversed: usize,
    pub(crate) value_table_bytes_read: usize,
    pub(crate) value_table_bytes_traversed: usize,
    pub(crate) topology_events: usize,
    pub(crate) value_records: usize,
    pub(crate) terminal_checks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexedCanonicalEpm1TerminalSourceImage {
    pub(crate) mode: EPathMapMode,
    pub(crate) declared_value_count: usize,
    pub(crate) map_values_seen: usize,
    pub(crate) event_index: usize,
    pub(crate) value_table_offset: usize,
    pub(crate) snapshot_len: usize,
    pub(crate) terminal_checks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexedCanonicalEpm1ActiveSourceImage {
    pub(crate) mode: EPathMapMode,
    pub(crate) declared_value_count: usize,
    pub(crate) map_values_seen: usize,
    pub(crate) event_index: usize,
    pub(crate) value_table_offset: usize,
    pub(crate) snapshot_len: usize,
}

/// One successful Map Value event, assembled from pre/post cursor observations
/// and its transient event. Every range is relative to the EPM1 snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexedCanonicalEpm1MapValueSourceReceipt {
    pub(crate) act_before: CanonicalCompactTrieCursorSourcePosition,
    pub(crate) act_after: Option<CanonicalCompactTrieCursorSourcePosition>,
    pub(crate) act_source_offset: usize,
    pub(crate) act_source: CanonicalByteProvenance,
    pub(crate) event_index_before: usize,
    pub(crate) event_index_after: usize,
    pub(crate) map_values_seen_before: usize,
    pub(crate) map_values_seen_after: usize,
    pub(crate) pre_value_table_offset: usize,
    pub(crate) length_prefix_start: usize,
    pub(crate) length_prefix_end: usize,
    pub(crate) body_len: usize,
    pub(crate) body_range: CanonicalByteRange,
    pub(crate) post_value_table_offset: usize,
    pub(crate) snapshot_len: usize,
}

fn map_value_act_successor_position(
    before: CanonicalCompactTrieCursorSourcePosition,
) -> Option<CanonicalCompactTrieCursorSourcePosition> {
    match before.stage {
        CanonicalCompactTrieCursorSourceStage::Value
            if before.line_start.is_none() && before.line_end.is_none() =>
        {
            Some(CanonicalCompactTrieCursorSourcePosition {
                stage: CanonicalCompactTrieCursorSourceStage::Children,
                ..before
            })
        }
        CanonicalCompactTrieCursorSourceStage::LineValue => {
            let line_len = before.line_end?.checked_sub(before.line_start?)?;
            let line_index = before.line_index.checked_add(1)?;
            if line_index != line_len {
                return None;
            }
            Some(CanonicalCompactTrieCursorSourcePosition {
                line_index,
                stage: CanonicalCompactTrieCursorSourceStage::Children,
                ..before
            })
        }
        _ => None,
    }
}

#[derive(Clone)]
enum IndexedCanonicalEpm1Terminal {
    Active,
    Done,
    Failed(TrieCodecError),
}

pub(crate) struct IndexedCanonicalEpm1TopologyCursor<'a> {
    mode: EPathMapMode,
    value_count: usize,
    length: usize,
    value_table_start: usize,
    value_table: IndexedByteRegion<'a>,
    value_reader: IndexedByteReader<'a>,
    value_count_provenance: CanonicalByteProvenance,
    empty_state: u8,
    empty_provenance: CanonicalByteProvenance,
    trie: Option<CanonicalCompactTrieIndexedTopologyCursor<IndexedByteRegion<'a>>>,
    prefix_bytes_read: usize,
    prefix_bytes_traversed: usize,
    topology_events: usize,
    value_records: usize,
    terminal_checks: usize,
    terminal: IndexedCanonicalEpm1Terminal,
}

pub(crate) struct IndexedCanonicalEpm1LineSpan<'cursor, 'src> {
    inner: CanonicalCompactTrieLineSpan<'cursor, IndexedByteRegion<'src>>,
    topology_events: &'cursor mut usize,
}

pub(crate) struct IndexedCanonicalEpm1PreparedLineSpan<'cursor, 'src> {
    inner: CanonicalCompactTriePreparedLineSpan<'cursor, IndexedByteRegion<'src>>,
    topology_events: &'cursor mut usize,
    next_events: usize,
    last_source: CanonicalByteProvenance,
}

impl<'cursor, 'src> IndexedCanonicalEpm1LineSpan<'cursor, 'src> {
    pub(crate) fn source_range(&self) -> Range<usize> { self.inner.source_range() }

    pub(crate) fn direction(&self) -> CanonicalCompactTrieLineDirection { self.inner.direction() }

    pub(crate) fn capacity(&self) -> usize {
        let available_events = usize::MAX - *self.topology_events;
        self.source_range().len().min(match self.direction() {
            CanonicalCompactTrieLineDirection::Forward => available_events / 2,
            CanonicalCompactTrieLineDirection::Unwind => available_events,
        })
    }

    pub(crate) fn prepare(
        self,
        width: usize,
    ) -> Option<IndexedCanonicalEpm1PreparedLineSpan<'cursor, 'src>> {
        if width == 0 || width > self.capacity() {
            return None;
        }
        let inner = self.inner.prepare(width)?;
        let next_events = self.topology_events.checked_add(inner.expanded_events())?;
        let last_source = inner
            .source()
            .position_at(inner.last_source_offset())?
            .physical;
        Some(IndexedCanonicalEpm1PreparedLineSpan {
            inner,
            topology_events: self.topology_events,
            next_events,
            last_source,
        })
    }
}

impl IndexedCanonicalEpm1PreparedLineSpan<'_, '_> {
    pub(crate) fn source_range(&self) -> Range<usize> { self.inner.source_range() }

    pub(crate) fn commit(self) -> (CanonicalByteProvenance, CanonicalCompactTrieLineCommit) {
        let publication = self.inner.commit();
        *self.topology_events = self.next_events;
        (self.last_source, publication)
    }
}

impl<'a> IndexedCanonicalEpm1TopologyCursor<'a> {
    pub(crate) fn source_window(&self) -> Option<SourceTreeWindow<'a>> {
        self.trie.as_ref()?.source().source_window()
    }

    pub(crate) fn line_span(&mut self) -> Option<IndexedCanonicalEpm1LineSpan<'_, 'a>> {
        if !matches!(self.terminal, IndexedCanonicalEpm1Terminal::Active)
            || self.mode == EPathMapMode::Empty
        {
            return None;
        }
        Some(IndexedCanonicalEpm1LineSpan {
            inner: self.trie.as_mut()?.line_span()?,
            topology_events: &mut self.topology_events,
        })
    }

    pub(crate) fn new(snapshot: IndexedByteRegion<'a>) -> Result<Self, TrieCodecError> {
        let prefix = parse_indexed_canonical_snapshot_prefix(&snapshot)?;
        let value_table = snapshot
            .subview(CanonicalByteRange {
                start: prefix.value_table_start,
                end: snapshot.len(),
            })
            .expect("an EPM1 value table starts inside its snapshot");

        match prefix.mode {
            EPathMapMode::Empty => {
                if !prefix.arena.is_empty() || prefix.value_count != 0 {
                    let failure = if !prefix.arena.is_empty() {
                        prefix.arena.position_at(0).unwrap().physical
                    } else {
                        prefix.value_count_provenance
                    };
                    return Err(TrieCodecError::located(
                        "neutral empty EPathMap carries trie or values",
                        prefix.mode_provenance,
                        failure,
                    ));
                }
                if !value_table.is_empty() {
                    let provenance = value_table.position_at(0).unwrap().physical;
                    return Err(TrieCodecError::located(
                        "trailing bytes after EPM1 payload",
                        provenance,
                        provenance,
                    ));
                }
            }
            EPathMapMode::Set => {
                if prefix.value_count != 0 {
                    return Err(TrieCodecError::located(
                        "set EPathMap carries map values",
                        prefix.value_count_provenance,
                        prefix.value_count_provenance,
                    ));
                }
                if !value_table.is_empty() {
                    let provenance = value_table.position_at(0).unwrap().physical;
                    return Err(TrieCodecError::located(
                        "trailing bytes after EPM1 payload",
                        provenance,
                        provenance,
                    ));
                }
            }
            EPathMapMode::Map => {}
        }
        let trie = if prefix.mode == EPathMapMode::Empty {
            None
        } else {
            let arena = prefix.arena;
            let trie = CanonicalCompactTrieIndexedTopologyCursor::from_source(arena.clone())
                .map_err(|error| indexed_act_error(&arena, error))?;
            validate_indexed_canonical_act_summary(
                prefix.mode,
                prefix.value_count,
                prefix.value_count_provenance,
                &trie,
            )?;
            Some(trie)
        };
        let value_reader = value_table.reader();

        Ok(Self {
            mode: prefix.mode,
            value_count: prefix.value_count,
            length: 0,
            value_table_start: prefix.value_table_start,
            value_table,
            value_reader,
            value_count_provenance: prefix.value_count_provenance,
            empty_state: 0,
            empty_provenance: prefix.mode_provenance,
            trie,
            prefix_bytes_read: prefix.bytes_read,
            prefix_bytes_traversed: prefix.bytes_traversed,
            topology_events: 0,
            value_records: 0,
            terminal_checks: 0,
            terminal: IndexedCanonicalEpm1Terminal::Active,
        })
    }

    pub(crate) const fn mode(&self) -> EPathMapMode { self.mode }

    #[cfg(test)]
    pub(crate) const fn value_count(&self) -> usize { self.value_count }

    #[cfg(test)]
    pub(crate) const fn length(&self) -> usize { self.length }

    pub(crate) fn stats(&self) -> IndexedCanonicalEpm1TopologyStats {
        IndexedCanonicalEpm1TopologyStats {
            prefix_bytes_read: self.prefix_bytes_read,
            prefix_bytes_traversed: self.prefix_bytes_traversed,
            value_table_bytes_read: self.value_reader.bytes_read(),
            value_table_bytes_traversed: self.value_reader.bytes_traversed(),
            topology_events: self.topology_events,
            value_records: self.value_records,
            terminal_checks: self.terminal_checks,
        }
    }

    pub(crate) fn topology_source_position(
        &self,
    ) -> Option<CanonicalCompactTrieCursorSourcePosition> {
        self.trie.as_ref()?.source_position()
    }

    #[cfg(test)]
    pub(crate) fn validated_act_plan_source(
        &self,
    ) -> Option<CanonicalCompactTrieValidatedPlanSource<'_, IndexedByteRegion<'a>>> {
        Some(self.trie.as_ref()?.validated_plan_source())
    }

    fn map_value_source_receipt(
        &self,
        before: IndexedCanonicalEpm1ActiveSourceImage,
        act_before: CanonicalCompactTrieCursorSourcePosition,
        event: &IndexedCanonicalEpm1TopologyEvent<'a>,
    ) -> Option<IndexedCanonicalEpm1MapValueSourceReceipt> {
        let IndexedCanonicalEpm1TopologyEvent::Value {
            has_value: true,
            value_body_range: Some(range),
            value_body: Some(body),
            source_offset,
            provenance,
        } = event
        else {
            return None;
        };
        let after = self.active_source_image()?;
        if before.mode != EPathMapMode::Map
            || after.mode != before.mode
            || after.declared_value_count != before.declared_value_count
            || after.snapshot_len != before.snapshot_len
            || before.event_index.checked_add(1)? != after.event_index
            || before.map_values_seen.checked_add(1)? != after.map_values_seen
            || before.value_table_offset >= range.start
            || range.start > range.end
            || range.end != after.value_table_offset
            || range.end > after.snapshot_len
            || !matches!(
                act_before.stage,
                CanonicalCompactTrieCursorSourceStage::Value
                    | CanonicalCompactTrieCursorSourceStage::LineValue
            )
            || (act_before.stage == CanonicalCompactTrieCursorSourceStage::Value
                && act_before.node_id != *source_offset)
        {
            return None;
        }
        let relative_start = range.start.checked_sub(self.value_table_start)?;
        let relative_end = range.end.checked_sub(self.value_table_start)?;
        let selected_body = self.value_table.subview(CanonicalByteRange {
            start: relative_start,
            end: relative_end,
        })?;
        // `next_event` already obtained `provenance` from this ACT source offset.
        // Re-reading its indexed position here would duplicate a source lookup.
        if !body.same_view(&selected_body)
            || self.topology_source_position()
                != Some(map_value_act_successor_position(act_before)?)
        {
            return None;
        }
        Some(IndexedCanonicalEpm1MapValueSourceReceipt {
            act_before,
            act_after: self.topology_source_position(),
            act_source_offset: *source_offset,
            act_source: *provenance,
            event_index_before: before.event_index,
            event_index_after: after.event_index,
            map_values_seen_before: before.map_values_seen,
            map_values_seen_after: after.map_values_seen,
            pre_value_table_offset: before.value_table_offset,
            length_prefix_start: before.value_table_offset,
            length_prefix_end: range.start,
            body_len: range.len(),
            body_range: CanonicalByteRange {
                start: range.start,
                end: range.end,
            },
            post_value_table_offset: after.value_table_offset,
            snapshot_len: after.snapshot_len,
        })
    }

    pub(crate) fn active_line_work_source(
        &self,
    ) -> Option<CanonicalCompactTrieActiveLineWorkSource> {
        if !matches!(self.terminal, IndexedCanonicalEpm1Terminal::Active)
            || self.mode == EPathMapMode::Empty
        {
            return None;
        }
        self.trie
            .as_ref()?
            .active_line_work_source(self.topology_events)
    }

    pub(crate) fn active_source_image(&self) -> Option<IndexedCanonicalEpm1ActiveSourceImage> {
        if !matches!(self.terminal, IndexedCanonicalEpm1Terminal::Active)
            || self.terminal_checks != 0
            || self.value_records > self.value_count
        {
            return None;
        }
        let value_table_offset = self
            .value_table_start
            .checked_add(self.value_reader.offset())?;
        let snapshot_len = self.value_table_start.checked_add(self.value_table.len())?;
        if value_table_offset > snapshot_len {
            return None;
        }
        Some(IndexedCanonicalEpm1ActiveSourceImage {
            mode: self.mode,
            declared_value_count: self.value_count,
            map_values_seen: self.value_records,
            event_index: self.topology_events,
            value_table_offset,
            snapshot_len,
        })
    }

    pub(crate) fn terminal_source_image(&self) -> Option<IndexedCanonicalEpm1TerminalSourceImage> {
        if !matches!(self.terminal, IndexedCanonicalEpm1Terminal::Done)
            || self.terminal_checks != 1
            || !self.value_reader.is_empty()
            || (self.mode == EPathMapMode::Map
                && (self.length != self.value_count || self.value_records != self.length))
            || (self.mode != EPathMapMode::Map
                && (self.value_records != 0 || self.value_count != 0))
        {
            return None;
        }
        let value_table_offset = self
            .value_table_start
            .checked_add(self.value_reader.offset())?;
        let snapshot_len = self.value_table_start.checked_add(self.value_table.len())?;
        if value_table_offset != snapshot_len {
            return None;
        }
        Some(IndexedCanonicalEpm1TerminalSourceImage {
            mode: self.mode,
            declared_value_count: self.value_count,
            map_values_seen: self.value_records,
            event_index: self.topology_events,
            value_table_offset,
            snapshot_len,
            terminal_checks: self.terminal_checks,
        })
    }

    fn latch_error<T>(&mut self, error: TrieCodecError) -> Result<T, TrieCodecError> {
        self.terminal = IndexedCanonicalEpm1Terminal::Failed(error.clone());
        Err(error)
    }

    fn finish_stream(
        &mut self,
    ) -> Result<Option<IndexedCanonicalEpm1TopologyEvent<'a>>, TrieCodecError> {
        self.terminal_checks += 1;
        if self.mode == EPathMapMode::Map && self.length != self.value_count {
            return self.latch_error(TrieCodecError::located(
                format!(
                    "map ACTree references {} values but EPM1 declares {}",
                    self.length, self.value_count
                ),
                self.value_count_provenance,
                self.value_count_provenance,
            ));
        }
        if !self.value_reader.is_empty() {
            let provenance = self
                .value_table
                .position_at(self.value_reader.offset())
                .expect("the first trailing EPM1 byte belongs to its value table")
                .physical;
            return self.latch_error(TrieCodecError::located(
                "trailing bytes after EPM1 payload",
                provenance,
                provenance,
            ));
        }
        self.terminal = IndexedCanonicalEpm1Terminal::Done;
        Ok(None)
    }

    /// Capture the ACT/value-table prestate, make exactly one indexed cursor
    /// call, then bind its returned Map body to the resulting source image.
    /// The receipt is transient and does not enlarge the cursor's hot state.
    pub(crate) fn next_event_with_source(
        &mut self,
    ) -> Result<Option<IndexedCanonicalEpm1TopologySourceEvent<'a>>, TrieCodecError> {
        let before = self.active_source_image();
        let act_before = self.topology_source_position();
        let Some(event) = self.next_event()? else {
            return Ok(None);
        };
        let map_value_receipt = match &event {
            IndexedCanonicalEpm1TopologyEvent::Value {
                has_value: true,
                provenance,
                ..
            } if self.mode == EPathMapMode::Map => match (before, act_before) {
                (Some(before), Some(act_before)) => {
                    match self.map_value_source_receipt(before, act_before, &event) {
                        Some(receipt) => Some(receipt),
                        None => {
                            return self.latch_error(TrieCodecError::located(
                                "EPM1 Map value lacks an exact indexed source receipt",
                                *provenance,
                                *provenance,
                            ));
                        }
                    }
                }
                _ => {
                    return self.latch_error(TrieCodecError::located(
                        "EPM1 Map value has no active ACT prestate",
                        *provenance,
                        *provenance,
                    ));
                }
            },
            _ => None,
        };
        Ok(Some(IndexedCanonicalEpm1TopologySourceEvent {
            event,
            map_value_receipt,
        }))
    }

    pub(crate) fn next_event(
        &mut self,
    ) -> Result<Option<IndexedCanonicalEpm1TopologyEvent<'a>>, TrieCodecError> {
        match self.terminal.clone() {
            IndexedCanonicalEpm1Terminal::Done => return Ok(None),
            IndexedCanonicalEpm1Terminal::Failed(error) => return Err(error),
            IndexedCanonicalEpm1Terminal::Active => {}
        }

        if self.mode == EPathMapMode::Empty {
            let event = match self.empty_state {
                0 => IndexedCanonicalEpm1TopologyEvent::Value {
                    has_value: false,
                    value_body_range: None,
                    value_body: None,
                    source_offset: 5,
                    provenance: self.empty_provenance,
                },
                1 => IndexedCanonicalEpm1TopologyEvent::Leave {
                    provenance: self.empty_provenance,
                },
                _ => return self.finish_stream(),
            };
            self.empty_state += 1;
            self.topology_events += 1;
            return Ok(Some(event));
        }

        let event = {
            let trie = self
                .trie
                .as_mut()
                .expect("a typed EPM1 cursor owns its indexed ACT cursor");
            match trie.next_event() {
                Ok(event) => Ok(event),
                Err(error) => Err(indexed_act_error(trie.source(), error)),
            }
        };
        let Some(event) = (match event {
            Ok(event) => event,
            Err(error) => return self.latch_error(error),
        }) else {
            return self.finish_stream();
        };
        self.topology_events += 1;

        let event_provenance = |source_offset| {
            self.trie
                .as_ref()
                .expect("a typed EPM1 cursor owns its indexed ACT cursor")
                .source()
                .position_at(source_offset)
                .expect("an ACT topology event belongs to its indexed arena")
                .physical
        };
        Ok(Some(match event {
            CanonicalCompactTrieTopologyEvent::Value {
                value,
                source_offset,
            } => {
                let provenance = event_provenance(source_offset);
                let (value_body_range, value_body) =
                    if self.mode == EPathMapMode::Map && value.is_some() {
                        let marker = value.expect("the map value branch checked presence");
                        if marker != 0 {
                            return self.latch_error(TrieCodecError::located(
                                format!("map ACTree value marker {marker} is not zero"),
                                provenance,
                                provenance,
                            ));
                        }
                        if self.length == self.value_count {
                            return self.latch_error(TrieCodecError::located(
                                format!(
                                    "map ACTree references {} values but EPM1 declares {}",
                                    self.length + 1,
                                    self.value_count,
                                ),
                                provenance,
                                provenance,
                            ));
                        }
                        let len = match read_indexed_len(
                            &self.value_table,
                            &mut self.value_reader,
                            "map value length",
                        ) {
                            Ok(len) => len,
                            Err(error) => return self.latch_error(error),
                        };
                        let relative_start = self.value_reader.offset();
                        let body = match take_indexed_region(
                            &self.value_table,
                            &mut self.value_reader,
                            len,
                            "map value payload",
                        ) {
                            Ok(body) => body,
                            Err(error) => return self.latch_error(error),
                        };
                        let Some(start) = self.value_table_start.checked_add(relative_start) else {
                            return self.latch_error(TrieCodecError::located(
                                "EPM1 value range overflows usize",
                                provenance,
                                provenance,
                            ));
                        };
                        let Some(end) = start.checked_add(len) else {
                            return self.latch_error(TrieCodecError::located(
                                "EPM1 value range overflows usize",
                                provenance,
                                provenance,
                            ));
                        };
                        self.length += 1;
                        self.value_records += 1;
                        (Some(start..end), Some(body))
                    } else {
                        if self.mode == EPathMapMode::Set {
                            if let Some(marker) = value {
                                if marker != 0 {
                                    return self.latch_error(TrieCodecError::located(
                                        "set ACTree value marker is not zero",
                                        provenance,
                                        provenance,
                                    ));
                                }
                                let Some(length) = self.length.checked_add(1) else {
                                    return self.latch_error(TrieCodecError::located(
                                        "EPathMap length overflows usize",
                                        provenance,
                                        provenance,
                                    ));
                                };
                                self.length = length;
                            }
                        }
                        (None, None)
                    };
                IndexedCanonicalEpm1TopologyEvent::Value {
                    has_value: value.is_some(),
                    value_body_range,
                    value_body,
                    source_offset,
                    provenance,
                }
            }
            CanonicalCompactTrieTopologyEvent::Edge {
                byte,
                source_offset,
            } => IndexedCanonicalEpm1TopologyEvent::Edge {
                byte,
                provenance: event_provenance(source_offset),
            },
            CanonicalCompactTrieTopologyEvent::Leave { source_offset } => {
                IndexedCanonicalEpm1TopologyEvent::Leave {
                    provenance: event_provenance(source_offset),
                }
            }
        }))
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OwnedCanonicalEpm1TopologyEvent {
    Value {
        has_value: bool,
        value_body_range: Option<Range<usize>>,
        value_body: Option<Bytes>,
        source_offset: usize,
    },
    Edge {
        byte: u8,
        source_offset: usize,
    },
    Leave {
        source_offset: usize,
    },
}

#[cfg(test)]
pub(crate) struct OwnedCanonicalEpm1TopologyCursor {
    bytes: Bytes,
    indexed: IndexedCanonicalEpm1TopologyCursor<'static>,
}

#[cfg(test)]
impl OwnedCanonicalEpm1TopologyCursor {
    pub(crate) fn new(bytes: Bytes) -> Result<Self, TrieCodecError> {
        let view = IndexedByteRegion::from_protobuf_region(ProtobufRegion::from(bytes.clone()), 0);
        let mut validator = IndexedCanonicalEpm1TopologyCursor::new(view.clone())?;
        while validator.next_event()?.is_some() {}
        let indexed = IndexedCanonicalEpm1TopologyCursor::new(view)?;
        Ok(Self { bytes, indexed })
    }

    pub(crate) const fn mode(&self) -> EPathMapMode { self.indexed.mode() }

    pub(crate) const fn value_count(&self) -> usize { self.indexed.value_count() }

    pub(crate) const fn length(&self) -> usize { self.indexed.length() }

    pub(crate) fn next_event(&mut self) -> Option<OwnedCanonicalEpm1TopologyEvent> {
        let event = self
            .indexed
            .next_event()
            .expect("the indexed constructor validated the immutable owned EPM1 bytes")?;
        Some(match event {
            IndexedCanonicalEpm1TopologyEvent::Value {
                has_value,
                value_body_range,
                value_body: _,
                provenance,
                ..
            } => {
                let value_body = value_body_range
                    .as_ref()
                    .map(|range| self.bytes.slice(range.clone()));
                OwnedCanonicalEpm1TopologyEvent::Value {
                    has_value,
                    value_body_range,
                    value_body,
                    source_offset: provenance.offset,
                }
            }
            IndexedCanonicalEpm1TopologyEvent::Edge { byte, provenance } => {
                OwnedCanonicalEpm1TopologyEvent::Edge {
                    byte,
                    source_offset: provenance.offset,
                }
            }
            IndexedCanonicalEpm1TopologyEvent::Leave { provenance } => {
                OwnedCanonicalEpm1TopologyEvent::Leave {
                    source_offset: provenance.offset,
                }
            }
        })
    }
}

/// The PathMap-native, value-independent prefix of an `EPM1` snapshot.
///
/// It contains the mode, compact ACTree03 arena, and value count. Map values
/// follow this prefix as `varint(protobuf_len) || protobuf_body` records. Keeping
/// only this topology in the shared cache is important: a nested map chain has
/// linear total topology, whereas caching the complete snapshot at every nested
/// node would retain every suffix and therefore quadratic bytes.
pub(crate) struct EpmLayout {
    prefix: Vec<u8>,
    value_count: usize,
}

impl EpmLayout {
    #[inline]
    pub(crate) fn prefix(&self) -> &[u8] { &self.prefix }

    #[inline]
    pub(crate) fn value_count(&self) -> usize { self.value_count }
}

/// Build the final-byte topology directly from PathMap. No `Par` projection,
/// encoded-value table, pointer identity, or occurrence index is retained.
/// Every ACT value endpoint carries the zero presence marker; map bodies are
/// associated positionally in the same zipper order.
pub(crate) fn layout<T>(repr: &EPathMapRepr<T>) -> EpmLayout
where T: Clone + Send + Sync + Unpin + 'static {
    layout_with_certified_value_count(repr, None)
}

/// Build an EPM1 topology with an optional maintained value count.  Callers
/// such as `EntryTrie` may supply their certified cardinality; generic callers
/// pay exactly one value-zipper counting pass and never build an address map.
pub(crate) fn layout_with_certified_value_count<T>(
    repr: &EPathMapRepr<T>,
    certified_value_count: Option<usize>,
) -> EpmLayout
where
    T: Clone + Send + Sync + Unpin + 'static,
{
    let mut prefix = Vec::new();
    prefix.extend_from_slice(EPM_MAGIC);
    prefix.push(EPM_VERSION);
    prefix.push(repr.mode() as u8);

    let value_count = match repr {
        EPathMapRepr::Empty => {
            push_varint(&mut prefix, 0);
            push_varint(&mut prefix, 0);
            0
        }
        EPathMapRepr::Set(map) => {
            let arena = ArenaCompactTree::from_zipper(map.read_zipper(), |_| 0);
            let bytes = arena.get_data();
            push_varint(&mut prefix, bytes.len() as u64);
            prefix.extend_from_slice(bytes);
            push_varint(&mut prefix, 0);
            0
        }
        EPathMapRepr::Map(map) => {
            let value_count = certified_value_count.unwrap_or_else(|| {
                let mut zipper = map.read_zipper();
                let mut count = usize::from(zipper.val().is_some());
                while zipper.to_next_get_val().is_some() {
                    count = count
                        .checked_add(1)
                        .expect("EPathMap value count cannot exceed usize");
                }
                count
            });
            let arena = ArenaCompactTree::from_zipper(map.read_zipper(), |_| 0);
            let bytes = arena.get_data();
            push_varint(&mut prefix, bytes.len() as u64);
            prefix.extend_from_slice(bytes);
            push_varint(&mut prefix, value_count as u64);
            value_count
        }
    };

    EpmLayout {
        prefix,
        value_count,
    }
}

/// Encode a specialized EPathMap representation directly as one `EPM1` byte
/// string. The compact arena is produced by PathMap's own jumping catamorphism;
/// values never leave the generated iterative protobuf codec.
pub fn encode(repr: &EPathMapRepr<Par>) -> Vec<u8> {
    let layout = layout(repr);
    encode_with_layout(repr, &layout)
}

pub(crate) fn encode_with_layout(repr: &EPathMapRepr<Par>, layout: &EpmLayout) -> Vec<u8> {
    let mut out = Vec::from(layout.prefix());
    if let EPathMapRepr::Map(map) = repr {
        let mut zipper = map.read_zipper();
        let mut emitted = 0usize;
        if let Some(value) = zipper.val() {
            protobuf_encoder::encode_length_delimited_body_into(value, &mut out);
            emitted += 1;
        }
        while let Some(value) = zipper.to_next_get_val() {
            protobuf_encoder::encode_length_delimited_body_into(value, &mut out);
            emitted += 1;
        }
        debug_assert_eq!(emitted, layout.value_count());
    }
    out
}

enum PendingTopology<'a> {
    Borrowed(CanonicalCompactTrieTopologyCursor<&'a [u8]>),
    Owned(CanonicalCompactTrieTopologyCursor<ProtobufRegion>),
}

impl PendingTopology<'_> {
    fn next_event(&mut self) -> Option<CanonicalCompactTrieTopologyEvent> {
        match self {
            Self::Borrowed(cursor) => cursor.next_event(),
            Self::Owned(cursor) => cursor.next_event(),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Borrowed(cursor) => cursor.is_empty(),
            Self::Owned(cursor) => cursor.is_empty(),
        }
    }

    fn value_summary(&self) -> CanonicalCompactTrieValueSummary {
        match self {
            Self::Borrowed(cursor) => cursor.value_summary(),
            Self::Owned(cursor) => cursor.value_summary(),
        }
    }
}

enum PendingScaffold<V> {
    Repeat(V),
    New(fn() -> V),
}

mod payload_seal {
    pub trait Sealed {}
    impl Sealed for () {}
    impl Sealed for crate::rhoapi::Par {}
    impl Sealed for crate::rust::par_construction::PlannedPar {}
}

pub(crate) trait EpmPayload:
    payload_seal::Sealed + Clone + Send + Sync + Unpin + 'static
{
    fn dispose(self);
}

impl EpmPayload for () {
    fn dispose(self) {}
}

impl EpmPayload for Par {
    fn dispose(self) { crate::rust::rholang::par_children::dismantle(self); }
}

impl EpmPayload for crate::rust::par_construction::PlannedPar {
    fn dispose(self) {}
}

#[derive(Clone, Copy)]
struct EpmPayloadDisposer;

impl<V: EpmPayload> DisposeValue<V> for EpmPayloadDisposer {
    fn dispose(&self, value: V) { value.dispose(); }
}

pub(crate) struct EpmReprOwner<V: EpmPayload, B: BarrierProvider<V>> {
    repr: EPathMapRepr<V>,
    pub(crate) barrier: B,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct ResidentEpmBarrier;

impl BarrierProvider<Par> for ResidentEpmBarrier {
    fn make_barrier(&self) -> Par { Par::default() }

    fn discard_barrier(&self, value: Par) { value.dispose(); }
}

pub(crate) type OwnedResidentEPathMap = EpmReprOwner<Par, ResidentEpmBarrier>;

impl<V: EpmPayload, B: BarrierProvider<V>> EpmReprOwner<V, B> {
    pub(crate) fn try_map_resident<E>(
        mut self,
        mut convert: impl FnMut(&V) -> Result<Par, E>,
    ) -> Result<OwnedResidentEPathMap, E> {
        use pathmap::zipper::Zipper;

        if !matches!(self.repr, EPathMapRepr::Map(_)) {
            let repr = match std::mem::take(&mut self.repr) {
                EPathMapRepr::Empty => EPathMapRepr::Empty,
                EPathMapRepr::Set(map) => EPathMapRepr::Set(map),
                EPathMapRepr::Map(_) => unreachable!(),
            };
            return Ok(OwnedResidentEPathMap::new(repr, ResidentEpmBarrier));
        }
        let EPathMapRepr::Map(source) = &self.repr else {
            unreachable!()
        };
        let mut reader = source.read_zipper();
        let mut destination = PendingWriter {
            state: PendingWriterState::Map(
                WriteZipperOwned::new(),
                PendingScaffold::New(Par::default),
            ),
        };
        let mut frames = Vec::new();
        let mut span_len = 0;
        let mut first_span = true;
        'enter: loop {
            let scaffold = reader.val().is_none() && !reader.path().is_empty();
            if let Some(value) = reader.val() {
                let value = convert(value)?;
                let PendingWriterState::Map(writer, _) = &mut destination.state else {
                    unreachable!()
                };
                writer.set_val(value);
            } else if scaffold {
                destination.install_scaffold_value();
            }
            #[cfg(test)]
            payload_mapping_tests::observe_span(span_len, scaffold);
            frames.push((reader.child_mask().iter(), span_len, scaffold));
            while let Some((children, _, _)) = frames.last_mut() {
                if let Some(byte) = children.next() {
                    let base = reader.path().len();
                    reader.descend_to_byte(byte);
                    while !reader.is_val() && reader.child_count() == 1 {
                        let byte = reader.child_mask().iter().next().expect("one child");
                        reader.descend_to_byte(byte);
                    }
                    let span = &reader.path()[base..];
                    span_len = span.len();
                    if first_span {
                        destination.descend_first_span(span);
                        first_span = false;
                    } else {
                        let PendingWriterState::Map(writer, _) = &mut destination.state else {
                            unreachable!()
                        };
                        writer.descend_to(span);
                    }
                    continue 'enter;
                }
                let (_, length, scaffold) = frames.pop().expect("live mapping frame");
                let PendingWriterState::Map(writer, _) = &mut destination.state else {
                    unreachable!()
                };
                if scaffold {
                    remove_value_preserving_path(writer)
                        .expect("installed mapping scaffold")
                        .dispose();
                }
                if length != 0 {
                    writer.ascend(length);
                    reader.ascend(length);
                }
            }
            break;
        }
        let repr = destination
            .into_repr()
            .expect("mapping retains a live destination");
        Ok(OwnedResidentEPathMap::new(repr, ResidentEpmBarrier))
    }

    pub(crate) fn new(repr: EPathMapRepr<V>, barrier: B) -> Self { Self { repr, barrier } }

    pub(crate) fn as_repr(&self) -> &EPathMapRepr<V> { &self.repr }

    pub(crate) fn as_repr_mut(&mut self) -> &mut EPathMapRepr<V> { &mut self.repr }

    pub(crate) fn normalize_empty(&mut self) {
        if self.repr.is_empty() {
            self.retire();
        }
    }

    pub(crate) fn with_barrier<C: BarrierProvider<V>>(mut self, barrier: C) -> EpmReprOwner<V, C> {
        EpmReprOwner::new(std::mem::take(&mut self.repr), barrier)
    }

    fn retire(&mut self) {
        match std::mem::take(&mut self.repr) {
            EPathMapRepr::Empty => {}
            EPathMapRepr::Set(map) => {
                retire_pathmap(map, &EpmPayloadDisposer, &PendingScaffold::Repeat(()));
            }
            EPathMapRepr::Map(map) => {
                retire_pathmap(map, &EpmPayloadDisposer, &self.barrier);
            }
        }
    }
}

impl<V: EpmPayload, B: BarrierProvider<V> + Default> Default for EpmReprOwner<V, B> {
    fn default() -> Self { Self::new(EPathMapRepr::Empty, B::default()) }
}

impl<V: EpmPayload, B: BarrierProvider<V>> Drop for EpmReprOwner<V, B> {
    fn drop(&mut self) { self.retire(); }
}

impl<V: Clone> PendingScaffold<V> {
    fn value(&self) -> V {
        match self {
            Self::Repeat(value) => value.clone(),
            Self::New(create) => create(),
        }
    }
}

impl<V: EpmPayload> BarrierProvider<V> for PendingScaffold<V> {
    fn make_barrier(&self) -> V { self.value() }

    fn discard_barrier(&self, value: V) { value.dispose(); }
}

struct EpmMapOwner<V: EpmPayload> {
    owned: Option<(PathMap<V>, PendingScaffold<V>)>,
}

impl<V: EpmPayload> EpmMapOwner<V> {
    fn from_writer(writer: WriteZipperOwned<V>, scaffold: PendingScaffold<V>) -> Self {
        Self {
            owned: Some((writer.into_map(), scaffold)),
        }
    }

    fn into_writer_parts(mut self) -> (WriteZipperOwned<V>, PendingScaffold<V>) {
        let (map, scaffold) = self.owned.take().expect("a live EPM map owns its root");
        (map.into_write_zipper([]), scaffold)
    }
}

impl<V: EpmPayload> Drop for EpmMapOwner<V> {
    fn drop(&mut self) {
        if let Some((map, scaffold)) = self.owned.take() {
            retire_pathmap(map, &EpmPayloadDisposer, &scaffold);
        }
    }
}

enum PendingWriterState<V: EpmPayload> {
    Empty,
    Set(WriteZipperOwned<()>),
    Map(WriteZipperOwned<V>, PendingScaffold<V>),
    Discard,
}

struct PendingWriter<V: EpmPayload> {
    state: PendingWriterState<V>,
}

impl<V: EpmPayload> Drop for PendingWriter<V> {
    fn drop(&mut self) {
        match std::mem::replace(&mut self.state, PendingWriterState::Discard) {
            PendingWriterState::Set(writer) => {
                drop(EpmMapOwner::from_writer(
                    writer,
                    PendingScaffold::Repeat(()),
                ));
            }
            PendingWriterState::Map(writer, scaffold) => {
                drop(EpmMapOwner::from_writer(writer, scaffold));
            }
            PendingWriterState::Empty | PendingWriterState::Discard => {}
        }
    }
}

impl<V: EpmPayload> PendingWriter<V> {
    fn discard() -> Self {
        Self {
            state: PendingWriterState::Discard,
        }
    }

    fn into_repr(mut self) -> Result<EPathMapRepr<V>, TrieCodecError> {
        Ok(
            match std::mem::replace(&mut self.state, PendingWriterState::Discard) {
                PendingWriterState::Empty => EPathMapRepr::Empty,
                PendingWriterState::Set(zipper) => EPathMapRepr::Set(zipper.into_map()),
                PendingWriterState::Map(zipper, _) => EPathMapRepr::Map(zipper.into_map()),
                PendingWriterState::Discard => {
                    return Err(TrieCodecError::new(
                        "EPM1 rejected without a retained semantic error",
                    ));
                }
            },
        )
    }

    fn install_scaffold_value(&mut self) {
        match &mut self.state {
            PendingWriterState::Set(zipper) => {
                zipper.set_val(());
            }
            PendingWriterState::Map(zipper, scaffold) => {
                zipper.set_val(scaffold.value());
            }
            PendingWriterState::Empty | PendingWriterState::Discard => {}
        }
    }

    fn descend_to_byte(&mut self, byte: u8) {
        match &mut self.state {
            PendingWriterState::Set(zipper) => zipper.descend_to_byte(byte),
            PendingWriterState::Map(zipper, _) => zipper.descend_to_byte(byte),
            PendingWriterState::Empty | PendingWriterState::Discard => {}
        }
    }

    /// Seed the first dangling edge through `PathMap::create_path`.  An owned
    /// zipper over a pristine map still references PathMap's immutable empty
    /// sentinel, while the map-level constructor is the public API that
    /// replaces that sentinel without installing a transient value.
    fn descend_first_byte(&mut self, byte: u8) { self.descend_first_span(&[byte]); }

    fn descend_first_span(&mut self, span: &[u8]) {
        match std::mem::replace(&mut self.state, PendingWriterState::Discard) {
            PendingWriterState::Set(zipper) => {
                let mut owner = EpmMapOwner::from_writer(zipper, PendingScaffold::Repeat(()));
                owner
                    .owned
                    .as_mut()
                    .expect("the transferred EPM map owns its root")
                    .0
                    .create_path(span);
                let (zipper, _) = owner.into_writer_parts();
                self.state = PendingWriterState::Set(zipper);
            }
            PendingWriterState::Map(zipper, scaffold) => {
                let mut owner = EpmMapOwner::from_writer(zipper, scaffold);
                owner
                    .owned
                    .as_mut()
                    .expect("the transferred EPM map owns its root")
                    .0
                    .create_path(span);
                let (zipper, scaffold) = owner.into_writer_parts();
                self.state = PendingWriterState::Map(zipper, scaffold);
            }
            PendingWriterState::Empty => self.state = PendingWriterState::Empty,
            PendingWriterState::Discard => {}
        }
        match &mut self.state {
            PendingWriterState::Set(zipper) => zipper.descend_to(span),
            PendingWriterState::Map(zipper, _) => zipper.descend_to(span),
            PendingWriterState::Empty | PendingWriterState::Discard => {}
        }
    }

    fn ascend_byte(&mut self) {
        match &mut self.state {
            PendingWriterState::Set(zipper) => {
                zipper.ascend_byte();
            }
            PendingWriterState::Map(zipper, _) => {
                zipper.ascend_byte();
            }
            PendingWriterState::Empty | PendingWriterState::Discard => {}
        }
    }

    fn remove_scaffold_value(&mut self) {
        match &mut self.state {
            PendingWriterState::Set(zipper) => {
                zipper.remove_val(false);
            }
            PendingWriterState::Map(zipper, _) => {
                zipper.remove_val(false);
            }
            PendingWriterState::Empty | PendingWriterState::Discard => {}
        }
    }
}

struct EpmOwnedCursor<V: EpmPayload> {
    writer: PendingWriter<V>,
    started: bool,
    exhausted: bool,
}

impl<V: EpmPayload> EpmOwnedCursor<V> {
    fn new(map: PathMap<V>, scaffold: PendingScaffold<V>) -> Self {
        Self {
            writer: PendingWriter {
                state: PendingWriterState::Map(map.into_write_zipper([]), scaffold),
            },
            started: false,
            exhausted: false,
        }
    }
}

impl<V: EpmPayload> Iterator for EpmOwnedCursor<V> {
    type Item = (Vec<u8>, V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        let PendingWriterState::Map(zipper, _) = &mut self.writer.state else {
            unreachable!("a consuming EPM cursor retains its guarded writer");
        };
        let root_value = !self.started && zipper.val().is_some();
        self.started = true;
        if !root_value && !zipper.to_next_val() {
            self.exhausted = true;
            return None;
        }
        let key = zipper.path().to_vec();
        let value = remove_value_preserving_path(zipper)
            .expect("a consuming EPM cursor advances only to a value");
        Some((key, value))
    }
}

macro_rules! owned_epm_cursor {
    ($name:ident, $payload:ty, $scaffold:expr) => {
        pub struct $name(EpmOwnedCursor<$payload>);

        impl $name {
            pub(crate) fn new(map: PathMap<$payload>) -> Self {
                Self(EpmOwnedCursor::new(map, $scaffold))
            }
        }

        impl Iterator for $name {
            type Item = (Vec<u8>, $payload);

            fn next(&mut self) -> Option<Self::Item> { self.0.next() }
        }

        impl std::iter::FusedIterator for $name {}
    };
}

owned_epm_cursor!(OwnedEPathMapSetEntries, (), PendingScaffold::Repeat(()));
owned_epm_cursor!(
    OwnedEPathMapMapEntries,
    Par,
    PendingScaffold::New(Par::default)
);

impl OwnedResidentEPathMap {
    pub(crate) fn into_entries(mut self) -> crate::rust::rhoapi_ext::OwnedEPathMapEntries {
        use crate::rust::rhoapi_ext::OwnedEPathMapEntries;
        match std::mem::take(&mut self.repr) {
            EPathMapRepr::Empty => OwnedEPathMapEntries::Empty,
            EPathMapRepr::Set(map) => OwnedEPathMapEntries::Set(OwnedEPathMapSetEntries::new(map)),
            EPathMapRepr::Map(map) => OwnedEPathMapEntries::Map(OwnedEPathMapMapEntries::new(map)),
        }
    }
}

struct PendingFraming {
    raw_mode: u8,
    arena: Range<usize>,
    value_table_start: usize,
    value_count: usize,
}

fn parse_pending_framing(bytes: &[u8]) -> Result<PendingFraming, TrieCodecError> {
    let mut cursor = 0usize;
    if take(bytes, &mut cursor, EPM_MAGIC.len(), "EPM1 magic")? != EPM_MAGIC {
        return Err(TrieCodecError::new("invalid EPM1 magic"));
    }
    let version = take(bytes, &mut cursor, 1, "EPM1 version")?[0];
    if version != EPM_VERSION {
        return Err(TrieCodecError::new(format!(
            "unsupported EPM1 version {version}"
        )));
    }
    let raw_mode = take(bytes, &mut cursor, 1, "EPM1 mode")?[0];
    let arena_len = read_len(bytes, &mut cursor, "ACTree03 length")?;
    let arena_start = cursor;
    take(bytes, &mut cursor, arena_len, "ACTree03 payload")?;
    let arena = arena_start..cursor;
    let value_count = read_len(bytes, &mut cursor, "map value count")?;
    let remaining = bytes.len() - cursor;
    if value_count > remaining {
        return Err(TrieCodecError::new(format!(
            "map value count {value_count} exceeds the {remaining}-byte payload lower bound"
        )));
    }

    let value_table_start = cursor;
    for _ in 0..value_count {
        let len = read_len(bytes, &mut cursor, "map value length")?;
        take(bytes, &mut cursor, len, "map value payload")?;
    }
    if cursor != bytes.len() {
        return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
    }
    Ok(PendingFraming {
        raw_mode,
        arena,
        value_table_start,
        value_count,
    })
}

/// Pausable EPM1 reader used by both the generated protobuf decoder and the
/// synchronous entry point. ACT topology is replayed directly into one owned
/// write zipper; only the current child body and traversal depth survive a PDA
/// suspension.
pub(crate) struct PendingEpmDecode<'a, S: AsRef<[u8]>, V: EpmPayload = Par> {
    snapshot: S,
    mode: Option<EPathMapMode>,
    topology: Option<PendingTopology<'a>>,
    writer: PendingWriter<V>,
    depth: usize,
    scaffold_frames: Vec<bool>,
    descended_once: bool,
    next_value_at: usize,
    remaining_values: usize,
    expected_values: usize,
    decoded_values: usize,
    current_value_range: Option<Range<usize>>,
    current_topology_value: bool,
    validate_canonical: bool,
    first_semantic_error: Option<TrieCodecError>,
    first_noncanonical_value: Option<usize>,
}

pub(crate) type OwnedPendingEpmDecode<V = Par> = PendingEpmDecode<'static, Bytes, V>;

impl<'a, S: AsRef<[u8]>, V: EpmPayload> PendingEpmDecode<'a, S, V> {
    fn from_parts(
        snapshot: S,
        framing: PendingFraming,
        topology: Result<Option<PendingTopology<'a>>, TrieCodecError>,
        validate_canonical: bool,
        scaffold: PendingScaffold<V>,
    ) -> Self {
        let mode = EPathMapMode::try_from(framing.raw_mode);
        let mut first_semantic_error = mode.as_ref().err().cloned();
        let mode = mode.ok();
        let mut topology = match topology {
            Ok(topology) => topology,
            Err(error) => {
                if first_semantic_error.is_none() {
                    first_semantic_error = Some(error);
                }
                None
            }
        };

        let mut writer = PendingWriter {
            state: match mode {
                Some(EPathMapMode::Empty) => PendingWriterState::Empty,
                Some(EPathMapMode::Set) => PendingWriterState::Set(WriteZipperOwned::new()),
                Some(EPathMapMode::Map) => {
                    PendingWriterState::Map(WriteZipperOwned::new(), scaffold)
                }
                None => PendingWriterState::Discard,
            },
        };

        match mode {
            Some(EPathMapMode::Empty) => {
                if !framing.arena.is_empty() || framing.value_count != 0 {
                    first_semantic_error.get_or_insert_with(|| {
                        TrieCodecError::new("neutral empty EPathMap carries trie or values")
                    });
                    writer = PendingWriter::discard();
                }
                topology = None;
            }
            Some(EPathMapMode::Set | EPathMapMode::Map) => {
                if let Some(cursor) = topology.as_ref() {
                    if let Err(error) = validate_canonical_act_summary(
                        mode.expect("typed mode is present"),
                        framing.value_count,
                        cursor.is_empty(),
                        cursor.value_summary(),
                    ) {
                        first_semantic_error.get_or_insert(error);
                    }
                }
                if topology.is_none() {
                    writer = PendingWriter::discard();
                }
            }
            None => {
                topology = None;
            }
        }

        if first_semantic_error.is_some() {
            // A rejected tree is never published. We keep a valid cursor, when
            // available, only to consume its event language and preserve body
            // error precedence; rejected values are not installed.
            writer = PendingWriter::discard();
        }

        Self {
            snapshot,
            mode,
            topology,
            writer,
            depth: 0,
            scaffold_frames: vec![false],
            descended_once: false,
            next_value_at: framing.value_table_start,
            remaining_values: framing.value_count,
            expected_values: framing.value_count,
            decoded_values: 0,
            current_value_range: None,
            current_topology_value: false,
            validate_canonical,
            first_semantic_error,
            first_noncanonical_value: None,
        }
    }

    fn latch_semantic(&mut self, error: TrieCodecError) {
        if self.first_semantic_error.is_none() {
            self.first_semantic_error = Some(error);
            self.writer = PendingWriter::discard();
        }
    }

    /// Consume ACT events through the next positional map endpoint. `true`
    /// means a value body must be attached at the current zipper focus.
    fn advance_to_map_value(&mut self) -> bool {
        loop {
            let Some(event) = self.topology.as_mut().and_then(PendingTopology::next_event) else {
                return false;
            };
            match event {
                CanonicalCompactTrieTopologyEvent::Value { value, .. } => {
                    if let Some(marker) = value {
                        if marker != 0 {
                            self.latch_semantic(TrieCodecError::new(format!(
                                "map ACTree value marker {marker} is not zero"
                            )));
                        }
                        if self.mode == Some(EPathMapMode::Set) {
                            if let PendingWriterState::Set(zipper) = &mut self.writer.state {
                                zipper.set_val(());
                            }
                        } else if self.mode == Some(EPathMapMode::Map) {
                            return true;
                        }
                    } else if self.depth != 0 {
                        // `create_path` intentionally leaves PathMap's empty
                        // sentinel at a dangling endpoint.  The public owned
                        // zipper cannot descend through that sentinel, so a
                        // value-free node with children needs a temporary
                        // value until its postorder Leave.  This is removed
                        // without pruning and is never externally observable.
                        self.writer.install_scaffold_value();
                        *self
                            .scaffold_frames
                            .last_mut()
                            .expect("every descended ACT node owns a depth frame") = true;
                    }
                }
                CanonicalCompactTrieTopologyEvent::Edge { byte, .. } => {
                    if self.descended_once {
                        self.writer.descend_to_byte(byte);
                    } else {
                        self.writer.descend_first_byte(byte);
                        self.descended_once = true;
                    }
                    self.depth = self
                        .depth
                        .checked_add(1)
                        .expect("validated ACT depth cannot exceed usize");
                    self.scaffold_frames.push(false);
                }
                CanonicalCompactTrieTopologyEvent::Leave { .. } => {
                    if self.scaffold_frames.last().copied().unwrap_or(false) {
                        self.writer.remove_scaffold_value();
                    }
                    if self.depth != 0 {
                        self.writer.ascend_byte();
                        self.depth -= 1;
                        self.scaffold_frames.pop();
                    }
                }
            }
        }
    }

    fn next_value_range(&mut self) -> Result<Option<Range<usize>>, TrieCodecError> {
        if self.current_value_range.is_some() {
            return Err(TrieCodecError::new(
                "next map value requested before the previous value was decoded",
            ));
        }
        if self.remaining_values == 0 {
            return Ok(None);
        }

        self.current_topology_value = if self.mode == Some(EPathMapMode::Map) {
            let found = self.advance_to_map_value();
            if !found {
                self.latch_semantic(TrieCodecError::new(format!(
                    "map ACTree references fewer values than EPM1 declares {}",
                    self.expected_values
                )));
            }
            found
        } else {
            false
        };

        let bytes = self.snapshot.as_ref();
        let len = read_len(bytes, &mut self.next_value_at, "map value length")?;
        let start = self.next_value_at;
        take(bytes, &mut self.next_value_at, len, "map value payload")?;
        self.remaining_values -= 1;
        let range = start..start + len;
        self.current_value_range = Some(range.clone());
        Ok(Some(range))
    }

    pub(crate) fn push_value(&mut self, value: V) -> Result<(), TrieCodecError> {
        if self.decoded_values == self.expected_values {
            return Err(TrieCodecError::new(
                "more map values were decoded than the EPM1 value count",
            ));
        }
        let range = self.current_value_range.take().ok_or_else(|| {
            TrieCodecError::new("map value supplied without a corresponding EPM1 value body")
        })?;

        if self.validate_canonical && self.first_noncanonical_value.is_none() {
            let region = IndexedByteRegion::from_borrowed(
                &self.snapshot.as_ref()[range.clone()],
                0,
                range.start,
            );
            if validate_canonical_par_protobuf_indexed(region).is_err() {
                self.first_noncanonical_value = Some(self.decoded_values);
            }
        }

        if self.current_topology_value {
            if let PendingWriterState::Map(zipper, _) = &mut self.writer.state {
                zipper.set_val(value);
            }
        }
        self.current_topology_value = false;
        self.decoded_values += 1;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<EPathMapRepr<V>, TrieCodecError> {
        if self.remaining_values != 0
            || self.current_value_range.is_some()
            || self.decoded_values != self.expected_values
        {
            self.latch_semantic(TrieCodecError::new(
                "fewer map values were decoded than the EPM1 value count",
            ));
        }

        while self.advance_to_map_value() {
            self.latch_semantic(TrieCodecError::new(format!(
                "map ACTree references more values than EPM1 declares {}",
                self.expected_values
            )));
        }
        if self.depth != 0 {
            self.latch_semantic(TrieCodecError::new(
                "map ACTree topology ended above its root",
            ));
        }

        if let Some(error) = self.first_semantic_error {
            return Err(error);
        }
        if let Some(position) = self.first_noncanonical_value {
            return Err(TrieCodecError::new(format!(
                "map value {position} protobuf body is not canonical"
            )));
        }

        self.writer.into_repr()
    }
}

#[cfg(test)]
impl PendingEpmDecode<'static, Bytes> {
    pub(crate) fn new(snapshot: Bytes, validate_canonical: bool) -> Result<Self, TrieCodecError> {
        Self::new_with_provider(
            snapshot,
            validate_canonical,
            PendingScaffold::New(Par::default),
        )
    }
}

impl<V: EpmPayload> PendingEpmDecode<'static, Bytes, V> {
    pub(crate) fn new_with_scaffold(
        snapshot: Bytes,
        validate_canonical: bool,
        scaffold: V,
    ) -> Result<Self, TrieCodecError> {
        Self::new_with_provider(
            snapshot,
            validate_canonical,
            PendingScaffold::Repeat(scaffold),
        )
    }

    fn new_with_provider(
        snapshot: Bytes,
        validate_canonical: bool,
        scaffold: PendingScaffold<V>,
    ) -> Result<Self, TrieCodecError> {
        let framing = parse_pending_framing(snapshot.as_ref())?;
        let topology = match EPathMapMode::try_from(framing.raw_mode) {
            Ok(EPathMapMode::Set | EPathMapMode::Map) => {
                let region = ProtobufRegion::from(snapshot.clone())
                    .subregion(CanonicalByteRange {
                        start: framing.arena.start,
                        end: framing.arena.end,
                    })
                    .expect("framing proved the ACT region belongs to the snapshot");
                CanonicalCompactTrieTopologyCursor::from_backing(region)
                    .map(PendingTopology::Owned)
                    .map(Some)
                    .map_err(TrieCodecError::from)
            }
            Ok(EPathMapMode::Empty) | Err(_) => Ok(None),
        };
        Ok(Self::from_parts(
            snapshot,
            framing,
            topology,
            validate_canonical,
            scaffold,
        ))
    }

    pub(crate) fn next_value_bytes(&mut self) -> Result<Option<Bytes>, TrieCodecError> {
        let Some(range) = self.next_value_range()? else {
            return Ok(None);
        };
        Ok(Some(self.snapshot.slice(range)))
    }
}

impl<'a> PendingEpmDecode<'a, &'a [u8]> {
    fn new_borrowed(bytes: &'a [u8], validate_canonical: bool) -> Result<Self, TrieCodecError> {
        let framing = parse_pending_framing(bytes)?;
        let topology = match EPathMapMode::try_from(framing.raw_mode) {
            Ok(EPathMapMode::Set | EPathMapMode::Map) => {
                CanonicalCompactTrieTopologyCursor::new(&bytes[framing.arena.clone()])
                    .map(PendingTopology::Borrowed)
                    .map(Some)
                    .map_err(TrieCodecError::from)
            }
            Ok(EPathMapMode::Empty) | Err(_) => Ok(None),
        };
        Ok(Self::from_parts(
            bytes,
            framing,
            topology,
            validate_canonical,
            PendingScaffold::New(Par::default),
        ))
    }
}

/// Decode through the same pausable positional writer used by generated
/// protobuf. This keeps synchronous and generated admission byte-for-byte and
/// error-for-error aligned without an entry vector or a second reconstruction.
fn decode_with(
    bytes: &[u8],
    decode_value: fn(&[u8]) -> Result<Par, prost::DecodeError>,
) -> Result<EPathMapRepr<Par>, TrieCodecError> {
    let mut pending = PendingEpmDecode::new_borrowed(bytes, true)?;
    while let Some(range) = pending.next_value_range()? {
        let position = pending.decoded_values;
        let value = decode_value(&bytes[range]).map_err(|error| {
            TrieCodecError::new(format!("map value {position} is not a Par: {error}"))
        })?;
        pending.push_value(value)?;
    }
    pending.finish()
}

pub fn decode(bytes: &[u8]) -> Result<EPathMapRepr<Par>, TrieCodecError> {
    decode_with(bytes, |payload: &[u8]| {
        protobuf_decoder::decode_par(payload)
    })
}

pub(crate) fn decode_deferred_key_validation(
    bytes: &[u8],
) -> Result<EPathMapRepr<Par>, TrieCodecError> {
    decode_with(bytes, |payload: &[u8]| {
        protobuf_decoder::decode_par_deferred_epathmap_validation(payload)
    })
}

#[cfg(test)]
fn visit_act(
    bytes: &[u8],
    mut visit: impl FnMut(&[u8], Option<u64>) -> Result<(), TrieCodecError>,
) -> Result<(), TrieCodecError> {
    let mut trie = CanonicalCompactTrieCursor::new(bytes)?;
    while let Some(endpoint) = trie.next_endpoint() {
        visit(endpoint.path, endpoint.value)?;
    }
    Ok(())
}

fn validate_canonical_act_summary(
    mode: EPathMapMode,
    value_count: usize,
    is_empty: bool,
    summary: CanonicalCompactTrieValueSummary,
) -> Result<(), TrieCodecError> {
    if is_empty {
        return Err(TrieCodecError::new(
            "typed EPathMap mode has empty ACTree topology; use neutral empty mode",
        ));
    }
    match mode {
        EPathMapMode::Set => {
            if summary.first_nonzero().is_some() {
                return Err(TrieCodecError::new("set ACTree value marker is not zero"));
            }
            if value_count != 0 {
                return Err(TrieCodecError::new("set EPathMap carries map values"));
            }
        }
        EPathMapMode::Map => {
            if let Some((_source_offset, marker)) = summary.first_nonzero() {
                return Err(TrieCodecError::new(format!(
                    "map ACTree value marker {marker} is not zero"
                )));
            }
            if summary.value_count() != value_count {
                return Err(TrieCodecError::new(format!(
                    "map ACTree references {} values but EPM1 declares {value_count}",
                    summary.value_count()
                )));
            }
        }
        EPathMapMode::Empty => unreachable!("empty mode has no ACT arena"),
    }
    Ok(())
}

fn collect_canonical_keys(
    arena_start: usize,
    arena: &[u8],
    mode: EPathMapMode,
    value_count: usize,
) -> Result<Vec<CanonicalSnapshotKey>, TrieCodecError> {
    let mut trie = CanonicalCompactTrieCursor::new(arena)?;
    validate_canonical_act_summary(mode, value_count, trie.is_empty(), trie.value_summary())?;
    let mut keys = Vec::with_capacity(value_count);
    while let Some(endpoint) = trie.next_value_endpoint() {
        if mode == EPathMapMode::Map {
            keys.push(match endpoint.contiguous_path_range {
                Some(range) => CanonicalSnapshotKey::Relative(
                    arena_start + range.start..arena_start + range.end,
                ),
                None => {
                    #[cfg(test)]
                    crate::rust::par_bag::epathmap_fast_import_tests::record_key_copy(
                        endpoint.path.len(),
                    );
                    CanonicalSnapshotKey::Owned(endpoint.path.to_vec())
                }
            });
        }
    }
    Ok(keys)
}

fn parse_canonical_snapshot_prefix(
    bytes: &[u8],
) -> Result<CanonicalSnapshotPrefix<'_>, TrieCodecError> {
    let mut cursor = 0usize;
    if take(bytes, &mut cursor, EPM_MAGIC.len(), "EPM1 magic")? != EPM_MAGIC {
        return Err(TrieCodecError::new("invalid EPM1 magic"));
    }
    let version = take(bytes, &mut cursor, 1, "EPM1 version")?[0];
    if version != EPM_VERSION {
        return Err(TrieCodecError::new(format!(
            "unsupported EPM1 version {version}"
        )));
    }
    let mode = EPathMapMode::try_from(take(bytes, &mut cursor, 1, "EPM1 mode")?[0])?;
    let arena_len = read_len(bytes, &mut cursor, "ACTree03 length")?;
    let arena_start = cursor;
    let arena = take(bytes, &mut cursor, arena_len, "ACTree03 payload")?;
    let value_count = read_len(bytes, &mut cursor, "map value count")?;
    let value_table_start = cursor;
    let remaining = bytes.len() - cursor;
    if value_count > remaining {
        return Err(TrieCodecError::new(format!(
            "map value count {value_count} exceeds the {remaining}-byte payload lower bound"
        )));
    }

    Ok(CanonicalSnapshotPrefix {
        mode,
        arena_start,
        arena,
        value_count,
        value_table_start,
    })
}

/// Peek only at the EPM1 prefix needed to select an implementation route.
///
/// No ACT node, key, map value, or proportional collection is traversed or
/// materialized. A successful hint is therefore not validation; consumers
/// selecting the hinted route must subsequently run the full canonical parser.
pub(crate) fn snapshot_route_hint(
    bytes: &[u8],
) -> Result<EPathMapSnapshotRouteHint, TrieCodecError> {
    let prefix = parse_canonical_snapshot_prefix(bytes)?;
    Ok(EPathMapSnapshotRouteHint {
        mode: prefix.mode,
        map_value_count: prefix.value_count,
    })
}

fn parse_canonical_snapshot_framing(
    bytes: &[u8],
) -> Result<CanonicalSnapshotFraming<'_>, TrieCodecError> {
    let CanonicalSnapshotPrefix {
        mode,
        arena_start,
        arena,
        value_count,
        value_table_start,
    } = parse_canonical_snapshot_prefix(bytes)?;
    let mut cursor = value_table_start;
    let mut value_bodies = Vec::with_capacity(value_count);
    for _ in 0..value_count {
        let len = read_len(bytes, &mut cursor, "map value length")?;
        let start = cursor;
        take(bytes, &mut cursor, len, "map value payload")?;
        value_bodies.push(start..cursor);
    }
    if cursor != bytes.len() {
        return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
    }

    Ok(CanonicalSnapshotFraming {
        mode,
        arena_start,
        arena,
        value_count,
        value_table_start,
        value_bodies,
    })
}

fn validate_canonical_snapshot_topology(
    mode: EPathMapMode,
    arena: &[u8],
    value_count: usize,
) -> Result<(), TrieCodecError> {
    match mode {
        EPathMapMode::Empty => {
            if !arena.is_empty() || value_count != 0 {
                return Err(TrieCodecError::new(
                    "neutral empty EPathMap carries trie or values",
                ));
            }
        }
        EPathMapMode::Set | EPathMapMode::Map => {
            let trie = CanonicalCompactTrieCursor::new(arena)?;
            validate_canonical_act_summary(
                mode,
                value_count,
                trie.is_empty(),
                trie.value_summary(),
            )?;
        }
    }
    Ok(())
}

/// Inspect canonical EPM1 framing and ACT topology without constructing a
/// temporary PathMap or decoding its canonical byte keys.
pub(crate) fn inspect_canonical_snapshot(
    bytes: &[u8],
) -> Result<CanonicalSnapshotInspection, TrieCodecError> {
    let framing = parse_canonical_snapshot_framing(bytes)?;

    let keys = match framing.mode {
        EPathMapMode::Empty => {
            if !framing.arena.is_empty() || framing.value_count != 0 {
                return Err(TrieCodecError::new(
                    "neutral empty EPathMap carries trie or values",
                ));
            }
            Vec::new()
        }
        EPathMapMode::Set | EPathMapMode::Map => collect_canonical_keys(
            framing.arena_start,
            framing.arena,
            framing.mode,
            framing.value_count,
        )?,
    };

    Ok(CanonicalSnapshotInspection {
        mode: framing.mode,
        keys,
        value_bodies: framing.value_bodies,
    })
}

/// Inspect canonical EPM1 topology and value bodies without materializing its
/// keys. This is the region-cursor counterpart of `inspect_canonical_snapshot`:
/// callers preserve the original compact arena and need only the ordered map
/// value bodies supplied in its canonical positional traversal order.
pub(crate) fn inspect_canonical_snapshot_value_regions(
    bytes: &[u8],
) -> Result<CanonicalSnapshotValueRegions, TrieCodecError> {
    let framing = parse_canonical_snapshot_framing(bytes)?;
    validate_canonical_snapshot_topology(framing.mode, framing.arena, framing.value_count)?;
    Ok(CanonicalSnapshotValueRegions {
        mode: framing.mode,
        prefix: 0..framing.value_table_start,
        value_bodies: framing.value_bodies,
    })
}

/// Validate one EPM1 image and stream its exact logical PathMap topology.
///
/// Canonical positional value-marker validation runs to completion before the first
/// event becomes observable. The validated topology cursor then expands ACT
/// line compression directly into the same DFS event language as a live
/// `PathMap` zipper. No PathMap, key collection, or map value is constructed.
pub(crate) fn visit_canonical_snapshot_topology(
    bytes: &[u8],
    mut visitor: impl FnMut(CanonicalSnapshotTopologyEvent),
) -> Result<(EPathMapMode, usize), TrieCodecError> {
    let prefix = parse_canonical_snapshot_prefix(bytes)?;
    let mut value_cursor = prefix.value_table_start;
    for _ in 0..prefix.value_count {
        let len = read_len(bytes, &mut value_cursor, "map value length")?;
        take(bytes, &mut value_cursor, len, "map value payload")?;
    }
    if value_cursor != bytes.len() {
        return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
    }

    if prefix.mode == EPathMapMode::Empty {
        if !prefix.arena.is_empty() || prefix.value_count != 0 {
            return Err(TrieCodecError::new(
                "neutral empty EPathMap carries trie or values",
            ));
        }
        visitor(CompactTrieTopologyEvent::Value(false));
        visitor(CompactTrieTopologyEvent::Leave);
        return Ok((prefix.mode, 0));
    }

    let mut trie = CanonicalCompactTrieTopologyCursor::new(prefix.arena)?;
    validate_canonical_act_summary(
        prefix.mode,
        prefix.value_count,
        trie.is_empty(),
        trie.value_summary(),
    )?;
    let length = trie.value_summary().value_count();
    while let Some(event) = trie.next_event() {
        visitor(match event {
            CanonicalCompactTrieTopologyEvent::Value { value, .. } => {
                CompactTrieTopologyEvent::Value(value.is_some())
            }
            CanonicalCompactTrieTopologyEvent::Edge { byte, .. } => {
                CompactTrieTopologyEvent::Edge(byte)
            }
            CanonicalCompactTrieTopologyEvent::Leave { .. } => CompactTrieTopologyEvent::Leave,
        });
    }
    Ok((prefix.mode, length))
}

pub(crate) fn inspect_canonical_map_snapshot(
    bytes: &[u8],
) -> Result<CanonicalSnapshotInspection, TrieCodecError> {
    #[cfg(test)]
    crate::rust::par_bag::epathmap_fast_import_tests::record_inspection();
    let inspection = inspect_canonical_snapshot(bytes)?;
    if inspection.mode != EPathMapMode::Map {
        return Err(TrieCodecError::new(
            "canonical-path map segment does not carry map-mode EPM1",
        ));
    }
    Ok(inspection)
}

/// Borrow the protobuf body associated with the first requested canonical key.
///
/// This is the encoded-view counterpart of `PathMap::get`: it validates the
/// canonical EPM1/ACT image, walks its prefix-compressed key topology without
/// constructing an `EPathMap`, and returns a slice of the existing value table.
/// `ordered_keys` supplies lookup precedence (notably bare-before-split for a
/// [`CursorKind::Prefix`](crate::rust::pathmap_integration::CursorKind::Prefix)
/// cursor). No `Par`, entry projection, or decoded-key collection is built.
pub fn canonical_map_value_body_at_ordered_keys<'a>(
    bytes: &'a [u8],
    ordered_keys: &[&[u8]],
) -> Result<Option<&'a [u8]>, TrieCodecError> {
    let prefix = parse_canonical_snapshot_prefix(bytes)?;
    let mut value_cursor = prefix.value_table_start;
    for _ in 0..prefix.value_count {
        let len = read_len(bytes, &mut value_cursor, "map value length")?;
        take(bytes, &mut value_cursor, len, "map value payload")?;
    }
    if value_cursor != bytes.len() {
        return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
    }
    let mut trie = match prefix.mode {
        EPathMapMode::Empty if !prefix.arena.is_empty() || prefix.value_count != 0 => {
            return Err(TrieCodecError::new(
                "neutral empty EPathMap carries trie or values",
            ));
        }
        EPathMapMode::Empty => {
            return Err(TrieCodecError::new(
                "canonical-path map segment does not carry map-mode EPM1",
            ));
        }
        EPathMapMode::Set | EPathMapMode::Map => CanonicalCompactTrieCursor::new(prefix.arena)?,
    };
    validate_canonical_act_summary(
        prefix.mode,
        prefix.value_count,
        trie.is_empty(),
        trie.value_summary(),
    )?;
    match prefix.mode {
        EPathMapMode::Map => {}
        EPathMapMode::Empty | EPathMapMode::Set => {
            return Err(TrieCodecError::new(
                "canonical-path map segment does not carry map-mode EPM1",
            ));
        }
    }
    value_cursor = prefix.value_table_start;
    let mut unmatched = ordered_keys.len();
    let mut found = None;
    while unmatched > 0 {
        let Some(endpoint) = trie.next_value_endpoint() else {
            break;
        };
        let len = read_len(bytes, &mut value_cursor, "map value length")
            .expect("the value table was validated before the walk");
        let body = take(bytes, &mut value_cursor, len, "map value payload")
            .expect("the value table was validated before the walk");
        let requested = &ordered_keys[..unmatched];
        if let Some(precedence) = requested.iter().position(|key| *key == endpoint.path) {
            unmatched = precedence;
            found = Some(body);
        }
    }
    Ok(found)
}

fn push_varint(out: &mut Vec<u8>, value: u64) { prost::encoding::encode_varint(value, out); }

fn read_len(bytes: &[u8], cursor: &mut usize, label: &str) -> Result<usize, TrieCodecError> {
    let value = read_protobuf_varint(bytes, cursor, label)?;
    usize::try_from(value).map_err(|_| TrieCodecError::new(format!("{label} overflows usize")))
}

fn read_protobuf_varint(
    bytes: &[u8],
    cursor: &mut usize,
    label: &str,
) -> Result<u64, TrieCodecError> {
    let start = *cursor;
    let mut value = 0u64;
    for index in 0..10usize {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| TrieCodecError::new(format!("truncated {label}")))?;
        *cursor += 1;
        if index == 9 && byte > 1 {
            return Err(TrieCodecError::new(format!("overflowing {label}")));
        }
        value |= ((byte & 0x7f) as u64) << (index * 7);
        if byte & 0x80 == 0 {
            let canonical_len = prost::encoding::encoded_len_varint(value);
            if *cursor - start != canonical_len {
                return Err(TrieCodecError::new(format!("non-canonical {label}")));
            }
            return Ok(value);
        }
    }
    Err(TrieCodecError::new(format!("unterminated {label}")))
}

fn take<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    label: &str,
) -> Result<&'a [u8], TrieCodecError> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| TrieCodecError::new(format!("{label} length overflow")))?;
    let region = bytes
        .get(*cursor..end)
        .ok_or_else(|| TrieCodecError::new(format!("truncated {label}")))?;
    *cursor = end;
    Ok(region)
}

#[cfg(test)]
#[path = "../../tests/support/epm1_line_batch.rs"]
mod line_batch_tests;

#[cfg(test)]
#[path = "../../tests/support/substitution_varint.rs"]
mod substitution_varint_tests;

#[cfg(test)]
#[path = "../../tests/support/substitution_value_regions.rs"]
mod substitution_value_regions_tests;

#[cfg(test)]
#[path = "../../tests/support/epm_payload_representation.rs"]
mod payload_representation_tests;

#[cfg(test)]
#[path = "../../tests/support/epm_payload_mapping.rs"]
pub(crate) mod payload_mapping_tests;

#[cfg(test)]
#[path = "../../tests/support/epm_owned_retirement.rs"]
mod owned_retirement_tests;

#[cfg(test)]
#[path = "../../tests/support/epm_owned_cursor.rs"]
mod owned_cursor_tests;

#[cfg(test)]
mod tests {
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    use pathmap::arena_compact::COMPACT_TREE_MAGIC;

    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{EList, Expr, Par};
    use crate::rust::canonical_path::encode_trie_path;
    use crate::rust::rholang::canonical_bytes_arena::{CanonicalBytesArena, CanonicalLocatedByte};

    const ACT_VALUE_FLAG: u8 = 0x40;
    const ACT_LINE_FLAG: u8 = 0x80;

    fn frozen_snapshot(bytes: &[u8], source: u32, base: usize) -> IndexedByteRegion<'static> {
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (index, byte) in bytes.iter().copied().enumerate() {
            handle = arena
                .append(
                    handle,
                    CanonicalLocatedByte::new(
                        byte,
                        CanonicalByteProvenance::new(source, base + index),
                    ),
                )
                .unwrap();
        }
        IndexedByteRegion::from_frozen(
            arena.freeze(handle),
            CanonicalByteProvenance::new(source, base + bytes.len()),
        )
    }

    fn int(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    /// Valid-input measurement control for comparing construction strategies.
    /// It consumes values positionally and inserts them immediately at the
    /// reusable endpoint cursor's current path: there is no `Vec<Par>` or
    /// retained key projection. Production keeps the formally verified event
    /// replay until this control has both semantic and performance authority.
    fn decode_endpoint_insertion_control(
        bytes: &[u8],
    ) -> Result<EPathMapRepr<Par>, TrieCodecError> {
        let framing = parse_pending_framing(bytes)?;
        let mode = EPathMapMode::try_from(framing.raw_mode)?;
        if mode == EPathMapMode::Empty {
            if !framing.arena.is_empty() || framing.value_count != 0 {
                return Err(TrieCodecError::new(
                    "neutral empty EPathMap carries trie or values",
                ));
            }
            return Ok(EPathMapRepr::Empty);
        }

        let mut trie = CanonicalCompactTrieCursor::new(&bytes[framing.arena.clone()])?;
        validate_canonical_act_summary(
            mode,
            framing.value_count,
            trie.is_empty(),
            trie.value_summary(),
        )?;
        let mut value_cursor = framing.value_table_start;
        let mut decoded_values = 0usize;

        match mode {
            EPathMapMode::Set => {
                let mut map = PathMap::new();
                while let Some(endpoint) = trie.next_endpoint() {
                    if endpoint.value.is_some() {
                        map.insert(endpoint.path, ());
                    } else if !endpoint.path.is_empty() {
                        map.create_path(endpoint.path);
                    }
                }
                Ok(EPathMapRepr::Set(map))
            }
            EPathMapMode::Map => {
                let mut map = PathMap::new();
                while let Some(endpoint) = trie.next_endpoint() {
                    if endpoint.value.is_some() {
                        let len = read_len(bytes, &mut value_cursor, "map value length")?;
                        let start = value_cursor;
                        let body = take(bytes, &mut value_cursor, len, "map value payload")?;
                        let value = protobuf_decoder::decode_par(body).map_err(|error| {
                            TrieCodecError::new(format!(
                                "map value {decoded_values} is not a Par: {error}"
                            ))
                        })?;
                        let region = IndexedByteRegion::from_borrowed(body, 0, start);
                        validate_canonical_par_protobuf_indexed(region).map_err(|_| {
                            TrieCodecError::new(format!(
                                "map value {decoded_values} protobuf body is not canonical"
                            ))
                        })?;
                        map.insert(endpoint.path, value);
                        decoded_values += 1;
                    } else if !endpoint.path.is_empty() {
                        map.create_path(endpoint.path);
                    }
                }
                if decoded_values != framing.value_count {
                    return Err(TrieCodecError::new(format!(
                        "map ACTree references {decoded_values} values but EPM1 declares {}",
                        framing.value_count
                    )));
                }
                Ok(EPathMapRepr::Map(map))
            }
            EPathMapMode::Empty => unreachable!("empty mode returned before ACT construction"),
        }
    }

    fn median_duration(mut samples: Vec<Duration>) -> Duration {
        samples.sort_unstable();
        samples[samples.len() / 2]
    }

    fn paths<V: Clone + Send + Sync + Unpin + 'static>(map: &PathMap<V>) -> Vec<Vec<u8>> {
        let mut zipper = map.read_zipper();
        let mut paths = Vec::new();
        while zipper.to_next_val() {
            paths.push(zipper.path().to_vec());
        }
        paths
    }

    fn map_snapshot_from_arena(arena: &[u8], values: &[Par]) -> Vec<u8> {
        let mut snapshot = Vec::new();
        snapshot.extend_from_slice(EPM_MAGIC);
        snapshot.push(EPM_VERSION);
        snapshot.push(EPathMapMode::Map as u8);
        push_varint(&mut snapshot, arena.len() as u64);
        snapshot.extend_from_slice(arena);
        push_varint(&mut snapshot, values.len() as u64);
        for value in values {
            protobuf_encoder::encode_length_delimited_body_into(value, &mut snapshot);
        }
        snapshot
    }

    fn map_snapshot_from_raw_bodies(
        arena: &[u8],
        declared_count: usize,
        bodies: &[&[u8]],
    ) -> Vec<u8> {
        let mut snapshot = Vec::new();
        snapshot.extend_from_slice(EPM_MAGIC);
        snapshot.push(EPM_VERSION);
        snapshot.push(EPathMapMode::Map as u8);
        push_varint(&mut snapshot, arena.len() as u64);
        snapshot.extend_from_slice(arena);
        push_varint(&mut snapshot, declared_count as u64);
        for body in bodies {
            push_varint(&mut snapshot, body.len() as u64);
            snapshot.extend_from_slice(body);
        }
        snapshot
    }

    fn decode_through_owned_pending(bytes: Vec<u8>) -> Result<EPathMapRepr<Par>, TrieCodecError> {
        let mut pending = PendingEpmDecode::new(Bytes::from(bytes), true)?;
        while let Some(payload) = pending.next_value_bytes()? {
            let value = protobuf_decoder::decode_par(payload.as_ref()).map_err(|error| {
                TrieCodecError::new(format!(
                    "map value {} is not a Par: {error}",
                    pending.decoded_values
                ))
            })?;
            pending.push_value(value)?;
        }
        pending.finish()
    }

    fn arena(root: usize, body: &[u8]) -> Vec<u8> {
        let mut arena = COMPACT_TREE_MAGIC.to_vec();
        arena.extend_from_slice(&(root as u64).to_le_bytes());
        arena.extend_from_slice(body);
        arena.extend_from_slice(&[0; 8]);
        arena
    }

    fn reconstruction_oracle(bytes: &[u8]) -> bool {
        let Ok(mut pending) = PendingEpmDecode::new(Bytes::copy_from_slice(bytes), true) else {
            return false;
        };
        loop {
            let payload = match pending.next_value_bytes() {
                Ok(Some(payload)) => payload,
                Ok(None) => break,
                Err(_) => return false,
            };
            let Ok(value) = protobuf_decoder::decode_par(payload) else {
                return false;
            };
            if pending.push_value(value).is_err() {
                return false;
            }
        }
        let Ok(repr) = pending.finish() else {
            return false;
        };
        encode(&repr) == bytes
    }

    #[test]
    fn neutral_empty_round_trips_without_choosing_a_mode() {
        let bytes = encode(&EPathMapRepr::Empty);
        let decoded = decode(&bytes).expect("neutral EPM1 payload decodes");
        assert_eq!(decoded.mode(), EPathMapMode::Empty);
        assert!(decoded.is_empty());
    }

    #[test]
    fn impossible_value_count_is_rejected_before_allocation() {
        // Empty arena, map mode, then a canonical value count of 128 with no
        // bytes left. The decoder must reject from the input-derived lower
        // bound; attempting `Vec::with_capacity(128)` would be harmless here,
        // but the same path with a larger canonical varint is the malformed
        // bincode mutation that previously exhausted the host.
        let bytes = [b'E', b'P', b'M', b'1', EPM_VERSION, 2, 0, 0x80, 0x01];
        let error = match decode(&bytes) {
            Err(error) => error,
            Ok(_) => panic!("the impossible value count must be rejected"),
        };
        assert_eq!(
            error.to_string(),
            "map value count 128 exceeds the 0-byte payload lower bound"
        );
    }

    #[test]
    fn route_hint_reads_only_mode_and_map_cardinality_without_claiming_validation() {
        let empty = encode(&EPathMapRepr::Empty);
        let empty_hint = snapshot_route_hint(&empty).expect("empty EPM1 prefix");
        assert_eq!(empty_hint.mode(), EPathMapMode::Empty);
        assert_eq!(empty_hint.map_value_count(), 0);

        let mut set = PathMap::new();
        set.insert(b"a", ());
        set.insert(b"b", ());
        let set = encode(&EPathMapRepr::Set(set));
        let set_hint = snapshot_route_hint(&set).expect("set EPM1 prefix");
        assert_eq!(set_hint.mode(), EPathMapMode::Set);
        assert_eq!(set_hint.map_value_count(), 0);

        let mut singleton = PathMap::new();
        singleton.insert(b"one", int(1));
        let singleton = encode(&EPathMapRepr::Map(singleton));
        let singleton_hint = snapshot_route_hint(&singleton).expect("singleton map EPM1 prefix");
        assert_eq!(singleton_hint.mode(), EPathMapMode::Map);
        assert_eq!(singleton_hint.map_value_count(), 1);

        let mut wide = PathMap::new();
        wide.insert(b"one", int(1));
        wide.insert(b"two", int(2));
        let wide = encode(&EPathMapRepr::Map(wide));
        let wide_hint = snapshot_route_hint(&wide).expect("multi-value map EPM1 prefix");
        assert_eq!(wide_hint.mode(), EPathMapMode::Map);
        assert_eq!(wide_hint.map_value_count(), 2);

        let malformed_act = [
            b'E',
            b'P',
            b'M',
            b'1',
            EPM_VERSION,
            EPathMapMode::Map as u8,
            1,
            0xff,
            1,
            0,
        ];
        let malformed_hint =
            snapshot_route_hint(&malformed_act).expect("route hint does not validate ACT topology");
        assert_eq!(malformed_hint.mode(), EPathMapMode::Map);
        assert_eq!(malformed_hint.map_value_count(), 1);
        assert!(inspect_canonical_snapshot_value_regions(&malformed_act).is_err());

        assert!(snapshot_route_hint(b"EPM").is_err());
        let impossible_count = [
            b'E',
            b'P',
            b'M',
            b'1',
            EPM_VERSION,
            EPathMapMode::Map as u8,
            0,
            0x80,
            0x01,
        ];
        assert!(snapshot_route_hint(&impossible_count).is_err());
    }

    #[test]
    fn pending_decoder_lends_value_body_from_owned_snapshot() {
        let mut map = PathMap::new();
        map.insert(b"shared/prefix/a", int(1));
        map.insert(b"shared/prefix/b", int(2));
        let snapshot = Bytes::from(encode(&EPathMapRepr::Map(map)));
        let storage_start = snapshot.as_ptr() as usize;
        let storage_end = storage_start + snapshot.len();

        let mut pending = PendingEpmDecode::new(snapshot, true).expect("EPM1 header is valid");
        let body = pending
            .next_value_bytes()
            .expect("canonical value framing")
            .expect("first map value body");
        let body_start = body.as_ptr() as usize;

        assert!(body_start >= storage_start);
        assert!(body_start + body.len() <= storage_end);
    }

    #[test]
    fn sibling_offset_cannot_reenter_its_parent() {
        // A two-child root whose first child occupies bytes 16..18. Advancing
        // to the alleged second sibling lands exactly on the root at byte 18.
        // Without the strict child-before-parent check, the walker repeatedly
        // re-entered that root instead of making progress through the arena.
        let mut arena = vec![0u8; 22];
        arena[..COMPACT_TREE_MAGIC.len()].copy_from_slice(&COMPACT_TREE_MAGIC);
        arena[8..16].copy_from_slice(&18u64.to_le_bytes());
        arena[16] = ACT_VALUE_FLAG;
        arena[17] = 0;
        arena[18] = 2;
        arena[19] = 2;
        arena[20] = 0;
        arena[21] = 1;

        let error = visit_act(&arena, |_path, _value| Ok(())).unwrap_err();
        assert_eq!(
            error.to_string(),
            "ACTree03 child does not precede its parent"
        );
    }

    #[test]
    fn set_round_trip_preserves_prefix_compressed_keys() {
        let mut map = PathMap::new();
        map.insert(b"prefix/a", ());
        map.insert(b"prefix/b", ());
        map.insert(b"prefix/deep/c", ());
        let expected = paths(&map);
        let bytes = encode(&EPathMapRepr::Set(map));
        let decoded = decode(&bytes).expect("set EPM1 payload decodes");
        let EPathMapRepr::Set(map) = decoded else {
            panic!("set EPM1 payload changed mode");
        };
        assert_eq!(paths(&map), expected);
        assert!(bytes
            .windows(COMPACT_TREE_MAGIC.len())
            .any(|w| w == COMPACT_TREE_MAGIC));
    }

    #[test]
    fn map_round_trip_uses_generated_stack_safe_value_codecs() {
        let mut map = PathMap::new();
        map.insert(b"same/prefix/a", int(1));
        map.insert(b"same/prefix/b", int(2));
        let bytes = encode(&EPathMapRepr::Map(map));
        let decoded = decode(&bytes).expect("map EPM1 payload decodes");
        let EPathMapRepr::Map(map) = decoded else {
            panic!("map EPM1 payload changed mode");
        };
        assert_eq!(
            protobuf_encoder::encode_to_vec(map.get(b"same/prefix/a").expect("a exists")),
            protobuf_encoder::encode_to_vec(&int(1))
        );
        assert_eq!(
            protobuf_encoder::encode_to_vec(map.get(b"same/prefix/b").expect("b exists")),
            protobuf_encoder::encode_to_vec(&int(2))
        );
    }

    #[test]
    fn endpoint_insertion_control_is_equivalent_and_measurable() {
        let entries = std::env::var("EPM1_ENDPOINT_CONTROL_ENTRIES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(256usize);
        let reps = std::env::var("EPM1_ENDPOINT_CONTROL_REPS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(3usize)
            .max(1);
        let value_bytes = 64usize;
        let mut map = PathMap::new();
        for index in 0..entries {
            let key = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![int(7), int(11), int(13), int(index as i64)],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }]);
            let value = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString(format!(
                    "{index:08}-{}",
                    "v".repeat(value_bytes)
                ))),
            }]);
            map.insert(encode_trie_path(&key), value);
        }
        let snapshot = encode(&EPathMapRepr::Map(map));

        let current = decode(&snapshot).expect("production positional replay");
        let endpoint =
            decode_endpoint_insertion_control(&snapshot).expect("endpoint insertion control");
        assert_eq!(encode(&current), snapshot);
        assert_eq!(encode(&endpoint), snapshot);

        let mut current_samples = Vec::with_capacity(reps);
        let mut endpoint_samples = Vec::with_capacity(reps);
        for _ in 0..reps {
            let started = Instant::now();
            black_box(decode(black_box(&snapshot)).expect("production positional replay"));
            current_samples.push(started.elapsed());

            let started = Instant::now();
            black_box(
                decode_endpoint_insertion_control(black_box(&snapshot))
                    .expect("endpoint insertion control"),
            );
            endpoint_samples.push(started.elapsed());
        }
        let current = median_duration(current_samples);
        let endpoint = median_duration(endpoint_samples);
        if std::env::var_os("EPM1_ENDPOINT_CONTROL_REPS").is_some() {
            println!(
                "epm1 endpoint construction control: entries={entries}, current={} ns, endpoint={} ns, current/endpoint={:.3}",
                current.as_nanos(),
                endpoint.as_nanos(),
                current.as_nanos() as f64 / endpoint.as_nanos() as f64
            );
        }
    }

    #[test]
    fn pathmap_anamorphism_prunes_topology_only_leaves() {
        let map = PathMap::<()>::new_from_ana(0u8, |state, _value, children, _path| {
            if state == 0 {
                children.push(b"topology-only", 1);
            }
        });
        assert!(
            !map.path_exists_at(b"topology-only"),
            "PathMap 0.3.4 prunes value-free anamorphic leaves; EPM1 cannot use a bare Map skeleton"
        );
        assert!(!map.contains(b"topology-only"));
    }

    #[test]
    fn epm1_map_layout_writes_zero_marker_for_every_value_occurrence() {
        let mut map = PathMap::new();
        map.write_zipper().set_val(int(0));
        map.insert(b"prefix", int(1));
        map.insert(b"prefix/child", int(2));
        map.insert(b"other", int(3));
        map.create_path(b"topology/only");

        let snapshot = encode(&EPathMapRepr::Map(map));
        let prefix = parse_canonical_snapshot_prefix(&snapshot).expect("canonical EPM1 prefix");
        let mut occurrences = 0usize;
        visit_act(prefix.arena, |_path, marker| {
            if let Some(marker) = marker {
                assert_eq!(marker, 0, "EPM1 map markers are presence-only");
                occurrences += 1;
            }
            Ok(())
        })
        .expect("canonical ACT image");
        assert_eq!(occurrences, prefix.value_count);
        assert_eq!(occurrences, 4);
    }

    #[test]
    fn positional_replay_preserves_root_prefix_dense_and_topology_only_nodes() {
        let mut map = PathMap::new();
        map.write_zipper().set_val(int(-1));
        map.insert(b"a", int(1));
        map.insert(b"a/proper-prefix", int(2));
        for byte in 0u8..=u8::MAX {
            map.insert([b'd', byte], int(i64::from(byte) + 10));
        }
        map.create_path(b"topology/only");

        let snapshot = encode(&EPathMapRepr::Map(map));
        let decoded = decode(&snapshot).expect("positional EPM1 replay");
        let EPathMapRepr::Map(decoded) = decoded else {
            panic!("map mode changed during positional replay");
        };

        assert_eq!(decoded.get([]), Some(&int(-1)));
        assert_eq!(decoded.get(b"a"), Some(&int(1)));
        assert_eq!(decoded.get(b"a/proper-prefix"), Some(&int(2)));
        for byte in 0u8..=u8::MAX {
            assert_eq!(decoded.get([b'd', byte]), Some(&int(i64::from(byte) + 10)));
        }
        assert!(decoded.path_exists_at(b"topology/only"));
        assert!(!decoded.contains(b"topology/only"));
        assert_eq!(encode(&EPathMapRepr::Map(decoded)), snapshot);
    }

    #[test]
    fn positional_occurrences_ignore_shared_cow_par_addresses() {
        let mut shared = PathMap::new();
        shared.insert(b"leaf", int(7));

        let mut map = PathMap::new();
        map.write_zipper_at_path(b"left/").graft_map(shared.clone());
        map.insert(b"middle", int(9));
        map.write_zipper_at_path(b"right/").graft_map(shared);

        let left = map.get(b"left/leaf").expect("left shared leaf");
        let right = map.get(b"right/leaf").expect("right shared leaf");
        assert!(
            std::ptr::eq(left, right),
            "fixture must expose one CoW value address at two trie occurrences"
        );

        let snapshot = encode(&EPathMapRepr::Map(map));
        let mut independently_allocated = PathMap::new();
        independently_allocated.insert(b"left/leaf", int(7));
        independently_allocated.insert(b"middle", int(9));
        independently_allocated.insert(b"right/leaf", int(7));
        assert_eq!(
            snapshot,
            encode(&EPathMapRepr::Map(independently_allocated)),
            "CoW sharing and independently allocated equal values must have one EPM1 image"
        );
        let hint = snapshot_route_hint(&snapshot).expect("canonical positional prefix");
        assert_eq!(hint.map_value_count(), 3);
        let EPathMapRepr::Map(decoded) = decode(&snapshot).expect("shared occurrence replay")
        else {
            panic!("shared map changed mode");
        };
        assert_eq!(decoded.get(b"left/leaf"), Some(&int(7)));
        assert_eq!(decoded.get(b"middle"), Some(&int(9)));
        assert_eq!(decoded.get(b"right/leaf"), Some(&int(7)));
    }

    #[test]
    fn certified_and_traversed_layout_counts_are_identical_for_every_mode() {
        fn assert_layout<T>(repr: &EPathMapRepr<T>, certified: usize)
        where T: Clone + Send + Sync + Unpin + 'static {
            let traversed = layout(repr);
            let maintained = layout_with_certified_value_count(repr, Some(certified));
            assert_eq!(maintained.prefix(), traversed.prefix());
            assert_eq!(maintained.value_count(), traversed.value_count());
        }

        assert_layout(&EPathMapRepr::<Par>::Empty, 0);

        let mut set = PathMap::new();
        set.insert(b"set/a", ());
        set.insert(b"set/b", ());
        set.create_path(b"set/topology-only");
        assert_layout(&EPathMapRepr::<Par>::Set(set), 0);

        let mut map = PathMap::new();
        map.write_zipper().set_val(int(0));
        map.insert(b"map/a", int(1));
        map.insert(b"map/b", int(2));
        map.create_path(b"map/topology-only");
        assert_layout(&EPathMapRepr::Map(map), 3);
    }

    #[test]
    fn missing_and_extra_framed_map_bodies_are_rejected_immediately() {
        let one_value_arena = arena(16, &[ACT_VALUE_FLAG, 0]);

        let missing = map_snapshot_from_raw_bodies(&one_value_arena, 1, &[]);
        let missing_error = PendingEpmDecode::new(Bytes::from(missing), true)
            .err()
            .expect("missing framed body must fail construction");
        assert_eq!(
            missing_error.to_string(),
            "map value count 1 exceeds the 0-byte payload lower bound"
        );

        let extra = map_snapshot_from_raw_bodies(&one_value_arena, 0, &[&[]]);
        let extra_error = PendingEpmDecode::new(Bytes::from(extra), true)
            .err()
            .expect("extra framed body must fail construction");
        assert_eq!(extra_error.to_string(), "trailing bytes after EPM1 payload");
    }

    #[test]
    fn pending_decoder_publishes_only_from_successful_finish_without_partial_result() {
        let one_value_arena = arena(16, &[ACT_VALUE_FLAG, 0]);
        let snapshot = map_snapshot_from_raw_bodies(&one_value_arena, 1, &[&[]]);

        let incomplete =
            PendingEpmDecode::new(Bytes::from(snapshot.clone()), true).expect("canonical framing");
        assert_eq!(
            incomplete
                .finish()
                .err()
                .expect("an undecoded body cannot publish a partial map")
                .to_string(),
            "fewer map values were decoded than the EPM1 value count"
        );

        let result = decode_through_owned_pending(snapshot).expect("complete pending replay");
        let EPathMapRepr::Map(map) = result else {
            panic!("successful finish changed map mode");
        };
        assert_eq!(map.get([]), Some(&Par::default()));
    }

    #[test]
    fn child_errors_precede_deferred_act_and_noncanonical_body_errors() {
        let nonzero_marker_arena = arena(16, &[ACT_VALUE_FLAG, 1]);
        let malformed = map_snapshot_from_raw_bodies(&nonzero_marker_arena, 1, &[&[0x0f]]);
        let child_error = decode_through_owned_pending(malformed)
            .err()
            .expect("malformed child must stop the generated pending path");
        assert!(
            child_error
                .to_string()
                .starts_with("map value 0 is not a Par:"),
            "child error lost precedence: {child_error}"
        );

        // Prost and the generated child PDA accept explicit false, but the
        // byte-native admission pass rejects it as a noncanonical default.
        let explicit_false = [0x50, 0x00];
        assert_eq!(
            protobuf_decoder::decode_par(explicit_false.as_slice())
                .expect("permissive child decode"),
            Par::default()
        );
        let deferred = map_snapshot_from_raw_bodies(&nonzero_marker_arena, 1, &[&explicit_false]);
        assert_eq!(
            decode_through_owned_pending(deferred)
                .err()
                .expect("invalid ACT must reject after the child succeeds")
                .to_string(),
            "map ACTree value marker 1 is not zero"
        );
    }

    #[test]
    fn strict_indexed_raw_par_admission_rejects_alias_without_reencoding() {
        let canonical_marker_arena = arena(16, &[ACT_VALUE_FLAG, 0]);
        let explicit_false = [0x50, 0x00];
        let snapshot = map_snapshot_from_raw_bodies(&canonical_marker_arena, 1, &[&explicit_false]);
        assert_eq!(
            decode_through_owned_pending(snapshot)
                .err()
                .expect("explicit default must fail strict byte-native admission")
                .to_string(),
            "map value 0 protobuf body is not canonical"
        );
    }

    #[test]
    fn synchronous_decode_and_generated_pending_core_are_byte_identical() {
        let mut map = PathMap::new();
        map.write_zipper().set_val(int(0));
        map.insert(b"shared/a", int(1));
        map.insert(b"shared/b", int(2));
        map.create_path(b"topology/only");
        let snapshot = encode(&EPathMapRepr::Map(map));

        let synchronous = decode(&snapshot).expect("synchronous positional decode");
        let generated_core = decode_through_owned_pending(snapshot.clone())
            .expect("owned pending positional decode");
        assert_eq!(encode(&synchronous), snapshot);
        assert_eq!(encode(&generated_core), snapshot);
    }

    #[test]
    fn positional_decoder_is_stack_safe_on_a_small_stack_at_depth_4096() {
        const DEPTH: usize = 4_096;
        const STACK: usize = 256 * 1024;

        let key = vec![0xa5; DEPTH];
        let mut map = PathMap::new();
        map.insert(&key, int(1));
        let snapshot = encode(&EPathMapRepr::Map(map));

        std::thread::Builder::new()
            .name("positional-epm1-decode-depth".to_owned())
            .stack_size(STACK)
            .spawn(move || {
                let EPathMapRepr::Map(decoded) =
                    decode(&snapshot).expect("deep positional EPM1 decode")
                else {
                    panic!("deep map changed mode");
                };
                assert_eq!(decoded.get(&key), Some(&int(1)));
            })
            .expect("spawn fixed small-stack positional decode probe")
            .join()
            .expect("positional decode and linear PathMap drop remain stack-safe");
    }

    #[test]
    fn map_count_crossing_u32_boundary_is_rejected_without_allocation() {
        let mut snapshot = Vec::new();
        snapshot.extend_from_slice(EPM_MAGIC);
        snapshot.push(EPM_VERSION);
        snapshot.push(EPathMapMode::Map as u8);
        push_varint(&mut snapshot, 0);
        push_varint(&mut snapshot, u64::from(u32::MAX) + 1);

        let error = PendingEpmDecode::new(Bytes::from(snapshot), true)
            .err()
            .expect("impossible boundary count must be rejected");
        #[cfg(target_pointer_width = "32")]
        assert_eq!(error.to_string(), "map value count overflows usize");
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            error.to_string(),
            "map value count 4294967296 exceeds the 0-byte payload lower bound"
        );
    }

    #[test]
    fn encoded_view_lookup_borrows_the_first_requested_act_value() {
        let mut map = PathMap::new();
        map.insert(b"same/prefix/a", int(1));
        map.insert(b"same/prefix/b", int(2));
        let bytes = encode(&EPathMapRepr::Map(map));

        let selected = canonical_map_value_body_at_ordered_keys(&bytes, &[
            b"absent",
            b"same/prefix/b",
            b"same/prefix/a",
        ])
        .expect("canonical EPM1 snapshot")
        .expect("the second requested key exists");
        assert_eq!(selected, protobuf_encoder::encode_to_vec(&int(2)));
        assert!(selected.as_ptr() >= bytes.as_ptr());
        assert!(selected.as_ptr() < bytes[bytes.len()..].as_ptr());

        let first =
            canonical_map_value_body_at_ordered_keys(&bytes, &[b"same/prefix/a", b"same/prefix/b"])
                .expect("canonical EPM1 snapshot")
                .expect("the first requested key exists");
        assert_eq!(first, protobuf_encoder::encode_to_vec(&int(1)));
        assert_eq!(
            canonical_map_value_body_at_ordered_keys(&bytes, &[b"absent"])
                .expect("canonical EPM1 snapshot"),
            None
        );
    }

    fn collected_map_value_body_at_ordered_keys<'a>(
        bytes: &'a [u8],
        ordered_keys: &[&[u8]],
    ) -> Result<Option<&'a [u8]>, TrieCodecError> {
        let inspection = inspect_canonical_map_snapshot(bytes)?;
        for requested in ordered_keys {
            for (key, value_body) in inspection.keys.iter().zip(&inspection.value_bodies) {
                let candidate = match key {
                    CanonicalSnapshotKey::Relative(range) => &bytes[range.clone()],
                    CanonicalSnapshotKey::Owned(key) => key.as_slice(),
                };
                if candidate == *requested {
                    return Ok(Some(&bytes[value_body.clone()]));
                }
            }
        }
        Ok(None)
    }

    fn lookup_image(entries: &std::collections::BTreeSet<Vec<u8>>, split_mask: &[bool]) -> Vec<u8> {
        let mut map = PathMap::new();
        let mut value = 0i64;
        for (key, &split) in entries.iter().zip(split_mask) {
            map.insert(key.as_slice(), int(value));
            value += 1;
            if split {
                let mut split_key = Vec::with_capacity(key.len() + 1);
                split_key.extend_from_slice(key);
                split_key.push(0);
                map.insert(split_key, int(value));
                value += 1;
            }
        }
        encode(&EPathMapRepr::Map(map))
    }

    fn oracle_preorder_keys(bytes: &[u8]) -> Vec<Vec<u8>> {
        inspect_canonical_map_snapshot(bytes)
            .expect("a generated map encodes a canonical map-mode EPM1 image")
            .keys
            .into_iter()
            .map(|key| match key {
                CanonicalSnapshotKey::Relative(range) => bytes[range].to_vec(),
                CanonicalSnapshotKey::Owned(key) => key,
            })
            .collect()
    }

    fn body_location(
        bytes: &[u8],
        lookup: Result<Option<&[u8]>, TrieCodecError>,
    ) -> Result<Option<Range<usize>>, TrieCodecError> {
        lookup.map(|body| {
            body.map(|body| {
                let start = body
                    .as_ptr()
                    .addr()
                    .checked_sub(bytes.as_ptr().addr())
                    .expect("a looked-up value body is borrowed from the image");
                start..start + body.len()
            })
        })
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        #[test]
        fn cursor_map_value_lookup_equals_the_collected_oracle(
            entries in proptest::collection::btree_set(
                proptest::collection::vec(0u8..3, 0..5),
                1..12,
            ),
            split_mask in proptest::collection::vec(proptest::bool::ANY, 12),
            requests in proptest::collection::vec(proptest::collection::vec(0u8..4, 0..6), 1..4),
        ) {
            let bytes = lookup_image(&entries, &split_mask);
            let preorder = oracle_preorder_keys(&bytes);
            let first = preorder.first().expect("a generated map has an entry").clone();
            let last = preorder.last().expect("a generated map has an entry").clone();
            let absent = vec![3u8];
            let mut cases =
                Vec::with_capacity(6 + preorder.iter().map(|key| 3 + key.len()).sum::<usize>());
            cases.push(vec![first.clone()]);
            cases.push(vec![last.clone()]);
            cases.push(vec![absent.clone()]);
            cases.push(vec![absent, last.clone(), first.clone()]);
            cases.push(vec![last, first]);
            cases.push(requests);
            for key in &preorder {
                let mut split = Vec::with_capacity(key.len() + 1);
                split.extend_from_slice(key);
                split.push(0);
                cases.push(vec![key.clone()]);
                cases.push(vec![split.clone()]);
                cases.push(vec![key.clone(), split]);
                cases.extend((0..key.len()).map(|end| vec![key[..end].to_vec()]));
            }
            for case in &cases {
                let ordered_keys: Vec<&[u8]> = case.iter().map(Vec::as_slice).collect();
                let looked_up = canonical_map_value_body_at_ordered_keys(&bytes, &ordered_keys);
                let collected = collected_map_value_body_at_ordered_keys(&bytes, &ordered_keys);
                proptest::prop_assert_eq!(
                    body_location(&bytes, looked_up),
                    body_location(&bytes, collected),
                    "ordered keys {:?}",
                    ordered_keys
                );
            }
        }

        #[test]
        fn cursor_map_value_lookup_returns_the_oracle_result_on_mutated_images(
            entries in proptest::collection::btree_set(
                proptest::collection::vec(0u8..3, 0..5),
                1..12,
            ),
            split_mask in proptest::collection::vec(proptest::bool::ANY, 12),
            mutation in 0u8..4,
            position in proptest::num::usize::ANY,
            byte in proptest::num::u8::ANY,
        ) {
            let bytes = lookup_image(&entries, &split_mask);
            let preorder = oracle_preorder_keys(&bytes);
            let mut image = bytes.clone();
            match mutation {
                0 => image.truncate(position % bytes.len()),
                1 => image[position % bytes.len()] ^= byte.max(1),
                2 => image.push(byte),
                _ => image[EPM_MAGIC.len() + 1] = byte % 4,
            }
            let first = preorder.first().expect("a generated map has an entry").as_slice();
            let last = preorder.last().expect("a generated map has an entry").as_slice();
            let absent: &[u8] = &[3];
            let mut cases: Vec<Vec<&[u8]>> = Vec::with_capacity(4 + preorder.len());
            cases.push(vec![first]);
            cases.push(vec![last]);
            cases.push(vec![absent]);
            cases.push(vec![absent, last, first]);
            cases.extend(preorder.iter().map(|key| vec![key.as_slice()]));
            for ordered_keys in &cases {
                let looked_up = canonical_map_value_body_at_ordered_keys(&image, ordered_keys);
                let collected = collected_map_value_body_at_ordered_keys(&image, ordered_keys);
                proptest::prop_assert_eq!(
                    body_location(&image, looked_up),
                    body_location(&image, collected),
                    "mutation {} at {} with {} on ordered keys {:?}",
                    mutation,
                    position,
                    byte,
                    ordered_keys
                );
            }
        }
    }

    #[test]
    fn map_value_lookup_allocates_only_the_validating_cursor_walk() {
        let mut map = PathMap::new();
        for index in 0..4096i64 {
            map.insert(format!("same/prefix/{index:04}"), int(index));
        }
        let bytes = encode(&EPathMapRepr::Map(map));
        let preorder = oracle_preorder_keys(&bytes);
        assert_eq!(preorder.len(), 4096);
        let arena = parse_canonical_snapshot_prefix(&bytes)
            .expect("canonical EPM1 snapshot")
            .arena;
        let walk = |calls: usize| {
            super::payload_mapping_tests::capture_allocations(|| {
                let mut trie = CanonicalCompactTrieCursor::new(arena).expect("canonical ACT image");
                for _ in 0..calls {
                    trie.next_value_endpoint();
                }
            })
            .1
        };
        let first: &[u8] = b"same/prefix/0000";
        let last: &[u8] = b"same/prefix/4095";
        let absent: &[u8] = b"same/prefix/4096";
        let cases: [(&[&[u8]], Option<i64>); 4] = [
            (&[absent], None),
            (&[first], Some(0)),
            (&[last], Some(4095)),
            (&[absent, last, first], Some(4095)),
        ];
        for (ordered_keys, expected) in cases {
            let first_key = ordered_keys[0];
            let visited = match preorder.iter().position(|key| key.as_slice() == first_key) {
                Some(ordinal) => ordinal + 1,
                None => preorder.len() + 1,
            };
            let (found, counts) = super::payload_mapping_tests::capture_allocations(|| {
                canonical_map_value_body_at_ordered_keys(&bytes, ordered_keys)
            });
            assert_eq!(
                found.expect("canonical EPM1 snapshot"),
                expected
                    .map(|value| protobuf_encoder::encode_to_vec(&int(value)))
                    .as_deref()
            );
            assert_eq!(
                counts,
                walk(visited),
                "the lookup allocated beyond its validating cursor walk"
            );
        }
    }

    #[test]
    fn canonical_epm1_cursor_lends_keys_and_value_regions_in_positional_order() {
        let mut map = PathMap::new();
        map.insert(b"same/prefix/a", int(1));
        map.insert(b"same/prefix/b", int(2));
        map.insert(b"other/branch/c", int(3));
        map.create_path(b"value/free/endpoint");
        let bytes = encode(&EPathMapRepr::Map(map));
        let inspection = inspect_canonical_snapshot(&bytes).expect("collected oracle");
        let mut cursor = CanonicalEpm1Cursor::new(&bytes).expect("streaming cursor");

        assert_eq!(cursor.mode(), EPathMapMode::Map);
        assert_eq!(cursor.value_count(), inspection.value_bodies.len());
        for (expected_key, expected_body) in inspection.keys.iter().zip(&inspection.value_bodies) {
            let entry = cursor.next_entry().expect("one streamed map entry");
            let expected_key = match expected_key {
                CanonicalSnapshotKey::Relative(range) => &bytes[range.clone()],
                CanonicalSnapshotKey::Owned(key) => key,
            };
            assert_eq!(entry.key(), expected_key);
            assert_eq!(entry.value_body(), Some(expected_body.clone()));
        }
        assert!(cursor.next_entry().is_none());

        let mut set = PathMap::new();
        set.insert(b"set/a", ());
        set.insert(b"set/b", ());
        set.create_path(b"set/value-free");
        let expected_keys = paths(&set);
        let bytes = encode(&EPathMapRepr::Set(set));
        let mut cursor = CanonicalEpm1Cursor::new(&bytes).expect("set cursor");
        assert_eq!(cursor.mode(), EPathMapMode::Set);
        assert_eq!(cursor.value_count(), 0);
        for expected_key in &expected_keys {
            let entry = cursor.next_entry().expect("one streamed set entry");
            assert_eq!(entry.key(), expected_key);
            assert_eq!(entry.value_body(), None);
        }
        assert!(cursor.next_entry().is_none());
    }

    #[test]
    fn owned_epm1_cursor_matches_borrowed_and_shares_value_bodies() {
        fn assert_match(bytes: Vec<u8>) {
            let shared = Bytes::from(bytes.clone());
            let shared_start = shared.as_ptr() as usize;
            let shared_end = shared_start + shared.len();
            let mut borrowed = CanonicalEpm1Cursor::new(&bytes).expect("borrowed cursor");
            let mut owned = OwnedCanonicalEpm1Cursor::new(shared.clone()).expect("owned cursor");
            assert_eq!(owned.mode(), borrowed.mode());
            assert_eq!(owned.value_count(), borrowed.value_count());

            loop {
                match (borrowed.next_entry(), owned.next_entry()) {
                    (Some(borrowed), Some(owned)) => {
                        assert_eq!(owned.key(), borrowed.key());
                        let owned_body = owned.value_body();
                        let borrowed_range = borrowed.value_body();
                        assert_eq!(owned.value_body_range(), borrowed_range);
                        match (borrowed_range, owned_body.as_ref()) {
                            (Some(range), Some(body)) => {
                                assert_eq!(body.as_ref(), &bytes[range]);
                                let body_start = body.as_ptr() as usize;
                                assert!(body_start >= shared_start);
                                assert!(body_start + body.len() <= shared_end);
                            }
                            (None, None) => {}
                            _ => panic!("borrowed and owned value regions differ"),
                        }
                    }
                    (None, None) => break,
                    _ => panic!("borrowed and owned entry counts differ"),
                }
            }
        }

        assert_match(encode(&EPathMapRepr::<Par>::Empty));

        let mut set = PathMap::new();
        set.insert(b"same/prefix/a", ());
        set.insert(b"same/prefix/b", ());
        set.create_path(b"value/free/endpoint");
        assert_match(encode(&EPathMapRepr::<Par>::Set(set)));

        let mut map = PathMap::new();
        map.insert(b"same/prefix/a", int(1));
        map.insert(b"same/prefix/b", int(2));
        map.insert(b"other/branch/c", int(3));
        map.create_path(b"value/free/endpoint");
        assert_match(encode(&EPathMapRepr::Map(map)));
    }

    #[test]
    fn owned_topology_cursor_matches_borrowed_events_and_reports_source_offsets() {
        fn assert_match(snapshot: Vec<u8>) {
            let prefix = parse_canonical_snapshot_prefix(&snapshot).expect("canonical prefix");
            let arena_start = prefix.arena_start;
            let arena_end = arena_start + prefix.arena.len();
            let mut expected = Vec::new();
            if prefix.mode == EPathMapMode::Empty {
                expected.push(CompactTrieTopologyEvent::Value(false));
                expected.push(CompactTrieTopologyEvent::Leave);
            } else {
                CompactTrie::parse_canonical(prefix.arena)
                    .expect("canonical borrowed ACT")
                    .visit_topology(|event| {
                        expected.push(event);
                        Ok::<(), ()>(())
                    })
                    .expect("infallible borrowed event collection");
            }

            let shared = Bytes::from(snapshot.clone());
            let shared_start = shared.as_ptr() as usize;
            let shared_end = shared_start + shared.len();
            let mut cursor =
                OwnedCanonicalEpm1TopologyCursor::new(shared.clone()).expect("owned topology");
            assert_eq!(cursor.mode(), prefix.mode);
            assert_eq!(cursor.value_count(), prefix.value_count);
            let mut actual = Vec::new();
            let mut length = 0usize;
            while let Some(event) = cursor.next_event() {
                let logical = match event {
                    OwnedCanonicalEpm1TopologyEvent::Value {
                        has_value,
                        value_body_range,
                        value_body,
                        source_offset,
                    } => {
                        if prefix.mode == EPathMapMode::Empty {
                            assert_eq!(source_offset, EPM_MAGIC.len() + 1);
                        } else {
                            assert!(source_offset >= arena_start + 16);
                            assert!(source_offset < arena_end - 8);
                        }
                        if prefix.mode == EPathMapMode::Map && has_value {
                            let range = value_body_range.expect("map value body range");
                            let body = value_body.expect("shared map value body");
                            assert_eq!(body.as_ref(), &snapshot[range]);
                            let body_start = body.as_ptr() as usize;
                            assert!(body_start >= shared_start);
                            assert!(body_start + body.len() <= shared_end);
                        } else {
                            assert_eq!(value_body_range, None);
                            assert_eq!(value_body, None);
                        }
                        if has_value {
                            length += 1;
                        }
                        CompactTrieTopologyEvent::Value(has_value)
                    }
                    OwnedCanonicalEpm1TopologyEvent::Edge {
                        byte,
                        source_offset,
                    } => {
                        assert!(source_offset >= arena_start + 16);
                        assert!(source_offset < arena_end - 8);
                        CompactTrieTopologyEvent::Edge(byte)
                    }
                    OwnedCanonicalEpm1TopologyEvent::Leave { source_offset } => {
                        if prefix.mode == EPathMapMode::Empty {
                            assert_eq!(source_offset, EPM_MAGIC.len() + 1);
                        } else {
                            assert!(source_offset >= arena_start + 16);
                            assert!(source_offset < arena_end - 8);
                        }
                        CompactTrieTopologyEvent::Leave
                    }
                };
                actual.push(logical);
            }
            assert_eq!(actual, expected);
            assert_eq!(cursor.length(), length);
        }

        assert_match(encode(&EPathMapRepr::<Par>::Empty));

        let mut set = PathMap::new();
        set.insert(b"shared/line/a", ());
        set.insert(b"shared/line/b", ());
        set.create_path(b"shared/dangling");
        for byte in 0u8..=63 {
            set.insert([b'd', byte], ());
        }
        assert_match(encode(&EPathMapRepr::<Par>::Set(set)));

        let mut map = PathMap::new();
        map.insert(b"same/suffix/x", int(1));
        map.insert(b"other/suffix/x", int(2));
        map.insert(b"same/suffix/y", int(3));
        map.create_path(b"value/free/endpoint");
        assert_match(encode(&EPathMapRepr::Map(map)));
    }

    #[test]
    fn indexed_topology_cursor_matches_owned_events_on_contiguous_and_frozen_sources() {
        fn assert_indexed(snapshot: Vec<u8>) {
            const SOURCE: u32 = 73;
            const BASE: usize = 4_096;

            let mut owned = OwnedCanonicalEpm1TopologyCursor::new(Bytes::from(snapshot.clone()))
                .expect("owned topology");
            let frozen = frozen_snapshot(&snapshot, SOURCE, BASE);
            let mut indexed =
                IndexedCanonicalEpm1TopologyCursor::new(frozen).expect("frozen topology");
            assert_eq!(indexed.mode(), owned.mode());
            assert_eq!(indexed.value_count(), owned.value_count());
            assert_eq!(indexed.length(), owned.length());

            loop {
                let indexed_observed = indexed
                    .next_event_with_source()
                    .expect("validated frozen event and source receipt");
                let map_receipt = indexed_observed
                    .as_ref()
                    .and_then(|observed| observed.map_value_receipt);
                match (
                    owned.next_event(),
                    indexed_observed.map(|observed| observed.event),
                ) {
                    (
                        Some(OwnedCanonicalEpm1TopologyEvent::Value {
                            has_value: owned_has_value,
                            value_body_range: owned_range,
                            value_body: owned_body,
                            source_offset,
                        }),
                        Some(IndexedCanonicalEpm1TopologyEvent::Value {
                            has_value,
                            value_body_range,
                            value_body,
                            provenance,
                            ..
                        }),
                    ) => {
                        assert_eq!(has_value, owned_has_value);
                        assert_eq!(value_body_range, owned_range);
                        assert_eq!(provenance.source, SOURCE);
                        assert_eq!(provenance.offset, BASE + source_offset);
                        match (value_body, owned_body, value_body_range) {
                            (Some(indexed_body), Some(owned_body), Some(range)) => {
                                let receipt = map_receipt
                                    .expect("each indexed Map body has one source receipt");
                                assert_eq!(receipt.body_range.start..receipt.body_range.end, range);
                                assert_eq!(receipt.act_source, provenance);
                                assert!(indexed_body.content_eq(
                                    &IndexedByteRegion::from_protobuf_region(
                                        ProtobufRegion::copy_from_slice(owned_body.as_ref()),
                                        0,
                                    )
                                ));
                                let body_start = indexed_body.position_at(0).unwrap().physical;
                                assert_eq!(body_start.source, SOURCE);
                                assert_eq!(body_start.offset, BASE + range.start);
                            }
                            (None, None, None) => assert!(map_receipt.is_none()),
                            _ => panic!("indexed and owned value bodies differ"),
                        }
                    }
                    (
                        Some(OwnedCanonicalEpm1TopologyEvent::Edge {
                            byte: owned_byte,
                            source_offset,
                        }),
                        Some(IndexedCanonicalEpm1TopologyEvent::Edge { byte, provenance }),
                    ) => {
                        assert_eq!(byte, owned_byte);
                        assert_eq!(provenance.source, SOURCE);
                        assert_eq!(provenance.offset, BASE + source_offset);
                    }
                    (
                        Some(OwnedCanonicalEpm1TopologyEvent::Leave { source_offset }),
                        Some(IndexedCanonicalEpm1TopologyEvent::Leave { provenance }),
                    ) => {
                        assert_eq!(provenance.source, SOURCE);
                        assert_eq!(provenance.offset, BASE + source_offset);
                    }
                    (None, None) => break,
                    _ => panic!("indexed and owned topology events differ"),
                }
            }
        }

        assert_indexed(encode(&EPathMapRepr::<Par>::Empty));

        let mut set = PathMap::new();
        set.insert(b"indexed/set/a", ());
        set.insert(b"indexed/set/b", ());
        set.create_path(b"indexed/value-free");
        assert_indexed(encode(&EPathMapRepr::<Par>::Set(set)));

        let mut map = PathMap::new();
        map.insert(b"indexed/map/a", int(1));
        map.insert(b"indexed/map/b", int(2));
        map.create_path(b"indexed/value-free");
        assert_indexed(encode(&EPathMapRepr::Map(map)));
    }

    #[test]
    fn indexed_topology_errors_match_contiguous_and_retain_physical_provenance() {
        const SOURCE: u32 = 91;
        const BASE: usize = 8_192;

        fn frozen_error(bytes: &[u8]) -> (TrieCodecError, bool) {
            let snapshot = frozen_snapshot(bytes, SOURCE, BASE);
            let mut cursor = match IndexedCanonicalEpm1TopologyCursor::new(snapshot) {
                Ok(cursor) => cursor,
                Err(error) => return (error, false),
            };
            loop {
                match cursor.next_event() {
                    Ok(Some(_)) => {}
                    Ok(None) => panic!("malformed indexed EPM1 reached end of stream"),
                    Err(error) => {
                        let stats = cursor.stats();
                        let repeated = cursor
                            .next_event()
                            .expect_err("an indexed terminal error remains latched");
                        assert_eq!(repeated, error);
                        assert_eq!(cursor.stats(), stats);
                        return (error, true);
                    }
                }
            }
        }

        let mut invalid_magic = encode(&EPathMapRepr::<Par>::Empty);
        invalid_magic[0] ^= 1;

        let mut trailing = encode(&EPathMapRepr::<Par>::Empty);
        trailing.push(0);

        let mut set = PathMap::new();
        set.insert(b"set/value", ());
        let mut set_with_value = encode(&EPathMapRepr::<Par>::Set(set));
        *set_with_value.last_mut().unwrap() = 1;
        set_with_value.push(0);

        let mut map = PathMap::new();
        map.insert(b"map/value", int(1));
        let mut map_count_mismatch = encode(&EPathMapRepr::Map(map));
        let count_offset = parse_canonical_snapshot_prefix(&map_count_mismatch)
            .unwrap()
            .value_table_start
            - 1;
        map_count_mismatch[count_offset] = 2;
        map_count_mismatch.push(0);

        for (bytes, deferred_to_stream) in [
            (invalid_magic, false),
            (trailing, false),
            (set_with_value, false),
            // The validated ACT plan proves positional value count before exposing the
            // first indexed topology event; this must fail at admission.
            (map_count_mismatch, false),
        ] {
            let contiguous = OwnedCanonicalEpm1TopologyCursor::new(Bytes::from(bytes.clone()))
                .err()
                .expect("contiguous malformed snapshot");
            let (frozen, was_deferred) = frozen_error(&bytes);
            assert_eq!(was_deferred, deferred_to_stream);
            assert_eq!(frozen.to_string(), contiguous.to_string());
            let reported = frozen
                .reported_provenance()
                .expect("indexed EPM1 errors report physical provenance");
            let failure = frozen
                .failure_provenance()
                .expect("indexed EPM1 errors retain physical failure provenance");
            assert_eq!(reported.source, SOURCE);
            assert_eq!(failure.source, SOURCE);
            assert!((BASE..=BASE + bytes.len()).contains(&reported.offset));
            assert!((BASE..=BASE + bytes.len()).contains(&failure.offset));
        }
    }

    #[test]
    fn indexed_topology_cursor_is_a_single_pass_over_epm1_topology_and_values() {
        const VALUES: usize = 128;
        let mut map = PathMap::new();
        for index in 0..VALUES {
            let key = format!("one-pass/shared-prefix/{index:04}");
            map.insert(key.as_bytes(), int(index as i64));
        }
        map.create_path(b"one-pass/shared-prefix/value-free");

        let snapshot = encode(&EPathMapRepr::Map(map));
        let prefix = parse_canonical_snapshot_prefix(&snapshot).expect("canonical prefix");
        let value_table_start = prefix.value_table_start;
        let value_table_len = snapshot.len() - value_table_start;
        let frozen = frozen_snapshot(&snapshot, 97, 12_288);
        let mut cursor =
            IndexedCanonicalEpm1TopologyCursor::new(frozen).expect("indexed topology framing");

        let initial = cursor.stats();
        assert_eq!(initial.prefix_bytes_read, value_table_start);
        assert!(initial.prefix_bytes_traversed <= initial.prefix_bytes_read);
        assert_eq!(initial.value_table_bytes_read, 0);
        assert_eq!(initial.value_table_bytes_traversed, 0);
        assert_eq!(initial.topology_events, 0);
        assert_eq!(initial.value_records, 0);
        assert_eq!(initial.terminal_checks, 0);
        assert_eq!(cursor.length(), 0);

        let mut events = 0usize;
        let mut records = 0usize;
        while let Some(event) = cursor.next_event().expect("single-pass indexed event") {
            events += 1;
            if matches!(event, IndexedCanonicalEpm1TopologyEvent::Value {
                value_body: Some(_),
                ..
            }) {
                records += 1;
            }
            let progress = cursor.stats();
            assert_eq!(progress.topology_events, events);
            assert_eq!(progress.value_records, records);
            assert!(progress.value_table_bytes_read <= value_table_len);
            assert!(
                progress.value_table_bytes_traversed <= progress.value_table_bytes_read,
                "the indexed reader may fast-forward payload bodies but never revisits them"
            );
        }

        let complete = cursor.stats();
        assert_eq!(records, VALUES);
        assert_eq!(cursor.length(), VALUES);
        assert_eq!(complete.topology_events, events);
        assert_eq!(complete.value_records, VALUES);
        assert_eq!(complete.value_table_bytes_read, value_table_len);
        assert!(complete.value_table_bytes_traversed <= value_table_len);
        assert_eq!(
            complete.prefix_bytes_read + complete.value_table_bytes_read,
            snapshot.len()
        );
        assert!(
            complete.prefix_bytes_traversed + complete.value_table_bytes_traversed
                <= snapshot.len(),
            "sequential EPM1 readers perform at most one physical traversal per snapshot byte"
        );
        assert_eq!(complete.terminal_checks, 1);

        assert!(cursor
            .next_event()
            .expect("completed cursor stays done")
            .is_none());
        assert_eq!(cursor.stats(), complete);
    }

    #[test]
    fn indexed_topology_cursor_visits_a_shared_prefix_comb_in_linear_events() {
        const DEPTH: usize = 1_024;
        let mut set = PathMap::new();
        let mut spine = Vec::with_capacity(DEPTH + 1);
        for _ in 0..DEPTH {
            spine.push(0);
            let mut sibling = spine.clone();
            sibling.push(1);
            set.insert(&sibling, ());
        }
        spine.push(0);
        set.insert(&spine, ());

        let snapshot = encode(&EPathMapRepr::<Par>::Set(set));
        let snapshot = frozen_snapshot(&snapshot, 101, 16_384);
        let mut cursor = IndexedCanonicalEpm1TopologyCursor::new(snapshot).expect("comb topology");
        let mut values = 0usize;
        let mut value_nodes = 0usize;
        let mut edges = 0usize;
        let mut leaves = 0usize;
        while let Some(event) = cursor.next_event().expect("validated comb event") {
            match event {
                IndexedCanonicalEpm1TopologyEvent::Value { has_value, .. } => {
                    value_nodes += 1;
                    values += usize::from(has_value);
                }
                IndexedCanonicalEpm1TopologyEvent::Edge { .. } => edges += 1,
                IndexedCanonicalEpm1TopologyEvent::Leave { .. } => leaves += 1,
            }
        }

        let logical_nodes = 2 * DEPTH + 2;
        assert_eq!(value_nodes, logical_nodes);
        assert_eq!(edges, logical_nodes - 1);
        assert_eq!(leaves, logical_nodes);
        assert_eq!(values, DEPTH + 1);
        assert_eq!(cursor.length(), DEPTH + 1);
    }

    #[test]
    fn owned_topology_cursor_rejects_malformed_act_and_value_framing() {
        let aliased = arena(22, &[
            ACT_VALUE_FLAG,
            0,
            ACT_VALUE_FLAG | 1,
            1,
            2,
            b'a',
            2,
            6,
            b'a',
            b'b',
        ]);
        let aliased = map_snapshot_from_arena(&aliased, &[int(1), int(2)]);
        assert!(OwnedCanonicalEpm1TopologyCursor::new(Bytes::from(aliased)).is_err());

        let mut map = PathMap::new();
        map.insert(b"canonical/key", int(1));
        let mut trailer = encode(&EPathMapRepr::Map(map));
        let trailer_offset = {
            let prefix = parse_canonical_snapshot_prefix(&trailer).expect("canonical prefix");
            prefix.arena_start + prefix.arena.len() - 1
        };
        trailer[trailer_offset] = 1;
        assert!(OwnedCanonicalEpm1TopologyCursor::new(Bytes::from(trailer)).is_err());

        let mut trailing = encode(&EPathMapRepr::<Par>::Empty);
        trailing.push(0);
        assert!(OwnedCanonicalEpm1TopologyCursor::new(Bytes::from(trailing)).is_err());
    }

    #[test]
    fn indexed_topology_cursor_is_stack_safe_for_a_frozen_depth_4096_comb() {
        const DEPTH: usize = 4_096;
        const STACK: usize = 256 * 1024;

        let mut set = PathMap::new();
        let mut spine = Vec::with_capacity(DEPTH + 1);
        for _ in 0..DEPTH {
            spine.push(0);
            let mut sibling = spine.clone();
            sibling.push(1);
            set.insert(&sibling, ());
        }
        spine.push(0);
        set.insert(&spine, ());
        let snapshot = encode(&EPathMapRepr::<Par>::Set(set));
        let snapshot = frozen_snapshot(&snapshot, 103, 32_768);

        std::thread::Builder::new()
            .name("indexed-epm1-topology-depth".to_owned())
            .stack_size(STACK)
            .spawn(move || {
                let mut cursor =
                    IndexedCanonicalEpm1TopologyCursor::new(snapshot).expect("deep topology");
                let mut events = 0usize;
                while cursor.next_event().expect("validated deep event").is_some() {
                    events += 1;
                }
                assert_eq!(events, 6 * DEPTH + 5);
                assert_eq!(cursor.length(), DEPTH + 1);
            })
            .expect("spawn fixed small-stack topology probe")
            .join()
            .expect("topology cursor depth remains on the heap");
    }

    #[test]
    fn direct_cursor_rejects_aliased_nodes_and_forward_edges() {
        let aliased = arena(22, &[
            ACT_VALUE_FLAG,
            0,
            ACT_VALUE_FLAG | 1,
            1,
            2,
            b'a',
            2,
            6,
            b'a',
            b'b',
        ]);
        let aliased = map_snapshot_from_arena(&aliased, &[int(1), int(2)]);
        let error = CanonicalEpm1Cursor::new(&aliased)
            .err()
            .expect("aliased ACT nodes are rejected");
        assert_eq!(
            error.to_string(),
            "ACTree03 node is referenced by more than one edge"
        );

        let forward = arena(18, &[ACT_VALUE_FLAG, 0, 1, 2, b'a']);
        let forward = map_snapshot_from_arena(&forward, &[int(1)]);
        assert!(CanonicalEpm1Cursor::new(&forward).is_err());
    }

    #[test]
    fn decoder_rejects_noncanonical_and_malformed_snapshots() {
        let canonical = encode(&EPathMapRepr::Empty);
        let mut trailing = canonical.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());

        let mut typed_empty = canonical;
        typed_empty[5] = EPathMapMode::Set as u8;
        assert!(decode(&typed_empty).is_err());
    }

    #[test]
    fn direct_act_replay_rejects_unreachable_arena_bytes_and_nonzero_trailer() {
        let mut map = PathMap::new();
        map.insert(b"canonical/key", int(1));
        let canonical = encode(&EPathMapRepr::Map(map));

        let mut cursor = 6usize;
        let arena_len_at = cursor;
        let arena_len = read_len(&canonical, &mut cursor, "test arena length").unwrap();
        assert!(
            arena_len < 0x7f,
            "the mutation assumes a one-byte arena length"
        );
        let arena_end = cursor + arena_len;

        let mut gap = canonical.clone();
        gap.insert(arena_end - 8, 0x5a);
        gap[arena_len_at] += 1;
        assert!(
            decode(&gap).is_err(),
            "an unreachable arena byte was accepted"
        );

        let mut trailer = canonical;
        trailer[arena_end - 1] = 1;
        assert!(
            decode(&trailer).is_err(),
            "a nonzero ACT canonical-varint trailer was accepted"
        );
    }

    #[test]
    fn direct_act_replay_rejects_uncompressed_unary_nodes_and_nonzero_map_markers() {
        // Alternate encoding of path "a": a value-free unary branch over a
        // value leaf. PathMap's jumping writer emits one line node instead.
        let unary = arena(18, &[ACT_VALUE_FLAG, 0, 1, 2, b'a']);
        assert!(decode(&map_snapshot_from_arena(&unary, &[int(1)])).is_err());

        // Structurally valid two-leaf branch, but its first value marker is
        // nonzero. EPM1 association is positional, so all stored markers are
        // the zero presence sentinel.
        let nonzero = arena(20, &[
            ACT_VALUE_FLAG,
            1,
            ACT_VALUE_FLAG,
            0,
            2,
            4,
            b'a',
            b'b',
        ]);
        assert!(
            decode(&map_snapshot_from_arena(&nonzero, &[int(1), int(2)])).is_err(),
            "a nonzero positional ACT marker was accepted"
        );
    }

    #[test]
    fn direct_act_replay_requires_pathmap_line_reuse_exactly() {
        // Both keys end in "x". The canonical writer stores the line data once
        // at byte 16 and points both leaf nodes at it.
        let canonical = arena(24, &[
            1,
            b'x',
            ACT_LINE_FLAG | ACT_VALUE_FLAG,
            0,
            2,
            ACT_LINE_FLAG | ACT_VALUE_FLAG,
            0,
            5,
            2,
            6,
            b'a',
            b'b',
        ]);
        let snapshot = map_snapshot_from_arena(&canonical, &[int(1), int(2)]);
        assert!(
            decode(&snapshot).is_ok(),
            "the canonical reused line was rejected"
        );

        // The same semantic trie with a duplicate second line-data object.
        // Every offset remains well-formed, but PathMap's deterministic line
        // cache would have reused the first object, so this is not an image.
        let duplicate = arena(26, &[
            1,
            b'x',
            1,
            b'x',
            ACT_LINE_FLAG | ACT_VALUE_FLAG,
            0,
            4,
            ACT_LINE_FLAG | ACT_VALUE_FLAG,
            0,
            5,
            2,
            6,
            b'a',
            b'b',
        ]);
        assert!(
            decode(&map_snapshot_from_arena(&duplicate, &[int(1), int(2)])).is_err(),
            "duplicate line data bypassed the canonical PathMap reuse rule"
        );
    }

    #[test]
    fn direct_act_replay_matches_reconstruction_oracle_on_writer_images_and_mutations() {
        let mut line_set = PathMap::new();
        line_set.insert(b"shared/line/a", ());
        line_set.insert(b"shared/line/b", ());
        line_set.create_path(b"shared/dangling");

        // The endpoint diverges at the root from the value-bearing key. The
        // canonical writer represents its leaf as a value-free childless
        // branch rather than a compressed line node.
        let mut root_divergent_endpoint_set = PathMap::new();
        root_divergent_endpoint_set.insert(b"value/key", ());
        root_divergent_endpoint_set.create_path(b"dangling/endpoint");

        let mut line_map = PathMap::new();
        line_map.insert(b"same/suffix/x", int(1));
        line_map.insert(b"other/suffix/x", int(2));
        line_map.insert(b"same/suffix/y", int(3));

        let mut dense_set = PathMap::new();
        for byte in 0u8..=63 {
            dense_set.insert([byte, b'k'], ());
        }

        let fixtures = [
            encode(&EPathMapRepr::Empty),
            encode(&EPathMapRepr::Set(line_set)),
            encode(&EPathMapRepr::Set(root_divergent_endpoint_set)),
            encode(&EPathMapRepr::Map(line_map)),
            encode(&EPathMapRepr::Set(dense_set)),
        ];

        for canonical in fixtures {
            assert!(reconstruction_oracle(&canonical));
            assert!(decode(&canonical).is_ok());
            assert!(CanonicalEpm1Cursor::new(&canonical).is_ok());
            for index in 0..canonical.len() {
                for bit in 0..8 {
                    let mut mutation = canonical.clone();
                    mutation[index] ^= 1 << bit;
                    assert_eq!(
                        OwnedCanonicalEpm1Cursor::new(Bytes::from(mutation.clone())).is_ok(),
                        CanonicalEpm1Cursor::new(&mutation).is_ok(),
                        "owned and borrowed EPM1 cursors diverged at byte {index}, bit {bit}"
                    );
                    assert_eq!(
                        CanonicalEpm1Cursor::new(&mutation).is_ok(),
                        inspect_canonical_snapshot(&mutation).is_ok(),
                        "streaming ACT cursor diverged from the collected inspector at byte {index}, bit {bit}"
                    );
                    assert_eq!(
                        decode(&mutation).is_ok(),
                        reconstruction_oracle(&mutation),
                        "direct ACT replay diverged from reconstruction at byte {index}, bit {bit}"
                    );
                }
            }
        }
    }

    #[test]
    fn streaming_cursor_is_stack_safe_for_a_depth_4096_branch_comb() {
        const DEPTH: usize = 4_096;
        const STACK: usize = 256 * 1024;

        std::thread::Builder::new()
            .name("canonical-epm1-cursor-depth".to_owned())
            .stack_size(STACK)
            .spawn(|| {
                let mut map = PathMap::new();
                let mut spine = Vec::with_capacity(DEPTH + 1);
                for _ in 0..DEPTH {
                    spine.push(0);
                    let mut sibling = spine.clone();
                    sibling.push(1);
                    map.insert(&sibling, ());
                }
                spine.push(0);
                map.insert(&spine, ());
                let bytes = encode(&EPathMapRepr::Set(map));
                let mut cursor = CanonicalEpm1Cursor::new(&bytes).expect("deep canonical ACT");
                let mut entries = 0usize;
                while cursor.next_entry().is_some() {
                    entries += 1;
                }
                assert_eq!(entries, DEPTH + 1);

                let mut cursor =
                    OwnedCanonicalEpm1Cursor::new(Bytes::from(bytes)).expect("owned deep ACT");
                let mut entries = 0usize;
                while cursor.next_entry().is_some() {
                    entries += 1;
                }
                assert_eq!(entries, DEPTH + 1);
            })
            .expect("spawn fixed small-stack ACT cursor probe")
            .join()
            .expect("ACT cursor depth remains on the heap");
    }
}
