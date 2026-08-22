//! Iterative shallow views over canonical Rholang protobuf regions.
//!
//! A resident [`crate::rhoapi::Par`] is an authoritative PathMap-backed bag.
//! Its component keys already contain canonical protobuf bodies. Recursive
//! consumers must not decode every successively shorter body and rebuild the
//! same PathMap suffixes at every level. This module instead cuts a canonical
//! message at each nested `Par` edge:
//!
//! - unchanged scalar and unknown fields remain shared byte regions;
//! - generated messages are traversed by an explicit heap worklist;
//! - every nested `Par` becomes one constant-size [`ProtobufRegion`] handle;
//! - EPathMap keeps its ACTree03 topology and exposes only its ordered map-value
//!   bodies as `Par` cuts;
//! - a bounded shallow shell is emitted exactly once and may be decoded by the
//!   existing generated decoder when a typed owner value is required.
//!
//! The generated match and protobuf programs are the only schema authority.
//! No field tag, oneof encoding, child type, or traversal edge is restated here.

use std::fmt;
use std::ops::Range;

use prost::bytes::Bytes;
use prost::encoding::{encode_key, encode_varint, encoded_len_varint, key_len, WireType};

use super::match_schema::{
    RhoMatchField, RhoMatchFieldKind, RhoMatchOneofVariant, RhoMatchPayloadKind, RhoMatchProgram,
};
use super::match_schema_tables::{PAR_PROTOBUF_REGION_PROGRAM, RHO_MATCH_REGISTRY};
use super::protobuf_cursor::{
    CanonicalByteRange, CanonicalProtobufError, CanonicalRegionField, CanonicalRegionFieldCursor,
    CanonicalWireType, ProtobufRegion,
};
use super::protobuf_schema::{ProtobufKind, ProtobufWireType};
use crate::rust::epathmap_trie_codec::{
    inspect_canonical_snapshot_value_regions, EPathMapMode, TrieCodecError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtobufRegionStepKind {
    Message,
    MapValue,
    EPathMapSnapshot,
    EPathMapValue,
}

/// One schema edge in the compact parent-linked path of a retained `Par`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtobufRegionStep {
    pub owner_type_id: u16,
    pub field_tag: u32,
    pub occurrence: u32,
    pub kind: ProtobufRegionStepKind,
}

#[derive(Clone, Debug)]
struct PathNode {
    parent: Option<usize>,
    step: ProtobufRegionStep,
}

/// One nested canonical `Par` body retained without decoding.
#[derive(Clone, Debug)]
pub struct ProtobufParCut {
    region: ProtobufRegion,
    path_leaf: usize,
}

impl ProtobufParCut {
    #[inline]
    pub fn region(&self) -> &ProtobufRegion { &self.region }
}

/// One EPathMap snapshot retained as native ACTree03 topology plus an ordered
/// range of `Par` value cuts. Set and neutral-empty modes have an empty range.
#[derive(Clone, Debug)]
pub struct ProtobufEPathMapRegion {
    mode: EPathMapMode,
    prefix: ProtobufRegion,
    cuts: Range<usize>,
    path_leaf: usize,
}

impl ProtobufEPathMapRegion {
    #[inline]
    pub const fn mode(&self) -> EPathMapMode { self.mode }

    #[inline]
    pub fn prefix(&self) -> &ProtobufRegion { &self.prefix }

    #[inline]
    pub fn cut_range(&self) -> Range<usize> { self.cuts.clone() }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProtobufCutStats {
    pub field_visits: usize,
    pub message_nodes: usize,
    pub par_cuts: usize,
    pub borrowed_field_bytes: usize,
    pub max_frames: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtobufRegionError {
    Framing(CanonicalProtobufError),
    EPathMap(TrieCodecError),
    Schema {
        type_name: &'static str,
        field: u32,
        detail: &'static str,
    },
    SizeOverflow,
}

impl fmt::Display for ProtobufRegionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(f, "{error}"),
            Self::EPathMap(error) => write!(f, "{error}"),
            Self::Schema {
                type_name,
                field,
                detail,
            } => write!(f, "{type_name} protobuf field {field}: {detail}"),
            Self::SizeOverflow => f.write_str("protobuf region output length overflows usize"),
        }
    }
}

impl std::error::Error for ProtobufRegionError {}

impl From<CanonicalProtobufError> for ProtobufRegionError {
    fn from(error: CanonicalProtobufError) -> Self { Self::Framing(error) }
}

impl From<TrieCodecError> for ProtobufRegionError {
    fn from(error: TrieCodecError) -> Self { Self::EPathMap(error) }
}

#[derive(Clone, Debug)]
enum Piece {
    Borrowed(ProtobufRegion),
    Child { tag: u32, node: usize },
    Par { tag: u32, cut: usize },
    EPathMap { tag: u32, group: usize },
}

#[derive(Clone, Debug)]
struct Node {
    pieces: Vec<Piece>,
    shallow_len: usize,
    original_len: usize,
}

#[derive(Clone, Copy, Debug)]
enum FrameSchema {
    Message(&'static RhoMatchProgram),
    MapEntry {
        owner_type_id: u16,
        outer_tag: u32,
        occurrence: u32,
        value_type_id: u16,
    },
}

impl FrameSchema {
    fn type_name(self) -> &'static str {
        match self {
            Self::Message(program) => program.type_name,
            Self::MapEntry { .. } => "protobuf map entry",
        }
    }
}

struct Frame {
    region: ProtobufRegion,
    fields: CanonicalRegionFieldCursor,
    schema: FrameSchema,
    path_leaf: Option<usize>,
    return_tag: Option<u32>,
    pieces: Vec<Piece>,
    last_tag: u32,
    occurrence: u32,
    seen_singular: Vec<bool>,
}

impl Frame {
    fn new(
        region: ProtobufRegion,
        schema: FrameSchema,
        path_leaf: Option<usize>,
        return_tag: Option<u32>,
    ) -> Self {
        let field_count = match schema {
            FrameSchema::Message(program) => program.fields.len(),
            FrameSchema::MapEntry { .. } => 2,
        };
        Self {
            fields: CanonicalRegionFieldCursor::new(region.clone()),
            region,
            schema,
            path_leaf,
            return_tag,
            pieces: Vec::new(),
            last_tag: 0,
            occurrence: 0,
            seen_singular: vec![false; field_count],
        }
    }

    fn next_occurrence(&mut self, tag: u32) -> u32 {
        if self.last_tag == tag {
            self.occurrence = self
                .occurrence
                .checked_add(1)
                .expect("a field occurrence cannot exceed the message byte length");
        } else {
            self.last_tag = tag;
            self.occurrence = 0;
        }
        self.occurrence
    }
}

/// One complete shallow cut plan. Nodes and paths are arenas rather than
/// recursively owned trees, so clone/drop and emission remain stack-safe.
#[derive(Clone, Debug)]
pub struct ProtobufCutPlan {
    nodes: Vec<Node>,
    root: usize,
    paths: Vec<PathNode>,
    cuts: Vec<ProtobufParCut>,
    epathmaps: Vec<ProtobufEPathMapRegion>,
    stats: ProtobufCutStats,
}

impl ProtobufCutPlan {
    #[inline]
    pub fn cuts(&self) -> &[ProtobufParCut] { &self.cuts }

    #[inline]
    pub fn epathmaps(&self) -> &[ProtobufEPathMapRegion] { &self.epathmaps }

    #[inline]
    pub const fn stats(&self) -> ProtobufCutStats { self.stats }

    pub fn cut_path(&self, cut: usize) -> Option<Vec<ProtobufRegionStep>> {
        let mut at = Some(self.cuts.get(cut)?.path_leaf);
        let mut path = Vec::new();
        while let Some(index) = at {
            let node = &self.paths[index];
            path.push(node.step);
            at = node.parent;
        }
        path.reverse();
        Some(path)
    }

    pub fn epathmap_path(&self, group: usize) -> Option<Vec<ProtobufRegionStep>> {
        let mut at = Some(self.epathmaps.get(group)?.path_leaf);
        let mut path = Vec::new();
        while let Some(index) = at {
            let node = &self.paths[index];
            path.push(node.step);
            at = node.parent;
        }
        path.reverse();
        Some(path)
    }

    pub fn shallow_bytes(&self) -> Bytes { self.emit(true) }

    pub fn reconstructed_bytes(&self) -> Bytes { self.emit(false) }
}

#[derive(Clone, Copy)]
enum ResolvedField {
    Regular {
        index: usize,
        field: &'static RhoMatchField,
        protobuf_kind: ProtobufKind,
    },
    Oneof {
        index: usize,
        variant: &'static RhoMatchOneofVariant,
    },
}

impl ResolvedField {
    fn index(self) -> usize {
        match self {
            Self::Regular { index, .. } | Self::Oneof { index, .. } => index,
        }
    }

    fn protobuf_kind(self) -> ProtobufKind {
        match self {
            Self::Regular { protobuf_kind, .. } => protobuf_kind,
            Self::Oneof { variant, .. } => variant.protobuf_kind,
        }
    }

    fn repeated(self) -> bool {
        match self {
            Self::Regular { field, .. } => matches!(
                field.kind,
                RhoMatchFieldKind::StringSequence
                    | RhoMatchFieldKind::BytesSequence
                    | RhoMatchFieldKind::MessageSequence(_)
                    | RhoMatchFieldKind::StringMessageMap(_)
            ),
            Self::Oneof { .. } => false,
        }
    }
}

fn resolve_field(program: &'static RhoMatchProgram, tag: u32) -> Option<ResolvedField> {
    debug_assert_eq!(program.fields.len(), program.protobuf_fields.len());
    for (index, (field, protobuf)) in program
        .fields
        .iter()
        .zip(program.protobuf_fields)
        .enumerate()
    {
        if let RhoMatchFieldKind::Oneof(variants) = field.kind {
            if let Some(variant) = variants.iter().find(|variant| variant.tag == tag) {
                return Some(ResolvedField::Oneof { index, variant });
            }
        } else if protobuf.tag == tag {
            return Some(ResolvedField::Regular {
                index,
                field,
                protobuf_kind: protobuf.kind,
            });
        }
    }
    None
}

#[inline]
const fn canonical_wire_type(wire: ProtobufWireType) -> CanonicalWireType {
    match wire {
        ProtobufWireType::Varint => CanonicalWireType::Varint,
        ProtobufWireType::Fixed64 => CanonicalWireType::Fixed64,
        ProtobufWireType::LengthDelimited => CanonicalWireType::LengthDelimited,
        ProtobufWireType::Fixed32 => CanonicalWireType::Fixed32,
    }
}

fn verify_wire(
    type_name: &'static str,
    field: &CanonicalRegionField,
    kind: ProtobufKind,
) -> Result<(), ProtobufRegionError> {
    let expected =
        kind.wire_type()
            .map(canonical_wire_type)
            .ok_or(ProtobufRegionError::Schema {
                type_name,
                field: field.number,
                detail: "an unresolved oneof marker has no concrete wire type",
            })?;
    if expected != field.wire_type {
        return Err(CanonicalProtobufError::UnexpectedWireType {
            offset: field.encoded_range.start,
            field: field.number,
            expected,
            actual: field.wire_type,
        }
        .into());
    }
    Ok(())
}

fn child_program(
    owner: &'static RhoMatchProgram,
    field: u32,
    type_id: u16,
) -> Result<&'static RhoMatchProgram, ProtobufRegionError> {
    let (_, program) = RHO_MATCH_REGISTRY
        .get(
            usize::from(type_id)
                .checked_sub(1)
                .ok_or(ProtobufRegionError::Schema {
                    type_name: owner.type_name,
                    field,
                    detail: "child type identifier is zero",
                })?,
        )
        .ok_or(ProtobufRegionError::Schema {
            type_name: owner.type_name,
            field,
            detail: "child type identifier is outside the generated registry",
        })?;
    if program.type_id != type_id {
        return Err(ProtobufRegionError::Schema {
            type_name: owner.type_name,
            field,
            detail: "child type identifier does not index itself",
        });
    }
    Ok(program)
}

struct Builder {
    nodes: Vec<Node>,
    paths: Vec<PathNode>,
    cuts: Vec<ProtobufParCut>,
    epathmaps: Vec<ProtobufEPathMapRegion>,
    stats: ProtobufCutStats,
    par_type_id: u16,
}

impl Builder {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            paths: Vec::new(),
            cuts: Vec::new(),
            epathmaps: Vec::new(),
            stats: ProtobufCutStats::default(),
            par_type_id: PAR_PROTOBUF_REGION_PROGRAM.type_id,
        }
    }

    fn path(&mut self, parent: Option<usize>, step: ProtobufRegionStep) -> usize {
        let index = self.paths.len();
        self.paths.push(PathNode { parent, step });
        index
    }

    fn cut(&mut self, region: ProtobufRegion, path_leaf: usize) -> usize {
        let index = self.cuts.len();
        self.cuts.push(ProtobufParCut { region, path_leaf });
        self.stats.par_cuts += 1;
        index
    }

    fn borrowed(&mut self, owner: &ProtobufRegion, range: CanonicalByteRange) -> Piece {
        let region = owner
            .subregion(range)
            .expect("a parsed protobuf field lies inside its owner region");
        self.stats.borrowed_field_bytes += region.len();
        Piece::Borrowed(region)
    }

    fn add_child(
        &mut self,
        frames: &mut Vec<Frame>,
        payload: ProtobufRegion,
        owner: &'static RhoMatchProgram,
        wire_tag: u32,
        path_tag: u32,
        occurrence: u32,
        kind: ProtobufRegionStepKind,
        type_id: u16,
        parent_path: Option<usize>,
    ) -> Result<(), ProtobufRegionError> {
        let path_leaf = self.path(parent_path, ProtobufRegionStep {
            owner_type_id: owner.type_id,
            field_tag: path_tag,
            occurrence,
            kind,
        });
        if type_id == self.par_type_id {
            let cut = self.cut(payload, path_leaf);
            frames
                .last_mut()
                .expect("a child always has a parent frame")
                .pieces
                .push(Piece::Par { tag: wire_tag, cut });
        } else {
            let program = child_program(owner, path_tag, type_id)?;
            frames.push(Frame::new(
                payload,
                FrameSchema::Message(program),
                Some(path_leaf),
                Some(wire_tag),
            ));
            self.stats.max_frames = self.stats.max_frames.max(frames.len());
        }
        Ok(())
    }

    fn add_epathmap(
        &mut self,
        frames: &mut [Frame],
        payload: ProtobufRegion,
        owner: &'static RhoMatchProgram,
        tag: u32,
        occurrence: u32,
        parent_path: Option<usize>,
    ) -> Result<(), ProtobufRegionError> {
        let inspection = inspect_canonical_snapshot_value_regions(payload.as_bytes())?;
        let group_path = self.path(parent_path, ProtobufRegionStep {
            owner_type_id: owner.type_id,
            field_tag: tag,
            occurrence,
            kind: ProtobufRegionStepKind::EPathMapSnapshot,
        });
        let prefix = payload
            .subregion(CanonicalByteRange {
                start: inspection.prefix.start,
                end: inspection.prefix.end,
            })
            .expect("the inspected EPM1 prefix lies inside its payload");
        let first_cut = self.cuts.len();
        for (ordinal, range) in inspection.value_bodies.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal)
                .expect("an EPM1 ordinal cannot exceed its input byte length");
            let path_leaf = self.path(Some(group_path), ProtobufRegionStep {
                owner_type_id: owner.type_id,
                field_tag: tag,
                occurrence: ordinal,
                kind: ProtobufRegionStepKind::EPathMapValue,
            });
            let region = payload
                .subregion(CanonicalByteRange {
                    start: range.start,
                    end: range.end,
                })
                .expect("an inspected EPM1 value body lies inside its payload");
            self.cut(region, path_leaf);
        }
        let end_cut = self.cuts.len();
        let group = self.epathmaps.len();
        self.epathmaps.push(ProtobufEPathMapRegion {
            mode: inspection.mode,
            prefix,
            cuts: first_cut..end_cut,
            path_leaf: group_path,
        });
        frames
            .last_mut()
            .expect("an EPathMap snapshot always has an owner frame")
            .pieces
            .push(Piece::EPathMap { tag, group });
        Ok(())
    }
}

fn mark_singular(
    frame: &mut Frame,
    index: usize,
    repeated: bool,
    tag: u32,
) -> Result<(), ProtobufRegionError> {
    if repeated {
        return Ok(());
    }
    let seen = frame
        .seen_singular
        .get_mut(index)
        .expect("a resolved schema field indexes its owner program");
    if std::mem::replace(seen, true) {
        return Err(ProtobufRegionError::Schema {
            type_name: frame.schema.type_name(),
            field: tag,
            detail: "canonical singular field is duplicated",
        });
    }
    Ok(())
}

fn process_message_field(
    builder: &mut Builder,
    frames: &mut Vec<Frame>,
    program: &'static RhoMatchProgram,
    path_leaf: Option<usize>,
    occurrence: u32,
    field: CanonicalRegionField,
) -> Result<(), ProtobufRegionError> {
    let Some(resolved) = resolve_field(program, field.number) else {
        let piece = builder.borrowed(
            &frames
                .last()
                .expect("the message owner frame exists")
                .region,
            field.encoded_range,
        );
        frames
            .last_mut()
            .expect("the message owner frame exists")
            .pieces
            .push(piece);
        return Ok(());
    };

    verify_wire(program.type_name, &field, resolved.protobuf_kind())?;
    mark_singular(
        frames.last_mut().expect("the message owner frame exists"),
        resolved.index(),
        resolved.repeated(),
        field.number,
    )?;

    match resolved {
        ResolvedField::Regular {
            field: schema_field,
            protobuf_kind,
            ..
        } => match (schema_field.kind, protobuf_kind) {
            (RhoMatchFieldKind::OptionalMessage(type_id), ProtobufKind::Message)
            | (RhoMatchFieldKind::MessageSequence(type_id), ProtobufKind::RepeatedMessage) => {
                let tag = field.number;
                let payload = field.into_length_delimited()?;
                builder.add_child(
                    frames,
                    payload,
                    program,
                    tag,
                    tag,
                    occurrence,
                    ProtobufRegionStepKind::Message,
                    type_id,
                    path_leaf,
                )?;
            }
            (
                RhoMatchFieldKind::StringMessageMap(value_type_id),
                ProtobufKind::MapStringMessage,
            ) => {
                let outer_tag = field.number;
                let payload = field.into_length_delimited()?;
                frames.push(Frame::new(
                    payload,
                    FrameSchema::MapEntry {
                        owner_type_id: program.type_id,
                        outer_tag,
                        occurrence,
                        value_type_id,
                    },
                    path_leaf,
                    Some(outer_tag),
                ));
                builder.stats.max_frames = builder.stats.max_frames.max(frames.len());
            }
            (RhoMatchFieldKind::PathMap, ProtobufKind::EPathMapSnapshot) => {
                let tag = field.number;
                let payload = field.into_length_delimited()?;
                builder.add_epathmap(frames, payload, program, tag, occurrence, path_leaf)?;
            }
            (
                RhoMatchFieldKind::Bool
                | RhoMatchFieldKind::I32
                | RhoMatchFieldKind::U32
                | RhoMatchFieldKind::I64
                | RhoMatchFieldKind::U64
                | RhoMatchFieldKind::Bytes
                | RhoMatchFieldKind::String
                | RhoMatchFieldKind::StringSequence
                | RhoMatchFieldKind::BytesSequence,
                _,
            ) => {
                let piece = builder.borrowed(
                    &frames
                        .last()
                        .expect("the message owner frame exists")
                        .region,
                    field.encoded_range,
                );
                frames
                    .last_mut()
                    .expect("the message owner frame exists")
                    .pieces
                    .push(piece);
            }
            _ => {
                return Err(ProtobufRegionError::Schema {
                    type_name: program.type_name,
                    field: field.number,
                    detail: "structural and protobuf field kinds disagree",
                });
            }
        },
        ResolvedField::Oneof { variant, .. } => match variant.payload {
            RhoMatchPayloadKind::Message(type_id) => {
                let tag = field.number;
                let payload = field.into_length_delimited()?;
                builder.add_child(
                    frames,
                    payload,
                    program,
                    tag,
                    tag,
                    occurrence,
                    ProtobufRegionStepKind::Message,
                    type_id,
                    path_leaf,
                )?;
            }
            RhoMatchPayloadKind::Bool
            | RhoMatchPayloadKind::I32
            | RhoMatchPayloadKind::U32
            | RhoMatchPayloadKind::I64
            | RhoMatchPayloadKind::U64
            | RhoMatchPayloadKind::Bytes
            | RhoMatchPayloadKind::String => {
                let piece = builder.borrowed(
                    &frames
                        .last()
                        .expect("the message owner frame exists")
                        .region,
                    field.encoded_range,
                );
                frames
                    .last_mut()
                    .expect("the message owner frame exists")
                    .pieces
                    .push(piece);
            }
        },
    }
    Ok(())
}

fn program_by_type_id(type_id: u16) -> Option<&'static RhoMatchProgram> {
    let (_, program) = RHO_MATCH_REGISTRY.get(usize::from(type_id).checked_sub(1)?)?;
    (program.type_id == type_id).then_some(*program)
}

fn process_map_entry_field(
    builder: &mut Builder,
    frames: &mut Vec<Frame>,
    owner_type_id: u16,
    outer_tag: u32,
    occurrence: u32,
    value_type_id: u16,
    parent_path: Option<usize>,
    field: CanonicalRegionField,
) -> Result<(), ProtobufRegionError> {
    let owner = program_by_type_id(owner_type_id).ok_or(ProtobufRegionError::Schema {
        type_name: "protobuf map entry",
        field: field.number,
        detail: "map owner type identifier is outside the generated registry",
    })?;
    match field.number {
        1 => {
            verify_wire("protobuf map entry", &field, ProtobufKind::String)?;
            mark_singular(
                frames.last_mut().expect("the map entry frame exists"),
                0,
                false,
                field.number,
            )?;
            let piece = builder.borrowed(
                &frames.last().expect("the map entry frame exists").region,
                field.encoded_range,
            );
            frames
                .last_mut()
                .expect("the map entry frame exists")
                .pieces
                .push(piece);
        }
        2 => {
            verify_wire("protobuf map entry", &field, ProtobufKind::Message)?;
            mark_singular(
                frames.last_mut().expect("the map entry frame exists"),
                1,
                false,
                field.number,
            )?;
            let payload = field.into_length_delimited()?;
            builder.add_child(
                frames,
                payload,
                owner,
                2,
                outer_tag,
                occurrence,
                ProtobufRegionStepKind::MapValue,
                value_type_id,
                parent_path,
            )?;
        }
        _ => {
            let piece = builder.borrowed(
                &frames.last().expect("the map entry frame exists").region,
                field.encoded_range,
            );
            frames
                .last_mut()
                .expect("the map entry frame exists")
                .pieces
                .push(piece);
        }
    }
    Ok(())
}

fn checked_add(left: usize, right: usize) -> Result<usize, ProtobufRegionError> {
    left.checked_add(right)
        .ok_or(ProtobufRegionError::SizeOverflow)
}

fn delimited_len(tag: u32, payload: usize) -> Result<usize, ProtobufRegionError> {
    let payload_u64 = u64::try_from(payload).map_err(|_| ProtobufRegionError::SizeOverflow)?;
    checked_add(
        checked_add(key_len(tag), encoded_len_varint(payload_u64))?,
        payload,
    )
}

fn epm_payload_len(
    prefix: &ProtobufRegion,
    cuts: Range<usize>,
    all_cuts: &[ProtobufParCut],
    shallow: bool,
) -> Result<usize, ProtobufRegionError> {
    let mut length = prefix.len();
    for cut in &all_cuts[cuts] {
        let body_len = if shallow { 0 } else { cut.region.len() };
        let body_u64 = u64::try_from(body_len).map_err(|_| ProtobufRegionError::SizeOverflow)?;
        length = checked_add(length, encoded_len_varint(body_u64))?;
        length = checked_add(length, body_len)?;
    }
    Ok(length)
}

fn finish_node(
    pieces: Vec<Piece>,
    nodes: &[Node],
    cuts: &[ProtobufParCut],
    epathmaps: &[ProtobufEPathMapRegion],
) -> Result<Node, ProtobufRegionError> {
    let mut shallow_len = 0usize;
    let mut original_len = 0usize;
    for piece in &pieces {
        let (shallow, original) = match piece {
            Piece::Borrowed(region) => (region.len(), region.len()),
            Piece::Child { tag, node } => {
                let child = &nodes[*node];
                (
                    delimited_len(*tag, child.shallow_len)?,
                    delimited_len(*tag, child.original_len)?,
                )
            }
            Piece::Par { tag, cut } => (
                delimited_len(*tag, 0)?,
                delimited_len(*tag, cuts[*cut].region.len())?,
            ),
            Piece::EPathMap { tag, group } => {
                let group = &epathmaps[*group];
                let shallow = epm_payload_len(&group.prefix, group.cuts.clone(), cuts, true)?;
                let original = epm_payload_len(&group.prefix, group.cuts.clone(), cuts, false)?;
                (
                    delimited_len(*tag, shallow)?,
                    delimited_len(*tag, original)?,
                )
            }
        };
        shallow_len = checked_add(shallow_len, shallow)?;
        original_len = checked_add(original_len, original)?;
    }
    Ok(Node {
        pieces,
        shallow_len,
        original_len,
    })
}

/// Build an iterative cut-at-`Par` plan from one canonical protobuf region.
///
/// `program` is normally the generated program for a component type. Pass
/// [`PAR_PROTOBUF_REGION_PROGRAM`] when the root itself is a protobuf `Par`;
/// its resident match program intentionally describes the native bag instead.
pub fn cut_at_par(
    input: ProtobufRegion,
    program: &'static RhoMatchProgram,
) -> Result<ProtobufCutPlan, ProtobufRegionError> {
    if program.fields.len() != program.protobuf_fields.len() {
        return Err(ProtobufRegionError::Schema {
            type_name: program.type_name,
            field: 0,
            detail: "structural and protobuf programs have different lengths",
        });
    }
    let mut builder = Builder::new();
    let mut frames = vec![Frame::new(input, FrameSchema::Message(program), None, None)];
    builder.stats.max_frames = 1;
    let root;

    loop {
        let next = frames
            .last_mut()
            .expect("the root frame remains until the plan is complete")
            .fields
            .next_field()?;
        if let Some(field) = next {
            builder.stats.field_visits += 1;
            let (schema, path_leaf, occurrence) = {
                let frame = frames
                    .last_mut()
                    .expect("the field cursor belongs to a live frame");
                let occurrence = frame.next_occurrence(field.number);
                (frame.schema, frame.path_leaf, occurrence)
            };
            match schema {
                FrameSchema::Message(owner) => process_message_field(
                    &mut builder,
                    &mut frames,
                    owner,
                    path_leaf,
                    occurrence,
                    field,
                )?,
                FrameSchema::MapEntry {
                    owner_type_id,
                    outer_tag,
                    occurrence,
                    value_type_id,
                } => process_map_entry_field(
                    &mut builder,
                    &mut frames,
                    owner_type_id,
                    outer_tag,
                    occurrence,
                    value_type_id,
                    path_leaf,
                    field,
                )?,
            }
            continue;
        }

        let frame = frames
            .pop()
            .expect("an exhausted cursor belongs to a live frame");
        let return_tag = frame.return_tag;
        let expected_original_len = frame.region.len();
        let node = finish_node(
            frame.pieces,
            &builder.nodes,
            &builder.cuts,
            &builder.epathmaps,
        )?;
        if node.original_len != expected_original_len {
            return Err(ProtobufRegionError::Schema {
                type_name: frame.schema.type_name(),
                field: return_tag.unwrap_or(0),
                detail: "region decomposition does not reconstruct the original length",
            });
        }
        let node_index = builder.nodes.len();
        builder.nodes.push(node);
        builder.stats.message_nodes += 1;
        if let Some(tag) = return_tag {
            frames
                .last_mut()
                .expect("a non-root node has a parent frame")
                .pieces
                .push(Piece::Child {
                    tag,
                    node: node_index,
                });
        } else {
            root = node_index;
            break;
        }
    }

    Ok(ProtobufCutPlan {
        nodes: builder.nodes,
        root,
        paths: builder.paths,
        cuts: builder.cuts,
        epathmaps: builder.epathmaps,
        stats: builder.stats,
    })
}

fn emit_delimited_header(tag: u32, payload_len: usize, out: &mut Vec<u8>) {
    encode_key(tag, WireType::LengthDelimited, out);
    encode_varint(
        u64::try_from(payload_len).expect("a planned usize protobuf length fits u64"),
        out,
    );
}

enum Emit {
    Node(usize),
    Piece { node: usize, piece: usize },
}

impl ProtobufCutPlan {
    fn emit(&self, shallow: bool) -> Bytes {
        let root_len = if shallow {
            self.nodes[self.root].shallow_len
        } else {
            self.nodes[self.root].original_len
        };
        let mut out = Vec::with_capacity(root_len);
        let mut work = vec![Emit::Node(self.root)];
        while let Some(item) = work.pop() {
            match item {
                Emit::Node(node) => {
                    for piece in (0..self.nodes[node].pieces.len()).rev() {
                        work.push(Emit::Piece { node, piece });
                    }
                }
                Emit::Piece { node, piece } => match &self.nodes[node].pieces[piece] {
                    Piece::Borrowed(region) => out.extend_from_slice(region.as_bytes()),
                    Piece::Child { tag, node } => {
                        let child_len = if shallow {
                            self.nodes[*node].shallow_len
                        } else {
                            self.nodes[*node].original_len
                        };
                        emit_delimited_header(*tag, child_len, &mut out);
                        work.push(Emit::Node(*node));
                    }
                    Piece::Par { tag, cut } => {
                        let body = &self.cuts[*cut].region;
                        let body_len = if shallow { 0 } else { body.len() };
                        emit_delimited_header(*tag, body_len, &mut out);
                        if !shallow {
                            out.extend_from_slice(body.as_bytes());
                        }
                    }
                    Piece::EPathMap { tag, group } => {
                        let group = &self.epathmaps[*group];
                        let payload_len =
                            epm_payload_len(&group.prefix, group.cuts.clone(), &self.cuts, shallow)
                                .expect("validated plan lengths remain representable");
                        emit_delimited_header(*tag, payload_len, &mut out);
                        out.extend_from_slice(group.prefix.as_bytes());
                        for cut in &self.cuts[group.cuts.clone()] {
                            let body_len = if shallow { 0 } else { cut.region.len() };
                            encode_varint(
                                u64::try_from(body_len)
                                    .expect("a planned usize protobuf length fits u64"),
                                &mut out,
                            );
                            if !shallow {
                                out.extend_from_slice(cut.region.as_bytes());
                            }
                        }
                    }
                },
            }
        }
        debug_assert_eq!(out.len(), root_len);
        Bytes::from(out)
    }
}
