use std::cell::RefCell;
use std::ops::Range;

use rspace_plus_plus::rspace::pathmap_codec::{
    CanonicalCompactTrieTopologyCursor, CanonicalCompactTrieTopologyEvent,
};

use super::*;
use crate::rust::epathmap_trie_codec::IndexedCanonicalEpm1ActiveSourceImage;
use crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte;
use crate::rust::rholang::canonical_path_admission::{
    CanonicalPathAdmissionStats, CanonicalPathBatchExactSourceObservation,
    CanonicalPathBatchHandleSource, CanonicalPathParserSourceImage,
};
use crate::rust::rholang::canonical_score_bytes::{
    CanonicalScoreSourceEntry, CanonicalScoreSourceKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RootFrameState {
    pub program: &'static RhoMatchProgram,
    pub program_type_id: u16,
    pub region_start: CanonicalByteProvenance,
    pub region_end: CanonicalByteProvenance,
    pub region_len: usize,
    pub cursor_empty: bool,
    pub cursor_fields_read: usize,
    pub cursor_bytes_read: usize,
    pub cursor_bytes_traversed: usize,
    pub cursor_bytes_fast_forwarded: usize,
    pub waiting: bool,
    pub waiting_detail: Option<RootWaitState>,
    pub last_slot: Option<usize>,
    pub seen_slots: Vec<usize>,
    pub semantic_slots: Vec<Vec<RootOccurrenceState>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MapFrameState {
    pub owner: &'static RhoMatchProgram,
    pub outer_field: u32,
    pub value_type_id: u16,
    pub body: Vec<u8>,
    pub region_start: CanonicalByteProvenance,
    pub region_end: CanonicalByteProvenance,
    pub cursor: IndexedCanonicalFieldCursorStats,
    pub key: Option<Vec<u8>>,
    pub key_position: Option<CanonicalByteProvenance>,
    pub saw_value: bool,
    pub value_position: Option<CanonicalByteProvenance>,
    pub value: Option<RootParState>,
    pub waiting_value: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PathWaitState {
    pub id: CanonicalPathObligationId,
    pub kind: CanonicalPathObligationKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EPathMapFrameState {
    pub owner: &'static RhoMatchProgram,
    pub field: u32,
    pub snapshot: Vec<u8>,
    pub region_start: CanonicalByteProvenance,
    pub region_end: CanonicalByteProvenance,
    pub mode: EPathMapMode,
    pub cursor: IndexedCanonicalEpm1TopologyStats,
    pub path: CanonicalPathAdmissionStats,
    pub active_obligation: Option<CanonicalPathObligationProgress>,
    pub entries_canonical_path_stable: bool,
    pub waiting_map_value: bool,
    pub waiting_obligation: Option<PathWaitState>,
    pub last_source: CanonicalPathSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CanonicalPathFrameState {
    pub input: Vec<u8>,
    pub region_start: CanonicalByteProvenance,
    pub region_end: CanonicalByteProvenance,
    pub bytes_yielded: usize,
    pub bytes_traversed: usize,
    pub reducer: CanonicalPathAdmissionStats,
    pub parser_source: CanonicalPathParserSourceImage,
    pub active_obligation: Option<CanonicalPathObligationProgress>,
    pub waiting: Option<PathWaitState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NonMessageFrameState {
    Map(MapFrameState),
    EPathMap(EPathMapFrameState),
    CanonicalPath(CanonicalPathFrameState),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ActiveFrameState {
    Message {
        frame: RootFrameState,
        bytes: Vec<u8>,
        anchors: Vec<CanonicalByteProvenance>,
    },
    NonMessage {
        frame: NonMessageFrameState,
        bytes: Vec<u8>,
        anchors: Vec<CanonicalByteProvenance>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EPathMapEventKind {
    Forward {
        width: usize,
        source_range: Range<usize>,
    },
    Unwind {
        width: usize,
        source_range: Range<usize>,
    },
    Edge(u8),
    Leave,
    Value {
        has_value: bool,
        value_body_range: Option<Range<usize>>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct EPathMapEventState {
    pub step_index: usize,
    pub kind: EPathMapEventKind,
    pub source: CanonicalByteProvenance,
    pub after: EPathMapFrameState,
    pub path_source: CanonicalPathBatchExactSourceObservation,
    pub value_reducer_stable: Option<bool>,
    pub cursor_source: IndexedCanonicalEpm1ActiveSourceImage,
}

pub(super) struct EPathMapScalarEdgeNoChildState {
    pub step_index: usize,
    pub byte: u8,
    pub provenance: CanonicalByteProvenance,
    pub cursor_before: IndexedCanonicalEpm1ActiveSourceImage,
    pub cursor_after: IndexedCanonicalEpm1ActiveSourceImage,
    pub path_before: CanonicalPathBatchHandleSource,
    pub path_after: CanonicalPathBatchHandleSource,
    pub parser_before: CanonicalPathParserSourceImage,
    pub parser_after: CanonicalPathParserSourceImage,
    pub no_child: bool,
    pub no_map_value_receipt: bool,
    pub owner_before: &'static RhoMatchProgram,
    pub owner_after: &'static RhoMatchProgram,
    pub field_before: u32,
    pub field_after: u32,
    pub region_start_before: CanonicalByteProvenance,
    pub region_start_after: CanonicalByteProvenance,
    pub snapshot_len_before: usize,
    pub snapshot_len_after: usize,
    pub waiting_before: bool,
    pub waiting_after: bool,
    pub waiting_child_len_before: Option<usize>,
    pub waiting_child_len_after: Option<usize>,
    pub entries_stable_before: bool,
    pub entries_stable_after: bool,
}

#[derive(Clone, Debug)]
pub(super) struct CanonicalPathByteState {
    pub step_index: usize,
    pub index: usize,
    pub byte: u8,
    pub source: CanonicalByteProvenance,
    pub skipped: usize,
    pub yielded_after: usize,
    pub traversed_after: usize,
    pub parser_source_after: CanonicalPathParserSourceImage,
}

#[derive(Clone, Debug)]
pub(super) struct NonMessagePushBodyState {
    pub step_index: usize,
    pub body: Vec<u8>,
    pub start: CanonicalByteProvenance,
    pub end: CanonicalByteProvenance,
    pub obligation: Option<PathWaitState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RootWaitState {
    Child {
        slot: usize,
        variant: Option<&'static str>,
        body: Vec<u8>,
        body_start: CanonicalByteProvenance,
        body_end: CanonicalByteProvenance,
        field_position: CanonicalByteProvenance,
    },
    Map {
        slot: usize,
        field_position: CanonicalByteProvenance,
    },
    EPathMap {
        slot: usize,
        field_position: CanonicalByteProvenance,
    },
}

impl RootWaitState {
    fn slot(&self) -> usize {
        match self {
            Self::Child { slot, .. } | Self::Map { slot, .. } | Self::EPathMap { slot, .. } => {
                *slot
            }
        }
    }
    fn field_position(&self) -> CanonicalByteProvenance {
        match self {
            Self::Child { field_position, .. }
            | Self::Map { field_position, .. }
            | Self::EPathMap { field_position, .. } => *field_position,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RootScalarState {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    String(Vec<u8>),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RootParState {
    pub score_handle: CanonicalScoreHandle,
    pub semantic_default: bool,
    pub canonical_path_stable: bool,
    pub identity_prefix: Vec<u8>,
    pub identity_suffix: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RootReturnedState {
    Opaque,
    Par(RootParState),
    Component(RootComponentState, CanonicalScoreHandle),
    Var(CanonicalScoreHandle),
    ReceiveBind {
        score: Option<CanonicalScoreHandle>,
        source_present: bool,
    },
    CostScore(CanonicalScoreHandle),
    MatchCase {
        score: Option<CanonicalScoreHandle>,
        required_present: bool,
        canonical_fixed_point: bool,
    },
    ExprPayload {
        score: Option<CanonicalScoreHandle>,
        canonical_fixed_point: bool,
        canonical_path_stable: bool,
        pathmap_snapshot: Option<Vec<u8>>,
    },
    KeyValuePair {
        key: Option<RootParState>,
        value: Option<RootParState>,
    },
    BigRational {
        numerator: Vec<u8>,
        denominator: Vec<u8>,
    },
    FixedPoint {
        unscaled: Vec<u8>,
        scale: u32,
    },
    ConnectiveBody(Vec<RootParState>),
    VarRef {
        index: i32,
        depth: i32,
    },
    Bytes(Vec<u8>),
    Unit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RootOccurrenceValue {
    Scalar(RootScalarState),
    Message(Box<RootReturnedState>),
    Map {
        key: Vec<u8>,
        key_position: CanonicalByteProvenance,
        value: Option<RootParState>,
    },
    EPathMap {
        mode: EPathMapMode,
        snapshot: Vec<u8>,
        snapshot_start: CanonicalByteProvenance,
        entries_canonical_path_stable: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RootOccurrenceState {
    pub slot: usize,
    pub variant: Option<&'static str>,
    pub body: Option<Vec<u8>>,
    pub body_start: Option<CanonicalByteProvenance>,
    pub body_end: Option<CanonicalByteProvenance>,
    pub field_position: CanonicalByteProvenance,
    pub value: RootOccurrenceValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RootSemanticVariant {
    Opaque,
    Par,
    Component,
    Var,
    ReceiveBind,
    CostScore,
    MatchCase,
    ExprPayload,
    KeyValuePair,
    BigRational,
    FixedPoint,
    ConnectiveBody,
    VarRef,
    Bytes,
    Unit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NoScorePayload {
    Opaque,
    KeyValuePair {
        key: Option<Vec<u8>>,
        value: Option<Vec<u8>>,
    },
    BigRational {
        numerator: Vec<u8>,
        denominator: Vec<u8>,
    },
    FixedPoint {
        unscaled: Vec<u8>,
        scale: u32,
    },
    ConnectiveBody(Vec<Vec<u8>>),
    VarRef {
        index: i32,
        depth: i32,
    },
    Bytes(Vec<u8>),
    Unit,
    ExprPayloadAbsent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RootComponentState {
    pub family: ParComponentTag,
    pub complete: bool,
    pub canonical_path_stable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RootSummaryState {
    pub variant: RootSemanticVariant,
    pub component: Option<RootComponentState>,
    pub no_score_payload: Option<NoScorePayload>,
    pub score_handle: Option<CanonicalScoreHandle>,
    pub score_encoded_len: Option<usize>,
    pub score_bytes: Option<Vec<u8>>,
}

pub(super) struct ChildFinishState {
    pub program: &'static RhoMatchProgram,
    pub frame: RootFrameState,
    pub body: Vec<u8>,
    pub summary: RootSummaryState,
}

pub(super) struct ChildEofState {
    pub program: &'static RhoMatchProgram,
    pub body: Vec<u8>,
    pub frame: RootFrameState,
    pub source_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MessageEofStepState {
    pub step_index: usize,
    pub frame: RootFrameState,
    pub source_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MessageCompleteStepState {
    pub step_index: usize,
    pub frame: RootFrameState,
    pub summary: RootReturnedState,
    pub source_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RootMapState {
    pub key: Vec<u8>,
    pub key_start: CanonicalByteProvenance,
    pub key_end: CanonicalByteProvenance,
    pub key_position: CanonicalByteProvenance,
    pub value: Option<RootParState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MapEofStepState {
    pub step_index: usize,
    pub frame: MapFrameState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MapCompleteStepState {
    pub step_index: usize,
    pub frame: MapFrameState,
    pub summary: RootMapState,
}

pub(super) struct PopAcceptState {
    pub child_kind: FrameRequestKind,
    pub child_program: Option<&'static RhoMatchProgram>,
    pub child_body: Option<Vec<u8>>,
    pub child_frame: Option<RootFrameState>,
    pub child_nonmessage: Option<NonMessageFrameState>,
    pub epathmap_child_snapshot: Option<Vec<u8>>,
    pub epathmap_returned_snapshot: Option<Vec<u8>>,
    pub epathmap_returned_mode: Option<EPathMapMode>,
    pub epathmap_returned_entries_stable: Option<bool>,
    pub summary: Option<RootSummaryState>,
    pub typed_summary: Option<RootReturnedState>,
    pub map_summary: Option<RootMapState>,
    pub before_len: usize,
    pub after_len: Option<usize>,
    pub parent_kind: Option<FrameRequestKind>,
    pub parent_before: Option<RootFrameState>,
    pub parent_after: Option<RootFrameState>,
    pub parent_nonmessage_before: Option<NonMessageFrameState>,
    pub parent_nonmessage_after: Option<NonMessageFrameState>,
    pub applied_obligation: Option<(CanonicalPathObligationId, CanonicalPathObligationResolution)>,
    pub source_len_before: usize,
    pub source_len_after: Option<usize>,
}

pub(super) struct EPathMapAcceptState {
    pub step_index: usize,
    pub parent_type_id: u16,
    pub slot: usize,
    pub tag: u32,
    pub returned_snapshot: Vec<u8>,
    pub stored_snapshot: Vec<u8>,
    pub returned_start: CanonicalByteProvenance,
    pub stored_start: CanonicalByteProvenance,
}

pub(super) struct EPathMapScoreState {
    pub step_index: usize,
    pub body: Vec<u8>,
    pub snapshot: Vec<u8>,
    pub snapshot_handle: CanonicalScoreHandle,
    pub score_handle: CanonicalScoreHandle,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct ExprFinishState {
    pub body: Vec<u8>,
    pub score_handle: CanonicalScoreHandle,
    pub complete: bool,
    pub canonical_path_stable: bool,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct ExprPayloadFinishState {
    pub program: &'static RhoMatchProgram,
    pub body: Vec<u8>,
    pub score_handle: Option<CanonicalScoreHandle>,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct EMapFinishState {
    pub body: Vec<u8>,
    pub pair_scores: Vec<CanonicalScoreHandle>,
    pub complete: bool,
    pub canonical_fixed_point: bool,
    pub score_handle: Option<CanonicalScoreHandle>,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct SendFinishState {
    pub body: Vec<u8>,
    pub input: SendFinishInput,
    pub complete: bool,
    pub score_handle: CanonicalScoreHandle,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct BundleFinishState {
    pub body: Vec<u8>,
    pub input: BundleFinishInput,
    pub complete: bool,
    pub score_handle: CanonicalScoreHandle,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct IfFinishState {
    pub body: Vec<u8>,
    pub input: IfFinishInput,
    pub complete: bool,
    pub score_handle: CanonicalScoreHandle,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct MatchFinishState {
    pub body: Vec<u8>,
    pub input: MatchFinishInput,
    pub complete: bool,
    pub score_handle: CanonicalScoreHandle,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct ReceiveFinishState {
    pub body: Vec<u8>,
    pub input: ReceiveFinishInput,
    pub complete: bool,
    pub score_handle: CanonicalScoreHandle,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct ReceiveBindFinishState {
    pub body: Vec<u8>,
    pub input: ReceiveBindFinishInput,
    pub score_handle: Option<CanonicalScoreHandle>,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct MatchCaseFinishState {
    pub body: Vec<u8>,
    pub input: MatchCaseFinishInput,
    pub score_handle: Option<CanonicalScoreHandle>,
    pub required_present: bool,
    pub canonical_fixed_point: bool,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

pub(super) struct NewFinishState {
    pub body: Vec<u8>,
    pub input: NewFinishInput,
    pub complete: bool,
    pub score_handle: CanonicalScoreHandle,
    pub source_len_before: usize,
    pub source_len_after: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DriverStepKind {
    Continue,
    Push(FramePushObservation),
    Complete,
}

pub(super) struct DriverStepBefore {
    frame_len: usize,
    capacity: usize,
    top_kind: FrameRequestKind,
    top_program: Option<&'static RhoMatchProgram>,
    score_len: usize,
    score_source: Option<Vec<CanonicalScoreSourceEntry>>,
    message: Option<RootFrameState>,
    nonmessage: Option<NonMessageFrameState>,
    epm_path_source: Option<CanonicalPathBatchExactSourceObservation>,
    active_frames: Option<Vec<ActiveFrameState>>,
}

pub(super) struct DriverStepState {
    pub kind: DriverStepKind,
    pub before_len: usize,
    pub after_len: usize,
    pub before_capacity: usize,
    pub after_capacity: usize,
    pub before_top_kind: FrameRequestKind,
    pub after_top_kind: Option<FrameRequestKind>,
    pub before_top_program: Option<&'static RhoMatchProgram>,
    pub after_top_program: Option<&'static RhoMatchProgram>,
    pub after_top_region: Option<(CanonicalByteProvenance, usize)>,
    pub score_len_before: usize,
    pub score_len_after: usize,
    pub score_appended: Vec<CanonicalScoreSourceEntry>,
    pub score_source_before: Option<Vec<CanonicalScoreSourceEntry>>,
    pub score_source_after: Option<Vec<CanonicalScoreSourceEntry>>,
    pub before_message: Option<RootFrameState>,
    pub after_message: Option<RootFrameState>,
    pub before_nonmessage: Option<NonMessageFrameState>,
    pub epm_path_source_before: Option<CanonicalPathBatchExactSourceObservation>,
    pub after_nonmessage: Option<NonMessageFrameState>,
    pub suspended_parent_after: Option<NonMessageFrameState>,
    pub active_frames_before: Option<Vec<ActiveFrameState>>,
    pub active_frames_after: Option<Vec<ActiveFrameState>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CheckedAdmissionEventState {
    pub step_index: usize,
    pub kind: DriverStepKind,
    pub projection: CheckedAdmissionProjection,
    pub before_top_kind: FrameRequestKind,
    pub after_top_kind: Option<FrameRequestKind>,
    pub before_len: usize,
    pub after_len: usize,
    pub score_len_before: usize,
    pub score_len_after: usize,
}

pub(super) struct MessageFieldState {
    pub step_index: usize,
    pub program: &'static RhoMatchProgram,
    pub body: Vec<u8>,
    pub body_positions: Vec<CanonicalByteProvenance>,
    pub body_end_position: CanonicalByteProvenance,
    pub frame_after: RootFrameState,
    pub tag: u32,
    pub field_position: CanonicalByteProvenance,
    pub indexed_field_position: CanonicalByteProvenance,
    pub wire: CanonicalWireType,
    pub encoded_range: CanonicalByteRange,
    pub decoded_varint: Option<u64>,
    pub payload_view_bytes: Option<Vec<u8>>,
    pub slot: usize,
    pub oneof_arm: Option<u32>,
    pub slot_count: usize,
    pub last_slot_before: Option<usize>,
    pub last_slot_after: Option<usize>,
    pub seen_before: bool,
    pub seen_after: bool,
    pub slot_occurrences_before: usize,
    pub slot_occurrences_after: usize,
    pub waiting_after: Option<FrameRequestKind>,
    pub child_kind: Option<FrameRequestKind>,
    pub child_type_id: Option<u16>,
    pub child_region_start: Option<CanonicalByteProvenance>,
    pub child_region_len: Option<usize>,
    pub payload_len: Option<usize>,
    pub payload_position: Option<CanonicalByteProvenance>,
    pub cursor_before: IndexedCanonicalFieldCursorStats,
    pub cursor_after: IndexedCanonicalFieldCursorStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MessageSuccessState {
    Field {
        step_index: usize,
        program_type_id: u16,
        field_number: u32,
        slot: usize,
        oneof_arm: Option<u32>,
        seen_before: bool,
        seen_after: bool,
        cursor_before: IndexedCanonicalFieldCursorStats,
        cursor_after: IndexedCanonicalFieldCursorStats,
        child_kind: Option<FrameRequestKind>,
        child_type_id: Option<u16>,
        child_start: Option<CanonicalByteProvenance>,
        child_end: Option<CanonicalByteProvenance>,
        child_len: Option<usize>,
        pushed: Option<FramePushObservation>,
    },
    Eof {
        step_index: usize,
        program_type_id: u16,
        selected_kind: SemanticKind,
        score_len_before: usize,
        score_len_after: usize,
        component: Option<ComponentFinishObservation>,
        complete_index: usize,
        root: bool,
    },
    Accepted {
        step_index: usize,
        program_type_id: u16,
        slot: usize,
        child_kind: FrameRequestKind,
        child_body: Vec<u8>,
        child_start: CanonicalByteProvenance,
        child_end: CanonicalByteProvenance,
        field_position: CanonicalByteProvenance,
        cursor_before: IndexedCanonicalFieldCursorStats,
        cursor_after: IndexedCanonicalFieldCursorStats,
        occurrences_before: usize,
        occurrences_after: usize,
        stack_len_before: usize,
        stack_len_after: usize,
        score_len_before_complete: usize,
        score_len_after_finish: usize,
        score_len_after_accept: usize,
        pop_index: usize,
    },
}

#[derive(Default)]
pub(super) struct RootFinishTrace {
    root_step: bool,
    full_active_frames: bool,
    scalar_edge_receipts: bool,
    message_success_stream: bool,
    pub driver_steps: Vec<DriverStepState>,
    pub checked_events: Vec<CheckedAdmissionEventState>,
    pub message_fields: Vec<MessageFieldState>,
    pub message_success: Vec<MessageSuccessState>,
    pub epathmap_events: Vec<EPathMapEventState>,
    pub epathmap_scalar_edge_no_child: Vec<EPathMapScalarEdgeNoChildState>,
    pub canonical_path_bytes: Vec<CanonicalPathByteState>,
    pub nonmessage_push_bodies: Vec<NonMessagePushBodyState>,
    pub children: Vec<ChildFinishState>,
    pub child_eofs: Vec<ChildEofState>,
    pub message_eofs: Vec<MessageEofStepState>,
    pub message_completes: Vec<MessageCompleteStepState>,
    pub map_eofs: Vec<MapEofStepState>,
    pub map_completes: Vec<MapCompleteStepState>,
    pub pop_accepts: Vec<PopAcceptState>,
    pub epathmap_accepts: Vec<EPathMapAcceptState>,
    pub epathmap_scores: Vec<EPathMapScoreState>,
    pub expr_finishes: Vec<ExprFinishState>,
    pub expr_payload_finishes: Vec<ExprPayloadFinishState>,
    pub emap_finishes: Vec<EMapFinishState>,
    pub send_finishes: Vec<SendFinishState>,
    pub bundle_finishes: Vec<BundleFinishState>,
    pub if_finishes: Vec<IfFinishState>,
    pub match_finishes: Vec<MatchFinishState>,
    pub receive_finishes: Vec<ReceiveFinishState>,
    pub receive_bind_finishes: Vec<ReceiveBindFinishState>,
    pub match_case_finishes: Vec<MatchCaseFinishState>,
    pub new_finishes: Vec<NewFinishState>,
    pub score_arena: Option<Vec<CanonicalScoreSourceEntry>>,
    pub eof: Option<RootFrameState>,
    pub reduced_frame: Option<RootFrameState>,
    pub reduced: Option<RootSummaryState>,
    pub complete_frame: Option<RootFrameState>,
    pub complete: Option<RootSummaryState>,
    pub pre_pop_frame: Option<RootFrameState>,
    pub pre_pop: Option<RootSummaryState>,
    pub returned: Option<RootSummaryState>,
    pub remaining_frames: Option<usize>,
}

impl RootFinishTrace {
    pub(super) fn with_epathmap_events(
        events: Vec<EPathMapEventState>,
        nonmessage_push_bodies: Vec<NonMessagePushBodyState>,
    ) -> Self {
        Self {
            epathmap_events: events,
            nonmessage_push_bodies,
            ..Self::default()
        }
    }
}

thread_local! {
    static TRACE: RefCell<Option<RootFinishTrace>> = const { RefCell::new(None) };
}

pub(super) fn start() { TRACE.with(|slot| *slot.borrow_mut() = Some(RootFinishTrace::default())); }

pub(super) fn start_with_active_frames() {
    TRACE.with(|slot| {
        *slot.borrow_mut() = Some(RootFinishTrace {
            full_active_frames: true,
            ..RootFinishTrace::default()
        });
    });
}

pub(super) fn start_with_scalar_edge_receipts() {
    TRACE.with(|slot| {
        *slot.borrow_mut() = Some(RootFinishTrace {
            full_active_frames: true,
            scalar_edge_receipts: true,
            ..RootFinishTrace::default()
        });
    });
}

pub(super) fn start_with_message_success_stream() {
    TRACE.with(|slot| {
        *slot.borrow_mut() = Some(RootFinishTrace {
            full_active_frames: true,
            message_success_stream: true,
            ..RootFinishTrace::default()
        });
    });
}

pub(super) fn is_recording_scalar_edge_receipts() -> bool {
    TRACE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|trace| trace.scalar_edge_receipts)
    })
}

pub(super) fn take() -> RootFinishTrace {
    TRACE.with(|slot| {
        slot.borrow_mut()
            .take()
            .expect("root finish trace was started")
    })
}

pub(super) fn set_root_step(root_step: bool) {
    TRACE.with(|slot| {
        if let Some(trace) = slot.borrow_mut().as_mut() {
            trace.root_step = root_step;
        }
    });
}

fn with_root_trace(record: impl FnOnce(&mut RootFinishTrace)) {
    TRACE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(trace) = slot.as_mut() {
            if trace.root_step {
                record(trace);
            }
        }
    });
}

fn with_trace(record: impl FnOnce(&mut RootFinishTrace)) {
    TRACE.with(|slot| {
        if let Some(trace) = slot.borrow_mut().as_mut() {
            record(trace);
        }
    });
}

fn frame_state(frame: &MessageFrame<'_>) -> RootFrameState {
    let cursor_stats = frame.cursor.stats();
    RootFrameState {
        program: frame.program,
        program_type_id: frame.program.type_id,
        region_start: frame.region_start,
        region_end: frame.body.end_position().physical,
        region_len: frame.body.len(),
        cursor_empty: frame.cursor.is_empty(),
        cursor_fields_read: cursor_stats.fields_read,
        cursor_bytes_read: cursor_stats.bytes_read,
        cursor_bytes_traversed: cursor_stats.bytes_traversed,
        cursor_bytes_fast_forwarded: cursor_stats.bytes_fast_forwarded,
        waiting: frame.waiting.is_some(),
        waiting_detail: frame.waiting.as_ref().map(|waiting| match waiting {
            MessageWait::Child {
                slot,
                variant,
                body,
                field_position,
            } => RootWaitState::Child {
                slot: *slot,
                variant: *variant,
                body: body.bytes().collect(),
                body_start: body.position_at(0).unwrap().physical,
                body_end: body.end_position().physical,
                field_position: *field_position,
            },
            MessageWait::Map {
                slot,
                field_position,
            } => RootWaitState::Map {
                slot: *slot,
                field_position: *field_position,
            },
            MessageWait::EPathMap {
                slot,
                field_position,
            } => RootWaitState::EPathMap {
                slot: *slot,
                field_position: *field_position,
            },
        }),
        last_slot: frame.slots.last_slot(),
        seen_slots: frame
            .slots
            .seen_slice()
            .iter()
            .enumerate()
            .filter_map(|(slot, seen)| seen.then_some(slot))
            .collect(),
        semantic_slots: frame
            .semantic_slots
            .iter()
            .enumerate()
            .map(|(slot, occurrences)| {
                occurrences
                    .iter()
                    .map(|occurrence| RootOccurrenceState {
                        slot,
                        variant: occurrence.variant,
                        body: occurrence.body.as_ref().map(|body| body.bytes().collect()),
                        body_start: occurrence
                            .body
                            .as_ref()
                            .map(|body| body.position_at(0).unwrap().physical),
                        body_end: occurrence
                            .body
                            .as_ref()
                            .map(|body| body.end_position().physical),
                        field_position: occurrence.field_position,
                        value: match &occurrence.value {
                            SemanticValue::Scalar(value) => {
                                RootOccurrenceValue::Scalar(match value {
                                    SemanticScalar::Bool(value) => RootScalarState::Bool(*value),
                                    SemanticScalar::I32(value) => RootScalarState::I32(*value),
                                    SemanticScalar::U32(value) => RootScalarState::U32(*value),
                                    SemanticScalar::I64(value) => RootScalarState::I64(*value),
                                    SemanticScalar::U64(value) => RootScalarState::U64(*value),
                                    SemanticScalar::String(value) => {
                                        RootScalarState::String(value.bytes().collect())
                                    }
                                    SemanticScalar::Bytes(value) => {
                                        RootScalarState::Bytes(value.bytes().collect())
                                    }
                                })
                            }
                            SemanticValue::Message(value) => RootOccurrenceValue::Message(
                                Box::new(returned_state(&value.semantic)),
                            ),
                            SemanticValue::Map(value) => RootOccurrenceValue::Map {
                                key: value.key.bytes().collect(),
                                key_position: value.key_position,
                                value: value.value.as_ref().map(par_state),
                            },
                            SemanticValue::EPathMap(value) => RootOccurrenceValue::EPathMap {
                                mode: value.mode,
                                snapshot: value.snapshot.bytes().collect(),
                                snapshot_start: value.snapshot.position_at(0).unwrap().physical,
                                entries_canonical_path_stable: value.entries_canonical_path_stable,
                            },
                        },
                    })
                    .collect()
            })
            .collect(),
    }
}

fn par_state(par: &ParSummary<'_>) -> RootParState {
    RootParState {
        score_handle: par.score,
        semantic_default: par.semantic_default,
        canonical_path_stable: par.canonical_path_stable,
        identity_prefix: par.identity_prefix.bytes().collect(),
        identity_suffix: par.identity_suffix.bytes().collect(),
    }
}

fn returned_state(summary: &SemanticSummary<'_>) -> RootReturnedState {
    match summary {
        SemanticSummary::Opaque => RootReturnedState::Opaque,
        SemanticSummary::Par(par) => RootReturnedState::Par(par_state(par)),
        SemanticSummary::Component(value) => RootReturnedState::Component(
            RootComponentState {
                family: value.family,
                complete: value.complete,
                canonical_path_stable: value.canonical_path_stable,
            },
            value.score,
        ),
        SemanticSummary::Var(value) => RootReturnedState::Var(*value),
        SemanticSummary::ReceiveBind {
            score,
            source_present,
        } => RootReturnedState::ReceiveBind {
            score: *score,
            source_present: *source_present,
        },
        SemanticSummary::CostScore(value) => RootReturnedState::CostScore(*value),
        SemanticSummary::MatchCase {
            score,
            required_present,
            canonical_fixed_point,
        } => RootReturnedState::MatchCase {
            score: *score,
            required_present: *required_present,
            canonical_fixed_point: *canonical_fixed_point,
        },
        SemanticSummary::ExprPayload(value) => RootReturnedState::ExprPayload {
            score: value.score,
            canonical_fixed_point: value.canonical_fixed_point,
            canonical_path_stable: value.canonical_path_stable,
            pathmap_snapshot: value
                .pathmap_snapshot
                .as_ref()
                .map(|region| region.bytes().collect()),
        },
        SemanticSummary::KeyValuePair { key, value } => RootReturnedState::KeyValuePair {
            key: key.as_ref().map(par_state),
            value: value.as_ref().map(par_state),
        },
        SemanticSummary::BigRational {
            numerator,
            denominator,
        } => RootReturnedState::BigRational {
            numerator: numerator.bytes().collect(),
            denominator: denominator.bytes().collect(),
        },
        SemanticSummary::FixedPoint { unscaled, scale } => RootReturnedState::FixedPoint {
            unscaled: unscaled.bytes().collect(),
            scale: *scale,
        },
        SemanticSummary::ConnectiveBody(values) => {
            RootReturnedState::ConnectiveBody(values.iter().map(par_state).collect())
        }
        SemanticSummary::VarRef { index, depth } => RootReturnedState::VarRef {
            index: *index,
            depth: *depth,
        },
        SemanticSummary::Bytes(value) => RootReturnedState::Bytes(value.bytes().collect()),
        SemanticSummary::Unit => RootReturnedState::Unit,
    }
}

fn path_wait_state(waiting: &PathObligationWait) -> PathWaitState {
    PathWaitState {
        id: waiting.id,
        kind: waiting.kind,
    }
}

fn map_state(frame: &MapFrame<'_>) -> MapFrameState {
    MapFrameState {
        owner: frame.owner,
        outer_field: frame.outer_field,
        value_type_id: frame.value_type_id,
        body: frame.region.bytes().collect(),
        region_start: frame.region_start,
        region_end: frame.region.end_position().physical,
        cursor: frame.cursor.stats(),
        key: frame.key.as_ref().map(|key| key.bytes().collect()),
        key_position: frame.key_position,
        saw_value: frame.saw_value,
        value_position: frame.value_position,
        value: frame.value.as_ref().map(par_state),
        waiting_value: frame.waiting_value,
    }
}

fn map_summary_state(summary: &MapSummary<'_>) -> RootMapState {
    RootMapState {
        key: summary.key.bytes().collect(),
        key_start: summary.key.position_at(0).unwrap().physical,
        key_end: summary.key.end_position().physical,
        key_position: summary.key_position,
        value: summary.value.as_ref().map(par_state),
    }
}

fn epathmap_state(frame: &EPathMapFrame<'_>) -> EPathMapFrameState {
    EPathMapFrameState {
        owner: frame.owner,
        field: frame.field,
        snapshot: frame.snapshot.bytes().collect(),
        region_start: frame.region_start,
        region_end: frame.snapshot.end_position().physical,
        mode: frame.mode,
        cursor: frame.cursor.stats(),
        path: frame.path.stats(),
        active_obligation: frame.path.active_obligation(),
        entries_canonical_path_stable: frame.entries_canonical_path_stable,
        waiting_map_value: matches!(frame.waiting.as_ref(), Some(EPathMapWait::MapValue)),
        waiting_obligation: match frame.waiting.as_ref() {
            Some(EPathMapWait::Obligation(waiting)) => Some(path_wait_state(waiting)),
            _ => None,
        },
        last_source: frame.last_source,
    }
}

fn nonmessage_state(frame: &Frame<'_>) -> Option<NonMessageFrameState> {
    match frame {
        Frame::Message(_) => None,
        Frame::Map(frame) => Some(NonMessageFrameState::Map(map_state(frame))),
        Frame::EPathMap(frame) => Some(NonMessageFrameState::EPathMap(epathmap_state(frame))),
        Frame::CanonicalPath(frame) => Some(NonMessageFrameState::CanonicalPath(
            CanonicalPathFrameState {
                input: frame.input.bytes().collect(),
                region_start: frame.input.position_at(0).unwrap().physical,
                region_end: frame.input.end_position().physical,
                bytes_yielded: frame.cursor.bytes_yielded(),
                bytes_traversed: frame.cursor.bytes_traversed(),
                reducer: frame.reducer.stats(),
                parser_source: frame.reducer.parser_source_image(&frame.input),
                active_obligation: frame.reducer.active_obligation(),
                waiting: frame.waiting.as_ref().map(path_wait_state),
            },
        )),
    }
}

fn active_frame_states(frames: &[Frame<'_>]) -> Vec<ActiveFrameState> {
    frames
        .iter()
        .map(|frame| {
            let region = match frame {
                Frame::Message(message) => &message.body,
                Frame::Map(map) => &map.region,
                Frame::EPathMap(epm) => &epm.snapshot,
                Frame::CanonicalPath(path) => &path.input,
            };
            let bytes = region.bytes().collect();
            let anchors = (0..region.len())
                .map(|index| {
                    region
                        .position_at(index)
                        .expect("active frame byte owns its source position")
                        .physical
                })
                .collect();
            match frame {
                Frame::Message(message) => ActiveFrameState::Message {
                    frame: frame_state(message),
                    bytes,
                    anchors,
                },
                _ => ActiveFrameState::NonMessage {
                    frame: nonmessage_state(frame)
                        .expect("nonmessage frame has a state projection"),
                    bytes,
                    anchors,
                },
            }
        })
        .collect()
}

fn top_program(frame: &Frame<'_>) -> Option<&'static RhoMatchProgram> {
    match frame {
        Frame::Message(message) => Some(message.program),
        _ => None,
    }
}

pub(super) fn driver_step_before(
    frames: &Vec<Frame<'_>>,
    scores: &CanonicalScoreArena<'_>,
) -> Option<Box<DriverStepBefore>> {
    TRACE.with(|slot| {
        slot.borrow().as_ref().map(|trace| {
            let top = frames.last().expect("admission step has a frame");
            Box::new(DriverStepBefore {
                frame_len: frames.len(),
                capacity: frames.capacity(),
                top_kind: top.request_kind(),
                top_program: top_program(top),
                score_len: scores.source_len(),
                score_source: matches!(top, Frame::Message(_)).then(|| scores.source_image()),
                message: match top {
                    Frame::Message(message) => Some(frame_state(message)),
                    _ => None,
                },
                nonmessage: nonmessage_state(top),
                epm_path_source: match top {
                    Frame::EPathMap(epm) => Some(epm.path.batch_exact_source_observation()),
                    _ => None,
                },
                active_frames: trace
                    .full_active_frames
                    .then(|| active_frame_states(frames)),
            })
        })
    })
}

pub(super) fn record_driver_step(
    before: Option<Box<DriverStepBefore>>,
    kind: DriverStepKind,
    frames: &Vec<Frame<'_>>,
    scores: &CanonicalScoreArena<'_>,
) {
    if let Some(before) = before {
        with_trace(|trace| {
            let score_len_after = scores.source_len();
            let message_continue =
                matches!(kind, DriverStepKind::Continue) && before.message.is_some();
            let score_source_after = message_continue.then(|| scores.source_image());
            let score_appended = if score_len_after > before.score_len {
                let mut source_image = scores.source_image();
                source_image.split_off(before.score_len)
            } else {
                Vec::new()
            };
            let active_frames_after = trace
                .full_active_frames
                .then(|| active_frame_states(frames));
            trace.driver_steps.push(DriverStepState {
                kind,
                before_len: before.frame_len,
                after_len: frames.len(),
                before_capacity: before.capacity,
                after_capacity: frames.capacity(),
                before_top_kind: before.top_kind,
                after_top_kind: frames.last().map(Frame::request_kind),
                before_top_program: before.top_program,
                after_top_program: frames.last().and_then(top_program),
                after_top_region: frames.last().and_then(|frame| match frame {
                    Frame::Message(message) => Some((message.region_start, message.body.len())),
                    Frame::Map(map) => Some((map.region_start, map.region.len())),
                    Frame::EPathMap(epm) => Some((epm.region_start, epm.snapshot.len())),
                    Frame::CanonicalPath(_) => None,
                }),
                score_len_before: before.score_len,
                score_len_after,
                score_appended,
                score_source_before: if message_continue {
                    before.score_source
                } else {
                    None
                },
                score_source_after,
                before_message: before.message,
                after_message: match frames.last() {
                    Some(Frame::Message(message)) => Some(frame_state(message)),
                    _ => None,
                },
                before_nonmessage: before.nonmessage,
                epm_path_source_before: before.epm_path_source,
                after_nonmessage: frames.last().and_then(nonmessage_state),
                suspended_parent_after: if matches!(kind, DriverStepKind::Push(_)) {
                    frames.get(frames.len() - 2).and_then(nonmessage_state)
                } else {
                    None
                },
                active_frames_before: before.active_frames,
                active_frames_after,
            });
        });
    }
}

pub(super) fn record_message_field(
    frame: &MessageFrame<'_>,
    source: &MessageFieldSourceReceipt,
    field: &IndexedCanonicalField<'_>,
) {
    with_trace(|trace| {
        let body: Vec<u8> = source.body.bytes().collect();
        assert_eq!(body, frame.body.bytes().collect::<Vec<_>>());
        let body_positions = (0..body.len())
            .map(|at| {
                source
                    .body
                    .position_at(at)
                    .expect("every traced body byte has a physical position")
                    .physical
            })
            .collect::<Vec<_>>();
        for (at, &anchor) in body_positions.iter().enumerate() {
            assert_eq!(
                frame.body.position_at(at).unwrap().physical,
                anchor,
                "pre-mutation and retained indexed bodies differ at logical byte {at}"
            );
        }
        assert_eq!(
            source.body.end_position().physical,
            frame.body.end_position().physical
        );
        trace.message_fields.push(MessageFieldState {
            step_index: trace.driver_steps.len(),
            program: frame.program,
            body,
            body_positions,
            body_end_position: source.body.end_position().physical,
            frame_after: frame_state(frame),
            tag: source.field_number,
            field_position: field.field_start.physical,
            indexed_field_position: frame
                .body
                .position_at(source.encoded_range.start)
                .expect("traced field starts inside its message")
                .physical,
            wire: source.wire,
            encoded_range: source.encoded_range,
            decoded_varint: match &source.field.value {
                IndexedCanonicalFieldValue::Varint(value) => Some(*value),
                _ => None,
            },
            payload_view_bytes: match &source.field.value {
                IndexedCanonicalFieldValue::Varint(_) => None,
                IndexedCanonicalFieldValue::Fixed64(region)
                | IndexedCanonicalFieldValue::LengthDelimited(region)
                | IndexedCanonicalFieldValue::Fixed32(region) => Some(region.bytes().collect()),
            },
            slot: source.resolved.slot(),
            oneof_arm: source.oneof_arm,
            slot_count: source.slot_count,
            last_slot_before: source.last_slot_before,
            last_slot_after: source.last_slot_after,
            seen_before: source.seen_before,
            seen_after: source.seen_after,
            slot_occurrences_before: source.slot_occurrences_before,
            slot_occurrences_after: source.slot_occurrences_after,
            waiting_after: source.waiting_after,
            child_kind: source.child_kind,
            child_type_id: source.child_type_id,
            child_region_start: source.child_region_start,
            child_region_len: source.child_region_len,
            payload_len: source.payload_len,
            payload_position: source.payload_position,
            cursor_before: source.cursor_before,
            cursor_after: source.cursor_after,
        });
    });
}

pub(super) fn record_message_field_success(
    source: &MessageFieldSourceReceipt<'_>,
    pushed: Option<&FramePushObservation>,
) {
    with_trace(|trace| {
        if !trace.message_success_stream {
            return;
        }
        let child_end = match (&source.field.value, pushed) {
            (IndexedCanonicalFieldValue::LengthDelimited(region), Some(_)) => {
                Some(region.end_position().physical)
            }
            _ => None,
        };
        trace.message_success.push(MessageSuccessState::Field {
            step_index: trace.driver_steps.len() - 1,
            program_type_id: source.program_type_id,
            field_number: source.field_number,
            slot: source.resolved.slot(),
            oneof_arm: source.oneof_arm,
            seen_before: source.seen_before,
            seen_after: source.seen_after,
            cursor_before: source.cursor_before,
            cursor_after: source.cursor_after,
            child_kind: source.child_kind,
            child_type_id: source.child_type_id,
            child_start: source.child_region_start,
            child_end,
            child_len: source.child_region_len,
            pushed: pushed.copied(),
        });
    });
}

pub(super) fn record_checked_admission_event(event: &CheckedAdmissionEvent<'_, '_>) {
    let kind = match &event.action {
        CheckedAdmissionAction::MessageField { pushed: None, .. }
        | CheckedAdmissionAction::OtherContinue => DriverStepKind::Continue,
        CheckedAdmissionAction::MessageField {
            pushed: Some(pushed),
            ..
        }
        | CheckedAdmissionAction::OtherPush { pushed, .. } => DriverStepKind::Push(**pushed),
        CheckedAdmissionAction::MessageComplete { .. }
        | CheckedAdmissionAction::OtherComplete { .. } => DriverStepKind::Complete,
    };
    with_trace(|trace| {
        if !trace.message_success_stream {
            return;
        }
        let step_index = trace.driver_steps.len() - 1;
        let step = &trace.driver_steps[step_index];
        assert_eq!(step.kind, kind);
        assert_eq!(step.before_top_kind, event.before.top_kind);
        assert_eq!(step.before_len, event.before.stack_len);
        assert_eq!(step.score_len_before, event.before.score_entries);
        assert_eq!(step.after_len, event.after_frames.len());
        assert_eq!(step.score_len_after, event.after_scores.source_len());
        assert_eq!(
            step.after_top_kind,
            event.after_frames.last().map(Frame::request_kind)
        );
        trace.checked_events.push(CheckedAdmissionEventState {
            step_index,
            kind,
            projection: event.projection(),
            before_top_kind: event.before.top_kind,
            after_top_kind: step.after_top_kind,
            before_len: event.before.stack_len,
            after_len: event.after_frames.len(),
            score_len_before: event.before.score_entries,
            score_len_after: event.after_scores.source_len(),
        });
    });
    match &event.action {
        CheckedAdmissionAction::MessageField { field, pushed } => {
            record_message_field_success(field, *pushed);
        }
        CheckedAdmissionAction::MessageComplete { eof, outcome, .. } => {
            record_message_eof_success(eof, outcome);
            if let CompleteAdmissionOutcome::AcceptedMessage(receipt) = outcome {
                record_message_accept_success(receipt);
            }
        }
        CheckedAdmissionAction::OtherComplete {
            outcome: CompleteAdmissionOutcome::AcceptedMessage(receipt),
            ..
        } => record_message_accept_success(receipt),
        CheckedAdmissionAction::OtherContinue
        | CheckedAdmissionAction::OtherPush { .. }
        | CheckedAdmissionAction::OtherComplete { .. } => {}
    }
}

pub(super) fn record_message_eof_success(
    source: &MessageEofSourceReceipt<'_>,
    outcome: &CompleteAdmissionOutcome<'_>,
) {
    with_trace(|trace| {
        if !trace.message_success_stream {
            return;
        }
        let step_index = trace.driver_steps.len() - 1;
        let complete_index = trace.message_completes.len() - 1;
        assert_eq!(
            trace.message_completes[complete_index].step_index,
            step_index
        );
        trace.message_success.push(MessageSuccessState::Eof {
            step_index,
            program_type_id: source.program_type_id,
            selected_kind: source.selected_kind,
            score_len_before: source.score_len_before,
            score_len_after: source.score_len_after,
            component: source.component,
            complete_index,
            root: matches!(outcome, CompleteAdmissionOutcome::Root(_)),
        });
    });
}

pub(super) fn record_message_accept_success(source: &MessageAcceptSourceReceipt<'_>) {
    with_trace(|trace| {
        if !trace.message_success_stream {
            return;
        }
        let step_index = trace.driver_steps.len() - 1;
        let pop_index = trace.pop_accepts.len() - 1;
        trace.message_success.push(MessageSuccessState::Accepted {
            step_index,
            program_type_id: source.before.program.type_id,
            slot: source.before.slot,
            child_kind: source.before.child_kind,
            child_body: source.before.child_body.bytes().collect(),
            child_start: source.before.child_body.position_at(0).unwrap().physical,
            child_end: source.before.child_body.end_position().physical,
            field_position: source.before.field_position,
            cursor_before: source.before.cursor,
            cursor_after: source.cursor_after,
            occurrences_before: source.before.occurrences,
            occurrences_after: source.occurrences_after,
            stack_len_before: source.before.stack_len,
            stack_len_after: source.stack_len_after,
            score_len_before_complete: source.before.score_len_before_complete,
            score_len_after_finish: source.before.score_len_after_finish,
            score_len_after_accept: source.score_len_after_accept,
            pop_index,
        });
    });
}

fn epm_varint(len: usize, at: &mut usize, byte_at: &mut impl FnMut(usize) -> u8) -> usize {
    let mut value = 0u128;
    for index in 0..10 {
        assert!(*at < len, "truncated EPM1 varint");
        let byte = byte_at(*at);
        *at += 1;
        value |= u128::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            assert!(index == 0 || byte & 0x7f != 0, "overlong EPM1 varint");
            return usize::try_from(value).expect("EPM1 varint fits usize");
        }
    }
    panic!("EPM1 varint exceeds ten bytes");
}

pub(super) fn epm_arena_range(len: usize, mut byte_at: impl FnMut(usize) -> u8) -> Range<usize> {
    assert!(len >= 6);
    for (index, expected) in b"EPM1".iter().enumerate() {
        assert_eq!(byte_at(index), *expected);
    }
    assert_eq!(byte_at(4), 1);
    let mut at = 6;
    let arena_len = epm_varint(len, &mut at, &mut byte_at);
    let end = at.checked_add(arena_len).expect("EPM1 arena length fits");
    assert!(end <= len);
    at..end
}

fn epm_value_body_ranges(snapshot: &[u8], arena_end: usize) -> Vec<Range<usize>> {
    let mut at = arena_end;
    let mut byte_at = |index| snapshot[index];
    let count = epm_varint(snapshot.len(), &mut at, &mut byte_at);
    assert!(count <= snapshot.len() - at);
    let mut ranges = Vec::with_capacity(count);
    for _ in 0..count {
        let body_len = epm_varint(snapshot.len(), &mut at, &mut byte_at);
        let end = at
            .checked_add(body_len)
            .expect("EPM1 value body length fits");
        assert!(end <= snapshot.len());
        ranges.push(at..end);
        at = end;
    }
    assert_eq!(at, snapshot.len());
    ranges
}

pub(super) fn assert_fragmented_epathmap_event_sources(
    trace: &RootFinishTrace,
    snapshot: &[u8],
    anchors: &[CanonicalByteProvenance],
    end_anchor: CanonicalByteProvenance,
) {
    assert_eq!(anchors.len(), snapshot.len());
    let arena = epm_arena_range(snapshot.len(), |index| snapshot[index]);
    let mode = EPathMapMode::try_from(snapshot[5]).expect("fragmented EPM1 has a valid mode");
    let value_bodies = epm_value_body_ranges(snapshot, arena.end);
    if arena.is_empty() {
        assert_eq!(mode, EPathMapMode::Empty);
        assert!(value_bodies.is_empty());
        assert_eq!(trace.epathmap_events.len(), 2);
        assert!(matches!(
            &trace.epathmap_events[0].kind,
            EPathMapEventKind::Value {
                has_value: false,
                value_body_range: None
            }
        ));
        assert!(matches!(
            &trace.epathmap_events[1].kind,
            EPathMapEventKind::Leave
        ));
        for event in &trace.epathmap_events {
            assert_eq!(event.after.snapshot.as_slice(), snapshot);
            assert_eq!(event.after.mode, mode);
            assert_eq!(event.source, anchors[5]);
        }
        return;
    }
    let mut cursor = CanonicalCompactTrieTopologyCursor::new(&snapshot[arena.clone()])
        .expect("fragmented EPM1 owns a canonical ACT arena");
    let mut topology = Vec::new();
    while let Some(event) = cursor.next_event() {
        topology.push(event);
    }
    let mut at = 0;
    let mut value_at = 0;
    for event in &trace.epathmap_events {
        assert_eq!(event.after.snapshot.as_slice(), snapshot);
        assert_eq!(event.after.mode, mode);
        let source_offset = match &event.kind {
            EPathMapEventKind::Forward {
                width,
                source_range,
            } => {
                assert!(*width > 0);
                assert_eq!(*width, source_range.len());
                for index in 0..*width {
                    let expected_offset = source_range.start + index;
                    let Some(CanonicalCompactTrieTopologyEvent::Edge { source_offset, .. }) =
                        topology.get(at)
                    else {
                        panic!("fragmented EPM1 forward edge is absent");
                    };
                    assert_eq!(*source_offset, expected_offset);
                    at += 1;
                    let Some(CanonicalCompactTrieTopologyEvent::Value {
                        value: None,
                        source_offset,
                    }) = topology.get(at)
                    else {
                        panic!("fragmented EPM1 forward empty value is absent");
                    };
                    assert_eq!(*source_offset, expected_offset);
                    at += 1;
                }
                source_range.end - 1
            }
            EPathMapEventKind::Unwind {
                width,
                source_range,
            } => {
                assert!(*width > 0);
                assert_eq!(*width, source_range.len());
                for index in 0..*width {
                    let Some(CanonicalCompactTrieTopologyEvent::Leave { source_offset }) =
                        topology.get(at)
                    else {
                        panic!("fragmented EPM1 unwind leave is absent");
                    };
                    assert_eq!(*source_offset, source_range.end - 1 - index);
                    at += 1;
                }
                source_range.start
            }
            EPathMapEventKind::Edge(byte) => {
                let Some(CanonicalCompactTrieTopologyEvent::Edge {
                    byte: expected,
                    source_offset,
                }) = topology.get(at)
                else {
                    panic!("fragmented EPM1 edge has no matching ACT event");
                };
                assert_eq!(byte, expected);
                at += 1;
                *source_offset
            }
            EPathMapEventKind::Leave => {
                let Some(CanonicalCompactTrieTopologyEvent::Leave { source_offset }) =
                    topology.get(at)
                else {
                    panic!("fragmented EPM1 leave has no matching ACT event");
                };
                at += 1;
                *source_offset
            }
            EPathMapEventKind::Value {
                has_value,
                value_body_range,
            } => {
                let Some(CanonicalCompactTrieTopologyEvent::Value {
                    value,
                    source_offset,
                }) = topology.get(at)
                else {
                    panic!("fragmented EPM1 value has no matching ACT event");
                };
                assert_eq!(*has_value, value.is_some());
                if value.is_some() && mode == EPathMapMode::Map {
                    assert_eq!(
                        value_body_range.as_ref(),
                        value_bodies.get(value_at),
                        "fragmented EPM1 map value follows the value table"
                    );
                    value_at += 1;
                } else {
                    assert!(value_body_range.is_none());
                }
                at += 1;
                *source_offset
            }
        };
        assert_eq!(event.source, anchors[arena.start + source_offset]);
        if let EPathMapEventKind::Value {
            value_body_range: Some(range),
            ..
        } = &event.kind
        {
            let body = trace
                .nonmessage_push_bodies
                .iter()
                .find(|body| body.step_index == event.step_index)
                .expect("fragmented EPM1 value pushes its exact body");
            assert_eq!(body.body.as_slice(), &snapshot[range.clone()]);
            assert_eq!(
                body.start,
                anchors.get(range.start).copied().unwrap_or(end_anchor)
            );
            assert_eq!(
                body.end,
                anchors.get(range.end).copied().unwrap_or(end_anchor)
            );
        }
    }
    assert_eq!(at, topology.len());
    assert_eq!(value_at, value_bodies.len());
}

pub(super) fn record_epathmap_event(
    frame: &EPathMapFrame<'_>,
    kind: EPathMapEventKind,
    source: CanonicalByteProvenance,
) {
    let value_reducer_stable = match &kind {
        EPathMapEventKind::Value { has_value, .. } => frame
            .path
            .value(
                *has_value,
                CanonicalPathSource::in_source(source.source, source.offset),
            )
            .map(|result| {
                result
                    .expect("recorded successful EPathMap Value has a reducer summary")
                    .canonical_path_stable
            }),
        _ => None,
    };
    let source_index = match &kind {
        EPathMapEventKind::Forward { source_range, .. } => Some(
            source_range
                .end
                .checked_sub(1)
                .expect("nonempty EPM1 forward span"),
        ),
        EPathMapEventKind::Unwind { source_range, .. } => Some(source_range.start),
        _ => None,
    };
    if let Some(source_index) = source_index {
        let arena_start = epm_arena_range(frame.snapshot.len(), |index| {
            frame.snapshot.byte_at(index).expect("EPM1 header byte")
        })
        .start;
        let expected = frame
            .snapshot
            .position_at(
                arena_start
                    .checked_add(source_index)
                    .expect("EPM1 batch source index fits"),
            )
            .expect("EPM1 batch source byte")
            .physical;
        assert_eq!(source, expected, "EPM1 batch source provenance");
    }
    with_trace(|trace| {
        trace.epathmap_events.push(EPathMapEventState {
            step_index: trace.driver_steps.len(),
            kind,
            source,
            after: epathmap_state(frame),
            path_source: frame.path.batch_exact_source_observation(),
            value_reducer_stable,
            cursor_source: frame
                .cursor
                .active_source_image()
                .expect("recorded EPathMap event retains its active cursor source"),
        });
    });
}

pub(super) fn record_epathmap_scalar_edge_no_child(receipt: &EPathMapScalarEdgeNoChildReceipt<'_>) {
    with_trace(|trace| {
        trace
            .epathmap_scalar_edge_no_child
            .push(EPathMapScalarEdgeNoChildState {
                step_index: trace.driver_steps.len(),
                byte: receipt.byte,
                provenance: receipt.provenance,
                cursor_before: receipt.before.cursor,
                cursor_after: receipt.after.cursor,
                path_before: receipt.before_path,
                path_after: receipt.after_path,
                parser_before: receipt
                    .parser_before
                    .clone()
                    .expect("opt-in Edge parser prestate"),
                parser_after: receipt
                    .parser_after
                    .clone()
                    .expect("opt-in Edge parser poststate"),
                no_child: receipt.no_child,
                no_map_value_receipt: receipt.no_map_value_receipt,
                owner_before: receipt.before.owner,
                owner_after: receipt.after.owner,
                field_before: receipt.before.field,
                field_after: receipt.after.field,
                region_start_before: receipt.before.region_start,
                region_start_after: receipt.after.region_start,
                snapshot_len_before: receipt.before.snapshot_len,
                snapshot_len_after: receipt.after.snapshot_len,
                waiting_before: receipt.before.waiting == EPathMapLoopWaitSource::None,
                waiting_after: receipt.after.waiting == EPathMapLoopWaitSource::None,
                waiting_child_len_before: receipt.before.waiting_child_len,
                waiting_child_len_after: receipt.after.waiting_child_len,
                entries_stable_before: receipt.before.entries_canonical_path_stable,
                entries_stable_after: receipt.after.entries_canonical_path_stable,
            });
    });
}

pub(super) fn record_canonical_path_byte(
    frame: &CanonicalPathFrame<'_>,
    index: usize,
    byte: crate::rust::rholang::canonical_bytes_arena::CanonicalLocatedByte,
    skipped: usize,
) {
    with_trace(|trace| {
        trace.canonical_path_bytes.push(CanonicalPathByteState {
            step_index: trace.driver_steps.len(),
            index,
            byte: byte.byte,
            source: byte.provenance,
            skipped,
            yielded_after: frame.cursor.bytes_yielded(),
            traversed_after: frame.cursor.bytes_traversed(),
            parser_source_after: frame.reducer.parser_source_image(&frame.input),
        });
    });
}

pub(super) fn record_nonmessage_push(frames: &[Frame<'_>], request: &FrameRequest<'_>) {
    let obligation = match frames.last() {
        Some(Frame::EPathMap(frame)) => match frame.waiting.as_ref() {
            Some(EPathMapWait::Obligation(waiting)) => Some(path_wait_state(waiting)),
            _ => None,
        },
        Some(Frame::CanonicalPath(frame)) => frame.waiting.as_ref().map(path_wait_state),
        Some(Frame::Map(_)) => None,
        _ => return,
    };
    let region = request.region();
    with_trace(|trace| {
        trace.nonmessage_push_bodies.push(NonMessagePushBodyState {
            step_index: trace.driver_steps.len(),
            body: region.bytes().collect(),
            start: region.position_at(0).unwrap().physical,
            end: region.end_position().physical,
            obligation,
        });
    });
}

pub(super) fn assert_nonmessage_event_projection(trace: &RootFinishTrace, label: &str) {
    use crate::rust::epathmap_trie_codec::{
        inspect_canonical_snapshot_value_regions, visit_canonical_snapshot_topology,
        CanonicalSnapshotTopologyEvent as TopologyEvent,
    };

    let mut epm_at = 0usize;
    let mut path_at = 0usize;
    let mut push_at = 0usize;
    let mut pop_at = 0usize;
    for (step_index, step) in trace.driver_steps.iter().enumerate() {
        let pushed =
            if matches!(step.kind, DriverStepKind::Push(_)) && step.before_nonmessage.is_some() {
                let item = trace
                    .nonmessage_push_bodies
                    .get(push_at)
                    .unwrap_or_else(|| panic!("{label}: non-message push lacks a body"));
                push_at += 1;
                assert_eq!(item.step_index, step_index, "{label}");
                assert_eq!(
                    step.after_top_region,
                    Some((item.start, item.body.len())),
                    "{label}"
                );
                if let Some(child) = step.after_message.as_ref() {
                    assert_eq!(child.region_start, item.start, "{label}");
                    assert_eq!(child.region_end, item.end, "{label}");
                    assert_eq!(child.region_len, item.body.len(), "{label}");
                }
                Some(item)
            } else {
                None
            };
        let completed = if matches!(step.kind, DriverStepKind::Complete) {
            let pop = &trace.pop_accepts[pop_at];
            pop_at += 1;
            pop.child_nonmessage.as_ref()
        } else {
            None
        };
        if let Some(NonMessageFrameState::EPathMap(before)) = step.before_nonmessage.as_ref() {
            let after = match step.kind {
                DriverStepKind::Push(_) => step.suspended_parent_after.as_ref(),
                DriverStepKind::Complete => completed,
                DriverStepKind::Continue => step.after_nonmessage.as_ref(),
            };
            let Some(NonMessageFrameState::EPathMap(after)) = after else {
                panic!("{label}: EPathMap step lost its frame snapshot");
            };
            let mut topology = Vec::new();
            let (mode, _) =
                visit_canonical_snapshot_topology(&before.snapshot, |event| topology.push(event))
                    .unwrap_or_else(|error| {
                        panic!("{label}: EPM1 source topology failed: {error}")
                    });
            let regions = inspect_canonical_snapshot_value_regions(&before.snapshot)
                .unwrap_or_else(|error| panic!("{label}: EPM1 source regions failed: {error}"));
            assert_eq!(mode, before.mode, "{label}");
            assert_eq!(regions.mode, mode, "{label}");
            assert_eq!(after.snapshot, before.snapshot, "{label}");
            let arena_start =
                epm_arena_range(before.snapshot.len(), |index| before.snapshot[index]).start;
            let mut event_index = before.cursor.topology_events;
            let mut value_index = before.cursor.value_records;
            assert!(event_index <= topology.len(), "{label}");
            while trace
                .epathmap_events
                .get(epm_at)
                .is_some_and(|event| event.step_index == step_index)
            {
                let event = &trace.epathmap_events[epm_at];
                epm_at += 1;
                assert_eq!(event.after.snapshot, before.snapshot, "{label}");
                assert_eq!(
                    event.after.last_source.source, event.source.source,
                    "{label}"
                );
                assert_eq!(
                    event.after.last_source.offset, event.source.offset,
                    "{label}"
                );
                let delta = match &event.kind {
                    EPathMapEventKind::Forward {
                        width,
                        source_range,
                    } => {
                        assert!(*width > 0 && *width == source_range.len(), "{label}");
                        for pair in topology[event_index..event_index + width * 2].chunks_exact(2) {
                            assert!(
                                matches!(pair, [
                                    TopologyEvent::Edge(_),
                                    TopologyEvent::Value(false)
                                ]),
                                "{label}"
                            );
                        }
                        width * 2
                    }
                    EPathMapEventKind::Unwind {
                        width,
                        source_range,
                    } => {
                        assert!(*width > 0 && *width == source_range.len(), "{label}");
                        assert!(
                            topology[event_index..event_index + width]
                                .iter()
                                .all(|item| *item == TopologyEvent::Leave),
                            "{label}"
                        );
                        *width
                    }
                    EPathMapEventKind::Edge(byte) => {
                        assert_eq!(topology[event_index], TopologyEvent::Edge(*byte), "{label}");
                        1
                    }
                    EPathMapEventKind::Leave => {
                        assert_eq!(topology[event_index], TopologyEvent::Leave, "{label}");
                        1
                    }
                    EPathMapEventKind::Value {
                        has_value,
                        value_body_range,
                    } => {
                        assert_eq!(
                            topology[event_index],
                            TopologyEvent::Value(*has_value),
                            "{label}"
                        );
                        if let Some(range) = value_body_range {
                            assert_eq!(regions.mode, EPathMapMode::Map, "{label}");
                            assert_eq!(
                                regions.value_bodies.get(value_index),
                                Some(range),
                                "{label}"
                            );
                            assert!(*has_value, "{label}");
                            value_index += 1;
                        } else if regions.mode == EPathMapMode::Map && *has_value {
                            panic!("{label}: map value has no indexed source region");
                        }
                        1
                    }
                };
                event_index += delta;
                assert_eq!(event.after.cursor.topology_events, event_index, "{label}");
                assert!(
                    event.after.cursor.topology_events <= topology.len(),
                    "{label}"
                );
                assert_eq!(event.after.cursor.value_records, value_index, "{label}");
                if before.region_start.source == before.region_end.source
                    && before
                        .region_start
                        .offset
                        .checked_add(before.snapshot.len())
                        == Some(before.region_end.offset)
                {
                    assert_eq!(event.source.source, before.region_start.source, "{label}");
                    let expected = match &event.kind {
                        EPathMapEventKind::Forward { source_range, .. } => {
                            Some(arena_start + source_range.end - 1)
                        }
                        EPathMapEventKind::Unwind { source_range, .. } => {
                            Some(arena_start + source_range.start)
                        }
                        _ => None,
                    };
                    if let Some(offset) = expected {
                        assert_eq!(
                            event.source.offset,
                            before.region_start.offset + offset,
                            "{label}"
                        );
                    } else {
                        assert!(event.source.offset >= before.region_start.offset, "{label}");
                        assert!(event.source.offset <= before.region_end.offset, "{label}");
                    }
                }
            }
            assert_eq!(event_index, after.cursor.topology_events, "{label}");
            if !matches!(step.kind, DriverStepKind::Complete)
                && event_index > before.cursor.topology_events
            {
                assert_eq!(trace.epathmap_events[epm_at - 1].after, *after, "{label}");
            }
            if matches!(step.kind, DriverStepKind::Complete) {
                assert_eq!(event_index, topology.len(), "{label}");
                assert_eq!(value_index, regions.value_bodies.len(), "{label}");
                assert_eq!(
                    after.cursor.terminal_checks,
                    before.cursor.terminal_checks + 1,
                    "{label}"
                );
            }
            assert_eq!(step.score_len_before, step.score_len_after, "{label}");
            if let Some(body) = pushed {
                assert_eq!(body.obligation, after.waiting_obligation, "{label}");
                if after.waiting_map_value {
                    assert!(body.obligation.is_none(), "{label}");
                    let source = trace.epathmap_events[epm_at - 1].kind.clone();
                    let EPathMapEventKind::Value {
                        value_body_range: Some(range),
                        ..
                    } = source
                    else {
                        panic!("{label}: map value push did not follow a value event");
                    };
                    assert_eq!(
                        body.body.as_slice(),
                        &before.snapshot[range.clone()],
                        "{label}"
                    );
                    assert_eq!(
                        step.after_top_kind,
                        Some(FrameRequestKind::Message),
                        "{label}"
                    );
                    assert!(
                        std::ptr::eq(
                            step.after_top_program.unwrap(),
                            &PAR_PROTOBUF_REGION_PROGRAM
                        ),
                        "{label}"
                    );
                    if before.region_start.source == before.region_end.source
                        && before
                            .region_start
                            .offset
                            .checked_add(before.snapshot.len())
                            == Some(before.region_end.offset)
                    {
                        assert_eq!(
                            body.start,
                            CanonicalByteProvenance::new(
                                before.region_start.source,
                                before.region_start.offset + range.start
                            ),
                            "{label}"
                        );
                    }
                } else if let Some(wait) = body.obligation {
                    assert_eq!(after.waiting_obligation, Some(wait), "{label}");
                    assert_eq!(
                        step.after_top_kind,
                        Some(match wait.kind {
                            CanonicalPathObligationKind::RawParEscape => FrameRequestKind::Message,
                            CanonicalPathObligationKind::NestedEpm1 => FrameRequestKind::EPathMap,
                        }),
                        "{label}"
                    );
                }
            }
        }
        if let Some(NonMessageFrameState::CanonicalPath(before)) = step.before_nonmessage.as_ref() {
            let after = match step.kind {
                DriverStepKind::Push(_) => step.suspended_parent_after.as_ref(),
                DriverStepKind::Complete => completed,
                DriverStepKind::Continue => step.after_nonmessage.as_ref(),
            };
            let Some(NonMessageFrameState::CanonicalPath(after)) = after else {
                panic!("{label}: canonical path step lost its frame snapshot");
            };
            assert_eq!(before.input, after.input, "{label}");
            let mut next = before.bytes_yielded;
            while trace
                .canonical_path_bytes
                .get(path_at)
                .is_some_and(|event| event.step_index == step_index)
            {
                let event = &trace.canonical_path_bytes[path_at];
                path_at += 1;
                assert_eq!(event.index, next, "{label}");
                assert_eq!(before.input[event.index], event.byte, "{label}");
                next = event.index + 1 + event.skipped;
                assert!(next <= before.input.len(), "{label}");
                assert_eq!(event.yielded_after, next, "{label}");
                assert!(event.traversed_after <= event.yielded_after, "{label}");
                if before.region_start.source == before.region_end.source
                    && before.region_start.offset.checked_add(before.input.len())
                        == Some(before.region_end.offset)
                {
                    assert_eq!(event.source.source, before.region_start.source, "{label}");
                    assert_eq!(
                        event.source.offset,
                        before.region_start.offset + event.index,
                        "{label}"
                    );
                }
            }
            assert_eq!(next, after.bytes_yielded, "{label}");
            assert_eq!(step.score_len_before, step.score_len_after, "{label}");
            if let Some(body) = pushed {
                assert_eq!(body.obligation, after.waiting, "{label}");
                match body
                    .obligation
                    .expect("canonical path push has an obligation")
                    .kind
                {
                    CanonicalPathObligationKind::RawParEscape => {
                        assert_eq!(
                            step.after_top_kind,
                            Some(FrameRequestKind::Message),
                            "{label}"
                        );
                        assert!(
                            std::ptr::eq(
                                step.after_top_program.unwrap(),
                                &PAR_PROTOBUF_REGION_PROGRAM
                            ),
                            "{label}"
                        );
                    }
                    CanonicalPathObligationKind::NestedEpm1 => {
                        assert_eq!(
                            step.after_top_kind,
                            Some(FrameRequestKind::EPathMap),
                            "{label}"
                        );
                        assert!(step.after_top_program.is_none(), "{label}");
                    }
                }
                assert!(
                    body.body.is_empty()
                        || before
                            .input
                            .windows(body.body.len())
                            .any(|window| window == body.body),
                    "{label}"
                );
            }
            if matches!(step.kind, DriverStepKind::Complete) {
                assert_eq!(after.bytes_yielded, after.input.len(), "{label}");
            }
        }
    }
    assert_eq!(epm_at, trace.epathmap_events.len(), "{label}");
    assert_eq!(path_at, trace.canonical_path_bytes.len(), "{label}");
    assert_eq!(push_at, trace.nonmessage_push_bodies.len(), "{label}");
}

#[test]
fn standalone_canonical_path_event_projection_uses_generated_par_body() {
    let generated_par =
        crate::rust::rholang::protobuf_encoder::encode_to_vec(&crate::rhoapi::Par::default());
    let mut escaped = vec![crate::rust::canonical_path::tag::ESCAPE];
    prost::encoding::encode_varint(generated_par.len() as u64, &mut escaped);
    let empty_body_start = escaped.len();
    escaped.extend_from_slice(&generated_par);
    let nonempty_par = crate::rust::rholang::protobuf_encoder::encode_to_vec(
        &crate::rhoapi::Par::default().with_connective_used(true),
    );
    let mut nonempty_escape = vec![crate::rust::canonical_path::tag::ESCAPE];
    prost::encoding::encode_varint(nonempty_par.len() as u64, &mut nonempty_escape);
    let nonempty_body_start = nonempty_escape.len();
    nonempty_escape.extend_from_slice(&nonempty_par);
    let ordinary = vec![crate::rust::canonical_path::tag::GBYTE_ARRAY, 1, b'x'];
    for (label, key, expected_body_start) in [
        (
            "generated empty Par escape",
            escaped,
            Some(empty_body_start),
        ),
        (
            "generated nonempty Par escape",
            nonempty_escape,
            Some(nonempty_body_start),
        ),
        ("canonical byte-array leaf", ordinary, None),
    ] {
        let expected_pushes = usize::from(expected_body_start.is_some());
        start();
        validate_canonical_path_indexed(IndexedByteRegion::from_borrowed(&key, 0, 0))
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        let trace = take();
        assert_nonmessage_event_projection(&trace, label);
        assert_eq!(
            trace.nonmessage_push_bodies.len(),
            expected_pushes,
            "{label}"
        );
        assert!(!trace.canonical_path_bytes.is_empty(), "{label}");

        let anchors = (0..key.len())
            .map(|index| CanonicalByteProvenance::new(300 + index as u32, 700 + index * 17))
            .collect::<Vec<_>>();
        let terminal = CanonicalByteProvenance::new(900, 9_000);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        for (&byte, &anchor) in key.iter().zip(&anchors) {
            handle = arena
                .append(handle, CanonicalLocatedByte::new(byte, anchor))
                .unwrap();
        }
        start();
        validate_canonical_path_indexed(OwnedIndexedByteRegion::from_frozen(
            arena.freeze(handle),
            terminal,
        ))
        .unwrap_or_else(|error| panic!("{label}: {error}"));
        let fragmented = take();
        assert_nonmessage_event_projection(&fragmented, label);
        assert_eq!(fragmented.nonmessage_push_bodies.len(), expected_pushes);
        let root_end = fragmented
            .driver_steps
            .first()
            .and_then(|step| step.before_nonmessage.as_ref())
            .and_then(|frame| match frame {
                NonMessageFrameState::CanonicalPath(path) => Some(path.region_end),
                _ => None,
            });
        assert_eq!(root_end, Some(terminal), "{label}");
        for event in &fragmented.canonical_path_bytes {
            assert_eq!(event.source, anchors[event.index], "{label}");
        }
        if let Some(body_start) = expected_body_start {
            let body = &fragmented.nonmessage_push_bodies[0];
            assert_eq!(body.body.as_slice(), &key[body_start..]);
            assert_eq!(
                body.start,
                anchors
                    .get(body_start)
                    .copied()
                    .unwrap_or_else(|| *anchors.last().expect("escape has a length byte"))
            );
            // The reducer completes this child at its last payload byte, or
            // at the length byte when the generated Par payload is empty.
            // The outer root's terminal anchor remains separate above.
            assert_eq!(body.end, *anchors.last().expect("escape has a length byte"));
        }
    }
}

pub(super) fn assert_driver_step_trace(
    trace: &RootFinishTrace,
    root_program: &'static RhoMatchProgram,
) {
    let first = trace
        .driver_steps
        .first()
        .expect("traced admission has a driver step");
    assert_eq!(first.before_len, 1);
    assert!(first
        .before_top_program
        .is_some_and(|program| std::ptr::eq(program, root_program)));
    let frame_bytes = std::mem::size_of::<Frame<'_>>();
    let mut prior: Option<&DriverStepState> = None;
    for step in &trace.driver_steps {
        assert!(step.before_len > 0);
        assert!(step.before_len <= step.before_capacity);
        assert!(step.after_len <= step.after_capacity);
        assert!(step.before_len.checked_mul(frame_bytes).is_some());
        assert!(step.before_capacity.checked_mul(frame_bytes).is_some());
        assert!(step.after_len.checked_mul(frame_bytes).is_some());
        assert!(step.after_capacity.checked_mul(frame_bytes).is_some());
        assert!(step.score_len_before <= step.score_len_after);
        if let Some(previous) = prior {
            assert_eq!(step.before_len, previous.after_len);
            assert_eq!(step.before_capacity, previous.after_capacity);
            assert_eq!(step.score_len_before, previous.score_len_after);
            assert_eq!(Some(step.before_top_kind), previous.after_top_kind);
            match (step.before_top_program, previous.after_top_program) {
                (None, None) => {}
                (Some(left), Some(right)) => assert!(std::ptr::eq(left, right)),
                _ => panic!("driver changed the top program between steps"),
            }
        }
        for program in [step.before_top_program, step.after_top_program]
            .into_iter()
            .flatten()
        {
            assert!(semantic_kind(program).is_some());
            if program.type_id == PAR_PROTOBUF_REGION_PROGRAM.type_id {
                assert_eq!(
                    matches!(semantic_kind(program), Some(SemanticKind::RawPar)),
                    std::ptr::eq(program, &PAR_PROTOBUF_REGION_PROGRAM),
                );
            }
        }
        match step.kind {
            DriverStepKind::Continue => {
                assert_eq!(step.after_len, step.before_len);
                assert_eq!(step.after_top_kind, Some(step.before_top_kind));
            }
            DriverStepKind::Push(observed) => {
                assert_eq!(step.before_len.checked_add(1), Some(step.after_len));
                assert_eq!(observed.before_len, step.before_len);
                assert_eq!(observed.after_len, step.after_len);
                assert_eq!(observed.before_capacity, step.before_capacity);
                assert_eq!(observed.after_capacity, step.after_capacity);
                assert_eq!(observed.request_kind, step.after_top_kind.unwrap());
                assert_eq!(observed.frame_variant_bytes, frame_bytes);
                assert_eq!(observed.stats_frame_variant_bytes, frame_bytes);
                assert_eq!(observed.used_bytes, step.after_len * frame_bytes);
                assert_eq!(observed.reserved_bytes, step.after_capacity * frame_bytes);
            }
            DriverStepKind::Complete => {
                assert_eq!(step.after_len.checked_add(1), Some(step.before_len));
                if step.after_len == 0 {
                    assert_eq!(step.after_top_kind, None);
                    assert!(step.after_top_program.is_none());
                }
            }
        }
        prior = Some(step);
    }
    assert_eq!(trace.driver_steps.last().unwrap().after_len, 0);
}

// This checks the observed production actions against the encoded field and
// the generated table, without calling the admission field resolver or cursor.
pub(super) fn assert_descriptor_driver_projection(
    trace: &RootFinishTrace,
    root_program: &'static RhoMatchProgram,
    label: &str,
) -> (usize, usize) {
    fn varint(body: &[u8], at: &mut usize) -> u64 {
        let mut value = 0u64;
        for shift in (0..=63).step_by(7) {
            let byte = *body.get(*at).expect("traced field is inside its message");
            *at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return value;
            }
        }
        panic!("traced field has an overlong varint");
    }
    fn scalar_from_wire(
        kind: ProtobufKind,
        payload: &[u8],
        decoded_varint: Option<u64>,
    ) -> RootScalarState {
        let varint = || decoded_varint.expect("scalar kind requires a varint wire value");
        match kind {
            ProtobufKind::Bool => RootScalarState::Bool(varint() != 0),
            ProtobufKind::Int32 => RootScalarState::I32(varint() as u32 as i32),
            ProtobufKind::Sint32 => {
                let encoded = varint() as u32;
                RootScalarState::I32(((encoded >> 1) as i32) ^ -((encoded & 1) as i32))
            }
            ProtobufKind::Sfixed32 => {
                RootScalarState::I32(i32::from_le_bytes(payload.try_into().unwrap()))
            }
            ProtobufKind::Uint32 => RootScalarState::U32(varint() as u32),
            ProtobufKind::Fixed32 | ProtobufKind::Float => {
                RootScalarState::U32(u32::from_le_bytes(payload.try_into().unwrap()))
            }
            ProtobufKind::Int64 => RootScalarState::I64(varint() as i64),
            ProtobufKind::Sint64 => {
                let encoded = varint();
                RootScalarState::I64(((encoded >> 1) as i64) ^ -((encoded & 1) as i64))
            }
            ProtobufKind::Sfixed64 => {
                RootScalarState::I64(i64::from_le_bytes(payload.try_into().unwrap()))
            }
            ProtobufKind::Uint64 => RootScalarState::U64(varint()),
            ProtobufKind::Fixed64 | ProtobufKind::Double => {
                RootScalarState::U64(u64::from_le_bytes(payload.try_into().unwrap()))
            }
            ProtobufKind::String | ProtobufKind::RepeatedString => {
                RootScalarState::String(payload.to_vec())
            }
            ProtobufKind::Bytes | ProtobufKind::RepeatedBytes => {
                RootScalarState::Bytes(payload.to_vec())
            }
            _ => panic!("message and map payloads cannot take scalar Continue"),
        }
    }
    fn frame_matches_stats(frame: &RootFrameState, stats: IndexedCanonicalFieldCursorStats) {
        assert_eq!(frame.cursor_fields_read, stats.fields_read);
        assert_eq!(frame.cursor_bytes_read, stats.bytes_read);
        assert_eq!(frame.cursor_bytes_traversed, stats.bytes_traversed);
        assert_eq!(
            frame.cursor_bytes_fast_forwarded,
            stats.bytes_fast_forwarded
        );
        assert_eq!(
            frame.cursor_bytes_read - frame.cursor_bytes_traversed,
            frame.cursor_bytes_fast_forwarded,
        );
    }
    fn expected_program(program: &'static RhoMatchProgram) -> &'static RhoMatchProgram {
        let generated = wire_program_by_type_id(program.type_id).unwrap();
        if std::ptr::eq(program, &PAR_PROTOBUF_REGION_PROGRAM) {
            assert!(std::ptr::eq(generated, &PAR_PROTOBUF_REGION_PROGRAM));
        } else {
            assert!(std::ptr::eq(program, generated));
        }
        generated
    }

    assert_driver_step_trace(trace, root_program);
    assert_nonmessage_event_projection(trace, label);
    assert_eq!(trace.driver_steps[0].score_len_before, 0, "{label}");
    let entries = trace
        .score_arena
        .as_ref()
        .expect("successful root has a score arena");
    let transcripts = replay_score_source(entries);
    assert_eq!(
        trace.driver_steps.last().unwrap().score_len_after,
        entries.len(),
        "{label}",
    );
    let mut field_at = 0usize;
    let mut pop_at = 0usize;
    let mut epm_accept_at = 0usize;
    let mut message_eof_at = 0usize;
    let mut message_complete_at = 0usize;
    let mut map_eof_at = 0usize;
    let mut map_complete_at = 0usize;
    let mut nonmessage_stack = vec![trace.driver_steps[0].before_nonmessage.clone()];
    for (step_index, step) in trace.driver_steps.iter().enumerate() {
        assert_eq!(
            nonmessage_stack.last(),
            Some(&step.before_nonmessage),
            "{label}"
        );
        assert!(step.score_len_before <= step.score_len_after, "{label}");
        assert!(step.score_len_after <= transcripts.len(), "{label}");
        assert_eq!(
            step.score_appended.as_slice(),
            &entries[step.score_len_before..step.score_len_after],
            "{label}",
        );
        let message_continue =
            matches!(step.kind, DriverStepKind::Continue) && step.before_message.is_some();
        assert_eq!(
            step.score_source_before.is_some(),
            message_continue,
            "{label}"
        );
        assert_eq!(
            step.score_source_after.is_some(),
            message_continue,
            "{label}"
        );
        if message_continue {
            let score_before = step.score_source_before.as_ref().unwrap();
            let score_after = step.score_source_after.as_ref().unwrap();
            assert_eq!(score_before.len(), step.score_len_before, "{label}");
            assert_eq!(score_after.len(), step.score_len_after, "{label}");
            assert_eq!(score_before, score_after, "{label}");
            assert_eq!(
                score_after.as_slice(),
                &entries[..step.score_len_after],
                "{label}"
            );
        }
        if !matches!(step.kind, DriverStepKind::Complete) {
            assert!(step.score_appended.is_empty(), "{label}");
        }
        if let Some(program) = step.before_top_program {
            expected_program(program);
            assert_eq!(step.before_top_kind, FrameRequestKind::Message, "{label}");
            assert!(
                step.before_message
                    .as_ref()
                    .is_some_and(|frame| std::ptr::eq(frame.program, program)),
                "{label}"
            );
            assert_eq!(
                step.before_message
                    .as_ref()
                    .map(|frame| frame.program_type_id),
                Some(program.type_id),
                "{label}",
            );
        } else {
            assert!(step.before_message.is_none(), "{label}");
            assert!(step.before_nonmessage.is_some(), "{label}");
        }
        if let Some(program) = step.after_top_program {
            expected_program(program);
            assert_eq!(
                step.after_top_kind,
                Some(FrameRequestKind::Message),
                "{label}"
            );
            assert!(
                step.after_message
                    .as_ref()
                    .is_some_and(|frame| std::ptr::eq(frame.program, program)),
                "{label}"
            );
            assert_eq!(
                step.after_message
                    .as_ref()
                    .map(|frame| frame.program_type_id),
                Some(program.type_id),
                "{label}",
            );
        } else {
            assert!(step.after_message.is_none(), "{label}");
            assert_eq!(
                step.after_nonmessage.is_some(),
                step.after_len > 0,
                "{label}"
            );
        }
        assert_eq!(
            step.before_message.is_some(),
            step.before_nonmessage.is_none(),
            "{label}"
        );
        assert!(
            !(step.after_message.is_some() && step.after_nonmessage.is_some()),
            "{label}"
        );
        assert_eq!(
            step.after_message.is_some() || step.after_nonmessage.is_some(),
            step.after_len > 0,
            "{label}",
        );
        if let Some(previous) = step_index
            .checked_sub(1)
            .and_then(|at| trace.driver_steps.get(at))
        {
            assert_eq!(step.before_message, previous.after_message, "{label}");
            assert_eq!(step.before_nonmessage, previous.after_nonmessage, "{label}");
        }
        if let Some(frame) = step.before_nonmessage.as_ref() {
            match frame {
                NonMessageFrameState::Map(map) => {
                    assert_eq!(step.before_top_kind, FrameRequestKind::Map, "{label}");
                    assert!(
                        std::ptr::eq(
                            map.owner,
                            wire_program_by_type_id(map.owner.type_id).unwrap()
                        ),
                        "{label}"
                    );
                    assert!(map.cursor.bytes_read <= map.body.len(), "{label}");
                    assert_eq!(map.key.is_some(), map.key_position.is_some(), "{label}");
                    assert_eq!(
                        map.waiting_value,
                        map.saw_value && map.value.is_none(),
                        "{label}"
                    );
                }
                NonMessageFrameState::EPathMap(epm) => {
                    assert_eq!(step.before_top_kind, FrameRequestKind::EPathMap, "{label}");
                    assert!(
                        std::ptr::eq(
                            epm.owner,
                            wire_program_by_type_id(epm.owner.type_id).unwrap()
                        ),
                        "{label}"
                    );
                    assert!(
                        !(epm.waiting_map_value && epm.waiting_obligation.is_some()),
                        "{label}"
                    );
                }
                NonMessageFrameState::CanonicalPath(path) => {
                    assert_eq!(
                        step.before_top_kind,
                        FrameRequestKind::CanonicalPath,
                        "{label}"
                    );
                    assert!(path.bytes_yielded <= path.input.len(), "{label}");
                    assert!(path.bytes_traversed <= path.bytes_yielded, "{label}");
                }
            }
        }
        if let Some(frame) = step.before_message.as_ref() {
            assert_eq!(
                frame.cursor_empty,
                frame.cursor_bytes_read == frame.region_len,
                "{label}"
            );
            assert_eq!(
                frame.region_len.checked_add(0),
                Some(frame.region_len),
                "{label}"
            );
            assert!(frame.cursor_bytes_read <= frame.region_len, "{label}");
            assert_eq!(
                frame.last_slot.is_none(),
                frame.seen_slots.is_empty(),
                "{label}"
            );
            assert!(
                frame.seen_slots.windows(2).all(|pair| pair[0] < pair[1]),
                "{label}"
            );
            assert_eq!(frame.waiting, frame.waiting_detail.is_some(), "{label}");
            assert_eq!(
                frame.semantic_slots.len(),
                frame.program.fields.len(),
                "{label}"
            );
        }
        match step.kind {
            DriverStepKind::Continue | DriverStepKind::Push(_) if step.before_message.is_some() => {
                let event = trace
                    .message_fields
                    .get(field_at)
                    .unwrap_or_else(|| panic!("{label}: message step lacks a field observation"));
                assert_eq!(event.step_index, step_index, "{label}");
                field_at += 1;
                let before = step.before_message.as_ref().unwrap();
                assert!(
                    std::ptr::eq(step.before_top_program.unwrap(), event.program),
                    "{label}"
                );
                assert_eq!(
                    before.region_start, event.frame_after.region_start,
                    "{label}"
                );
                assert_eq!(before.region_end, event.frame_after.region_end, "{label}");
                assert_eq!(before.region_len, event.body.len(), "{label}");
                assert_eq!(event.body_positions.len(), event.body.len(), "{label}");
                assert_eq!(event.body_end_position, before.region_end, "{label}");
                assert!(!event.body_positions.is_empty(), "{label}");
                assert_eq!(event.body_positions[0], before.region_start, "{label}");
                expected_program(event.program);
                frame_matches_stats(before, event.cursor_before);
                frame_matches_stats(&event.frame_after, event.cursor_after);
                assert!(!before.waiting, "{label}");
                assert_eq!(
                    event.encoded_range.start, event.cursor_before.bytes_read,
                    "{label}"
                );
                assert_eq!(
                    event.encoded_range.end, event.cursor_after.bytes_read,
                    "{label}"
                );
                assert!(
                    event.encoded_range.start < event.body_positions.len(),
                    "{label}"
                );
                assert_eq!(
                    event.indexed_field_position, event.field_position,
                    "{label}: field must retain its indexed physical anchor"
                );
                assert_eq!(
                    event.body_positions[event.encoded_range.start], event.field_position,
                    "{label}: indexed field anchor must come from the dense body position image"
                );
                assert_eq!(
                    event.cursor_after.fields_read,
                    event.cursor_before.fields_read + 1,
                    "{label}"
                );
                assert!(
                    event.cursor_after.bytes_traversed >= event.cursor_before.bytes_traversed,
                    "{label}"
                );
                let mut at = event.encoded_range.start;
                let key = varint(&event.body, &mut at);
                assert_eq!(key >> 3, u64::from(event.tag), "{label}");
                assert_eq!(key & 7, u64::from(event.wire as u8), "{label}");
                let payload_start;
                let mut decoded_varint = None;
                let payload_len = match event.wire {
                    CanonicalWireType::Varint => {
                        payload_start = at;
                        decoded_varint = Some(varint(&event.body, &mut at));
                        None
                    }
                    CanonicalWireType::Fixed64 => {
                        payload_start = at;
                        at = at.checked_add(8).unwrap();
                        Some(8)
                    }
                    CanonicalWireType::Fixed32 => {
                        payload_start = at;
                        at = at.checked_add(4).unwrap();
                        Some(4)
                    }
                    CanonicalWireType::LengthDelimited => {
                        let len = usize::try_from(varint(&event.body, &mut at)).unwrap();
                        payload_start = at;
                        at = at.checked_add(len).unwrap();
                        Some(len)
                    }
                };
                assert_eq!(at, event.encoded_range.end, "{label}");
                assert!(at <= event.body.len(), "{label}");
                assert_eq!(event.payload_len, payload_len, "{label}");
                let mut found = None;
                for (slot, (field, protobuf)) in event
                    .program
                    .fields
                    .iter()
                    .zip(event.program.protobuf_fields)
                    .enumerate()
                {
                    if let RhoMatchFieldKind::Oneof(arms) = field.kind {
                        if let Some(arm) = arms.iter().find(|arm| arm.tag == event.tag) {
                            found = Some((
                                slot,
                                arm.protobuf_kind,
                                arm.protobuf_kind.wire_type().unwrap() as u8,
                                matches!(arm.payload, RhoMatchPayloadKind::Message(_)),
                                match arm.payload {
                                    RhoMatchPayloadKind::Message(id) => Some(id),
                                    _ => None,
                                },
                                false,
                            ));
                            break;
                        }
                    } else if protobuf.tag == event.tag {
                        let child = match field.kind {
                            RhoMatchFieldKind::OptionalMessage(id)
                            | RhoMatchFieldKind::MessageSequence(id) => Some(id),
                            _ => None,
                        };
                        let special = matches!(
                            field.kind,
                            RhoMatchFieldKind::StringMessageMap(_) | RhoMatchFieldKind::PathMap
                        );
                        found = Some((
                            slot,
                            protobuf.kind,
                            protobuf.kind.wire_type().unwrap() as u8,
                            child.is_some(),
                            child,
                            special,
                        ));
                        break;
                    }
                }
                let (slot, protobuf_kind, descriptor_wire, message_child, child_type, special) =
                    found.unwrap_or_else(|| {
                        panic!(
                            "{label}: field {} absent from generated descriptor",
                            event.tag
                        )
                    });
                assert_eq!(slot, event.slot, "{label}");
                assert_eq!(descriptor_wire, event.wire as u8, "{label}");
                assert!(before.last_slot.is_none_or(|last| last <= slot), "{label}");
                if before.last_slot == Some(slot) {
                    assert!(before.seen_slots.contains(&slot), "{label}");
                }
                if !matches!(
                    event.program.fields[slot].kind,
                    RhoMatchFieldKind::StringSequence
                        | RhoMatchFieldKind::BytesSequence
                        | RhoMatchFieldKind::MessageSequence(_)
                        | RhoMatchFieldKind::StringMessageMap(_)
                ) {
                    assert!(!before.seen_slots.contains(&slot), "{label}");
                }
                let mut expected_seen = before.seen_slots.clone();
                if !expected_seen.contains(&slot) {
                    expected_seen.push(slot);
                }
                expected_seen.sort_unstable();
                assert_eq!(event.frame_after.seen_slots, expected_seen, "{label}");
                assert_eq!(event.frame_after.last_slot, Some(slot), "{label}");
                let pushes = message_child || special;
                assert_eq!(
                    matches!(step.kind, DriverStepKind::Push(_)),
                    pushes,
                    "{label}"
                );
                assert_eq!(event.frame_after.waiting, pushes, "{label}");
                if before.region_start.source == before.region_end.source
                    && before.region_start.offset.checked_add(before.region_len)
                        == Some(before.region_end.offset)
                {
                    assert_eq!(
                        event.field_position.source, before.region_start.source,
                        "{label}"
                    );
                    assert_eq!(
                        event.field_position.offset,
                        before.region_start.offset + event.encoded_range.start,
                        "{label}"
                    );
                }
                assert_eq!(
                    event.frame_after.semantic_slots.len(),
                    before.semantic_slots.len(),
                    "{label}"
                );
                for other in 0..before.semantic_slots.len() {
                    if other != slot || pushes {
                        assert_eq!(
                            event.frame_after.semantic_slots[other], before.semantic_slots[other],
                            "{label}"
                        );
                    }
                }
                if pushes {
                    let waiting = event
                        .frame_after
                        .waiting_detail
                        .as_ref()
                        .expect("pushed message field retains its exact waiting request");
                    assert_eq!(waiting.slot(), slot, "{label}");
                    assert_eq!(waiting.field_position(), event.field_position, "{label}");
                    let (start, len) = step.after_top_region.expect("pushed frame has a region");
                    assert_eq!(event.payload_position, Some(start), "{label}");
                    assert_eq!(payload_len, Some(len), "{label}");
                    assert_eq!(
                        event.payload_position,
                        Some(
                            event
                                .body_positions
                                .get(payload_start)
                                .copied()
                                .unwrap_or(event.body_end_position)
                        ),
                        "{label}: payload anchor must come from the dense body position image"
                    );
                    if let Some(id) = child_type {
                        let RootWaitState::Child {
                            variant,
                            body,
                            body_start,
                            ..
                        } = waiting
                        else {
                            panic!("{label}: message push retained the wrong wait");
                        };
                        let expected_variant = match event.program.fields[slot].kind {
                            RhoMatchFieldKind::Oneof(arms) => arms
                                .iter()
                                .find(|arm| arm.tag == event.tag)
                                .map(|arm| arm.name),
                            _ => None,
                        };
                        assert_eq!(*variant, expected_variant, "{label}");
                        assert_eq!(body, &event.body[payload_start..at], "{label}");
                        assert_eq!(*body_start, start, "{label}");
                        assert_eq!(
                            step.after_top_kind,
                            Some(FrameRequestKind::Message),
                            "{label}"
                        );
                        assert!(
                            std::ptr::eq(
                                step.after_top_program.unwrap(),
                                wire_program_by_type_id(id).unwrap()
                            ),
                            "{label}"
                        );
                    } else if matches!(event.program.fields[slot].kind, RhoMatchFieldKind::PathMap)
                    {
                        assert!(matches!(waiting, RootWaitState::EPathMap { .. }), "{label}");
                        assert_eq!(
                            step.after_top_kind,
                            Some(FrameRequestKind::EPathMap),
                            "{label}"
                        );
                    } else {
                        assert!(matches!(waiting, RootWaitState::Map { .. }), "{label}");
                        assert_eq!(step.after_top_kind, Some(FrameRequestKind::Map), "{label}");
                    }
                } else {
                    assert!(event.payload_position.is_none(), "{label}");
                    assert!(event.frame_after.waiting_detail.is_none(), "{label}");
                    let actual = &event.frame_after.semantic_slots[slot];
                    assert_eq!(
                        actual.len(),
                        before.semantic_slots[slot].len() + 1,
                        "{label}"
                    );
                    assert_eq!(
                        &actual[..actual.len() - 1],
                        before.semantic_slots[slot].as_slice(),
                        "{label}"
                    );
                    let occurrence = actual.last().unwrap();
                    assert_eq!(occurrence.slot, slot, "{label}");
                    assert_eq!(occurrence.field_position, event.field_position, "{label}");
                    assert_eq!(
                        occurrence.field_position, event.body_positions[event.encoded_range.start],
                        "{label}: scalar occurrence must retain its indexed byte anchor"
                    );
                    assert!(occurrence.body.is_none(), "{label}");
                    let expected_variant = match event.program.fields[slot].kind {
                        RhoMatchFieldKind::Oneof(arms) => arms
                            .iter()
                            .find(|arm| arm.tag == event.tag)
                            .map(|arm| arm.name),
                        _ => None,
                    };
                    assert_eq!(occurrence.variant, expected_variant, "{label}");
                    let RootOccurrenceValue::Scalar(scalar) = &occurrence.value else {
                        panic!("{label}: scalar Continue stored a non-scalar occurrence");
                    };
                    let scalar_kind = match scalar {
                        RootScalarState::Bool(_) => 0,
                        RootScalarState::I32(_) => 1,
                        RootScalarState::U32(_) => 2,
                        RootScalarState::I64(_) => 3,
                        RootScalarState::U64(_) => 4,
                        RootScalarState::String(_) => 5,
                        RootScalarState::Bytes(_) => 6,
                    };
                    let expected_kind = match event.program.fields[slot].kind {
                        RhoMatchFieldKind::Bool => 0,
                        RhoMatchFieldKind::I32 => 1,
                        RhoMatchFieldKind::U32 => 2,
                        RhoMatchFieldKind::I64 => 3,
                        RhoMatchFieldKind::U64 => 4,
                        RhoMatchFieldKind::String | RhoMatchFieldKind::StringSequence => 5,
                        RhoMatchFieldKind::Bytes | RhoMatchFieldKind::BytesSequence => 6,
                        RhoMatchFieldKind::Oneof(arms) => match arms
                            .iter()
                            .find(|arm| arm.tag == event.tag)
                            .expect("selected oneof arm is in its descriptor")
                            .payload
                        {
                            RhoMatchPayloadKind::Bool => 0,
                            RhoMatchPayloadKind::I32 => 1,
                            RhoMatchPayloadKind::U32 => 2,
                            RhoMatchPayloadKind::I64 => 3,
                            RhoMatchPayloadKind::U64 => 4,
                            RhoMatchPayloadKind::String => 5,
                            RhoMatchPayloadKind::Bytes => 6,
                            RhoMatchPayloadKind::Message(_) => {
                                panic!("{label}: message oneof arm did not push")
                            }
                        },
                        _ => panic!("{label}: descriptor scalar kind did not continue"),
                    };
                    assert_eq!(scalar_kind, expected_kind, "{label}");
                    assert_eq!(
                        scalar,
                        &scalar_from_wire(
                            protobuf_kind,
                            &event.body[payload_start..at],
                            decoded_varint,
                        ),
                        "{label}: scalar Continue must preserve the independently decoded wire value"
                    );
                    match scalar {
                        RootScalarState::String(bytes) | RootScalarState::Bytes(bytes) => {
                            assert_eq!(bytes, &event.body[payload_start..at], "{label}")
                        }
                        _ => {}
                    }
                    assert_eq!(
                        step.after_message.as_ref(),
                        Some(&event.frame_after),
                        "{label}"
                    );
                }
                if before.region_start.source == before.region_end.source
                    && before.region_start.offset.checked_add(before.region_len)
                        == Some(before.region_end.offset)
                    && pushes
                {
                    assert_eq!(
                        event.payload_position.unwrap().source,
                        before.region_start.source,
                        "{label}"
                    );
                    assert_eq!(
                        event.payload_position.unwrap().offset,
                        before
                            .region_start
                            .offset
                            .checked_add(payload_start)
                            .unwrap(),
                        "{label}"
                    );
                }
            }
            DriverStepKind::Continue => {
                let (
                    Some(NonMessageFrameState::Map(before)),
                    Some(NonMessageFrameState::Map(after)),
                ) = (
                    step.before_nonmessage.as_ref(),
                    step.after_nonmessage.as_ref(),
                )
                else {
                    panic!("{label}: only a map key has a non-message Continue");
                };
                assert_eq!(before.body, after.body, "{label}");
                assert_eq!(
                    after.cursor.fields_read,
                    before.cursor.fields_read + 1,
                    "{label}"
                );
                assert!(after.cursor.bytes_read <= before.body.len(), "{label}");
                assert!(before.key.is_none() && after.key.is_some(), "{label}");
                assert!(!after.waiting_value && !after.saw_value, "{label}");
                let mut at = before.cursor.bytes_read;
                assert_eq!(varint(&before.body, &mut at), 10, "{label}");
                let len = usize::try_from(varint(&before.body, &mut at)).unwrap();
                assert_eq!(
                    at.checked_add(len),
                    Some(after.cursor.bytes_read),
                    "{label}"
                );
                assert_eq!(
                    after.key.as_deref(),
                    Some(&before.body[at..at + len]),
                    "{label}"
                );
            }
            DriverStepKind::Push(_) => {
                match (
                    step.before_nonmessage.as_ref(),
                    step.suspended_parent_after.as_ref(),
                ) {
                    (
                        Some(NonMessageFrameState::Map(before)),
                        Some(NonMessageFrameState::Map(after)),
                    ) => {
                        assert_eq!(before.body, after.body, "{label}");
                        assert_eq!(
                            after.cursor.fields_read,
                            before.cursor.fields_read + 1,
                            "{label}"
                        );
                        assert!(!before.waiting_value && after.waiting_value, "{label}");
                        assert_eq!(before.key, after.key, "{label}");
                        let mut at = before.cursor.bytes_read;
                        assert_eq!(varint(&before.body, &mut at), 18, "{label}");
                        let len = usize::try_from(varint(&before.body, &mut at)).unwrap();
                        assert_eq!(
                            at.checked_add(len),
                            Some(after.cursor.bytes_read),
                            "{label}"
                        );
                        assert_eq!(
                            step.after_top_kind,
                            Some(FrameRequestKind::Message),
                            "{label}"
                        );
                        let child = step.after_message.as_ref().unwrap();
                        assert!(
                            std::ptr::eq(child.program, &PAR_PROTOBUF_REGION_PROGRAM),
                            "{label}"
                        );
                        assert_eq!(child.region_len, len, "{label}");
                        assert_eq!(
                            child.region_start,
                            step.after_top_region.unwrap().0,
                            "{label}"
                        );
                        if before.region_start.source == before.region_end.source
                            && before.region_start.offset.checked_add(before.body.len())
                                == Some(before.region_end.offset)
                        {
                            assert_eq!(
                                child.region_start.source, before.region_start.source,
                                "{label}"
                            );
                            assert_eq!(
                                child.region_start.offset,
                                before.region_start.offset + at,
                                "{label}"
                            );
                        }
                    }
                    (
                        Some(NonMessageFrameState::EPathMap(before)),
                        Some(NonMessageFrameState::EPathMap(after)),
                    ) => {
                        assert_eq!(before.snapshot, after.snapshot, "{label}");
                        assert_eq!(before.region_start, after.region_start, "{label}");
                        assert!(
                            !before.waiting_map_value && before.waiting_obligation.is_none(),
                            "{label}"
                        );
                        assert!(
                            after.waiting_map_value || after.waiting_obligation.is_some(),
                            "{label}"
                        );
                    }
                    (
                        Some(NonMessageFrameState::CanonicalPath(before)),
                        Some(NonMessageFrameState::CanonicalPath(after)),
                    ) => {
                        assert_eq!(before.input, after.input, "{label}");
                        assert_eq!(before.region_start, after.region_start, "{label}");
                        assert!(
                            before.waiting.is_none() && after.waiting.is_some(),
                            "{label}"
                        );
                        assert_eq!(
                            step.after_top_kind,
                            Some(FrameRequestKind::Message),
                            "{label}"
                        );
                        assert!(
                            std::ptr::eq(
                                step.after_top_program.unwrap(),
                                &PAR_PROTOBUF_REGION_PROGRAM
                            ),
                            "{label}"
                        );
                    }
                    _ => panic!("{label}: non-message Push changed its parent frame kind"),
                }
            }
            DriverStepKind::Complete => {
                let pop = trace
                    .pop_accepts
                    .get(pop_at)
                    .unwrap_or_else(|| panic!("{label}: complete step has no pop observation"));
                pop_at += 1;
                if let Some(before) = step.before_message.as_ref() {
                    let eof = trace.message_eofs.get(message_eof_at).unwrap_or_else(|| {
                        panic!("{label}: message Complete lacks pre-finish EOF")
                    });
                    message_eof_at += 1;
                    let complete = trace
                        .message_completes
                        .get(message_complete_at)
                        .unwrap_or_else(|| {
                            panic!("{label}: message Complete lacks returned summary")
                        });
                    message_complete_at += 1;
                    assert_eq!(eof.step_index, step_index, "{label}");
                    assert_eq!(complete.step_index, step_index, "{label}");
                    assert_eq!(&eof.frame, before, "{label}");
                    assert_eq!(&complete.frame, before, "{label}");
                    assert_eq!(eof.source_len, step.score_len_before, "{label}");
                    assert_eq!(complete.source_len, step.score_len_after, "{label}");
                    assert_eq!(pop.child_frame.as_ref(), Some(before), "{label}");
                    assert_eq!(
                        pop.typed_summary.as_ref(),
                        Some(&complete.summary),
                        "{label}"
                    );
                    assert!(before.cursor_empty && !before.waiting, "{label}");
                    assert_eq!(before.cursor_bytes_read, before.region_len, "{label}");
                }
                if let Some(NonMessageFrameState::Map(before)) = step.before_nonmessage.as_ref() {
                    let eof = trace
                        .map_eofs
                        .get(map_eof_at)
                        .unwrap_or_else(|| panic!("{label}: Map Complete lacks pre-finish EOF"));
                    map_eof_at += 1;
                    let complete = trace
                        .map_completes
                        .get(map_complete_at)
                        .unwrap_or_else(|| panic!("{label}: Map Complete lacks returned summary"));
                    map_complete_at += 1;
                    assert_eq!(eof.step_index, step_index, "{label}");
                    assert_eq!(complete.step_index, step_index, "{label}");
                    assert_eq!(&eof.frame, before, "{label}");
                    assert_eq!(
                        pop.child_nonmessage.as_ref(),
                        Some(&NonMessageFrameState::Map(complete.frame.clone())),
                        "{label}"
                    );
                    assert_eq!(pop.map_summary.as_ref(), Some(&complete.summary), "{label}");
                    assert_eq!(
                        complete.summary.key,
                        before.key.clone().unwrap_or_default(),
                        "{label}"
                    );
                    assert_eq!(
                        complete.summary.key_position,
                        before.key_position.unwrap_or(before.region_start),
                        "{label}"
                    );
                    assert_eq!(complete.summary.value, before.value, "{label}");
                    assert!(complete.frame.value.is_none(), "{label}");
                    assert!(!complete.frame.waiting_value, "{label}");
                    assert_eq!(step.score_len_before, step.score_len_after, "{label}");
                    if let Some(key) = before.key.as_ref() {
                        let mut at = 0;
                        assert_eq!(varint(&before.body, &mut at), 10, "{label}");
                        let len = usize::try_from(varint(&before.body, &mut at)).unwrap();
                        assert_eq!(len, key.len(), "{label}");
                        assert_eq!(&before.body[at..at + len], key, "{label}");
                        if before.region_start.source == before.region_end.source
                            && before.region_start.offset.checked_add(before.body.len())
                                == Some(before.region_end.offset)
                        {
                            assert_eq!(
                                complete.summary.key_start,
                                CanonicalByteProvenance::new(
                                    before.region_start.source,
                                    before.region_start.offset + at
                                ),
                                "{label}"
                            );
                            assert_eq!(
                                complete.summary.key_end,
                                CanonicalByteProvenance::new(
                                    before.region_start.source,
                                    before.region_start.offset + at + len
                                ),
                                "{label}"
                            );
                        }
                    } else {
                        assert_eq!(complete.summary.key_start, before.region_start, "{label}");
                        assert_eq!(complete.summary.key_end, before.region_start, "{label}");
                    }
                }
                assert_eq!(pop.before_len, step.before_len, "{label}");
                assert_eq!(pop.after_len, Some(step.after_len), "{label}");
                assert_eq!(pop.child_kind, step.before_top_kind, "{label}");
                assert_eq!(pop.parent_kind, step.after_top_kind, "{label}");
                assert_eq!(pop.source_len_before, step.score_len_after, "{label}");
                assert_eq!(pop.source_len_after, Some(step.score_len_after), "{label}");
                if let Some(handle) = pop
                    .summary
                    .as_ref()
                    .and_then(|summary| summary.score_handle)
                {
                    assert!(handle.index() < step.score_len_after, "{label}");
                }
                assert_eq!(pop.parent_after, step.after_message, "{label}");
                assert_eq!(
                    pop.parent_nonmessage_after, step.after_nonmessage,
                    "{label}"
                );
                match (
                    step.before_nonmessage.as_ref(),
                    pop.child_nonmessage.as_ref(),
                ) {
                    (None, None) => {}
                    (
                        Some(NonMessageFrameState::Map(before)),
                        Some(NonMessageFrameState::Map(after)),
                    ) => {
                        assert!(step.score_appended.is_empty(), "{label}");
                        assert_eq!(before.body, after.body, "{label}");
                        assert_eq!(before.region_start, after.region_start, "{label}");
                        assert_eq!(before.cursor, after.cursor, "{label}");
                        assert_eq!(after.cursor.bytes_read, after.body.len(), "{label}");
                        assert!(!after.waiting_value, "{label}");
                    }
                    (
                        Some(NonMessageFrameState::EPathMap(before)),
                        Some(NonMessageFrameState::EPathMap(after)),
                    ) => {
                        assert!(step.score_appended.is_empty(), "{label}");
                        assert_eq!(before.snapshot, after.snapshot, "{label}");
                        assert_eq!(before.region_start, after.region_start, "{label}");
                        assert_eq!(before.mode, after.mode, "{label}");
                        assert!(
                            !after.waiting_map_value && after.waiting_obligation.is_none(),
                            "{label}"
                        );
                        assert_eq!(
                            pop.epathmap_child_snapshot.as_deref(),
                            Some(after.snapshot.as_slice()),
                            "{label}"
                        );
                        assert_eq!(
                            pop.epathmap_returned_snapshot.as_deref(),
                            Some(after.snapshot.as_slice()),
                            "{label}"
                        );
                    }
                    (
                        Some(NonMessageFrameState::CanonicalPath(before)),
                        Some(NonMessageFrameState::CanonicalPath(after)),
                    ) => {
                        assert!(step.score_appended.is_empty(), "{label}");
                        assert_eq!(before.input, after.input, "{label}");
                        assert_eq!(before.region_start, after.region_start, "{label}");
                        assert!(after.bytes_yielded >= before.bytes_yielded, "{label}");
                        assert_eq!(after.bytes_yielded, after.input.len(), "{label}");
                        assert!(after.waiting.is_none(), "{label}");
                    }
                    _ => panic!("{label}: completed non-message frame changed kind"),
                }
                if let (Some(before), Some(after)) = (
                    pop.parent_nonmessage_before.as_ref(),
                    pop.parent_nonmessage_after.as_ref(),
                ) {
                    match (before, after) {
                        (NonMessageFrameState::Map(prior), NonMessageFrameState::Map(next)) => {
                            assert_eq!(prior.body, next.body, "{label}");
                            assert_eq!(prior.region_start, next.region_start, "{label}");
                            assert_eq!(prior.cursor, next.cursor, "{label}");
                            assert!(prior.waiting_value && !next.waiting_value, "{label}");
                            assert!(next.value.is_some(), "{label}");
                            let RootReturnedState::Par(returned) = pop
                                .typed_summary
                                .as_ref()
                                .expect("Map value returned a typed Par")
                            else {
                                panic!("{label}: Map value returned a non-Par summary");
                            };
                            assert_eq!(next.value.as_ref(), Some(returned), "{label}");
                        }
                        (
                            NonMessageFrameState::EPathMap(prior),
                            NonMessageFrameState::EPathMap(next),
                        ) => {
                            assert_eq!(prior.snapshot, next.snapshot, "{label}");
                            assert_eq!(prior.region_start, next.region_start, "{label}");
                            assert_eq!(prior.cursor, next.cursor, "{label}");
                            assert!(
                                prior.waiting_map_value || prior.waiting_obligation.is_some(),
                                "{label}"
                            );
                            assert!(
                                !next.waiting_map_value && next.waiting_obligation.is_none(),
                                "{label}"
                            );
                        }
                        (
                            NonMessageFrameState::CanonicalPath(prior),
                            NonMessageFrameState::CanonicalPath(next),
                        ) => {
                            assert_eq!(prior.input, next.input, "{label}");
                            assert_eq!(prior.region_start, next.region_start, "{label}");
                            assert_eq!(prior.bytes_yielded, next.bytes_yielded, "{label}");
                            assert!(prior.waiting.is_some() && next.waiting.is_none(), "{label}");
                        }
                        _ => panic!("{label}: parent frame kind changed during acceptance"),
                    }
                }
                if let (Some(parent_before), Some(parent_after)) =
                    (pop.parent_before.as_ref(), pop.parent_after.as_ref())
                {
                    let waiting = parent_before
                        .waiting_detail
                        .as_ref()
                        .expect("accepted message parent was waiting for one child");
                    assert!(parent_after.waiting_detail.is_none(), "{label}");
                    assert_eq!(parent_after.last_slot, parent_before.last_slot, "{label}");
                    assert_eq!(parent_after.seen_slots, parent_before.seen_slots, "{label}");
                    assert_eq!(
                        parent_after.cursor_bytes_read, parent_before.cursor_bytes_read,
                        "{label}"
                    );
                    let slot = waiting.slot();
                    for other in 0..parent_before.semantic_slots.len() {
                        if other != slot {
                            assert_eq!(
                                parent_after.semantic_slots[other],
                                parent_before.semantic_slots[other],
                                "{label}"
                            );
                        }
                    }
                    let prior = &parent_before.semantic_slots[slot];
                    let accepted = &parent_after.semantic_slots[slot];
                    assert_eq!(accepted.len(), prior.len() + 1, "{label}");
                    assert_eq!(&accepted[..prior.len()], prior.as_slice(), "{label}");
                    let occurrence = accepted.last().unwrap();
                    assert_eq!(occurrence.slot, slot, "{label}");
                    assert_eq!(
                        occurrence.field_position,
                        waiting.field_position(),
                        "{label}"
                    );
                    match waiting {
                        RootWaitState::Child {
                            variant,
                            body,
                            body_start,
                            body_end,
                            ..
                        } => {
                            assert_eq!(occurrence.variant, *variant, "{label}");
                            assert_eq!(occurrence.body.as_ref(), Some(body), "{label}");
                            assert_eq!(occurrence.body_start, Some(*body_start), "{label}");
                            assert_eq!(occurrence.body_end, Some(*body_end), "{label}");
                            assert_eq!(pop.child_body.as_ref(), Some(body), "{label}");
                            assert_eq!(
                                occurrence.value,
                                RootOccurrenceValue::Message(Box::new(
                                    pop.typed_summary
                                        .clone()
                                        .expect("message child has typed summary")
                                )),
                                "{label}"
                            );
                        }
                        RootWaitState::Map { .. } => {
                            let returned = pop
                                .map_summary
                                .as_ref()
                                .expect("Map return retains its key and value");
                            assert_eq!(
                                occurrence.value,
                                RootOccurrenceValue::Map {
                                    key: returned.key.clone(),
                                    key_position: returned.key_position,
                                    value: returned.value.clone(),
                                },
                                "{label}"
                            );
                            assert!(occurrence.body.is_none(), "{label}");
                        }
                        RootWaitState::EPathMap { .. } => {
                            let RootOccurrenceValue::EPathMap { snapshot, .. } = &occurrence.value
                            else {
                                panic!("{label}: EPathMap return stored the wrong occurrence");
                            };
                            assert_eq!(
                                Some(snapshot.as_slice()),
                                pop.epathmap_returned_snapshot.as_deref(),
                                "{label}"
                            );
                        }
                    }
                }
                if let Some(frame) = step.before_message.as_ref() {
                    assert!(frame.cursor_empty && !frame.waiting, "{label}");
                    assert_eq!(frame.cursor_bytes_read, frame.region_len, "{label}");
                    assert_eq!(pop.child_frame.as_ref(), Some(frame), "{label}");
                    assert!(
                        pop.child_program.is_some_and(|program| std::ptr::eq(
                            program,
                            step.before_top_program.unwrap()
                        )),
                        "{label}"
                    );
                    if let Some(summary) = pop.summary.as_ref() {
                        assert_summary_score_source(summary, &transcripts, label);
                    }
                }
                if pop.child_kind == FrameRequestKind::EPathMap
                    && pop.parent_kind == Some(FrameRequestKind::Message)
                {
                    let receipt = trace
                        .epathmap_accepts
                        .get(epm_accept_at)
                        .unwrap_or_else(|| panic!("{label}: EPathMap return lacks acceptance"));
                    epm_accept_at += 1;
                    assert_eq!(
                        pop.epathmap_child_snapshot.as_deref(),
                        pop.epathmap_returned_snapshot.as_deref(),
                        "{label}"
                    );
                    assert_eq!(
                        pop.epathmap_returned_snapshot.as_deref(),
                        Some(receipt.returned_snapshot.as_slice()),
                        "{label}"
                    );
                    assert_eq!(
                        receipt.returned_snapshot, receipt.stored_snapshot,
                        "{label}"
                    );
                    assert_eq!(receipt.returned_start, receipt.stored_start, "{label}");
                    assert_eq!(
                        receipt.parent_type_id,
                        step.after_top_program.unwrap().type_id,
                        "{label}"
                    );
                    assert_eq!(
                        step.after_message.as_ref().unwrap().last_slot,
                        Some(receipt.slot),
                        "{label}"
                    );
                    assert_eq!(
                        step.after_top_program.unwrap().fields[receipt.slot].tag,
                        receipt.tag,
                        "{label}"
                    );
                }
            }
        }
        match step.kind {
            DriverStepKind::Continue => {
                assert!(step.suspended_parent_after.is_none(), "{label}");
                *nonmessage_stack.last_mut().unwrap() = step.after_nonmessage.clone();
            }
            DriverStepKind::Push(_) => {
                *nonmessage_stack.last_mut().unwrap() = step.suspended_parent_after.clone();
                nonmessage_stack.push(step.after_nonmessage.clone());
            }
            DriverStepKind::Complete => {
                assert!(step.suspended_parent_after.is_none(), "{label}");
                nonmessage_stack
                    .pop()
                    .expect("complete pops one observed frame");
                if let Some(parent) = nonmessage_stack.last_mut() {
                    assert_eq!(
                        parent,
                        &trace.pop_accepts[pop_at - 1].parent_nonmessage_before,
                        "{label}",
                    );
                    *parent = step.after_nonmessage.clone();
                } else {
                    assert_eq!(step.after_len, 0, "{label}");
                }
            }
        }
    }
    assert!(nonmessage_stack.is_empty(), "{label}");
    assert_eq!(field_at, trace.message_fields.len(), "{label}");
    assert_eq!(pop_at, trace.pop_accepts.len(), "{label}");
    assert_eq!(epm_accept_at, trace.epathmap_accepts.len(), "{label}");
    assert_eq!(message_eof_at, trace.message_eofs.len(), "{label}");
    assert_eq!(
        message_complete_at,
        trace.message_completes.len(),
        "{label}"
    );
    assert_eq!(map_eof_at, trace.map_eofs.len(), "{label}");
    assert_eq!(map_complete_at, trace.map_completes.len(), "{label}");
    (field_at, pop_at)
}

fn summary_state(
    semantic: &SemanticSummary<'_>,
    scores: &CanonicalScoreArena<'_>,
) -> RootSummaryState {
    let (variant, component, score_handle) = match semantic {
        SemanticSummary::Opaque => (RootSemanticVariant::Opaque, None, None),
        SemanticSummary::Par(par) => (RootSemanticVariant::Par, None, Some(par.score)),
        SemanticSummary::Component(component) => (
            RootSemanticVariant::Component,
            Some(RootComponentState {
                family: component.family,
                complete: component.complete,
                canonical_path_stable: component.canonical_path_stable,
            }),
            Some(component.score),
        ),
        SemanticSummary::Var(score) => (RootSemanticVariant::Var, None, Some(*score)),
        SemanticSummary::ReceiveBind { score, .. } => {
            (RootSemanticVariant::ReceiveBind, None, *score)
        }
        SemanticSummary::CostScore(score) => (RootSemanticVariant::CostScore, None, Some(*score)),
        SemanticSummary::MatchCase { score, .. } => (RootSemanticVariant::MatchCase, None, *score),
        SemanticSummary::ExprPayload(payload) => {
            (RootSemanticVariant::ExprPayload, None, payload.score)
        }
        SemanticSummary::KeyValuePair { .. } => (RootSemanticVariant::KeyValuePair, None, None),
        SemanticSummary::BigRational { .. } => (RootSemanticVariant::BigRational, None, None),
        SemanticSummary::FixedPoint { .. } => (RootSemanticVariant::FixedPoint, None, None),
        SemanticSummary::ConnectiveBody(_) => (RootSemanticVariant::ConnectiveBody, None, None),
        SemanticSummary::VarRef { .. } => (RootSemanticVariant::VarRef, None, None),
        SemanticSummary::Bytes(_) => (RootSemanticVariant::Bytes, None, None),
        SemanticSummary::Unit => (RootSemanticVariant::Unit, None, None),
    };
    let no_score_payload = match semantic {
        SemanticSummary::Opaque => Some(NoScorePayload::Opaque),
        SemanticSummary::KeyValuePair { key, value } => Some(NoScorePayload::KeyValuePair {
            key: key.as_ref().map(|par| scores.cursor(par.score).collect()),
            value: value.as_ref().map(|par| scores.cursor(par.score).collect()),
        }),
        SemanticSummary::BigRational {
            numerator,
            denominator,
        } => Some(NoScorePayload::BigRational {
            numerator: numerator.bytes().collect(),
            denominator: denominator.bytes().collect(),
        }),
        SemanticSummary::FixedPoint { unscaled, scale } => Some(NoScorePayload::FixedPoint {
            unscaled: unscaled.bytes().collect(),
            scale: *scale,
        }),
        SemanticSummary::ConnectiveBody(pars) => Some(NoScorePayload::ConnectiveBody(
            pars.iter()
                .map(|par| scores.cursor(par.score).collect())
                .collect(),
        )),
        SemanticSummary::VarRef { index, depth } => Some(NoScorePayload::VarRef {
            index: *index,
            depth: *depth,
        }),
        SemanticSummary::Bytes(bytes) => Some(NoScorePayload::Bytes(bytes.bytes().collect())),
        SemanticSummary::Unit => Some(NoScorePayload::Unit),
        SemanticSummary::ExprPayload(payload) if payload.score.is_none() => {
            Some(NoScorePayload::ExprPayloadAbsent)
        }
        _ => None,
    };
    RootSummaryState {
        variant,
        component,
        no_score_payload,
        score_handle,
        score_encoded_len: score_handle.map(|handle| scores.encoded_len(handle)),
        score_bytes: score_handle.map(|handle| scores.cursor(handle).collect()),
    }
}

fn frame_summary_state(
    summary: &FrameSummary<'_>,
    scores: &CanonicalScoreArena<'_>,
) -> Option<RootSummaryState> {
    match summary {
        FrameSummary::Message(message) => Some(summary_state(&message.semantic, scores)),
        _ => None,
    }
}

pub(super) fn record_eof(frame: &MessageFrame<'_>, scores: &CanonicalScoreArena<'_>) {
    with_trace(|trace| {
        trace.message_eofs.push(MessageEofStepState {
            step_index: trace.driver_steps.len(),
            frame: frame_state(frame),
            source_len: scores.source_len(),
        });
        if trace.root_step {
            trace.eof = Some(frame_state(frame));
        } else {
            trace.child_eofs.push(ChildEofState {
                program: frame.program,
                body: frame.body.bytes().collect(),
                frame: frame_state(frame),
                source_len: scores.source_len(),
            });
        }
    });
}

pub(super) fn record_reduced(
    frame: &MessageFrame<'_>,
    semantic: &SemanticSummary<'_>,
    scores: &CanonicalScoreArena<'_>,
) {
    with_trace(|trace| {
        if trace.root_step {
            trace.reduced_frame = Some(frame_state(frame));
            trace.reduced = Some(summary_state(semantic, scores));
        } else {
            trace.children.push(ChildFinishState {
                program: frame.program,
                frame: frame_state(frame),
                body: frame.body.bytes().collect(),
                summary: summary_state(semantic, scores),
            });
        }
    });
}

pub(super) fn record_send_finish(
    frame: &MessageFrame<'_>,
    input: SendFinishInput,
    result: (bool, CanonicalScoreHandle),
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.send_finishes.push(SendFinishState {
            body: frame.body.bytes().collect(),
            input,
            complete: result.0,
            score_handle: result.1,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_bundle_finish(
    frame: &MessageFrame<'_>,
    input: BundleFinishInput,
    result: (bool, CanonicalScoreHandle),
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.bundle_finishes.push(BundleFinishState {
            body: frame.body.bytes().collect(),
            input,
            complete: result.0,
            score_handle: result.1,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_if_finish(
    frame: &MessageFrame<'_>,
    input: IfFinishInput,
    result: (bool, CanonicalScoreHandle),
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.if_finishes.push(IfFinishState {
            body: frame.body.bytes().collect(),
            input,
            complete: result.0,
            score_handle: result.1,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_match_finish(
    frame: &MessageFrame<'_>,
    input: MatchFinishInput,
    result: (bool, CanonicalScoreHandle),
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.match_finishes.push(MatchFinishState {
            body: frame.body.bytes().collect(),
            input,
            complete: result.0,
            score_handle: result.1,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_receive_finish(
    frame: &MessageFrame<'_>,
    input: ReceiveFinishInput,
    result: (bool, CanonicalScoreHandle),
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.receive_finishes.push(ReceiveFinishState {
            body: frame.body.bytes().collect(),
            input,
            complete: result.0,
            score_handle: result.1,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_receive_bind_finish(
    frame: &MessageFrame<'_>,
    input: ReceiveBindFinishInput,
    score_handle: Option<CanonicalScoreHandle>,
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.receive_bind_finishes.push(ReceiveBindFinishState {
            body: frame.body.bytes().collect(),
            input,
            score_handle,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_match_case_finish(
    frame: &MessageFrame<'_>,
    input: MatchCaseFinishInput,
    score_handle: Option<CanonicalScoreHandle>,
    required_present: bool,
    canonical_fixed_point: bool,
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.match_case_finishes.push(MatchCaseFinishState {
            body: frame.body.bytes().collect(),
            input,
            score_handle,
            required_present,
            canonical_fixed_point,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_new_finish(
    frame: &MessageFrame<'_>,
    input: NewFinishInput,
    result: (bool, CanonicalScoreHandle),
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.new_finishes.push(NewFinishState {
            body: frame.body.bytes().collect(),
            input,
            complete: result.0,
            score_handle: result.1,
            source_len_before,
            source_len_after,
        })
    });
}

pub(super) fn record_complete(
    frame: &MessageFrame<'_>,
    action: &FrameAction<'_>,
    scores: &CanonicalScoreArena<'_>,
) {
    with_trace(|trace| {
        if let FrameAction::Complete(summary) = action {
            let FrameSummary::Message(message) = summary else {
                panic!("message completion returned a different frame kind");
            };
            trace.message_completes.push(MessageCompleteStepState {
                step_index: trace.driver_steps.len(),
                frame: frame_state(frame),
                summary: returned_state(&message.semantic),
                source_len: scores.source_len(),
            });
            if trace.root_step {
                trace.complete_frame = Some(frame_state(frame));
                trace.complete = frame_summary_state(summary, scores);
            }
        }
    });
}

pub(super) fn record_map_eof(frame: &MapFrame<'_>) {
    with_trace(|trace| {
        trace.map_eofs.push(MapEofStepState {
            step_index: trace.driver_steps.len(),
            frame: map_state(frame),
        });
    });
}

pub(super) fn record_map_complete(frame: &MapFrame<'_>, summary: &MapSummary<'_>) {
    with_trace(|trace| {
        trace.map_completes.push(MapCompleteStepState {
            step_index: trace.driver_steps.len(),
            frame: map_state(frame),
            summary: map_summary_state(summary),
        });
    });
}

pub(super) fn record_pre_pop(
    frames: &[Frame<'_>],
    summary: &FrameSummary<'_>,
    scores: &CanonicalScoreArena<'_>,
) {
    with_trace(|trace| {
        let child = frames.last().expect("completed frame remains before pop");
        let parent = frames
            .len()
            .checked_sub(2)
            .and_then(|index| frames.get(index));
        let parent_before = parent.and_then(|frame| match frame {
            Frame::Message(frame) => Some(frame_state(frame)),
            _ => None,
        });
        let parent_nonmessage_before = parent.and_then(nonmessage_state);
        if frames.len() == 1 {
            if let Frame::Message(frame) = child {
                trace.pre_pop_frame = Some(frame_state(frame));
                trace.pre_pop = frame_summary_state(summary, scores);
            }
        }
        let (child_program, child_body, child_frame) = match child {
            Frame::Message(frame) => (
                Some(frame.program),
                Some(frame.body.bytes().collect()),
                Some(frame_state(frame)),
            ),
            _ => (None, None, None),
        };
        let epathmap_child_snapshot = match child {
            Frame::EPathMap(frame) => Some(frame.snapshot.bytes().collect()),
            _ => None,
        };
        let epathmap_returned_snapshot = match summary {
            FrameSummary::EPathMap(summary) => Some(summary.snapshot.bytes().collect()),
            _ => None,
        };
        trace.pop_accepts.push(PopAcceptState {
            child_kind: child.request_kind(),
            child_program,
            child_body,
            child_frame,
            child_nonmessage: nonmessage_state(child),
            epathmap_child_snapshot,
            epathmap_returned_snapshot,
            epathmap_returned_mode: match summary {
                FrameSummary::EPathMap(summary) => Some(summary.mode),
                _ => None,
            },
            epathmap_returned_entries_stable: match summary {
                FrameSummary::EPathMap(summary) => Some(summary.entries_canonical_path_stable),
                _ => None,
            },
            summary: frame_summary_state(summary, scores),
            typed_summary: match summary {
                FrameSummary::Message(value) => Some(returned_state(&value.semantic)),
                _ => None,
            },
            map_summary: match summary {
                FrameSummary::Map(value) => Some(map_summary_state(value)),
                _ => None,
            },
            before_len: frames.len(),
            after_len: None,
            parent_kind: parent.map(Frame::request_kind),
            parent_before,
            parent_after: None,
            parent_nonmessage_before,
            parent_nonmessage_after: None,
            applied_obligation: None,
            source_len_before: scores.source_len(),
            source_len_after: None,
        });
    });
}

pub(super) fn record_epathmap_accept(
    parent: &MessageFrame<'_>,
    slot: usize,
    returned: &AdmissionRegion<'_>,
    stored: &AdmissionRegion<'_>,
) {
    with_trace(|trace| {
        trace.epathmap_accepts.push(EPathMapAcceptState {
            step_index: trace.driver_steps.len(),
            parent_type_id: parent.program.type_id,
            slot,
            tag: parent.program.fields[slot].tag,
            returned_snapshot: returned.bytes().collect(),
            stored_snapshot: stored.bytes().collect(),
            returned_start: returned.position_at(0).unwrap().physical,
            stored_start: stored.position_at(0).unwrap().physical,
        });
    });
}

pub(super) fn record_epathmap_score(
    frame: &MessageFrame<'_>,
    snapshot: &AdmissionRegion<'_>,
    snapshot_handle: CanonicalScoreHandle,
    score_handle: CanonicalScoreHandle,
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.epathmap_scores.push(EPathMapScoreState {
            step_index: trace.driver_steps.len(),
            body: frame.body.bytes().collect(),
            snapshot: snapshot.bytes().collect(),
            snapshot_handle,
            score_handle,
            source_len_before,
            source_len_after,
        });
    });
}

pub(super) fn record_expr_finish(
    frame: &MessageFrame<'_>,
    score_handle: CanonicalScoreHandle,
    complete: bool,
    canonical_path_stable: bool,
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.expr_finishes.push(ExprFinishState {
            body: frame.body.bytes().collect(),
            score_handle,
            complete,
            canonical_path_stable,
            source_len_before,
            source_len_after,
        });
    });
}

pub(super) fn record_expr_payload_finish(
    frame: &MessageFrame<'_>,
    score_handle: Option<CanonicalScoreHandle>,
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.expr_payload_finishes.push(ExprPayloadFinishState {
            program: frame.program,
            body: frame.body.bytes().collect(),
            score_handle,
            source_len_before,
            source_len_after,
        });
    });
}

pub(super) fn record_emap_finish(
    frame: &MessageFrame<'_>,
    pair_scores: Vec<CanonicalScoreHandle>,
    complete: bool,
    canonical_fixed_point: bool,
    score_handle: Option<CanonicalScoreHandle>,
    source_len_before: usize,
    source_len_after: usize,
) {
    with_trace(|trace| {
        trace.emap_finishes.push(EMapFinishState {
            body: frame.body.bytes().collect(),
            pair_scores,
            complete,
            canonical_fixed_point,
            score_handle,
            source_len_before,
            source_len_after,
        });
    });
}

pub(super) fn record_parent_accept(
    parent: &Frame<'_>,
    effect: &FrameAcceptEffect<'_>,
    scores: &CanonicalScoreArena<'_>,
    after_len: usize,
) {
    with_trace(|trace| {
        let transition = trace
            .pop_accepts
            .last_mut()
            .expect("a child pre-pop observation precedes parent acceptance");
        transition.after_len = Some(after_len);
        assert_eq!(transition.parent_kind, Some(parent.request_kind()));
        transition.parent_after = match parent {
            Frame::Message(frame) => Some(frame_state(frame)),
            _ => None,
        };
        transition.parent_nonmessage_after = nonmessage_state(parent);
        transition.applied_obligation = match effect {
            FrameAcceptEffect::ResolvedObligation { id, resolution } => Some((*id, *resolution)),
            _ => None,
        };
        transition.source_len_after = Some(scores.source_len());
    });
}

pub(super) fn record_return(
    summary: &FrameSummary<'_>,
    scores: &CanonicalScoreArena<'_>,
    remaining_frames: usize,
) {
    with_root_trace(|trace| {
        let transition = trace
            .pop_accepts
            .last_mut()
            .expect("root pre-pop observation precedes return");
        transition.after_len = Some(remaining_frames);
        transition.source_len_after = Some(scores.source_len());
        trace.returned = frame_summary_state(summary, scores);
        trace.remaining_frames = Some(remaining_frames);
        trace.score_arena = Some(scores.source_image());
    });
}

pub(super) fn replay_score_source(entries: &[CanonicalScoreSourceEntry]) -> Vec<Vec<u8>> {
    fn atom(prefix: u8, bytes: &[u8]) -> Vec<u8> {
        let mut transcript = vec![1, prefix];
        for &byte in bytes {
            transcript.push(byte);
            if byte == 0 {
                transcript.push(0xff);
            }
        }
        transcript.extend([0, 0]);
        transcript
    }

    let mut transcripts: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let transcript = match &entry.kind {
            CanonicalScoreSourceKind::Integer(value) => {
                let mut transcript = vec![1, 1];
                transcript.extend(((*value as u64) ^ (1u64 << 63)).to_be_bytes());
                transcript
            }
            CanonicalScoreSourceKind::StringOwned(bytes)
            | CanonicalScoreSourceKind::StringShared(bytes)
            | CanonicalScoreSourceKind::StringRegion(bytes)
            | CanonicalScoreSourceKind::StringIndexed(bytes) => atom(2, bytes),
            CanonicalScoreSourceKind::BytesOwned(bytes)
            | CanonicalScoreSourceKind::BytesShared(bytes)
            | CanonicalScoreSourceKind::BytesRegion(bytes)
            | CanonicalScoreSourceKind::BytesIndexed(bytes) => atom(3, bytes),
            CanonicalScoreSourceKind::Node(children) => {
                let mut transcript = vec![2];
                for &child in children {
                    assert!(child < index, "score child handle must precede its parent");
                    transcript.extend_from_slice(&transcripts[child]);
                }
                transcript.push(0);
                transcript
            }
        };
        assert_eq!(entry.encoded_len, transcript.len());
        transcripts.push(transcript);
    }
    transcripts
}

pub(super) fn assert_summary_score_source(
    summary: &RootSummaryState,
    transcripts: &[Vec<u8>],
    label: &str,
) {
    match summary.score_handle {
        Some(handle) => {
            let transcript = transcripts
                .get(handle.index())
                .unwrap_or_else(|| panic!("{label}: score handle is outside the source arena"));
            assert_eq!(summary.score_encoded_len, Some(transcript.len()), "{label}");
            assert_eq!(
                summary.score_bytes.as_deref(),
                Some(transcript.as_slice()),
                "{label}"
            );
        }
        None => {
            assert!(summary.score_encoded_len.is_none(), "{label}");
            assert!(summary.score_bytes.is_none(), "{label}");
        }
    }
}

pub(super) fn assert_send_finish_source(
    finish: &SendFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    match &finish.input {
        SendFinishInput::Incomplete => {
            assert!(!finish.complete, "{label}");
            assert_eq!(after, before + 2, "{label}");
            assert_eq!(finish.score_handle.index(), before + 1, "{label}");
            assert_eq!(
                entries[before].kind,
                CanonicalScoreSourceKind::Integer(i64::from(
                    semantics::NONSEMANTIC_SCORE_CONSTRUCTOR,
                )),
                "{label}"
            );
            assert_eq!(
                entries[before + 1].kind,
                CanonicalScoreSourceKind::Node(vec![before]),
                "{label}"
            );
        }
        SendFinishInput::Complete {
            persistent,
            channel,
            data,
            connective_used,
            cost,
        } => {
            assert!(finish.complete, "{label}");
            assert_eq!(after, before + 4, "{label}");
            assert_eq!(finish.score_handle.index(), before + 3, "{label}");
            assert_eq!(
                entries[before].kind,
                CanonicalScoreSourceKind::Integer(i64::from(*persistent)),
                "{label}"
            );
            assert_eq!(
                entries[before + 1].kind,
                CanonicalScoreSourceKind::Integer(i64::from(*connective_used)),
                "{label}"
            );
            assert_eq!(
                entries[before + 2].kind,
                CanonicalScoreSourceKind::Integer(i64::from(Score::SEND)),
                "{label}"
            );
            let mut children = vec![before + 2, before, channel.index()];
            children.extend(data.iter().map(|handle| handle.index()));
            children.push(before + 1);
            if let Some(cost) = cost {
                children.push(cost.index());
            }
            assert_eq!(
                entries[before + 3].kind,
                CanonicalScoreSourceKind::Node(children),
                "{label}"
            );
        }
    }
    let handle = finish.score_handle.index();
    assert_eq!(
        entries[handle].encoded_len,
        transcripts[handle].len(),
        "{label}"
    );
}

pub(super) fn assert_bundle_finish_source(
    finish: &BundleFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    assert_eq!(after, before + 2, "{label}");
    assert_eq!(finish.score_handle.index(), before + 1, "{label}");
    let (constructor, children) = match &finish.input {
        BundleFinishInput::Incomplete => {
            assert!(!finish.complete, "{label}");
            (semantics::NONSEMANTIC_SCORE_CONSTRUCTOR, vec![before])
        }
        BundleFinishInput::Complete { read, write, body } => {
            assert!(finish.complete, "{label}");
            (semantics::bundle_constructor(*read, *write), vec![
                before,
                body.index(),
            ])
        }
    };
    assert_eq!(
        entries[before].kind,
        CanonicalScoreSourceKind::Integer(i64::from(constructor)),
        "{label}"
    );
    assert_eq!(
        entries[before + 1].kind,
        CanonicalScoreSourceKind::Node(children),
        "{label}"
    );
    assert_eq!(
        entries[before + 1].encoded_len,
        transcripts[before + 1].len(),
        "{label}"
    );
}

pub(super) fn assert_if_finish_source(
    finish: &IfFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    let (constructor, children, node_index) = match &finish.input {
        IfFinishInput::Incomplete => {
            assert!(!finish.complete, "{label}");
            assert_eq!(after, before + 2, "{label}");
            (
                semantics::NONSEMANTIC_SCORE_CONSTRUCTOR,
                vec![before],
                before + 1,
            )
        }
        IfFinishInput::Complete {
            condition,
            if_true,
            if_false,
            connective_used,
        } => {
            assert!(finish.complete, "{label}");
            assert_eq!(after, before + 3, "{label}");
            assert_eq!(
                entries[before].kind,
                CanonicalScoreSourceKind::Integer(i64::from(*connective_used)),
                "{label}"
            );
            (
                Score::IF,
                vec![
                    before + 1,
                    condition.index(),
                    if_true.index(),
                    if_false.index(),
                    before,
                ],
                before + 2,
            )
        }
    };
    assert_eq!(finish.score_handle.index(), node_index, "{label}");
    assert_eq!(
        entries[node_index - 1].kind,
        CanonicalScoreSourceKind::Integer(i64::from(constructor)),
        "{label}"
    );
    assert_eq!(
        entries[node_index].kind,
        CanonicalScoreSourceKind::Node(children),
        "{label}"
    );
    assert_eq!(
        entries[node_index].encoded_len,
        transcripts[node_index].len(),
        "{label}"
    );
}

pub(super) fn assert_match_finish_source(
    finish: &MatchFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    let (constructor, children, node_index) = match &finish.input {
        MatchFinishInput::Incomplete => {
            assert!(!finish.complete, "{label}");
            assert_eq!(after, before + 2, "{label}");
            (
                semantics::NONSEMANTIC_SCORE_CONSTRUCTOR,
                vec![before],
                before + 1,
            )
        }
        MatchFinishInput::Complete {
            target,
            cases,
            connective_used,
        } => {
            assert!(finish.complete, "{label}");
            assert_eq!(after, before + 3, "{label}");
            assert_eq!(
                entries[before].kind,
                CanonicalScoreSourceKind::Integer(i64::from(*connective_used)),
                "{label}"
            );
            let mut children = vec![before + 1, target.index()];
            children.extend(cases.iter().map(|handle| handle.index()));
            children.push(before);
            (Score::MATCH, children, before + 2)
        }
    };
    assert_eq!(finish.score_handle.index(), node_index, "{label}");
    assert_eq!(
        entries[node_index - 1].kind,
        CanonicalScoreSourceKind::Integer(i64::from(constructor)),
        "{label}"
    );
    assert_eq!(
        entries[node_index].kind,
        CanonicalScoreSourceKind::Node(children),
        "{label}"
    );
    assert_eq!(
        entries[node_index].encoded_len,
        transcripts[node_index].len(),
        "{label}"
    );
}

fn assert_empty_par_append(
    entries: &[CanonicalScoreSourceEntry],
    before: usize,
    label: &str,
) -> usize {
    assert!(before + 3 <= entries.len(), "{label}");
    assert_eq!(
        entries[before].kind,
        CanonicalScoreSourceKind::Integer(0),
        "{label}"
    );
    assert_eq!(
        entries[before + 1].kind,
        CanonicalScoreSourceKind::Integer(i64::from(Score::PAR)),
        "{label}"
    );
    assert_eq!(
        entries[before + 2].kind,
        CanonicalScoreSourceKind::Node(vec![before + 1, before]),
        "{label}"
    );
    before + 2
}

pub(super) fn assert_receive_finish_source(
    finish: &ReceiveFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    match &finish.input {
        ReceiveFinishInput::Incomplete => {
            assert!(!finish.complete, "{label}");
            assert_eq!(after, before + 2, "{label}");
            assert_eq!(finish.score_handle.index(), before + 1, "{label}");
            assert_eq!(
                entries[before].kind,
                CanonicalScoreSourceKind::Integer(i64::from(
                    semantics::NONSEMANTIC_SCORE_CONSTRUCTOR,
                )),
                "{label}"
            );
            assert_eq!(
                entries[before + 1].kind,
                CanonicalScoreSourceKind::Node(vec![before]),
                "{label}"
            );
        }
        ReceiveFinishInput::Complete {
            persistent,
            peek,
            binds,
            body,
            bind_count,
            connective_used,
            condition,
            cost,
        } => {
            assert!(finish.complete, "{label}");
            let (start, condition_handle) = match condition {
                Some(handle) => (before, handle.index()),
                None => (before + 3, assert_empty_par_append(entries, before, label)),
            };
            assert_eq!(after, start + 6, "{label}");
            assert_eq!(finish.score_handle.index(), start + 5, "{label}");
            for (offset, value) in [
                i64::from(*persistent),
                i64::from(*peek),
                i64::from(*bind_count),
                i64::from(*connective_used),
                i64::from(Score::RECEIVE),
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(
                    entries[start + offset].kind,
                    CanonicalScoreSourceKind::Integer(value),
                    "{label}"
                );
            }
            let mut children = vec![start + 4, start, start + 1];
            children.extend(binds.iter().map(|handle| handle.index()));
            children.extend([body.index(), start + 2, start + 3, condition_handle]);
            if let Some(cost) = cost {
                children.push(cost.index());
            }
            assert_eq!(
                entries[start + 5].kind,
                CanonicalScoreSourceKind::Node(children),
                "{label}"
            );
        }
    }
    let handle = finish.score_handle.index();
    assert_eq!(
        entries[handle].encoded_len,
        transcripts[handle].len(),
        "{label}"
    );
}

pub(super) fn assert_receive_bind_finish_source(
    finish: &ReceiveBindFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    match &finish.input {
        ReceiveBindFinishInput::SourceAbsent => {
            assert_eq!(after, before, "{label}");
            assert!(finish.score_handle.is_none(), "{label}");
        }
        ReceiveBindFinishInput::SourcePresent {
            source,
            patterns,
            remainder,
            cost,
        } => {
            assert_eq!(after, before + 2, "{label}");
            assert_eq!(
                finish.score_handle.map(|handle| handle.index()),
                Some(before + 1),
                "{label}"
            );
            assert_eq!(
                entries[before].kind,
                CanonicalScoreSourceKind::Integer(i64::from(Score::ABSENT)),
                "{label}"
            );
            let mut children = vec![source.index()];
            children.extend(patterns.iter().map(|handle| handle.index()));
            children.push(remainder.map_or(before, |handle| handle.index()));
            if let Some(cost) = cost {
                children.push(cost.index());
            }
            assert_eq!(
                entries[before + 1].kind,
                CanonicalScoreSourceKind::Node(children),
                "{label}"
            );
            assert_eq!(
                entries[before + 1].encoded_len,
                transcripts[before + 1].len(),
                "{label}"
            );
        }
    }
}

pub(super) fn assert_match_case_finish_source(
    finish: &MatchCaseFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    match &finish.input {
        MatchCaseFinishInput::RequiredAbsent => {
            assert!(!finish.required_present, "{label}");
            assert_eq!(after, before, "{label}");
            assert!(finish.score_handle.is_none(), "{label}");
        }
        MatchCaseFinishInput::RequiredPresent {
            pattern,
            source,
            guard,
            free_count,
        } => {
            assert!(finish.required_present, "{label}");
            assert_eq!(after, before + 5, "{label}");
            assert_eq!(
                finish.score_handle.map(|handle| handle.index()),
                Some(before + 4),
                "{label}"
            );
            let empty_guard = assert_empty_par_append(entries, before, label);
            assert_eq!(
                entries[before + 3].kind,
                CanonicalScoreSourceKind::Integer(i64::from(*free_count)),
                "{label}"
            );
            assert_eq!(
                entries[before + 4].kind,
                CanonicalScoreSourceKind::Node(vec![
                    pattern.index(),
                    source.index(),
                    before + 3,
                    guard.map_or(empty_guard, |handle| handle.index()),
                ]),
                "{label}"
            );
            assert_eq!(
                entries[before + 4].encoded_len,
                transcripts[before + 4].len(),
                "{label}"
            );
        }
    }
}

pub(super) fn assert_new_finish_source(
    finish: &NewFinishState,
    entries: &[CanonicalScoreSourceEntry],
    transcripts: &[Vec<u8>],
    label: &str,
) {
    let before = finish.source_len_before;
    let after = finish.source_len_after;
    assert!(before <= after && after <= entries.len(), "{label}");
    match &finish.input {
        NewFinishInput::Incomplete => {
            assert!(!finish.complete, "{label}");
            assert_eq!(after, before + 2, "{label}");
            assert_eq!(finish.score_handle.index(), before + 1, "{label}");
            assert_eq!(
                entries[before].kind,
                CanonicalScoreSourceKind::Integer(i64::from(
                    semantics::NONSEMANTIC_SCORE_CONSTRUCTOR,
                )),
                "{label}"
            );
            assert_eq!(
                entries[before + 1].kind,
                CanonicalScoreSourceKind::Node(vec![before]),
                "{label}"
            );
        }
        NewFinishInput::Complete {
            body,
            bind_count,
            uris,
            injections,
        } => {
            assert!(finish.complete, "{label}");
            let mut cursor = before;
            let mut uri_handles = Vec::new();
            if uris.is_empty() {
                assert_eq!(
                    entries[cursor].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(Score::ABSENT)),
                    "{label}"
                );
                uri_handles.push(cursor);
                cursor += 1;
            } else {
                for uri in uris {
                    assert_eq!(
                        entries[cursor].kind,
                        CanonicalScoreSourceKind::StringIndexed(uri.clone()),
                        "{label}"
                    );
                    uri_handles.push(cursor);
                    cursor += 1;
                }
            }
            let mut injection_handles = Vec::new();
            if injections.is_empty() {
                assert_eq!(
                    entries[cursor].kind,
                    CanonicalScoreSourceKind::Integer(i64::from(Score::ABSENT)),
                    "{label}"
                );
                injection_handles.push(cursor);
                cursor += 1;
            } else {
                for (key, value) in injections {
                    let value_handle = match value {
                        Some(handle) => handle.index(),
                        None => {
                            let empty = assert_empty_par_append(entries, cursor, label);
                            cursor += 3;
                            empty
                        }
                    };
                    assert_eq!(
                        entries[cursor].kind,
                        CanonicalScoreSourceKind::StringIndexed(key.clone()),
                        "{label}"
                    );
                    let key_handle = cursor;
                    cursor += 1;
                    assert_eq!(
                        entries[cursor].kind,
                        CanonicalScoreSourceKind::Node(vec![key_handle, value_handle]),
                        "{label}"
                    );
                    injection_handles.push(cursor);
                    cursor += 1;
                }
            }
            assert_eq!(after, cursor + 3, "{label}");
            assert_eq!(finish.score_handle.index(), cursor + 2, "{label}");
            assert_eq!(
                entries[cursor].kind,
                CanonicalScoreSourceKind::Integer(i64::from(*bind_count)),
                "{label}"
            );
            assert_eq!(
                entries[cursor + 1].kind,
                CanonicalScoreSourceKind::Integer(i64::from(Score::NEW)),
                "{label}"
            );
            let mut children = vec![cursor + 1, cursor];
            children.extend(uri_handles);
            children.extend(injection_handles);
            children.push(body.index());
            assert_eq!(
                entries[cursor + 2].kind,
                CanonicalScoreSourceKind::Node(children),
                "{label}"
            );
        }
    }
    let handle = finish.score_handle.index();
    assert_eq!(
        entries[handle].encoded_len,
        transcripts[handle].len(),
        "{label}"
    );
}

#[path = "rpb3_finish_invariant_properties.rs"]
mod rpb3_finish_invariant_properties;
