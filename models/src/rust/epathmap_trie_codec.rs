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
use rspace_plus_plus::rspace::pathmap_codec::{CompactPath, CompactTrie, CompactTrieError};

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
    if trie.is_empty() {
        return Err(TrieCodecError::new(
            "typed EPathMap mode has empty ACTree topology; use neutral empty mode",
        ));
    }

    let mut keys = Vec::with_capacity(value_count);
    trie.visit_compact(|path, ordinal| {
        if let Some(ordinal) = ordinal {
            match mode {
                EPathMapMode::Set => {
                    if ordinal != 0 {
                        return Err(TrieCodecError::new("set ACTree value ordinal is not zero"));
                    }
                }
                EPathMapMode::Map => {
                    if ordinal != keys.len() as u64 {
                        return Err(TrieCodecError::new(format!(
                            "map ACTree value ordinal {ordinal} is not canonical ordinal {}",
                            keys.len()
                        )));
                    }
                    keys.push(match path {
                        CompactPath::Relative(range) => CanonicalSnapshotKey::Relative(
                            arena_start + range.start..arena_start + range.end,
                        ),
                        CompactPath::Owned(path) => CanonicalSnapshotKey::Owned(path),
                    });
                }
                EPathMapMode::Empty => unreachable!("empty mode has no ACT arena"),
            }
        }
        Ok(())
    })?;

    if mode == EPathMapMode::Map && keys.len() != value_count {
        return Err(TrieCodecError::new(format!(
            "map ACTree references {} values but EPM1 declares {value_count}",
            keys.len()
        )));
    }
    if mode == EPathMapMode::Set && value_count != 0 {
        return Err(TrieCodecError::new("set EPathMap carries map values"));
    }
    Ok(keys)
}

/// Inspect canonical EPM1 framing and ACT topology without constructing a
/// temporary PathMap or decoding its canonical byte keys.
pub(crate) fn inspect_canonical_snapshot(
    bytes: &[u8],
) -> Result<CanonicalSnapshotInspection, TrieCodecError> {
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
    let remaining = bytes.len() - cursor;
    if value_count > remaining {
        return Err(TrieCodecError::new(format!(
            "map value count {value_count} exceeds the {remaining}-byte payload lower bound"
        )));
    }
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

    let keys = match mode {
        EPathMapMode::Empty => {
            if !arena.is_empty() || value_count != 0 {
                return Err(TrieCodecError::new(
                    "neutral empty EPathMap carries trie or values",
                ));
            }
            Vec::new()
        }
        EPathMapMode::Set | EPathMapMode::Map => {
            let trie = CompactTrie::parse_canonical(arena)?;
            collect_canonical_keys(arena_start, &trie, mode, value_count)?
        }
    };

    Ok(CanonicalSnapshotInspection {
        mode,
        keys,
        value_bodies,
    })
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
            encode(&EPathMapRepr::Map(line_map)),
            encode(&EPathMapRepr::Set(dense_set)),
        ];

        for canonical in fixtures {
            assert!(reconstruction_oracle(&canonical));
            assert!(decode(&canonical).is_ok());
            for index in 0..canonical.len() {
                for bit in 0..8 {
                    let mut mutation = canonical.clone();
                    mutation[index] ^= 1 << bit;
                    assert_eq!(
                        decode(&mutation).is_ok(),
                        reconstruction_oracle(&mutation),
                        "direct ACT replay diverged from reconstruction at byte {index}, bit {bit}"
                    );
                }
            }
        }
    }
}
