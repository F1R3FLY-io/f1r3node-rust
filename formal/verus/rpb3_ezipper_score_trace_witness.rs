//! A reachable EZipper score over a scored PathMap owner, as an integrated trace.
//!
//! The trace starts from a root Expr message (program 24) of `admission_root_initial` shape whose
//! fifteen source bytes hold field 33, an EZipper (program 32); the EZipper holds field 1, its
//! PathMap owner (program 31); the owner holds field 9, the eight bytes of an empty EPM1 snapshot.
//! Nine integrated steps push the EZipper and the owner, push the snapshot as an EPathMap frame,
//! hand it its EPathMap frame, take the two events of an empty snapshot, accept it into the owner,
//! score the owner at its EOF with the EPathMap owner score step, and return the scored owner into
//! the EZipper. On the last state the EZipper is at EOF with its PathMap present, and
//! `reachable_ezipper_eof_has_score_step` shows that its score step is Some and is the selected
//! finish. Each step is proved in its own lemmas over named constants. A fact about a constant is
//! passed between lemmas sealed, under a quantifier whose trigger `trace_seal` only the lemmas over
//! symbolic states write, so no query unfolds a definition over a constant it only carries along.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_live_handoff as handoff, rpb3_epm_source_accept_refinement as epm_source_accept,
    rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_ezipper_bool_slot_readable as ezipper_flags,
    rpb3_ezipper_snapshot_provenance as provenance, rpb3_integrated_dispatcher as dispatcher,
    rpb3_map_frame_refinement as map, rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as score_bridge, rpb3_score_arena_source_refinement as arena,
    rpb3_score_transcript as score, rpb3_wire_cursor_source_refinement as wire,
    LiveRuntimeProductAction, LiveRuntimeProductState, LiveSourceProductState, ProductScoreState,
};

verus! {

pub open spec fn trace_root_bytes() -> Seq<u8> {
    seq![
        0x8au8, 0x02u8, 0x0cu8, 0x0au8, 0x0au8, 0x4au8, 0x08u8, 0x45u8, 0x50u8, 0x4du8,
        0x31u8, 0x01u8, 0x00u8, 0x00u8, 0x00u8,
    ]
}

pub open spec fn trace_zipper_bytes() -> Seq<u8> {
    trace_root_bytes().subrange(3, 15)
}

pub open spec fn trace_owner_bytes() -> Seq<u8> {
    trace_root_bytes().subrange(5, 15)
}

pub open spec fn trace_snapshot_bytes() -> Seq<u8> {
    trace_root_bytes().subrange(7, 15)
}

pub open spec fn trace_region(bytes: Seq<u8>, start: int) -> epm::IndexedRegionImage {
    epm::IndexedRegionImage {
        bytes,
        positions: Seq::new(bytes.len(), |i: int| epm::SourceAnchor {
            source: 0,
            offset: (start + i) as usize,
        }),
        end: epm::SourceAnchor { source: 0, offset: 15 },
    }
}

pub open spec fn trace_root_region() -> epm::IndexedRegionImage {
    trace_region(trace_root_bytes(), 0)
}

pub open spec fn trace_zipper_region() -> epm::IndexedRegionImage {
    trace_region(trace_zipper_bytes(), 3)
}

pub open spec fn trace_owner_region() -> epm::IndexedRegionImage {
    trace_region(trace_owner_bytes(), 5)
}

pub open spec fn trace_snapshot_region() -> epm::IndexedRegionImage {
    trace_region(trace_snapshot_bytes(), 7)
}

pub open spec fn trace_indexed_backing(start: usize, len: usize) -> epm::IndexedBackingImage {
    epm::IndexedBackingImage::Borrowed {
        bytes: trace_root_bytes(),
        source: 0,
        absolute_start: 0,
        start,
        len,
        end_override: None,
    }
}

pub open spec fn trace_wire_backing(start: usize, len: usize) -> wire::WireRuntimeBackingImage {
    wire::WireRuntimeBackingImage::Borrowed {
        bytes: trace_root_bytes(),
        source: 0,
        absolute_start: 0,
        start,
        len,
        end_override: None,
    }
}

pub open spec fn trace_cursor(
    start: usize, len: usize, at: usize, last_field: u32, fields_read: usize,
) -> wire::WireRuntimeCursorImage {
    wire::wire_runtime_message_pre_read(trace_wire_backing(start, len), at, last_field, fields_read)
}

pub open spec fn trace_reader(bytes: Seq<u8>, at: usize, last_field: u32) -> wire::WireSourceState {
    wire::WireSourceState {
        cursor: admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: bytes.len() as usize,
            next: at,
            source_bytes: bytes,
        },
        last_field,
        enforce_numeric_order: false,
        terminal_error: None,
    }
}

pub open spec fn trace_field_start(offset: usize) -> wire::WirePhysicalPosition {
    wire::WirePhysicalPosition {
        logical_offset: 0,
        physical: wire::WireSourceAnchor { source: 0, offset },
        end: false,
    }
}

pub open spec fn trace_zipper_field() -> admission::IndexedCanonicalFieldModel {
    admission::IndexedCanonicalFieldModel {
        number: 33,
        encoded_start: 0,
        encoded_end: 15,
        payload_start: 3,
        payload_end: 15,
        key_width: 2,
        payload_width: 1,
        payload_length: 12,
        wire: 2,
        decoded_varint: 0,
        payload_bytes: trace_zipper_bytes(),
    }
}

pub open spec fn trace_owner_field() -> admission::IndexedCanonicalFieldModel {
    admission::IndexedCanonicalFieldModel {
        number: 1,
        encoded_start: 0,
        encoded_end: 12,
        payload_start: 2,
        payload_end: 12,
        key_width: 1,
        payload_width: 1,
        payload_length: 10,
        wire: 2,
        decoded_varint: 0,
        payload_bytes: trace_owner_bytes(),
    }
}

pub open spec fn trace_snapshot_field() -> admission::IndexedCanonicalFieldModel {
    admission::IndexedCanonicalFieldModel {
        number: 9,
        encoded_start: 0,
        encoded_end: 10,
        payload_start: 2,
        payload_end: 10,
        key_width: 1,
        payload_width: 1,
        payload_length: 8,
        wire: 2,
        decoded_varint: 0,
        payload_bytes: trace_snapshot_bytes(),
    }
}

pub open spec fn trace_root_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 0,
        program_type_id: 24,
        region_start: 0,
        region_end: 15,
        source_bytes: trace_root_bytes(),
        cursor: 0,
        slot_count: 1,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Expr,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: None,
        frame_kind: 1,
        body_id: 0,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

pub open spec fn trace_root_waiting_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        cursor: 15,
        last_slot: Some(0usize),
        seen: Set::empty().insert(0int),
        waiting_id: Some(0u64),
        waiting_kind: 1,
        waiting_slot: Some(0usize),
        waiting_tag: Some(33u32),
        ..trace_root_frame()
    }
}

pub open spec fn trace_offer(
    physical_start: usize, program_type_id: u16, region_end: usize, source_bytes: Seq<u8>,
    slot_count: usize, required: Set<int>, frame_kind: u8,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 0,
        physical_start,
        program_type_id,
        region_start: 0,
        region_end,
        source_bytes,
        cursor: 0,
        slot_count,
        last_slot: None,
        seen: Set::empty(),
        required,
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Other,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: None,
        frame_kind,
        body_id: 0,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

pub open spec fn trace_zipper_offer() -> admission::AdmissionFrameState {
    trace_offer(3, 32, 12, trace_zipper_bytes(), 6, Set::empty(), 1)
}

pub open spec fn trace_owner_offer() -> admission::AdmissionFrameState {
    trace_offer(5, 31, 10, trace_owner_bytes(), 4, Set::empty().insert(3int), 1)
}

pub open spec fn trace_snapshot_offer() -> admission::AdmissionFrameState {
    trace_offer(7, 0, 8, trace_snapshot_bytes(), 0, Set::empty(), 3)
}

pub open spec fn trace_zipper_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        return_id: Some(0u64),
        body_id: 1,
        ..trace_zipper_offer()
    }
}

pub open spec fn trace_zipper_waiting_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        cursor: 12,
        last_slot: Some(0usize),
        seen: Set::empty().insert(0int),
        waiting_id: Some(1u64),
        waiting_kind: 1,
        waiting_slot: Some(0usize),
        waiting_tag: Some(1u32),
        ..trace_zipper_frame()
    }
}

pub open spec fn trace_owner_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        return_id: Some(1u64),
        body_id: 2,
        ..trace_owner_offer()
    }
}

pub open spec fn trace_owner_waiting_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        cursor: 10,
        last_slot: Some(3usize),
        seen: Set::empty().insert(3int),
        waiting_id: Some(2u64),
        waiting_kind: 3,
        waiting_slot: Some(3usize),
        waiting_tag: Some(9u32),
        nested_started: 1,
        ..trace_owner_frame()
    }
}

pub open spec fn trace_snapshot_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        return_id: Some(2u64),
        body_id: 3,
        ..trace_snapshot_offer()
    }
}

pub open spec fn trace_snapshot_read_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState { cursor: 8, ..trace_snapshot_frame() }
}

pub open spec fn trace_owner_snapshot_return() -> admission::AdmissionReturnedSummary {
    admission::AdmissionReturnedSummary {
        slot: 3,
        tag: 9,
        body_id: 3,
        body_bytes: trace_snapshot_bytes(),
        score_handle: None,
        score_origin_body_id: None,
        kind: admission::AdmissionReturnedKind::EPathMapSnapshot {
            mode: 0,
            entries_canonical_path_stable: true,
        },
    }
}

pub open spec fn trace_owner_accepted_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        returns: seq![trace_owner_snapshot_return()],
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        nested_resolved: 1,
        ..trace_owner_waiting_frame()
    }
}

pub open spec fn trace_owner_scored_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        score_handle: Some(3usize),
        ..trace_owner_accepted_frame()
    }
}

pub open spec fn trace_zipper_payload() -> admission::AdmissionReturnedKind {
    admission::AdmissionReturnedKind::ExprPayload {
        score_present: true,
        canonical_fixed_point: true,
        canonical_path_stable: true,
    }
}

pub open spec fn trace_zipper_owner_return() -> admission::AdmissionReturnedSummary {
    admission::AdmissionReturnedSummary {
        slot: 0,
        tag: 1,
        body_id: 2,
        body_bytes: trace_owner_bytes(),
        score_handle: Some(3usize),
        score_origin_body_id: Some(2u64),
        kind: trace_zipper_payload(),
    }
}

pub open spec fn trace_zipper_complete_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        returns: seq![trace_zipper_owner_return()],
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        ..trace_zipper_waiting_frame()
    }
}

pub open spec fn trace_admission_0() -> admission::AdmissionState {
    admission::admission_initial_state(trace_root_frame())
}

pub open spec fn trace_admission_1() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![trace_root_waiting_frame(), trace_zipper_frame()],
        arena: Seq::empty(),
        root_end: 15,
        pushed: 2,
        completed: 0,
        next_obligation_id: 1,
    }
}

pub open spec fn trace_admission_2() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![trace_root_waiting_frame(), trace_zipper_waiting_frame(), trace_owner_frame()],
        arena: Seq::empty(),
        root_end: 15,
        pushed: 3,
        completed: 0,
        next_obligation_id: 2,
    }
}

pub open spec fn trace_admission_3() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![
            trace_root_waiting_frame(), trace_zipper_waiting_frame(), trace_owner_waiting_frame(),
            trace_snapshot_frame(),
        ],
        arena: Seq::empty(),
        root_end: 15,
        pushed: 4,
        completed: 0,
        next_obligation_id: 3,
    }
}

pub open spec fn trace_admission_5() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![
            trace_root_waiting_frame(), trace_zipper_waiting_frame(), trace_owner_waiting_frame(),
            trace_snapshot_read_frame(),
        ],
        arena: Seq::empty(),
        root_end: 15,
        pushed: 4,
        completed: 0,
        next_obligation_id: 3,
    }
}

pub open spec fn trace_admission_7() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![
            trace_root_waiting_frame(), trace_zipper_waiting_frame(), trace_owner_accepted_frame(),
        ],
        arena: Seq::empty(),
        root_end: 15,
        pushed: 4,
        completed: 1,
        next_obligation_id: 3,
    }
}

pub open spec fn trace_bridge_7() -> score_bridge::ScoreAdmissionBridge {
    score_bridge::ScoreAdmissionBridge {
        admission: trace_admission_7(),
        transcript: score::empty_arena(),
    }
}

pub open spec fn trace_bridge_8() -> score_bridge::ScoreAdmissionBridge {
    score_bridge::bridge_score_finish_selected(trace_bridge_7()).unwrap()
}

pub open spec fn trace_admission_9() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![trace_root_waiting_frame(), trace_zipper_complete_frame()],
        arena: trace_bridge_8().admission.arena,
        root_end: 15,
        pushed: 4,
        completed: 2,
        next_obligation_id: 3,
    }
}

pub open spec fn trace_epm_backing() -> epm::IndexedBackingImage {
    trace_indexed_backing(7, 8)
}

pub open spec fn trace_epm_prefix() -> epm::EpmPrefixImage {
    epm::EpmPrefixImage {
        mode: epm::EpmMode::Empty,
        trie_start: 7,
        trie_end: 7,
        value_count: 0,
        value_table_start: 8,
    }
}

pub open spec fn trace_epm_frame_0() -> epm::EpmFrame {
    epm::initial_epm_candidate(
        trace_epm_backing(), trace_snapshot_region(), trace_epm_prefix(), None, None,
    )
}

pub open spec fn trace_epm_frame_1() -> epm::EpmFrame {
    epm::EpmFrame {
        cursor: epm::EpmCursorImage { at: 1, ..trace_epm_frame_0().cursor },
        last_source: epm::SourceAnchor { source: 0, offset: 12 },
        ..trace_epm_frame_0()
    }
}

pub open spec fn trace_epm_frame_2() -> epm::EpmFrame {
    epm::EpmFrame {
        path: epm::PathTopology { root_left: true, ..trace_epm_frame_1().path },
        cursor: epm::EpmCursorImage { at: 2, ..trace_epm_frame_1().cursor },
        ..trace_epm_frame_1()
    }
}

pub open spec fn trace_regions_4() -> Seq<epm::IndexedRegionImage> {
    seq![trace_root_region(), trace_zipper_region(), trace_owner_region(), trace_snapshot_region()]
}

pub open spec fn trace_regions_3() -> Seq<epm::IndexedRegionImage> {
    seq![trace_root_region(), trace_zipper_region(), trace_owner_region()]
}

pub open spec fn trace_accepted_source() -> epm::LinkedEpmState {
    epm::LinkedEpmState {
        epm: trace_epm_frame_2(),
        admission: trace_admission_5(),
        indexed_regions: trace_regions_4(),
    }
}

pub open spec fn trace_runtime(
    bridge: score_bridge::ScoreAdmissionBridge,
) -> runtime::RuntimeFrameImage {
    runtime::RuntimeFrameImage {
        score: bridge,
        capacity: 4,
        frame_bytes: 1,
        max_frames: 4,
        max_storage_bytes: 4,
        max_capacity_bytes: 4,
    }
}

pub open spec fn trace_product(
    admission: admission::AdmissionState,
    transcript: score::ScoreArena,
    map_frames: Seq<Option<map::MapFrameModel>>,
    epm_frames: Seq<Option<epm::EpmFrame>>,
    indexed_regions: Seq<epm::IndexedRegionImage>,
    accepted_epm_sources: Seq<Option<epm::LinkedEpmState>>,
) -> ProductScoreState {
    ProductScoreState {
        bridge: score_bridge::ScoreAdmissionBridge { admission, transcript },
        map: map::IntegratedAdmissionState { admission, map_frames, transcript },
        epm_frames,
        indexed_regions,
        accepted_epm_sources,
    }
}

pub open spec fn trace_live(
    product: ProductScoreState,
    arena: arena::RuntimeScoreArena,
    wire: Seq<Option<wire::WireSourceState>>,
    cursors: Seq<Option<wire::WireRuntimeCursorImage>>,
) -> LiveRuntimeProductState {
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: trace_runtime(product.bridge),
            arena,
            wire,
        },
        cursors,
    }
}

pub open spec fn trace_empty_arena() -> arena::RuntimeScoreArena {
    arena::RuntimeScoreArena { entries: Seq::empty() }
}

pub open spec fn trace_root_cursor() -> wire::WireRuntimeCursorImage {
    trace_cursor(0, 15, 0, 0, 0)
}

pub open spec fn trace_root_read_cursor() -> wire::WireRuntimeCursorImage {
    trace_cursor(0, 15, 15, 33, 1)
}

pub open spec fn trace_zipper_cursor() -> wire::WireRuntimeCursorImage {
    trace_cursor(3, 12, 0, 0, 0)
}

pub open spec fn trace_zipper_read_cursor() -> wire::WireRuntimeCursorImage {
    trace_cursor(3, 12, 12, 1, 1)
}

pub open spec fn trace_owner_cursor() -> wire::WireRuntimeCursorImage {
    trace_cursor(5, 10, 0, 0, 0)
}

pub open spec fn trace_owner_read_cursor() -> wire::WireRuntimeCursorImage {
    trace_cursor(5, 10, 10, 9, 1)
}

pub open spec fn trace_root_read_reader() -> wire::WireSourceState {
    trace_reader(trace_root_bytes(), 15, 33)
}

pub open spec fn trace_zipper_read_reader() -> wire::WireSourceState {
    trace_reader(trace_zipper_bytes(), 12, 1)
}

pub open spec fn trace_owner_read_reader() -> wire::WireSourceState {
    trace_reader(trace_owner_bytes(), 10, 9)
}

pub open spec fn trace_live_0() -> LiveRuntimeProductState {
    trace_live(
        trace_product(
            trace_admission_0(), score::empty_arena(), seq![None], seq![None],
            seq![trace_root_region()], seq![None],
        ),
        trace_empty_arena(),
        seq![Some(trace_reader(trace_root_bytes(), 0, 0))],
        seq![Some(trace_root_cursor())],
    )
}

pub open spec fn trace_live_1() -> LiveRuntimeProductState {
    trace_live(
        trace_product(
            trace_admission_1(), score::empty_arena(), seq![None, None], seq![None, None],
            seq![trace_root_region(), trace_zipper_region()], seq![None, None],
        ),
        trace_empty_arena(),
        seq![Some(trace_root_read_reader()), Some(trace_reader(trace_zipper_bytes(), 0, 0))],
        seq![Some(trace_root_read_cursor()), Some(trace_zipper_cursor())],
    )
}

pub open spec fn trace_live_2() -> LiveRuntimeProductState {
    trace_live(
        trace_product(
            trace_admission_2(), score::empty_arena(), seq![None, None, None],
            seq![None, None, None], trace_regions_3(), seq![None, None, None],
        ),
        trace_empty_arena(),
        seq![
            Some(trace_root_read_reader()), Some(trace_zipper_read_reader()),
            Some(trace_reader(trace_owner_bytes(), 0, 0)),
        ],
        seq![
            Some(trace_root_read_cursor()), Some(trace_zipper_read_cursor()),
            Some(trace_owner_cursor()),
        ],
    )
}

pub open spec fn trace_epm_live(
    admission: admission::AdmissionState, epm_frame: Option<epm::EpmFrame>,
) -> LiveRuntimeProductState {
    trace_live(
        trace_product(
            admission, score::empty_arena(), seq![None, None, None, None],
            seq![None, None, None, epm_frame], trace_regions_4(), seq![None, None, None, None],
        ),
        trace_empty_arena(),
        seq![
            Some(trace_root_read_reader()), Some(trace_zipper_read_reader()),
            Some(trace_owner_read_reader()), None,
        ],
        seq![
            Some(trace_root_read_cursor()), Some(trace_zipper_read_cursor()),
            Some(trace_owner_read_cursor()), None,
        ],
    )
}

pub open spec fn trace_live_3() -> LiveRuntimeProductState {
    trace_epm_live(trace_admission_3(), None)
}

pub open spec fn trace_live_4() -> LiveRuntimeProductState {
    trace_epm_live(trace_admission_3(), Some(trace_epm_frame_0()))
}

pub open spec fn trace_live_5() -> LiveRuntimeProductState {
    trace_epm_live(trace_admission_5(), Some(trace_epm_frame_1()))
}

pub open spec fn trace_live_6() -> LiveRuntimeProductState {
    trace_epm_live(trace_admission_5(), Some(trace_epm_frame_2()))
}

pub open spec fn trace_owner_live(
    admission: admission::AdmissionState,
    transcript: score::ScoreArena,
    arena: arena::RuntimeScoreArena,
) -> LiveRuntimeProductState {
    trace_live(
        trace_product(
            admission, transcript, seq![None, None, None], seq![None, None, None],
            trace_regions_3(), seq![None, None, Some(trace_accepted_source())],
        ),
        arena,
        seq![
            Some(trace_root_read_reader()), Some(trace_zipper_read_reader()),
            Some(trace_owner_read_reader()),
        ],
        seq![
            Some(trace_root_read_cursor()), Some(trace_zipper_read_cursor()),
            Some(trace_owner_read_cursor()),
        ],
    )
}

pub open spec fn trace_live_7() -> LiveRuntimeProductState {
    trace_owner_live(trace_admission_7(), score::empty_arena(), trace_empty_arena())
}

pub open spec fn trace_live_8() -> LiveRuntimeProductState {
    trace_owner_live(
        trace_bridge_8().admission,
        trace_bridge_8().transcript,
        integrated::product_runtime_arena_from_score(trace_bridge_8().transcript),
    )
}

pub open spec fn trace_live_9() -> LiveRuntimeProductState {
    trace_live(
        trace_product(
            trace_admission_9(), trace_bridge_8().transcript, seq![None, None], seq![None, None],
            seq![trace_root_region(), trace_zipper_region()], seq![None, None],
        ),
        integrated::product_runtime_arena_from_score(trace_bridge_8().transcript),
        seq![Some(trace_root_read_reader()), Some(trace_zipper_read_reader())],
        seq![Some(trace_root_read_cursor()), Some(trace_zipper_read_cursor())],
    )
}

pub open spec fn trace_request(
    request_kind: runtime::SourceFrameRequestKind,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
) -> runtime::SourceFrameRequestImage {
    runtime::SourceFrameRequestImage { request_kind, field, child }
}

pub open spec fn trace_storage(
    request_kind: runtime::SourceFrameRequestKind, before_len: usize, after_len: usize,
) -> runtime::SourceFramePushStorageImage {
    runtime::SourceFramePushStorageImage {
        request_kind,
        before_len,
        after_len,
        before_capacity: 4,
        after_capacity: 4,
        frame_variant_bytes: 1,
        stats_frame_variant_bytes: 1,
        used_bytes: after_len,
        reserved_bytes: 4,
    }
}

pub open spec fn trace_push_source_action(
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    region: epm::IndexedRegionImage,
) -> admission::ConcreteFrameActionModel {
    admission::ConcreteFrameActionModel::PushIndexed {
        field,
        child,
        parent_source: integrated::product_admission_source_image(region),
        frame_bytes: 1,
    }
}

pub open spec fn trace_indexed_message_action(
    before: LiveRuntimeProductState, request: runtime::SourceFrameRequestImage,
) -> admission::ConcreteFrameActionModel {
    admission::ConcreteFrameActionModel::PushIndexed {
        field: request.field,
        child: request.child,
        parent_source: integrated::product_admission_source_image(
            before.source.product.indexed_regions.last(),
        ),
        frame_bytes: before.source.runtime.frame_bytes,
    }
}

pub open spec fn trace_push_action(
    request_kind: runtime::SourceFrameRequestKind,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    after_len: usize,
) -> LiveRuntimeProductAction {
    LiveRuntimeProductAction::MessagePush {
        request: trace_request(request_kind, field, child),
        actual_top_kind: Some(request_kind),
        after_len,
        after_capacity: 4,
        stats_frame_variant_bytes: 1,
    }
}

pub open spec fn trace_action_0() -> dispatcher::IntegratedAction {
    dispatcher::IntegratedAction::Live {
        live_action: trace_push_action(
            runtime::SourceFrameRequestKind::Message, trace_zipper_field(), trace_zipper_offer(), 2,
        ),
        route: dispatcher::IntegratedPushRoute::Indexed,
    }
}

pub open spec fn trace_action_1() -> dispatcher::IntegratedAction {
    dispatcher::IntegratedAction::Live {
        live_action: trace_push_action(
            runtime::SourceFrameRequestKind::Message, trace_owner_field(), trace_owner_offer(), 3,
        ),
        route: dispatcher::IntegratedPushRoute::Indexed,
    }
}

pub open spec fn trace_epm_push_action() -> LiveRuntimeProductAction {
    LiveRuntimeProductAction::MessageEpmPush {
        request: trace_request(
            runtime::SourceFrameRequestKind::EPathMap,
            trace_snapshot_field(),
            trace_snapshot_offer(),
        ),
        actual_top_kind: Some(runtime::SourceFrameRequestKind::EPathMap),
        after_len: 4,
        after_capacity: 4,
        stats_frame_variant_bytes: 1,
    }
}

pub open spec fn trace_action_2() -> dispatcher::IntegratedAction {
    dispatcher::IntegratedAction::Live {
        live_action: trace_epm_push_action(),
        route: dispatcher::IntegratedPushRoute::Plain,
    }
}

pub open spec fn trace_action_3() -> dispatcher::IntegratedAction {
    dispatcher::IntegratedAction::EpmHandoff {
        backing: trace_epm_backing(),
        plan: None,
        validated: None,
    }
}

pub open spec fn trace_value_event() -> epm::EpmEvent {
    epm::EpmEvent::Value { has_value: false, value_body: None, source_offset: 5 }
}

pub open spec fn trace_leave_event() -> epm::EpmEvent {
    epm::EpmEvent::Leave { source_offset: 5 }
}

pub open spec fn trace_plain(
    live_action: LiveRuntimeProductAction,
) -> dispatcher::IntegratedAction {
    dispatcher::IntegratedAction::Live {
        live_action,
        route: dispatcher::IntegratedPushRoute::Plain,
    }
}

pub open spec fn trace_action_4() -> dispatcher::IntegratedAction {
    trace_plain(LiveRuntimeProductAction::EpmContinue { event: trace_value_event() })
}

pub open spec fn trace_action_5() -> dispatcher::IntegratedAction {
    trace_plain(LiveRuntimeProductAction::EpmContinue { event: trace_leave_event() })
}

pub open spec fn trace_action_6() -> dispatcher::IntegratedAction {
    trace_plain(LiveRuntimeProductAction::EpmFinishAccept)
}

pub open spec fn trace_action_7() -> dispatcher::IntegratedAction {
    trace_plain(LiveRuntimeProductAction::MessageChildScore)
}

pub open spec fn trace_action_8() -> dispatcher::IntegratedAction {
    trace_plain(LiveRuntimeProductAction::MessageChildComplete)
}

pub open spec fn trace_active(state: LiveRuntimeProductState) -> dispatcher::IntegratedState {
    dispatcher::IntegratedState::Active { product: state }
}

pub open spec fn trace_states() -> Seq<dispatcher::IntegratedState> {
    seq![
        trace_active(trace_live_0()), trace_active(trace_live_1()), trace_active(trace_live_2()),
        trace_active(trace_live_3()), trace_active(trace_live_4()), trace_active(trace_live_5()),
        trace_active(trace_live_6()), trace_active(trace_live_7()), trace_active(trace_live_8()),
        trace_active(trace_live_9()),
    ]
}

pub open spec fn trace_actions() -> Seq<dispatcher::IntegratedAction> {
    seq![
        trace_action_0(), trace_action_1(), trace_action_2(), trace_action_3(), trace_action_4(),
        trace_action_5(), trace_action_6(), trace_action_7(), trace_action_8(),
    ]
}

pub open spec fn trace_owner_returned_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        returns: seq![
            admission::AdmissionReturnedSummary {
                kind: admission::AdmissionReturnedKind::Other,
                ..trace_owner_snapshot_return()
            },
        ],
        ..trace_owner_accepted_frame()
    }
}

pub open spec fn trace_returned_admission() -> admission::AdmissionState {
    admission::AdmissionState {
        frames: seq![
            trace_root_waiting_frame(), trace_zipper_waiting_frame(), trace_owner_returned_frame(),
        ],
        ..trace_admission_7()
    }
}

pub open spec fn trace_accepted_parent(
    parent: admission::AdmissionFrameState, mode: u8, stable: bool,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        returns: parent.returns.drop_last().push(
            admission::AdmissionReturnedSummary {
                kind: admission::AdmissionReturnedKind::EPathMapSnapshot {
                    mode,
                    entries_canonical_path_stable: stable,
                },
                ..parent.returns.last()
            },
        ),
        ..parent
    }
}

pub open spec fn trace_accepted_admission(
    base: admission::AdmissionState, mode: u8, stable: bool,
) -> admission::AdmissionState {
    admission::AdmissionState {
        frames: base.frames.update(
            base.frames.len() - 1, trace_accepted_parent(base.frames.last(), mode, stable),
        ),
        ..base
    }
}

pub open spec fn trace_seal(key: int) -> bool {
    true
}

pub open spec fn trace_leaf_seal(key: int) -> bool {
    true
}

pub open spec fn trace_sealed_wf(state: LiveRuntimeProductState) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> integrated::live_runtime_product_wf(state)
}

pub open spec fn trace_sealed_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: dispatcher::IntegratedAction,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> dispatcher::integrated_step(
        trace_active(before), trace_active(after), action,
    )
}

pub open spec fn trace_sealed_live_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: LiveRuntimeProductAction,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> integrated::live_runtime_product_step(before, after, action)
}

pub open spec fn trace_field_push(
    parent: wire::WireRuntimeCursorImage,
    region: epm::IndexedRegionImage,
    field: admission::IndexedCanonicalFieldModel,
    child_region: epm::IndexedRegionImage,
    next_parent: wire::WireRuntimeCursorImage,
    next_parent_reader: wire::WireSourceState,
) -> bool {
    &&& match wire::wire_runtime_next_field(parent) {
        Some(wire::WireConcreteTransition::Field {
            state: concrete_after,
            field: read,
            start: _,
        }) => read == field
            && integrated::runtime_wire_after_field(parent, concrete_after) == next_parent,
        _ => false,
    }
    &&& wire::wire_runtime_cursor_project(next_parent) == Some(next_parent_reader)
    &&& integrated::product_wire_indexed_source_agrees(parent, region)
    &&& integrated::product_indexed_child(region, field) == Some(child_region)
}

pub open spec fn trace_sealed_field_push(
    parent: wire::WireRuntimeCursorImage,
    region: epm::IndexedRegionImage,
    field: admission::IndexedCanonicalFieldModel,
    child_region: epm::IndexedRegionImage,
    next_parent: wire::WireRuntimeCursorImage,
    next_parent_reader: wire::WireSourceState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> trace_field_push(
        parent, region, field, child_region, next_parent, next_parent_reader,
    )
}

pub open spec fn trace_sealed_field_step(
    bytes: Seq<u8>,
    field: admission::IndexedCanonicalFieldModel,
    next: usize,
    number: u32,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> wire::wire_field_step(
        trace_reader(bytes, 0, 0).cursor, 0, false, false,
    ) == (wire::WireFieldResult::Field { field, next, last_field: number })
}

pub open spec fn trace_sealed_field_at(
    cursor: admission::IndexedCanonicalFieldCursorModel,
    field: admission::IndexedCanonicalFieldModel,
) -> bool {
    forall|key: int| #[trigger] trace_leaf_seal(key)
        ==> admission::indexed_field_at_cursor(cursor, field)
}

pub open spec fn trace_child_view(
    parent: wire::WireRuntimeCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    child: wire::WireRuntimeCursorImage,
    child_reader: wire::WireSourceState,
) -> bool {
    &&& integrated::live_runtime_child_cursor(parent, field) == Some(child)
    &&& wire::wire_runtime_cursor_project(child) == Some(child_reader)
}

pub open spec fn trace_sealed_child_view(
    parent: wire::WireRuntimeCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    child: wire::WireRuntimeCursorImage,
    child_reader: wire::WireSourceState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> trace_child_view(
        parent, field, child, child_reader,
    )
}

pub open spec fn trace_admission_push(
    state: admission::AdmissionState,
    action: admission::ConcreteFrameActionModel,
    next: admission::AdmissionState,
) -> bool {
    &&& admission::concrete_message_driver_step(state, action)
        == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: next })
    &&& map::integrated_source_message_action(state, action) == Some(action)
}

pub open spec fn trace_sealed_admission_push(
    state: admission::AdmissionState,
    action: admission::ConcreteFrameActionModel,
    next: admission::AdmissionState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> trace_admission_push(state, action, next)
}

pub open spec fn trace_sealed_map_step(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> integrated::product_map_step(before, after, action)
}

pub open spec fn trace_sealed_indexed_push(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> integrated::live_runtime_product_message_push_indexed_step(
            before, after, request, storage,
        )
}

pub open spec fn trace_plain_frames(state: map::IntegratedAdmissionState) -> bool {
    &&& state.map_frames.len() == state.admission.frames.len()
    &&& forall|i: int| 0 <= i < state.map_frames.len()
        ==> (#[trigger] state.map_frames[i]).is_none()
    &&& forall|i: int| 0 <= i < state.admission.frames.len()
        ==> (#[trigger] state.admission.frames[i]).frame_kind != 2
}

pub open spec fn trace_sealed_epm_push_arm(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> integrated::live_runtime_product_message_epm_push_indexed_observed_step(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        )
}

pub open spec fn trace_sealed_epm_continue_arm(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    event: epm::EpmEvent,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> unwind_product::live_runtime_internal_epm_unwind_step(before, after, event)
}

pub open spec fn trace_sealed_finish_accept_arm(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> integrated::live_runtime_product_epm_finish_accept_step(before, after)
}

pub open spec fn trace_sealed_child_score_arm(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> integrated::live_runtime_product_message_child_score_step(before, after)
}

pub open spec fn trace_sealed_child_complete_arm(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> integrated::live_runtime_product_message_child_complete_step(before, after)
}

pub open spec fn trace_handoff_parts(
    parent: admission::AdmissionFrameState,
    top: admission::AdmissionFrameState,
    parent_region: epm::IndexedRegionImage,
    region: epm::IndexedRegionImage,
    backing: epm::IndexedBackingImage,
    frame: epm::EpmFrame,
) -> bool {
    &&& epm::source_epm_initial_frame(top, backing, None, None) == Some(frame)
    &&& frame.snapshot == region
    &&& parent.frame_kind == 1
    &&& epm::indexed_image_wf(parent_region)
    &&& top.physical_start as int - parent.physical_start as int >= 0
    &&& top.physical_start as int - parent.physical_start as int + top.region_end as int
        <= parent_region.bytes.len()
    &&& region == epm::indexed_subview(
        parent_region,
        epm::ByteSpan {
            start: (top.physical_start as int - parent.physical_start as int) as usize,
            end: (top.physical_start as int - parent.physical_start as int
                + top.region_end as int) as usize,
        },
    )
}

pub open spec fn trace_sealed_handoff_parts(
    parent: admission::AdmissionFrameState,
    top: admission::AdmissionFrameState,
    parent_region: epm::IndexedRegionImage,
    region: epm::IndexedRegionImage,
    backing: epm::IndexedBackingImage,
    frame: epm::EpmFrame,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> trace_handoff_parts(
        parent, top, parent_region, region, backing, frame,
    )
}

pub open spec fn trace_sealed_event_step(
    before: epm::EpmFrame,
    after: epm::EpmFrame,
    event: epm::EpmEvent,
    next_id: u64,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key)
        ==> epm::event_step(before, after, event, next_id, epm::EpmAction::Continue)
}

pub open spec fn trace_sealed_end_step(frame: epm::EpmFrame, next_id: u64) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> epm::event_step(
        frame,
        epm_source_accept::epm_end_frame(frame),
        epm::EpmEvent::End,
        next_id,
        epm::EpmAction::Complete { mode: frame.mode, entries_stable: frame.entries_stable },
    )
}

pub open spec fn trace_completion_parts(
    state: admission::AdmissionState, base: admission::AdmissionState,
) -> bool {
    &&& epm_source_accept::epm_program31_parent(state)
    &&& state.frames.last().waiting_id.is_none()
    &&& state.frames.last().nested_started == state.frames.last().nested_resolved
    &&& state.frames.last().cursor == state.frames.last().region_end
    &&& state.frames.last().family == admission::AdmissionFamily::Other
    &&& state.frames.last().return_id.is_some()
    &&& admission::admission_complete_child_step(
        state, state.frames.last().return_id.unwrap(), 3, admission::AdmissionReturnedKind::Other,
    ) == Some(base)
}

pub open spec fn trace_sealed_completion_parts(
    state: admission::AdmissionState, base: admission::AdmissionState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> trace_completion_parts(state, base)
}

pub open spec fn trace_owner_parts(top: admission::AdmissionFrameState) -> bool {
    &&& provenance::epathmap_owner_frame(top)
    &&& top.waiting_id.is_none()
    &&& top.score_handle.is_none()
    &&& top.cursor == top.region_end
    &&& top.nested_started == top.nested_resolved
    &&& top.returns.len() > 0
    &&& top.returns.last().kind is EPathMapSnapshot
    &&& score_bridge::bridge_bool_slot(top, 1, 4).is_some()
    &&& admission::admission_slots_complete(top)
    &&& score::bytes_transcript(top.returns.last().body_bytes).len() + 22 <= usize::MAX
}

pub open spec fn trace_sealed_owner_parts(top: admission::AdmissionFrameState) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> trace_owner_parts(top)
}

pub open spec fn trace_sealed_end_read(cursor: wire::WireRuntimeCursorImage) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> wire::wire_runtime_next_field(cursor) == Some(
        wire::WireConcreteTransition::End {
            state: wire::wire_runtime_derive_cursor(cursor).unwrap(),
        },
    )
}

pub open spec fn trace_sealed_child_completion(
    state: admission::AdmissionState, next: admission::AdmissionState,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> admission::concrete_message_driver_step(
        state, admission::ConcreteFrameActionModel::Complete,
    ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: next })
}

pub open spec fn trace_sealed_trace(
    states: Seq<dispatcher::IntegratedState>, actions: Seq<dispatcher::IntegratedAction>,
) -> bool {
    forall|key: int| #[trigger] trace_seal(key) ==> dispatcher::integrated_trace(states, actions)
}

pub proof fn trace_bytes_nest()
    ensures
        trace_root_bytes().len() == 15,
        trace_root_bytes().subrange(0, 15) == trace_root_bytes(),
        trace_zipper_bytes().len() == 12,
        trace_owner_bytes().len() == 10,
        trace_snapshot_bytes().len() == 8,
        trace_root_bytes().subrange(0, 15).subrange(3, 15) == trace_zipper_bytes(),
        trace_zipper_bytes().subrange(2, 12) == trace_owner_bytes(),
        trace_owner_bytes().subrange(2, 10) == trace_snapshot_bytes(),
{
    let root = trace_root_bytes();
    assert(root.subrange(0, 15) =~= root);
    assert(root.subrange(0, 15).subrange(3, 15) =~= root.subrange(3, 15));
    assert(root.subrange(3, 15).subrange(2, 12) =~= root.subrange(5, 15));
    assert(root.subrange(5, 15).subrange(2, 10) =~= root.subrange(7, 15));
}

pub proof fn trace_snapshot_is_modeled_empty()
    ensures
        trace_snapshot_bytes() == score_bridge::bridge_epathmap_witness_snapshot(),
        trace_snapshot_bytes() == integrated::product_epm_modeled_empty_snapshot_bytes(),
{
    assert(trace_snapshot_bytes() =~= score_bridge::bridge_epathmap_witness_snapshot());
    assert(trace_snapshot_bytes() =~= integrated::product_epm_modeled_empty_snapshot_bytes());
}

pub proof fn trace_region_subview(bytes: Seq<u8>, start: int, sub_start: usize, sub_end: usize)
    requires
        sub_start <= sub_end,
        sub_end as int == bytes.len(),
    ensures
        epm::indexed_image_wf(trace_region(bytes, start)),
        epm::indexed_subview(
            trace_region(bytes, start), epm::ByteSpan { start: sub_start, end: sub_end },
        ) == trace_region(bytes.subrange(sub_start as int, sub_end as int), start + sub_start),
{
    let region = trace_region(bytes, start);
    let child = trace_region(bytes.subrange(sub_start as int, sub_end as int), start + sub_start);
    assert(region.positions.subrange(sub_start as int, sub_end as int) =~= child.positions);
}

pub proof fn trace_regions_nest()
    ensures
        epm::indexed_image_wf(trace_root_region()),
        epm::indexed_image_wf(trace_zipper_region()),
        epm::indexed_image_wf(trace_owner_region()),
        epm::indexed_image_wf(trace_snapshot_region()),
        integrated::product_indexed_child(trace_root_region(), trace_zipper_field())
            == Some(trace_zipper_region()),
        integrated::product_indexed_child(trace_zipper_region(), trace_owner_field())
            == Some(trace_owner_region()),
        integrated::product_indexed_child(trace_owner_region(), trace_snapshot_field())
            == Some(trace_snapshot_region()),
{
    trace_bytes_nest();
    trace_region_subview(trace_root_bytes(), 0, 3, 15);
    trace_region_subview(trace_zipper_bytes(), 3, 2, 12);
    trace_region_subview(trace_owner_bytes(), 5, 2, 10);
    trace_region_subview(trace_snapshot_bytes(), 7, 8, 8);
}

pub proof fn trace_cursor_projects(
    start: usize, len: usize, at: usize, last_field: u32, fields_read: usize,
)
    requires
        start as int + len as int == 15,
        at <= len,
    ensures ({
        let cursor = trace_cursor(start, len, at, last_field, fields_read);
        let bytes = trace_root_bytes().subrange(start as int, 15);
        &&& wire::wire_runtime_derive_cursor(cursor).is_some()
        &&& wire::wire_concrete_cursor_wf(wire::wire_runtime_derive_cursor(cursor).unwrap())
        &&& wire::wire_runtime_cursor_project(cursor) == Some(trace_reader(bytes, at, last_field))
        &&& integrated::product_wire_indexed_source_agrees(
            cursor, trace_region(bytes, start as int),
        )
        &&& wire::wire_runtime_project_region(trace_wire_backing(start, len)) == Some(
            wire::wire_borrowed_window_region(
                trace_root_bytes(), 0, 0, start, len, None, 0,
            ),
        )
    }),
{
    let backing = trace_wire_backing(start, len);
    assert(trace_root_bytes().len() == 15);
    wire::wire_runtime_borrowed_window_projects(trace_root_bytes(), 0, 0, start, len, None, 0);
    assert(backing == wire::wire_runtime_borrowed_window(
        trace_root_bytes(), 0, 0, start, len, None, 0,
    ));
    let observed = wire::wire_borrowed_window_region(trace_root_bytes(), 0, 0, start, len, None, 0);
    wire::wire_runtime_message_pre_read_projects(backing, observed, at, last_field, fields_read);
}

pub proof fn trace_root_cursor_projects()
    ensures
        wire::wire_runtime_derive_cursor(trace_root_cursor()).is_some(),
        wire::wire_concrete_cursor_wf(
            wire::wire_runtime_derive_cursor(trace_root_cursor()).unwrap(),
        ),
        wire::wire_runtime_cursor_project(trace_root_cursor())
            == Some(trace_reader(trace_root_bytes(), 0, 0)),
        integrated::product_wire_indexed_source_agrees(trace_root_cursor(), trace_root_region()),
        wire::wire_runtime_initial(trace_wire_backing(0, 15), false) == Some(trace_root_cursor()),
{
    trace_bytes_nest();
    trace_cursor_projects(0, 15, 0, 0, 0);
}

pub proof fn trace_root_frame_is_initial()
    ensures
        admission::admission_root_initial(trace_root_frame()),
        admission::admission_state_wf(trace_admission_0()),
{
    let root = trace_root_frame();
    assert forall|tag: u32| !admission::admission_required_descriptor_slot(24, tag, 0) by {}
    assert forall|slot: int| 0 <= slot < root.slot_count as int implies
        root.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(root.program_type_id, tag, slot)) by {
        assert(slot == 0);
    }
    assert(admission::admission_required_slots_generated(root));
    assert(admission::admission_root_initial(root));
    admission::admission_initial_wf(root);
}

pub proof fn trace_root_product_is_initial()
    ensures
        integrated::product_root_initial(trace_root_frame(), trace_indexed_backing(0, 15))
            == Some(trace_live_0().source.product),
        integrated::product_score_wf(trace_live_0().source.product),
        score_bridge::bridge_well_formed(trace_live_0().source.product.bridge),
{
    let root = trace_root_frame();
    let backing = trace_indexed_backing(0, 15);
    trace_root_frame_is_initial();
    trace_bytes_nest();
    map::integrated_initial_is_well_formed(root);
    let image = epm::indexed_backing_project(backing).unwrap();
    assert(image.positions =~= trace_root_region().positions);
    assert(epm::indexed_backing_project(backing) == Some(trace_root_region()));
    assert(integrated::product_root_initial(root, backing) == Some(trace_live_0().source.product));
    integrated::product_root_initial_exact(root, backing);
    score_bridge::bridge_initial_exact(root);
}

pub proof fn trace_initial_state_is_wf()
    ensures
        trace_sealed_wf(trace_live_0()),
{
    let state = trace_live_0();
    trace_root_product_is_initial();
    trace_root_frame_is_initial();
    trace_root_cursor_projects();
    trace_bytes_nest();
    score::empty_arena_well_formed();
    assert(runtime::runtime_frame_image_wf(state.source.runtime));
    assert(arena::runtime_projects_score(
        state.source.arena, state.source.product.bridge.transcript,
    ));
    assert forall|i: int| 0 <= i < state.source.wire.len() implies {
        let frame = state.source.product.bridge.admission.frames[i];
        match #[trigger] state.source.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        assert(i == 0);
    }
    assert(integrated::live_source_product_wf(state.source));
    assert forall|i: int| 0 <= i < state.cursors.len() implies
        match #[trigger] state.cursors[i] {
            None => true,
            Some(cursor) => integrated::product_wire_indexed_source_agrees(
                cursor, state.source.product.indexed_regions[i],
            ),
        } by {
        assert(i == 0);
    }
    assert forall|i: int| 0 <= i < state.cursors.len() implies {
        match (#[trigger] state.cursors[i], state.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire::wire_concrete_cursor_wf(wire::wire_runtime_derive_cursor(cursor).unwrap())
                &&& wire::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(i == 0);
    }
    assert(integrated::live_runtime_product_wf(state));
}

pub proof fn trace_start_invariants()
    ensures
        provenance::integrated_epathmap_score_nodes_shaped(trace_active(trace_live_0())),
        ezipper_flags::integrated_ezipper_flags_readable(trace_active(trace_live_0())),
{
    let state = trace_live_0();
    let frames = state.source.product.bridge.admission.frames;
    assert(frames == seq![trace_root_frame()]);
    assert forall|i: int| 0 <= i < frames.len() implies {
        &&& (#[trigger] frames[i]).returns.len() == 0
        &&& frames[i].score_handle.is_none()
    } by {
        assert(i == 0);
    }
    provenance::unscored_start_is_shaped(state);
    assert forall|i: int| 0 <= i < frames.len() implies {
        &&& (#[trigger] frames[i]).scalar_occurrences.len() == 0
        &&& !frames[i].seen.contains(4)
        &&& !frames[i].scalar_slots.contains(4)
    } by {
        assert(i == 0);
    }
    ezipper_flags::unrecorded_start_is_readable(state);
}

pub proof fn trace_field_read_seals(
    start: usize,
    len: usize,
    bytes: Seq<u8>,
    field: admission::IndexedCanonicalFieldModel,
    next: usize,
    number: u32,
    child_region: epm::IndexedRegionImage,
)
    requires
        start as int + len as int == 15,
        bytes == trace_root_bytes().subrange(start as int, 15),
        0 < next <= len,
        trace_sealed_field_step(bytes, field, next, number),
        field.encoded_start == 0,
        field.payload_start <= field.payload_end,
        field.payload_end == len,
        field.payload_bytes
            == bytes.subrange(field.payload_start as int, field.payload_end as int),
        child_region == trace_region(field.payload_bytes, start + field.payload_start),
    ensures
        trace_sealed_field_push(
            trace_cursor(start, len, 0, 0, 0), trace_region(bytes, start as int), field,
            child_region, trace_cursor(start, len, next, number, 1),
            trace_reader(bytes, next, number),
        ),
{
    assert(trace_seal(0));
    assert(trace_root_bytes().len() == 15);
    trace_cursor_projects(start, len, 0, 0, 0);
    trace_cursor_projects(start, len, next, number, 1);
    trace_region_subview(bytes, start as int, field.payload_start, field.payload_end);
    let cursor = trace_cursor(start, len, 0, 0, 0);
    let read_cursor = trace_cursor(start, len, next, number, 1);
    let derived = wire::wire_runtime_derive_cursor(cursor).unwrap();
    let reader = trace_reader(bytes, 0, 0);
    wire::wire_concrete_next_field_or_matches_model(derived);
    assert(wire::wire_source_next_field(reader) == wire::WireSourceTransition::Field {
        state: wire::wire_source_with_cursor(reader, next, number, None),
        field,
    });
    assert(wire::wire_runtime_next_field(cursor)
        == Some(wire::WireConcreteTransition::Field {
            state: wire::wire_runtime_derive_cursor(read_cursor).unwrap(),
            field,
            start: trace_field_start(start),
        }));
    assert(integrated::runtime_wire_after_field(
        cursor, wire::wire_runtime_derive_cursor(read_cursor).unwrap(),
    ) == read_cursor);
    assert(trace_field_push(
        cursor, trace_region(bytes, start as int), field, child_region, read_cursor,
        trace_reader(bytes, next, number),
    ));
}

pub proof fn trace_live_wf_parts(state: LiveRuntimeProductState)
    requires integrated::live_runtime_product_wf(state),
    ensures
        integrated::product_score_wf(state.source.product),
        integrated::product_frame_source_anchors_match(state.source.product),
        score_bridge::bridge_well_formed(state.source.product.bridge),
        map::integrated_admission_wf(state.source.product.map),
        admission::admission_state_wf(state.source.product.bridge.admission),
        runtime::runtime_frame_image_wf(state.source.runtime),
        state.source.runtime.score == state.source.product.bridge,
{
}

pub proof fn trace_step_extends(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    action: dispatcher::IntegratedAction,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_step(before, after, action),
    ensures
        trace_sealed_wf(after),
{
    assert(trace_seal(0));
    dispatcher::integrated_step_preserves_wf(trace_active(before), trace_active(after), action);
}

pub proof fn trace_live_step_lifts(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    live_action: LiveRuntimeProductAction,
    action: dispatcher::IntegratedAction,
)
    requires
        trace_sealed_live_step(before, after, live_action),
        !(live_action is MessagePush),
        !(live_action is MapValuePush),
        !(live_action is RootComplete),
        !(live_action is RootCompleteNoScore),
        action == trace_plain(live_action),
    ensures
        trace_sealed_step(before, after, action),
{
    assert(trace_seal(0));
    dispatcher::integrated_live_step_establishes_kind_guard(before, after, live_action);
}

pub proof fn trace_runtime_push(
    before: score_bridge::ScoreAdmissionBridge,
    after: admission::AdmissionState,
    request: runtime::SourceFrameRequestImage,
    parent_source: admission::AdmissionIndexedSourceImage,
)
    requires
        runtime::runtime_frame_image_wf(trace_runtime(before)),
        before.admission.frames.len() < 4,
        after.frames.len() == before.admission.frames.len() + 1,
        runtime::source_frame_request_kind_code(request.request_kind) == request.child.frame_kind,
        admission::concrete_message_driver_step(
            before.admission,
            admission::ConcreteFrameActionModel::PushIndexed {
                field: request.field,
                child: request.child,
                parent_source,
                frame_bytes: 1,
            },
        ) == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: after }),
    ensures ({
        let before_len = before.admission.frames.len() as usize;
        let storage = trace_storage(request.request_kind, before_len, (before_len + 1) as usize);
        &&& runtime::source_frame_push_observe_after(
            request.request_kind, Some(request.request_kind), before_len, 4,
            (before_len + 1) as usize, 4, 1, 1,
        ) == (runtime::SourceFramePushObservationResult::Success { storage })
        &&& runtime::source_checked_frame_request_push_indexed(
            trace_runtime(before), request, parent_source, storage,
        ) == Some(trace_runtime(score_bridge::ScoreAdmissionBridge {
            admission: after,
            transcript: before.transcript,
        }))
    }),
{
}

pub proof fn trace_indexed_push_assembles(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
    concrete_after: wire::WireConcreteCursorImage,
    start: wire::WirePhysicalPosition,
    child: wire::WireRuntimeCursorImage,
    next_parent_reader: wire::WireSourceState,
    child_reader: wire::WireSourceState,
)
    requires
        integrated::live_runtime_product_wf(before),
        integrated::product_frame_source_anchors_match(before.source.product),
        before.cursors.len() > 0,
        before.source.product.indexed_regions.len() > 0,
        before.cursors.last().is_some(),
        wire::wire_runtime_next_field(before.cursors.last().unwrap())
            == Some(wire::WireConcreteTransition::Field {
                state: concrete_after,
                field: request.field,
                start,
            }),
        integrated::live_runtime_child_cursor(before.cursors.last().unwrap(), request.field)
            == Some(child),
        wire::wire_runtime_cursor_project(
            integrated::runtime_wire_after_field(before.cursors.last().unwrap(), concrete_after),
        ) == Some(next_parent_reader),
        wire::wire_runtime_cursor_project(child) == Some(child_reader),
        integrated::product_wire_indexed_source_agrees(
            before.cursors.last().unwrap(), before.source.product.indexed_regions.last(),
        ),
        request.child.source_bytes == request.field.payload_bytes,
        request.child.frame_kind == 1 || request.child.frame_kind == 2,
        map::integrated_source_message_action(
            before.source.product.map.admission, trace_indexed_message_action(before, request),
        ) == Some(trace_indexed_message_action(before, request)),
        integrated::product_map_step(
            before.source.product,
            after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: trace_indexed_message_action(before, request),
            },
        ),
        runtime::source_checked_frame_request_push_indexed(
            before.source.runtime,
            request,
            integrated::product_admission_source_image(
                before.source.product.indexed_regions.last(),
            ),
            storage,
        ) == Some(after.source.runtime),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire.update(
            before.source.wire.len() - 1, Some(next_parent_reader),
        ).push(Some(child_reader)),
        after.cursors == before.cursors.update(
            before.cursors.len() - 1,
            Some(integrated::runtime_wire_after_field(
                before.cursors.last().unwrap(), concrete_after,
            )),
        ).push(Some(child)),
    ensures
        integrated::live_runtime_product_message_push_indexed_step(before, after, request, storage),
{
}

pub proof fn trace_push_enters_map(
    before: LiveRuntimeProductState,
    after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    child_region: epm::IndexedRegionImage,
    parent: wire::WireRuntimeCursorImage,
    next_parent: wire::WireRuntimeCursorImage,
    next_parent_reader: wire::WireSourceState,
)
    requires
        trace_sealed_wf(before),
        before.source.product.indexed_regions.len() > 0,
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
        trace_plain_frames(before.source.product.map),
        trace_sealed_field_push(
            parent, before.source.product.indexed_regions.last(), field, child_region,
            next_parent, next_parent_reader,
        ),
        trace_sealed_admission_push(
            before.source.product.bridge.admission,
            trace_push_source_action(field, child, before.source.product.indexed_regions.last()),
            after.bridge.admission,
        ),
        after.bridge.transcript == before.source.product.bridge.transcript,
        after.map.admission == after.bridge.admission,
        after.map.transcript == before.source.product.bridge.transcript,
        after.map.map_frames == before.source.product.map.map_frames.push(None),
        trace_plain_frames(after.map),
        after.indexed_regions == before.source.product.indexed_regions.push(child_region),
        after.epm_frames == before.source.product.epm_frames.push(None),
        after.accepted_epm_sources
            == integrated::product_epm_receipts_carried(before.source.product, after),
    ensures
        trace_sealed_map_step(
            before.source.product,
            after,
            map::IntegratedAdmissionAction::Message {
                action: trace_push_source_action(
                    field, child, before.source.product.indexed_regions.last(),
                ),
            },
        ),
{
    assert(trace_seal(0));
    let product = before.source.product;
    let action = trace_push_source_action(field, child, product.indexed_regions.last());
    admission::concrete_message_driver_preserves_wf_and_arena(product.bridge.admission, action);
    assert(map::integrated_admission_wf(after.map));
    assert(map::integrated_admission_driver_step(
        product.map, map::IntegratedAdmissionAction::Message { action },
    ) == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: after.map }));
}

pub proof fn trace_indexed_push_seals(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    before_len: usize,
    after_len: usize,
    parent: wire::WireRuntimeCursorImage,
    child_region: epm::IndexedRegionImage,
    next_parent: wire::WireRuntimeCursorImage,
    next_parent_reader: wire::WireSourceState,
    child: wire::WireRuntimeCursorImage,
    child_reader: wire::WireSourceState,
    storage: runtime::SourceFramePushStorageImage,
)
    requires
        trace_sealed_wf(before),
        before.source.runtime == trace_runtime(before.source.product.bridge),
        before_len == before.source.product.bridge.admission.frames.len(),
        after_len == before_len + 1,
        before_len < 4,
        after.source.product.bridge.admission.frames.len() == after_len,
        storage == trace_storage(request.request_kind, before_len, after_len),
        before.cursors.len() > 0,
        before.cursors.last() == Some(parent),
        before.source.product.indexed_regions.len() > 0,
        request.request_kind == runtime::SourceFrameRequestKind::Message,
        request.child.frame_kind == 1,
        request.child.source_bytes == request.field.payload_bytes,
        trace_sealed_field_push(
            parent, before.source.product.indexed_regions.last(), request.field, child_region,
            next_parent, next_parent_reader,
        ),
        trace_sealed_child_view(parent, request.field, child, child_reader),
        trace_sealed_admission_push(
            before.source.product.bridge.admission,
            trace_push_source_action(
                request.field, request.child, before.source.product.indexed_regions.last(),
            ),
            after.source.product.bridge.admission,
        ),
        trace_sealed_map_step(
            before.source.product,
            after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: trace_push_source_action(
                    request.field, request.child, before.source.product.indexed_regions.last(),
                ),
            },
        ),
        after.source.product.bridge.transcript == before.source.product.bridge.transcript,
        after.source.runtime == trace_runtime(after.source.product.bridge),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire.update(
            before.source.wire.len() - 1, Some(next_parent_reader),
        ).push(Some(child_reader)),
        after.cursors == before.cursors.update(
            before.cursors.len() - 1, Some(next_parent),
        ).push(Some(child)),
    ensures
        trace_sealed_indexed_push(before, after, request, storage),
{
    assert(trace_seal(0));
    let region = before.source.product.indexed_regions.last();
    let parent_source = integrated::product_admission_source_image(region);
    trace_live_wf_parts(before);
    trace_runtime_push(
        before.source.product.bridge, after.source.product.bridge.admission, request,
        parent_source,
    );
    assert(trace_indexed_message_action(before, request)
        == trace_push_source_action(request.field, request.child, region));
    match wire::wire_runtime_next_field(parent) {
        Some(wire::WireConcreteTransition::Field { state: concrete_after, field: _, start }) => {
            trace_indexed_push_assembles(
                before, after, request, storage, concrete_after, start, child,
                next_parent_reader, child_reader,
            );
        },
        _ => {},
    }
}

pub proof fn trace_indexed_push_lifts(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    before_len: usize,
    after_len: usize,
    storage: runtime::SourceFramePushStorageImage,
    action: dispatcher::IntegratedAction,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_indexed_push(before, after, request, storage),
        storage == trace_storage(request.request_kind, before_len, after_len),
        action == (dispatcher::IntegratedAction::Live {
            live_action: LiveRuntimeProductAction::MessagePush {
                request,
                actual_top_kind: Some(request.request_kind),
                after_len,
                after_capacity: 4,
                stats_frame_variant_bytes: 1,
            },
            route: dispatcher::IntegratedPushRoute::Indexed,
        }),
        before.source.runtime == trace_runtime(before.source.product.bridge),
        before_len == before.source.product.bridge.admission.frames.len(),
        after_len == before_len + 1,
        0 < before_len < 4,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
    ensures
        trace_sealed_step(before, after, action),
{
    assert(trace_seal(0));
}

pub proof fn trace_epm_push_assembles(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    storage: runtime::SourceFramePushStorageImage,
    concrete_after: wire::WireConcreteCursorImage,
    start: wire::WirePhysicalPosition,
    next_parent_reader: wire::WireSourceState,
)
    requires
        integrated::live_runtime_product_wf(before),
        before.cursors.len() > 0,
        before.source.product.indexed_regions.len() > 0,
        before.cursors.last().is_some(),
        wire::wire_runtime_next_field(before.cursors.last().unwrap())
            == Some(wire::WireConcreteTransition::Field {
                state: concrete_after,
                field: request.field,
                start,
            }),
        wire::wire_runtime_cursor_project(
            integrated::runtime_wire_after_field(before.cursors.last().unwrap(), concrete_after),
        ) == Some(next_parent_reader),
        integrated::product_wire_indexed_source_agrees(
            before.cursors.last().unwrap(), before.source.product.indexed_regions.last(),
        ),
        request.request_kind == runtime::SourceFrameRequestKind::EPathMap,
        request.child.frame_kind == 3,
        request.child.source_bytes == request.field.payload_bytes,
        map::integrated_source_message_action(
            before.source.product.map.admission, trace_indexed_message_action(before, request),
        ) == Some(trace_indexed_message_action(before, request)),
        integrated::product_map_step(
            before.source.product,
            after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: trace_indexed_message_action(before, request),
            },
        ),
        runtime::source_checked_frame_request_push_indexed(
            before.source.runtime,
            request,
            integrated::product_admission_source_image(
                before.source.product.indexed_regions.last(),
            ),
            storage,
        ) == Some(after.source.runtime),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire.update(
            before.source.wire.len() - 1, Some(next_parent_reader),
        ).push(None),
        after.cursors == before.cursors.update(
            before.cursors.len() - 1,
            Some(integrated::runtime_wire_after_field(
                before.cursors.last().unwrap(), concrete_after,
            )),
        ).push(None),
    ensures
        integrated::live_runtime_product_message_epm_push_indexed_step(
            before, after, request, storage,
        ),
{
}

pub proof fn trace_epm_push_seals(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    before_len: usize,
    after_len: usize,
    parent: wire::WireRuntimeCursorImage,
    child_region: epm::IndexedRegionImage,
    next_parent: wire::WireRuntimeCursorImage,
    next_parent_reader: wire::WireSourceState,
)
    requires
        trace_sealed_wf(before),
        before.source.runtime == trace_runtime(before.source.product.bridge),
        before_len == before.source.product.bridge.admission.frames.len(),
        after_len == before_len + 1,
        before_len < 4,
        after.source.product.bridge.admission.frames.len() == after_len,
        before.cursors.len() > 0,
        before.cursors.last() == Some(parent),
        before.source.product.indexed_regions.len() > 0,
        request.request_kind == runtime::SourceFrameRequestKind::EPathMap,
        request.child.frame_kind == 3,
        request.child.source_bytes == request.field.payload_bytes,
        trace_sealed_field_push(
            parent, before.source.product.indexed_regions.last(), request.field, child_region,
            next_parent, next_parent_reader,
        ),
        trace_sealed_admission_push(
            before.source.product.bridge.admission,
            trace_push_source_action(
                request.field, request.child, before.source.product.indexed_regions.last(),
            ),
            after.source.product.bridge.admission,
        ),
        trace_sealed_map_step(
            before.source.product,
            after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: trace_push_source_action(
                    request.field, request.child, before.source.product.indexed_regions.last(),
                ),
            },
        ),
        after.source.product.bridge.transcript == before.source.product.bridge.transcript,
        after.source.runtime == trace_runtime(after.source.product.bridge),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire.update(
            before.source.wire.len() - 1, Some(next_parent_reader),
        ).push(None),
        after.cursors == before.cursors.update(
            before.cursors.len() - 1, Some(next_parent),
        ).push(None),
    ensures
        trace_sealed_epm_push_arm(
            before, after, request, Some(request.request_kind), after_len, 4, 1,
        ),
{
    assert(trace_seal(0));
    let storage = trace_storage(request.request_kind, before_len, after_len);
    let region = before.source.product.indexed_regions.last();
    let parent_source = integrated::product_admission_source_image(region);
    trace_live_wf_parts(before);
    trace_runtime_push(
        before.source.product.bridge, after.source.product.bridge.admission, request,
        parent_source,
    );
    assert(trace_indexed_message_action(before, request)
        == trace_push_source_action(request.field, request.child, region));
    match wire::wire_runtime_next_field(parent) {
        Some(wire::WireConcreteTransition::Field { state: concrete_after, field: _, start }) => {
            trace_epm_push_assembles(
                before, after, request, storage, concrete_after, start, next_parent_reader,
            );
        },
        _ => {},
    }
}

pub proof fn trace_epm_push_lifts(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    request: runtime::SourceFrameRequestImage,
    actual_top_kind: Option<runtime::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
    live_action: LiveRuntimeProductAction,
)
    requires
        trace_sealed_epm_push_arm(
            before, after, request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        ),
        live_action == (LiveRuntimeProductAction::MessageEpmPush {
            request,
            actual_top_kind,
            after_len,
            after_capacity,
            stats_frame_variant_bytes,
        }),
    ensures
        trace_sealed_live_step(before, after, live_action),
{
    hide(integrated::live_runtime_product_message_epm_push_indexed_observed_step);
}

pub proof fn trace_epm_continue_lifts(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    event: epm::EpmEvent,
    live_action: LiveRuntimeProductAction,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_epm_continue_arm(before, after, event),
        live_action == (LiveRuntimeProductAction::EpmContinue { event }),
    ensures
        trace_sealed_live_step(before, after, live_action),
{
    hide(unwind_product::live_runtime_internal_epm_unwind_step);
}

pub proof fn trace_finish_accept_lifts(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    live_action: LiveRuntimeProductAction,
)
    requires
        trace_sealed_finish_accept_arm(before, after),
        live_action == LiveRuntimeProductAction::EpmFinishAccept,
    ensures
        trace_sealed_live_step(before, after, live_action),
{
    hide(integrated::live_runtime_product_epm_finish_accept_step);
}

pub proof fn trace_child_score_lifts(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    live_action: LiveRuntimeProductAction,
)
    requires
        trace_sealed_child_score_arm(before, after),
        live_action == LiveRuntimeProductAction::MessageChildScore,
    ensures
        trace_sealed_live_step(before, after, live_action),
{
    hide(integrated::live_runtime_product_message_child_score_step);
}

pub proof fn trace_child_complete_lifts(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    live_action: LiveRuntimeProductAction,
)
    requires
        trace_sealed_child_complete_arm(before, after),
        live_action == LiveRuntimeProductAction::MessageChildComplete,
    ensures
        trace_sealed_live_step(before, after, live_action),
{
    hide(integrated::live_runtime_product_message_child_complete_step);
}

pub proof fn trace_snapshot_prefix_is_empty()
    ensures
        trace_snapshot_bytes().len() == 8,
        epm::indexed_image_wf(trace_snapshot_region()),
        epm::indexed_backing_project(trace_epm_backing()) == Some(trace_snapshot_region()),
        epm::epm_prefix_from_bytes(trace_snapshot_bytes()) == Some(trace_epm_prefix()),
{
    let bytes = trace_snapshot_bytes();
    trace_bytes_nest();
    trace_snapshot_is_modeled_empty();
    let image = epm::indexed_backing_project(trace_epm_backing()).unwrap();
    assert(image.positions =~= trace_snapshot_region().positions);
    assert(epm::indexed_backing_project(trace_epm_backing()) == Some(trace_snapshot_region()));
    assert(bytes == integrated::product_epm_modeled_empty_snapshot_bytes());
    assert(bytes.len() == 8);
    assert(bytes[5] == 0 && bytes[6] == 0 && bytes[7] == 0);
    dispatcher::act_forward_witness_single_byte_varint(bytes, 6, 0);
    dispatcher::act_forward_witness_single_byte_varint(bytes, 7, 0);
    assert(epm::canonical_varint_at(bytes, 6)
        == Some(epm::CanonicalVarintImage { value: 0, end: 7 }));
    assert(epm::canonical_varint_at(bytes, 7)
        == Some(epm::CanonicalVarintImage { value: 0, end: 8 }));
}

pub proof fn trace_epm_frame_layout(frame: epm::EpmFrame)
    requires
        frame.snapshot == trace_snapshot_region(),
        frame.backing == trace_epm_backing(),
        frame.mode == epm::EpmMode::Empty,
        frame.trie_start == 7,
        frame.trie_end == 7,
        frame.value_table_start == 8,
        frame.mode_offset == 5,
        frame.cursor.declared_value_count == 0,
        frame.cursor.act_plan.is_none(),
        frame.cursor.validated_plan.is_none(),
        frame.cursor.at <= 2,
        frame.cursor.value_table_offset == 8,
        frame.cursor.map_values_seen == 0,
        frame.path.path_bytes.len() == frame.path.path_positions.len(),
    ensures
        epm::frame_layout_wf(frame),
        epm::cursor_event_count(frame) == 2,
{
    trace_snapshot_prefix_is_empty();
}

pub proof fn trace_handoff_seals(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    parent: admission::AdmissionFrameState,
    top: admission::AdmissionFrameState,
    parent_region: epm::IndexedRegionImage,
    region: epm::IndexedRegionImage,
    frame: epm::EpmFrame,
    action: dispatcher::IntegratedAction,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_handoff_parts(parent, top, parent_region, region, backing, frame),
        before.source.product.bridge.admission.frames.len() >= 2,
        parent == before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ],
        top == before.source.product.bridge.admission.frames.last(),
        parent_region == before.source.product.indexed_regions[
            before.source.product.indexed_regions.len() - 2
        ],
        region == before.source.product.indexed_regions.last(),
        before.source.product.epm_frames.last().is_none(),
        after.source.product.bridge == before.source.product.bridge,
        after.source.product.map == before.source.product.map,
        after.source.product.indexed_regions == before.source.product.indexed_regions,
        after.source.product.epm_frames == before.source.product.epm_frames.update(
            before.source.product.epm_frames.len() - 1, Some(frame),
        ),
        after.source.product.accepted_epm_sources == integrated::product_epm_receipts_carried(
            before.source.product, after.source.product,
        ),
        after.source.runtime == before.source.runtime,
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire,
        after.cursors == before.cursors,
        action == (dispatcher::IntegratedAction::EpmHandoff {
            backing,
            plan: None,
            validated: None,
        }),
    ensures
        trace_sealed_step(before, after, action),
{
    assert(trace_seal(0));
    reveal(handoff::live_runtime_epm_handoff_step);
    trace_live_wf_parts(before);
    let old = before.source.product;
    let state = old.bridge.admission;
    let regions = old.indexed_regions;
    let linked = epm::LinkedEpmState { epm: frame, admission: state, indexed_regions: regions };
    assert(epm::linked_indexed_sources(linked));
    assert(epm::epm_handoff_parent_source(state, regions, frame.snapshot));
    assert(epm::linked_map_epm_handoff(state, regions, backing, None, None) == Some(linked));
    assert(integrated::product_epm_handoff_step(
        old, after.source.product, backing, None, None,
    ));
    assert(handoff::live_runtime_epm_handoff_step(before, after, backing, None, None));
    dispatcher::integrated_epm_handoff_is_integrated_step(before, after, backing, None, None);
}

pub proof fn trace_continue_seals(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    event: epm::EpmEvent,
    prior_frame: epm::EpmFrame,
    next_frame: epm::EpmFrame,
    next_id: u64,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_event_step(prior_frame, next_frame, event, next_id),
        next_id == before.source.product.bridge.admission.next_obligation_id,
        before.source.product.epm_frames.last() == Some(prior_frame),
        before.source.product.bridge.admission.frames.last().waiting_id.is_none(),
        before.source.product.bridge.admission.frames.last().cursor
            <= prior_frame.cursor.value_table_offset,
        next_frame.cursor.value_table_offset
            <= before.source.product.bridge.admission.frames.last().region_end,
        after.source.product.bridge.admission == dispatcher::integrated_epm_cursor_admission(
            before.source.product.bridge.admission, next_frame.cursor.value_table_offset,
        ),
        after.source.product.bridge.transcript == before.source.product.bridge.transcript,
        after.source.product.map.admission == after.source.product.bridge.admission,
        after.source.product.map.transcript == before.source.product.map.transcript,
        after.source.product.map.map_frames == before.source.product.map.map_frames,
        after.source.product.indexed_regions == before.source.product.indexed_regions,
        after.source.product.epm_frames == before.source.product.epm_frames.update(
            before.source.product.epm_frames.len() - 1, Some(next_frame),
        ),
        after.source.product.accepted_epm_sources == integrated::product_epm_receipts_carried(
            before.source.product, after.source.product,
        ),
        after.source.runtime == runtime::runtime_after_admission_step(
            before.source.runtime, after.source.product.bridge.admission,
        ),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire,
        after.cursors == before.cursors,
    ensures
        trace_sealed_epm_continue_arm(before, after, event),
{
    assert(trace_seal(0));
    trace_live_wf_parts(before);
    let old = before.source.product;
    let new = after.source.product;
    let consumed = next_frame.cursor.value_table_offset;
    let last = old.bridge.admission.frames.len() - 1;
    let prior = epm::LinkedEpmState {
        epm: prior_frame,
        admission: old.bridge.admission,
        indexed_regions: old.indexed_regions,
    };
    let next = epm::LinkedEpmState {
        epm: next_frame,
        admission: new.bridge.admission,
        indexed_regions: new.indexed_regions,
    };
    assert(old.epm_frames[last] == Some(prior_frame));
    assert(epm::indexed_image_wf(old.indexed_regions[last]));
    assert(old.bridge.admission.frames[last].frame_kind == 3);
    assert(prior_frame.snapshot == old.indexed_regions[last]);
    dispatcher::integrated_epm_cursor_admission_preserves_wf(old.bridge.admission, consumed);
    assert(integrated::product_linked_top(old) == Some(prior));
    assert(integrated::product_linked_top(new) == Some(next));
    assert(epm::linked_indexed_sources(prior));
    assert forall|i: int| 0 <= i < next.admission.frames.len() implies
        epm::indexed_image_wf(#[trigger] next.indexed_regions[i])
            && next.indexed_regions[i].bytes == next.admission.frames[i].source_bytes
            && epm::indexed_position_at(next.indexed_regions[i], 0).source
                == next.admission.frames[i].physical_source
            && epm::indexed_position_at(next.indexed_regions[i], 0).offset
                == next.admission.frames[i].physical_start by {
        assert(epm::indexed_image_wf(old.indexed_regions[i]));
        if i != last {
            assert(next.admission.frames[i] == old.bridge.admission.frames[i]);
        }
    }
    assert(epm::linked_indexed_sources(next));
    assert(epm::linked_epm_continue(prior, next, event));
    assert(integrated::product_epm_continue_step(old, new, event));
}

pub proof fn trace_finish_accept_seals(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    prior_frame: epm::EpmFrame,
    next_id: u64,
    base: admission::AdmissionState,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_end_step(prior_frame, next_id),
        trace_sealed_completion_parts(before.source.product.bridge.admission, base),
        next_id == before.source.product.bridge.admission.next_obligation_id,
        before.source.product.epm_frames.last() == Some(prior_frame),
        before.cursors.len() > 1,
        before.cursors.last().is_none(),
        after.source.product.bridge.admission == trace_accepted_admission(
            base, epm_source_accept::epm_mode_code(prior_frame.mode), prior_frame.entries_stable,
        ),
        after.source.product.bridge.transcript == before.source.product.bridge.transcript,
        after.source.product.map.admission == after.source.product.bridge.admission,
        after.source.product.map.transcript == before.source.product.map.transcript,
        after.source.product.map.map_frames == before.source.product.map.map_frames.drop_last(),
        after.source.product.epm_frames == before.source.product.epm_frames.drop_last(),
        after.source.product.indexed_regions == before.source.product.indexed_regions.drop_last(),
        after.source.product.accepted_epm_sources == integrated::product_epm_receipts_carried(
            before.source.product, after.source.product,
        ).update(
            after.source.product.bridge.admission.frames.len() - 1,
            Some(epm::LinkedEpmState {
                epm: prior_frame,
                admission: before.source.product.bridge.admission,
                indexed_regions: before.source.product.indexed_regions,
            }),
        ),
        after.source.runtime == runtime::runtime_after_admission_step(
            before.source.runtime, after.source.product.bridge.admission,
        ),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire.drop_last(),
        after.cursors == before.cursors.drop_last(),
    ensures
        trace_sealed_finish_accept_arm(before, after),
{
    assert(trace_seal(0));
    trace_live_wf_parts(before);
    let old = before.source.product;
    let new = after.source.product;
    let last = old.bridge.admission.frames.len() - 1;
    let prior = epm::LinkedEpmState {
        epm: prior_frame,
        admission: old.bridge.admission,
        indexed_regions: old.indexed_regions,
    };
    let finished = epm_source_accept::epm_end_linked(prior);
    assert(old.epm_frames[last] == Some(prior_frame));
    assert(epm::indexed_image_wf(old.indexed_regions[last]));
    assert(old.bridge.admission.frames[last].frame_kind == 3);
    assert(prior_frame.snapshot == old.indexed_regions[last]);
    assert(admission::admission_frame_wf(old.bridge.admission.frames[last]));
    assert(admission::admission_family_admissible(old.bridge.admission.frames[last]));
    assert(integrated::product_linked_top(old) == Some(prior));
    assert(epm::linked_indexed_sources(prior));
    assert(epm_source_accept::epm_source_end(prior, finished));
    assert(epm_source_accept::epm_source_finish_accept(prior) == Some(new.bridge.admission));
    assert(integrated::product_epm_finish_accept_step(old, new));
}

pub proof fn trace_end_read_seals(start: usize, len: usize, last_field: u32, fields_read: usize)
    requires
        start as int + len as int == 15,
    ensures
        trace_sealed_end_read(trace_cursor(start, len, len, last_field, fields_read)),
{
    assert(trace_root_bytes().len() == 15);
    trace_cursor_projects(start, len, len, last_field, fields_read);
    let cursor = trace_cursor(start, len, len, last_field, fields_read);
    let derived = wire::wire_runtime_derive_cursor(cursor).unwrap();
    let bytes = trace_root_bytes().subrange(start as int, 15);
    let reader = trace_reader(bytes, len, last_field);
    wire::wire_concrete_next_field_or_matches_model(derived);
    assert(bytes.len() == len);
    assert(wire::wire_cursor_shape(reader.cursor));
    assert(wire::wire_field_step(reader.cursor, last_field, false, false)
        == wire::WireFieldResult::End);
    assert(wire::wire_source_next_field(reader)
        == (wire::WireSourceTransition::End { state: reader }));
    assert(wire::wire_runtime_next_field(cursor)
        == Some(wire::WireConcreteTransition::End { state: derived }));
}

pub proof fn trace_owner_has_no_unscored_kind(image: runtime::RuntimeFrameImage)
    requires
        image.score.admission.frames.last().family == admission::AdmissionFamily::Other,
        image.score.admission.frames.last().program_type_id == 31,
    ensures
        runtime::runtime_noscore_child_kind(image).is_none(),
{
}

pub proof fn trace_owner_score_shapes(
    before: LiveRuntimeProductState,
    top: admission::AdmissionFrameState,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_owner_parts(top),
        before.source.product.bridge.admission.frames.len() == 3,
        top == before.source.product.bridge.admission.frames.last(),
        before.source.product.bridge.transcript.entries.len() == 0,
    ensures
        score_bridge::bridge_score_finish_selected(before.source.product.bridge).is_some(),
        ({
            let old = before.source.product.bridge;
            let next = score_bridge::bridge_score_finish_selected(old).unwrap();
            &&& next.admission.frames.len() == 3
            &&& next.admission.frames[0] == old.admission.frames[0]
            &&& next.admission.frames[1] == old.admission.frames[1]
            &&& next.admission.frames[2] == admission::AdmissionFrameState {
                score_handle: Some(3usize),
                ..top
            }
            &&& next.admission.pushed == old.admission.pushed
            &&& next.admission.completed == old.admission.completed
            &&& next.admission.root_end == old.admission.root_end
            &&& next.admission.next_obligation_id == old.admission.next_obligation_id
            &&& next.transcript.entries.len() == 4
            &&& next.admission.arena.len() == 4
            &&& next.admission.arena[3].body_id == top.body_id
            &&& next.admission.arena[3].semantic
            &&& next.admission.arena[3].encoded_len > 0
            &&& next.admission.arena[3].transcript_bytes.len()
                == next.admission.arena[3].encoded_len
        }),
{
    assert(trace_seal(0));
    trace_live_wf_parts(before);
    let old = before.source.product.bridge;
    let snapshot = top.returns.last().body_bytes;
    provenance::owner_finish_selects_owner_step(old);
    score_bridge::bridge_pathmap_score_append_succeeds(old, snapshot, 13i32, 1usize, 4u32);
    score_bridge::bridge_pathmap_score_append_exact(old, snapshot, 13i32, 1usize, 4u32);
    assert(admission::admission_family_complete(top));
    assert(score_bridge::bridge_epathmap_owner_score_step(old)
        == score_bridge::bridge_pathmap_score_append(old, snapshot, 13i32, 1usize, 4u32));
    let next = score_bridge::bridge_score_finish_selected(old).unwrap();
    assert(next.admission.frames.last() == next.admission.frames[2]);
    assert(next.admission.arena.len() == next.transcript.entries.len());
}

pub proof fn trace_child_score_seals(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    top: admission::AdmissionFrameState,
    cursor: wire::WireRuntimeCursorImage,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_owner_parts(top),
        trace_sealed_end_read(cursor),
        before.source.product.bridge.admission.frames.len() > 1,
        top == before.source.product.bridge.admission.frames.last(),
        before.cursors.len() > 1,
        before.cursors.last() == Some(cursor),
        before.source.product.map.map_frames.last().is_none(),
        score_bridge::bridge_score_finish_selected(before.source.product.bridge)
            == Some(after.source.product.bridge),
        after.source.product.map.admission == after.source.product.bridge.admission,
        after.source.product.map.transcript == after.source.product.bridge.transcript,
        after.source.product.map.map_frames == before.source.product.map.map_frames,
        after.source.product.epm_frames == before.source.product.epm_frames,
        after.source.product.indexed_regions == before.source.product.indexed_regions,
        after.source.product.accepted_epm_sources == integrated::product_epm_receipts_carried(
            before.source.product, after.source.product,
        ),
        after.source.runtime == (runtime::RuntimeFrameImage {
            score: after.source.product.bridge,
            ..before.source.runtime
        }),
        after.source.arena == integrated::product_runtime_arena_from_score(
            after.source.product.bridge.transcript,
        ),
        after.source.wire == before.source.wire,
        after.cursors == before.cursors,
    ensures
        trace_sealed_child_score_arm(before, after),
{
    assert(trace_seal(0));
    trace_live_wf_parts(before);
    trace_owner_has_no_unscored_kind(before.source.runtime);
    score_bridge::bridge_score_finish_selected_is_source_step(before.source.product.bridge);
    assert(integrated::product_message_child_score_step(
        before.source.product, after.source.product,
    ));
}

pub proof fn trace_complete_enters_map(before: LiveRuntimeProductState, after: ProductScoreState)
    requires
        trace_sealed_wf(before),
        trace_sealed_child_completion(
            before.source.product.bridge.admission, after.bridge.admission,
        ),
        before.source.product.bridge.admission.frames.len() > 1,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
        before.source.product.epm_frames.len() > 1,
        before.source.product.epm_frames.last().is_none(),
        before.source.product.epm_frames[before.source.product.epm_frames.len() - 2].is_none(),
        after.bridge.transcript == before.source.product.bridge.transcript,
        after.map.admission == after.bridge.admission,
        after.map.transcript == before.source.product.bridge.transcript,
        after.map.map_frames == before.source.product.map.map_frames.subrange(
            0, before.source.product.map.map_frames.len() - 1,
        ),
        trace_plain_frames(after.map),
        after.indexed_regions == before.source.product.indexed_regions.drop_last(),
        after.epm_frames == before.source.product.epm_frames.drop_last(),
        after.accepted_epm_sources
            == integrated::product_epm_receipts_carried(before.source.product, after),
    ensures
        trace_sealed_map_step(
            before.source.product,
            after,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ),
{
    assert(trace_seal(0));
    trace_live_wf_parts(before);
    let product = before.source.product;
    let action = admission::ConcreteFrameActionModel::Complete;
    admission::concrete_message_driver_preserves_wf_and_arena(product.bridge.admission, action);
    assert(map::integrated_admission_wf(after.map));
    assert(map::integrated_admission_driver_step(
        product.map, map::IntegratedAdmissionAction::Message { action },
    ) == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted { state: after.map }));
}

pub proof fn trace_child_complete_seals(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    cursor: wire::WireRuntimeCursorImage,
)
    requires
        trace_sealed_wf(before),
        trace_sealed_end_read(cursor),
        trace_sealed_map_step(
            before.source.product,
            after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ),
        before.cursors.len() > 1,
        before.cursors.last() == Some(cursor),
        before.source.product.bridge.admission.frames.len() > 1,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
        before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind == 1,
        after.source.runtime == runtime::runtime_after_admission_step(
            before.source.runtime, after.source.product.bridge.admission,
        ),
        after.source.arena == before.source.arena,
        after.source.wire == before.source.wire.drop_last(),
        after.cursors == before.cursors.drop_last(),
    ensures
        trace_sealed_child_complete_arm(before, after),
{
    assert(trace_seal(0));
}

pub proof fn trace_root_field_model()
    ensures
        trace_sealed_field_step(trace_root_bytes(), trace_zipper_field(), 15, 33),
        trace_sealed_field_at(trace_reader(trace_root_bytes(), 0, 0).cursor, trace_zipper_field()),
{
    trace_bytes_nest();
    let bytes = trace_root_bytes();
    let cursor = trace_reader(bytes, 0, 0).cursor;
    reveal_with_fuel(wire::wire_varint_loop, 2);
    reveal_with_fuel(admission::varint_multiplier, 2);
    assert(wire::wire_varint_read(bytes, 0, 15)
        == wire::WireVarintResult::Value { value: 266, next: 2 });
    assert(wire::wire_varint_read(bytes, 2, 15)
        == wire::WireVarintResult::Value { value: 12, next: 3 });
    assert(wire::wire_field_step(cursor, 0, false, false)
        == (wire::WireFieldResult::Field {
            field: trace_zipper_field(),
            next: 15,
            last_field: 33,
        }));
    wire::wire_field_step_refines(cursor, 0, false, false);
}

pub proof fn trace_root_field_is_read()
    ensures
        trace_sealed_field_push(
            trace_root_cursor(), trace_root_region(), trace_zipper_field(), trace_zipper_region(),
            trace_root_read_cursor(), trace_root_read_reader(),
        ),
{
    trace_bytes_nest();
    trace_root_field_model();
    trace_field_read_seals(
        0, 15, trace_root_bytes(), trace_zipper_field(), 15, 33, trace_zipper_region(),
    );
}

pub proof fn trace_root_child_cursor()
    ensures
        trace_sealed_child_view(
            trace_root_cursor(), trace_zipper_field(), trace_zipper_cursor(),
            trace_reader(trace_zipper_bytes(), 0, 0),
        ),
{
    trace_bytes_nest();
    trace_root_cursor_projects();
    trace_cursor_projects(3, 12, 0, 0, 0);
    assert(trace_child_view(
        trace_root_cursor(), trace_zipper_field(), trace_zipper_cursor(),
        trace_reader(trace_zipper_bytes(), 0, 0),
    ));
}

pub proof fn trace_zipper_push_admission()
    ensures
        trace_sealed_admission_push(
            trace_admission_0(),
            trace_push_source_action(
                trace_zipper_field(), trace_zipper_offer(), trace_root_region(),
            ),
            trace_admission_1(),
        ),
{
    trace_root_field_model();
    trace_bytes_nest();
    assert(trace_leaf_seal(0));
    let state = trace_admission_0();
    let child = trace_zipper_offer();
    let field = trace_zipper_field();
    let parent_source = integrated::product_admission_source_image(trace_root_region());
    score_bridge::bridge_ezipper_frame_required_slots_generated(child);
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_child_physical_contained(trace_root_frame(), child));
    assert(admission::concrete_message_view(state).is_some());
    let view = admission::concrete_message_view(state).unwrap();
    assert(view.cursor == trace_reader(trace_root_bytes(), 0, 0).cursor);
    assert(admission::admission_indexed_source_wf(parent_source));
    assert(admission::admission_indexed_source_position(parent_source, 0)
        == Some(admission::AdmissionSourceAnchor { source: 0, offset: 0 }));
    assert(admission::admission_indexed_source_position(parent_source, 3)
        == Some(admission::AdmissionSourceAnchor { source: 0, offset: 3 }));
    let next = admission::admission_push_step(state, 15, child, 0, false, 33, 1);
    assert(next.is_some());
    assert(next.unwrap().frames =~= trace_admission_1().frames);
    assert(next == Some(trace_admission_1()));
    assert(admission::concrete_message_push_indexed(view, field, child, parent_source, 1)
        == Some(trace_admission_1()));
    assert(trace_admission_push(
        state, trace_push_source_action(field, child, trace_root_region()), trace_admission_1(),
    ));
}

pub proof fn trace_zipper_push_map()
    ensures
        trace_sealed_map_step(
            trace_live_0().source.product,
            trace_live_1().source.product,
            map::IntegratedAdmissionAction::Message {
                action: trace_push_source_action(
                    trace_zipper_field(), trace_zipper_offer(), trace_root_region(),
                ),
            },
        ),
{
    let before = trace_live_0();
    let after = trace_live_1().source.product;
    trace_initial_state_is_wf();
    trace_root_field_is_read();
    trace_zipper_push_admission();
    assert(before.source.product.bridge.admission == trace_admission_0());
    assert(after.bridge.admission == trace_admission_1());
    assert(before.source.product.indexed_regions.last() == trace_root_region());
    assert(before.source.product.map.map_frames.push(None) =~= after.map.map_frames);
    assert(before.source.product.indexed_regions.push(trace_zipper_region())
        =~= after.indexed_regions);
    assert(before.source.product.epm_frames.push(None) =~= after.epm_frames);
    assert(integrated::product_epm_receipts_carried(before.source.product, after)
        =~= after.accepted_epm_sources);
    assert forall|i: int| 0 <= i < before.source.product.map.map_frames.len() implies
        (#[trigger] before.source.product.map.map_frames[i]).is_none() by {
        assert(i == 0);
    }
    assert forall|i: int| 0 <= i < before.source.product.map.admission.frames.len() implies
        (#[trigger] before.source.product.map.admission.frames[i]).frame_kind != 2 by {
        assert(i == 0);
    }
    assert forall|i: int| 0 <= i < after.map.map_frames.len() implies
        (#[trigger] after.map.map_frames[i]).is_none() by {
        assert(i == 0 || i == 1);
    }
    assert forall|i: int| 0 <= i < after.map.admission.frames.len() implies
        (#[trigger] after.map.admission.frames[i]).frame_kind != 2 by {
        assert(i == 0 || i == 1);
    }
    trace_push_enters_map(
        before, after, trace_zipper_field(), trace_zipper_offer(), trace_zipper_region(),
        trace_root_cursor(), trace_root_read_cursor(), trace_root_read_reader(),
    );
}

pub proof fn trace_zipper_push_live()
    ensures
        trace_sealed_indexed_push(
            trace_live_0(),
            trace_live_1(),
            trace_request(
                runtime::SourceFrameRequestKind::Message,
                trace_zipper_field(),
                trace_zipper_offer(),
            ),
            trace_storage(runtime::SourceFrameRequestKind::Message, 1, 2),
        ),
{
    let before = trace_live_0();
    let after = trace_live_1();
    let request = trace_request(
        runtime::SourceFrameRequestKind::Message, trace_zipper_field(), trace_zipper_offer(),
    );
    trace_initial_state_is_wf();
    trace_root_field_is_read();
    trace_root_child_cursor();
    trace_zipper_push_admission();
    trace_zipper_push_map();
    assert(request.field == trace_zipper_field());
    assert(request.child == trace_zipper_offer());
    assert(before.source.product.bridge.admission == trace_admission_0());
    assert(after.source.product.bridge.admission == trace_admission_1());
    assert(before.source.product.indexed_regions.last() == trace_root_region());
    assert(before.cursors.last() == Some(trace_root_cursor()));
    assert(after.source.wire =~= before.source.wire.update(0, Some(trace_root_read_reader()))
        .push(Some(trace_reader(trace_zipper_bytes(), 0, 0))));
    assert(after.cursors =~= before.cursors.update(0, Some(trace_root_read_cursor()))
        .push(Some(trace_zipper_cursor())));
    trace_indexed_push_seals(
        before, after, request, 1, 2, trace_root_cursor(), trace_zipper_region(),
        trace_root_read_cursor(), trace_root_read_reader(), trace_zipper_cursor(),
        trace_reader(trace_zipper_bytes(), 0, 0),
        trace_storage(runtime::SourceFrameRequestKind::Message, 1, 2),
    );
}

pub proof fn trace_zipper_push_step()
    ensures
        trace_sealed_step(trace_live_0(), trace_live_1(), trace_action_0()),
{
    let request = trace_request(
        runtime::SourceFrameRequestKind::Message, trace_zipper_field(), trace_zipper_offer(),
    );
    trace_initial_state_is_wf();
    trace_zipper_push_live();
    trace_indexed_push_lifts(
        trace_live_0(), trace_live_1(), request, 1, 2,
        trace_storage(runtime::SourceFrameRequestKind::Message, 1, 2), trace_action_0(),
    );
}

pub proof fn trace_state_1_is_wf()
    ensures
        trace_sealed_wf(trace_live_1()),
{
    trace_initial_state_is_wf();
    trace_zipper_push_step();
    trace_step_extends(trace_live_0(), trace_live_1(), trace_action_0());
}

pub proof fn trace_zipper_field_model()
    ensures
        trace_sealed_field_step(trace_zipper_bytes(), trace_owner_field(), 12, 1),
        trace_sealed_field_at(trace_reader(trace_zipper_bytes(), 0, 0).cursor, trace_owner_field()),
{
    trace_bytes_nest();
    let bytes = trace_zipper_bytes();
    let cursor = trace_reader(bytes, 0, 0).cursor;
    assert(wire::wire_varint_read(bytes, 0, 12)
        == wire::WireVarintResult::Value { value: 10, next: 1 });
    assert(wire::wire_varint_read(bytes, 1, 12)
        == wire::WireVarintResult::Value { value: 10, next: 2 });
    assert(wire::wire_field_step(cursor, 0, false, false)
        == (wire::WireFieldResult::Field { field: trace_owner_field(), next: 12, last_field: 1 }));
    wire::wire_field_step_refines(cursor, 0, false, false);
}

pub proof fn trace_zipper_field_is_read()
    ensures
        trace_sealed_field_push(
            trace_zipper_cursor(), trace_zipper_region(), trace_owner_field(), trace_owner_region(),
            trace_zipper_read_cursor(), trace_zipper_read_reader(),
        ),
{
    trace_bytes_nest();
    trace_zipper_field_model();
    trace_field_read_seals(
        3, 12, trace_zipper_bytes(), trace_owner_field(), 12, 1, trace_owner_region(),
    );
}

pub proof fn trace_zipper_child_cursor()
    ensures
        trace_sealed_child_view(
            trace_zipper_cursor(), trace_owner_field(), trace_owner_cursor(),
            trace_reader(trace_owner_bytes(), 0, 0),
        ),
{
    trace_bytes_nest();
    trace_cursor_projects(3, 12, 0, 0, 0);
    trace_cursor_projects(5, 10, 0, 0, 0);
    assert(trace_child_view(
        trace_zipper_cursor(), trace_owner_field(), trace_owner_cursor(),
        trace_reader(trace_owner_bytes(), 0, 0),
    ));
}

pub proof fn trace_owner_push_admission()
    ensures
        trace_sealed_admission_push(
            trace_admission_1(),
            trace_push_source_action(
                trace_owner_field(), trace_owner_offer(), trace_zipper_region(),
            ),
            trace_admission_2(),
        ),
{
    trace_zipper_field_model();
    trace_bytes_nest();
    assert(trace_leaf_seal(0));
    let state = trace_admission_1();
    let child = trace_owner_offer();
    let field = trace_owner_field();
    let parent_source = integrated::product_admission_source_image(trace_zipper_region());
    score_bridge::bridge_epathmap_frame_required_slots_generated(child);
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_child_physical_contained(trace_zipper_frame(), child));
    assert(admission::concrete_message_view(state).is_some());
    let view = admission::concrete_message_view(state).unwrap();
    assert(view.cursor == trace_reader(trace_zipper_bytes(), 0, 0).cursor);
    assert(admission::admission_indexed_source_wf(parent_source));
    assert(admission::admission_indexed_source_position(parent_source, 0)
        == Some(admission::AdmissionSourceAnchor { source: 0, offset: 3 }));
    assert(admission::admission_indexed_source_position(parent_source, 2)
        == Some(admission::AdmissionSourceAnchor { source: 0, offset: 5 }));
    let next = admission::admission_push_step(state, 12, child, 0, false, 1, 1);
    assert(next.is_some());
    assert(next.unwrap().frames =~= trace_admission_2().frames);
    assert(next == Some(trace_admission_2()));
    assert(admission::concrete_message_push_indexed(view, field, child, parent_source, 1)
        == Some(trace_admission_2()));
    assert(trace_admission_push(
        state, trace_push_source_action(field, child, trace_zipper_region()), trace_admission_2(),
    ));
}

pub proof fn trace_owner_push_map()
    ensures
        trace_sealed_map_step(
            trace_live_1().source.product,
            trace_live_2().source.product,
            map::IntegratedAdmissionAction::Message {
                action: trace_push_source_action(
                    trace_owner_field(), trace_owner_offer(), trace_zipper_region(),
                ),
            },
        ),
{
    let before = trace_live_1();
    let after = trace_live_2().source.product;
    trace_state_1_is_wf();
    trace_zipper_field_is_read();
    trace_owner_push_admission();
    assert(before.source.product.bridge.admission == trace_admission_1());
    assert(after.bridge.admission == trace_admission_2());
    assert(before.source.product.indexed_regions.last() == trace_zipper_region());
    assert(before.source.product.map.map_frames.push(None) =~= after.map.map_frames);
    assert(before.source.product.indexed_regions.push(trace_owner_region())
        =~= after.indexed_regions);
    assert(before.source.product.epm_frames.push(None) =~= after.epm_frames);
    assert(integrated::product_epm_receipts_carried(before.source.product, after)
        =~= after.accepted_epm_sources);
    assert forall|i: int| 0 <= i < before.source.product.map.map_frames.len() implies
        (#[trigger] before.source.product.map.map_frames[i]).is_none() by {
        assert(i == 0 || i == 1);
    }
    assert forall|i: int| 0 <= i < before.source.product.map.admission.frames.len() implies
        (#[trigger] before.source.product.map.admission.frames[i]).frame_kind != 2 by {
        assert(i == 0 || i == 1);
    }
    assert forall|i: int| 0 <= i < after.map.map_frames.len() implies
        (#[trigger] after.map.map_frames[i]).is_none() by {
        assert(i == 0 || i == 1 || i == 2);
    }
    assert forall|i: int| 0 <= i < after.map.admission.frames.len() implies
        (#[trigger] after.map.admission.frames[i]).frame_kind != 2 by {
        assert(i == 0 || i == 1 || i == 2);
    }
    trace_push_enters_map(
        before, after, trace_owner_field(), trace_owner_offer(), trace_owner_region(),
        trace_zipper_cursor(), trace_zipper_read_cursor(), trace_zipper_read_reader(),
    );
}

pub proof fn trace_owner_push_live()
    ensures
        trace_sealed_indexed_push(
            trace_live_1(),
            trace_live_2(),
            trace_request(
                runtime::SourceFrameRequestKind::Message, trace_owner_field(), trace_owner_offer(),
            ),
            trace_storage(runtime::SourceFrameRequestKind::Message, 2, 3),
        ),
{
    let before = trace_live_1();
    let after = trace_live_2();
    let request = trace_request(
        runtime::SourceFrameRequestKind::Message, trace_owner_field(), trace_owner_offer(),
    );
    trace_state_1_is_wf();
    trace_zipper_field_is_read();
    trace_zipper_child_cursor();
    trace_owner_push_admission();
    trace_owner_push_map();
    assert(request.field == trace_owner_field());
    assert(request.child == trace_owner_offer());
    assert(before.source.product.bridge.admission == trace_admission_1());
    assert(after.source.product.bridge.admission == trace_admission_2());
    assert(before.source.product.indexed_regions.last() == trace_zipper_region());
    assert(before.cursors.last() == Some(trace_zipper_cursor()));
    assert(after.source.wire =~= before.source.wire.update(1, Some(trace_zipper_read_reader()))
        .push(Some(trace_reader(trace_owner_bytes(), 0, 0))));
    assert(after.cursors =~= before.cursors.update(1, Some(trace_zipper_read_cursor()))
        .push(Some(trace_owner_cursor())));
    trace_indexed_push_seals(
        before, after, request, 2, 3, trace_zipper_cursor(), trace_owner_region(),
        trace_zipper_read_cursor(), trace_zipper_read_reader(), trace_owner_cursor(),
        trace_reader(trace_owner_bytes(), 0, 0),
        trace_storage(runtime::SourceFrameRequestKind::Message, 2, 3),
    );
}

pub proof fn trace_owner_push_step()
    ensures
        trace_sealed_step(trace_live_1(), trace_live_2(), trace_action_1()),
{
    let request = trace_request(
        runtime::SourceFrameRequestKind::Message, trace_owner_field(), trace_owner_offer(),
    );
    trace_state_1_is_wf();
    trace_owner_push_live();
    trace_indexed_push_lifts(
        trace_live_1(), trace_live_2(), request, 2, 3,
        trace_storage(runtime::SourceFrameRequestKind::Message, 2, 3), trace_action_1(),
    );
}

pub proof fn trace_state_2_is_wf()
    ensures
        trace_sealed_wf(trace_live_2()),
{
    trace_state_1_is_wf();
    trace_owner_push_step();
    trace_step_extends(trace_live_1(), trace_live_2(), trace_action_1());
}

pub proof fn trace_owner_field_model()
    ensures
        trace_sealed_field_step(trace_owner_bytes(), trace_snapshot_field(), 10, 9),
        trace_sealed_field_at(
            trace_reader(trace_owner_bytes(), 0, 0).cursor, trace_snapshot_field(),
        ),
{
    trace_bytes_nest();
    let bytes = trace_owner_bytes();
    let cursor = trace_reader(bytes, 0, 0).cursor;
    assert(wire::wire_varint_read(bytes, 0, 10)
        == wire::WireVarintResult::Value { value: 74, next: 1 });
    assert(wire::wire_varint_read(bytes, 1, 10)
        == wire::WireVarintResult::Value { value: 8, next: 2 });
    assert(wire::wire_field_step(cursor, 0, false, false)
        == (wire::WireFieldResult::Field {
            field: trace_snapshot_field(),
            next: 10,
            last_field: 9,
        }));
    wire::wire_field_step_refines(cursor, 0, false, false);
}

pub proof fn trace_owner_field_is_read()
    ensures
        trace_sealed_field_push(
            trace_owner_cursor(), trace_owner_region(), trace_snapshot_field(),
            trace_snapshot_region(), trace_owner_read_cursor(), trace_owner_read_reader(),
        ),
{
    trace_bytes_nest();
    trace_owner_field_model();
    trace_field_read_seals(
        5, 10, trace_owner_bytes(), trace_snapshot_field(), 10, 9, trace_snapshot_region(),
    );
}

pub proof fn trace_snapshot_push_admission()
    ensures
        trace_sealed_admission_push(
            trace_admission_2(),
            trace_push_source_action(
                trace_snapshot_field(), trace_snapshot_offer(), trace_owner_region(),
            ),
            trace_admission_3(),
        ),
{
    trace_owner_field_model();
    trace_bytes_nest();
    assert(trace_leaf_seal(0));
    let state = trace_admission_2();
    let child = trace_snapshot_offer();
    let field = trace_snapshot_field();
    let parent_source = integrated::product_admission_source_image(trace_owner_region());
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_child_physical_contained(trace_owner_frame(), child));
    assert(admission::concrete_message_view(state).is_some());
    let view = admission::concrete_message_view(state).unwrap();
    assert(view.cursor == trace_reader(trace_owner_bytes(), 0, 0).cursor);
    assert(admission::admission_indexed_source_wf(parent_source));
    assert(admission::admission_indexed_source_position(parent_source, 0)
        == Some(admission::AdmissionSourceAnchor { source: 0, offset: 5 }));
    assert(admission::admission_indexed_source_position(parent_source, 2)
        == Some(admission::AdmissionSourceAnchor { source: 0, offset: 7 }));
    let next = admission::admission_push_step(state, 10, child, 3, false, 9, 1);
    assert(next.is_some());
    assert(next.unwrap().frames =~= trace_admission_3().frames);
    assert(next == Some(trace_admission_3()));
    assert(admission::concrete_message_push_indexed(view, field, child, parent_source, 1)
        == Some(trace_admission_3()));
    assert(trace_admission_push(
        state, trace_push_source_action(field, child, trace_owner_region()), trace_admission_3(),
    ));
}

pub proof fn trace_snapshot_push_map()
    ensures
        trace_sealed_map_step(
            trace_live_2().source.product,
            trace_live_3().source.product,
            map::IntegratedAdmissionAction::Message {
                action: trace_push_source_action(
                    trace_snapshot_field(), trace_snapshot_offer(), trace_owner_region(),
                ),
            },
        ),
{
    let before = trace_live_2();
    let after = trace_live_3().source.product;
    trace_state_2_is_wf();
    trace_owner_field_is_read();
    trace_snapshot_push_admission();
    assert(before.source.product.bridge.admission == trace_admission_2());
    assert(after.bridge.admission == trace_admission_3());
    assert(before.source.product.indexed_regions.last() == trace_owner_region());
    assert(before.source.product.map.map_frames.push(None) =~= after.map.map_frames);
    assert(before.source.product.indexed_regions.push(trace_snapshot_region())
        =~= after.indexed_regions);
    assert(before.source.product.epm_frames.push(None) =~= after.epm_frames);
    assert(integrated::product_epm_receipts_carried(before.source.product, after)
        =~= after.accepted_epm_sources);
    assert forall|i: int| 0 <= i < before.source.product.map.map_frames.len() implies
        (#[trigger] before.source.product.map.map_frames[i]).is_none() by {
        assert(i == 0 || i == 1 || i == 2);
    }
    assert forall|i: int| 0 <= i < before.source.product.map.admission.frames.len() implies
        (#[trigger] before.source.product.map.admission.frames[i]).frame_kind != 2 by {
        assert(i == 0 || i == 1 || i == 2);
    }
    assert forall|i: int| 0 <= i < after.map.map_frames.len() implies
        (#[trigger] after.map.map_frames[i]).is_none() by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
    assert forall|i: int| 0 <= i < after.map.admission.frames.len() implies
        (#[trigger] after.map.admission.frames[i]).frame_kind != 2 by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
    trace_push_enters_map(
        before, after, trace_snapshot_field(), trace_snapshot_offer(), trace_snapshot_region(),
        trace_owner_cursor(), trace_owner_read_cursor(), trace_owner_read_reader(),
    );
}

pub proof fn trace_snapshot_push_live()
    ensures
        trace_sealed_live_step(trace_live_2(), trace_live_3(), trace_epm_push_action()),
{
    let before = trace_live_2();
    let after = trace_live_3();
    let request = trace_request(
        runtime::SourceFrameRequestKind::EPathMap, trace_snapshot_field(), trace_snapshot_offer(),
    );
    trace_state_2_is_wf();
    trace_owner_field_is_read();
    trace_snapshot_push_admission();
    trace_snapshot_push_map();
    assert(request.field == trace_snapshot_field());
    assert(request.child == trace_snapshot_offer());
    assert(before.source.product.bridge.admission == trace_admission_2());
    assert(after.source.product.bridge.admission == trace_admission_3());
    assert(before.source.product.indexed_regions.last() == trace_owner_region());
    assert(before.cursors.last() == Some(trace_owner_cursor()));
    assert(after.source.wire =~= before.source.wire.update(2, Some(trace_owner_read_reader()))
        .push(None));
    assert(after.cursors =~= before.cursors.update(2, Some(trace_owner_read_cursor()))
        .push(None));
    trace_epm_push_seals(
        before, after, request, 3, 4, trace_owner_cursor(), trace_snapshot_region(),
        trace_owner_read_cursor(), trace_owner_read_reader(),
    );
    trace_epm_push_lifts(
        before, after, request, Some(request.request_kind), 4, 4, 1, trace_epm_push_action(),
    );
}

pub proof fn trace_snapshot_push_step()
    ensures
        trace_sealed_step(trace_live_2(), trace_live_3(), trace_action_2()),
{
    trace_snapshot_push_live();
    trace_live_step_lifts(
        trace_live_2(), trace_live_3(), trace_epm_push_action(), trace_action_2(),
    );
}

pub proof fn trace_state_3_is_wf()
    ensures
        trace_sealed_wf(trace_live_3()),
{
    trace_state_2_is_wf();
    trace_snapshot_push_step();
    trace_step_extends(trace_live_2(), trace_live_3(), trace_action_2());
}

pub proof fn trace_handoff_parts_hold()
    ensures
        trace_sealed_handoff_parts(
            trace_owner_waiting_frame(), trace_snapshot_frame(), trace_owner_region(),
            trace_snapshot_region(), trace_epm_backing(), trace_epm_frame_0(),
        ),
{
    let parent = trace_owner_waiting_frame();
    let top = trace_snapshot_frame();
    let frame = trace_epm_frame_0();
    trace_bytes_nest();
    trace_snapshot_prefix_is_empty();
    trace_epm_frame_layout(frame);
    trace_region_subview(trace_owner_bytes(), 5, 2, 10);
    assert(admission::admission_frame_wf(top));
    assert(epm::source_epm_initial_frame(top, trace_epm_backing(), None, None) == Some(frame));
    assert(trace_handoff_parts(
        parent, top, trace_owner_region(), trace_snapshot_region(), trace_epm_backing(), frame,
    ));
}

pub proof fn trace_handoff_step()
    ensures
        trace_sealed_step(trace_live_3(), trace_live_4(), trace_action_3()),
{
    let before = trace_live_3();
    let after = trace_live_4();
    trace_state_3_is_wf();
    trace_handoff_parts_hold();
    assert(before.source.product.bridge.admission == trace_admission_3());
    assert(before.source.product.bridge.admission.frames[2] == trace_owner_waiting_frame());
    assert(before.source.product.bridge.admission.frames.last() == trace_snapshot_frame());
    assert(before.source.product.indexed_regions[2] == trace_owner_region());
    assert(before.source.product.indexed_regions.last() == trace_snapshot_region());
    assert(before.source.product.epm_frames.last().is_none());
    assert(after.source.product.epm_frames
        =~= before.source.product.epm_frames.update(3, Some(trace_epm_frame_0())));
    assert(integrated::product_epm_receipts_carried(before.source.product, after.source.product)
        =~= after.source.product.accepted_epm_sources);
    trace_handoff_seals(
        before, after, trace_epm_backing(), trace_owner_waiting_frame(), trace_snapshot_frame(),
        trace_owner_region(), trace_snapshot_region(), trace_epm_frame_0(), trace_action_3(),
    );
}

pub proof fn trace_state_4_is_wf()
    ensures
        trace_sealed_wf(trace_live_4()),
{
    trace_state_3_is_wf();
    trace_handoff_step();
    trace_step_extends(trace_live_3(), trace_live_4(), trace_action_3());
}

pub proof fn trace_value_event_steps()
    ensures
        trace_sealed_event_step(trace_epm_frame_0(), trace_epm_frame_1(), trace_value_event(), 3),
{
    let before = trace_epm_frame_0();
    let after = trace_epm_frame_1();
    trace_bytes_nest();
    trace_epm_frame_layout(before);
    trace_epm_frame_layout(after);
    assert(epm::topology_position(before, 5) == epm::SourceAnchor { source: 0, offset: 12 });
    assert(epm::cursor_event_step(before, after, trace_value_event()));
    assert(epm::value_return(
        before, after, false, None, epm::topology_position(before, 5), 3,
        epm::EpmAction::Continue,
    ));
    assert(epm::event_step(before, after, trace_value_event(), 3, epm::EpmAction::Continue));
}

pub proof fn trace_value_continue_live()
    ensures
        trace_sealed_live_step(
            trace_live_4(),
            trace_live_5(),
            LiveRuntimeProductAction::EpmContinue { event: trace_value_event() },
        ),
{
    let before = trace_live_4();
    let after = trace_live_5();
    trace_state_4_is_wf();
    trace_value_event_steps();
    assert(before.source.product.bridge.admission == trace_admission_3());
    assert(after.source.product.bridge.admission == trace_admission_5());
    assert(before.source.product.bridge.admission.frames.last() == trace_snapshot_frame());
    assert(before.source.product.epm_frames.last() == Some(trace_epm_frame_0()));
    assert(trace_epm_frame_0().cursor.value_table_offset == 8);
    assert(trace_epm_frame_1().cursor.value_table_offset == 8);
    assert(trace_snapshot_read_frame()
        == epm::epm_advanced_admission_cursor(trace_snapshot_frame(), 8));
    assert(trace_admission_5().frames =~= trace_admission_3().frames.update(
        3, epm::epm_advanced_admission_cursor(trace_snapshot_frame(), 8),
    ));
    assert(trace_admission_5() == dispatcher::integrated_epm_cursor_admission(
        trace_admission_3(), 8,
    ));
    assert(after.source.product.epm_frames
        =~= before.source.product.epm_frames.update(3, Some(trace_epm_frame_1())));
    assert(integrated::product_epm_receipts_carried(before.source.product, after.source.product)
        =~= after.source.product.accepted_epm_sources);
    trace_continue_seals(
        before, after, trace_value_event(), trace_epm_frame_0(), trace_epm_frame_1(), 3,
    );
    trace_epm_continue_lifts(
        before, after, trace_value_event(),
        LiveRuntimeProductAction::EpmContinue { event: trace_value_event() },
    );
}

pub proof fn trace_value_continue_step()
    ensures
        trace_sealed_step(trace_live_4(), trace_live_5(), trace_action_4()),
{
    trace_value_continue_live();
    trace_live_step_lifts(
        trace_live_4(),
        trace_live_5(),
        LiveRuntimeProductAction::EpmContinue { event: trace_value_event() },
        trace_action_4(),
    );
}

pub proof fn trace_state_5_is_wf()
    ensures
        trace_sealed_wf(trace_live_5()),
{
    trace_state_4_is_wf();
    trace_value_continue_step();
    trace_step_extends(trace_live_4(), trace_live_5(), trace_action_4());
}

pub proof fn trace_leave_event_steps()
    ensures
        trace_sealed_event_step(trace_epm_frame_1(), trace_epm_frame_2(), trace_leave_event(), 3),
{
    let before = trace_epm_frame_1();
    let after = trace_epm_frame_2();
    trace_bytes_nest();
    trace_epm_frame_layout(before);
    trace_epm_frame_layout(after);
    assert(epm::topology_position(before, 5) == epm::SourceAnchor { source: 0, offset: 12 });
    assert(epm::parser_gate_matches(before.path.gate, before.path.parser));
    assert(epm::cursor_event_step(before, after, trace_leave_event()));
    assert(epm::leave_return(before, after, epm::topology_position(before, 5)));
    assert(epm::event_step(before, after, trace_leave_event(), 3, epm::EpmAction::Continue));
}

pub proof fn trace_leave_continue_live()
    ensures
        trace_sealed_live_step(
            trace_live_5(),
            trace_live_6(),
            LiveRuntimeProductAction::EpmContinue { event: trace_leave_event() },
        ),
{
    let before = trace_live_5();
    let after = trace_live_6();
    trace_state_5_is_wf();
    trace_leave_event_steps();
    assert(before.source.product.bridge.admission == trace_admission_5());
    assert(after.source.product.bridge.admission == trace_admission_5());
    assert(before.source.product.bridge.admission.frames.last() == trace_snapshot_read_frame());
    assert(before.source.product.epm_frames.last() == Some(trace_epm_frame_1()));
    assert(trace_epm_frame_1().cursor.value_table_offset == 8);
    assert(trace_epm_frame_2().cursor.value_table_offset == 8);
    assert(trace_snapshot_read_frame()
        == epm::epm_advanced_admission_cursor(trace_snapshot_read_frame(), 8));
    assert(trace_admission_5().frames =~= trace_admission_5().frames.update(
        3, epm::epm_advanced_admission_cursor(trace_snapshot_read_frame(), 8),
    ));
    assert(trace_admission_5() == dispatcher::integrated_epm_cursor_admission(
        trace_admission_5(), 8,
    ));
    assert(after.source.product.epm_frames
        =~= before.source.product.epm_frames.update(3, Some(trace_epm_frame_2())));
    assert(integrated::product_epm_receipts_carried(before.source.product, after.source.product)
        =~= after.source.product.accepted_epm_sources);
    trace_continue_seals(
        before, after, trace_leave_event(), trace_epm_frame_1(), trace_epm_frame_2(), 3,
    );
    trace_epm_continue_lifts(
        before, after, trace_leave_event(),
        LiveRuntimeProductAction::EpmContinue { event: trace_leave_event() },
    );
}

pub proof fn trace_leave_continue_step()
    ensures
        trace_sealed_step(trace_live_5(), trace_live_6(), trace_action_5()),
{
    trace_leave_continue_live();
    trace_live_step_lifts(
        trace_live_5(),
        trace_live_6(),
        LiveRuntimeProductAction::EpmContinue { event: trace_leave_event() },
        trace_action_5(),
    );
}

pub proof fn trace_state_6_is_wf()
    ensures
        trace_sealed_wf(trace_live_6()),
{
    trace_state_5_is_wf();
    trace_leave_continue_step();
    trace_step_extends(trace_live_5(), trace_live_6(), trace_action_5());
}

pub proof fn trace_end_event_steps()
    ensures
        trace_sealed_end_step(trace_epm_frame_2(), 3),
{
    let frame = trace_epm_frame_2();
    let finished = epm_source_accept::epm_end_frame(frame);
    trace_bytes_nest();
    trace_epm_frame_layout(frame);
    trace_epm_frame_layout(finished);
    let action = epm::EpmAction::Complete {
        mode: frame.mode,
        entries_stable: frame.entries_stable,
    };
    assert(epm::path_finish_ok(frame.path));
    assert(epm::cursor_event_step(frame, finished, epm::EpmEvent::End));
    assert(epm::end_return(frame, action));
    assert(epm::event_step(frame, finished, epm::EpmEvent::End, 3, action));
}

pub proof fn trace_snapshot_child_completes()
    ensures
        trace_sealed_completion_parts(trace_admission_5(), trace_returned_admission()),
{
    let state = trace_admission_5();
    let child = trace_snapshot_read_frame();
    let parent = trace_owner_waiting_frame();
    let summary = admission::AdmissionReturnedKind::Other;
    trace_bytes_nest();
    assert(state.frames.len() == 4);
    assert(state.frames[2] == parent);
    assert(state.frames[3] == child);
    assert(state.frames.last() == child);
    assert(admission::admission_epathmap_snapshot_descriptor());
    assert(epm_source_accept::epm_program31_parent(state));
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_family_admissible(child));
    assert(admission::admission_child_summary_agrees(child, summary, state.arena));
    assert(admission::admission_epathmap_child_edge(parent, child));
    assert(admission::admission_arm_child_summary_agrees(parent, child, summary, state.arena));
    let next = admission::admission_complete_child_step(state, 2, 3, summary);
    assert(next.is_some());
    assert(next.unwrap().frames =~= trace_returned_admission().frames);
    assert(next == Some(trace_returned_admission()));
    assert(trace_completion_parts(state, trace_returned_admission()));
}

pub proof fn trace_finish_accept_live()
    ensures
        trace_sealed_live_step(
            trace_live_6(), trace_live_7(), LiveRuntimeProductAction::EpmFinishAccept,
        ),
{
    let before = trace_live_6();
    let after = trace_live_7();
    let base = trace_returned_admission();
    trace_state_6_is_wf();
    trace_end_event_steps();
    trace_snapshot_child_completes();
    assert(before.source.product.bridge.admission == trace_admission_5());
    assert(after.source.product.bridge.admission == trace_admission_7());
    assert(before.source.product.epm_frames.last() == Some(trace_epm_frame_2()));
    assert(before.cursors.last().is_none());
    assert(epm_source_accept::epm_mode_code(trace_epm_frame_2().mode) == 0);
    assert(trace_epm_frame_2().entries_stable);
    assert(base.frames.last() == trace_owner_returned_frame());
    assert(trace_accepted_parent(trace_owner_returned_frame(), 0, true).returns
        =~= trace_owner_accepted_frame().returns);
    assert(trace_accepted_parent(trace_owner_returned_frame(), 0, true)
        == trace_owner_accepted_frame());
    assert(trace_accepted_admission(base, 0, true).frames =~= trace_admission_7().frames);
    assert(trace_admission_7() == trace_accepted_admission(base, 0, true));
    assert(after.source.product.map.map_frames
        =~= before.source.product.map.map_frames.drop_last());
    assert(after.source.product.epm_frames =~= before.source.product.epm_frames.drop_last());
    assert(after.source.product.indexed_regions
        =~= before.source.product.indexed_regions.drop_last());
    assert(after.source.product.accepted_epm_sources
        =~= integrated::product_epm_receipts_carried(before.source.product, after.source.product)
            .update(2, Some(trace_accepted_source())));
    assert(after.source.wire =~= before.source.wire.drop_last());
    assert(after.cursors =~= before.cursors.drop_last());
    trace_finish_accept_seals(before, after, trace_epm_frame_2(), 3, base);
    trace_finish_accept_lifts(before, after, LiveRuntimeProductAction::EpmFinishAccept);
}

pub proof fn trace_finish_accept_step()
    ensures
        trace_sealed_step(trace_live_6(), trace_live_7(), trace_action_6()),
{
    trace_finish_accept_live();
    trace_live_step_lifts(
        trace_live_6(), trace_live_7(), LiveRuntimeProductAction::EpmFinishAccept,
        trace_action_6(),
    );
}

pub proof fn trace_state_7_is_wf()
    ensures
        trace_sealed_wf(trace_live_7()),
{
    trace_state_6_is_wf();
    trace_finish_accept_step();
    trace_step_extends(trace_live_6(), trace_live_7(), trace_action_6());
}

pub proof fn trace_owner_flag_is_unset()
    ensures
        trace_sealed_owner_parts(trace_owner_accepted_frame()),
{
    let top = trace_owner_accepted_frame();
    trace_snapshot_is_modeled_empty();
    score_bridge::bridge_epathmap_witness_snapshot_score_length();
    assert(top.returns.last() == trace_owner_snapshot_return());
    assert(top.scalar_occurrences.len() == 0);
    assert(score_bridge::bridge_scalar_slot(top.scalar_occurrences, 1, 4, 1u8)
        == Some(None::<u64>));
    assert(!top.seen.contains(1));
    assert(!top.scalar_slots.contains(1));
    assert(score_bridge::bridge_bool_slot(top, 1, 4) == Some(false));
    assert(admission::admission_slots_complete(top));
    assert(trace_owner_parts(top));
}

pub proof fn trace_owner_end_is_read()
    ensures
        trace_sealed_end_read(trace_owner_read_cursor()),
{
    trace_end_read_seals(5, 10, 9, 1);
}

pub proof fn trace_scored_owner_shape()
    ensures
        score_bridge::bridge_score_finish_selected(trace_bridge_7()) == Some(trace_bridge_8()),
        trace_bridge_8().admission.frames == seq![
            trace_root_waiting_frame(), trace_zipper_waiting_frame(), trace_owner_scored_frame(),
        ],
        trace_bridge_8().admission.root_end == 15,
        trace_bridge_8().admission.pushed == 4,
        trace_bridge_8().admission.completed == 1,
        trace_bridge_8().admission.next_obligation_id == 3,
        trace_bridge_8().transcript.entries.len() == 4,
        trace_bridge_8().admission.arena.len() == 4,
        trace_bridge_8().admission.arena[3].body_id == 2,
        trace_bridge_8().admission.arena[3].semantic,
        trace_bridge_8().admission.arena[3].encoded_len > 0,
        trace_bridge_8().admission.arena[3].transcript_bytes.len()
            == trace_bridge_8().admission.arena[3].encoded_len,
{
    let before = trace_live_7();
    let top = trace_owner_accepted_frame();
    trace_state_7_is_wf();
    trace_owner_flag_is_unset();
    assert(before.source.product.bridge == trace_bridge_7());
    assert(before.source.product.bridge.admission.frames.len() == 3);
    assert(before.source.product.bridge.admission.frames.last() == top);
    assert(before.source.product.bridge.transcript.entries.len() == 0);
    trace_owner_score_shapes(before, top);
    let next = trace_bridge_8();
    assert(trace_owner_scored_frame() == admission::AdmissionFrameState {
        score_handle: Some(3usize),
        ..top
    });
    assert(next.admission.frames =~= seq![
        trace_root_waiting_frame(), trace_zipper_waiting_frame(), trace_owner_scored_frame(),
    ]);
}

pub proof fn trace_owner_score_live()
    ensures
        trace_sealed_live_step(
            trace_live_7(), trace_live_8(), LiveRuntimeProductAction::MessageChildScore,
        ),
{
    let before = trace_live_7();
    let after = trace_live_8();
    let top = trace_owner_accepted_frame();
    trace_state_7_is_wf();
    trace_owner_flag_is_unset();
    trace_owner_end_is_read();
    trace_scored_owner_shape();
    assert(before.source.product.bridge == trace_bridge_7());
    assert(after.source.product.bridge == trace_bridge_8());
    assert(before.source.product.bridge.admission.frames.len() == 3);
    assert(before.source.product.bridge.admission.frames.last() == top);
    assert(before.cursors.last() == Some(trace_owner_read_cursor()));
    assert(before.source.product.map.map_frames.last().is_none());
    assert(after.source.product.accepted_epm_sources
        =~= integrated::product_epm_receipts_carried(before.source.product, after.source.product));
    assert(after.source.runtime == (runtime::RuntimeFrameImage {
        score: after.source.product.bridge,
        ..before.source.runtime
    }));
    trace_child_score_seals(before, after, top, trace_owner_read_cursor());
    trace_child_score_lifts(before, after, LiveRuntimeProductAction::MessageChildScore);
}

pub proof fn trace_owner_score_step()
    ensures
        trace_sealed_step(trace_live_7(), trace_live_8(), trace_action_7()),
{
    trace_owner_score_live();
    trace_live_step_lifts(
        trace_live_7(), trace_live_8(), LiveRuntimeProductAction::MessageChildScore,
        trace_action_7(),
    );
}

pub proof fn trace_state_8_is_wf()
    ensures
        trace_sealed_wf(trace_live_8()),
{
    trace_state_7_is_wf();
    trace_owner_score_step();
    trace_step_extends(trace_live_7(), trace_live_8(), trace_action_7());
}

pub proof fn trace_owner_child_completes()
    ensures
        trace_sealed_child_completion(trace_bridge_8().admission, trace_admission_9()),
{
    let state = trace_bridge_8().admission;
    let parent = trace_zipper_waiting_frame();
    let child = trace_owner_scored_frame();
    let summary = trace_zipper_payload();
    trace_scored_owner_shape();
    trace_bytes_nest();
    assert(state.frames.len() == 3);
    assert(state.frames[1] == parent);
    assert(state.frames[2] == child);
    score_bridge::bridge_epathmap_frame_required_slots_generated(child);
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_family_admissible(child));
    assert(admission::admission_epathmap_snapshot_descriptor());
    assert(child.returns.last() == trace_owner_snapshot_return());
    assert(admission::admission_epathmap_path_stability(child) == Some(true));
    assert(admission::admission_ezipper_pathmap_payload(parent, child) == Some(summary));
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(summary));
    assert(admission::admission_score_body_agrees(child, state.arena));
    assert(admission::admission_child_summary_agrees(child, summary, state.arena));
    assert(admission::admission_arm_child_summary_agrees(parent, child, summary, state.arena));
    let next = admission::admission_complete_child_step(state, 1, 1, summary);
    assert(next.is_some());
    assert(next.unwrap().frames[1].returns =~= trace_zipper_complete_frame().returns);
    assert(next.unwrap().frames =~= trace_admission_9().frames);
    assert(next == Some(trace_admission_9()));
    assert(admission::concrete_message_view(trace_admission_9()).is_some());
    assert(admission::concrete_message_driver_step(
        state, admission::ConcreteFrameActionModel::Complete,
    ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted {
        state: trace_admission_9(),
    }));
}

pub proof fn trace_owner_complete_map()
    ensures
        trace_sealed_map_step(
            trace_live_8().source.product,
            trace_live_9().source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ),
{
    let before = trace_live_8();
    let after = trace_live_9().source.product;
    trace_state_8_is_wf();
    trace_owner_child_completes();
    trace_scored_owner_shape();
    assert(before.source.product.bridge.admission == trace_bridge_8().admission);
    assert(after.bridge.admission == trace_admission_9());
    assert(before.source.product.bridge.admission.frames.len() == 3);
    assert(before.source.product.bridge.admission.frames.last() == trace_owner_scored_frame());
    assert(before.source.product.epm_frames.len() == 3);
    assert(after.map.map_frames =~= before.source.product.map.map_frames.subrange(
        0, before.source.product.map.map_frames.len() - 1,
    ));
    assert(after.indexed_regions =~= before.source.product.indexed_regions.drop_last());
    assert(after.epm_frames =~= before.source.product.epm_frames.drop_last());
    assert(integrated::product_epm_receipts_carried(before.source.product, after)
        =~= after.accepted_epm_sources);
    assert forall|i: int| 0 <= i < after.map.map_frames.len() implies
        (#[trigger] after.map.map_frames[i]).is_none() by {
        assert(i == 0 || i == 1);
    }
    assert forall|i: int| 0 <= i < after.map.admission.frames.len() implies
        (#[trigger] after.map.admission.frames[i]).frame_kind != 2 by {
        assert(i == 0 || i == 1);
    }
    trace_complete_enters_map(before, after);
}

pub proof fn trace_owner_complete_live()
    ensures
        trace_sealed_live_step(
            trace_live_8(), trace_live_9(), LiveRuntimeProductAction::MessageChildComplete,
        ),
{
    let before = trace_live_8();
    let after = trace_live_9();
    trace_state_8_is_wf();
    trace_owner_end_is_read();
    trace_owner_complete_map();
    trace_scored_owner_shape();
    assert(before.source.product.bridge.admission == trace_bridge_8().admission);
    assert(before.source.product.bridge.admission.frames.len() == 3);
    assert(before.source.product.bridge.admission.frames.last() == trace_owner_scored_frame());
    assert(before.source.product.bridge.admission.frames[1] == trace_zipper_waiting_frame());
    assert(before.cursors.last() == Some(trace_owner_read_cursor()));
    assert(after.source.runtime == runtime::runtime_after_admission_step(
        before.source.runtime, after.source.product.bridge.admission,
    ));
    assert(after.source.wire =~= before.source.wire.drop_last());
    assert(after.cursors =~= before.cursors.drop_last());
    trace_child_complete_seals(before, after, trace_owner_read_cursor());
    trace_child_complete_lifts(before, after, LiveRuntimeProductAction::MessageChildComplete);
}

pub proof fn trace_owner_complete_step()
    ensures
        trace_sealed_step(trace_live_8(), trace_live_9(), trace_action_8()),
{
    trace_owner_complete_live();
    trace_live_step_lifts(
        trace_live_8(), trace_live_9(), LiveRuntimeProductAction::MessageChildComplete,
        trace_action_8(),
    );
}

pub proof fn trace_state_9_is_wf()
    ensures
        trace_sealed_wf(trace_live_9()),
{
    trace_state_8_is_wf();
    trace_owner_complete_step();
    trace_step_extends(trace_live_8(), trace_live_9(), trace_action_8());
}

pub proof fn trace_prefix_starts(
    states: Seq<dispatcher::IntegratedState>,
    actions: Seq<dispatcher::IntegratedAction>,
    first: LiveRuntimeProductState,
)
    requires
        states.len() > 0,
        states[0] == trace_active(first),
        trace_sealed_wf(first),
    ensures
        trace_sealed_trace(states.subrange(0, 1), actions.subrange(0, 0)),
{
    assert(trace_seal(0));
    let prefix = states.subrange(0, 1);
    let prefix_actions = actions.subrange(0, 0);
    assert(prefix.len() == 1);
    assert(prefix_actions.len() == 0);
    assert(prefix[0] == states[0]);
    assert(dispatcher::integrated_wf(prefix[0]));
    assert(dispatcher::integrated_trace(prefix, prefix_actions));
}

pub proof fn trace_prefix_extends(
    states: Seq<dispatcher::IntegratedState>,
    actions: Seq<dispatcher::IntegratedAction>,
    count: int,
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires
        0 <= count < actions.len(),
        states.len() == actions.len() + 1,
        trace_sealed_trace(states.subrange(0, count + 1), actions.subrange(0, count)),
        states[count] == trace_active(before),
        states[count + 1] == trace_active(after),
        trace_sealed_step(before, after, actions[count]),
    ensures
        trace_sealed_trace(states.subrange(0, count + 2), actions.subrange(0, count + 1)),
{
    assert(trace_seal(0));
    let prefix = states.subrange(0, count + 1);
    let prefix_actions = actions.subrange(0, count);
    assert(prefix.last() == states[count]);
    dispatcher::integrated_trace_wf_append(
        prefix, prefix_actions, states[count + 1], actions[count],
    );
    assert(prefix.push(states[count + 1]) =~= states.subrange(0, count + 2));
    assert(prefix_actions.push(actions[count]) =~= actions.subrange(0, count + 1));
}

pub proof fn trace_opens(
    states: Seq<dispatcher::IntegratedState>, actions: Seq<dispatcher::IntegratedAction>,
)
    requires
        trace_sealed_trace(states, actions),
    ensures
        dispatcher::integrated_trace(states, actions),
{
    assert(trace_seal(0));
}

pub proof fn trace_zipper_score_fires(
    states: Seq<dispatcher::IntegratedState>,
    actions: Seq<dispatcher::IntegratedAction>,
    last: LiveRuntimeProductState,
)
    requires
        trace_sealed_trace(states, actions),
        provenance::integrated_epathmap_score_nodes_shaped(states[0]),
        ezipper_flags::integrated_ezipper_flags_readable(states[0]),
        states.len() == 10,
        states[9] == trace_active(last),
        last.source.product.bridge.admission.frames.len() > 0,
        provenance::epathmap_ezipper_eof_present(
            last.source.product.bridge.admission.frames.last(),
        ),
        provenance::epathmap_score_append_room(last.source.product.bridge.transcript),
    ensures
        dispatcher::integrated_trace(states, actions),
        score_bridge::bridge_ezipper_score_step(last.source.product.bridge).is_some(),
        score_bridge::bridge_score_finish_selected(last.source.product.bridge)
            == score_bridge::bridge_ezipper_score_step(last.source.product.bridge),
{
    assert(trace_seal(0));
    provenance::reachable_ezipper_eof_has_score_step(states, actions, 9);
    assert(provenance::epathmap_ezipper_score_dichotomy(last.source.product.bridge));
}

pub proof fn trace_is_sealed()
    ensures
        trace_sealed_trace(trace_states(), trace_actions()),
{
    let states = trace_states();
    let actions = trace_actions();
    trace_initial_state_is_wf();
    trace_zipper_push_step();
    trace_owner_push_step();
    trace_snapshot_push_step();
    trace_handoff_step();
    trace_value_continue_step();
    trace_leave_continue_step();
    trace_finish_accept_step();
    trace_owner_score_step();
    trace_owner_complete_step();
    assert(states.len() == 10);
    assert(actions.len() == 9);
    trace_prefix_starts(states, actions, trace_live_0());
    trace_prefix_extends(states, actions, 0, trace_live_0(), trace_live_1());
    trace_prefix_extends(states, actions, 1, trace_live_1(), trace_live_2());
    trace_prefix_extends(states, actions, 2, trace_live_2(), trace_live_3());
    trace_prefix_extends(states, actions, 3, trace_live_3(), trace_live_4());
    trace_prefix_extends(states, actions, 4, trace_live_4(), trace_live_5());
    trace_prefix_extends(states, actions, 5, trace_live_5(), trace_live_6());
    trace_prefix_extends(states, actions, 6, trace_live_6(), trace_live_7());
    trace_prefix_extends(states, actions, 7, trace_live_7(), trace_live_8());
    trace_prefix_extends(states, actions, 8, trace_live_8(), trace_live_9());
    assert(states.subrange(0, 10) =~= states);
    assert(actions.subrange(0, 9) =~= actions);
}

pub proof fn trace_action_kinds()
    ensures
        trace_actions().len() == 9,
        trace_actions()[0] is Live,
        trace_actions()[1] is Live,
        trace_actions()[2] is Live,
        trace_actions()[3] is EpmHandoff,
        trace_actions()[4] is Live,
        trace_actions()[5] is Live,
        trace_actions()[6] is Live,
        trace_actions()[7] == trace_plain(LiveRuntimeProductAction::MessageChildScore),
        trace_actions()[8] == trace_plain(LiveRuntimeProductAction::MessageChildComplete),
{
}

pub proof fn trace_owner_finish_is_owner_score()
    ensures
        score_bridge::bridge_score_finish_selected(trace_bridge_7())
            == score_bridge::bridge_epathmap_owner_score_step(trace_bridge_7()),
        score_bridge::bridge_epathmap_owner_score_step(trace_bridge_7()) == Some(trace_bridge_8()),
{
    let top = trace_owner_accepted_frame();
    trace_scored_owner_shape();
    assert(trace_bridge_7().admission.frames.len() == 3);
    assert(trace_bridge_7().admission.frames.last() == top);
    assert(provenance::epathmap_owner_frame(top));
    provenance::owner_finish_selects_owner_step(trace_bridge_7());
}

pub proof fn trace_state_is_wf(index: int)
    requires
        0 <= index < 10,
    ensures
        dispatcher::integrated_wf(trace_states()[index]),
{
    trace_is_sealed();
    trace_opens(trace_states(), trace_actions());
    assert(trace_states().len() == 10);
    dispatcher::integrated_trace_wf(trace_states(), trace_actions(), index);
}

pub proof fn trace_reaches_zipper_score()
    ensures
        dispatcher::integrated_trace(trace_states(), trace_actions()),
        trace_states().len() == 10,
        trace_states()[0] == trace_active(trace_live_0()),
        trace_states()[8] == trace_active(trace_live_8()),
        trace_states()[9] == trace_active(trace_live_9()),
        admission::admission_root_initial(trace_root_frame()),
        trace_live_0().source.product.bridge.admission
            == admission::admission_initial_state(trace_root_frame()),
        ({
            let owner = trace_live_8().source.product.bridge.admission.frames.last();
            &&& provenance::epathmap_owner_frame(owner)
            &&& owner.score_handle.is_some()
        }),
        ({
            let bridge = trace_live_9().source.product.bridge;
            let top = bridge.admission.frames.last();
            &&& bridge.admission.frames.len() == 2
            &&& provenance::epathmap_ezipper_eof_present(top)
            &&& top.returns[0].score_handle.is_some()
            &&& provenance::epathmap_score_append_room(bridge.transcript)
            &&& score_bridge::bridge_ezipper_score_step(bridge).is_some()
            &&& score_bridge::bridge_score_finish_selected(bridge)
                == score_bridge::bridge_ezipper_score_step(bridge)
        }),
{
    let last = trace_live_9();
    let top = last.source.product.bridge.admission.frames.last();
    assert(trace_states().len() == 10);
    assert(trace_states()[0] == trace_active(trace_live_0()));
    assert(trace_states()[8] == trace_active(trace_live_8()));
    assert(trace_states()[9] == trace_active(last));
    assert(last.source.product.bridge.admission.frames.len() == 2);
    assert(top == trace_zipper_complete_frame());
    assert(top.returns[0] == trace_zipper_owner_return());
    assert(top.returns[0].score_handle.is_some());
    assert(provenance::epathmap_ezipper_eof_present(top));
    trace_is_sealed();
    trace_start_invariants();
    trace_root_frame_is_initial();
    trace_scored_owner_shape();
    assert(trace_live_8().source.product.bridge.admission.frames.last()
        == trace_owner_scored_frame());
    assert(last.source.product.bridge.transcript == trace_bridge_8().transcript);
    assert(provenance::epathmap_score_append_room(last.source.product.bridge.transcript));
    trace_zipper_score_fires(trace_states(), trace_actions(), last);
}

}
