use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_admission_cursor.rs"]
pub mod admission;
#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor as admission;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_path_parser_refinement.rs"]
pub mod parser;
#[cfg(rpb3_integrated)]
pub use crate::rpb3_path_parser_refinement as parser;

verus! {

pub enum PathObligationKind {
    RawParEscape,
    NestedEpm1,
}

pub enum EpmMode {
    Empty,
    Set,
    Map,
}

pub struct SourceAnchor {
    pub source: usize,
    pub offset: usize,
}

pub struct IndexedRegionImage {
    pub ghost bytes: Seq<u8>,
    pub ghost positions: Seq<SourceAnchor>,
    pub end: SourceAnchor,
}

pub struct SourceTreeImage {
    pub ghost bytes: Seq<u8>,
    pub ghost positions: Seq<SourceAnchor>,
}

pub enum IndexedBackingImage {
    Borrowed {
        ghost bytes: Seq<u8>,
        source: usize,
        absolute_start: usize,
        start: usize,
        len: usize,
        end_override: Option<SourceAnchor>,
    },
    Tree {
        ghost root: SourceTreeImage,
        start: usize,
        len: usize,
        root_end: SourceAnchor,
    },
}

pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

pub struct PathObligation {
    pub id: u64,
    pub kind: PathObligationKind,
    pub payload_len: usize,
    pub tag_source: SourceAnchor,
    pub payload_start: usize,
    pub payload_end: SourceAnchor,
    pub awaiting_resolution: bool,
}

pub struct PathGate {
    pub dead: bool,
    pub stack_empty: bool,
    pub split_complete: bool,
    pub completed_segments: usize,
    pub first_is_list: bool,
    pub stable: bool,
    pub escape_allowed: bool,
}

pub struct PathCheckpoint {
    pub pending: Option<PathObligation>,
    pub gate: PathGate,
    pub path_len: usize,
    pub ghost parser: parser::ParserImage,
}

pub struct PathTopology {
    pub next_obligation: u64,
    pub pending: Option<PathObligation>,
    pub root_left: bool,
    pub ghost restore: Seq<PathCheckpoint>,
    pub ghost path_bytes: Seq<u8>,
    pub ghost path_positions: Seq<SourceAnchor>,
    pub gate: PathGate,
    pub ghost parser: parser::ParserImage,
}

pub enum EpmWait {
    MapValue { return_id: u64 },
    Obligation { id: u64, kind: PathObligationKind, return_id: u64 },
}

pub struct EpmFrame {
    pub path: PathTopology,
    pub cursor: EpmCursorImage,
    pub wait: Option<EpmWait>,
    pub mode: EpmMode,
    pub entries_stable: bool,
    pub last_source: SourceAnchor,
    pub ghost snapshot: IndexedRegionImage,
    pub ghost backing: IndexedBackingImage,
    pub trie_start: usize,
    pub trie_end: usize,
    pub value_table_start: usize,
    pub mode_offset: usize,
}

pub enum WireTopologyEvent {
    Edge { source_offset: usize, origin: EdgeOrigin },
    Leave { source_offset: usize },
    Value { has_value: bool, value_body: Option<ByteSpan>, source_offset: usize },
}

pub struct EpmCursorImage {
    pub ghost act_plan: Option<ActPlanImage>,
    pub ghost validated_plan: Option<ValidatedActPlanImage>,
    pub at: usize,
    pub declared_value_count: usize,
    pub map_values_seen: usize,
    pub value_table_offset: usize,
    pub done: bool,
}

pub struct ActRecordImage {
    pub node_id: usize,
    pub subtree_end: usize,
    pub value: Option<u64>,
    pub value_source_offset: usize,
    pub line_start: usize,
    pub line_len: usize,
    pub line_encoded_start: usize,
    pub wrapper_id: Option<usize>,
    pub wrapper_len: usize,
    pub incoming_source_offset: Option<usize>,
    pub incoming_origin: Option<EdgeOrigin>,
    pub ghost children: Seq<usize>,
}

pub struct ActPlanImage {
    pub ghost records: Seq<ActRecordImage>,
    pub ghost finish_order: Seq<usize>,
    pub ghost line_hash: spec_fn(Seq<u8>) -> u64,
}

pub struct ValidatedTopologyRecordImage {
    pub node_id: usize,
    pub value: u64,
    pub value_source_offset: usize,
    pub line_start: usize,
    pub line_len: usize,
    pub incoming_source_offset: usize,
    pub child_count: u16,
    pub incoming_byte: u8,
    pub flags: u8,
}

pub struct ValidatedActPlanImage {
    pub ghost records: Seq<ValidatedTopologyRecordImage>,
    pub empty: bool,
    pub value_count: usize,
    pub first_nonzero: Option<(usize, u64)>,
}

pub enum ChildRequest {
    ParValue { return_id: u64, ghost body: IndexedRegionImage },
    RawParEscape { id: u64, return_id: u64, ghost body: IndexedRegionImage },
    NestedEpm1 { id: u64, return_id: u64, ghost body: IndexedRegionImage },
}

pub enum ChildSummary {
    Par { canonical_path_stable: bool },
    Epm1 { mode: EpmMode, entries_stable: bool },
    Other,
}

pub enum EpmAction {
    Continue,
    Push(ChildRequest),
    Complete { mode: EpmMode, entries_stable: bool },
}

pub enum EpmEvent {
    Edge { source_offset: usize, origin: EdgeOrigin, effect: PathEdgeEffect },
    Leave { source_offset: usize },
    Value { has_value: bool, value_body: Option<ByteSpan>, source_offset: usize },
    End,
}

pub enum EdgeOrigin {
    Direct,
    Dense { mask_start: usize, bit: u8 },
}

pub enum PathEdgeEffect {
    Plain,
    Start { kind: PathObligationKind, payload_len: usize, tag: SourceAnchor },
    Payload,
}

pub open spec fn indexed_image_wf(image: IndexedRegionImage) -> bool {
    image.bytes.len() == image.positions.len()
}

pub open spec fn indexed_backing_project(
    backing: IndexedBackingImage,
) -> Option<IndexedRegionImage> {
    match backing {
        IndexedBackingImage::Borrowed {
            bytes, source, absolute_start, start, len, end_override,
        } => {
            if start as int + len as int > bytes.len()
                || absolute_start as int + start as int + len as int > usize::MAX as int
            {
                None
            } else {
                let end = match end_override {
                    Some(anchor) => anchor,
                    None => SourceAnchor {
                        source, offset: (absolute_start as int + start as int + len as int) as usize,
                    },
                };
                Some(IndexedRegionImage {
                    bytes: bytes.subrange(start as int, start as int + len as int),
                    positions: Seq::new(len as nat, |i: int| SourceAnchor {
                        source,
                        offset: (absolute_start as int + start as int + i) as usize,
                    }),
                    end,
                })
            }
        },
        IndexedBackingImage::Tree { root, start, len, root_end } => {
            if root.bytes.len() != root.positions.len()
                || start as int + len as int > root.bytes.len()
            {
                None
            } else {
                let end_index = start as int + len as int;
                Some(IndexedRegionImage {
                    bytes: root.bytes.subrange(start as int, end_index),
                    positions: root.positions.subrange(start as int, end_index),
                    end: if end_index == root.bytes.len() {
                        root_end
                    } else {
                        root.positions[end_index]
                    },
                })
            }
        },
    }
}

pub open spec fn indexed_position_at(image: IndexedRegionImage, index: usize) -> SourceAnchor
    recommends indexed_image_wf(image), index <= image.bytes.len(),
{
    if index == image.bytes.len() { image.end } else { image.positions[index as int] }
}

pub open spec fn indexed_subview(image: IndexedRegionImage, span: ByteSpan) -> IndexedRegionImage
    recommends indexed_image_wf(image), span.start <= span.end,
        span.end <= image.bytes.len(),
{
    IndexedRegionImage {
        bytes: image.bytes.subrange(span.start as int, span.end as int),
        positions: image.positions.subrange(span.start as int, span.end as int),
        end: indexed_position_at(image, span.end),
    }
}

pub open spec fn indexed_from_source(
    source: SourceTreeImage, end: SourceAnchor,
) -> IndexedRegionImage {
    IndexedRegionImage { bytes: source.bytes, positions: source.positions, end }
}

pub open spec fn frame_layout_wf(frame: EpmFrame) -> bool {
    &&& indexed_image_wf(frame.snapshot)
    &&& indexed_backing_project(frame.backing) == Some(frame.snapshot)
    &&& match epm_prefix_from_bytes(frame.snapshot.bytes) {
        Some(prefix) => {
            &&& frame.mode == prefix.mode
            &&& frame.trie_start == prefix.trie_start
            &&& frame.trie_end == prefix.trie_end
            &&& frame.value_table_start == prefix.value_table_start
            &&& frame.mode_offset == 5
            &&& frame.cursor.declared_value_count == prefix.value_count
        },
        None => false,
    }
    &&& frame.path.path_bytes.len() == frame.path.path_positions.len()
    &&& match (frame.cursor.act_plan, frame.cursor.validated_plan) {
        (Some(plan), Some(validated)) => frame.mode != EpmMode::Empty
            && act_plan_projects_validated(frame, plan, validated)
            && act_plan_events(frame, plan).is_some(),
        (None, None) => {
            &&& frame.mode == EpmMode::Empty
            &&& frame.cursor.declared_value_count == 0
            &&& frame.value_table_start == frame.snapshot.bytes.len()
        },
        _ => false,
    }
    &&& frame.cursor.at <= cursor_event_count(frame)
    &&& frame.value_table_start <= frame.cursor.value_table_offset
    &&& frame.cursor.value_table_offset <= frame.snapshot.bytes.len()
    &&& (frame.cursor.at != 0 || (
        frame.cursor.value_table_offset == frame.value_table_start
        && frame.cursor.map_values_seen == 0
    ))
}

pub open spec fn trie_position(frame: EpmFrame, offset: usize) -> SourceAnchor
    recommends frame_layout_wf(frame), offset < frame.trie_end - frame.trie_start,
{
    frame.snapshot.positions[(frame.trie_start + offset) as int]
}

pub open spec fn trie_byte(frame: EpmFrame, offset: usize) -> u8
    recommends frame_layout_wf(frame), offset < frame.trie_end - frame.trie_start,
{
    frame.snapshot.bytes[(frame.trie_start + offset) as int]
}

pub open spec fn power_of_two(exponent: nat) -> nat
    decreases exponent,
{
    if exponent == 0 { 1 } else { 2 * power_of_two((exponent as int - 1) as nat) }
}

pub struct CanonicalVarintImage {
    pub value: u64,
    pub end: usize,
}

pub struct EpmPrefixImage {
    pub mode: EpmMode,
    pub trie_start: usize,
    pub trie_end: usize,
    pub value_count: usize,
    pub value_table_start: usize,
}

pub open spec fn canonical_varint_tail(
    bytes: Seq<u8>, at: usize, group: usize, accumulated: int,
) -> Option<CanonicalVarintImage>
    decreases 10 - group,
{
    if group >= 10 || at >= bytes.len() {
        None
    } else {
        let byte = bytes[at as int];
        if group == 9 && byte > 1 {
            None
        } else {
            let value = accumulated
                + ((byte as int % 128) * power_of_two((7 * group) as nat) as int);
            if byte < 128 {
                if (group > 0 && byte == 0) || value > u64::MAX as int {
                    None
                } else {
                    Some(CanonicalVarintImage {
                        value: value as u64,
                        end: (at as int + 1) as usize,
                    })
                }
            } else if group == 9 {
                None
            } else {
                canonical_varint_tail(
                    bytes, (at as int + 1) as usize,
                    (group as int + 1) as usize, value,
                )
            }
        }
    }
}

pub open spec fn canonical_varint_at(
    bytes: Seq<u8>, at: usize,
) -> Option<CanonicalVarintImage> {
    canonical_varint_tail(bytes, at, 0, 0)
}

pub open spec fn epm_mode_byte(byte: u8) -> Option<EpmMode> {
    match byte {
        0 => Some(EpmMode::Empty),
        1 => Some(EpmMode::Set),
        2 => Some(EpmMode::Map),
        _ => None,
    }
}

pub open spec fn epm_prefix_from_bytes(bytes: Seq<u8>) -> Option<EpmPrefixImage> {
    if bytes.len() < 6
        || bytes[0] != 0x45 || bytes[1] != 0x50
        || bytes[2] != 0x4d || bytes[3] != 0x31
        || bytes[4] != 1
    {
        None
    } else {
        match (epm_mode_byte(bytes[5]), canonical_varint_at(bytes, 6)) {
            (Some(mode), Some(trie_len)) if trie_len.value <= usize::MAX => {
                let trie_start = trie_len.end;
                let trie_end = trie_start as int + trie_len.value as int;
                if trie_end > bytes.len() || trie_end > usize::MAX as int {
                    None
                } else {
                    match canonical_varint_at(bytes, trie_end as usize) {
                        Some(count) if count.value <= usize::MAX => {
                            let value_table_start = count.end;
                            let remaining = bytes.len() - value_table_start;
                            if count.value as int > remaining
                                || (mode == EpmMode::Empty
                                    && (trie_len.value != 0 || count.value != 0
                                        || remaining != 0))
                                || (mode == EpmMode::Set
                                    && (count.value != 0 || remaining != 0))
                            {
                                None
                            } else {
                                Some(EpmPrefixImage {
                                    mode,
                                    trie_start,
                                    trie_end: trie_end as usize,
                                    value_count: count.value as usize,
                                    value_table_start,
                                })
                            }
                        },
                        _ => None,
                    }
                }
            },
            _ => None,
        }
    }
}

pub open spec fn map_value_body_at(
    bytes: Seq<u8>, at: usize,
) -> Option<ByteSpan> {
    match canonical_varint_at(bytes, at) {
        Some(length) if length.value <= usize::MAX => {
            let end = length.end as int + length.value as int;
            if at as int <= end && end <= bytes.len() && end <= usize::MAX as int {
                Some(ByteSpan { start: length.end, end: end as usize })
            } else {
                None
            }
        },
        _ => None,
    }
}

pub struct ActLeafImage {
    pub root_id: usize,
    pub line_start: usize,
    pub line_len: usize,
    pub is_line: bool,
}

pub struct ActEdgeImage {
    pub byte: u8,
    pub source_offset: usize,
    pub origin: EdgeOrigin,
}

pub struct ActBranchImage {
    pub value: Option<u64>,
    pub first_child: Option<usize>,
    pub ghost edges: Seq<ActEdgeImage>,
    pub encoded_len: usize,
}

pub struct ActSemanticImage {
    pub value: Option<u64>,
    pub value_source_offset: usize,
    pub line_start: usize,
    pub line_len: usize,
    pub line_encoded_start: usize,
    pub wrapper_id: Option<usize>,
    pub wrapper_len: usize,
    pub first_child: Option<usize>,
    pub ghost edges: Seq<ActEdgeImage>,
    pub encoded_len: usize,
}

pub open spec fn act_value_at(
    bytes: Seq<u8>, node_id: usize,
) -> Option<(Option<u64>, usize)> {
    if node_id >= bytes.len() {
        None
    } else if (bytes[node_id as int] as int / 64) % 2 == 0 {
        Some((None, (node_id as int + 1) as usize))
    } else {
        match act_varint_at(bytes, (node_id as int + 1) as usize) {
            Some(value) => Some((Some(value.value), value.end)),
            None => None,
        }
    }
}

pub open spec fn sparse_act_edges(
    bytes: Seq<u8>, start: usize, count: usize,
) -> Option<Seq<ActEdgeImage>>
    decreases count,
{
    if count == 0 {
        Some(Seq::empty())
    } else if start as int + count as int > bytes.len() {
        None
    } else {
        match sparse_act_edges(bytes, start, (count as int - 1) as usize) {
            Some(previous) => {
                let offset = (start as int + count as int - 1) as usize;
                let byte = bytes[offset as int];
                if previous.len() > 0 && previous.last().byte >= byte {
                    None
                } else {
                    Some(previous.push(ActEdgeImage {
                        byte, source_offset: offset, origin: EdgeOrigin::Direct,
                    }))
                }
            },
            None => None,
        }
    }
}

pub open spec fn dense_act_edges(
    bytes: Seq<u8>, mask_start: usize, next_byte: usize,
) -> Seq<ActEdgeImage>
    decreases 256 - next_byte,
{
    if next_byte >= 256 {
        Seq::empty()
    } else {
        let offset = mask_start as int + (next_byte / 8) as int;
        let bit = next_byte % 8;
        let rest = dense_act_edges(bytes, mask_start, (next_byte as int + 1) as usize);
        if bytes[offset] as int % (power_of_two((bit as nat) + 1) as int)
            >= power_of_two(bit as nat) as int
        {
            seq![ActEdgeImage {
                byte: next_byte as u8,
                source_offset: offset as usize,
                origin: EdgeOrigin::Dense { mask_start, bit: bit as u8 },
            }] + rest
        } else {
            rest
        }
    }
}

pub open spec fn act_branch_at(
    bytes: Seq<u8>, node_id: usize,
) -> Option<ActBranchImage> {
    if node_id >= bytes.len() || bytes[node_id as int] >= 128 {
        None
    } else {
        let count = bytes[node_id as int] as int % 64;
        if count > 32 {
            None
        } else {
            match act_value_at(bytes, node_id) {
                Some((value, after_value)) => {
                    if count == 0 {
                        Some(ActBranchImage {
                            value, first_child: None,
                            edges: Seq::empty(),
                            encoded_len: (after_value as int - node_id as int) as usize,
                        })
                    } else {
                        match act_varint_at(bytes, after_value) {
                            Some(distance) if distance.value > 0
                                && distance.value as int <= node_id as int - 16 => {
                                let first_child = (node_id as int - distance.value as int) as usize;
                                if count < 32 {
                                    match sparse_act_edges(bytes, distance.end, count as usize) {
                                        Some(edges) => Some(ActBranchImage {
                                            value, first_child: Some(first_child), edges,
                                            encoded_len: (distance.end as int + count
                                                - node_id as int) as usize,
                                        }),
                                        None => None,
                                    }
                                } else if distance.end as int + 32 <= bytes.len() {
                                    let edges = dense_act_edges(bytes, distance.end, 0);
                                    if edges.len() < 32 {
                                        None
                                    } else {
                                        Some(ActBranchImage {
                                            value, first_child: Some(first_child), edges,
                                            encoded_len: (distance.end as int + 32
                                                - node_id as int) as usize,
                                        })
                                    }
                                } else {
                                    None
                                }
                            },
                            _ => None,
                        }
                    }
                },
                None => None,
            }
        }
    }
}

pub open spec fn act_semantic_at(
    bytes: Seq<u8>, node_id: usize,
) -> Option<ActSemanticImage> {
    if node_id >= bytes.len() {
        None
    } else if bytes[node_id as int] < 128 {
        match act_branch_at(bytes, node_id) {
            Some(branch) if !(branch.value.is_none() && branch.edges.len() == 1) => {
                Some(ActSemanticImage {
                    value: branch.value,
                    value_source_offset: node_id,
                    line_start: 0, line_len: 0,
                    line_encoded_start: 0,
                    wrapper_id: None, wrapper_len: 0,
                    first_child: branch.first_child,
                    edges: branch.edges,
                    encoded_len: branch.encoded_len,
                })
            },
            _ => None,
        }
    } else {
        let head = bytes[node_id as int];
        if head as int % 64 >= 2 {
            None
        } else {
            match act_value_at(bytes, node_id) {
                Some((line_value, after_value)) => {
                    let child = if head as int % 2 == 1 {
                        match act_varint_at(bytes, after_value) {
                            Some(distance) if distance.value > 0
                                && distance.value as int <= node_id as int - 16 => {
                                Some((Some((node_id as int - distance.value as int) as usize),
                                    distance.end))
                            },
                            _ => None,
                        }
                    } else {
                        Some((None, after_value))
                    };
                    match child {
                        Some((wrapper, after_child)) => {
                            match act_varint_at(bytes, after_child) {
                                Some(distance) if distance.value > 0
                                    && distance.value as int <= node_id as int - 16 => {
                                    let line_id = (node_id as int - distance.value as int) as usize;
                                    match act_varint_at(bytes, line_id) {
                                        Some(length) if length.value > 0
                                            && length.end as int + length.value as int
                                                <= node_id as int => {
                                            match wrapper {
                                                None => Some(ActSemanticImage {
                                                    value: line_value,
                                                    value_source_offset: node_id,
                                                    line_start: length.end,
                                                    line_len: length.value as usize,
                                                    line_encoded_start: line_id,
                                                    wrapper_id: None, wrapper_len: 0,
                                                    first_child: None,
                                                    edges: Seq::empty(),
                                                    encoded_len: (distance.end as int
                                                        - node_id as int) as usize,
                                                }),
                                                Some(wrapper_id) if line_value.is_none() => {
                                                    match act_branch_at(bytes, wrapper_id) {
                                                        Some(branch) if branch.edges.len() > 0
                                                            && !(branch.value.is_none()
                                                                && branch.edges.len() == 1) => {
                                                            Some(ActSemanticImage {
                                                                value: branch.value,
                                                                value_source_offset: wrapper_id,
                                                                line_start: length.end,
                                                                line_len: length.value as usize,
                                                                line_encoded_start: line_id,
                                                                wrapper_id: Some(wrapper_id),
                                                                wrapper_len: branch.encoded_len,
                                                                first_child: branch.first_child,
                                                                edges: branch.edges,
                                                                encoded_len: (distance.end as int
                                                                    - node_id as int) as usize,
                                                            })
                                                        },
                                                        _ => None,
                                                    }
                                                },
                                                _ => None,
                                            }
                                        },
                                        _ => None,
                                    }
                                },
                                _ => None,
                            }
                        },
                        None => None,
                    }
                },
                None => None,
            }
        }
    }
}

pub open spec fn act_record_matches(
    bytes: Seq<u8>, records: Seq<ActRecordImage>, index: int,
) -> bool
    recommends 0 <= index < records.len(),
{
    let record = records[index];
    match act_semantic_at(bytes, record.node_id) {
        Some(semantic) => {
            &&& record.node_id >= 16
            &&& record.node_id as int + semantic.encoded_len as int <= bytes.len() - 8
            &&& record.value == semantic.value
            &&& record.value_source_offset == semantic.value_source_offset
            &&& record.line_start == semantic.line_start
            &&& record.line_len == semantic.line_len
            &&& record.line_encoded_start == semantic.line_encoded_start
            &&& record.wrapper_id == semantic.wrapper_id
            &&& record.wrapper_len == semantic.wrapper_len
            &&& (record.value.is_none() || record.value == Some(0))
            &&& record.children.len() == semantic.edges.len()
            &&& index < record.subtree_end
            &&& record.subtree_end <= records.len()
            &&& if index == 0 {
                record.incoming_source_offset.is_none()
                    && record.incoming_origin.is_none()
            } else {
                record.incoming_source_offset.is_some()
                    && record.incoming_origin.is_some()
            }
            &&& if record.children.len() == 0 {
                record.subtree_end == index + 1
                    && semantic.first_child.is_none()
            } else {
                &&& forall|child_number: int|
                    0 <= child_number < record.children.len()
                        ==> record.children[child_number] < records.len()
                &&& record.children[0] == index + 1
                &&& semantic.first_child
                    == Some(records[record.children[0] as int].node_id)
                &&& record.subtree_end
                    == records[record.children.last() as int].subtree_end
                &&& forall|child_number: int|
                    0 <= child_number < record.children.len() ==> {
                        let child_index = #[trigger] record.children[child_number];
                        let child = records[child_index as int];
                        let parent_id = match semantic.wrapper_id {
                            Some(wrapper_id) => wrapper_id,
                            None => record.node_id,
                        };
                        &&& index < child_index
                        &&& child_index < record.subtree_end
                        &&& child.node_id < parent_id
                        &&& child.incoming_source_offset
                            == Some(semantic.edges[child_number].source_offset)
                        &&& child.incoming_origin
                            == Some(semantic.edges[child_number].origin)
                        &&& if child_number == 0 {
                            child_index == index + 1
                        } else {
                            let previous = records[record.children[child_number - 1] as int];
                            &&& child_index == previous.subtree_end
                            &&& match act_semantic_at(bytes, previous.node_id) {
                                Some(previous_semantic) => {
                                    child.node_id as int == previous.node_id as int
                                        + previous_semantic.encoded_len as int
                                },
                                None => false,
                            }
                        }
                    }
            }
        },
        None => false,
    }
}

pub struct ActProducerLine {
    pub encoded_start: usize,
    pub ghost suffix: Seq<u8>,
}

pub struct ActProducerState {
    pub position: usize,
    pub ghost lines: Seq<ActProducerLine>,
    pub ghost line_hash: spec_fn(Seq<u8>) -> u64,
}

pub open spec fn latest_hashed_act_line(
    lines: Seq<ActProducerLine>, suffix: Seq<u8>,
    line_hash: spec_fn(Seq<u8>) -> u64,
) -> Option<ActProducerLine>
    decreases lines.len(),
{
    if lines.len() == 0 {
        None
    } else if line_hash(lines.last().suffix) == line_hash(suffix) {
        Some(lines.last())
    } else {
        latest_hashed_act_line(lines.drop_last(), suffix, line_hash)
    }
}

pub open spec fn act_line_hash_observations_match(
    bytes: Seq<u8>, plan: ActPlanImage, observed_hashes: Seq<u64>,
) -> bool {
    &&& observed_hashes.len() == plan.records.len()
    &&& forall|i: int| 0 <= i < plan.records.len()
        && plan.records[i].line_len > 0 ==> {
            let record = plan.records[i];
            let end = record.line_start as int + record.line_len as int;
            &&& end <= bytes.len()
            &&& observed_hashes[i] == (plan.line_hash)(
                bytes.subrange(record.line_start as int, end))
        }
}

pub open spec fn act_inserted_hashes_match(
    lines: Seq<ActProducerLine>, observed_hashes: Seq<u64>,
    line_hash: spec_fn(Seq<u8>) -> u64,
) -> bool {
    &&& lines.len() == observed_hashes.len()
    &&& forall|i: int| 0 <= i < lines.len()
        ==> observed_hashes[i] == line_hash(lines[i].suffix)
}

pub open spec fn act_observed_hash_buckets(
    lines: Seq<ActProducerLine>, observed_hashes: Seq<u64>,
) -> Map<u64, ActProducerLine>
    decreases lines.len(),
{
    if lines.len() == 0 || lines.len() != observed_hashes.len() {
        Map::empty()
    } else {
        act_observed_hash_buckets(lines.drop_last(), observed_hashes.drop_last())
            .insert(observed_hashes.last(), lines.last())
    }
}

pub proof fn act_hashed_scan_matches_observed_buckets(
    lines: Seq<ActProducerLine>, observed_hashes: Seq<u64>,
    suffix: Seq<u8>, line_hash: spec_fn(Seq<u8>) -> u64,
)
    requires act_inserted_hashes_match(lines, observed_hashes, line_hash),
    ensures {
        let buckets = act_observed_hash_buckets(lines, observed_hashes);
        let key = line_hash(suffix);
        match latest_hashed_act_line(lines, suffix, line_hash) {
            Some(line) => buckets.contains_key(key) && buckets[key] == line,
            None => !buckets.contains_key(key),
        }
    },
    decreases lines.len(),
{
    if lines.len() > 0 {
        let prior_lines = lines.drop_last();
        let prior_hashes = observed_hashes.drop_last();
        assert(act_inserted_hashes_match(
            prior_lines, prior_hashes, line_hash)) by {
            assert forall|i: int| 0 <= i < prior_lines.len()
                implies prior_hashes[i] == line_hash(prior_lines[i].suffix) by {
                assert(prior_lines[i] == lines[i]);
                assert(prior_hashes[i] == observed_hashes[i]);
            }
        }
        act_hashed_scan_matches_observed_buckets(
            prior_lines, prior_hashes, suffix, line_hash);
        assert(observed_hashes.last() == line_hash(lines.last().suffix));
        let key = line_hash(suffix);
        let prior_buckets = act_observed_hash_buckets(prior_lines, prior_hashes);
        let buckets = act_observed_hash_buckets(lines, observed_hashes);
        assert(buckets == prior_buckets.insert(observed_hashes.last(), lines.last()));
        if observed_hashes.last() == key {
            assert(buckets.contains_key(key));
            assert(buckets[key] == lines.last());
            assert(latest_hashed_act_line(lines, suffix, line_hash)
                == Some(lines.last()));
        } else {
            assert(buckets.contains_key(key) == prior_buckets.contains_key(key));
            if prior_buckets.contains_key(key) {
                assert(buckets[key] == prior_buckets[key]);
            }
            assert(latest_hashed_act_line(lines, suffix, line_hash)
                == latest_hashed_act_line(prior_lines, suffix, line_hash));
        }
    }
}

pub proof fn act_collision_hides_stale_equal_line(
    lines: Seq<ActProducerLine>, suffix: Seq<u8>, colliding: ActProducerLine,
    line_hash: spec_fn(Seq<u8>) -> u64,
)
    requires
        line_hash(colliding.suffix) == line_hash(suffix),
        colliding.suffix != suffix,
    ensures
        latest_hashed_act_line(lines.push(colliding), suffix, line_hash)
            == Some(colliding),
{
}

pub struct ActHashTraceState {
    pub position: usize,
    pub ghost lines: Seq<ActProducerLine>,
    pub ghost inserted_hashes: Seq<u64>,
}

pub enum ActHashTraceDecision {
    Append,
    Reuse(usize),
    Reject,
}

pub open spec fn act_hash_trace_physical_consistent(
    plan: ActPlanImage, hash_results: Seq<u64>,
) -> bool {
    &&& hash_results.len() == plan.records.len()
    &&& forall|i: int, j: int| 0 <= i < plan.records.len()
        && 0 <= j < plan.records.len()
        && plan.records[i].line_len > 0
        && plan.records[j].line_len > 0
        && plan.records[i].line_encoded_start
            == plan.records[j].line_encoded_start
        ==> hash_results[i] == hash_results[j]
}

pub open spec fn latest_trace_hashed_act_line(
    lines: Seq<ActProducerLine>, inserted_hashes: Seq<u64>, hash: u64,
) -> Option<ActProducerLine>
    decreases lines.len(),
{
    if lines.len() == 0 || lines.len() != inserted_hashes.len() {
        None
    } else if inserted_hashes.last() == hash {
        Some(lines.last())
    } else {
        latest_trace_hashed_act_line(
            lines.drop_last(), inserted_hashes.drop_last(), hash)
    }
}

pub open spec fn act_hash_trace_line_decision(
    lines: Seq<ActProducerLine>, inserted_hashes: Seq<u64>,
    suffix: Seq<u8>, encoded_start: usize, position: usize, hash: u64,
) -> ActHashTraceDecision {
    if lines.len() != inserted_hashes.len() {
        ActHashTraceDecision::Reject
    } else {
        match latest_trace_hashed_act_line(lines, inserted_hashes, hash) {
            Some(previous) if previous.suffix == suffix => {
                if previous.encoded_start == encoded_start {
                    ActHashTraceDecision::Reuse(previous.encoded_start)
                } else {
                    ActHashTraceDecision::Reject
                }
            },
            _ => if encoded_start == position {
                ActHashTraceDecision::Append
            } else {
                ActHashTraceDecision::Reject
            },
        }
    }
}

pub proof fn act_hash_trace_reuse_is_byte_equal(
    lines: Seq<ActProducerLine>, inserted_hashes: Seq<u64>,
    suffix: Seq<u8>, encoded_start: usize, position: usize, hash: u64,
)
    ensures match act_hash_trace_line_decision(
        lines, inserted_hashes, suffix, encoded_start, position, hash,
    ) {
        ActHashTraceDecision::Reuse(id) => {
            &&& id == encoded_start
            &&& match latest_trace_hashed_act_line(lines, inserted_hashes, hash) {
                Some(previous) => previous.suffix == suffix
                    && previous.encoded_start == id,
                None => false,
            }
        },
        _ => true,
    },
{
}

pub proof fn act_hash_trace_unequal_collision_cannot_reuse(
    lines: Seq<ActProducerLine>, inserted_hashes: Seq<u64>,
    suffix: Seq<u8>, encoded_start: usize, position: usize, hash: u64,
)
    requires
        lines.len() == inserted_hashes.len(),
        latest_trace_hashed_act_line(lines, inserted_hashes, hash).is_some(),
        latest_trace_hashed_act_line(lines, inserted_hashes, hash).unwrap().suffix != suffix,
    ensures match act_hash_trace_line_decision(
        lines, inserted_hashes, suffix, encoded_start, position, hash,
    ) {
        ActHashTraceDecision::Reuse(_) => false,
        ActHashTraceDecision::Append => encoded_start == position,
        ActHashTraceDecision::Reject => encoded_start != position,
    },
{
}

pub proof fn act_trace_scan_matches_observed_buckets(
    lines: Seq<ActProducerLine>, inserted_hashes: Seq<u64>, hash: u64,
)
    requires lines.len() == inserted_hashes.len(),
    ensures {
        let buckets = act_observed_hash_buckets(lines, inserted_hashes);
        match latest_trace_hashed_act_line(lines, inserted_hashes, hash) {
            Some(line) => buckets.contains_key(hash) && buckets[hash] == line,
            None => !buckets.contains_key(hash),
        }
    },
    decreases lines.len(),
{
    if lines.len() > 0 {
        let prior_lines = lines.drop_last();
        let prior_hashes = inserted_hashes.drop_last();
        act_trace_scan_matches_observed_buckets(prior_lines, prior_hashes, hash);
        let prior_buckets = act_observed_hash_buckets(prior_lines, prior_hashes);
        let buckets = act_observed_hash_buckets(lines, inserted_hashes);
        assert(buckets == prior_buckets.insert(inserted_hashes.last(), lines.last()));
        if inserted_hashes.last() == hash {
            assert(buckets.contains_key(hash));
            assert(buckets[hash] == lines.last());
        } else {
            assert(buckets.contains_key(hash) == prior_buckets.contains_key(hash));
            if prior_buckets.contains_key(hash) {
                assert(buckets[hash] == prior_buckets[hash]);
            }
        }
    }
}

pub open spec fn act_direct_children_encoded_len(
    bytes: Seq<u8>, records: Seq<ActRecordImage>,
    children: Seq<usize>, at: usize,
) -> usize
    decreases children.len() - at,
{
    if at >= children.len() || at == usize::MAX {
        0
    } else {
        let child = records[children[at as int] as int];
        let length = match act_semantic_at(bytes, child.node_id) {
            Some(semantic) => semantic.encoded_len,
            None => 0,
        };
        (length as int + act_direct_children_encoded_len(
            bytes, records, children, (at as int + 1) as usize,
        ) as int) as usize
    }
}

pub open spec fn act_producer_record_step(
    bytes: Seq<u8>, records: Seq<ActRecordImage>,
    index: usize, before: ActProducerState,
) -> Option<ActProducerState>
    recommends index < records.len(),
{
    let record = records[index as int];
    let child_position = if record.children.len() == 0 {
        Some(before.position)
    } else if records[record.children[0] as int].node_id != before.position {
        None
    } else {
        let length = act_direct_children_encoded_len(bytes, records, record.children, 0);
        if before.position as int + length as int > usize::MAX as int {
            None
        } else {
            Some((before.position as int + length as int) as usize)
        }
    };
    match child_position {
        Some(position) => {
            let line_state = if record.line_len == 0 {
                Some(ActProducerState {
                    position, lines: before.lines, line_hash: before.line_hash,
                })
            } else {
                let line_end = record.line_start as int + record.line_len as int;
                if line_end > bytes.len() || line_end > usize::MAX as int {
                    None
                } else {
                    let suffix = bytes.subrange(record.line_start as int, line_end);
                    let line = ActProducerLine {
                        encoded_start: record.line_encoded_start, suffix,
                    };
                    if record.line_encoded_start == position {
                        if match latest_hashed_act_line(
                            before.lines, suffix, before.line_hash,
                        ) {
                            Some(previous) => previous.suffix == suffix,
                            None => false,
                        } {
                            None
                        } else {
                            Some(ActProducerState {
                                position: line_end as usize,
                                lines: before.lines.push(line),
                                line_hash: before.line_hash,
                            })
                        }
                    } else {
                        match latest_hashed_act_line(
                            before.lines, suffix, before.line_hash,
                        ) {
                            Some(previous) if previous.suffix == suffix
                                && previous.encoded_start == record.line_encoded_start => {
                                Some(ActProducerState {
                                    position, lines: before.lines,
                                    line_hash: before.line_hash,
                                })
                            },
                            _ => None,
                        }
                    }
                }
            };
            match line_state {
                Some(state) => match record.wrapper_id {
                    Some(wrapper_id) if wrapper_id == state.position
                        && state.position as int + record.wrapper_len as int
                            <= usize::MAX as int => {
                        Some(ActProducerState {
                            position: (state.position as int + record.wrapper_len as int) as usize,
                            lines: state.lines,
                            line_hash: state.line_hash,
                        })
                    },
                    None => Some(state),
                    _ => None,
                },
                None => None,
            }
        },
        None => None,
    }
}

pub open spec fn act_hash_trace_record_step(
    bytes: Seq<u8>, records: Seq<ActRecordImage>,
    index: usize, hash: u64, before: ActHashTraceState,
) -> Option<ActHashTraceState>
    recommends index < records.len(),
{
    if before.lines.len() != before.inserted_hashes.len() {
        None
    } else {
        let record = records[index as int];
        let child_position = if record.children.len() == 0 {
            Some(before.position)
        } else if records[record.children[0] as int].node_id != before.position {
            None
        } else {
            let length = act_direct_children_encoded_len(bytes, records, record.children, 0);
            if before.position as int + length as int > usize::MAX as int {
                None
            } else {
                Some((before.position as int + length as int) as usize)
            }
        };
        match child_position {
            Some(position) => {
                let line_state = if record.line_len == 0 {
                    Some(ActHashTraceState {
                        position, lines: before.lines,
                        inserted_hashes: before.inserted_hashes,
                    })
                } else {
                    let line_end = record.line_start as int + record.line_len as int;
                    if line_end > bytes.len() || line_end > usize::MAX as int {
                        None
                    } else {
                        let suffix = bytes.subrange(record.line_start as int, line_end);
                        match act_hash_trace_line_decision(
                            before.lines, before.inserted_hashes, suffix,
                            record.line_encoded_start, position, hash,
                        ) {
                            ActHashTraceDecision::Append => Some(ActHashTraceState {
                                position: line_end as usize,
                                lines: before.lines.push(ActProducerLine {
                                    encoded_start: record.line_encoded_start, suffix,
                                }),
                                inserted_hashes: before.inserted_hashes.push(hash),
                            }),
                            ActHashTraceDecision::Reuse(id)
                                if id == record.line_encoded_start => {
                                Some(ActHashTraceState {
                                    position, lines: before.lines,
                                    inserted_hashes: before.inserted_hashes,
                                })
                            },
                            _ => None,
                        }
                    }
                };
                match line_state {
                    Some(state) => match record.wrapper_id {
                        Some(wrapper_id) if wrapper_id == state.position
                            && state.position as int + record.wrapper_len as int
                                <= usize::MAX as int => {
                            Some(ActHashTraceState {
                                position: (state.position as int
                                    + record.wrapper_len as int) as usize,
                                lines: state.lines,
                                inserted_hashes: state.inserted_hashes,
                            })
                        },
                        None => Some(state),
                        _ => None,
                    },
                    None => None,
                }
            },
            None => None,
        }
    }
}

pub proof fn act_hash_trace_record_step_preserves_alignment(
    bytes: Seq<u8>, records: Seq<ActRecordImage>,
    index: usize, hash: u64, before: ActHashTraceState,
)
    requires index < records.len(),
    ensures match act_hash_trace_record_step(bytes, records, index, hash, before) {
        Some(next) => next.lines.len() == next.inserted_hashes.len(),
        None => true,
    },
{
}

pub open spec fn act_hash_trace_replay(
    bytes: Seq<u8>, plan: ActPlanImage, hash_results: Seq<u64>,
    at: usize, state: ActHashTraceState,
) -> Option<ActHashTraceState>
    decreases plan.finish_order.len() - at,
{
    if hash_results.len() != plan.records.len() {
        None
    } else if at >= plan.finish_order.len() || at == usize::MAX {
        Some(state)
    } else {
        let index = plan.finish_order[at as int];
        if index >= plan.records.len() {
            None
        } else {
            match act_hash_trace_record_step(
                bytes, plan.records, index, hash_results[index as int], state,
            ) {
                Some(next) => act_hash_trace_replay(
                    bytes, plan, hash_results, (at as int + 1) as usize, next,
                ),
                None => None,
            }
        }
    }
}

pub open spec fn act_hash_trace_matches_frame(
    bytes: Seq<u8>, plan: ActPlanImage, hash_results: Seq<u64>,
) -> bool {
    if !act_finish_order_wf(plan)
        || !act_hash_trace_physical_consistent(plan, hash_results)
    {
        false
    } else {
        match act_hash_trace_replay(
            bytes, plan, hash_results, 0,
            ActHashTraceState {
                position: 16, lines: Seq::empty(), inserted_hashes: Seq::empty(),
            },
        ) {
            Some(final_state) => match act_semantic_at(bytes, plan.records[0].node_id) {
                Some(root) => {
                    &&& final_state.position == plan.records[0].node_id
                    &&& final_state.position as int + root.encoded_len as int
                        == bytes.len() - 8
                    &&& final_state.lines.len() == final_state.inserted_hashes.len()
                },
                None => false,
            },
            None => false,
        }
    }
}

pub open spec fn act_producer_replay(
    bytes: Seq<u8>, plan: ActPlanImage,
    at: usize, state: ActProducerState,
) -> Option<ActProducerState>
    decreases plan.finish_order.len() - at,
{
    if at >= plan.finish_order.len() || at == usize::MAX {
        Some(state)
    } else {
        match act_producer_record_step(
            bytes, plan.records, plan.finish_order[at as int], state,
        ) {
            Some(next) => act_producer_replay(
                bytes, plan, (at as int + 1) as usize, next,
            ),
            None => None,
        }
    }
}

pub open spec fn act_finish_order_wf(plan: ActPlanImage) -> bool {
    &&& plan.finish_order.len() == plan.records.len()
    &&& forall|i: int| 0 <= i < plan.finish_order.len()
        ==> plan.finish_order[i] < plan.records.len()
        &&& forall|i: int, j: int|
            #![trigger plan.finish_order[i], plan.finish_order[j]]
            0 <= i < j < plan.finish_order.len()
        ==> {
            let earlier = plan.finish_order[i];
            let later = plan.finish_order[j];
            &&& earlier != later
            &&& (plan.records[earlier as int].subtree_end
                < plan.records[later as int].subtree_end
                || (plan.records[earlier as int].subtree_end
                    == plan.records[later as int].subtree_end
                    && earlier > later))
        }
}

pub open spec fn act_producer_matches_frame(
    bytes: Seq<u8>, plan: ActPlanImage,
) -> bool {
    if !act_finish_order_wf(plan) {
        false
    } else {
        match act_producer_replay(
            bytes, plan, 0,
            ActProducerState {
                position: 16, lines: Seq::empty(), line_hash: plan.line_hash,
            },
        ) {
            Some(final_state) => match act_semantic_at(bytes, plan.records[0].node_id) {
                Some(root) => {
                    &&& final_state.position == plan.records[0].node_id
                    &&& final_state.position as int + root.encoded_len as int
                        == bytes.len() - 8
                },
                None => false,
            },
            None => false,
        }
    }
}

pub open spec fn act_plan_matches_frame(
    frame: EpmFrame, plan: ActPlanImage,
) -> bool {
    if frame.trie_start > frame.trie_end
        || frame.trie_end > frame.snapshot.bytes.len()
    {
        false
    } else {
        let bytes = frame.snapshot.bytes.subrange(
            frame.trie_start as int, frame.trie_end as int,
        );
        &&& act_magic_and_trailer(bytes)
        &&& plan.records.len() > 0
        &&& plan.records[0].node_id as int == little_endian_uint(bytes, 8, 8)
        &&& plan.records[0].subtree_end == plan.records.len()
        &&& (plan.records[0].value.is_some()
            || plan.records[0].line_len > 0
            || plan.records[0].children.len() > 0)
        &&& forall|i: int| 0 <= i < plan.records.len()
            ==> act_record_matches(bytes, plan.records, i)
        &&& forall|i: int, j: int|
            0 <= i < j < plan.records.len()
                ==> plan.records[i].node_id != plan.records[j].node_id
        &&& act_producer_matches_frame(bytes, plan)
    }
}

pub open spec fn act_value_count(
    records: Seq<ActRecordImage>, index: usize,
) -> usize
    decreases records.len() - index,
{
    if index >= records.len() || index == usize::MAX {
        0
    } else {
        let added: int = if records[index as int].value.is_some() { 1 } else { 0 };
        (added + act_value_count(records, (index as int + 1) as usize) as int) as usize
    }
}

pub open spec fn act_source_edge_byte(
    bytes: Seq<u8>, source_offset: usize, origin: EdgeOrigin,
) -> Option<u8> {
    if source_offset >= bytes.len() {
        None
    } else {
        match origin {
            EdgeOrigin::Direct => Some(bytes[source_offset as int]),
            EdgeOrigin::Dense { mask_start, bit } => {
                if bit >= 8 || source_offset < mask_start
                    || source_offset as int >= mask_start as int + 32
                    || bytes[source_offset as int] as int
                        % (power_of_two((bit as nat) + 1) as int)
                        < power_of_two(bit as nat) as int
                {
                    None
                } else {
                    Some(((source_offset as int - mask_start as int) * 8 + bit as int) as u8)
                }
            },
        }
    }
}

pub open spec fn act_record_projects_validated(
    bytes: Seq<u8>, record: ActRecordImage,
    validated: ValidatedTopologyRecordImage,
) -> bool {
    &&& validated.flags < 4
    &&& record.node_id == validated.node_id
    &&& record.value_source_offset == validated.value_source_offset
    &&& record.line_start == validated.line_start
    &&& record.line_len == validated.line_len
    &&& record.children.len() == validated.child_count as int
    &&& if validated.flags as int % 2 == 1 {
        record.value == Some(validated.value)
    } else {
        record.value.is_none() && validated.value == 0
    }
    &&& if validated.flags as int / 2 == 1 {
        match (record.incoming_source_offset, record.incoming_origin) {
            (Some(offset), Some(origin)) => {
                &&& validated.incoming_source_offset == offset
                &&& act_source_edge_byte(bytes, offset, origin)
                    == Some(validated.incoming_byte)
            },
            _ => false,
        }
    } else {
        &&& record.incoming_source_offset.is_none()
        &&& record.incoming_origin.is_none()
        &&& validated.incoming_source_offset == 0
        &&& validated.incoming_byte == 0
    }
}

pub open spec fn act_plan_projects_validated(
    frame: EpmFrame, plan: ActPlanImage,
    validated: ValidatedActPlanImage,
) -> bool {
    if frame.trie_start > frame.trie_end
        || frame.trie_end > frame.snapshot.bytes.len()
    {
        false
    } else {
        let bytes = frame.snapshot.bytes.subrange(
            frame.trie_start as int, frame.trie_end as int,
        );
        &&& !validated.empty
        &&& validated.records.len() == plan.records.len()
        &&& validated.value_count == act_value_count(plan.records, 0)
        &&& validated.first_nonzero.is_none()
        &&& forall|i: int| 0 <= i < plan.records.len()
            ==> act_record_projects_validated(bytes, plan.records[i], validated.records[i])
    }
}

pub enum ActWorkItem {
    Enter(usize),
    Emit(WireTopologyEvent),
}

pub open spec fn act_line_forward_work(
    record: ActRecordImage, index: usize,
) -> Seq<ActWorkItem>
    decreases record.line_len - index,
{
    if index >= record.line_len {
        Seq::empty()
    } else {
        let final_value = index as int + 1 == record.line_len as int;
        seq![
            ActWorkItem::Emit(WireTopologyEvent::Edge {
                source_offset: (record.line_start as int + index as int) as usize,
                origin: EdgeOrigin::Direct,
            }),
            ActWorkItem::Emit(WireTopologyEvent::Value {
                has_value: final_value && record.value.is_some(),
                value_body: None,
                source_offset: if final_value && record.value.is_some() {
                    record.value_source_offset
                } else {
                    (record.line_start as int + index as int) as usize
                },
            }),
        ] + act_line_forward_work(record, (index as int + 1) as usize)
    }
}

pub open spec fn act_line_leave_work(
    record: ActRecordImage, index: usize,
) -> Seq<ActWorkItem>
    decreases record.line_len - index,
{
    if index >= record.line_len {
        Seq::empty()
    } else {
        seq![ActWorkItem::Emit(WireTopologyEvent::Leave {
            source_offset: (record.line_start as int + record.line_len as int
                - 1 - index as int) as usize,
        })] + act_line_leave_work(record, (index as int + 1) as usize)
    }
}

pub open spec fn act_children_work(
    plan: ActPlanImage, record: ActRecordImage, child_number: usize,
) -> Seq<ActWorkItem>
    decreases record.children.len() - child_number,
{
    if child_number >= record.children.len() || child_number == usize::MAX {
        Seq::empty()
    } else {
        let child_index = record.children[child_number as int];
        if child_index >= plan.records.len() {
            Seq::empty()
        } else {
            let child = plan.records[child_index as int];
            match (child.incoming_source_offset, child.incoming_origin) {
                (Some(offset), Some(origin)) => seq![
                    ActWorkItem::Emit(WireTopologyEvent::Edge {
                        source_offset: offset, origin,
                    }),
                    ActWorkItem::Enter(child_index),
                ] + act_children_work(plan, record, (child_number as int + 1) as usize),
                _ => Seq::empty(),
            }
        }
    }
}

pub open spec fn act_record_work(
    plan: ActPlanImage, index: usize,
) -> Seq<ActWorkItem>
    recommends index < plan.records.len(),
{
    let record = plan.records[index as int];
    seq![ActWorkItem::Emit(WireTopologyEvent::Value {
        has_value: record.line_len == 0 && record.value.is_some(),
        value_body: None,
        source_offset: record.node_id,
    })]
        + act_line_forward_work(record, 0)
        + act_children_work(plan, record, 0)
        + act_line_leave_work(record, 0)
        + seq![ActWorkItem::Emit(WireTopologyEvent::Leave {
            source_offset: record.node_id,
        })]
}

pub open spec fn act_work_budget(plan: ActPlanImage, index: usize) -> nat
    decreases plan.records.len() - index,
{
    if index >= plan.records.len() || index == usize::MAX {
        0
    } else {
        (act_record_work(plan, index).len() + 1) as nat
            + act_work_budget(plan, (index as int + 1) as usize)
    }
}

pub open spec fn act_expand_work(
    plan: ActPlanImage, work: Seq<ActWorkItem>, fuel: nat,
) -> Option<Seq<WireTopologyEvent>>
    decreases fuel,
{
    if work.len() == 0 {
        Some(Seq::empty())
    } else if fuel == 0 {
        None
    } else {
        let rest = work.drop_first();
        match work[0] {
            ActWorkItem::Enter(index) => {
                if index >= plan.records.len() {
                    None
                } else {
                    act_expand_work(
                        plan, act_record_work(plan, index) + rest,
                        (fuel as int - 1) as nat,
                    )
                }
            },
            ActWorkItem::Emit(event) => {
                match act_expand_work(plan, rest, (fuel as int - 1) as nat) {
                    Some(events) => Some(seq![event] + events),
                    None => None,
                }
            },
        }
    }
}

pub struct AttachedActEvents {
    pub ghost events: Seq<WireTopologyEvent>,
    pub next_value_offset: usize,
    pub map_values_seen: usize,
}

pub open spec fn attach_act_value_bodies(
    bytes: Seq<u8>, mode: EpmMode,
    events: Seq<WireTopologyEvent>, value_offset: usize,
) -> Option<AttachedActEvents>
    decreases events.len(),
{
    if events.len() == 0 {
        Some(AttachedActEvents {
            events: Seq::empty(), next_value_offset: value_offset,
            map_values_seen: 0,
        })
    } else {
        let rest = events.drop_first();
        match events[0] {
            WireTopologyEvent::Value { has_value: true, source_offset, .. }
                if mode == EpmMode::Map => {
                match map_value_body_at(bytes, value_offset) {
                    Some(span) => match attach_act_value_bodies(bytes, mode, rest, span.end) {
                        Some(tail) if tail.map_values_seen < usize::MAX => Some(AttachedActEvents {
                            events: seq![WireTopologyEvent::Value {
                                has_value: true, value_body: Some(span), source_offset,
                            }] + tail.events,
                            next_value_offset: tail.next_value_offset,
                            map_values_seen: (tail.map_values_seen as int + 1) as usize,
                        }),
                        _ => None,
                    },
                    None => None,
                }
            },
            event => match attach_act_value_bodies(bytes, mode, rest, value_offset) {
                Some(tail) => Some(AttachedActEvents {
                    events: seq![event] + tail.events,
                    next_value_offset: tail.next_value_offset,
                    map_values_seen: tail.map_values_seen,
                }),
                None => None,
            },
        }
    }
}

pub open spec fn act_event_source_valid(
    bytes: Seq<u8>, event: WireTopologyEvent,
) -> bool {
    match event {
        WireTopologyEvent::Edge { source_offset, origin } => {
            act_source_edge_byte(bytes, source_offset, origin).is_some()
        },
        WireTopologyEvent::Leave { source_offset } => source_offset < bytes.len(),
        WireTopologyEvent::Value { source_offset, .. } => source_offset < bytes.len(),
    }
}

pub open spec fn act_plan_events(
    frame: EpmFrame, plan: ActPlanImage,
) -> Option<Seq<WireTopologyEvent>> {
    if !act_plan_matches_frame(frame, plan) {
        None
    } else {
        match act_expand_work(
            plan, seq![ActWorkItem::Enter(0)], act_work_budget(plan, 0) + 1,
        ) {
            Some(topology) => match attach_act_value_bodies(
                frame.snapshot.bytes, frame.mode, topology, frame.value_table_start,
            ) {
                Some(attached) if attached.next_value_offset == frame.snapshot.bytes.len()
                    && (frame.mode != EpmMode::Map
                        || attached.map_values_seen == frame.cursor.declared_value_count)
                    && forall|i: int| 0 <= i < attached.events.len()
                        ==> act_event_source_valid(
                            frame.snapshot.bytes.subrange(
                                frame.trie_start as int, frame.trie_end as int,
                            ),
                            attached.events[i],
                        ) => {
                    Some(attached.events)
                },
                _ => None,
            },
            None => None,
        }
    }
}

pub open spec fn little_endian_uint(
    bytes: Seq<u8>, at: usize, width: usize,
) -> int
    recommends at as int + width as int <= bytes.len(), width <= 8,
    decreases width,
{
    if width == 0 {
        0
    } else {
        bytes[at as int] as int
            + 256 * little_endian_uint(
                bytes, (at as int + 1) as usize, (width as int - 1) as usize,
            )
    }
}

pub open spec fn act_varint_at(
    bytes: Seq<u8>, at: usize,
) -> Option<CanonicalVarintImage> {
    if at >= bytes.len() {
        None
    } else {
        let first = bytes[at as int];
        if first <= 247 {
            Some(CanonicalVarintImage { value: first as u64, end: (at as int + 1) as usize })
        } else {
            let width = first as int - 247;
            let end = at as int + 1 + width;
            if width < 1 || width > 8 || end > bytes.len()
                || bytes[end - 1] == 0
            {
                None
            } else {
                let value = little_endian_uint(bytes, (at as int + 1) as usize, width as usize);
                if value <= 247 || value > u64::MAX as int {
                    None
                } else {
                    Some(CanonicalVarintImage { value: value as u64, end: end as usize })
                }
            }
        }
    }
}

pub open spec fn act_magic_and_trailer(bytes: Seq<u8>) -> bool {
    &&& bytes.len() >= 25
    &&& bytes[0] == 0x41 && bytes[1] == 0x43
    &&& bytes[2] == 0x54 && bytes[3] == 0x72
    &&& bytes[4] == 0x65 && bytes[5] == 0x65
    &&& bytes[6] == 0x30 && bytes[7] == 0x33
    &&& forall|i: int| bytes.len() - 8 <= i < bytes.len() ==> bytes[i] == 0
}

pub open spec fn act_single_leaf_from_bytes(bytes: Seq<u8>) -> Option<ActLeafImage> {
    if !act_magic_and_trailer(bytes) {
        None
    } else {
        let root = little_endian_uint(bytes, 8, 8);
        let trailer = bytes.len() - 8;
        if root < 16 || root >= trailer || root > usize::MAX as int {
            None
        } else if bytes[root] == 0x40 && root == 16 {
            match act_varint_at(bytes, (root + 1) as usize) {
                Some(value) if value.value == 0 && value.end == trailer => {
                    Some(ActLeafImage {
                        root_id: root as usize, line_start: 0,
                        line_len: 0, is_line: false,
                    })
                },
                _ => None,
            }
        } else if bytes[root] == 0xc0 {
            match act_varint_at(bytes, (root + 1) as usize) {
                Some(value) if value.value == 0 => {
                    match act_varint_at(bytes, value.end) {
                        Some(relative) if relative.end == trailer
                            && relative.value as int == root - 16 => {
                            match act_varint_at(bytes, 16) {
                                Some(length) if length.value > 0
                                    && length.end as int + length.value as int == root
                                    && 3 * length.value as int + 2 <= usize::MAX as int => {
                                    Some(ActLeafImage {
                                        root_id: root as usize,
                                        line_start: length.end,
                                        line_len: length.value as usize,
                                        is_line: true,
                                    })
                                },
                                _ => None,
                            }
                        },
                        _ => None,
                    }
                },
                _ => None,
            }
        } else {
            None
        }
    }
}

pub open spec fn act_leaf_from_frame(frame: EpmFrame) -> Option<ActLeafImage> {
    if frame.trie_start > frame.trie_end
        || frame.trie_end > frame.snapshot.bytes.len()
    {
        None
    } else {
        act_single_leaf_from_bytes(frame.snapshot.bytes.subrange(
            frame.trie_start as int, frame.trie_end as int,
        ))
    }
}

pub open spec fn cursor_event_count(frame: EpmFrame) -> usize {
    if frame.mode == EpmMode::Empty {
        2
    } else {
        match frame.cursor.act_plan {
            Some(plan) => match act_plan_events(frame, plan) {
                Some(events) => events.len() as usize,
                None => 0,
            },
            None => 0,
        }
    }
}

pub open spec fn cursor_value_body(frame: EpmFrame) -> Option<ByteSpan> {
    if frame.mode == EpmMode::Map {
        map_value_body_at(frame.snapshot.bytes, frame.value_table_start)
    } else {
        None
    }
}

pub open spec fn cursor_event_at(
    frame: EpmFrame, at: usize,
) -> Option<WireTopologyEvent>
    recommends at < cursor_event_count(frame),
{
    if frame.mode == EpmMode::Empty {
        if at == 0 {
            Some(WireTopologyEvent::Value {
                has_value: false, value_body: None, source_offset: frame.mode_offset,
            })
        } else {
            Some(WireTopologyEvent::Leave { source_offset: frame.mode_offset })
        }
    } else {
        match frame.cursor.act_plan {
            Some(plan) => match act_plan_events(frame, plan) {
                Some(events) if at < events.len() => Some(events[at as int]),
                _ => None,
            },
            None => None,
        }
    }
}

pub open spec fn singleton_cursor_event_at(
    frame: EpmFrame, at: usize,
) -> Option<WireTopologyEvent> {
    if frame.mode == EpmMode::Empty {
        None
    } else {
        match act_leaf_from_frame(frame) {
            Some(leaf) if !leaf.is_line => {
                if at == 0 {
                    Some(WireTopologyEvent::Value {
                        has_value: true, value_body: cursor_value_body(frame),
                        source_offset: leaf.root_id,
                    })
                } else {
                    Some(WireTopologyEvent::Leave { source_offset: leaf.root_id })
                }
            },
            Some(leaf) => {
                let line_len = leaf.line_len as int;
                let index = at as int;
                if at == 0 {
                    Some(WireTopologyEvent::Value {
                        has_value: false, value_body: None,
                        source_offset: leaf.root_id,
                    })
                } else if index < 1 + 2 * line_len {
                    let line_index = (index - 1) / 2;
                    if (index - 1) % 2 == 0 {
                        Some(WireTopologyEvent::Edge {
                            source_offset: (leaf.line_start as int + line_index) as usize,
                            origin: EdgeOrigin::Direct,
                        })
                    } else {
                        let final_value = line_index + 1 == line_len;
                        Some(WireTopologyEvent::Value {
                            has_value: final_value,
                            value_body: if final_value { cursor_value_body(frame) } else { None },
                            source_offset: if final_value { leaf.root_id } else {
                                (leaf.line_start as int + line_index) as usize
                            },
                        })
                    }
                } else if index < 1 + 3 * line_len {
                    Some(WireTopologyEvent::Leave {
                        source_offset: (leaf.line_start as int + line_len - 1
                            - (index - 1 - 2 * line_len)) as usize,
                    })
                } else {
                    Some(WireTopologyEvent::Leave { source_offset: leaf.root_id })
                }
            },
            None => None,
        }
    }
}

pub open spec fn edge_origin_valid(
    frame: EpmFrame, offset: usize, origin: EdgeOrigin,
) -> bool {
    &&& frame_layout_wf(frame)
    &&& offset < frame.trie_end - frame.trie_start
    &&& match origin {
        EdgeOrigin::Direct => true,
        EdgeOrigin::Dense { mask_start, bit } => {
            &&& mask_start <= offset
            &&& mask_start + 32 <= frame.trie_end - frame.trie_start
            &&& offset < mask_start + 32
            &&& bit < 8
            &&& trie_byte(frame, offset) as int % (power_of_two((bit as nat) + 1) as int)
                >= power_of_two(bit as nat) as int
        },
    }
}

pub open spec fn edge_byte(
    frame: EpmFrame, offset: usize, origin: EdgeOrigin,
) -> u8
    recommends edge_origin_valid(frame, offset, origin),
{
    match origin {
        EdgeOrigin::Direct => trie_byte(frame, offset),
        EdgeOrigin::Dense { mask_start, bit } => {
            (((offset - mask_start) as int * 8 + bit as int) as u8)
        },
    }
}

pub open spec fn topology_offset_valid(frame: EpmFrame, offset: usize) -> bool {
    if frame.mode == EpmMode::Empty {
        offset == frame.mode_offset
    } else {
        offset < frame.trie_end - frame.trie_start
    }
}

pub open spec fn topology_position(frame: EpmFrame, offset: usize) -> SourceAnchor
    recommends frame_layout_wf(frame), topology_offset_valid(frame, offset),
{
    if frame.mode == EpmMode::Empty {
        frame.snapshot.positions[frame.mode_offset as int]
    } else {
        trie_position(frame, offset)
    }
}

pub open spec fn same_frame_source(before: EpmFrame, after: EpmFrame) -> bool {
    &&& after.snapshot == before.snapshot
    &&& after.backing == before.backing
    &&& after.trie_start == before.trie_start
    &&& after.trie_end == before.trie_end
    &&& after.value_table_start == before.value_table_start
    &&& after.mode_offset == before.mode_offset
}

pub open spec fn wire_event(event: EpmEvent) -> Option<WireTopologyEvent> {
    match event {
        EpmEvent::Edge { source_offset, origin, .. } => {
            Some(WireTopologyEvent::Edge { source_offset, origin })
        },
        EpmEvent::Leave { source_offset } => {
            Some(WireTopologyEvent::Leave { source_offset })
        },
        EpmEvent::Value { has_value, value_body, source_offset } => {
            Some(WireTopologyEvent::Value { has_value, value_body, source_offset })
        },
        EpmEvent::End => None,
    }
}

pub open spec fn cursor_event_step(
    before: EpmFrame, after: EpmFrame, event: EpmEvent,
) -> bool {
    &&& frame_layout_wf(before)
    &&& frame_layout_wf(after)
    &&& !before.cursor.done
    &&& after.cursor.act_plan == before.cursor.act_plan
    &&& after.cursor.validated_plan == before.cursor.validated_plan
    &&& after.cursor.declared_value_count == before.cursor.declared_value_count
    &&& match wire_event(event) {
        Some(wire) => {
            &&& before.cursor.at < cursor_event_count(before)
            &&& cursor_event_at(before, before.cursor.at) == Some(wire)
            &&& after.cursor.at == before.cursor.at + 1
            &&& !after.cursor.done
            &&& match wire {
                WireTopologyEvent::Value { value_body: Some(span), .. } => {
                    &&& before.cursor.map_values_seen < usize::MAX
                    &&& after.cursor.map_values_seen == before.cursor.map_values_seen + 1
                    &&& map_value_body_at(
                        before.snapshot.bytes, before.cursor.value_table_offset,
                    ) == Some(span)
                    &&& after.cursor.value_table_offset == span.end
                },
                _ => {
                    &&& after.cursor.map_values_seen == before.cursor.map_values_seen
                    &&& after.cursor.value_table_offset == before.cursor.value_table_offset
                },
            }
        },
        None => {
            &&& before.cursor.at == cursor_event_count(before)
            &&& after.cursor.at == before.cursor.at
            &&& after.cursor.map_values_seen == before.cursor.map_values_seen
            &&& after.cursor.value_table_offset == before.cursor.value_table_offset
            &&& before.cursor.value_table_offset == before.snapshot.bytes.len()
            &&& (before.mode != EpmMode::Map
                || before.cursor.map_values_seen == before.cursor.declared_value_count)
            &&& after.cursor.done
        },
    }
}

pub open spec fn obligation_ready(path: PathTopology) -> bool {
    match path.pending {
        Some(obligation) => obligation.awaiting_resolution
            && obligation.payload_start <= path.path_bytes.len()
            && path.path_bytes.len() - obligation.payload_start == obligation.payload_len
            && path.path_bytes.len() == path.path_positions.len(),
        None => false,
    }
}

pub open spec fn request_for_obligation(
    path: PathTopology, return_id: u64,
) -> Option<ChildRequest> {
    match path.pending {
        Some(obligation) if obligation_ready(path) => match obligation.kind {
            PathObligationKind::RawParEscape => Some(ChildRequest::RawParEscape {
                id: obligation.id,
                return_id,
                body: indexed_from_source(SourceTreeImage {
                    bytes: path.path_bytes.subrange(
                        obligation.payload_start as int, path.path_bytes.len() as int,
                    ),
                    positions: path.path_positions.subrange(
                        obligation.payload_start as int, path.path_positions.len() as int,
                    ),
                }, obligation.payload_end),
            }),
            PathObligationKind::NestedEpm1 => Some(ChildRequest::NestedEpm1 {
                id: obligation.id,
                return_id,
                body: indexed_from_source(SourceTreeImage {
                    bytes: path.path_bytes.subrange(
                        obligation.payload_start as int, path.path_bytes.len() as int,
                    ),
                    positions: path.path_positions.subrange(
                        obligation.payload_start as int, path.path_positions.len() as int,
                    ),
                }, obligation.payload_end),
            }),
        },
        _ => None,
    }
}

pub open spec fn path_value_ok(path: PathTopology) -> bool {
    &&& !path.gate.dead
    &&& path.pending.is_none()
    &&& path.gate.stack_empty
    &&& (path.gate.split_complete
        || (path.gate.completed_segments == 1 && !path.gate.first_is_list))
}

pub open spec fn path_finish_ok(path: PathTopology) -> bool {
    &&& path.pending.is_none()
    &&& path.root_left
    &&& path.restore.len() == 0
}

pub open spec fn parser_gate_matches(gate: PathGate, image: parser::ParserImage) -> bool {
    let expected = parser::parser_root_gate(image);
    &&& gate.dead == expected.dead
    &&& gate.stack_empty == expected.stack_empty
    &&& gate.split_complete == expected.split_complete
    &&& gate.completed_segments as int == expected.completed_segments as int
    &&& gate.first_is_list == expected.first_is_list
    &&& gate.stable == expected.stable
    &&& gate.escape_allowed == expected.escape_allowed
}

pub open spec fn can_step(frame: EpmFrame) -> bool {
    frame.wait.is_none()
}

pub open spec fn obligation_tag_byte(kind: PathObligationKind) -> u8 {
    match kind {
        PathObligationKind::RawParEscape => 0x0f,
        PathObligationKind::NestedEpm1 => 0x10,
    }
}

pub open spec fn parser_kind_matches(
    kind: PathObligationKind, parser_kind: parser::ParserObligationKind,
) -> bool {
    match (kind, parser_kind) {
        (PathObligationKind::RawParEscape, parser::ParserObligationKind::RawParEscape)
        | (PathObligationKind::NestedEpm1, parser::ParserObligationKind::NestedEpm1) => true,
        _ => false,
    }
}

pub open spec fn start_obligation(
    before: PathTopology,
    after: PathTopology,
    kind: PathObligationKind,
    payload_len: usize,
    tag: SourceAnchor,
    byte: u8,
    end: SourceAnchor,
) -> bool {
    &&& !before.root_left
    &&& !before.gate.dead
    &&& before.pending.is_none()
    &&& before.next_obligation < u64::MAX
    &&& after.next_obligation == before.next_obligation + 1
    &&& match after.pending { Some(obligation) => {
        &&& obligation.id == before.next_obligation
        &&& obligation.kind == kind
        &&& obligation.payload_len == payload_len
        &&& obligation.tag_source.source == tag.source
        &&& obligation.tag_source.offset == tag.offset
        &&& obligation.payload_start == after.path_bytes.len()
        &&& obligation.payload_end.source == end.source
        &&& obligation.payload_end.offset == end.offset
        &&& obligation.awaiting_resolution == (payload_len == 0)
    }, None => false }
    &&& after.root_left == before.root_left
    &&& after.path_bytes == before.path_bytes.push(byte)
    &&& after.path_positions == before.path_positions.push(end)
    &&& after.restore == before.restore.push(PathCheckpoint {
        pending: before.pending,
        gate: before.gate,
        path_len: before.path_bytes.len() as usize,
        parser: before.parser,
    })
    &&& after.gate == before.gate
}

pub open spec fn consume_obligation_byte(
    before: PathTopology,
    after: PathTopology,
    byte: u8,
    end: SourceAnchor,
) -> bool {
    &&& match before.pending { Some(old) => {
        &&& !old.awaiting_resolution
        &&& old.payload_start <= before.path_bytes.len()
        &&& before.path_bytes.len() - old.payload_start < old.payload_len
        &&& match after.pending { Some(new) => {
            &&& new.id == old.id
            &&& new.kind == old.kind
            &&& new.payload_len == old.payload_len
            &&& new.tag_source == old.tag_source
            &&& new.payload_start == old.payload_start
            &&& new.payload_end == end
            &&& new.awaiting_resolution
                == (after.path_bytes.len() - old.payload_start == old.payload_len)
        }, None => false }
    }, None => false }
    &&& after.next_obligation == before.next_obligation
    &&& after.root_left == before.root_left
    &&& after.path_bytes == before.path_bytes.push(byte)
    &&& after.path_positions == before.path_positions.push(end)
    &&& after.restore == before.restore.push(PathCheckpoint {
        pending: before.pending,
        gate: before.gate,
        path_len: before.path_bytes.len() as usize,
        parser: before.parser,
    })
    &&& after.gate == before.gate
}

/// The path/restore part of a plain edge. The parser gate needs its own
/// transition: an exact append does not imply an unchanged gate.
pub open spec fn plain_edge_path_shape(
    before: PathTopology, after: PathTopology, byte: u8, source: SourceAnchor,
) -> bool {
    &&& after.next_obligation == before.next_obligation
    &&& after.pending == before.pending
    &&& after.root_left == before.root_left
    &&& after.path_bytes == before.path_bytes.push(byte)
    &&& after.path_positions == before.path_positions.push(source)
    &&& after.restore == before.restore.push(PathCheckpoint {
        pending: before.pending,
        gate: before.gate,
        path_len: before.path_bytes.len() as usize,
        parser: before.parser,
    })
}

/// Exact gate outcomes for two ordinary tags on a fresh root path. In
/// `CanonicalPathMachine::feed_tag`, 0x06 opens the byte-array length Varint
/// frame, whereas 0x02 completes a Boolean segment through `bubble`. This
/// source-shaped special case is checkable from the input byte and initial
/// parser state; it is not a general gate transition for later frame kinds.
pub open spec fn initial_plain_tag_after(
    byte: u8, source: SourceAnchor,
) -> PathTopology
    recommends byte == 0x06 || byte == 0x02,
{
    let before = initial_epm_path();
    PathTopology {
        restore: before.restore.push(PathCheckpoint {
            pending: before.pending,
            gate: before.gate,
            path_len: 0,
            parser: before.parser,
        }),
        path_bytes: before.path_bytes.push(byte),
        path_positions: before.path_positions.push(source),
        parser: parser::feed_parser_byte(before.parser, byte,
            parser::ParserSource { source: source.source, offset: source.offset }, 1).image,
        gate: if byte == 0x06 {
            PathGate { stack_empty: false, ..before.gate }
        } else {
            PathGate { completed_segments: 1, ..before.gate }
        },
        ..before
    }
}

/// Prefixes of one byte-array segment with a minimal, one-byte length. The
/// source machine is in Tag at the first byte, Varint at the second byte,
/// then Bytes until the final payload byte bubbles the completed segment.
/// The completed segment can have length zero. Longer varints and other
/// parser frames require a separate source-state projection.
pub open spec fn simple_byte_array_prefix(bytes: Seq<u8>) -> bool {
    &&& bytes.len() >= 1
    &&& bytes[0] == 0x06
    &&& (bytes.len() == 1 || {
        &&& bytes[1] < 0x80
        &&& bytes.len() <= 2 + bytes[1] as int
    })
}

pub open spec fn simple_byte_array_gate(bytes: Seq<u8>) -> PathGate
    recommends simple_byte_array_prefix(bytes),
{
    let complete = bytes.len() >= 2 && bytes.len() == 2 + bytes[1] as int;
    PathGate {
        dead: false,
        stack_empty: complete,
        split_complete: false,
        completed_segments: if complete { 1 } else { 0 },
        first_is_list: false,
        stable: true,
        escape_allowed: true,
    }
}

pub open spec fn append_plain_path_with_gate(
    before: PathTopology, byte: u8, source: SourceAnchor, gate: PathGate,
) -> PathTopology {
    PathTopology {
        restore: before.restore.push(PathCheckpoint {
            pending: before.pending,
            gate: before.gate,
            path_len: before.path_bytes.len() as usize,
            parser: before.parser,
        }),
        path_bytes: before.path_bytes.push(byte),
        path_positions: before.path_positions.push(source),
        gate,
        parser: parser::feed_parser_byte(before.parser, byte,
            parser::ParserSource { source: source.source, offset: source.offset },
            (before.path_bytes.len() + 1) as usize).image,
        ..before
    }
}

/// Every ordinary byte is selected by the deterministic source-shaped parser
/// transition, including nested Region focus, bubbling, malformed/dead paths,
/// and later successful Continue edges after a persistent fault.
pub open spec fn modeled_plain_gate_transition(
    before: PathTopology, after: PathTopology, byte: u8,
) -> bool {
    &&& before.path_bytes.len() < usize::MAX as int
    &&& after.path_positions.len() > 0
    &&& parser_gate_matches(before.gate, before.parser)
    &&& before.next_obligation == before.parser.next_obligation
    &&& {
        let source = after.path_positions.last();
        let step = parser::feed_parser_byte(before.parser, byte,
            parser::ParserSource { source: source.source, offset: source.offset },
            (before.path_bytes.len() + 1) as usize);
        &&& step.event == parser::ParserEvent::Continue
        &&& after.parser == step.image
        &&& after.next_obligation == after.parser.next_obligation
        &&& parser_gate_matches(after.gate, after.parser)
    }
}

/// Both witnesses satisfy the exact Plain path/restore shape but contradict
/// its gate-equality clause. Neither proof assumes a fabricated successor.
pub proof fn initial_plain_tags_refute_unchanged_gate()
    ensures {
        let before = initial_epm_path();
        let source = SourceAnchor { source: 1, offset: 7 };
        let array_after = initial_plain_tag_after(0x06, source);
        let bool_after = initial_plain_tag_after(0x02, source);
        &&& plain_edge_path_shape(before, array_after, 0x06, source)
        &&& plain_edge_path_shape(before, bool_after, 0x02, source)
        &&& before.gate.stack_empty
        &&& !array_after.gate.stack_empty
        &&& before.gate.completed_segments == 0
        &&& bool_after.gate.completed_segments == 1
        &&& array_after.gate != before.gate
        &&& bool_after.gate != before.gate
        &&& modeled_plain_gate_transition(before, array_after, 0x06)
        &&& modeled_plain_gate_transition(before, bool_after, 0x02)
    },
{
    let before = initial_epm_path();
    let source = SourceAnchor { source: 1, offset: 7 };
    let array_after = initial_plain_tag_after(0x06, source);
    let bool_after = initial_plain_tag_after(0x02, source);
    assert(plain_edge_path_shape(before, array_after, 0x06, source));
    assert(plain_edge_path_shape(before, bool_after, 0x02, source));
    assert(before.gate.stack_empty && !array_after.gate.stack_empty);
    assert(before.gate.completed_segments == 0 && bool_after.gate.completed_segments == 1);
    assert(array_after.gate != before.gate);
    assert(bool_after.gate != before.gate);
    assert(modeled_plain_gate_transition(before, array_after, 0x06));
    assert(modeled_plain_gate_transition(before, bool_after, 0x02));
}

/// A complete ordinary byte-array key crosses both parser-state changes:
/// Tag -> Varint -> Bytes -> no frame, with one completed path segment.
pub proof fn simple_byte_array_plain_gate_trace()
    ensures {
        let start = initial_epm_path();
        let s = SourceAnchor { source: 1, offset: 7 };
        let tag = initial_plain_tag_after(0x06, s);
        let length = append_plain_path_with_gate(tag, 0x02, s,
            simple_byte_array_gate(tag.path_bytes.push(0x02)));
        let first = append_plain_path_with_gate(length, 0xab, s,
            simple_byte_array_gate(length.path_bytes.push(0xab)));
        let last = append_plain_path_with_gate(first, 0xcd, s,
            simple_byte_array_gate(first.path_bytes.push(0xcd)));
        &&& plain_edge_path_shape(start, tag, 0x06, s)
        &&& plain_edge_path_shape(tag, length, 0x02, s)
        &&& plain_edge_path_shape(length, first, 0xab, s)
        &&& plain_edge_path_shape(first, last, 0xcd, s)
        &&& modeled_plain_gate_transition(start, tag, 0x06)
        &&& modeled_plain_gate_transition(tag, length, 0x02)
        &&& modeled_plain_gate_transition(length, first, 0xab)
        &&& modeled_plain_gate_transition(first, last, 0xcd)
        &&& !tag.gate.stack_empty
        &&& !length.gate.stack_empty
        &&& !first.gate.stack_empty
        &&& last.gate.stack_empty
        &&& last.gate.completed_segments == 1
    },
{
    let start = initial_epm_path();
    let s = SourceAnchor { source: 1, offset: 7 };
    let tag = initial_plain_tag_after(0x06, s);
    let length_bytes = tag.path_bytes.push(0x02);
    assert(simple_byte_array_prefix(length_bytes));
    parser::initial_single_byte_varint(0x02);
    let length = append_plain_path_with_gate(tag, 0x02, s,
        simple_byte_array_gate(length_bytes));
    let first_bytes = length.path_bytes.push(0xab);
    assert(simple_byte_array_prefix(first_bytes));
    let first = append_plain_path_with_gate(length, 0xab, s,
        simple_byte_array_gate(first_bytes));
    let last_bytes = first.path_bytes.push(0xcd);
    assert(simple_byte_array_prefix(last_bytes));
    let last = append_plain_path_with_gate(first, 0xcd, s,
        simple_byte_array_gate(last_bytes));
    assert(plain_edge_path_shape(start, tag, 0x06, s));
    assert(plain_edge_path_shape(tag, length, 0x02, s));
    assert(plain_edge_path_shape(length, first, 0xab, s));
    assert(plain_edge_path_shape(first, last, 0xcd, s));
    assert(parser_gate_matches(start.gate, start.parser));
    assert(parser_gate_matches(tag.gate, tag.parser));
    assert(parser_gate_matches(length.gate, length.parser));
    assert(parser_gate_matches(first.gate, first.parser));
    assert(parser_gate_matches(last.gate, last.parser));
    assert(start.next_obligation == start.parser.next_obligation);
    assert(tag.next_obligation == tag.parser.next_obligation);
    assert(length.next_obligation == length.parser.next_obligation);
    assert(first.next_obligation == first.parser.next_obligation);
    assert(last.next_obligation == last.parser.next_obligation);
    assert(modeled_plain_gate_transition(start, tag, 0x06));
    assert(modeled_plain_gate_transition(tag, length, 0x02));
    assert(modeled_plain_gate_transition(length, first, 0xab));
    assert(modeled_plain_gate_transition(first, last, 0xcd));
    assert(!tag.gate.stack_empty && !length.gate.stack_empty && !first.gate.stack_empty);
    assert(last.gate.stack_empty && last.gate.completed_segments == 1);
}

pub open spec fn edge_return(
    before: EpmFrame,
    after: EpmFrame,
    byte: u8,
    source_offset: usize,
    source: SourceAnchor,
    origin: EdgeOrigin,
    effect: PathEdgeEffect,
    admission_next_id: u64,
    action: EpmAction,
) -> bool {
    &&& can_step(before)
    &&& frame_layout_wf(before)
    &&& !before.path.root_left
    &&& after.mode == before.mode
    &&& after.entries_stable == before.entries_stable
    &&& after.last_source == source
    &&& same_frame_source(before, after)
    &&& source_offset < before.trie_end - before.trie_start
    &&& source == trie_position(before, source_offset)
    &&& edge_origin_valid(before, source_offset, origin)
    &&& byte == edge_byte(before, source_offset, origin)
    &&& before.path.path_bytes.len() < usize::MAX as int
    &&& before.path.next_obligation == before.path.parser.next_obligation
    &&& parser_gate_matches(before.path.gate, before.path.parser)
    &&& {
        let step = parser::feed_parser_byte(before.path.parser, byte,
            parser::ParserSource { source: source.source, offset: source.offset },
            (before.path.path_bytes.len() + 1) as usize);
        &&& after.path.parser == step.image
        &&& after.path.next_obligation == step.image.next_obligation
        &&& parser_gate_matches(after.path.gate, after.path.parser)
        &&& match (effect, step.event) {
            (PathEdgeEffect::Plain, parser::ParserEvent::Continue) => true,
            (
                PathEdgeEffect::Start { kind, payload_len, .. },
                parser::ParserEvent::ObligationStarted {
                    id, kind: parser_kind, payload_len: parser_len,
                },
            ) => id == before.path.next_obligation
                && parser_kind_matches(kind, parser_kind)
                && payload_len as int == parser_len as int,
            (PathEdgeEffect::Payload, parser::ParserEvent::ObligationByte { id, .. }) =>
                match before.path.pending { Some(obligation) => id == obligation.id, None => false },
            _ => false,
        }
    }
    &&& match effect {
        PathEdgeEffect::Plain => {
            &&& plain_edge_path_shape(before.path, after.path, byte, source)
            &&& modeled_plain_gate_transition(before.path, after.path, byte)
        },
        PathEdgeEffect::Start { kind, payload_len, tag } => {
            &&& exists|i: int| 0 <= i < before.path.path_bytes.len()
                && before.path.path_positions[i] == tag
                && before.path.path_bytes[i] == obligation_tag_byte(kind)
            &&& (kind != PathObligationKind::RawParEscape || before.path.gate.escape_allowed)
            &&& start_obligation(before.path, after.path, kind, payload_len, tag, byte, source)
        },
        PathEdgeEffect::Payload => {
            consume_obligation_byte(before.path, after.path, byte, source)
        },
    }
    &&& match request_for_obligation(after.path, admission_next_id) {
        Some(request) => match (after.wait, action, request) {
            (
                Some(EpmWait::Obligation {
                    id: waiting_id, kind: waiting_kind, return_id: waiting_return_id,
                }),
                EpmAction::Push(pushed),
                expected,
            ) => {
                &&& admission_next_id < u64::MAX
                &&& waiting_return_id == admission_next_id
                &&& match (pushed, expected) {
                    (
                        ChildRequest::RawParEscape { id: actual_id, return_id: actual_return_id, body: actual },
                        ChildRequest::RawParEscape { id: expected_id, return_id: expected_return_id, body: expected },
                    ) => actual_id == expected_id && actual_return_id == expected_return_id
                        && actual == expected,
                    (
                        ChildRequest::NestedEpm1 { id: actual_id, return_id: actual_return_id, body: actual },
                        ChildRequest::NestedEpm1 { id: expected_id, return_id: expected_return_id, body: expected },
                    ) => actual_id == expected_id && actual_return_id == expected_return_id
                        && actual == expected,
                    _ => false,
                }
                &&& match after.path.pending { Some(obligation) => {
                    waiting_id == obligation.id && waiting_kind == obligation.kind
                }, None => false }
            },
            _ => false,
        },
        None => after.wait.is_none() && action is Continue,
    }
}

pub open spec fn valid_child_resolution(kind: PathObligationKind, summary: ChildSummary) -> bool {
    match (kind, summary) {
        (PathObligationKind::RawParEscape, ChildSummary::Par { .. }) => true,
        (PathObligationKind::NestedEpm1, ChildSummary::Epm1 { .. }) => true,
        _ => false,
    }
}

pub open spec fn child_reducer_dead(kind: PathObligationKind, summary: ChildSummary) -> bool {
    match (kind, summary) {
        (PathObligationKind::RawParEscape, ChildSummary::Par { canonical_path_stable }) => {
            canonical_path_stable
        },
        (PathObligationKind::NestedEpm1, ChildSummary::Epm1 { mode, entries_stable }) => {
            mode != EpmMode::Map || !entries_stable
        },
        _ => true,
    }
}

pub open spec fn parser_resolution(summary: ChildSummary) -> parser::ParserResolution {
    match summary {
        ChildSummary::Par { canonical_path_stable } =>
            parser::ParserResolution::RawPar { canonical_path_stable },
        ChildSummary::Epm1 { mode, entries_stable } =>
            parser::ParserResolution::NestedEpm1 {
                is_map: mode == EpmMode::Map, entries_stable,
            },
        ChildSummary::Other => parser::ParserResolution::RawPar {
            canonical_path_stable: true,
        },
    }
}

pub open spec fn accept_child(
    before: EpmFrame,
    after: EpmFrame,
    return_id: Option<u64>,
    summary: ChildSummary,
) -> bool {
    &&& after.mode == before.mode
    &&& after.last_source == before.last_source
    &&& same_frame_source(before, after)
    &&& after.cursor == before.cursor
    &&& after.wait.is_none()
    &&& match before.wait {
        Some(EpmWait::MapValue { return_id: expected_id }) => match (return_id, summary) {
            (Some(actual_id), ChildSummary::Par { canonical_path_stable }) if actual_id == expected_id => {
                &&& after.path == before.path
                &&& after.entries_stable == (before.entries_stable && canonical_path_stable)
            },
            _ => false,
        },
        Some(EpmWait::Obligation { id, kind, return_id: expected_id }) => {
            &&& return_id == Some(expected_id)
            &&& valid_child_resolution(kind, summary)
            &&& match before.path.pending { Some(obligation) => {
                &&& obligation.id == id
                &&& obligation.kind == kind
                &&& obligation_ready(before.path)
            }, None => false }
            &&& after.path.pending.is_none()
            &&& after.path.next_obligation == before.path.next_obligation
            &&& after.path.root_left == before.path.root_left
            &&& after.path.restore == before.path.restore
            &&& after.path.path_bytes == before.path.path_bytes
            &&& after.path.path_positions == before.path.path_positions
            &&& parser_gate_matches(before.path.gate, before.path.parser)
            &&& after.path.parser == parser::resolve_parser_obligation(
                before.path.parser, id, parser_resolution(summary))
            &&& parser_gate_matches(after.path.gate, after.path.parser)
            &&& (before.path.gate.dead || child_reducer_dead(kind, summary))
                ==> after.path.gate.dead
            &&& match (kind, summary) {
                (
                    PathObligationKind::RawParEscape,
                    ChildSummary::Par { canonical_path_stable: false },
                ) => !after.path.gate.stable,
                _ => true,
            }
            &&& after.entries_stable == before.entries_stable
        },
        None => false,
    }
}

pub open spec fn leave_return(before: EpmFrame, after: EpmFrame, source: SourceAnchor) -> bool {
    &&& can_step(before)
    &&& !before.path.root_left
    &&& parser_gate_matches(before.path.gate, before.path.parser)
    &&& after.wait.is_none()
    &&& after.mode == before.mode
    &&& after.entries_stable == before.entries_stable
    &&& after.last_source == source
    &&& same_frame_source(before, after)
    &&& after.path.next_obligation == before.path.next_obligation
    &&& after.path.next_obligation == after.path.parser.next_obligation
    &&& parser_gate_matches(after.path.gate, after.path.parser)
    &&& if before.path.restore.len() > 0 {
        &&& after.path.restore == before.path.restore.drop_last()
        &&& after.path.pending == before.path.restore.last().pending
        &&& after.path.gate == before.path.restore.last().gate
        &&& after.path.parser == parser::restore_parser_checkpoint(
            before.path.restore.last().parser, before.path.parser)
        &&& after.path.path_bytes == before.path.path_bytes.subrange(
            0, before.path.restore.last().path_len as int,
        )
        &&& after.path.path_positions == before.path.path_positions.subrange(
            0, before.path.restore.last().path_len as int,
        )
        &&& !after.path.root_left
    } else {
        &&& after.path.restore == before.path.restore
        &&& after.path.pending == before.path.pending
        &&& after.path.gate == before.path.gate
        &&& after.path.parser == before.path.parser
        &&& after.path.path_bytes == before.path.path_bytes
        &&& after.path.path_positions == before.path.path_positions
        &&& after.path.root_left
    }
}

pub open spec fn value_return(
    before: EpmFrame,
    after: EpmFrame,
    has_value: bool,
    value_body: Option<ByteSpan>,
    source: SourceAnchor,
    admission_next_id: u64,
    action: EpmAction,
) -> bool {
    &&& can_step(before)
    &&& (!has_value || path_value_ok(before.path))
    &&& after.path == before.path
    &&& after.mode == before.mode
    &&& after.last_source == source
    &&& same_frame_source(before, after)
    &&& (value_body.is_some() == (before.mode == EpmMode::Map && has_value))
    &&& after.entries_stable == (before.entries_stable && (!has_value || before.path.gate.stable))
    &&& match value_body {
        Some(span) => {
            &&& admission_next_id < u64::MAX
            &&& frame_layout_wf(before)
            &&& before.value_table_start <= span.start
            &&& span.start <= span.end
            &&& span.end <= before.snapshot.bytes.len()
            &&& match (after.wait, action) {
            (
                Some(EpmWait::MapValue { return_id: waiting_id }),
                EpmAction::Push(ChildRequest::ParValue {
                    return_id: child_id, body,
                }),
            ) => waiting_id == admission_next_id && child_id == admission_next_id
                && body == indexed_subview(before.snapshot, span),
            _ => false,
            }
        },
        None => after.wait.is_none() && action is Continue,
    }
}

pub open spec fn end_return(frame: EpmFrame, action: EpmAction) -> bool {
    &&& can_step(frame)
    &&& path_finish_ok(frame.path)
    &&& match action {
        EpmAction::Complete { mode, entries_stable } => {
            mode == frame.mode && entries_stable == frame.entries_stable
        },
        _ => false,
    }
}

pub open spec fn event_step(
    before: EpmFrame,
    after: EpmFrame,
    event: EpmEvent,
    admission_next_id: u64,
    action: EpmAction,
) -> bool {
    &&& cursor_event_step(before, after, event)
    &&& match event {
        EpmEvent::Edge { source_offset, origin, effect } => {
            &&& frame_layout_wf(before)
            &&& edge_origin_valid(before, source_offset, origin)
            &&& edge_return(
                before, after, edge_byte(before, source_offset, origin), source_offset,
                trie_position(before, source_offset), origin, effect, admission_next_id, action,
            )
        },
        EpmEvent::Leave { source_offset } => {
            &&& frame_layout_wf(before)
            &&& topology_offset_valid(before, source_offset)
            &&& leave_return(before, after, topology_position(before, source_offset))
            &&& action is Continue
        },
        EpmEvent::Value { has_value, value_body, source_offset } => {
            &&& frame_layout_wf(before)
            &&& topology_offset_valid(before, source_offset)
            &&& value_return(
                before, after, has_value, value_body,
                topology_position(before, source_offset), admission_next_id, action,
            )
        },
        EpmEvent::End => {
            &&& after.path == before.path
            &&& after.wait == before.wait
            &&& after.mode == before.mode
            &&& after.entries_stable == before.entries_stable
            &&& after.last_source == before.last_source
            &&& same_frame_source(before, after)
            &&& end_return(before, action)
        },
    }
}

pub proof fn end_requires_no_outstanding(frame: EpmFrame, action: EpmAction)
    requires end_return(frame, action)
    ensures frame.wait.is_none() && frame.path.pending.is_none()
        && frame.path.root_left && frame.path.restore.len() == 0
{}

pub proof fn wrong_global_return_id_cannot_resolve(
    before: EpmFrame,
    after: EpmFrame,
    expected_id: u64,
    wrong_id: u64,
    summary: ChildSummary,
)
    requires match before.wait {
            Some(EpmWait::Obligation { return_id, .. }) => return_id == expected_id,
            _ => false,
        },
        wrong_id != expected_id,
    ensures !accept_child(before, after, Some(wrong_id), summary)
{}

pub proof fn wrong_local_obligation_id_cannot_resolve(
    before: EpmFrame,
    after: EpmFrame,
    expected_id: u64,
    wrong_id: u64,
    return_id: u64,
    summary: ChildSummary,
)
    requires match before.wait {
            Some(EpmWait::Obligation { id, .. }) => id == expected_id,
            _ => false,
        },
        match before.path.pending {
            Some(obligation) => obligation.id == wrong_id,
            None => false,
        },
        wrong_id != expected_id,
    ensures !accept_child(before, after, Some(return_id), summary)
{}

pub struct LinkedEpmState {
    pub epm: EpmFrame,
    pub admission: admission::AdmissionState,
    pub ghost indexed_regions: Seq<IndexedRegionImage>,
}

pub open spec fn initial_epm_path() -> PathTopology {
    PathTopology {
        next_obligation: 0,
        pending: None,
        root_left: false,
        restore: Seq::empty(),
        path_bytes: Seq::empty(),
        path_positions: Seq::empty(),
        parser: parser::initial_parser(),
        gate: PathGate {
            dead: false,
            stack_empty: true,
            split_complete: false,
            completed_segments: 0,
            first_is_list: false,
            stable: true,
            escape_allowed: true,
        },
    }
}

pub open spec fn initial_epm_candidate(
    backing: IndexedBackingImage,
    snapshot: IndexedRegionImage,
    prefix: EpmPrefixImage,
    plan: Option<ActPlanImage>,
    validated: Option<ValidatedActPlanImage>,
) -> EpmFrame
    recommends indexed_image_wf(snapshot),
{
    EpmFrame {
        path: initial_epm_path(),
        cursor: EpmCursorImage {
            act_plan: plan,
            validated_plan: validated,
            at: 0,
            declared_value_count: prefix.value_count,
            map_values_seen: 0,
            value_table_offset: prefix.value_table_start,
            done: false,
        },
        wait: None,
        mode: prefix.mode,
        entries_stable: true,
        last_source: indexed_position_at(snapshot, 0),
        snapshot,
        backing,
        trie_start: prefix.trie_start,
        trie_end: prefix.trie_end,
        value_table_start: prefix.value_table_start,
        mode_offset: 5,
    }
}

pub open spec fn source_epm_initial_frame(
    top: admission::AdmissionFrameState,
    backing: IndexedBackingImage,
    plan: Option<ActPlanImage>,
    validated: Option<ValidatedActPlanImage>,
) -> Option<EpmFrame> {
    match indexed_backing_project(backing) {
        Some(snapshot) if indexed_image_wf(snapshot)
            && admission::admission_frame_wf(top)
            && top.frame_kind == 3
            && top.region_start == 0
            && top.region_end == snapshot.bytes.len()
            && top.source_bytes == snapshot.bytes
            && top.cursor == 0
            && top.waiting_id.is_none()
            && top.score_handle.is_none() => {
            match epm_prefix_from_bytes(snapshot.bytes) {
                Some(prefix) if prefix.mode == EpmMode::Empty
                    && plan.is_none() && validated.is_none() => {
                    let frame = initial_epm_candidate(
                        backing, snapshot, prefix, None, None,
                    );
                    if frame_layout_wf(frame) { Some(frame) } else { None }
                },
                Some(prefix) if prefix.mode != EpmMode::Empty
                    && plan.is_some() && validated.is_some() => {
                    let frame = initial_epm_candidate(
                        backing, snapshot, prefix, plan, validated,
                    );
                    if frame_layout_wf(frame) { Some(frame) } else { None }
                },
                _ => None,
            }
        },
        _ => None,
    }
}

pub open spec fn epm_handoff_parent_source(
    state: admission::AdmissionState,
    indexed_regions: Seq<IndexedRegionImage>,
    snapshot: IndexedRegionImage,
) -> bool {
    if state.frames.len() < 2 || indexed_regions.len() != state.frames.len() {
        true
    } else {
        let parent = state.frames[state.frames.len() - 2];
        let child = state.frames.last();
        if parent.frame_kind != 1 && parent.frame_kind != 2 {
            true
        } else {
            let parent_source = indexed_regions[indexed_regions.len() - 2];
            let start = child.physical_start as int - parent.physical_start as int;
            &&& indexed_image_wf(parent_source)
            &&& start >= 0
            &&& start + child.region_end as int <= parent_source.bytes.len()
            &&& snapshot == indexed_subview(parent_source, ByteSpan {
                start: start as usize,
                end: (start + child.region_end as int) as usize,
            })
        }
    }
}

pub open spec fn linked_map_epm_handoff(
    admission_state: admission::AdmissionState,
    indexed_regions: Seq<IndexedRegionImage>,
    backing: IndexedBackingImage,
    plan: Option<ActPlanImage>,
    validated: Option<ValidatedActPlanImage>,
) -> Option<LinkedEpmState> {
    if !admission::admission_state_wf(admission_state)
        || admission_state.frames.len() == 0
        || indexed_regions.len() != admission_state.frames.len()
    {
        None
    } else {
        match source_epm_initial_frame(
            admission_state.frames.last(), backing, plan, validated,
        ) {
            Some(epm) if indexed_regions.last() == epm.snapshot
                && epm_handoff_parent_source(
                    admission_state, indexed_regions, epm.snapshot,
                ) => {
                let linked = LinkedEpmState {
                    epm, admission: admission_state, indexed_regions,
                };
                if linked_indexed_sources(linked) { Some(linked) } else { None }
            },
            _ => None,
        }
    }
}

pub open spec fn request_return_id(request: ChildRequest) -> u64 {
    match request {
        ChildRequest::ParValue { return_id, .. }
        | ChildRequest::RawParEscape { return_id, .. }
        | ChildRequest::NestedEpm1 { return_id, .. } => return_id,
    }
}

pub open spec fn request_frame_kind(request: ChildRequest) -> u8 {
    match request {
        ChildRequest::NestedEpm1 { .. } => 3,
        _ => 1,
    }
}

pub open spec fn request_body(request: ChildRequest) -> IndexedRegionImage {
    match request {
        ChildRequest::ParValue { body, .. }
        | ChildRequest::RawParEscape { body, .. }
        | ChildRequest::NestedEpm1 { body, .. } => body,
    }
}

pub open spec fn linked_indexed_sources(state: LinkedEpmState) -> bool {
    &&& state.indexed_regions.len() == state.admission.frames.len()
    &&& forall|i: int| 0 <= i < state.admission.frames.len()
        ==> indexed_image_wf(state.indexed_regions[i])
            && state.indexed_regions[i].bytes == state.admission.frames[i].source_bytes
            && indexed_position_at(state.indexed_regions[i], 0).source
                == state.admission.frames[i].physical_source
            && indexed_position_at(state.indexed_regions[i], 0).offset
                == state.admission.frames[i].physical_start
}

/// EPM1 validates its prefix and ACT region when the indexed cursor is made.
/// The remaining byte cursor is the checked end of the value-table prefix,
/// including each length-delimited map value already consumed. Event order is
/// tracked separately by `EpmCursorImage::at`; ACT events need not visit source
/// offsets in byte order.
pub open spec fn epm_advanced_admission_cursor(
    frame: admission::AdmissionFrameState, consumed: usize,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: frame.physical_source,
        physical_start: frame.physical_start,
        program_type_id: frame.program_type_id,
        region_start: frame.region_start,
        region_end: frame.region_end,
        source_bytes: frame.source_bytes,
        cursor: consumed,
        slot_count: frame.slot_count,
        last_slot: frame.last_slot,
        seen: frame.seen,
        required: frame.required,
        scalar_slots: frame.scalar_slots,
        scalar_occurrences: frame.scalar_occurrences,
        returns: frame.returns,
        family: frame.family,
        waiting_id: frame.waiting_id,
        waiting_kind: frame.waiting_kind,
        waiting_slot: frame.waiting_slot,
        waiting_tag: frame.waiting_tag,
        return_id: frame.return_id,
        frame_kind: frame.frame_kind,
        body_id: frame.body_id,
        score_handle: frame.score_handle,
        nested_started: frame.nested_started,
        nested_resolved: frame.nested_resolved,
    }
}

pub open spec fn linked_epm_continue(
    before: LinkedEpmState, after: LinkedEpmState, event: EpmEvent,
) -> bool {
    &&& admission::admission_state_wf(before.admission)
    &&& linked_indexed_sources(before)
    &&& before.admission.frames.len() > 0
    &&& before.admission.frames.last().frame_kind == 3
    &&& before.admission.frames.last().waiting_id.is_none()
    &&& before.epm.snapshot == before.indexed_regions.last()
    &&& before.admission.frames.last().cursor
        <= before.epm.cursor.value_table_offset
    &&& event_step(
        before.epm, after.epm, event,
        before.admission.next_obligation_id, EpmAction::Continue,
    )
    &&& after.admission == admission::AdmissionState {
        frames: before.admission.frames.update(
            before.admission.frames.len() - 1,
            epm_advanced_admission_cursor(
                before.admission.frames.last(), after.epm.cursor.value_table_offset,
            ),
        ),
        arena: before.admission.arena,
        root_end: before.admission.root_end,
        pushed: before.admission.pushed,
        completed: before.admission.completed,
        next_obligation_id: before.admission.next_obligation_id,
    }
    &&& after.indexed_regions == before.indexed_regions
    &&& admission::admission_state_wf(after.admission)
    &&& linked_indexed_sources(after)
}

pub proof fn linked_epm_continue_preserves_source_and_progress(
    before: LinkedEpmState, after: LinkedEpmState, event: EpmEvent,
)
    requires linked_epm_continue(before, after, event),
    ensures
        after.admission.arena == before.admission.arena,
        after.indexed_regions == before.indexed_regions,
        after.admission.frames.len() == before.admission.frames.len(),
        after.admission.frames.last().source_bytes
            == before.admission.frames.last().source_bytes,
        before.admission.frames.last().cursor
            <= after.admission.frames.last().cursor,
        after.admission.frames.last().cursor
            == after.epm.cursor.value_table_offset,
        after.admission.frames.last().cursor
            <= after.admission.frames.last().region_end,
{}

pub proof fn linked_epm_push_preserves_parent_progress(
    before: LinkedEpmState, after: LinkedEpmState,
    event: EpmEvent, action: EpmAction,
    child: admission::AdmissionFrameState,
)
    requires linked_epm_push(before, after, event, action, child),
    ensures
        after.admission.arena == before.admission.arena,
        after.admission.frames.len() == before.admission.frames.len() + 1,
        after.admission.frames[after.admission.frames.len() - 2].cursor
            == after.epm.cursor.value_table_offset,
        before.admission.frames.last().cursor
            <= after.admission.frames[after.admission.frames.len() - 2].cursor,
{}

pub open spec fn epm_parent_waiting(
    parent: admission::AdmissionFrameState, return_id: u64, kind: u8,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: parent.physical_source,
        physical_start: parent.physical_start,
        program_type_id: parent.program_type_id,
        region_start: parent.region_start,
        region_end: parent.region_end,
        source_bytes: parent.source_bytes,
        cursor: parent.cursor,
        slot_count: parent.slot_count,
        last_slot: parent.last_slot,
        seen: parent.seen,
        required: parent.required,
        scalar_slots: parent.scalar_slots,
        scalar_occurrences: parent.scalar_occurrences,
        returns: parent.returns,
        family: parent.family,
        waiting_id: Some(return_id),
        waiting_kind: kind,
        waiting_slot: None,
        waiting_tag: None,
        return_id: parent.return_id,
        frame_kind: parent.frame_kind,
        body_id: parent.body_id,
        score_handle: parent.score_handle,
        nested_started: if kind == 3 {
            (parent.nested_started as int + 1) as u64
        } else {
            parent.nested_started
        },
        nested_resolved: parent.nested_resolved,
    }
}

pub open spec fn epm_parent_resolved(
    parent: admission::AdmissionFrameState,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: parent.physical_source,
        physical_start: parent.physical_start,
        program_type_id: parent.program_type_id,
        region_start: parent.region_start,
        region_end: parent.region_end,
        source_bytes: parent.source_bytes,
        cursor: parent.cursor,
        slot_count: parent.slot_count,
        last_slot: parent.last_slot,
        seen: parent.seen,
        required: parent.required,
        scalar_slots: parent.scalar_slots,
        scalar_occurrences: parent.scalar_occurrences,
        returns: parent.returns,
        family: parent.family,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: parent.return_id,
        frame_kind: parent.frame_kind,
        body_id: parent.body_id,
        score_handle: parent.score_handle,
        nested_started: parent.nested_started,
        nested_resolved: if parent.waiting_kind == 3 {
            (parent.nested_resolved as int + 1) as u64
        } else {
            parent.nested_resolved
        },
    }
}

pub open spec fn epm_assigned_child(
    child: admission::AdmissionFrameState, return_id: u64,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: child.physical_source,
        physical_start: child.physical_start,
        program_type_id: child.program_type_id,
        region_start: child.region_start,
        region_end: child.region_end,
        source_bytes: child.source_bytes,
        cursor: child.cursor,
        slot_count: child.slot_count,
        last_slot: child.last_slot,
        seen: child.seen,
        required: child.required,
        scalar_slots: child.scalar_slots,
        scalar_occurrences: child.scalar_occurrences,
        returns: child.returns,
        family: child.family,
        waiting_id: child.waiting_id,
        waiting_kind: child.waiting_kind,
        waiting_slot: child.waiting_slot,
        waiting_tag: child.waiting_tag,
        return_id: Some(return_id),
        frame_kind: child.frame_kind,
        body_id: (return_id as int + 1) as u64,
        score_handle: child.score_handle,
        nested_started: child.nested_started,
        nested_resolved: child.nested_resolved,
    }
}

pub open spec fn linked_epm_push(
    before: LinkedEpmState,
    after: LinkedEpmState,
    event: EpmEvent,
    action: EpmAction,
    child: admission::AdmissionFrameState,
) -> bool {
    &&& admission::admission_state_wf(before.admission)
    &&& linked_indexed_sources(before)
    &&& before.admission.frames.len() > 0
    &&& before.admission.pushed < usize::MAX
    &&& before.admission.next_obligation_id < u64::MAX
    &&& before.admission.frames.last().frame_kind == 3
    &&& before.admission.frames.last().waiting_id.is_none()
    &&& before.admission.frames.last().cursor
        <= before.epm.cursor.value_table_offset
    &&& before.epm.snapshot == before.indexed_regions.last()
    &&& event_step(
        before.epm, after.epm, event, before.admission.next_obligation_id, action,
    )
    &&& match action {
        EpmAction::Push(request) => {
            let body = request_body(request);
            let kind = request_frame_kind(request);
            let parent = before.admission.frames.last();
            &&& request_return_id(request) == before.admission.next_obligation_id
            &&& (kind != 3 || parent.nested_started < u64::MAX)
            &&& child.frame_kind == kind
            &&& child.return_id.is_none()
            &&& child.waiting_id.is_none()
            &&& child.last_slot.is_none()
            &&& child.seen == Set::empty()
            &&& child.scalar_slots == Set::empty()
            &&& child.scalar_occurrences.len() == 0
            &&& child.returns.len() == 0
            &&& child.score_handle.is_none()
            &&& child.nested_started == 0
            &&& child.nested_resolved == 0
            &&& child.source_bytes == body.bytes
            &&& child.region_start == 0
            &&& child.cursor == 0
            &&& child.region_end == body.bytes.len()
            &&& (kind != 1 || child.program_type_id == 1)
            &&& admission::admission_frame_wf(child)
            &&& after.admission == admission::AdmissionState {
                frames: before.admission.frames.update(
                    before.admission.frames.len() - 1,
                    epm_parent_waiting(
                        epm_advanced_admission_cursor(
                            parent, after.epm.cursor.value_table_offset,
                        ),
                        request_return_id(request), kind,
                    ),
                ).push(epm_assigned_child(child, request_return_id(request))),
                arena: before.admission.arena,
                root_end: before.admission.root_end,
                pushed: (before.admission.pushed as int + 1) as usize,
                completed: before.admission.completed,
                next_obligation_id: (before.admission.next_obligation_id as int + 1) as u64,
            }
            &&& after.indexed_regions == before.indexed_regions.push(body)
            &&& admission::admission_state_wf(after.admission)
            &&& linked_indexed_sources(after)
        },
        _ => false,
    }
}

pub open spec fn linked_epm_child_return(
    before: LinkedEpmState,
    after: LinkedEpmState,
    summary: ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_before: Option<EpmFrame>,
    nested_after: Option<EpmFrame>,
) -> bool {
    &&& admission::admission_state_wf(before.admission)
    &&& linked_indexed_sources(before)
    &&& before.admission.frames.len() > 1
    &&& before.admission.completed < usize::MAX
    &&& before.epm.snapshot == before.indexed_regions[before.indexed_regions.len() - 2]
    &&& before.admission.frames[before.admission.frames.len() - 2].frame_kind == 3
    &&& before.admission.frames.last().cursor == before.admission.frames.last().region_end
    &&& before.admission.frames.last().waiting_id.is_none()
    &&& before.admission.frames.last().nested_started
        == before.admission.frames.last().nested_resolved
    &&& admission::admission_frame_wf(before.admission.frames.last())
    &&& admission::admission_family_admissible(before.admission.frames.last())
    &&& admission::admission_child_summary_agrees(
        before.admission.frames.last(), admission_summary, before.admission.arena,
    )
    &&& match (summary, admission_summary, nested_before, nested_after) {
        (
            ChildSummary::Par { .. },
            admission::AdmissionReturnedKind::Par { .. },
            None,
            None,
        ) => before.admission.frames.last().frame_kind == 1,
        (
            ChildSummary::Epm1 { mode, entries_stable },
            admission::AdmissionReturnedKind::Other,
            Some(child_before),
            Some(child_after),
        ) => {
            &&& before.admission.frames.last().frame_kind == 3
            &&& child_before.snapshot == before.indexed_regions.last()
            &&& event_step(
                child_before, child_after, EpmEvent::End,
                before.admission.next_obligation_id,
                EpmAction::Complete { mode, entries_stable },
            )
            &&& child_after.cursor.done
        },
        _ => false,
    }
    &&& match before.admission.frames.last().return_id {
        Some(return_id) => {
            &&& before.admission.frames[before.admission.frames.len() - 2].waiting_id
                == Some(return_id)
            &&& before.admission.frames[before.admission.frames.len() - 2].waiting_kind
                == before.admission.frames.last().frame_kind
            &&& (before.admission.frames.last().frame_kind != 3
                || before.admission.frames[before.admission.frames.len() - 2].nested_resolved
                    < u64::MAX)
            &&& accept_child(before.epm, after.epm, Some(return_id), summary)
        },
        None => false,
    }
    &&& after.admission == admission::AdmissionState {
        frames: before.admission.frames.subrange(0, before.admission.frames.len() - 1)
            .update(
                before.admission.frames.len() - 2,
                epm_parent_resolved(before.admission.frames[before.admission.frames.len() - 2]),
            ),
        arena: before.admission.arena,
        root_end: before.admission.root_end,
        pushed: before.admission.pushed,
        completed: (before.admission.completed as int + 1) as usize,
        next_obligation_id: before.admission.next_obligation_id,
    }
    &&& after.indexed_regions == before.indexed_regions.drop_last()
    &&& admission::admission_state_wf(after.admission)
    &&& linked_indexed_sources(after)
}

pub open spec fn linked_epm_complete(
    before: LinkedEpmState,
    after: LinkedEpmState,
    action: EpmAction,
) -> bool {
    &&& admission::admission_state_wf(before.admission)
    &&& linked_indexed_sources(before)
    &&& before.admission.frames.len() > 0
    &&& before.admission.frames.last().frame_kind == 3
    &&& before.admission.frames.last().waiting_id.is_none()
    &&& before.admission.frames.last().nested_started
        == before.admission.frames.last().nested_resolved
    &&& before.admission.frames.last().cursor == before.admission.frames.last().region_end
    &&& admission::admission_frame_wf(before.admission.frames.last())
    &&& admission::admission_family_admissible(before.admission.frames.last())
    &&& before.epm.snapshot == before.indexed_regions.last()
    &&& event_step(
        before.epm, after.epm, EpmEvent::End,
        before.admission.next_obligation_id, action,
    )
    &&& after.admission == before.admission
    &&& after.indexed_regions == before.indexed_regions
}

}

fn main() {}
