//! Canonical, prefix-compressed byte snapshots for `EPathMap`.
//!
//! `EPM1` keeps the PathMap trie intact across every serialization boundary:
//! the key topology is PathMap's compact `ACTree03` arena and map values are a
//! separate ordinal table encoded by the generated stack-safe protobuf PDA.
//! No `Vec<Par>` entry projection participates in either direction.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use pathmap::arena_compact::ArenaCompactTree;
use pathmap::zipper::ZipperReadOnlyIteration;
#[cfg(test)]
use pathmap::zipper::{ZipperIteration, ZipperMoving};
use pathmap::PathMap;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieCursor, CanonicalCompactTrieTopologyCursor,
    CanonicalCompactTrieTopologyEvent, CompactPath, CompactTrie, CompactTrieError,
    CompactTrieTopologyEvent,
};

use crate::rhoapi::Par;
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
}

impl TrieCodecError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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

#[allow(dead_code)]
pub(crate) struct CanonicalEpm1Entry<'a> {
    key: &'a [u8],
    value_body: Option<Range<usize>>,
}

impl CanonicalEpm1Entry<'_> {
    pub(crate) fn key(&self) -> &[u8] { self.key }

    pub(crate) fn value_body(&self) -> Option<Range<usize>> { self.value_body.clone() }
}

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

fn validate_typed_epm1_trie<B>(
    mode: EPathMapMode,
    value_count: usize,
    trie: &mut CanonicalCompactTrieCursor<B>,
) -> Result<(), TrieCodecError>
where
    B: AsRef<[u8]>,
{
    if trie.is_empty() {
        return Err(TrieCodecError::new(
            "typed EPathMap mode has empty ACTree topology; use neutral empty mode",
        ));
    }
    let mut referenced_values = 0usize;
    while let Some(endpoint) = trie.next_value_endpoint() {
        let ordinal = endpoint
            .value
            .expect("a value-endpoint cursor yields an ordinal");
        match mode {
            EPathMapMode::Set => {
                if ordinal != 0 {
                    return Err(TrieCodecError::new("set ACTree value ordinal is not zero"));
                }
            }
            EPathMapMode::Map => {
                if ordinal != referenced_values as u64 {
                    return Err(TrieCodecError::new(format!(
                        "map ACTree value ordinal {ordinal} is not canonical ordinal {referenced_values}"
                    )));
                }
                referenced_values = referenced_values
                    .checked_add(1)
                    .ok_or_else(|| TrieCodecError::new("EPathMap value count overflows usize"))?;
            }
            EPathMapMode::Empty => unreachable!("empty mode has no ACT arena"),
        }
    }
    if mode == EPathMapMode::Set && value_count != 0 {
        return Err(TrieCodecError::new("set EPathMap carries map values"));
    }
    if mode == EPathMapMode::Map && referenced_values != value_count {
        return Err(TrieCodecError::new(format!(
            "map ACTree references {referenced_values} values but EPM1 declares {value_count}"
        )));
    }
    trie.rewind();
    Ok(())
}

fn validate_typed_epm1_topology<B>(
    mode: EPathMapMode,
    value_count: usize,
    trie: &mut CanonicalCompactTrieTopologyCursor<B>,
) -> Result<usize, TrieCodecError>
where
    B: AsRef<[u8]>,
{
    if trie.is_empty() {
        return Err(TrieCodecError::new(
            "typed EPathMap mode has empty ACTree topology; use neutral empty mode",
        ));
    }
    let mut referenced_values = 0usize;
    while let Some(event) = trie.next_event() {
        let CanonicalCompactTrieTopologyEvent::Value {
            value: Some(ordinal),
            ..
        } = event
        else {
            continue;
        };
        match mode {
            EPathMapMode::Set => {
                if ordinal != 0 {
                    return Err(TrieCodecError::new("set ACTree value ordinal is not zero"));
                }
            }
            EPathMapMode::Map => {
                if ordinal != referenced_values as u64 {
                    return Err(TrieCodecError::new(format!(
                        "map ACTree value ordinal {ordinal} is not canonical ordinal {referenced_values}"
                    )));
                }
            }
            EPathMapMode::Empty => unreachable!("empty mode has no ACT arena"),
        }
        referenced_values = referenced_values
            .checked_add(1)
            .ok_or_else(|| TrieCodecError::new("EPathMap length overflows usize"))?;
    }
    if mode == EPathMapMode::Set && value_count != 0 {
        return Err(TrieCodecError::new("set EPathMap carries map values"));
    }
    if mode == EPathMapMode::Map && referenced_values != value_count {
        return Err(TrieCodecError::new(format!(
            "map ACTree references {referenced_values} values but EPM1 declares {value_count}"
        )));
    }
    trie.rewind();
    Ok(referenced_values)
}

#[allow(dead_code)]
pub(crate) struct CanonicalEpm1Cursor<'a> {
    bytes: &'a [u8],
    mode: EPathMapMode,
    value_count: usize,
    value_cursor: usize,
    remaining_values: usize,
    trie: Option<CanonicalCompactTrieCursor<&'a [u8]>>,
}

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

#[allow(dead_code)]
pub(crate) struct OwnedCanonicalEpm1Entry<'a> {
    key: &'a [u8],
    value_body_range: Option<Range<usize>>,
    value_body: Option<Bytes>,
}

impl OwnedCanonicalEpm1Entry<'_> {
    pub(crate) fn key(&self) -> &[u8] { self.key }

    pub(crate) fn value_body_range(&self) -> Option<Range<usize>> { self.value_body_range.clone() }

    pub(crate) fn value_body(&self) -> Option<Bytes> { self.value_body.clone() }
}

#[allow(dead_code)]
pub(crate) struct OwnedCanonicalEpm1Cursor {
    bytes: Bytes,
    mode: EPathMapMode,
    value_count: usize,
    value_cursor: usize,
    remaining_values: usize,
    trie: Option<CanonicalCompactTrieCursor<Bytes>>,
}

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

#[allow(dead_code)]
pub(crate) struct OwnedCanonicalEpm1TopologyCursor {
    bytes: Bytes,
    mode: EPathMapMode,
    value_count: usize,
    length: usize,
    arena_start: usize,
    value_cursor: usize,
    remaining_values: usize,
    empty_state: u8,
    trie: Option<CanonicalCompactTrieTopologyCursor<Bytes>>,
}

impl OwnedCanonicalEpm1TopologyCursor {
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

        let (trie, length) = match mode {
            EPathMapMode::Empty => {
                if !arena.is_empty() || value_count != 0 {
                    return Err(TrieCodecError::new(
                        "neutral empty EPathMap carries trie or values",
                    ));
                }
                (None, 0)
            }
            EPathMapMode::Set | EPathMapMode::Map => {
                let mut trie =
                    CanonicalCompactTrieTopologyCursor::from_backing(bytes.slice(arena.clone()))?;
                let length = validate_typed_epm1_topology(mode, value_count, &mut trie)?;
                (Some(trie), length)
            }
        };

        Ok(Self {
            bytes,
            mode,
            value_count,
            length,
            arena_start: arena.start,
            value_cursor: value_table_start,
            remaining_values: value_count,
            empty_state: 0,
            trie,
        })
    }

    pub(crate) const fn mode(&self) -> EPathMapMode { self.mode }

    pub(crate) const fn value_count(&self) -> usize { self.value_count }

    pub(crate) const fn length(&self) -> usize { self.length }

    pub(crate) fn next_event(&mut self) -> Option<OwnedCanonicalEpm1TopologyEvent> {
        if self.mode == EPathMapMode::Empty {
            let event = match self.empty_state {
                0 => OwnedCanonicalEpm1TopologyEvent::Value {
                    has_value: false,
                    value_body_range: None,
                    value_body: None,
                    source_offset: EPM_MAGIC.len() + 1,
                },
                1 => OwnedCanonicalEpm1TopologyEvent::Leave {
                    source_offset: EPM_MAGIC.len() + 1,
                },
                _ => return None,
            };
            self.empty_state += 1;
            return Some(event);
        }

        let event = self.trie.as_mut()?.next_event()?;
        Some(match event {
            CanonicalCompactTrieTopologyEvent::Value {
                value,
                source_offset,
            } => {
                let value_body_range = if self.mode == EPathMapMode::Map && value.is_some() {
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
                OwnedCanonicalEpm1TopologyEvent::Value {
                    has_value: value.is_some(),
                    value_body_range,
                    value_body,
                    source_offset: self
                        .arena_start
                        .checked_add(source_offset)
                        .expect("validated EPM1 arena offset remains in the snapshot"),
                }
            }
            CanonicalCompactTrieTopologyEvent::Edge {
                byte,
                source_offset,
            } => OwnedCanonicalEpm1TopologyEvent::Edge {
                byte,
                source_offset: self
                    .arena_start
                    .checked_add(source_offset)
                    .expect("validated EPM1 arena offset remains in the snapshot"),
            },
            CanonicalCompactTrieTopologyEvent::Leave { source_offset } => {
                OwnedCanonicalEpm1TopologyEvent::Leave {
                    source_offset: self
                        .arena_start
                        .checked_add(source_offset)
                        .expect("validated EPM1 arena offset remains in the snapshot"),
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

/// Build the final-byte topology directly from PathMap. No `Par` projection or
/// encoded-value table is retained: the only temporary index maps the stable
/// addresses supplied by PathMap to ACTree03 value ordinals.
pub(crate) fn layout<T>(repr: &EPathMapRepr<T>) -> EpmLayout
where T: Clone + Send + Sync + Unpin + 'static {
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
            let mut ordinal_by_address = HashMap::new();
            let mut zipper = map.read_zipper();
            let mut value_count = 0usize;
            while let Some(value) = zipper.to_next_get_val() {
                ordinal_by_address.insert(value as *const T as usize, value_count as u64);
                value_count += 1;
            }
            let arena = ArenaCompactTree::from_zipper(map.read_zipper(), |value| {
                ordinal_by_address[&(value as *const T as usize)]
            });
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
        while let Some(value) = zipper.to_next_get_val() {
            protobuf_encoder::encode_length_delimited_body_into(value, &mut out);
            emitted += 1;
        }
        debug_assert_eq!(emitted, layout.value_count());
    }
    out
}

/// Pausable EPM1 reader used by the generated protobuf decoder. The snapshot
/// bytes remain shared, and every map value is handed to the same explicit PDA
/// as an independent protobuf body.
pub(crate) struct PendingEpmDecode<S: AsRef<[u8]>> {
    snapshot: S,
    mode: EPathMapMode,
    /// ACTree03's byte range inside `snapshot`. Keeping an index rather than a
    /// copied `Vec<u8>` makes the pausable protobuf decoder zero-copy for the
    /// compact trie arena while remaining valid for every owned snapshot type.
    arena: Range<usize>,
    next_value_at: usize,
    remaining_values: usize,
    expected_values: usize,
    values: Vec<Option<Par>>,
}

pub(crate) type OwnedPendingEpmDecode = PendingEpmDecode<Bytes>;

impl<S: AsRef<[u8]>> PendingEpmDecode<S> {
    pub(crate) fn new(snapshot: S) -> Result<Self, TrieCodecError> {
        let bytes = snapshot.as_ref();
        let mut cursor = 0usize;
        let magic = take(bytes, &mut cursor, EPM_MAGIC.len(), "EPM1 magic")?;
        if magic != EPM_MAGIC {
            return Err(TrieCodecError::new("invalid EPM1 magic"));
        }
        let version = *take(bytes, &mut cursor, 1, "EPM1 version")?
            .first()
            .expect("a one-byte slice has a first byte");
        if version != EPM_VERSION {
            return Err(TrieCodecError::new(format!(
                "unsupported EPM1 version {version}"
            )));
        }
        let mode = EPathMapMode::try_from(
            *take(bytes, &mut cursor, 1, "EPM1 mode")?
                .first()
                .expect("a one-byte slice has a first byte"),
        )?;
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

        let next_value_at = cursor;
        for _ in 0..value_count {
            let len = read_len(bytes, &mut cursor, "map value length")?;
            take(bytes, &mut cursor, len, "map value payload")?;
        }
        if cursor != bytes.len() {
            return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
        }

        Ok(Self {
            snapshot,
            mode,
            arena,
            next_value_at,
            remaining_values: value_count,
            expected_values: value_count,
            values: Vec::with_capacity(value_count),
        })
    }

    fn next_value_range(&mut self) -> Result<Option<Range<usize>>, TrieCodecError> {
        if self.remaining_values == 0 {
            return Ok(None);
        }
        let bytes = self.snapshot.as_ref();
        let len = read_len(bytes, &mut self.next_value_at, "map value length")?;
        let start = self.next_value_at;
        take(bytes, &mut self.next_value_at, len, "map value payload")?;
        self.remaining_values -= 1;
        Ok(Some(start..start + len))
    }

    pub(crate) fn push_value(&mut self, value: Par) -> Result<(), TrieCodecError> {
        if self.values.len() == self.expected_values {
            return Err(TrieCodecError::new(
                "more map values were decoded than the EPM1 value count",
            ));
        }
        self.values.push(Some(value));
        Ok(())
    }

    pub(crate) fn finish(
        self,
        validate_canonical: bool,
    ) -> Result<EPathMapRepr<Par>, TrieCodecError> {
        let Self {
            snapshot,
            mode,
            arena,
            next_value_at: _,
            remaining_values,
            expected_values,
            values,
        } = self;
        if remaining_values != 0 || values.len() != expected_values {
            return Err(TrieCodecError::new(
                "fewer map values were decoded than the EPM1 value count",
            ));
        }
        if validate_canonical {
            let inspection = inspect_canonical_snapshot(snapshot.as_ref())?;
            if inspection.mode != mode || inspection.value_bodies.len() != values.len() {
                return Err(TrieCodecError::new(
                    "EPM1 structural inspection disagrees with its decoded framing",
                ));
            }
            for (ordinal, (range, value)) in inspection.value_bodies.iter().zip(&values).enumerate()
            {
                let value = value
                    .as_ref()
                    .expect("a completed pending EPM decode retains every value");
                if protobuf_encoder::encode_to_vec(value) != snapshot.as_ref()[range.clone()] {
                    return Err(TrieCodecError::new(format!(
                        "map value {ordinal} protobuf body is not canonical"
                    )));
                }
            }
        }
        let repr = build_repr(mode, &snapshot.as_ref()[arena], values)?;
        Ok(repr)
    }
}

impl PendingEpmDecode<Bytes> {
    pub(crate) fn next_value_bytes(&mut self) -> Result<Option<Bytes>, TrieCodecError> {
        let Some(range) = self.next_value_range()? else {
            return Ok(None);
        };
        Ok(Some(self.snapshot.slice(range)))
    }
}

fn build_repr(
    mode: EPathMapMode,
    arena: &[u8],
    mut values: Vec<Option<Par>>,
) -> Result<EPathMapRepr<Par>, TrieCodecError> {
    match mode {
        EPathMapMode::Empty => {
            if !arena.is_empty() || !values.is_empty() {
                return Err(TrieCodecError::new(
                    "neutral empty EPathMap carries trie or values",
                ));
            }
            Ok(EPathMapRepr::Empty)
        }
        EPathMapMode::Set => {
            if !values.is_empty() {
                return Err(TrieCodecError::new("set EPathMap carries map values"));
            }
            let mut map = PathMap::<()>::new();
            visit_act(arena, |path, ordinal| {
                if !path.is_empty() {
                    map.create_path(path);
                }
                if let Some(ordinal) = ordinal {
                    if ordinal != 0 {
                        return Err(TrieCodecError::new("set ACTree value ordinal is not zero"));
                    }
                    map.insert(path, ());
                }
                Ok(())
            })?;
            if map.is_empty() {
                return Err(TrieCodecError::new(
                    "typed set mode is empty; use neutral empty mode",
                ));
            }
            Ok(EPathMapRepr::Set(map))
        }
        EPathMapMode::Map => {
            let mut map = PathMap::<Par>::new();
            visit_act(arena, |path, ordinal| {
                if !path.is_empty() {
                    map.create_path(path);
                }
                let Some(ordinal) = ordinal else {
                    return Ok(());
                };
                let index = usize::try_from(ordinal)
                    .map_err(|_| TrieCodecError::new("map value ordinal overflows usize"))?;
                let slot = values.get_mut(index).ok_or_else(|| {
                    TrieCodecError::new(format!("map value ordinal {index} is out of range"))
                })?;
                let value = slot.take().ok_or_else(|| {
                    TrieCodecError::new(format!("map value ordinal {index} is reused"))
                })?;
                map.insert(path, value);
                Ok(())
            })?;
            if map.is_empty() {
                return Err(TrieCodecError::new(
                    "typed map mode is empty; use neutral empty mode",
                ));
            }
            if let Some(missing) = values.iter().position(Option::is_some) {
                return Err(TrieCodecError::new(format!(
                    "map value ordinal {missing} is unreferenced"
                )));
            }
            Ok(EPathMapRepr::Map(map))
        }
    }
}

/// Decode and validate `EPM1`, then reconstruct PathMap directly through its
/// insertion API. A canonicality fence rejects alternate encodings of the same
/// trie, malformed ACT offsets, duplicate/missing value ordinals, and trailing
/// bytes.
fn decode_with(
    bytes: &[u8],
    decode_value: fn(&[u8]) -> Result<Par, prost::DecodeError>,
) -> Result<EPathMapRepr<Par>, TrieCodecError> {
    let mut cursor = 0usize;
    let magic = take(bytes, &mut cursor, EPM_MAGIC.len(), "EPM1 magic")?;
    if magic != EPM_MAGIC {
        return Err(TrieCodecError::new("invalid EPM1 magic"));
    }
    let version = *take(bytes, &mut cursor, 1, "EPM1 version")?
        .first()
        .expect("a one-byte slice has a first byte");
    if version != EPM_VERSION {
        return Err(TrieCodecError::new(format!(
            "unsupported EPM1 version {version}"
        )));
    }
    let mode = EPathMapMode::try_from(
        *take(bytes, &mut cursor, 1, "EPM1 mode")?
            .first()
            .expect("a one-byte slice has a first byte"),
    )?;
    let arena_len = read_len(bytes, &mut cursor, "ACTree03 length")?;
    let arena = take(bytes, &mut cursor, arena_len, "ACTree03 payload")?;
    let value_count = read_len(bytes, &mut cursor, "map value count")?;

    // Every value, including an empty protobuf `Par`, needs at least one byte
    // for its length varint. Prove that lower bound against the actual input
    // before reserving `Option<Par>` slots. This is not an artificial traversal
    // ceiling: it is the information-theoretic bound `value_count <= remaining
    // bytes`, and therefore accepts every physically representable EPM1 value
    // table. Without it, one mutated count varint can ask the allocator for
    // exabytes before the first missing length prefix is observed.
    let remaining = bytes.len() - cursor;
    if value_count > remaining {
        return Err(TrieCodecError::new(format!(
            "map value count {value_count} exceeds the {remaining}-byte payload lower bound"
        )));
    }

    let mut values = Vec::with_capacity(value_count);
    for ordinal in 0..value_count {
        let len = read_len(bytes, &mut cursor, "map value length")?;
        let payload = take(bytes, &mut cursor, len, "map value payload")?;
        let value = decode_value(payload).map_err(|error| {
            TrieCodecError::new(format!("map value {ordinal} is not a Par: {error}"))
        })?;
        values.push(Some(value));
    }
    if cursor != bytes.len() {
        return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
    }

    let inspection = inspect_canonical_snapshot(bytes)?;
    if inspection.mode != mode || inspection.value_bodies.len() != values.len() {
        return Err(TrieCodecError::new(
            "EPM1 structural inspection disagrees with its decoded framing",
        ));
    }
    for (ordinal, (range, value)) in inspection.value_bodies.iter().zip(&values).enumerate() {
        let value = value
            .as_ref()
            .expect("the direct EPM decoder retains every value before construction");
        if protobuf_encoder::encode_to_vec(value) != bytes[range.clone()] {
            return Err(TrieCodecError::new(format!(
                "map value {ordinal} protobuf body is not canonical"
            )));
        }
    }
    build_repr(mode, arena, values)
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

fn visit_act(
    bytes: &[u8],
    visit: impl FnMut(&[u8], Option<u64>) -> Result<(), TrieCodecError>,
) -> Result<(), TrieCodecError> {
    CompactTrie::parse(bytes)?.visit(visit)
}

fn collect_canonical_keys(
    arena_start: usize,
    trie: &CompactTrie<'_>,
    mode: EPathMapMode,
    value_count: usize,
) -> Result<Vec<CanonicalSnapshotKey>, TrieCodecError> {
    let mut keys = Vec::with_capacity(value_count);
    visit_canonical_value_paths(trie, mode, value_count, |path| {
        keys.push(match path {
            CompactPath::Relative(range) => {
                CanonicalSnapshotKey::Relative(arena_start + range.start..arena_start + range.end)
            }
            CompactPath::Owned(path) => CanonicalSnapshotKey::Owned(path),
        });
        Ok::<(), TrieCodecError>(())
    })?;
    Ok(keys)
}

fn visit_canonical_value_paths(
    trie: &CompactTrie<'_>,
    mode: EPathMapMode,
    value_count: usize,
    mut visit_map_path: impl FnMut(CompactPath) -> Result<(), TrieCodecError>,
) -> Result<(), TrieCodecError> {
    if trie.is_empty() {
        return Err(TrieCodecError::new(
            "typed EPathMap mode has empty ACTree topology; use neutral empty mode",
        ));
    }
    let mut referenced_values = 0usize;
    trie.visit_compact(|path, ordinal| {
        let Some(ordinal) = ordinal else {
            return Ok(());
        };
        match mode {
            EPathMapMode::Set => {
                if ordinal != 0 {
                    return Err(TrieCodecError::new("set ACTree value ordinal is not zero"));
                }
            }
            EPathMapMode::Map => {
                if ordinal != referenced_values as u64 {
                    return Err(TrieCodecError::new(format!(
                        "map ACTree value ordinal {ordinal} is not canonical ordinal {referenced_values}"
                    )));
                }
                referenced_values += 1;
                visit_map_path(path)?;
            }
            EPathMapMode::Empty => unreachable!("empty mode has no ACT arena"),
        }
        Ok(())
    })?;
    if mode == EPathMapMode::Map && referenced_values != value_count {
        return Err(TrieCodecError::new(format!(
            "map ACTree references {referenced_values} values but EPM1 declares {value_count}"
        )));
    }
    if mode == EPathMapMode::Set && value_count != 0 {
        return Err(TrieCodecError::new("set EPathMap carries map values"));
    }
    Ok(())
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
            let trie = CompactTrie::parse_canonical(arena)?;
            visit_canonical_value_paths(&trie, mode, value_count, |_path| Ok(()))?;
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
        EPathMapMode::Set | EPathMapMode::Map => {
            let trie = CompactTrie::parse_canonical(framing.arena)?;
            collect_canonical_keys(
                framing.arena_start,
                &trie,
                framing.mode,
                framing.value_count,
            )?
        }
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
/// value bodies supplied by its canonical ordinals.
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
/// Canonical value-ordinal validation runs to completion before the first
/// event becomes observable. `CompactTrie::visit_topology` then expands ACT
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

    let trie = CompactTrie::parse_canonical(prefix.arena)?;
    visit_canonical_value_paths(&trie, prefix.mode, prefix.value_count, |_path| Ok(()))?;
    let mut length = 0usize;
    trie.visit_topology(|event| {
        if event == CompactTrieTopologyEvent::Value(true) {
            length = length
                .checked_add(1)
                .ok_or_else(|| TrieCodecError::new("EPathMap length overflows usize"))?;
        }
        visitor(event);
        Ok::<(), TrieCodecError>(())
    })?;
    Ok((prefix.mode, length))
}

pub(crate) fn inspect_canonical_map_snapshot(
    bytes: &[u8],
) -> Result<CanonicalSnapshotInspection, TrieCodecError> {
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
mod tests {
    use pathmap::arena_compact::COMPACT_TREE_MAGIC;

    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{Expr, Par};

    const ACT_VALUE_FLAG: u8 = 0x40;
    const ACT_LINE_FLAG: u8 = 0x80;

    fn int(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
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

    fn arena(root: usize, body: &[u8]) -> Vec<u8> {
        let mut arena = COMPACT_TREE_MAGIC.to_vec();
        arena.extend_from_slice(&(root as u64).to_le_bytes());
        arena.extend_from_slice(body);
        arena.extend_from_slice(&[0; 8]);
        arena
    }

    fn reconstruction_oracle(bytes: &[u8]) -> bool {
        let Ok(mut pending) = PendingEpmDecode::new(Bytes::copy_from_slice(bytes)) else {
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
        let Ok(repr) = pending.finish(false) else {
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
    fn pending_decoder_borrows_act_region_from_owned_snapshot() {
        let mut map = PathMap::new();
        map.insert(b"shared/prefix/a", int(1));
        map.insert(b"shared/prefix/b", int(2));
        let snapshot = Bytes::from(encode(&EPathMapRepr::Map(map)));
        let storage_start = snapshot.as_ptr();

        let pending = PendingEpmDecode::new(snapshot).expect("EPM1 header is valid");
        let arena = &pending.snapshot.as_ref()[pending.arena.clone()];

        assert_eq!(
            arena.as_ptr(),
            storage_start.wrapping_add(pending.arena.start),
            "the pausable decoder must index the owned snapshot, not copy the ACT arena"
        );
        assert!(arena.starts_with(&COMPACT_TREE_MAGIC));
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

    #[test]
    fn canonical_epm1_cursor_lends_keys_and_value_regions_in_ordinal_order() {
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
    fn owned_topology_cursor_visits_a_shared_prefix_comb_in_linear_events() {
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

        let snapshot = Bytes::from(encode(&EPathMapRepr::<Par>::Set(set)));
        let mut cursor = OwnedCanonicalEpm1TopologyCursor::new(snapshot).expect("comb topology");
        let mut values = 0usize;
        let mut value_nodes = 0usize;
        let mut edges = 0usize;
        let mut leaves = 0usize;
        while let Some(event) = cursor.next_event() {
            match event {
                OwnedCanonicalEpm1TopologyEvent::Value { has_value, .. } => {
                    value_nodes += 1;
                    values += usize::from(has_value);
                }
                OwnedCanonicalEpm1TopologyEvent::Edge { .. } => edges += 1,
                OwnedCanonicalEpm1TopologyEvent::Leave { .. } => leaves += 1,
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
    fn owned_topology_cursor_is_stack_safe_for_a_depth_4096_comb() {
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
        let snapshot = Bytes::from(encode(&EPathMapRepr::<Par>::Set(set)));

        std::thread::Builder::new()
            .name("owned-epm1-topology-depth".to_owned())
            .stack_size(STACK)
            .spawn(move || {
                let mut cursor =
                    OwnedCanonicalEpm1TopologyCursor::new(snapshot).expect("deep topology");
                let mut events = 0usize;
                while cursor.next_event().is_some() {
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
    fn direct_act_replay_rejects_uncompressed_unary_nodes_and_ordinal_permutations() {
        // Alternate encoding of path "a": a value-free unary branch over a
        // value leaf. PathMap's jumping writer emits one line node instead.
        let unary = arena(18, &[ACT_VALUE_FLAG, 0, 1, 2, b'a']);
        assert!(decode(&map_snapshot_from_arena(&unary, &[int(1)])).is_err());

        // Structurally valid two-leaf branch, but its value ordinals run 1,0
        // instead of PathMap zipper order 0,1.
        let permuted = arena(20, &[
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
            decode(&map_snapshot_from_arena(&permuted, &[int(1), int(2)])).is_err(),
            "a noncanonical ACT ordinal permutation was accepted"
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
            1,
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
            1,
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
