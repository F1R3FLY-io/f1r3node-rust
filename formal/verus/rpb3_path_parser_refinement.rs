//! Deterministic active-state model of `CanonicalPathMachine` byte admission.
//! `feed_parser_byte` models every successful Edge event, including Plain
//! continuations through varints, UTF-8, Kids, nested Regions, and persistent
//! faults. `source_parser_matches` states the exact frame/owner projection
//! expected of the read-only Rust parser snapshot; that Rust-to-Verus
//! correspondence is an explicit source premise, not asserted by this file.

use vstd::prelude::*;

verus! {

/// A semantic, active-chain image of CanonicalPathMachine. A frame's `next`
/// link is its predecessor in `stack`, so the arena's unused historical slots
/// do not affect a transition. The top frame is the final sequence element.
pub enum ParserStatus {
    Open,
    SplitComplete,
    Dead,
}

pub enum ParserObligationKind {
    RawParEscape,
    NestedEpm1,
}

pub enum ParserVarintRole {
    Scalar,
    Scale,
    Utf8Bytes,
    RawBytes,
    Count,
    RegionLength,
    ObligationLength { kind: ParserObligationKind, tag_source: ParserSource },
}

pub struct ParserSource {
    pub source: usize,
    pub offset: usize,
}

pub struct ParserVarint {
    pub value: u64,
    pub shift: u32,
    pub bytes: u8,
}

pub struct ParserUtf8 {
    pub remaining: u8,
    pub next_min: u8,
    pub next_max: u8,
}

pub enum ParserObligationStatus {
    Payload,
    AwaitingResolution { payload_end: ParserSource },
}

pub enum ParserFrame {
    Tag { escape_allowed: bool },
    Varint { role: ParserVarintRole, state: ParserVarint },
    Bytes { remaining: u64, utf8: Option<ParserUtf8> },
    Kids { remaining: u64 },
    RegionLength {
        remaining: u64,
        ghost previous: Option<Seq<u8>>,
        state: ParserVarint,
    },
    /// The owner entry carries this frame's fields while its child is focused.
    RegionActive,
    Obligation {
        id: u64,
        kind: ParserObligationKind,
        payload_len: u64,
        tag_source: ParserSource,
        payload_start: usize,
        status: ParserObligationStatus,
    },
}

pub struct ParserPath {
    pub status: ParserStatus,
    pub completed_segments: u64,
    pub first_tag: Option<(u8, ParserSource)>,
    pub stable: bool,
    pub escape_allowed: bool,
    pub ghost stack: Seq<ParserFrame>,
}

pub struct ParserRegionOwner {
    pub parent: ParserPath,
    pub remaining: u64,
    pub ghost previous: Option<Seq<u8>>,
    pub declared_len: u64,
    pub path_remaining: u64,
    pub start_depth: usize,
    pub ghost child_bytes: Seq<u8>,
}

/// `owners` is ordered from the outermost active Region to the nearest one.
/// Every owner consumes the next byte, including bytes in nested Regions.
pub struct ParserImage {
    pub focus: ParserPath,
    pub ghost owners: Seq<ParserRegionOwner>,
    pub next_obligation: u64,
    pub stamp: usize,
    pub fault: bool,
}

pub struct ParserGate {
    pub dead: bool,
    pub stack_empty: bool,
    pub split_complete: bool,
    pub completed_segments: u64,
    pub first_is_list: bool,
    pub stable: bool,
    pub escape_allowed: bool,
}

pub enum ParserResolution {
    RawPar { canonical_path_stable: bool },
    NestedEpm1 { is_map: bool, entries_stable: bool },
}

pub enum ParserVarintFeed {
    More { state: ParserVarint },
    Complete { value: u64 },
}

pub enum ParserEvent {
    Continue,
    ObligationStarted { id: u64, kind: ParserObligationKind, payload_len: u64 },
    ObligationByte { id: u64, last: bool },
    /// Rust's checked execution has no successful edge at this counter limit.
    NoSuccessfulEdge,
}

pub struct ParserStep {
    pub image: ParserImage,
    pub event: ParserEvent,
}

/// Logical mirror of CanonicalPathParserSourceImage. Frame indices are the
/// read-only snapshot's reachable-frame DAG indices, not arena slot indices.
pub struct ParserLocated {
    pub byte: u8,
    pub source: ParserSource,
}

pub enum ParserSourceVarintRole {
    Scalar,
    Scale,
    Bytes { utf8: bool },
    Count,
    RegionLength,
    ObligationLength { kind: ParserObligationKind, tag_source: ParserSource },
}

pub struct ParserSourcePath {
    pub status: ParserStatus,
    pub completed_segments: u64,
    pub first_tag: Option<ParserLocated>,
    pub stable: bool,
    pub escape_allowed: bool,
    pub stack: Option<usize>,
}

pub enum ParserSourceRegionPhase {
    PathLength { state: ParserVarint },
    Path {
        declared_len: u64,
        remaining: u64,
        start_depth: usize,
        path: ParserSourcePath,
    },
}

pub enum ParserSourceFrame {
    Tag { escape_allowed: bool, next: Option<usize> },
    Varint {
        role: ParserSourceVarintRole,
        state: ParserVarint,
        next: Option<usize>,
    },
    Bytes { remaining: u64, utf8: Option<ParserUtf8>, next: Option<usize> },
    Kids { remaining: u64, next: Option<usize> },
    Region {
        remaining: u64,
        ghost previous: Option<Seq<ParserLocated>>,
        phase: ParserSourceRegionPhase,
        next: Option<usize>,
    },
    Obligation {
        id: u64,
        kind: ParserObligationKind,
        payload_len: u64,
        tag_source: ParserSource,
        payload_start: usize,
        status: ParserObligationStatus,
        next: Option<usize>,
    },
}

pub struct ParserSourceOwner {
    pub id: usize,
    pub owner: ParserSourcePath,
    pub parent: Option<usize>,
    pub region_end: u128,
    pub path_end: u128,
    pub all_boundary_end: u128,
    pub all_boundary_owner: usize,
    pub path_boundary_end: u128,
    pub path_boundary_owner: usize,
    pub outermost: usize,
}

pub struct ParserSourceSnapshot {
    pub next_obligation: u64,
    pub stamp: usize,
    pub ghost path_bytes: Seq<ParserLocated>,
    pub focused: ParserSourcePath,
    pub root: ParserSourcePath,
    pub ghost owner_chain: Seq<ParserSourceOwner>,
    pub ghost frames: Seq<ParserSourceFrame>,
    pub fault: bool,
    pub root_gate: ParserGate,
}

pub open spec fn located_values(bytes: Seq<ParserLocated>) -> Seq<u8> {
    Seq::new(bytes.len(), |i: int| bytes[i].byte)
}

pub open spec fn source_varint_role_matches(
    source: ParserSourceVarintRole, modeled: ParserVarintRole,
) -> bool {
    match (source, modeled) {
        (ParserSourceVarintRole::Scalar, ParserVarintRole::Scalar)
        | (ParserSourceVarintRole::Scale, ParserVarintRole::Scale)
        | (ParserSourceVarintRole::Count, ParserVarintRole::Count)
        | (ParserSourceVarintRole::RegionLength, ParserVarintRole::RegionLength) => true,
        (ParserSourceVarintRole::Bytes { utf8: true }, ParserVarintRole::Utf8Bytes)
        | (ParserSourceVarintRole::Bytes { utf8: false }, ParserVarintRole::RawBytes) => true,
        (
            ParserSourceVarintRole::ObligationLength { kind: source_kind, tag_source: source_tag },
            ParserVarintRole::ObligationLength { kind: modeled_kind, tag_source: modeled_tag },
        ) => source_kind == modeled_kind && source_tag == modeled_tag,
        _ => false,
    }
}

pub open spec fn source_frame_next(frame: ParserSourceFrame) -> Option<usize> {
    match frame {
        ParserSourceFrame::Tag { next, .. }
        | ParserSourceFrame::Varint { next, .. }
        | ParserSourceFrame::Bytes { next, .. }
        | ParserSourceFrame::Kids { next, .. }
        | ParserSourceFrame::Region { next, .. }
        | ParserSourceFrame::Obligation { next, .. } => next,
    }
}

pub open spec fn source_frame_matches(
    source: ParserSourceFrame, modeled: ParserFrame,
) -> bool {
    match (source, modeled) {
        (
            ParserSourceFrame::Tag { escape_allowed: source_allowed, .. },
            ParserFrame::Tag { escape_allowed: modeled_allowed },
        ) => source_allowed == modeled_allowed,
        (
            ParserSourceFrame::Varint { role: source_role, state: source_state, .. },
            ParserFrame::Varint { role: modeled_role, state: modeled_state },
        ) => source_varint_role_matches(source_role, modeled_role)
            && source_state == modeled_state,
        (
            ParserSourceFrame::Bytes { remaining: source_remaining, utf8: source_utf8, .. },
            ParserFrame::Bytes { remaining: modeled_remaining, utf8: modeled_utf8 },
        ) => source_remaining == modeled_remaining && source_utf8 == modeled_utf8,
        (
            ParserSourceFrame::Kids { remaining: source_remaining, .. },
            ParserFrame::Kids { remaining: modeled_remaining },
        ) => source_remaining == modeled_remaining,
        (
            ParserSourceFrame::Region {
                remaining: source_remaining, previous: source_previous,
                phase: ParserSourceRegionPhase::PathLength { state: source_state }, ..
            },
            ParserFrame::RegionLength {
                remaining: modeled_remaining, previous: modeled_previous,
                state: modeled_state,
            },
        ) => source_remaining == modeled_remaining
            && source_state == modeled_state
            && match (source_previous, modeled_previous) {
                (None, None) => true,
                (Some(bytes), Some(values)) => located_values(bytes) == values,
                _ => false,
            },
        (
            ParserSourceFrame::Region {
                phase: ParserSourceRegionPhase::Path { .. }, ..
            }, ParserFrame::RegionActive,
        ) => true,
        (
            ParserSourceFrame::Obligation {
                id: source_id, kind: source_kind, payload_len: source_len,
                tag_source: source_tag, payload_start: source_start,
                status: source_status, ..
            },
            ParserFrame::Obligation {
                id: modeled_id, kind: modeled_kind, payload_len: modeled_len,
                tag_source: modeled_tag, payload_start: modeled_start,
                status: modeled_status,
            },
        ) => source_id == modeled_id && source_kind == modeled_kind
            && source_len == modeled_len && source_tag == modeled_tag
            && source_start == modeled_start && source_status == modeled_status,
        _ => false,
    }
}

pub open spec fn source_stack_matches(
    frames: Seq<ParserSourceFrame>, top: Option<usize>, modeled: Seq<ParserFrame>,
) -> bool
    decreases modeled.len(),
{
    match top {
        None => modeled.len() == 0,
        Some(index) => modeled.len() > 0 && index < frames.len()
            && source_frame_matches(frames[index as int], modeled.last())
            && source_stack_matches(frames, source_frame_next(frames[index as int]),
                modeled.drop_last()),
    }
}

pub open spec fn source_path_matches(
    frames: Seq<ParserSourceFrame>, source: ParserSourcePath, modeled: ParserPath,
) -> bool {
    &&& source.status == modeled.status
    &&& source.completed_segments == modeled.completed_segments
    &&& source.first_tag == match modeled.first_tag {
        Some((byte, anchor)) => Some(ParserLocated { byte, source: anchor }),
        None => None,
    }
    &&& source.stable == modeled.stable
    &&& source.escape_allowed == modeled.escape_allowed
    &&& source_stack_matches(frames, source.stack, modeled.stack)
}

pub open spec fn source_owner_matches(
    source: ParserSourceSnapshot, index: int, modeled: ParserRegionOwner,
) -> bool
    recommends 0 <= index < source.owner_chain.len(),
{
    let owner = source.owner_chain[index];
    &&& source_path_matches(source.frames, owner.owner, modeled.parent)
    &&& owner.region_end >= source.stamp as u128
    &&& owner.path_end >= source.stamp as u128
    &&& owner.region_end as int - source.stamp as int == modeled.remaining as int
    &&& owner.path_end as int - source.stamp as int == modeled.path_remaining as int
    &&& owner.owner.stack.is_some()
    &&& owner.owner.stack.unwrap() < source.frames.len()
    &&& match source.frames[owner.owner.stack.unwrap() as int] {
        ParserSourceFrame::Region {
            previous,
            phase: ParserSourceRegionPhase::Path {
                declared_len, start_depth, ..
            }, ..
        } => {
            &&& declared_len == modeled.declared_len
            &&& start_depth == modeled.start_depth
            &&& match (previous, modeled.previous) {
                (None, None) => true,
                (Some(bytes), Some(values)) => located_values(bytes) == values,
                _ => false,
            }
        },
        _ => false,
    }
    &&& modeled.start_depth <= source.stamp
    &&& source.stamp as int <= source.path_bytes.len()
    &&& modeled.child_bytes == located_values(source.path_bytes.subrange(
        modeled.start_depth as int, source.stamp as int,
    ))
    &&& (index == 0 ==> owner.parent.is_none())
    &&& (index > 0 ==> owner.parent == Some(source.owner_chain[index - 1].id))
}

/// Full active parser-state projection from the production read-only source
/// snapshot. This relation is exact about frame continuations, nested Region
/// residuals, the selected stamp, and fault, rather than just the gate.
pub open spec fn source_parser_matches(
    source: ParserSourceSnapshot, modeled: ParserImage,
) -> bool {
    &&& source.next_obligation == modeled.next_obligation
    &&& source.stamp == modeled.stamp
    &&& source.fault == modeled.fault
    &&& source_path_matches(source.frames, source.focused, modeled.focus)
    &&& source.owner_chain.len() == modeled.owners.len()
    &&& forall|i: int| 0 <= i < source.owner_chain.len() ==>
        #[trigger] source_owner_matches(source, i, modeled.owners[i])
    &&& source_path_matches(source.frames, source.root, parser_root(modeled))
    &&& source.root_gate == parser_root_gate(modeled)
}

pub open spec fn initial_parser_source_snapshot() -> ParserSourceSnapshot {
    let path = ParserSourcePath {
        status: ParserStatus::Open,
        completed_segments: 0,
        first_tag: None,
        stable: true,
        escape_allowed: true,
        stack: None,
    };
    ParserSourceSnapshot {
        next_obligation: 0,
        stamp: 0,
        path_bytes: Seq::empty(),
        focused: path,
        root: path,
        owner_chain: Seq::empty(),
        frames: Seq::empty(),
        fault: false,
        root_gate: parser_root_gate(initial_parser()),
    }
}

pub proof fn initial_parser_source_snapshot_has_witness()
    ensures source_parser_matches(initial_parser_source_snapshot(), initial_parser()),
{}

pub open spec fn initial_parser_path(escape_allowed: bool) -> ParserPath {
    ParserPath {
        status: ParserStatus::Open,
        completed_segments: 0,
        first_tag: None,
        stable: true,
        escape_allowed,
        stack: Seq::empty(),
    }
}

pub open spec fn initial_parser() -> ParserImage {
    ParserImage {
        focus: initial_parser_path(true),
        owners: Seq::empty(),
        next_obligation: 0,
        stamp: 0,
        fault: false,
    }
}

pub open spec fn initial_varint() -> ParserVarint {
    ParserVarint { value: 0, shift: 0, bytes: 0 }
}

pub open spec fn initial_utf8() -> ParserUtf8 {
    ParserUtf8 { remaining: 0, next_min: 0, next_max: 0 }
}

/// This mirrors VarintState::feed, including overflow and minimality checks.
/// A failed feed is represented by None and is a Dead parser transition.
pub open spec fn feed_parser_varint(state: ParserVarint, byte: u8) -> Option<ParserVarintFeed> {
    let group = byte & 0x7f;
    if state.shift > 63 || (state.shift == 63 && group > 1) {
        None
    } else {
        let value = state.value | ((group as u64) << state.shift);
        let bytes = state.bytes + 1;
        if byte & 0x80 != 0 {
            Some(ParserVarintFeed::More {
                state: ParserVarint {
                    value, shift: (state.shift + 7) as u32, bytes: bytes as u8,
                },
            })
        } else if byte == 0 && bytes > 1 {
            None
        } else {
            Some(ParserVarintFeed::Complete { value })
        }
    }
}

pub proof fn initial_single_byte_varint(byte: u8)
    requires byte < 0x80,
    ensures feed_parser_varint(initial_varint(), byte)
        == Some(ParserVarintFeed::Complete { value: byte as u64 }),
{
    assert((byte & 0x7fu8) == byte) by (bit_vector) requires byte < 0x80;
    assert((byte & 0x80u8) == 0u8) by (bit_vector) requires byte < 0x80;
    assert((0u64 | ((byte as u64) << 0u32)) == byte as u64) by (bit_vector);
    assert(feed_parser_varint(initial_varint(), byte)
        == Some(ParserVarintFeed::Complete { value: byte as u64 }));
}

pub proof fn two_byte_varint_decodes_130()
    ensures {
        let first = ParserVarint { value: 2, shift: 7, bytes: 1 };
        &&& feed_parser_varint(initial_varint(), 0x82)
            == Some(ParserVarintFeed::More { state: first })
        &&& feed_parser_varint(first, 0x01)
            == Some(ParserVarintFeed::Complete { value: 130 })
    },
{
    assert((0x82u8 & 0x7fu8) == 2u8) by (bit_vector);
    assert((0x82u8 & 0x80u8) != 0u8) by (bit_vector);
    assert((0u64 | (2u64 << 0u32)) == 2u64) by (bit_vector);
    assert((0x01u8 & 0x7fu8) == 1u8) by (bit_vector);
    assert((0x01u8 & 0x80u8) == 0u8) by (bit_vector);
    assert((2u64 | (1u64 << 7u32)) == 130u64) by (bit_vector);
}

/// Exact Utf8State::feed transition, including the special first-continuation
/// ranges for E0, ED, F0, and F4.
pub open spec fn feed_parser_utf8(state: ParserUtf8, byte: u8) -> Option<ParserUtf8> {
    if state.remaining != 0 {
        if byte < state.next_min || byte > state.next_max {
            None
        } else {
            Some(ParserUtf8 {
                remaining: (state.remaining - 1) as u8,
                next_min: 0x80,
                next_max: 0xbf,
            })
        }
    } else if byte <= 0x7f {
        Some(initial_utf8())
    } else if 0xc2 <= byte && byte <= 0xdf {
        Some(ParserUtf8 { remaining: 1, next_min: 0x80, next_max: 0xbf })
    } else if byte == 0xe0 {
        Some(ParserUtf8 { remaining: 2, next_min: 0xa0, next_max: 0xbf })
    } else if (0xe1 <= byte && byte <= 0xec) || (0xee <= byte && byte <= 0xef) {
        Some(ParserUtf8 { remaining: 2, next_min: 0x80, next_max: 0xbf })
    } else if byte == 0xed {
        Some(ParserUtf8 { remaining: 2, next_min: 0x80, next_max: 0x9f })
    } else if byte == 0xf0 {
        Some(ParserUtf8 { remaining: 3, next_min: 0x90, next_max: 0xbf })
    } else if 0xf1 <= byte && byte <= 0xf3 {
        Some(ParserUtf8 { remaining: 3, next_min: 0x80, next_max: 0xbf })
    } else if byte == 0xf4 {
        Some(ParserUtf8 { remaining: 3, next_min: 0x80, next_max: 0x8f })
    } else {
        None
    }
}

pub proof fn utf8_ed_rejects_surrogate_prefix()
    ensures {
        let lead = feed_parser_utf8(initial_utf8(), 0xed).unwrap();
        &&& lead.remaining == 2
        &&& lead.next_min == 0x80
        &&& lead.next_max == 0x9f
        &&& feed_parser_utf8(lead, 0xa0).is_none()
    },
{}

pub open spec fn parser_root(image: ParserImage) -> ParserPath {
    if image.owners.len() == 0 { image.focus } else { image.owners[0].parent }
}

pub open spec fn parser_root_stack_empty(image: ParserImage) -> bool {
    parser_root(image).stack.len() == 0
}

pub open spec fn parser_root_gate(image: ParserImage) -> ParserGate {
    let root = parser_root(image);
    ParserGate {
        dead: image.fault || root.status == ParserStatus::Dead,
        stack_empty: root.stack.len() == 0,
        split_complete: root.status == ParserStatus::SplitComplete,
        completed_segments: root.completed_segments,
        first_is_list: match root.first_tag {
            Some((tag, _)) => tag == 0x0b,
            None => false,
        },
        stable: root.stable,
        escape_allowed: root.escape_allowed,
    }
}

/// Trie Leave selects a historical parser handle but does not roll back the
/// machine-global obligation counter, which is outside persistent checkpoints.
pub open spec fn restore_parser_checkpoint(
    saved: ParserImage, current: ParserImage,
) -> ParserImage {
    ParserImage { next_obligation: current.next_obligation, ..saved }
}

pub open spec fn parser_dead(path: ParserPath) -> ParserPath {
    ParserPath { status: ParserStatus::Dead, ..path }
}

/// Bubble is a deterministic continuation reduction. A non-Kids frame stops
/// bubbling; a Kids frame either completes its parent or opens the next tag.
pub open spec fn bubble_parser(path: ParserPath, segment_stable: bool) -> ParserPath
    decreases path.stack.len(),
{
    if path.stack.len() == 0 {
        ParserPath {
            completed_segments: (path.completed_segments + 1) as u64,
            stable: path.stable && segment_stable,
            ..path
        }
    } else {
        let top = path.stack.last();
        match top {
            ParserFrame::Kids { remaining } => {
                let rest = path.stack.drop_last();
                if remaining == 1 {
                    bubble_parser(ParserPath { stack: rest, ..path }, segment_stable)
                } else {
                    ParserPath {
                        stack: rest.push(ParserFrame::Kids { remaining: (remaining - 1) as u64 })
                            .push(ParserFrame::Tag { escape_allowed: false }),
                        ..path
                    }
                }
            },
            _ => path,
        }
    }
}

/// Source machine first-tag and top-level terminator behavior. The caller
/// then feeds the top frame if this returns an Open path.
pub open spec fn prepare_parser_tag(path: ParserPath, byte: u8, source: ParserSource) -> ParserPath {
    if path.stack.len() != 0 || path.status != ParserStatus::Open {
        path
    } else if byte == 0 {
        ParserPath { status: ParserStatus::SplitComplete, ..path }
    } else {
        ParserPath {
            first_tag: if path.first_tag.is_none() { Some((byte, source)) } else { path.first_tag },
            stack: path.stack.push(ParserFrame::Tag { escape_allowed: path.escape_allowed }),
            ..path
        }
    }
}

/// The raw byte ordering used by the Region duplicate/ascending-key check.
pub open spec fn parser_bytes_less(left: Seq<u8>, right: Seq<u8>) -> bool {
    ||| (left.len() < right.len()
        && forall|i: int| 0 <= i < left.len() ==> #[trigger] left[i] == right[i])
    ||| exists|i: int| 0 <= i < left.len() && i < right.len()
        && left[i] < right[i]
        && forall|j: int| 0 <= j < i ==> #[trigger] left[j] == right[j]
}

pub open spec fn replace_parser_top(path: ParserPath, frame: ParserFrame) -> ParserPath
    recommends path.stack.len() > 0,
{
    ParserPath { stack: path.stack.drop_last().push(frame), ..path }
}

pub open spec fn pop_parser_top(path: ParserPath) -> ParserPath
    recommends path.stack.len() > 0,
{
    ParserPath { stack: path.stack.drop_last(), ..path }
}

pub open spec fn push_parser_varint(path: ParserPath, role: ParserVarintRole) -> ParserPath {
    ParserPath { stack: path.stack.push(ParserFrame::Varint { role, state: initial_varint() }), ..path }
}

pub open spec fn complete_parser_frame(path: ParserPath, stable: bool) -> ParserPath
    recommends path.stack.len() > 0,
{
    bubble_parser(pop_parser_top(path), stable)
}

/// Exact Tag continuation from CanonicalPathMachine::feed_tag. The caller
/// has already added the first top-level tag, if needed, and placed Tag on top.
pub open spec fn feed_parser_tag(
    path: ParserPath, byte: u8, source: ParserSource, escape_allowed: bool,
) -> ParserPath
    recommends path.stack.len() > 0,
{
    let rest = pop_parser_top(path);
    match byte {
        0x01 | 0x02 => bubble_parser(rest, true),
        0x03 => push_parser_varint(rest, ParserVarintRole::Scalar),
        0x04 | 0x05 => push_parser_varint(rest, ParserVarintRole::Utf8Bytes),
        0x06 | 0x08 | 0x0e => push_parser_varint(rest, ParserVarintRole::RawBytes),
        0x07 => ParserPath {
            stack: rest.stack.push(ParserFrame::Bytes { remaining: 8, utf8: None }),
            ..rest
        },
        0x09 => {
            let denominator = push_parser_varint(rest, ParserVarintRole::RawBytes);
            push_parser_varint(denominator, ParserVarintRole::RawBytes)
        },
        0x0a => {
            let scale = push_parser_varint(rest, ParserVarintRole::Scale);
            push_parser_varint(scale, ParserVarintRole::RawBytes)
        },
        0x0b | 0x0c => push_parser_varint(rest, ParserVarintRole::Count),
        0x0d => push_parser_varint(rest, ParserVarintRole::RegionLength),
        0x0f => if escape_allowed {
            push_parser_varint(rest, ParserVarintRole::ObligationLength {
                kind: ParserObligationKind::RawParEscape, tag_source: source,
            })
        } else { parser_dead(path) },
        0x10 => push_parser_varint(rest, ParserVarintRole::ObligationLength {
            kind: ParserObligationKind::NestedEpm1, tag_source: source,
        }),
        _ => parser_dead(path),
    }
}

pub open spec fn finish_parser_varint(
    image: ParserImage, role: ParserVarintRole, value: u64,
    source: ParserSource, path_len: usize,
) -> ParserStep
    recommends image.focus.stack.len() > 0,
{
    let rest = pop_parser_top(image.focus);
    match role {
        ParserVarintRole::Scalar => ParserStep {
            image: ParserImage { focus: bubble_parser(rest, true), ..image },
            event: ParserEvent::Continue,
        },
        ParserVarintRole::Scale => ParserStep {
            image: ParserImage {
                focus: if value > u32::MAX { parser_dead(image.focus) }
                    else { bubble_parser(rest, true) },
                ..image
            },
            event: ParserEvent::Continue,
        },
        ParserVarintRole::Utf8Bytes | ParserVarintRole::RawBytes => {
            let utf8 = match role {
                ParserVarintRole::Utf8Bytes => Some(initial_utf8()),
                _ => None,
            };
            ParserStep {
                image: ParserImage {
                    focus: if value == 0 { bubble_parser(rest, true) } else {
                        ParserPath {
                            stack: rest.stack.push(ParserFrame::Bytes { remaining: value, utf8 }),
                            ..rest
                        }
                    },
                    ..image
                },
                event: ParserEvent::Continue,
            }
        },
        ParserVarintRole::Count => ParserStep {
            image: ParserImage {
                focus: if value == 0 { bubble_parser(rest, true) } else {
                    ParserPath {
                        stack: rest.stack.push(ParserFrame::Kids { remaining: value })
                            .push(ParserFrame::Tag { escape_allowed: false }),
                        ..rest
                    }
                },
                ..image
            },
            event: ParserEvent::Continue,
        },
        ParserVarintRole::RegionLength => ParserStep {
            image: ParserImage {
                focus: if value == 0 { bubble_parser(rest, true) } else {
                    ParserPath {
                        stack: rest.stack.push(ParserFrame::RegionLength {
                            remaining: value, previous: None, state: initial_varint(),
                        }),
                        ..rest
                    }
                },
                ..image
            },
            event: ParserEvent::Continue,
        },
        ParserVarintRole::ObligationLength { kind, tag_source } => {
            let id = image.next_obligation;
            if id == u64::MAX {
                ParserStep { image, event: ParserEvent::NoSuccessfulEdge }
            } else {
                let frame = ParserFrame::Obligation {
                    id, kind, payload_len: value, tag_source, payload_start: path_len,
                    status: if value == 0 {
                        ParserObligationStatus::AwaitingResolution { payload_end: source }
                    } else { ParserObligationStatus::Payload },
                };
                ParserStep {
                    image: ParserImage {
                        focus: ParserPath { stack: rest.stack.push(frame), ..rest },
                        next_obligation: (id + 1) as u64,
                        ..image
                    },
                    event: ParserEvent::ObligationStarted { id, kind, payload_len: value },
                }
            }
        },
    }
}

pub open spec fn feed_parser_bytes(
    path: ParserPath, remaining: u64, utf8: Option<ParserUtf8>, byte: u8,
) -> ParserPath
    recommends path.stack.len() > 0 && remaining > 0,
{
    let updated_utf8 = match utf8 {
        Some(state) => feed_parser_utf8(state, byte),
        None => Some(initial_utf8()),
    };
    if updated_utf8.is_none() {
        parser_dead(path)
    } else if remaining > 1 {
        replace_parser_top(path, ParserFrame::Bytes {
            remaining: (remaining - 1) as u64,
            utf8: if utf8.is_some() { updated_utf8 } else { None },
        })
    } else if utf8.is_some() && updated_utf8.unwrap().remaining != 0 {
        parser_dead(path)
    } else {
        complete_parser_frame(path, true)
    }
}

pub open spec fn parser_continue(image: ParserImage) -> ParserStep {
    ParserStep { image, event: ParserEvent::Continue }
}

/// Feed the current focused path. Active Region children are represented by
/// `owners`, so this function sees RegionLength but never RegionActive on top.
pub open spec fn feed_parser_focus(
    image: ParserImage, byte: u8, source: ParserSource, path_len: usize,
) -> ParserStep {
    let path = image.focus;
    if path.status == ParserStatus::Dead {
        parser_continue(image)
    } else if path.status == ParserStatus::SplitComplete {
        parser_continue(ParserImage { focus: parser_dead(path), ..image })
    } else {
        let prepared = prepare_parser_tag(path, byte, source);
        if prepared.status == ParserStatus::SplitComplete {
            parser_continue(ParserImage { focus: prepared, ..image })
        } else {
            let top = prepared.stack.last();
            match top {
                ParserFrame::Tag { escape_allowed } => parser_continue(ParserImage {
                    focus: feed_parser_tag(prepared, byte, source, escape_allowed), ..image
                }),
                ParserFrame::Varint { role, state } => {
                    match feed_parser_varint(state, byte) {
                        None => parser_continue(ParserImage {
                            focus: parser_dead(prepared), ..image
                        }),
                        Some(ParserVarintFeed::More { state }) => parser_continue(ParserImage {
                            focus: replace_parser_top(prepared, ParserFrame::Varint { role, state }),
                            ..image
                        }),
                        Some(ParserVarintFeed::Complete { value }) => {
                            finish_parser_varint(ParserImage { focus: prepared, ..image },
                                role, value, source, path_len)
                        },
                    }
                },
                ParserFrame::Bytes { remaining, utf8 } => parser_continue(ParserImage {
                    focus: feed_parser_bytes(prepared, remaining, utf8, byte), ..image
                }),
                ParserFrame::Kids { .. } | ParserFrame::RegionActive => parser_continue(
                    ParserImage { focus: parser_dead(prepared), ..image }
                ),
                ParserFrame::RegionLength { remaining, previous, state } => {
                    if remaining == 0 {
                        parser_continue(ParserImage { focus: parser_dead(prepared), ..image })
                    } else {
                        let after_remaining = (remaining - 1) as u64;
                        match feed_parser_varint(state, byte) {
                            None => parser_continue(ParserImage {
                                focus: parser_dead(prepared), ..image
                            }),
                            Some(ParserVarintFeed::More { state }) => {
                                parser_continue(ParserImage {
                                    focus: if after_remaining == 0 { parser_dead(prepared) }
                                        else { replace_parser_top(prepared,
                                            ParserFrame::RegionLength {
                                                remaining: after_remaining, previous, state,
                                            }) },
                                    ..image
                                })
                            },
                            Some(ParserVarintFeed::Complete { value }) => {
                                if value == 0 || value > after_remaining {
                                    parser_continue(ParserImage {
                                        focus: parser_dead(prepared), ..image
                                    })
                                } else {
                                    let parent = replace_parser_top(prepared,
                                        ParserFrame::RegionActive);
                                    let owner = ParserRegionOwner {
                                        parent, remaining: after_remaining, previous,
                                        declared_len: value, path_remaining: value,
                                        start_depth: path_len, child_bytes: Seq::empty(),
                                    };
                                    parser_continue(ParserImage {
                                        focus: initial_parser_path(false),
                                        owners: image.owners.push(owner),
                                        ..image
                                    })
                                }
                            },
                        }
                    }
                },
                ParserFrame::Obligation {
                    id, kind, payload_len, tag_source, payload_start, status,
                } => {
                    match status {
                        ParserObligationStatus::Payload => {
                            if path_len < payload_start
                                || path_len as int - payload_start as int > payload_len as int {
                                parser_continue(ParserImage {
                                    focus: parser_dead(prepared), ..image
                                })
                            } else {
                                let last = path_len as int - payload_start as int == payload_len as int;
                                let focus = if last {
                                    replace_parser_top(prepared, ParserFrame::Obligation {
                                        id, kind, payload_len, tag_source, payload_start,
                                        status: ParserObligationStatus::AwaitingResolution {
                                            payload_end: source,
                                        },
                                    })
                                } else { prepared };
                                ParserStep {
                                    image: ParserImage { focus, ..image },
                                    event: ParserEvent::ObligationByte { id, last },
                                }
                            }
                        },
                        ParserObligationStatus::AwaitingResolution { .. } => parser_continue(
                            ParserImage { focus: parser_dead(prepared), ..image }
                        ),
                    }
                },
            }
        }
    }
}

pub open spec fn parser_owner_has_room(image: ParserImage) -> bool {
    forall|i: int| 0 <= i < image.owners.len() ==>
        #[trigger] image.owners[i].remaining > 0
        && image.owners[i].path_remaining > 0
}

/// The production reducer can return an interior obligation byte with the
/// same persistent state id. `PersistentCheckpoints::get` rebases a healthy
/// state's stamp to the selected handle depth, so owner residuals advance
/// even though the focused frame chain is retained.
pub open spec fn reusable_parser_payload(
    image: ParserImage, path_len: usize,
) -> Option<u64> {
    if image.fault || image.focus.status != ParserStatus::Open
        || image.stamp as int + 1 != path_len as int
        || !parser_owner_has_room(image)
        || exists|i: int| 0 <= i < image.owners.len()
            && (#[trigger] image.owners[i].remaining <= 1
                || image.owners[i].path_remaining <= 1) {
        None
    } else if image.focus.stack.len() == 0 {
        None
    } else {
        match image.focus.stack.last() {
            ParserFrame::Obligation {
                id, payload_len, payload_start,
                status: ParserObligationStatus::Payload, ..
            } if payload_start <= image.stamp
                && image.stamp as int - payload_start as int + 1 < payload_len as int => Some(id),
            _ => None,
        }
    }
}

pub open spec fn consume_parser_owner_byte(image: ParserImage, byte: u8) -> ParserImage
    recommends parser_owner_has_room(image),
{
    ParserImage {
        owners: Seq::new(image.owners.len(), |i: int| {
            let owner = image.owners[i];
            ParserRegionOwner {
                remaining: (owner.remaining - 1) as u64,
                path_remaining: (owner.path_remaining - 1) as u64,
                child_bytes: owner.child_bytes.push(byte),
                ..owner
            }
        }),
        ..image
    }
}

/// Bulk interior Forward is repeated reusable-payload advancement. The
/// selected persistent state id and focused frame are unchanged, while each
/// historical handle rebases the stamp and every active Region residual.
pub open spec fn parser_forward_capacity(image: ParserImage, width: int) -> bool {
    &&& width > 0
    &&& !parser_root_gate(image).dead
    &&& image.focus.status == ParserStatus::Open
    &&& image.stamp as int + width <= usize::MAX as int
    &&& image.focus.stack.len() > 0
    &&& match image.focus.stack.last() {
        ParserFrame::Obligation {
            payload_len, payload_start, status: ParserObligationStatus::Payload, ..
        } => payload_start <= image.stamp
            && image.stamp as int - payload_start as int + width < payload_len as int,
        _ => false,
    }
    &&& forall|i: int| 0 <= i < image.owners.len() ==>
        #[trigger] image.owners[i].remaining as int > width
        && image.owners[i].path_remaining as int > width
}

pub open spec fn forward_parser_payload(image: ParserImage, bytes: Seq<u8>) -> ParserImage
    recommends parser_forward_capacity(image, bytes.len() as int),
{
    let width = bytes.len();
    ParserImage {
        stamp: (image.stamp as int + width) as usize,
        owners: Seq::new(image.owners.len(), |i: int| {
            let owner = image.owners[i];
            ParserRegionOwner {
                remaining: (owner.remaining as int - width) as u64,
                path_remaining: (owner.path_remaining as int - width) as u64,
                child_bytes: owner.child_bytes + bytes,
                ..owner
            }
        }),
        ..image
    }
}

pub open spec fn parser_valid_finished_path(path: ParserPath) -> bool {
    &&& path.status != ParserStatus::Dead
    &&& path.stack.len() == 0
    &&& (path.status == ParserStatus::SplitComplete
        || (path.completed_segments == 1
            && match path.first_tag { Some((tag, _)) => tag != 0x0b, None => false }))
}

pub open spec fn parser_waiting_obligation(path: ParserPath) -> bool {
    path.stack.len() > 0 && match path.stack.last() {
        ParserFrame::Obligation {
            status: ParserObligationStatus::AwaitingResolution { .. }, ..
        } => true,
        _ => false,
    }
}

/// Settle every owner after the byte, from the nearest Region outward. A
/// completed child either reopens PathLength, bubbles the Region segment, or
/// kills the owning path exactly as finish_region_path does.
pub open spec fn settle_parser_owners(image: ParserImage) -> ParserImage
    decreases image.owners.len(),
{
    if image.owners.len() == 0 {
        image
    } else {
        let owner = image.owners.last();
        if image.focus.status == ParserStatus::Dead
            || owner.path_remaining != 0 || parser_waiting_obligation(image.focus) {
            image
        } else {
            let parent = if image.focus.status == ParserStatus::Dead
                || !parser_valid_finished_path(image.focus)
                || !image.focus.stable
                || owner.child_bytes.len() != owner.declared_len as int
                || match owner.previous {
                    Some(previous) => !parser_bytes_less(previous, owner.child_bytes),
                    None => false,
                } {
                parser_dead(owner.parent)
            } else if owner.remaining == 0 {
                bubble_parser(pop_parser_top(owner.parent), true)
            } else {
                replace_parser_top(owner.parent, ParserFrame::RegionLength {
                    remaining: owner.remaining,
                    previous: Some(owner.child_bytes),
                    state: initial_varint(),
                })
            };
            settle_parser_owners(ParserImage {
                focus: parent,
                owners: image.owners.drop_last(),
                ..image
            })
        }
    }
}

pub open spec fn feed_parser_byte(
    image: ParserImage, byte: u8, source: ParserSource, path_len: usize,
) -> ParserStep {
    if parser_root_gate(image).dead {
        parser_continue(image)
    } else if reusable_parser_payload(image, path_len).is_some() {
        ParserStep {
            image: ParserImage {
                stamp: path_len,
                ..consume_parser_owner_byte(image, byte)
            },
            event: ParserEvent::ObligationByte {
                id: reusable_parser_payload(image, path_len).unwrap(), last: false,
            },
        }
    } else if !parser_owner_has_room(image) {
        let dead = ParserImage { focus: parser_dead(image.focus), ..image };
        parser_continue(if image.owners.len() > 0 {
            ParserImage { fault: true, ..image }
        } else { ParserImage { stamp: path_len, ..dead } })
    } else {
        let selected = consume_parser_owner_byte(image, byte);
        let step = feed_parser_focus(selected, byte, source, path_len);
        let settled = settle_parser_owners(step.image);
        let after = if settled.focus.status == ParserStatus::Dead && image.owners.len() > 0 {
            ParserImage { fault: true, next_obligation: settled.next_obligation, ..image }
        } else {
            ParserImage { stamp: path_len, ..settled }
        };
        ParserStep { image: after, event: step.event }
    }
}

/// CanonicalPathMachine::resolve_leaf_obligation, followed by the same owner
/// rebuilding/finalization used after a byte. Rejected summaries kill the
/// focused path; RawPar instability is the one accepted unstable segment.
pub open spec fn resolve_parser_obligation(
    image: ParserImage, id: u64, resolution: ParserResolution,
) -> ParserImage
    recommends image.focus.stack.len() > 0,
{
    let path = image.focus;
    let focus = match path.stack.last() {
        ParserFrame::Obligation {
            id: active_id, kind, status: ParserObligationStatus::AwaitingResolution { .. }, ..
        } if active_id == id => {
            match (kind, resolution) {
                (
                    ParserObligationKind::RawParEscape,
                    ParserResolution::RawPar { canonical_path_stable: false },
                ) => bubble_parser(pop_parser_top(path), false),
                (
                    ParserObligationKind::NestedEpm1,
                    ParserResolution::NestedEpm1 { is_map: true, entries_stable: true },
                ) => bubble_parser(pop_parser_top(path), true),
                _ => parser_dead(path),
            }
        },
        _ => parser_dead(path),
    };
    let selected = settle_parser_owners(ParserImage { focus, ..image });
    if selected.focus.status == ParserStatus::Dead && image.owners.len() > 0 {
        ParserImage { fault: true, next_obligation: selected.next_obligation, ..image }
    } else {
        selected
    }
}

/// Two real first-byte transitions that invalidate the old Plain-gate rule.
pub proof fn initial_plain_parser_tags_change_gate()
    ensures {
        let source = ParserSource { source: 1, offset: 7 };
        let array = feed_parser_byte(initial_parser(), 0x06, source, 1).image;
        let boolean = feed_parser_byte(initial_parser(), 0x02, source, 1).image;
        &&& !parser_root_gate(array).dead
        &&& !parser_root_gate(array).stack_empty
        &&& parser_root_gate(array).completed_segments == 0
        &&& !parser_root_gate(boolean).dead
        &&& parser_root_gate(boolean).stack_empty
        &&& parser_root_gate(boolean).completed_segments == 1
    },
{}

/// The count-one list completes only when its Boolean child bubbles through
/// Kids. A gate-only model cannot infer that continuation from the last byte.
pub proof fn one_child_list_parser_bubbles_to_root()
    ensures {
        let source = ParserSource { source: 1, offset: 7 };
        let tag = feed_parser_byte(initial_parser(), 0x0b, source, 1).image;
        let count = feed_parser_byte(tag, 0x01, source, 2).image;
        let child = feed_parser_byte(count, 0x02, source, 3).image;
        &&& !parser_root_gate(tag).stack_empty
        &&& !parser_root_gate(count).stack_empty
        &&& parser_root_gate(child).stack_empty
        &&& parser_root_gate(child).completed_segments == 1
        &&& parser_root_gate(child).first_is_list
    },
{
    let source = ParserSource { source: 1, offset: 7 };
    let tag = feed_parser_byte(initial_parser(), 0x0b, source, 1).image;
    assert(tag.focus.stack.len() == 1);
    initial_single_byte_varint(0x01);
    let count = feed_parser_byte(tag, 0x01, source, 2).image;
    assert(count.focus.stack.len() == 2);
    assert(count.focus.stack.last() == ParserFrame::Tag { escape_allowed: false });
    assert(count.focus.stack[0] == ParserFrame::Kids { remaining: 1 });
    assert(count.focus.status == ParserStatus::Open);
    assert(!count.fault);
    assert(!parser_root_gate(count).dead);
    let popped = pop_parser_top(count.focus);
    assert(popped.stack.len() == 1);
    assert(popped.stack.last() == ParserFrame::Kids { remaining: 1 });
    reveal_with_fuel(bubble_parser, 3);
    let bubbled = bubble_parser(popped, true);
    assert(bubbled.stack.len() == 0);
    assert(bubbled.completed_segments == 1);
    assert(feed_parser_tag(count.focus, 0x02, source, false).stack.len() == 0);
    let child = feed_parser_byte(count, 0x02, source, 3).image;
    assert(child.focus.stack.len() == 0);
    assert(child.focus.completed_segments == 1);
    assert(child.focus.first_tag.is_some());
}

/// A framed region with one Boolean key opens a focused child after its path
/// length byte, then folds that child and completes the outer segment.
pub proof fn one_key_region_parser_focuses_and_settles()
    ensures {
        let source = ParserSource { source: 1, offset: 7 };
        let tag = feed_parser_byte(initial_parser(), 0x0d, source, 1).image;
        let region_len = feed_parser_byte(tag, 0x02, source, 2).image;
        let key_len = feed_parser_byte(region_len, 0x01, source, 3).image;
        let key = feed_parser_byte(key_len, 0x02, source, 4).image;
        &&& region_len.owners.len() == 0
        &&& key_len.owners.len() == 1
        &&& !parser_root_gate(key_len).stack_empty
        &&& key.owners.len() == 0
        &&& parser_root_gate(key).stack_empty
        &&& parser_root_gate(key).completed_segments == 1
    },
{
    reveal_with_fuel(bubble_parser, 3);
    reveal_with_fuel(settle_parser_owners, 3);
    let source = ParserSource { source: 1, offset: 7 };
    let tag = feed_parser_byte(initial_parser(), 0x0d, source, 1).image;
    assert(tag.focus.stack.len() == 1);
    initial_single_byte_varint(0x02);
    let region_len = feed_parser_byte(tag, 0x02, source, 2).image;
    assert(region_len.focus.stack.len() == 1);
    initial_single_byte_varint(0x01);
    let key_len = feed_parser_byte(region_len, 0x01, source, 3).image;
    assert(key_len.owners.len() == 1);
    let key = feed_parser_byte(key_len, 0x02, source, 4).image;
    assert(key.owners.len() == 0);
    assert(key.focus.completed_segments == 1);
}

/// Persistent selection retains a pre-failure focused Region state and stamp,
/// records the fault, and lets later dead-branch edges append without feeding.
pub proof fn nested_invalid_tag_fault_freezes_parser_on_later_plain_edge()
    ensures {
        let source = ParserSource { source: 1, offset: 7 };
        let tag = feed_parser_byte(initial_parser(), 0x0d, source, 1).image;
        let region_len = feed_parser_byte(tag, 0x02, source, 2).image;
        let key_len = feed_parser_byte(region_len, 0x01, source, 3).image;
        let bad = feed_parser_byte(key_len, 0xff, source, 4);
        let later = feed_parser_byte(bad.image, 0x06, source, 5);
        &&& key_len.stamp == 3
        &&& key_len.owners.len() == 1
        &&& bad.event == ParserEvent::Continue
        &&& bad.image.fault
        &&& bad.image.stamp == 3
        &&& bad.image.owners == key_len.owners
        &&& bad.image.focus == key_len.focus
        &&& parser_root_gate(bad.image).dead
        &&& later.event == ParserEvent::Continue
        &&& later.image == bad.image
    },
{
    reveal_with_fuel(settle_parser_owners, 3);
    let source = ParserSource { source: 1, offset: 7 };
    let tag = feed_parser_byte(initial_parser(), 0x0d, source, 1).image;
    initial_single_byte_varint(0x02);
    let region_len = feed_parser_byte(tag, 0x02, source, 2).image;
    initial_single_byte_varint(0x01);
    let key_len = feed_parser_byte(region_len, 0x01, source, 3).image;
    assert(key_len.owners.len() == 1);
    let bad = feed_parser_byte(key_len, 0xff, source, 4);
    assert(bad.image.fault);
    let later = feed_parser_byte(bad.image, 0x06, source, 5);
    assert(later.image == bad.image);
}

}

fn main() {}
