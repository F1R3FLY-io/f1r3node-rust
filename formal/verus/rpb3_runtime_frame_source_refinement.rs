use vstd::prelude::*;

#[cfg(rpb3_runtime_standalone)]
#[path = "rpb3_admission_cursor.rs"]
mod rpb3_admission_cursor;
#[cfg(rpb3_runtime_standalone)]
#[path = "rpb3_score_transcript.rs"]
mod rpb3_score_transcript;
#[cfg(rpb3_runtime_standalone)]
#[path = "rpb3_score_admission_bridge.rs"]
mod rpb3_score_admission_bridge;
#[cfg(rpb3_runtime_standalone)]
#[path = "rpb3_finish_macro_refinement.rs"]
mod rpb3_finish_macro_refinement;

use {
    rpb3_admission_cursor as admission, rpb3_finish_macro_refinement as finish,
    rpb3_score_admission_bridge as bridge, rpb3_score_transcript as score,
};

#[cfg(not(rpb3_runtime_standalone))]
use crate::rpb3_admission_cursor;
#[cfg(not(rpb3_runtime_standalone))]
use crate::rpb3_finish_macro_refinement;
#[cfg(not(rpb3_runtime_standalone))]
use crate::rpb3_score_admission_bridge;
#[cfg(not(rpb3_runtime_standalone))]
use crate::rpb3_score_transcript;

verus! {

pub struct RuntimeFrameImage {
    pub ghost score: bridge::ScoreAdmissionBridge,
    pub capacity: usize,
    pub frame_bytes: usize,
    pub max_frames: usize,
    pub max_storage_bytes: usize,
    pub max_capacity_bytes: usize,
}

pub enum SourceFrameRequestKind {
    Message,
    Map,
    EPathMap,
    CanonicalPath,
}

pub open spec fn source_frame_request_kind_code(kind: SourceFrameRequestKind) -> u8 {
    match kind {
        SourceFrameRequestKind::Message => 1,
        SourceFrameRequestKind::Map => 2,
        SourceFrameRequestKind::EPathMap => 3,
        SourceFrameRequestKind::CanonicalPath => 4,
    }
}

pub struct SourceFrameRequestImage {
    pub request_kind: SourceFrameRequestKind,
    pub ghost field: admission::IndexedCanonicalFieldModel,
    pub ghost child: admission::AdmissionFrameState,
}

/// `same_view` in the Rust admission layer compares the selected region with
/// the region retained by the pushed frame. The identity is an observed view
/// identity; the indexed image also retains every byte anchor and the end.
pub struct SourceSelectedFrameView {
    pub identity: usize,
    pub ghost indexed: admission::AdmissionIndexedSourceImage,
}

pub open spec fn source_selected_frame_view_matches(
    selected: SourceSelectedFrameView, retained: SourceSelectedFrameView,
) -> bool {
    selected.identity == retained.identity
        && admission::admission_indexed_source_wf(selected.indexed)
        && selected.indexed.bytes == retained.indexed.bytes
        && selected.indexed.positions == retained.indexed.positions
        && selected.indexed.end == retained.indexed.end
}

/// The scalar state tested by `IndexedCanonicalFieldCursor::matches_initial_view`.
/// Its two retained views are deliberately separate: a fresh offset alone does
/// not prove that either the cursor message or its reader selects this region.
pub struct SourceInitialIndexedCursor {
    pub ghost message_view: SourceSelectedFrameView,
    pub ghost reader_view: SourceSelectedFrameView,
    pub enforce_numeric_order: bool,
    pub last_field: u32,
    pub fields_read: usize,
    pub terminal_error_present: bool,
    pub reader_offset: usize,
    pub reader_bytes_read: usize,
    pub reader_bytes_traversed: usize,
    pub reader_empty: bool,
}

pub open spec fn source_initial_indexed_cursor_matches(
    cursor: SourceInitialIndexedCursor,
    selected: SourceSelectedFrameView,
    enforce_numeric_order: bool,
) -> bool {
    source_selected_frame_view_matches(selected, cursor.message_view)
        && source_selected_frame_view_matches(selected, cursor.reader_view)
        && cursor.enforce_numeric_order == enforce_numeric_order
        && cursor.last_field == 0
        && cursor.fields_read == 0
        && !cursor.terminal_error_present
        && cursor.reader_offset == 0
        && cursor.reader_bytes_read == 0
        && cursor.reader_bytes_traversed == 0
        && cursor.reader_empty == (selected.indexed.bytes.len() == 0)
}

/// Fields absent from `AdmissionFrameState` remain explicit source observations.
/// They must be extracted from the actual pushed Rust frame before this model
/// can be used as a production simulation theorem.
pub enum SourceFreshFrameDetail {
    Message {
        request_program_identity: usize,
        pushed_program_identity: usize,
        program_type_id: u16,
        field_count: usize,
        ghost cursor: SourceInitialIndexedCursor,
        seen_len: usize,
        seen_nonempty: bool,
        semantic_slots_len: usize,
        semantic_slots_nonempty: bool,
        waiting_present: bool,
        par_components: usize,
        par_locally_free_range_present: bool,
    },
    Map {
        request_owner_identity: usize,
        pushed_owner_identity: usize,
        request_field: u32,
        pushed_field: u32,
        request_value_type_id: u16,
        pushed_value_type_id: u16,
        ghost cursor: SourceInitialIndexedCursor,
        key_present: bool,
        key_position_present: bool,
        saw_value: bool,
        value_position_present: bool,
        value_present: bool,
        waiting_value: bool,
    },
    EPathMap {
        request_owner_identity: usize,
        pushed_owner_identity: usize,
        request_field: u32,
        pushed_field: u32,
        cursor_mode: u8,
        frame_mode: u8,
        value_table_bytes_read: usize,
        value_table_bytes_traversed: usize,
        topology_events: usize,
        value_records: usize,
        terminal_checks: usize,
        cursor_terminal_source_present: bool,
        waiting_present: bool,
        waiting_child_present: bool,
        path_obligation_present: bool,
        entries_canonical_path_stable: bool,
        last_source: admission::AdmissionSourceAnchor,
        terminal_source_present: bool,
    },
    CanonicalPath {
        bytes_yielded: usize,
        bytes_traversed: usize,
        reducer_obligation_present: bool,
        waiting_present: bool,
        waiting_child_present: bool,
        terminal_source_present: bool,
    },
}

pub struct SourceFreshFrameObservation {
    pub ghost selected: SourceSelectedFrameView,
    pub ghost pushed: SourceSelectedFrameView,
    pub ghost observed_frame: admission::AdmissionFrameState,
    pub ghost detail: SourceFreshFrameDetail,
}

/// Source-shaped post-push check. `observed_frame` is the frame actually at
/// the worklist top, whereas `request.child` is the pre-push model argument;
/// the driver may add a return obligation between those two observations.
pub open spec fn source_fresh_frame_matches(
    request: SourceFrameRequestImage,
    pushed: admission::AdmissionFrameState,
    observed: SourceFreshFrameObservation,
) -> bool {
    &&& observed.observed_frame == pushed
    &&& source_frame_request_kind_code(request.request_kind) == pushed.frame_kind
    &&& source_selected_frame_view_matches(observed.selected, observed.pushed)
    &&& pushed.source_bytes == observed.pushed.indexed.bytes
    &&& pushed.region_start == 0
    &&& pushed.region_end as int == observed.pushed.indexed.bytes.len()
    &&& pushed.cursor == 0
    &&& admission::admission_indexed_source_position(observed.pushed.indexed, 0)
        == Some(admission::AdmissionSourceAnchor {
            source: pushed.physical_source,
            offset: pushed.physical_start,
        })
    &&& match (request.request_kind, observed.detail) {
        (SourceFrameRequestKind::Message, SourceFreshFrameDetail::Message {
            request_program_identity, pushed_program_identity, program_type_id,
            field_count, cursor, seen_len, seen_nonempty, semantic_slots_len,
            semantic_slots_nonempty, waiting_present, par_components,
            par_locally_free_range_present,
        }) => {
            &&& request_program_identity == pushed_program_identity
            &&& pushed.program_type_id == program_type_id
            &&& pushed.slot_count == field_count
            &&& source_initial_indexed_cursor_matches(cursor, observed.pushed, false)
            &&& pushed.last_slot.is_none()
            &&& pushed.seen == Set::empty()
            &&& pushed.scalar_slots == Set::empty()
            &&& pushed.scalar_occurrences.len() == 0
            &&& pushed.returns.len() == 0
            &&& seen_len == field_count && !seen_nonempty
            &&& semantic_slots_len == field_count && !semantic_slots_nonempty
            &&& pushed.waiting_id.is_none() && !waiting_present
            &&& par_components == 0 && !par_locally_free_range_present
        },
        (SourceFrameRequestKind::Map, SourceFreshFrameDetail::Map {
            request_owner_identity, pushed_owner_identity, request_field,
            pushed_field, request_value_type_id, pushed_value_type_id,
            cursor, key_present, key_position_present, saw_value,
            value_position_present, value_present, waiting_value,
        }) => {
            &&& request_owner_identity == pushed_owner_identity
            &&& request_field == pushed_field
            &&& request_value_type_id == pushed_value_type_id
            &&& source_initial_indexed_cursor_matches(cursor, observed.pushed, true)
            &&& !key_present && !key_position_present && !saw_value
            &&& !value_position_present && !value_present && !waiting_value
        },
        (SourceFrameRequestKind::EPathMap, SourceFreshFrameDetail::EPathMap {
            request_owner_identity, pushed_owner_identity, request_field,
            pushed_field, cursor_mode, frame_mode, value_table_bytes_read,
            value_table_bytes_traversed, topology_events, value_records,
            terminal_checks, cursor_terminal_source_present, waiting_present,
            waiting_child_present, path_obligation_present,
            entries_canonical_path_stable, last_source, terminal_source_present,
        }) => {
            &&& request_owner_identity == pushed_owner_identity
            &&& request_field == pushed_field && cursor_mode == frame_mode
            &&& value_table_bytes_read == 0 && value_table_bytes_traversed == 0
            &&& topology_events == 0 && value_records == 0
            &&& terminal_checks == 0 && !cursor_terminal_source_present
            &&& pushed.waiting_id.is_none() && !waiting_present
            &&& !waiting_child_present && !path_obligation_present
            &&& entries_canonical_path_stable
            &&& last_source == admission::AdmissionSourceAnchor {
                source: pushed.physical_source,
                offset: pushed.physical_start,
            }
            &&& !terminal_source_present
        },
        (SourceFrameRequestKind::CanonicalPath, SourceFreshFrameDetail::CanonicalPath {
            bytes_yielded, bytes_traversed, reducer_obligation_present,
            waiting_present, waiting_child_present, terminal_source_present,
        }) => {
            &&& bytes_yielded == 0 && bytes_traversed == 0
            &&& !reducer_obligation_present && pushed.waiting_id.is_none()
            &&& !waiting_present && !waiting_child_present
            &&& !terminal_source_present
        },
        _ => false,
    }
}

pub struct SourceFramePushStorageImage {
    pub request_kind: SourceFrameRequestKind,
    pub before_len: usize,
    pub after_len: usize,
    pub before_capacity: usize,
    pub after_capacity: usize,
    pub frame_variant_bytes: usize,
    pub stats_frame_variant_bytes: usize,
    pub used_bytes: usize,
    pub reserved_bytes: usize,
}

pub enum SourceFramePushObservationResult {
    UsedOverflow,
    ReservedOverflow,
    InvariantFailure,
    Success { storage: SourceFramePushStorageImage },
}

pub open spec fn source_frame_push_observe_after(
    request_kind: SourceFrameRequestKind,
    actual_top_kind: Option<SourceFrameRequestKind>,
    before_len: usize,
    before_capacity: usize,
    after_len: usize,
    after_capacity: usize,
    frame_variant_bytes: usize,
    stats_frame_variant_bytes: usize,
) -> SourceFramePushObservationResult {
    let used = after_len as int * frame_variant_bytes as int;
    let reserved = after_capacity as int * frame_variant_bytes as int;
    if used > usize::MAX as int {
        SourceFramePushObservationResult::UsedOverflow
    } else if reserved > usize::MAX as int {
        SourceFramePushObservationResult::ReservedOverflow
    } else if before_len as int + 1 > usize::MAX as int
        || before_len as int + 1 != after_len as int
        || after_capacity < after_len
        || after_capacity < before_capacity
        || match actual_top_kind {
            Some(kind) => source_frame_request_kind_code(kind)
                != source_frame_request_kind_code(request_kind),
            None => true,
        }
    {
        SourceFramePushObservationResult::InvariantFailure
    } else {
        SourceFramePushObservationResult::Success {
            storage: SourceFramePushStorageImage {
                request_kind,
                before_len,
                before_capacity,
                after_len,
                after_capacity,
                frame_variant_bytes,
                stats_frame_variant_bytes,
                used_bytes: used as usize,
                reserved_bytes: reserved as usize,
            },
        }
    }
}

pub proof fn source_frame_push_observe_after_exact(
    request_kind: SourceFrameRequestKind,
    actual_top_kind: Option<SourceFrameRequestKind>,
    before_len: usize,
    before_capacity: usize,
    after_len: usize,
    after_capacity: usize,
    frame_variant_bytes: usize,
    stats_frame_variant_bytes: usize,
)
    ensures match source_frame_push_observe_after(
        request_kind, actual_top_kind, before_len, before_capacity,
        after_len, after_capacity, frame_variant_bytes,
        stats_frame_variant_bytes,
    ) {
        SourceFramePushObservationResult::Success { storage } => {
            &&& storage.before_len == before_len
            &&& storage.before_capacity == before_capacity
            &&& storage.after_len == after_len
            &&& storage.after_capacity == after_capacity
            &&& storage.frame_variant_bytes == frame_variant_bytes
            &&& storage.stats_frame_variant_bytes == stats_frame_variant_bytes
            &&& storage.used_bytes as int
                == after_len as int * frame_variant_bytes as int
            &&& storage.reserved_bytes as int
                == after_capacity as int * frame_variant_bytes as int
            &&& before_len as int + 1 == after_len as int
            &&& after_len <= after_capacity
            &&& before_capacity <= after_capacity
            &&& actual_top_kind.is_some()
            &&& source_frame_request_kind_code(actual_top_kind.unwrap())
                == source_frame_request_kind_code(request_kind)
        },
        SourceFramePushObservationResult::UsedOverflow =>
            after_len as int * frame_variant_bytes as int > usize::MAX as int,
        SourceFramePushObservationResult::ReservedOverflow =>
            after_len as int * frame_variant_bytes as int <= usize::MAX as int
                && after_capacity as int * frame_variant_bytes as int
                    > usize::MAX as int,
        SourceFramePushObservationResult::InvariantFailure =>
            after_len as int * frame_variant_bytes as int <= usize::MAX as int
                && after_capacity as int * frame_variant_bytes as int
                    <= usize::MAX as int,
    },
{}

pub proof fn source_frame_push_observed_storage_is_checked(
    before: RuntimeFrameImage,
    request_kind: SourceFrameRequestKind,
    actual_top_kind: Option<SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
)
    requires
        runtime_frame_image_wf(before),
        before.score.admission.frames.len() <= usize::MAX as int,
        match source_frame_push_observe_after(
            request_kind, actual_top_kind,
            before.score.admission.frames.len() as usize,
            before.capacity, after_len, after_capacity,
            before.frame_bytes, before.frame_bytes,
        ) {
            SourceFramePushObservationResult::Success { .. } => true,
            _ => false,
        },
    ensures match source_frame_push_observe_after(
            request_kind, actual_top_kind,
            before.score.admission.frames.len() as usize,
            before.capacity, after_len, after_capacity,
            before.frame_bytes, before.frame_bytes,
        ) {
            SourceFramePushObservationResult::Success { storage } =>
                source_successful_push_storage(before, storage),
            _ => false,
        },
{
    source_frame_push_observe_after_exact(
        request_kind, actual_top_kind,
        before.score.admission.frames.len() as usize,
        before.capacity, after_len, after_capacity,
        before.frame_bytes, before.frame_bytes,
    );
}

pub open spec fn source_frame_request_matches_child(
    request: SourceFrameRequestImage,
) -> bool {
    source_frame_request_kind_code(request.request_kind) == request.child.frame_kind
}

pub open spec fn source_successful_push_storage(
    before: RuntimeFrameImage, storage: SourceFramePushStorageImage,
) -> bool {
    runtime_frame_image_wf(before)
        && storage.before_len as int == before.score.admission.frames.len()
        && storage.before_capacity == before.capacity
        && storage.frame_variant_bytes == before.frame_bytes
        && storage.stats_frame_variant_bytes == storage.frame_variant_bytes
        && storage.frame_variant_bytes > 0
        && storage.after_len as int == storage.before_len as int + 1
        && storage.after_len <= storage.after_capacity
        && storage.before_capacity <= storage.after_capacity
        && storage.used_bytes as int
            == storage.after_len as int * storage.frame_variant_bytes as int
        && storage.reserved_bytes as int
            == storage.after_capacity as int * storage.frame_variant_bytes as int
}

pub open spec fn source_checked_frame_request_push(
    before: RuntimeFrameImage, request: SourceFrameRequestImage,
    storage: SourceFramePushStorageImage,
) -> Option<RuntimeFrameImage> {
    if source_frame_request_kind_code(storage.request_kind)
        != source_frame_request_kind_code(request.request_kind)
        || !source_frame_request_matches_child(request)
        || !source_successful_push_storage(before, storage)
    {
        None
    } else {
        runtime_push(before, request.field, request.child, storage.after_capacity)
    }
}

pub open spec fn source_checked_frame_request_push_indexed(
    before: RuntimeFrameImage,
    request: SourceFrameRequestImage,
    parent_source: admission::AdmissionIndexedSourceImage,
    storage: SourceFramePushStorageImage,
) -> Option<RuntimeFrameImage> {
    if source_frame_request_kind_code(storage.request_kind)
        != source_frame_request_kind_code(request.request_kind)
        || !source_frame_request_matches_child(request)
        || !source_successful_push_storage(before, storage)
    {
        None
    } else {
        runtime_push_indexed(
            before, request.field, request.child, parent_source,
            storage.after_capacity,
        )
    }
}

/// This checked route corresponds to `matches_pushed_frame` only when the
/// supplied observation has been taken from the actual post-push top frame.
/// The older route above models the Vec/storage check and frame-kind check.
pub open spec fn source_checked_frame_request_push_fresh(
    before: RuntimeFrameImage,
    request: SourceFrameRequestImage,
    storage: SourceFramePushStorageImage,
    observed: SourceFreshFrameObservation,
) -> Option<RuntimeFrameImage> {
    match source_checked_frame_request_push(before, request, storage) {
        Some(after) if source_fresh_frame_matches(
            request, after.score.admission.frames.last(), observed,
        ) => Some(after),
        _ => None,
    }
}

/// For a Message/Map field push, the selected child is precisely the indexed
/// payload view. CanonicalPath and EPathMap Push sources have separate path
/// reducers, so this indexed-field relation does not purport to cover them.
pub open spec fn source_indexed_child_selected_view_matches(
    before: RuntimeFrameImage,
    request: SourceFrameRequestImage,
    parent_source: admission::AdmissionIndexedSourceImage,
    selected: SourceSelectedFrameView,
) -> bool {
    if before.score.admission.frames.len() == 0 {
        false
    } else {
        let parent = before.score.admission.frames.last();
        let child_source = admission::admission_indexed_field_child_source(
            parent, parent_source, request.field,
        );
        &&& (source_frame_request_kind_code(request.request_kind) == 1
            || source_frame_request_kind_code(request.request_kind) == 2)
        &&& child_source.is_some()
        &&& admission::admission_indexed_source_wf(selected.indexed)
        &&& selected.indexed.bytes == child_source.unwrap().bytes
        &&& selected.indexed.positions == child_source.unwrap().positions
        &&& selected.indexed.end == child_source.unwrap().end
    }
}

pub open spec fn source_checked_frame_request_push_indexed_fresh(
    before: RuntimeFrameImage,
    request: SourceFrameRequestImage,
    parent_source: admission::AdmissionIndexedSourceImage,
    storage: SourceFramePushStorageImage,
    observed: SourceFreshFrameObservation,
) -> Option<RuntimeFrameImage> {
    if !source_indexed_child_selected_view_matches(
        before, request, parent_source, observed.selected,
    ) {
        None
    } else {
        match source_checked_frame_request_push_indexed(
            before, request, parent_source, storage,
        ) {
            Some(after) if source_fresh_frame_matches(
                request, after.score.admission.frames.last(), observed,
            ) => Some(after),
            _ => None,
        }
    }
}

pub struct SourceFreshRootObservation {
    pub ghost frame: SourceFreshFrameObservation,
    pub before_len: usize,
    pub after_len: usize,
    pub score_entries_before: usize,
    pub score_entries_after: usize,
}

/// `RootInitializationSource::matches_root` accepts only Message and
/// CanonicalPath roots. The root starts with an empty score arena and one
/// fresh frame; the view/cursor checks are shared with child publication.
pub open spec fn source_fresh_root_matches(
    request: SourceFrameRequestImage,
    observed: SourceFreshRootObservation,
) -> bool {
    &&& observed.before_len == 0
    &&& observed.after_len == 1
    &&& observed.score_entries_before == 0
    &&& observed.score_entries_after == 0
    &&& request.child.body_id == 0
    &&& request.child.return_id.is_none()
    &&& request.child.score_handle.is_none()
    &&& source_fresh_frame_matches(request, request.child, observed.frame)
    &&& match request.request_kind {
        SourceFrameRequestKind::Message => admission::admission_root_initial(request.child),
        SourceFrameRequestKind::CanonicalPath =>
            admission::admission_frame_wf(request.child)
                && admission::admission_canonical_path_slotless(request.child)
                && request.child.program_type_id == 0
                && request.child.nested_started == 0
                && request.child.nested_resolved == 0,
        _ => false,
    }
}

pub open spec fn source_checked_fresh_root(
    request: SourceFrameRequestImage,
    observed: SourceFreshRootObservation,
) -> Option<admission::AdmissionState> {
    if source_fresh_root_matches(request, observed) {
        Some(admission::admission_initial_state(request.child))
    } else {
        None
    }
}

pub struct SourceMessageFinishResult {
    pub ghost updated_top: admission::AdmissionFrameState,
    pub ghost score_arena: Seq<admission::AdmissionScoreBinding>,
    pub ghost transcript: score::ScoreArena,
}

pub open spec fn source_message_finish_bridge(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
) -> Option<bridge::ScoreAdmissionBridge> {
    if before.score.admission.frames.len() == 0 {
        None
    } else {
        let prior = before.score.admission;
        Some(bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: prior.frames.update(prior.frames.len() - 1, result.updated_top),
                arena: result.score_arena,
                root_end: prior.root_end,
                pushed: prior.pushed,
                completed: prior.completed,
                next_obligation_id: prior.next_obligation_id,
            },
            transcript: result.transcript,
        })
    }
}

pub proof fn source_message_finish_keeps_driver_control(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
)
    ensures match source_message_finish_bridge(before, result) {
        None => before.score.admission.frames.len() == 0,
        Some(after) => {
            &&& after.admission.frames.len()
                == before.score.admission.frames.len()
            &&& after.admission.pushed == before.score.admission.pushed
            &&& after.admission.completed == before.score.admission.completed
            &&& after.admission.next_obligation_id
                == before.score.admission.next_obligation_id
            &&& after.admission.root_end == before.score.admission.root_end
            &&& forall|i: int| 0 <= i < after.admission.frames.len() - 1
                ==> after.admission.frames[i] == before.score.admission.frames[i]
        },
    },
{}

pub open spec fn runtime_empty_send_before() -> RuntimeFrameImage {
    RuntimeFrameImage {
        score: finish::empty_send_finish_before(),
        capacity: 1,
        frame_bytes: 1,
        max_frames: 1,
        max_storage_bytes: 1,
        max_capacity_bytes: 1,
    }
}

pub open spec fn runtime_empty_send_return() -> Option<RuntimeRootReturnImage> {
    match finish::empty_send_finish_after() {
        None => None,
        Some(after_finish) => runtime_message_eof_root_return(
            runtime_empty_send_before(), after_finish,
        ),
    }
}

pub open spec fn runtime_noscore_receive_bind_before() -> RuntimeFrameImage {
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::admission_fabricated_child_state(
                admission::AdmissionFamily::Receive, 0,
                admission::AdmissionFamily::ReceiveBindFrame, 1,
            ),
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_match_case_before() -> RuntimeFrameImage {
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::admission_fabricated_child_state(
                admission::AdmissionFamily::Match, 1,
                admission::AdmissionFamily::MatchCaseFrame, 2,
            ),
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_big_rational_before() -> RuntimeFrameImage {
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::admission_fabricated_child_state(
                admission::AdmissionFamily::Expr, 0,
                admission::AdmissionFamily::BigRationalFrame, 36,
            ),
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_evar_before() -> RuntimeFrameImage {
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::admission_fabricated_child_state(
                admission::AdmissionFamily::Expr, 0,
                admission::AdmissionFamily::EVarFrame, 19,
            ),
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_expr_unary_before() -> RuntimeFrameImage {
    let state = admission::admission_fabricated_child_state(
        admission::AdmissionFamily::Expr, 0,
        admission::AdmissionFamily::Other, 5,
    );
    let child = admission::AdmissionFrameState {
        program_type_id: 36,
        slot_count: 1,
        ..state.frames[1]
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: state.frames.update(1, child),
                ..state
            },
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_expr_binary_before() -> RuntimeFrameImage {
    let state = admission::admission_fabricated_child_state(
        admission::AdmissionFamily::Expr, 0,
        admission::AdmissionFamily::Other, 7,
    );
    let child = admission::AdmissionFrameState {
        program_type_id: 38,
        slot_count: 2,
        ..state.frames[1]
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: state.frames.update(1, child),
                ..state
            },
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_emethod_before() -> RuntimeFrameImage {
    let state = admission::admission_fabricated_child_state(
        admission::AdmissionFamily::Expr, 0,
        admission::AdmissionFamily::Other, 24,
    );
    let child = admission::AdmissionFrameState {
        program_type_id: 33,
        slot_count: 5,
        ..state.frames[1]
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: state.frames.update(1, child),
                ..state
            },
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_key_value_pair_before() -> RuntimeFrameImage {
    let parent = admission::AdmissionFrameState {
        program_type_id: 30,
        slot_count: 4,
        last_slot: Some(0),
        seen: Set::empty().insert(0),
        waiting_id: Some(0),
        waiting_kind: 1,
        waiting_slot: Some(0),
        waiting_tag: Some(1),
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    let child = admission::AdmissionFrameState {
        program_type_id: 34,
        slot_count: 2,
        return_id: Some(0),
        body_id: 8,
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: Seq::empty().push(parent).push(child),
                arena: Seq::empty(),
                root_end: 0,
                pushed: 2,
                completed: 0,
                next_obligation_id: 1,
            },
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_key_value_pair_present_before() -> RuntimeFrameImage {
    let empty = runtime_noscore_key_value_pair_before();
    let key = admission::AdmissionReturnedSummary {
        slot: 0,
        tag: 1,
        body_id: 9,
        body_bytes: Seq::empty(),
        score_handle: Some(0),
        score_origin_body_id: Some(9),
        kind: admission::AdmissionReturnedKind::Par {
            semantic_default: false, canonical_path_stable: true,
        },
    };
    let value = admission::AdmissionReturnedSummary {
        slot: 1,
        tag: 2,
        body_id: 10,
        body_bytes: Seq::empty(),
        score_handle: Some(1),
        score_origin_body_id: Some(10),
        kind: admission::AdmissionReturnedKind::Par {
            semantic_default: false, canonical_path_stable: true,
        },
    };
    let child = admission::AdmissionFrameState {
        last_slot: Some(1),
        seen: Set::empty().insert(0).insert(1),
        returns: Seq::empty().push(key).push(value),
        nested_started: 2,
        nested_resolved: 2,
        ..empty.score.admission.frames[1]
    };
    let key_bytes = score::integer_transcript(0);
    let value_bytes = score::integer_transcript(1);
    let key_entry = score::ScoreEntry {
        kind: score::ScoreNode::Integer(0),
        transcript: key_bytes,
        encoded_len: 10,
    };
    let value_entry = score::ScoreEntry {
        kind: score::ScoreNode::Integer(1),
        transcript: value_bytes,
        encoded_len: 10,
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: Seq::empty().push(empty.score.admission.frames[0]).push(child),
                arena: Seq::empty().push(admission::AdmissionScoreBinding {
                    body_id: 9, semantic: true, encoded_len: 10,
                    transcript_bytes: key_bytes,
                }).push(admission::AdmissionScoreBinding {
                    body_id: 10, semantic: true, encoded_len: 10,
                    transcript_bytes: value_bytes,
                }),
                ..empty.score.admission
            },
            transcript: score::ScoreArena {
                entries: Seq::empty().push(key_entry).push(value_entry),
            },
        },
        ..empty
    }
}

pub open spec fn runtime_noscore_opaque_before() -> RuntimeFrameImage {
    let parent = admission::AdmissionFrameState {
        program_type_id: 2,
        slot_count: 4,
        last_slot: Some(0),
        seen: Set::empty().insert(0),
        waiting_id: Some(0),
        waiting_kind: 1,
        waiting_slot: Some(0),
        waiting_tag: Some(1),
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    let child = admission::AdmissionFrameState {
        program_type_id: 3,
        slot_count: 2,
        return_id: Some(0),
        body_id: 8,
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: Seq::empty().push(parent).push(child),
                arena: Seq::empty(),
                root_end: 0,
                pushed: 2,
                completed: 0,
                next_obligation_id: 1,
            },
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_witness_program(program: u16) -> bool {
    program == 24 || program == 26 || program == 30 || program == 32 || program == 55
        || program == 56 || program == 57 || program == 60 || program == 61 || program == 64
}

pub open spec fn runtime_noscore_leaf_before(
    parent_family: admission::AdmissionFamily, field_tag: u32,
    child_family: admission::AdmissionFamily, child_program: u16, child_slots: usize,
) -> RuntimeFrameImage {
    let state = admission::admission_fabricated_child_state(
        parent_family, 0, child_family, field_tag,
    );
    let child = admission::AdmissionFrameState {
        program_type_id: child_program,
        slot_count: child_slots,
        ..state.frames[1]
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: state.frames.update(1, child),
                ..state
            },
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_noscore_absent_expr_payload() -> admission::AdmissionReturnedKind {
    admission::AdmissionReturnedKind::ExprPayload {
        score_present: false, canonical_fixed_point: true, canonical_path_stable: false,
    }
}

pub open spec fn runtime_noscore_ezipper_absent_before() -> RuntimeFrameImage {
    runtime_noscore_leaf_before(
        admission::AdmissionFamily::Expr, 33, admission::AdmissionFamily::Other, 32, 6,
    )
}

pub open spec fn runtime_noscore_emap_absent_pair() -> admission::AdmissionReturnedSummary {
    admission::AdmissionReturnedSummary {
        slot: 0,
        tag: 1,
        body_id: 9,
        body_bytes: Seq::empty(),
        score_handle: None,
        score_origin_body_id: None,
        kind: admission::AdmissionReturnedKind::KeyValuePair { key: None, value_par: None },
    }
}

pub open spec fn runtime_noscore_emap_incomplete_before() -> RuntimeFrameImage {
    let leaf = runtime_noscore_leaf_before(
        admission::AdmissionFamily::Expr, 23, admission::AdmissionFamily::Other, 30, 4,
    );
    let child = admission::AdmissionFrameState {
        last_slot: Some(0usize),
        seen: Set::empty().insert(0int),
        returns: Seq::empty().push(runtime_noscore_emap_absent_pair()),
        ..leaf.score.admission.frames[1]
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: leaf.score.admission.frames.update(1, child),
                ..leaf.score.admission
            },
            ..leaf.score
        },
        ..leaf
    }
}

pub open spec fn runtime_epm_root_before() -> RuntimeFrameImage {
    let snapshot = admission::AdmissionReturnedSummary {
        slot: 3,
        tag: 9,
        body_id: 8,
        body_bytes: Seq::empty(),
        score_handle: None,
        score_origin_body_id: None,
        kind: admission::AdmissionReturnedKind::EPathMapSnapshot {
            mode: 0, entries_canonical_path_stable: true,
        },
    };
    let root = admission::AdmissionFrameState {
        program_type_id: 31,
        slot_count: 4,
        last_slot: Some(3),
        seen: Set::empty().insert(3),
        required: Set::empty().insert(3),
        returns: Seq::empty().push(snapshot),
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::Other, Set::empty(), Seq::empty(),
        )
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: Seq::empty().push(root),
                arena: Seq::empty(),
                root_end: 0,
                pushed: 1,
                completed: 0,
                next_obligation_id: 0,
            },
            transcript: score::empty_arena(),
        },
        capacity: 1,
        frame_bytes: 1,
        max_frames: 1,
        max_storage_bytes: 1,
        max_capacity_bytes: 1,
    }
}

pub open spec fn runtime_epm_child_before() -> RuntimeFrameImage {
    let root = runtime_epm_root_before();
    let parent = admission::AdmissionFrameState {
        program_type_id: 24,
        slot_count: 1,
        last_slot: Some(0),
        seen: Set::empty().insert(0),
        waiting_id: Some(0),
        waiting_kind: 1,
        waiting_slot: Some(0),
        waiting_tag: Some(32),
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::Expr, Set::empty(), Seq::empty(),
        )
    };
    let child = admission::AdmissionFrameState {
        return_id: Some(0),
        body_id: 8,
        ..root.score.admission.frames[0]
    };
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: admission::AdmissionState {
                frames: Seq::empty().push(parent).push(child),
                arena: Seq::empty(),
                root_end: 0,
                pushed: 2,
                completed: 0,
                next_obligation_id: 1,
            },
            transcript: score::empty_arena(),
        },
        capacity: 2,
        frame_bytes: 1,
        max_frames: 2,
        max_storage_bytes: 2,
        max_capacity_bytes: 2,
    }
}

pub open spec fn runtime_frame_image_wf(image: RuntimeFrameImage) -> bool {
    admission::admission_state_wf(image.score.admission)
        && bridge::bridge_well_formed(image.score)
        && image.score.admission.frames.len() > 0
        && image.frame_bytes > 0
        && image.score.admission.frames.len() <= image.capacity as int
        && image.capacity as int * image.frame_bytes as int <= usize::MAX as int
        && image.max_frames as int >= image.score.admission.frames.len()
        && image.max_storage_bytes as int
            >= image.score.admission.frames.len() * image.frame_bytes as int
        && image.max_capacity_bytes as int
            >= image.capacity as int * image.frame_bytes as int
}

pub open spec fn runtime_after_admission_step(
    before: RuntimeFrameImage, next: admission::AdmissionState,
) -> RuntimeFrameImage {
    RuntimeFrameImage {
        score: bridge::ScoreAdmissionBridge {
            admission: next,
            transcript: before.score.transcript,
        },
        capacity: before.capacity,
        frame_bytes: before.frame_bytes,
        max_frames: before.max_frames,
        max_storage_bytes: before.max_storage_bytes,
        max_capacity_bytes: before.max_capacity_bytes,
    }
}

pub open spec fn runtime_continue(
    before: RuntimeFrameImage, field: admission::IndexedCanonicalFieldModel,
) -> Option<RuntimeFrameImage> {
    if !runtime_frame_image_wf(before) {
        None
    } else { match admission::concrete_message_driver_step(
        before.score.admission,
        admission::ConcreteFrameActionModel::Continue { field },
    ) {
        Some(admission::ConcreteFrameOutcomeModel::Continued { state }) => {
            Some(runtime_after_admission_step(before, state))
        },
        _ => None,
    } }
}

pub open spec fn runtime_continue_indexed(
    before: RuntimeFrameImage, field: admission::IndexedCanonicalFieldModel,
    parent_source: admission::AdmissionIndexedSourceImage,
) -> Option<RuntimeFrameImage> {
    if !runtime_frame_image_wf(before) {
        None
    } else { match admission::concrete_message_driver_step(
        before.score.admission,
        admission::ConcreteFrameActionModel::ContinueIndexed {
            field, parent_source,
        },
    ) {
        Some(admission::ConcreteFrameOutcomeModel::Continued { state }) => {
            Some(runtime_after_admission_step(before, state))
        },
        _ => None,
    } }
}

pub open spec fn runtime_push(
    before: RuntimeFrameImage, field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState, observed_capacity: usize,
) -> Option<RuntimeFrameImage> {
    match admission::concrete_message_driver_step(
        before.score.admission,
        admission::ConcreteFrameActionModel::Push {
            field,
            child,
            frame_bytes: before.frame_bytes,
        },
    ) {
        Some(admission::ConcreteFrameOutcomeModel::Pushed { state }) => {
            if !runtime_frame_image_wf(before)
                || state.frames.len() > observed_capacity as int
                || observed_capacity as int * before.frame_bytes as int > usize::MAX as int
            {
                None
            } else {
                let used = state.frames.len() * before.frame_bytes as int;
                let reserved = observed_capacity as int * before.frame_bytes as int;
                Some(RuntimeFrameImage {
                    score: bridge::ScoreAdmissionBridge {
                        admission: state,
                        transcript: before.score.transcript,
                    },
                    capacity: observed_capacity,
                    frame_bytes: before.frame_bytes,
                    max_frames: if before.max_frames as int >= state.frames.len() {
                        before.max_frames
                    } else {
                        state.frames.len() as usize
                    },
                    max_storage_bytes: if before.max_storage_bytes as int >= used {
                        before.max_storage_bytes
                    } else {
                        used as usize
                    },
                    max_capacity_bytes: if before.max_capacity_bytes as int >= reserved {
                        before.max_capacity_bytes
                    } else {
                        reserved as usize
                    },
                })
            }
        },
        _ => None,
    }
}

pub open spec fn runtime_push_indexed(
    before: RuntimeFrameImage,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    observed_capacity: usize,
) -> Option<RuntimeFrameImage> {
    match admission::concrete_message_driver_step(
        before.score.admission,
        admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes: before.frame_bytes,
        },
    ) {
        Some(admission::ConcreteFrameOutcomeModel::Pushed { state }) => {
            if !runtime_frame_image_wf(before)
                || state.frames.len() > observed_capacity as int
                || observed_capacity as int * before.frame_bytes as int > usize::MAX as int
            {
                None
            } else {
                let used = state.frames.len() * before.frame_bytes as int;
                let reserved = observed_capacity as int * before.frame_bytes as int;
                Some(RuntimeFrameImage {
                    score: bridge::ScoreAdmissionBridge {
                        admission: state,
                        transcript: before.score.transcript,
                    },
                    capacity: observed_capacity,
                    frame_bytes: before.frame_bytes,
                    max_frames: if before.max_frames as int >= state.frames.len() {
                        before.max_frames
                    } else {
                        state.frames.len() as usize
                    },
                    max_storage_bytes: if before.max_storage_bytes as int >= used {
                        before.max_storage_bytes
                    } else {
                        used as usize
                    },
                    max_capacity_bytes: if before.max_capacity_bytes as int >= reserved {
                        before.max_capacity_bytes
                    } else {
                        reserved as usize
                    },
                })
            }
        },
        _ => None,
    }
}

#[verifier::opaque]
pub open spec fn runtime_pre_pop_child_accept(
    pre_pop: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    if !runtime_frame_image_wf(pre_pop)
        || pre_pop.score.admission.frames.len() <= 1 {
        None
    } else {
        match admission::concrete_message_driver_step(
            pre_pop.score.admission, admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                Some(runtime_after_admission_step(pre_pop, state))
            },
            _ => None,
        }
    }
}

#[verifier::opaque]
pub open spec fn runtime_score_source_edge(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    bridge::bridge_score_source_step(before, after)
}

pub proof fn runtime_score_source_edge_exact(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
)
    ensures runtime_score_source_edge(before, after)
        == bridge::bridge_score_source_step(before, after),
{
    reveal(runtime_score_source_edge);
}

#[verifier::opaque]
pub open spec fn runtime_child_eof_gate(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
) -> bool {
    runtime_frame_image_wf(before)
        && before.score.admission.frames.len() > 1
        && before.score.admission.frames.last().frame_kind == 1
        && before.score.admission.frames.last().waiting_id.is_none()
        && before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end
        && before.score.admission.frames.last().score_handle.is_none()
        && runtime_score_source_edge(before.score, after_finish)
        && bridge::bridge_well_formed(after_finish)
        && admission::admission_state_wf(after_finish.admission)
        && after_finish.admission.frames.len()
            == before.score.admission.frames.len()
        && after_finish.admission.pushed == before.score.admission.pushed
        && after_finish.admission.completed == before.score.admission.completed
}

pub proof fn runtime_child_eof_gate_intro(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
)
    requires
        runtime_frame_image_wf(before),
        before.score.admission.frames.len() > 1,
        before.score.admission.frames.last().frame_kind == 1,
        before.score.admission.frames.last().waiting_id.is_none(),
        before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end,
        before.score.admission.frames.last().score_handle.is_none(),
        runtime_score_source_edge(before.score, after_finish),
        bridge::bridge_well_formed(after_finish),
        admission::admission_state_wf(after_finish.admission),
        after_finish.admission.frames.len() == before.score.admission.frames.len(),
        after_finish.admission.pushed == before.score.admission.pushed,
        after_finish.admission.completed == before.score.admission.completed,
    ensures runtime_child_eof_gate(before, after_finish),
{
    reveal(runtime_child_eof_gate);
}

pub proof fn runtime_child_eof_gate_depth(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
)
    requires runtime_child_eof_gate(before, after_finish),
    ensures
        before.score.admission.frames.len() > 1,
        after_finish.admission.frames.len()
            == before.score.admission.frames.len(),
        after_finish.admission.pushed == before.score.admission.pushed,
        after_finish.admission.completed == before.score.admission.completed,
{
    reveal(runtime_child_eof_gate);
}

pub open spec fn runtime_score_finish_control(
    before: bridge::ScoreAdmissionBridge, after: bridge::ScoreAdmissionBridge,
) -> bool {
    after.admission.frames.len() == before.admission.frames.len()
        && after.admission.pushed == before.admission.pushed
        && after.admission.completed == before.admission.completed
        && after.admission.next_obligation_id == before.admission.next_obligation_id
        && after.admission.root_end == before.admission.root_end
}

pub proof fn runtime_score_finish_control_transitive(
    before: bridge::ScoreAdmissionBridge, middle: bridge::ScoreAdmissionBridge,
    after: bridge::ScoreAdmissionBridge,
)
    requires
        runtime_score_finish_control(before, middle),
        runtime_score_finish_control(middle, after),
    ensures runtime_score_finish_control(before, after),
{}

pub proof fn runtime_unattached_keeps_driver_control(
    before: bridge::ScoreAdmissionBridge, kind: score::ScoreNode,
)
    ensures match bridge::bridge_append_unattached(before, kind) {
        None => true,
        Some((after, _)) => runtime_score_finish_control(before, after),
    },
{
    if let Some((after, _)) = bridge::bridge_append_unattached(before, kind) {
        assert(after.admission.frames == before.admission.frames);
        assert(after.admission.pushed == before.admission.pushed);
        assert(after.admission.completed == before.admission.completed);
        assert(after.admission.next_obligation_id == before.admission.next_obligation_id);
        assert(after.admission.root_end == before.admission.root_end);
    }
}

pub proof fn runtime_constructor_keeps_driver_control(
    before: bridge::ScoreAdmissionBridge, constructor: i32, children: Seq<usize>,
)
    ensures match bridge::bridge_constructor_step(before, constructor, children) {
        None => true,
        Some((after, _, _)) => runtime_score_finish_control(before, after),
    },
{
    if let Some((after, _, _)) = bridge::bridge_constructor_step(before, constructor, children) {
        let kind = score::ScoreNode::Integer(constructor as i64);
        runtime_unattached_keeps_driver_control(before, kind);
        let (after_constructor, _) = bridge::bridge_append_unattached(before, kind).unwrap();
        let node_kind = score::ScoreNode::Node(
            seq![bridge::bridge_append_unattached(before, kind).unwrap().1] + children,
        );
        runtime_unattached_keeps_driver_control(after_constructor, node_kind);
        let (after_node, score_handle) = bridge::bridge_append_unattached(
            after_constructor, node_kind,
        ).unwrap();
        let attached = admission::admission_attach_score_step(
            after_node.admission, score_handle, before.admission.frames.last().body_id,
        ).unwrap();
        assert(runtime_score_finish_control(before, after_constructor));
        assert(runtime_score_finish_control(after_constructor, after_node));
        assert(runtime_score_finish_control(after_node, bridge::ScoreAdmissionBridge {
            admission: attached,
            transcript: after_node.transcript,
        }));
        runtime_score_finish_control_transitive(before, after_constructor, after_node);
        runtime_score_finish_control_transitive(before, after_node, after);
    }
}

pub open spec fn runtime_send_finish(
    before: RuntimeFrameImage,
) -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_send_step(before.score) {
        None => None,
        Some((after, _, _, _, _)) => Some(after),
    }
}

pub open spec fn runtime_send_child_eof_ready(before: RuntimeFrameImage) -> bool {
    runtime_frame_image_wf(before)
        && before.score.admission.frames.len() > 1
        && before.score.admission.frames.last().frame_kind == 1
        && before.score.admission.frames.last().waiting_id.is_none()
        && before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end
        && before.score.admission.frames.last().score_handle.is_none()
        && bridge::bridge_send_step(before.score).is_some()
}

pub proof fn runtime_send_finish_discharges_child_gate(before: RuntimeFrameImage)
    requires runtime_send_child_eof_ready(before),
    ensures match runtime_send_finish(before) {
        None => false,
        Some(after) => runtime_child_eof_gate(before, after),
    },
{
    let (after, _, _, _, _) = bridge::bridge_send_step(before.score).unwrap();
    bridge::bridge_send_exact(before.score);
    bridge::bridge_send_shape(before.score);
    let top = before.score.admission.frames.last();
    let (persistent, channel, data, connective_used, cost) = bridge::bridge_send_parts(
        top, before.score.admission.arena,
    ).unwrap();
    let persistent_kind = score::ScoreNode::Integer(if persistent { 1i64 } else { 0i64 });
    let connective_kind = score::ScoreNode::Integer(if connective_used { 1i64 } else { 0i64 });
    runtime_unattached_keeps_driver_control(before.score, persistent_kind);
    let (after_persistent, persistent_handle) = bridge::bridge_append_unattached(
        before.score, persistent_kind,
    ).unwrap();
    runtime_unattached_keeps_driver_control(after_persistent, connective_kind);
    let (after_connective, connective_handle) = bridge::bridge_append_unattached(
        after_persistent, connective_kind,
    ).unwrap();
    let children = seq![persistent_handle, channel] + data + seq![connective_handle] + cost;
    runtime_constructor_keeps_driver_control(after_connective, 300i32, children);
    runtime_score_finish_control_transitive(before.score, after_persistent, after_connective);
    runtime_score_finish_control_transitive(before.score, after_connective, after);
    assert(runtime_score_finish_control(before.score, after));
    assert(bridge::bridge_same(after, after));
    assert(bridge::bridge_score_source_step(before.score, after));
    runtime_score_source_edge_exact(before.score, after);
    reveal(runtime_child_eof_gate);
}

pub open spec fn runtime_send_child_eof_accept(
    before: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    match runtime_send_finish(before) {
        None => None,
        Some(after_finish) => runtime_message_eof_child_accept(before, after_finish),
    }
}

pub proof fn runtime_send_child_eof_accept_refines(before: RuntimeFrameImage)
    requires runtime_send_child_eof_ready(before),
    ensures match runtime_send_child_eof_accept(before) {
        None => true,
        Some(after) => {
            let finish = runtime_send_finish(before).unwrap();
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == finish.admission.arena
            &&& after.score.transcript == finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    runtime_send_finish_discharges_child_gate(before);
    let after_finish = runtime_send_finish(before).unwrap();
    runtime_child_eof_completes_after_source_score(before, after_finish);
}

pub open spec fn runtime_send_parent_accepts(before: RuntimeFrameImage) -> bool {
    match runtime_send_finish(before) {
        None => false,
        Some(after_finish) => match admission::concrete_message_driver_step(
            after_finish.admission, admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: _ }) => true,
            _ => false,
        },
    }
}

pub proof fn runtime_send_child_eof_accepts_when_parent_accepts(before: RuntimeFrameImage)
    requires
        runtime_send_child_eof_ready(before),
        runtime_send_parent_accepts(before),
    ensures match runtime_send_child_eof_accept(before) {
        None => false,
        Some(after) => match admission::concrete_message_driver_step(
            runtime_send_finish(before).unwrap().admission,
            admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                &&& after.score.admission == state
                &&& after.score.transcript == runtime_send_finish(before).unwrap().transcript
                &&& after.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& after.score.admission.completed as int
                    == before.score.admission.completed as int + 1
            },
            _ => false,
        },
    },
{
    runtime_send_finish_discharges_child_gate(before);
    let after_finish = runtime_send_finish(before).unwrap();
    let pre_pop = RuntimeFrameImage { score: after_finish, ..before };
    reveal(runtime_child_eof_gate);
    assert(admission::admission_state_wf(pre_pop.score.admission));
    assert(bridge::bridge_well_formed(pre_pop.score));
    assert(pre_pop.score.admission.frames.len() == before.score.admission.frames.len());
    assert(runtime_frame_image_wf(pre_pop));
    reveal(runtime_message_eof_child_accept);
    reveal(runtime_pre_pop_child_accept);
    admission::concrete_message_driver_step_simulates(
        after_finish.admission, admission::ConcreteFrameActionModel::Complete,
    );
}

pub open spec fn runtime_bundle_finish(
    before: RuntimeFrameImage,
) -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_bundle_step(before.score) {
        None => None,
        Some((after, _, _)) => Some(after),
    }
}

pub open spec fn runtime_bundle_child_eof_ready(before: RuntimeFrameImage) -> bool {
    runtime_frame_image_wf(before)
        && before.score.admission.frames.len() > 1
        && before.score.admission.frames.last().frame_kind == 1
        && before.score.admission.frames.last().family == admission::AdmissionFamily::Bundle
        && before.score.admission.frames.last().waiting_id.is_none()
        && before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end
        && before.score.admission.frames.last().score_handle.is_none()
        && bridge::bridge_bundle_step(before.score).is_some()
}

pub proof fn runtime_bundle_finish_discharges_child_gate(before: RuntimeFrameImage)
    requires runtime_bundle_child_eof_ready(before),
    ensures match runtime_bundle_finish(before) {
        None => false,
        Some(after) => {
            &&& runtime_child_eof_gate(before, after)
            &&& after.admission.arena[after.admission.frames.last().score_handle.unwrap()
                as int].semantic
        },
    },
{
    let (after, _, _) = bridge::bridge_bundle_step(before.score).unwrap();
    bridge::bridge_bundle_exact(before.score);
    bridge::bridge_bundle_source_finish(before.score);
    runtime_score_source_edge_exact(before.score, after);
    reveal(runtime_child_eof_gate);
}

pub open spec fn runtime_bundle_child_eof_accept(
    before: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    match runtime_bundle_finish(before) {
        None => None,
        Some(after_finish) => runtime_message_eof_child_accept(before, after_finish),
    }
}

pub proof fn runtime_bundle_child_eof_accept_refines(before: RuntimeFrameImage)
    requires runtime_bundle_child_eof_ready(before),
    ensures match runtime_bundle_child_eof_accept(before) {
        None => true,
        Some(after) => {
            let finish = runtime_bundle_finish(before).unwrap();
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == finish.admission.arena
            &&& after.score.transcript == finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    runtime_bundle_finish_discharges_child_gate(before);
    let after_finish = runtime_bundle_finish(before).unwrap();
    runtime_child_eof_completes_after_source_score(before, after_finish);
}

pub open spec fn runtime_bundle_parent_accepts(before: RuntimeFrameImage) -> bool {
    match runtime_bundle_finish(before) {
        None => false,
        Some(after_finish) => match admission::concrete_message_driver_step(
            after_finish.admission, admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: _ }) => true,
            _ => false,
        },
    }
}

pub proof fn runtime_bundle_child_eof_accepts_when_parent_accepts(
    before: RuntimeFrameImage,
)
    requires
        runtime_bundle_child_eof_ready(before),
        runtime_bundle_parent_accepts(before),
    ensures match runtime_bundle_child_eof_accept(before) {
        None => false,
        Some(after) => match admission::concrete_message_driver_step(
            runtime_bundle_finish(before).unwrap().admission,
            admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                &&& after.score.admission == state
                &&& after.score.transcript == runtime_bundle_finish(before).unwrap().transcript
                &&& after.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& after.score.admission.completed as int
                    == before.score.admission.completed as int + 1
            },
            _ => false,
        },
    },
{
    runtime_bundle_finish_discharges_child_gate(before);
    let after_finish = runtime_bundle_finish(before).unwrap();
    let pre_pop = RuntimeFrameImage { score: after_finish, ..before };
    reveal(runtime_child_eof_gate);
    assert(admission::admission_state_wf(pre_pop.score.admission));
    assert(bridge::bridge_well_formed(pre_pop.score));
    assert(pre_pop.score.admission.frames.len() == before.score.admission.frames.len());
    assert(runtime_frame_image_wf(pre_pop));
    reveal(runtime_message_eof_child_accept);
    reveal(runtime_pre_pop_child_accept);
    admission::concrete_message_driver_step_simulates(
        after_finish.admission, admission::ConcreteFrameActionModel::Complete,
    );
}

pub open spec fn runtime_if_finish(
    before: RuntimeFrameImage,
) -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_if_step(before.score) {
        None => None,
        Some((after, _, _, _)) => Some(after),
    }
}

pub open spec fn runtime_if_child_eof_ready(before: RuntimeFrameImage) -> bool {
    runtime_frame_image_wf(before)
        && before.score.admission.frames.len() > 1
        && before.score.admission.frames.last().frame_kind == 1
        && before.score.admission.frames.last().family == admission::AdmissionFamily::If
        && before.score.admission.frames.last().waiting_id.is_none()
        && before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end
        && before.score.admission.frames.last().score_handle.is_none()
        && bridge::bridge_if_step(before.score).is_some()
}

pub proof fn runtime_if_finish_discharges_child_gate(before: RuntimeFrameImage)
    requires runtime_if_child_eof_ready(before),
    ensures match runtime_if_finish(before) {
        None => false,
        Some(after) => {
            &&& runtime_child_eof_gate(before, after)
            &&& after.admission.arena[after.admission.frames.last().score_handle.unwrap()
                as int].semantic
        },
    },
{
    let (after, _, _, _) = bridge::bridge_if_step(before.score).unwrap();
    bridge::bridge_if_exact(before.score);
    let top = before.score.admission.frames.last();
    let (condition, if_true, if_false, connective_used) = bridge::bridge_if_parts(
        top, before.score.admission.arena,
    ).unwrap();
    let flag_kind = score::ScoreNode::Integer(if connective_used { 1i64 } else { 0i64 });
    runtime_unattached_keeps_driver_control(before.score, flag_kind);
    let (after_flag, flag_handle) = bridge::bridge_append_unattached(
        before.score, flag_kind,
    ).unwrap();
    let children = seq![condition, if_true, if_false, flag_handle];
    runtime_constructor_keeps_driver_control(after_flag, 309i32, children);
    runtime_score_finish_control_transitive(before.score, after_flag, after);
    assert(runtime_score_finish_control(before.score, after));
    assert(bridge::bridge_same(after, after));
    assert(bridge::bridge_score_source_step(before.score, after));
    runtime_score_source_edge_exact(before.score, after);
    reveal(runtime_child_eof_gate);
}

pub open spec fn runtime_if_child_eof_accept(
    before: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    match runtime_if_finish(before) {
        None => None,
        Some(after_finish) => runtime_message_eof_child_accept(before, after_finish),
    }
}

pub proof fn runtime_if_child_eof_accept_refines(before: RuntimeFrameImage)
    requires runtime_if_child_eof_ready(before),
    ensures match runtime_if_child_eof_accept(before) {
        None => true,
        Some(after) => {
            let finish = runtime_if_finish(before).unwrap();
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == finish.admission.arena
            &&& after.score.transcript == finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    runtime_if_finish_discharges_child_gate(before);
    let after_finish = runtime_if_finish(before).unwrap();
    runtime_child_eof_completes_after_source_score(before, after_finish);
}

pub open spec fn runtime_if_parent_accepts(before: RuntimeFrameImage) -> bool {
    match runtime_if_finish(before) {
        None => false,
        Some(after_finish) => match admission::concrete_message_driver_step(
            after_finish.admission, admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: _ }) => true,
            _ => false,
        },
    }
}

pub proof fn runtime_if_child_eof_accepts_when_parent_accepts(before: RuntimeFrameImage)
    requires
        runtime_if_child_eof_ready(before),
        runtime_if_parent_accepts(before),
    ensures match runtime_if_child_eof_accept(before) {
        None => false,
        Some(after) => match admission::concrete_message_driver_step(
            runtime_if_finish(before).unwrap().admission,
            admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                &&& after.score.admission == state
                &&& after.score.transcript == runtime_if_finish(before).unwrap().transcript
                &&& after.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& after.score.admission.completed as int
                    == before.score.admission.completed as int + 1
            },
            _ => false,
        },
    },
{
    runtime_if_finish_discharges_child_gate(before);
    let after_finish = runtime_if_finish(before).unwrap();
    let pre_pop = RuntimeFrameImage { score: after_finish, ..before };
    reveal(runtime_child_eof_gate);
    assert(admission::admission_state_wf(pre_pop.score.admission));
    assert(bridge::bridge_well_formed(pre_pop.score));
    assert(pre_pop.score.admission.frames.len() == before.score.admission.frames.len());
    assert(runtime_frame_image_wf(pre_pop));
    reveal(runtime_message_eof_child_accept);
    reveal(runtime_pre_pop_child_accept);
    admission::concrete_message_driver_step_simulates(
        after_finish.admission, admission::ConcreteFrameActionModel::Complete,
    );
}

pub open spec fn runtime_match_finish(
    before: RuntimeFrameImage,
) -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_match_step(before.score) {
        None => None,
        Some((after, _, _, _)) => Some(after),
    }
}

pub open spec fn runtime_match_child_eof_ready(before: RuntimeFrameImage) -> bool {
    runtime_frame_image_wf(before)
        && before.score.admission.frames.len() > 1
        && before.score.admission.frames.last().frame_kind == 1
        && before.score.admission.frames.last().family == admission::AdmissionFamily::Match
        && before.score.admission.frames.last().waiting_id.is_none()
        && before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end
        && before.score.admission.frames.last().score_handle.is_none()
        && bridge::bridge_match_step(before.score).is_some()
}

pub proof fn runtime_match_finish_discharges_child_gate(before: RuntimeFrameImage)
    requires runtime_match_child_eof_ready(before),
    ensures match runtime_match_finish(before) {
        None => false,
        Some(after) => {
            &&& runtime_child_eof_gate(before, after)
            &&& after.admission.arena[after.admission.frames.last().score_handle.unwrap()
                as int].semantic
        },
    },
{
    let (after, _, _, _) = bridge::bridge_match_step(before.score).unwrap();
    bridge::bridge_match_exact(before.score);
    let top = before.score.admission.frames.last();
    let (target, cases, connective_used) = bridge::bridge_match_parts(
        top, before.score.admission.arena,
    ).unwrap();
    let flag_kind = score::ScoreNode::Integer(if connective_used { 1i64 } else { 0i64 });
    runtime_unattached_keeps_driver_control(before.score, flag_kind);
    let (after_flag, flag_handle) = bridge::bridge_append_unattached(
        before.score, flag_kind,
    ).unwrap();
    let children = seq![target] + cases + seq![flag_handle];
    runtime_constructor_keeps_driver_control(after_flag, 304i32, children);
    runtime_score_finish_control_transitive(before.score, after_flag, after);
    assert(runtime_score_finish_control(before.score, after));
    assert(bridge::bridge_same(after, after));
    assert(bridge::bridge_score_source_step(before.score, after));
    runtime_score_source_edge_exact(before.score, after);
    reveal(runtime_child_eof_gate);
}

pub open spec fn runtime_match_child_eof_accept(
    before: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    match runtime_match_finish(before) {
        None => None,
        Some(after_finish) => runtime_message_eof_child_accept(before, after_finish),
    }
}

pub proof fn runtime_match_child_eof_accept_refines(before: RuntimeFrameImage)
    requires runtime_match_child_eof_ready(before),
    ensures match runtime_match_child_eof_accept(before) {
        None => true,
        Some(after) => {
            let finish = runtime_match_finish(before).unwrap();
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == finish.admission.arena
            &&& after.score.transcript == finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    runtime_match_finish_discharges_child_gate(before);
    let after_finish = runtime_match_finish(before).unwrap();
    runtime_child_eof_completes_after_source_score(before, after_finish);
}

pub open spec fn runtime_match_parent_accepts(before: RuntimeFrameImage) -> bool {
    match runtime_match_finish(before) {
        None => false,
        Some(after_finish) => match admission::concrete_message_driver_step(
            after_finish.admission, admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: _ }) => true,
            _ => false,
        },
    }
}

pub proof fn runtime_match_child_eof_accepts_when_parent_accepts(before: RuntimeFrameImage)
    requires
        runtime_match_child_eof_ready(before),
        runtime_match_parent_accepts(before),
    ensures match runtime_match_child_eof_accept(before) {
        None => false,
        Some(after) => match admission::concrete_message_driver_step(
            runtime_match_finish(before).unwrap().admission,
            admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                &&& after.score.admission == state
                &&& after.score.transcript == runtime_match_finish(before).unwrap().transcript
                &&& after.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& after.score.admission.completed as int
                    == before.score.admission.completed as int + 1
            },
            _ => false,
        },
    },
{
    runtime_match_finish_discharges_child_gate(before);
    let after_finish = runtime_match_finish(before).unwrap();
    let pre_pop = RuntimeFrameImage { score: after_finish, ..before };
    reveal(runtime_child_eof_gate);
    assert(admission::admission_state_wf(pre_pop.score.admission));
    assert(bridge::bridge_well_formed(pre_pop.score));
    assert(pre_pop.score.admission.frames.len() == before.score.admission.frames.len());
    assert(runtime_frame_image_wf(pre_pop));
    reveal(runtime_message_eof_child_accept);
    reveal(runtime_pre_pop_child_accept);
    admission::concrete_message_driver_step_simulates(
        after_finish.admission, admission::ConcreteFrameActionModel::Complete,
    );
}

pub open spec fn runtime_nonsemantic_finish(
    before: RuntimeFrameImage,
) -> Option<bridge::ScoreAdmissionBridge> {
    match bridge::bridge_nonsemantic_step(before.score) {
        None => None,
        Some((after, _, _)) => Some(after),
    }
}

pub open spec fn runtime_nonsemantic_child_eof_ready(before: RuntimeFrameImage) -> bool {
    runtime_frame_image_wf(before)
        && before.score.admission.frames.len() > 1
        && before.score.admission.frames.last().frame_kind == 1
        && bridge::bridge_nonsemantic_family(before.score.admission.frames.last().family)
        && !admission::admission_family_complete(before.score.admission.frames.last())
        && before.score.admission.frames.last().waiting_id.is_none()
        && before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end
        && before.score.admission.frames.last().score_handle.is_none()
        && bridge::bridge_nonsemantic_step(before.score).is_some()
}

pub proof fn runtime_nonsemantic_finish_discharges_child_gate(before: RuntimeFrameImage)
    requires runtime_nonsemantic_child_eof_ready(before),
    ensures match runtime_nonsemantic_finish(before) {
        None => false,
        Some(after) => {
            let (_, constructor_handle, score_handle) =
                bridge::bridge_nonsemantic_step(before.score).unwrap();
            &&& runtime_child_eof_gate(before, after)
            &&& after.admission.frames.last().score_handle == Some(score_handle)
            &&& constructor_handle as int == before.score.transcript.entries.len()
            &&& score_handle as int == before.score.transcript.entries.len() + 1
            &&& score::handle_contents(after.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(i32::MAX as i64))
            &&& score::handle_contents(after.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle]))
            &&& after.transcript.entries.take(before.score.transcript.entries.len() as int)
                == before.score.transcript.entries
            &&& after.admission.arena.take(before.score.admission.arena.len() as int)
                == before.score.admission.arena
            &&& after.admission.arena[score_handle as int].transcript_bytes
                == bridge::bridge_constructor_transcript(
                    before.score, i32::MAX, Seq::empty(),
                )
            &&& !after.admission.arena[score_handle as int].semantic
        },
    },
{
    let (after, _, _) = bridge::bridge_nonsemantic_step(before.score).unwrap();
    bridge::bridge_nonsemantic_exact(before.score);
    bridge::bridge_constructor_exact(before.score, i32::MAX, Seq::empty());
    runtime_constructor_keeps_driver_control(before.score, i32::MAX, Seq::empty());
    assert(bridge::bridge_constructor_step(before.score, i32::MAX, Seq::empty())
        == bridge::bridge_nonsemantic_step(before.score));
    assert(runtime_score_finish_control(before.score, after));
    assert(bridge::bridge_same(after, after));
    assert(bridge::bridge_score_source_step(before.score, after));
    runtime_score_source_edge_exact(before.score, after);
    reveal(runtime_child_eof_gate);
}

pub open spec fn runtime_nonsemantic_child_eof_accept(
    before: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    match runtime_nonsemantic_finish(before) {
        None => None,
        Some(after_finish) => runtime_message_eof_child_accept(before, after_finish),
    }
}

pub proof fn runtime_nonsemantic_child_eof_accept_refines(before: RuntimeFrameImage)
    requires runtime_nonsemantic_child_eof_ready(before),
    ensures match runtime_nonsemantic_child_eof_accept(before) {
        None => true,
        Some(after) => {
            let finish = runtime_nonsemantic_finish(before).unwrap();
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == finish.admission.arena
            &&& after.score.transcript == finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    runtime_nonsemantic_finish_discharges_child_gate(before);
    let after_finish = runtime_nonsemantic_finish(before).unwrap();
    runtime_child_eof_completes_after_source_score(before, after_finish);
}

pub open spec fn runtime_nonsemantic_parent_accepts(before: RuntimeFrameImage) -> bool {
    match runtime_nonsemantic_finish(before) {
        None => false,
        Some(after_finish) => match admission::concrete_message_driver_step(
            after_finish.admission, admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: _ }) => true,
            _ => false,
        },
    }
}

pub proof fn runtime_nonsemantic_child_eof_accepts_when_parent_accepts(
    before: RuntimeFrameImage,
)
    requires
        runtime_nonsemantic_child_eof_ready(before),
        runtime_nonsemantic_parent_accepts(before),
    ensures match runtime_nonsemantic_child_eof_accept(before) {
        None => false,
        Some(after) => match admission::concrete_message_driver_step(
            runtime_nonsemantic_finish(before).unwrap().admission,
            admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                &&& after.score.admission == state
                &&& after.score.transcript
                    == runtime_nonsemantic_finish(before).unwrap().transcript
                &&& after.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& after.score.admission.completed as int
                    == before.score.admission.completed as int + 1
            },
            _ => false,
        },
    },
{
    runtime_nonsemantic_finish_discharges_child_gate(before);
    let after_finish = runtime_nonsemantic_finish(before).unwrap();
    let pre_pop = RuntimeFrameImage { score: after_finish, ..before };
    reveal(runtime_child_eof_gate);
    assert(admission::admission_state_wf(pre_pop.score.admission));
    assert(bridge::bridge_well_formed(pre_pop.score));
    assert(pre_pop.score.admission.frames.len() == before.score.admission.frames.len());
    assert(runtime_frame_image_wf(pre_pop));
    reveal(runtime_message_eof_child_accept);
    reveal(runtime_pre_pop_child_accept);
    admission::concrete_message_driver_step_simulates(
        after_finish.admission, admission::ConcreteFrameActionModel::Complete,
    );
}

pub open spec fn runtime_incomplete_send_finish(
    before: RuntimeFrameImage,
) -> Option<bridge::ScoreAdmissionBridge> {
    runtime_nonsemantic_finish(before)
}

pub open spec fn runtime_incomplete_send_child_eof_ready(before: RuntimeFrameImage) -> bool {
    runtime_nonsemantic_child_eof_ready(before)
        && before.score.admission.frames.last().family == admission::AdmissionFamily::Send
}

pub proof fn runtime_incomplete_send_finish_discharges_child_gate(before: RuntimeFrameImage)
    requires runtime_incomplete_send_child_eof_ready(before),
    ensures match runtime_incomplete_send_finish(before) {
        None => false,
        Some(after) => {
            &&& runtime_child_eof_gate(before, after)
            &&& !after.admission.arena[after.admission.frames.last().score_handle.unwrap()
                as int].semantic
        },
    },
{
    runtime_nonsemantic_finish_discharges_child_gate(before);
}

pub open spec fn runtime_incomplete_send_child_eof_accept(
    before: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    runtime_nonsemantic_child_eof_accept(before)
}

pub proof fn runtime_incomplete_send_child_eof_accept_refines(before: RuntimeFrameImage)
    requires runtime_incomplete_send_child_eof_ready(before),
    ensures match runtime_incomplete_send_child_eof_accept(before) {
        None => true,
        Some(after) => {
            let finish = runtime_incomplete_send_finish(before).unwrap();
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == finish.admission.arena
            &&& after.score.transcript == finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    runtime_nonsemantic_child_eof_accept_refines(before);
}

pub open spec fn runtime_incomplete_send_parent_accepts(before: RuntimeFrameImage) -> bool {
    runtime_nonsemantic_parent_accepts(before)
}

pub proof fn runtime_incomplete_send_child_eof_accepts_when_parent_accepts(
    before: RuntimeFrameImage,
)
    requires
        runtime_incomplete_send_child_eof_ready(before),
        runtime_incomplete_send_parent_accepts(before),
    ensures match runtime_incomplete_send_child_eof_accept(before) {
        None => false,
        Some(after) => match admission::concrete_message_driver_step(
            runtime_incomplete_send_finish(before).unwrap().admission,
            admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                &&& after.score.admission == state
                &&& after.score.transcript
                    == runtime_incomplete_send_finish(before).unwrap().transcript
                &&& after.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& after.score.admission.completed as int
                    == before.score.admission.completed as int + 1
            },
            _ => false,
        },
    },
{
    runtime_nonsemantic_child_eof_accepts_when_parent_accepts(before);
}

pub open spec fn runtime_incomplete_bundle_child_eof_ready(before: RuntimeFrameImage) -> bool {
    runtime_nonsemantic_child_eof_ready(before)
        && before.score.admission.frames.last().family == admission::AdmissionFamily::Bundle
}

pub proof fn runtime_incomplete_bundle_child_eof_uses_nonsemantic_finish(
    before: RuntimeFrameImage,
)
    requires runtime_incomplete_bundle_child_eof_ready(before),
    ensures match runtime_nonsemantic_finish(before) {
        None => false,
        Some(after_finish) => {
            &&& runtime_child_eof_gate(before, after_finish)
            &&& !after_finish.admission.arena[
                after_finish.admission.frames.last().score_handle.unwrap() as int
            ].semantic
            &&& match runtime_nonsemantic_child_eof_accept(before) {
                None => true,
                Some(after) => {
                    &&& after.score.admission.frames.len() + 1
                        == before.score.admission.frames.len()
                    &&& after.score.admission.completed as int
                        == before.score.admission.completed as int + 1
                    &&& after.score.admission.arena == after_finish.admission.arena
                    &&& after.score.transcript == after_finish.transcript
                },
            }
        },
    },
{
    runtime_nonsemantic_finish_discharges_child_gate(before);
    runtime_nonsemantic_child_eof_accept_refines(before);
}

pub proof fn runtime_incomplete_bundle_child_eof_accepts_when_parent_accepts(
    before: RuntimeFrameImage,
)
    requires
        runtime_incomplete_bundle_child_eof_ready(before),
        runtime_nonsemantic_parent_accepts(before),
    ensures match runtime_nonsemantic_child_eof_accept(before) {
        None => false,
        Some(after) => match admission::concrete_message_driver_step(
            runtime_nonsemantic_finish(before).unwrap().admission,
            admission::ConcreteFrameActionModel::Complete,
        ) {
            Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
                &&& after.score.admission == state
                &&& after.score.transcript == runtime_nonsemantic_finish(before).unwrap().transcript
                &&& after.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& after.score.admission.completed as int
                    == before.score.admission.completed as int + 1
            },
            _ => false,
        },
    },
{
    runtime_nonsemantic_child_eof_accepts_when_parent_accepts(before);
}

#[verifier::opaque]
pub open spec fn source_message_finish_ready(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
) -> bool {
    match source_message_finish_bridge(before, result) {
        None => false,
        Some(after_finish) => {
            &&& runtime_frame_image_wf(before)
            &&& before.score.admission.frames.len() > 1
            &&& before.score.admission.frames.last().frame_kind == 1
            &&& before.score.admission.frames.last().waiting_id.is_none()
            &&& before.score.admission.frames.last().cursor
                == before.score.admission.frames.last().region_end
            &&& before.score.admission.frames.last().score_handle.is_none()
            &&& runtime_score_source_edge(before.score, after_finish)
            &&& bridge::bridge_well_formed(after_finish)
            &&& admission::admission_state_wf(after_finish.admission)
        },
    }
}

pub proof fn source_message_finish_discharges_child_gate(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
)
    requires source_message_finish_ready(before, result),
    ensures match source_message_finish_bridge(before, result) {
        None => false,
        Some(after_finish) => runtime_child_eof_gate(before, after_finish),
    },
{
    reveal(source_message_finish_ready);
    source_message_finish_keeps_driver_control(before, result);
    reveal(runtime_child_eof_gate);
}

#[verifier::opaque]
pub open spec fn runtime_message_eof_child_accept(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
) -> Option<RuntimeFrameImage> {
    if !runtime_child_eof_gate(before, after_finish) {
        None
    } else {
        runtime_pre_pop_child_accept(RuntimeFrameImage {
            score: after_finish,
            ..before
        })
    }
}

pub open spec fn source_message_eof_child_accept(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
) -> Option<RuntimeFrameImage> {
    if !source_message_finish_ready(before, result) {
        None
    } else {
        match source_message_finish_bridge(before, result) {
            None => None,
            Some(after_finish) => runtime_message_eof_child_accept(before, after_finish),
        }
    }
}

#[verifier::opaque]
pub open spec fn source_noscore_message_finish_ready(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
) -> bool {
    if !runtime_frame_image_wf(before)
        || before.score.admission.frames.len() <= 1
        || before.score.admission.frames.last().frame_kind != 1
        || before.score.admission.frames.last().waiting_id.is_some()
        || before.score.admission.frames.last().cursor
            != before.score.admission.frames.last().region_end
        || before.score.admission.frames.last().score_handle.is_some()
        || source_message_finish_bridge(before, result) != Some(before.score)
    {
        false
    } else {
        let child = before.score.admission.frames.last();
        let parent = before.score.admission.frames[
            before.score.admission.frames.len() - 2
        ];
        match admission::concrete_derived_summary_with_arena(
            parent, child, before.score.admission.arena,
        ) {
            None => false,
            Some(kind) => !admission::admission_summary_has_score(kind),
        }
    }
}

pub open spec fn source_noscore_message_eof_child_accept(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
) -> Option<RuntimeFrameImage> {
    if !source_noscore_message_finish_ready(before, result) {
        None
    } else {
        runtime_pre_pop_child_accept(before)
    }
}

pub open spec fn runtime_noscore_child_kind(
    before: RuntimeFrameImage,
) -> Option<admission::AdmissionReturnedKind> {
    if !runtime_frame_image_wf(before)
        || before.score.admission.frames.len() <= 1
        || before.score.admission.frames.last().frame_kind != 1
        || before.score.admission.frames.last().waiting_id.is_some()
        || before.score.admission.frames.last().cursor
            != before.score.admission.frames.last().region_end
        || before.score.admission.frames.last().score_handle.is_some()
    {
        None
    } else {
        let child = before.score.admission.frames.last();
        let parent = before.score.admission.frames[
            before.score.admission.frames.len() - 2
        ];
        match (
            child.family, child.program_type_id,
            admission::concrete_derived_summary(parent, child),
        ) {
            (
                admission::AdmissionFamily::ReceiveBindFrame,
                16,
                Some(kind @ admission::AdmissionReturnedKind::ReceiveBind {
                    source_present: false,
                }),
            ) => if !admission::admission_family_complete(child) {
                Some(kind)
            } else { None },
            (
                admission::AdmissionFamily::MatchCaseFrame,
                21,
                Some(kind @ admission::AdmissionReturnedKind::MatchCase {
                    required_present: false, ..
                }),
            ) => if !admission::admission_family_complete(child) {
                Some(kind)
            } else { None },
            (
                admission::AdmissionFamily::BigRationalFrame, 25,
                Some(kind @ admission::AdmissionReturnedKind::BigRational { .. }),
            ) => Some(kind),
            (
                admission::AdmissionFamily::FixedPointFrame, 26,
                Some(kind @ admission::AdmissionReturnedKind::FixedPoint { .. }),
            ) => Some(kind),
            (
                admission::AdmissionFamily::Other, 7 | 64,
                Some(kind @ admission::AdmissionReturnedKind::Unit),
            ) => Some(kind),
            (
                admission::AdmissionFamily::Other, 56,
                Some(kind @ admission::AdmissionReturnedKind::VarRef { .. }),
            ) => Some(kind),
            (
                admission::AdmissionFamily::Other, 57,
                Some(kind @ admission::AdmissionReturnedKind::ConnectiveBody { .. }),
            ) => Some(kind),
            (
                admission::AdmissionFamily::Other, 61 | 62 | 63,
                Some(kind @ admission::AdmissionReturnedKind::Bytes { .. }),
            ) => Some(kind),
            (
                admission::AdmissionFamily::EVarFrame, 35,
                Some(kind @ admission::AdmissionReturnedKind::ExprPayload {
                    score_present: false, canonical_fixed_point: true,
                    canonical_path_stable: false,
                }),
            ) => if parent.family == admission::AdmissionFamily::Expr
                && parent.program_type_id == 24
                && parent.waiting_slot == Some(0)
                && parent.waiting_tag == Some(19)
                && !admission::admission_family_complete(child)
            {
                Some(kind)
            } else { None },
            (
                admission::AdmissionFamily::Other, program_type_id,
                Some(kind @ admission::AdmissionReturnedKind::ExprPayload {
                    score_present: false, canonical_fixed_point: true,
                    canonical_path_stable: false,
                }),
            ) => if program_type_id == 33
                && parent.family == admission::AdmissionFamily::Expr
                && parent.program_type_id == 24
                && parent.waiting_slot == Some(0)
                && parent.waiting_tag == Some(24)
                && !admission::admission_returned_par(child, 1)
                && admission::admission_expr_structural_payload(parent, child) == Some(kind)
            {
                Some(kind)
            } else if 36 <= program_type_id && program_type_id <= 54
                && parent.family == admission::AdmissionFamily::Expr
                && parent.program_type_id == 24
                && parent.waiting_slot == Some(0)
                && admission::admission_expr_structural_payload(parent, child) == Some(kind)
                && (!admission::admission_returned_par(child, 0)
                    || (bridge::bridge_expr_structural_constructor(program_type_id).is_some()
                        && bridge::bridge_expr_structural_constructor(program_type_id)
                            .unwrap().1
                        && !admission::admission_returned_par(child, 1)))
            {
                Some(kind)
            } else if program_type_id == 32
                && parent.family == admission::AdmissionFamily::Expr
                && parent.program_type_id == 24
                && parent.waiting_slot == Some(0)
                && parent.waiting_tag == Some(33)
                && admission::admission_expr_structural_payload(parent, child) == Some(kind)
            {
                Some(kind)
            } else { None },
            (
                admission::AdmissionFamily::Other, 34,
                Some(kind @ admission::AdmissionReturnedKind::KeyValuePair {
                    key, value_par,
                }),
            ) => if parent.family == admission::AdmissionFamily::Other
                && parent.program_type_id == 30
                && parent.frame_kind == 1
                && parent.waiting_slot == Some(0)
                && parent.waiting_tag == Some(1)
                && admission::admission_key_value_pair_bindings(
                    key, value_par, before.score.admission.arena,
                )
            {
                Some(kind)
            } else { None },
            (
                admission::AdmissionFamily::Other, 3 | 14 | 17,
                Some(kind @ admission::AdmissionReturnedKind::Other),
            ) => if admission::admission_opaque_child_edge(parent, child) {
                Some(kind)
            } else { None },
            (admission::AdmissionFamily::Other, 30, _) => match
                admission::concrete_derived_summary_with_arena(
                    parent, child, before.score.admission.arena,
                ) {
                Some(kind @ admission::AdmissionReturnedKind::ExprPayload {
                    score_present: false, ..
                }) => if parent.family == admission::AdmissionFamily::Expr
                    && parent.program_type_id == 24
                    && parent.waiting_slot == Some(0)
                    && parent.waiting_tag == Some(23)
                {
                    Some(kind)
                } else { None },
                _ => None,
            },
            _ => None,
        }
    }
}

pub open spec fn runtime_noscore_child_eof_accept(
    before: RuntimeFrameImage,
) -> Option<RuntimeFrameImage> {
    if runtime_noscore_child_kind(before).is_some() {
        runtime_pre_pop_child_accept(before)
    } else {
        None
    }
}

pub proof fn runtime_emap_ezipper_score_groups_quiet(before: bridge::ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 30
            || before.admission.frames.last().program_type_id == 32,
        bridge::bridge_emap_finish_step(before).is_none(),
    ensures
        bridge::bridge_score_finish_selection_group_1(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_2(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_3(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_4(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_5(
            before, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selected(before).is_none(),
{
    let empty = bridge::BridgeScoreFinishSelection::NoCandidate;
    assert(bridge::bridge_score_finish_selection_group_1(before, empty) == empty) by {
        assert(bridge::bridge_raw_par_step(before).is_none());
        assert(bridge::bridge_cost_atom_absent_step(before).is_none());
        assert(bridge::bridge_cost_atom_bytes_step(before).is_none());
        assert(bridge::bridge_cost_bound_step(before).is_none());
        assert(bridge::bridge_cost_atom_bound_forward_step(before).is_none());
        assert(bridge::bridge_cost_signature_step(before).is_none());
        assert(bridge::bridge_cost_region_step(before).is_none());
        assert(bridge::bridge_cost_token_stack_step(before).is_none());
        reveal(bridge::bridge_score_finish_selection_group_1);
    }
    assert(bridge::bridge_score_finish_selection_group_2(before, empty) == empty) by {
        assert(bridge::bridge_bundle_step(before).is_none());
        assert(bridge::bridge_if_step(before).is_none());
        assert(bridge::bridge_send_step(before).is_none());
        assert(bridge::bridge_match_step(before).is_none());
        assert(bridge::bridge_absent_specialized_constructor_step(before).is_none());
        assert(bridge::bridge_absent_connective_step(before).is_none());
        assert(bridge::bridge_receive_bind_step(before).is_none());
        assert(bridge::bridge_match_case_step(before).is_none());
        reveal(bridge::bridge_score_finish_selection_group_2);
    }
    assert(bridge::bridge_score_finish_selection_group_3(before, empty) == empty) by {
        assert(bridge::bridge_bool_oneof_step(before).is_none());
        assert(bridge::bridge_expr_atom_step(before).is_none());
        assert(bridge::bridge_expr_numeric_step(before).is_none());
        assert(bridge::bridge_expr_big_rational_step(before).is_none());
        assert(bridge::bridge_expr_fixed_point_step(before).is_none());
        assert(bridge::bridge_sys_auth_token_step(before).is_none());
        assert(bridge::bridge_new_empty_collections_step(before).is_none());
        assert(bridge::bridge_new_one_injection_step(before).is_none());
        reveal(bridge::bridge_score_finish_selection_group_3);
    }
    assert(bridge::bridge_score_finish_selection_group_4(before, empty) == empty) by {
        assert(bridge::bridge_new_one_absent_injection_step(before).is_none());
        assert(bridge::bridge_new_one_uri_step(before).is_none());
        assert(bridge::bridge_new_collections_step(before).is_none());
        assert(bridge::bridge_expr_payload_forward_step(before).is_none());
        assert(bridge::bridge_expr_structural_constructor(
            before.admission.frames.last().program_type_id,
        ).is_none());
        assert(bridge::bridge_expr_structural_step(before).is_none());
        assert(bridge::bridge_evar_step(before).is_none());
        assert(bridge::bridge_ordered_sequence_step(before).is_none());
        assert(bridge::bridge_emethod_step(before).is_none());
        reveal(bridge::bridge_score_finish_selection_group_4);
    }
    assert(bridge::bridge_score_finish_selection_group_5(before, empty) == empty) by {
        assert(bridge::bridge_connective_not_step(before).is_none());
        assert(bridge::bridge_connective_varref_step(before).is_none());
        assert(bridge::bridge_connective_collection_step(before).is_none());
        assert(bridge::bridge_unforgeable_bytes_step(before).is_none());
        assert(bridge::bridge_receive_present_condition_step(before).is_none());
        assert(bridge::bridge_receive_absent_condition_step(before).is_none());
        assert(bridge::bridge_nonsemantic_step(before).is_none());
        reveal(bridge::bridge_score_finish_selection_group_5);
    }
    reveal(bridge::bridge_score_finish_selected);
}

pub proof fn runtime_noscore_emap_ezipper_selected_quiet(before: RuntimeFrameImage)
    requires
        runtime_noscore_child_kind(before).is_some(),
        before.score.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.score.admission.frames.last().program_type_id == 30
            || before.score.admission.frames.last().program_type_id == 32,
    ensures
        bridge::bridge_emap_finish_step(before.score).is_none(),
        bridge::bridge_score_finish_selection_group_1(
            before.score, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_2(
            before.score, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_3(
            before.score, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_4(
            before.score, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selection_group_5(
            before.score, bridge::BridgeScoreFinishSelection::NoCandidate,
        ) == bridge::BridgeScoreFinishSelection::NoCandidate,
        bridge::bridge_score_finish_selected(before.score).is_none(),
{
    let child = before.score.admission.frames.last();
    let parent = before.score.admission.frames[before.score.admission.frames.len() - 2];
    let arena = before.score.admission.arena;
    if child.program_type_id == 30 {
        assert(parent.family == admission::AdmissionFamily::Expr);
        assert(parent.program_type_id == 24);
        assert(parent.waiting_slot == Some(0usize));
        assert(parent.waiting_tag == Some(23u32));
        assert(admission::concrete_derived_summary_with_arena(parent, child, arena)
            == admission::admission_expr_structural_payload_with_arena(parent, child, arena));
        assert(admission::admission_emap_source_summary(child, arena).is_some());
        assert(!admission::admission_emap_source_summary(child, arena).unwrap().0);
        bridge::bridge_emap_finish_source_complete_and_incomplete(before.score);
    }
    assert(bridge::bridge_emap_finish_step(before.score).is_none());
    runtime_emap_ezipper_score_groups_quiet(before.score);
}

pub proof fn runtime_noscore_child_selector_excluded(before: RuntimeFrameImage)
    ensures runtime_noscore_child_kind(before).is_some()
        ==> bridge::bridge_score_finish_selected(before.score).is_none(),
{
    if runtime_noscore_child_kind(before).is_some() {
        if bridge::bridge_no_score_child_frame(before.score) {
            bridge::bridge_no_score_child_finish_selected_none(before.score);
        } else if bridge::bridge_emethod_missing_target_frame(before.score) {
            bridge::bridge_emethod_missing_target_finish_selected_none(before.score);
        } else if bridge::bridge_expr_structural_missing_par_frame(before.score) {
            bridge::bridge_expr_structural_missing_par_finish_selected_none(before.score);
        } else if bridge::bridge_structural_no_score_child_frame(before.score) {
            bridge::bridge_structural_no_score_child_finish_selected_none(before.score);
        } else if before.score.admission.frames.last().family == admission::AdmissionFamily::Other
            && (before.score.admission.frames.last().program_type_id == 30
                || before.score.admission.frames.last().program_type_id == 32)
        {
            runtime_noscore_emap_ezipper_selected_quiet(before);
        } else {
            let child = before.score.admission.frames.last();
            if child.family == admission::AdmissionFamily::Other {
                assert(child.frame_kind == 1);
                assert(child.score_handle.is_none());
                assert(child.program_type_id == 3 || child.program_type_id == 14
                    || child.program_type_id == 17 || child.program_type_id == 34);
                bridge::bridge_other_summary_child_finish_selected_none(before.score);
            } else {
                assert(child.family == admission::AdmissionFamily::EVarFrame);
                assert(!admission::admission_returned_var(child, 0));
                bridge::bridge_evar_missing_var_finish_selected_none(before.score);
            }
        }
    }
}

pub struct RuntimeRootReturnImage {
    pub summary: admission::ConcreteRootSummaryModel,
    pub ghost pre_pop: RuntimeFrameImage,
    pub ghost score_bytes: Seq<u8>,
    pub remaining_frames: usize,
}

pub open spec fn runtime_message_eof_root_return(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
) -> Option<RuntimeRootReturnImage> {
    if !runtime_frame_image_wf(before) {
        None
    } else {
        match finish::modeled_message_eof_to_root_return(before.score, after_finish) {
            None => None,
            Some(returned) => {
                match admission::concrete_message_driver_step(
                    after_finish.admission, admission::ConcreteFrameActionModel::Complete,
                ) {
                    Some(admission::ConcreteFrameOutcomeModel::RootFinished { summary }) => {
                        Some(RuntimeRootReturnImage {
                            summary,
                            pre_pop: RuntimeFrameImage { score: after_finish, ..before },
                            score_bytes: returned.score_bytes,
                            remaining_frames: returned.remaining_frames,
                        })
                    },
                    _ => None,
                }
            },
        }
    }
}

pub enum SourceMessageFinishRoute {
    Child {
        kind: admission::AdmissionReturnedKind,
        image: RuntimeFrameImage,
    },
    Root {
        returned: RuntimeRootReturnImage,
    },
}

pub open spec fn source_message_finish_route(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
) -> Option<SourceMessageFinishRoute> {
    match source_message_finish_bridge(before, result) {
        None => None,
        Some(after_finish) => {
            if before.score.admission.frames.len() == 1 {
                match runtime_message_eof_root_return(before, after_finish) {
                    None => None,
                    Some(returned) => Some(SourceMessageFinishRoute::Root { returned }),
                }
            } else if after_finish.admission.frames.len() > 1 {
                let accepted = match source_message_eof_child_accept(before, result) {
                    Some(image) => Some(image),
                    None => source_noscore_message_eof_child_accept(before, result),
                };
                match accepted {
                    None => None,
                    Some(image) => {
                        let child = after_finish.admission.frames.last();
                        let parent = after_finish.admission.frames[
                            after_finish.admission.frames.len() - 2
                        ];
                        match admission::concrete_derived_summary_with_arena(
                            parent, child, after_finish.admission.arena,
                        ) {
                            None => None,
                            Some(kind) => Some(SourceMessageFinishRoute::Child { kind, image }),
                        }
                    },
                }
            } else {
                None
            }
        },
    }
}

pub enum RuntimeMessageEvent {
    FieldContinue { field: admission::IndexedCanonicalFieldModel },
    FieldPush {
        field: admission::IndexedCanonicalFieldModel,
        child: admission::AdmissionFrameState,
        observed_capacity: usize,
    },
    Eof,
    EofEPathMap { ghost snapshot_bytes: Seq<u8> },
}

pub enum RuntimeMessageOutcome {
    Continued { image: RuntimeFrameImage },
    Pushed { image: RuntimeFrameImage },
    ParentAccepted { image: RuntimeFrameImage },
    RootReturned { returned: RuntimeRootReturnImage },
}

pub open spec fn runtime_epm_source_score_finish_selected(
    before: RuntimeFrameImage, snapshot_bytes: Seq<u8>,
) -> Option<bridge::ScoreAdmissionBridge> {
    if !runtime_frame_image_wf(before)
        || before.score.admission.frames.len() == 0
        || before.score.admission.frames.last().frame_kind != 1
        || before.score.admission.frames.last().family != admission::AdmissionFamily::Other
        || before.score.admission.frames.last().program_type_id != 31
        || before.score.admission.frames.last().waiting_id.is_some()
        || before.score.admission.frames.last().score_handle.is_some()
        || before.score.admission.frames.last().cursor
            != before.score.admission.frames.last().region_end
        || !admission::admission_family_complete(before.score.admission.frames.last())
        || before.score.admission.frames.last().returns.len() == 0
        || before.score.admission.frames.last().returns.last().body_bytes != snapshot_bytes
        || match before.score.admission.frames.last().returns.last().kind {
            admission::AdmissionReturnedKind::EPathMapSnapshot { .. } => false,
            _ => true,
        }
    {
        None
    } else {
        match bridge::bridge_bool_slot(before.score.admission.frames.last(), 1, 4) {
            None => None,
            Some(connective_used) => {
                let flag = if connective_used { 1i64 } else { 0i64 };
                match bridge::bridge_append_unattached(
                    before.score, score::ScoreNode::BytesAtom(snapshot_bytes),
                ) {
                    None => None,
                    Some((after_bytes, bytes_handle)) =>
                        match bridge::bridge_append_unattached(
                            after_bytes, score::ScoreNode::Integer(flag),
                        ) {
                            None => None,
                            Some((after_flag, flag_handle)) =>
                                match bridge::bridge_constructor_step(
                                    after_flag, 13i32,
                                    seq![bytes_handle, flag_handle],
                                ) {
                                    None => None,
                                    Some((next, _, _)) => Some(next),
                                },
                        },
                }
            },
        }
    }
}

pub proof fn runtime_epm_source_score_finish_is_checked(
    before: RuntimeFrameImage, snapshot_bytes: Seq<u8>,
)
    ensures match runtime_epm_source_score_finish_selected(before, snapshot_bytes) {
        None => true,
        Some(next) => {
            &&& runtime_frame_image_wf(RuntimeFrameImage { score: next, ..before })
            &&& bridge::bridge_score_step_shape(before.score, next)
            &&& next.admission.frames.last().score_handle.is_some()
            &&& before.score.admission.frames.last().returns.last().body_bytes
                == snapshot_bytes
        },
    },
{
    if let Some(next) = runtime_epm_source_score_finish_selected(before, snapshot_bytes) {
        let flag = if bridge::bridge_bool_slot(
            before.score.admission.frames.last(), 1, 4,
        ).unwrap() { 1i64 } else { 0i64 };
        let bytes_kind = score::ScoreNode::BytesAtom(snapshot_bytes);
        assert(runtime_frame_image_wf(before));
        bridge::bridge_append_unattached_exact(before.score, bytes_kind);
        bridge::bridge_append_unattached_preserves_admission_wf(before.score, bytes_kind);
        bridge::bridge_append_unattached_shape(before.score, bytes_kind);
        let (after_bytes, bytes_handle) = bridge::bridge_append_unattached(
            before.score, bytes_kind,
        ).unwrap();
        let flag_kind = score::ScoreNode::Integer(flag);
        bridge::bridge_append_unattached_exact(after_bytes, flag_kind);
        bridge::bridge_append_unattached_preserves_admission_wf(after_bytes, flag_kind);
        bridge::bridge_append_unattached_shape(after_bytes, flag_kind);
        let (after_flag, flag_handle) = bridge::bridge_append_unattached(
            after_bytes, flag_kind,
        ).unwrap();
        bridge::bridge_constructor_exact(
            after_flag, 13i32, seq![bytes_handle, flag_handle],
        );
        bridge::bridge_constructor_shape(
            after_flag, 13i32, seq![bytes_handle, flag_handle],
        );
        let (constructed, _, score_handle) = bridge::bridge_constructor_step(
            after_flag, 13i32, seq![bytes_handle, flag_handle],
        ).unwrap();
        bridge::bridge_score_step_shape_transitive(
            before.score, after_bytes, after_flag,
        );
        bridge::bridge_score_step_shape_transitive(
            before.score, after_flag, constructed,
        );
        assert(next == constructed);
        assert(next.admission.frames.last().score_handle == Some(score_handle));
        assert(runtime_frame_image_wf(RuntimeFrameImage { score: next, ..before }));
    }
}

pub open spec fn runtime_epm_source_eof(
    before: RuntimeFrameImage, snapshot_bytes: Seq<u8>,
) -> Option<RuntimeMessageOutcome> {
    match runtime_epm_source_score_finish_selected(before, snapshot_bytes) {
        None => None,
        Some(after_finish) => {
            if before.score.admission.frames.len() == 1 {
                match admission::concrete_message_driver_step(
                    after_finish.admission,
                    admission::ConcreteFrameActionModel::Complete,
                ) {
                    Some(admission::ConcreteFrameOutcomeModel::RootFinished { summary }) => {
                        if (summary.score_handle as int) >= after_finish.admission.arena.len() {
                            None
                        } else {
                            Some(RuntimeMessageOutcome::RootReturned {
                                returned: RuntimeRootReturnImage {
                                    summary,
                                    pre_pop: RuntimeFrameImage { score: after_finish, ..before },
                                    score_bytes: after_finish.admission.arena[
                                        summary.score_handle as int
                                    ].transcript_bytes,
                                    remaining_frames: 0,
                                },
                            })
                        }
                    },
                    _ => None,
                }
            } else {
                match runtime_pre_pop_child_accept(RuntimeFrameImage {
                    score: after_finish, ..before
                }) {
                    None => None,
                    Some(image) => Some(RuntimeMessageOutcome::ParentAccepted { image }),
                }
            }
        },
    }
}

pub open spec fn runtime_message_driver_step(
    before: RuntimeFrameImage, event: RuntimeMessageEvent,
) -> Option<RuntimeMessageOutcome> {
    match event {
        RuntimeMessageEvent::FieldContinue { field } => {
            match runtime_continue(before, field) {
                None => None,
                Some(image) => Some(RuntimeMessageOutcome::Continued { image }),
            }
        },
        RuntimeMessageEvent::FieldPush { field, child, observed_capacity } => {
            match runtime_push(before, field, child, observed_capacity) {
                None => None,
                Some(image) => Some(RuntimeMessageOutcome::Pushed { image }),
            }
        },
        RuntimeMessageEvent::Eof => {
            if runtime_noscore_child_kind(before).is_some() {
                match runtime_noscore_child_eof_accept(before) {
                    None => None,
                    Some(image) => Some(RuntimeMessageOutcome::ParentAccepted { image }),
                }
            } else {
                match bridge::bridge_score_finish_selected(before.score) {
                    None => None,
                    Some(after_finish) => {
                        if before.score.admission.frames.len() == 1 {
                            match runtime_message_eof_root_return(before, after_finish) {
                                None => None,
                                Some(returned) => Some(RuntimeMessageOutcome::RootReturned { returned }),
                            }
                        } else {
                            match runtime_message_eof_child_accept(before, after_finish) {
                                None => None,
                                Some(image) => Some(RuntimeMessageOutcome::ParentAccepted { image }),
                            }
                        }
                    },
                }
            }
        },
        RuntimeMessageEvent::EofEPathMap { snapshot_bytes } =>
            runtime_epm_source_eof(before, snapshot_bytes),
    }
}

pub proof fn runtime_selected_score_finish_is_checked(before: RuntimeFrameImage)
    requires runtime_frame_image_wf(before),
    ensures match bridge::bridge_score_finish_selected(before.score) {
        None => true,
        Some(after_finish) => {
            &&& runtime_score_source_edge(before.score, after_finish)
            &&& bridge::bridge_well_formed(after_finish)
            &&& admission::admission_state_wf(after_finish.admission)
            &&& bridge::bridge_score_step_shape(before.score, after_finish)
            &&& after_finish.admission.frames.len()
                == before.score.admission.frames.len()
            &&& after_finish.transcript.entries.take(
                before.score.transcript.entries.len() as int,
            ) == before.score.transcript.entries
            &&& after_finish.admission.arena.take(
                before.score.admission.arena.len() as int,
            ) == before.score.admission.arena
        },
    },
{
    bridge::bridge_score_finish_selected_preserves_checked(before.score);
    if let Some(after_finish) = bridge::bridge_score_finish_selected(before.score) {
        runtime_score_source_edge_exact(before.score, after_finish);
    }
}

pub proof fn runtime_message_driver_control_split(
    before: RuntimeFrameImage, event: RuntimeMessageEvent,
)
    ensures match runtime_message_driver_step(before, event) {
        None => true,
        Some(RuntimeMessageOutcome::Continued { image }) => {
            &&& image.score.admission.frames.len()
                == before.score.admission.frames.len()
            &&& image.score.transcript == before.score.transcript
        },
        Some(RuntimeMessageOutcome::Pushed { image }) => {
            &&& image.score.admission.frames.len()
                == before.score.admission.frames.len() + 1
            &&& image.score.transcript == before.score.transcript
        },
        Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
            &&& before.score.admission.frames.len() > 1
            &&& image.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
        },
        Some(RuntimeMessageOutcome::RootReturned { returned }) => {
            &&& before.score.admission.frames.len() == 1
            &&& returned.remaining_frames == 0
            &&& returned.pre_pop.score.admission.frames.len() == 1
        },
    },
{
    match event {
        RuntimeMessageEvent::FieldContinue { field } => {
            runtime_continue_retains_stack_and_score(before, field);
        },
        RuntimeMessageEvent::FieldPush { field, child, observed_capacity } => {
            runtime_push_checked_storage(before, field, child, observed_capacity);
        },
        RuntimeMessageEvent::Eof => {
            if runtime_noscore_child_kind(before).is_some() {
                runtime_noscore_child_eof_is_checked(before);
            } else if let Some(after_finish) = bridge::bridge_score_finish_selected(before.score) {
                if before.score.admission.frames.len() == 1 {
                    runtime_root_return_is_pre_pop_exact(before, after_finish);
                } else {
                    runtime_child_eof_completes_after_source_score(before, after_finish);
                }
            }
        },
        RuntimeMessageEvent::EofEPathMap { snapshot_bytes } => {
            if let Some(after_finish) = runtime_epm_source_score_finish_selected(
                before, snapshot_bytes,
            ) {
                if before.score.admission.frames.len() == 1 {
                    admission::concrete_message_driver_step_simulates(
                        after_finish.admission,
                        admission::ConcreteFrameActionModel::Complete,
                    );
                } else {
                    runtime_child_accept_is_derived(RuntimeFrameImage {
                        score: after_finish, ..before
                    });
                }
            }
        },
    }
}

pub proof fn runtime_selected_eof_outcome_is_checked(before: RuntimeFrameImage)
    requires runtime_frame_image_wf(before),
    ensures match runtime_message_driver_step(before, RuntimeMessageEvent::Eof) {
        None => true,
        Some(RuntimeMessageOutcome::ParentAccepted { image }) =>
            if runtime_noscore_child_kind(before).is_some() {
                &&& before.score.admission.frames.len() > 1
                &&& bridge::bridge_score_finish_selected(before.score).is_none()
                &&& !admission::admission_summary_has_score(
                    runtime_noscore_child_kind(before).unwrap(),
                )
                &&& image.score.admission.frames.len() + 1
                    == before.score.admission.frames.len()
                &&& image.score.admission.completed as int
                    == before.score.admission.completed as int + 1
                &&& image.score.admission.arena == before.score.admission.arena
                &&& image.score.transcript == before.score.transcript
                &&& image.score.admission.frames[image.score.admission.frames.len() - 1]
                    .waiting_id.is_none()
                &&& image.score.admission.frames[image.score.admission.frames.len() - 1]
                    .returns.last().kind == runtime_noscore_child_kind(before).unwrap()
                &&& image.score.admission.frames[image.score.admission.frames.len() - 1]
                    .returns.last().score_handle.is_none()
            } else {
                match bridge::bridge_score_finish_selected(before.score) {
                    None => false,
                    Some(after_finish) => {
                        &&& before.score.admission.frames.len() > 1
                        &&& runtime_score_source_edge(before.score, after_finish)
                        &&& runtime_child_eof_gate(before, after_finish)
                        &&& image.score.admission.frames.len() + 1
                            == before.score.admission.frames.len()
                        &&& image.score.admission.completed as int
                            == before.score.admission.completed as int + 1
                        &&& image.score.admission.arena == after_finish.admission.arena
                        &&& image.score.transcript == after_finish.transcript
                        &&& image.score.admission.frames[image.score.admission.frames.len() - 1]
                            .waiting_id.is_none()
                    },
                }
            },
        Some(RuntimeMessageOutcome::RootReturned { returned }) =>
            match bridge::bridge_score_finish_selected(before.score) {
                None => false,
                Some(after_finish) => {
                    &&& before.score.admission.frames.len() == 1
                    &&& runtime_score_source_edge(before.score, after_finish)
                    &&& returned.remaining_frames == 0
                    &&& returned.pre_pop.score == after_finish
                    &&& returned.pre_pop.score.admission.frames.len() == 1
                    &&& returned.summary.score_handle
                        == after_finish.admission.frames[0].score_handle.unwrap()
                    &&& returned.summary.family == after_finish.admission.frames[0].family
                    &&& returned.summary.complete
                        == admission::admission_family_complete(after_finish.admission.frames[0])
                    &&& returned.summary.body_bytes
                        == after_finish.admission.frames[0].source_bytes
                    &&& returned.score_bytes
                        == after_finish.admission.arena[
                            returned.summary.score_handle as int].transcript_bytes
                },
            },
        _ => false,
    },
{
    runtime_selected_score_finish_is_checked(before);
    if runtime_noscore_child_kind(before).is_some() {
        runtime_noscore_child_eof_is_checked(before);
    } else if let Some(after_finish) = bridge::bridge_score_finish_selected(before.score) {
        if before.score.admission.frames.len() == 1 {
            runtime_root_return_is_pre_pop_exact(before, after_finish);
        } else {
            runtime_child_eof_completes_after_source_score(before, after_finish);
            reveal(runtime_message_eof_child_accept);
            if runtime_message_eof_child_accept(before, after_finish).is_some() {
                assert(runtime_child_eof_gate(before, after_finish));
            }
        }
    }
}

pub proof fn runtime_push_checked_storage(
    before: RuntimeFrameImage, field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState, observed_capacity: usize,
)
    ensures match runtime_push(before, field, child, observed_capacity) {
        None => true,
        Some(after) => {
            &&& runtime_frame_image_wf(after)
            &&& after.score.admission.frames.len()
                == before.score.admission.frames.len() + 1
            &&& after.score.admission.pushed as int
                == before.score.admission.pushed as int + 1
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
            &&& after.capacity == observed_capacity
        },
    },
{
    admission::concrete_message_driver_step_simulates(
        before.score.admission,
        admission::ConcreteFrameActionModel::Push {
            field, child, frame_bytes: before.frame_bytes,
        },
    );
    if runtime_frame_image_wf(before) {
        admission::concrete_message_driver_preserves_wf_and_arena(
            before.score.admission,
            admission::ConcreteFrameActionModel::Push {
                field, child, frame_bytes: before.frame_bytes,
            },
        );
    }
    if runtime_push(before, field, child, observed_capacity).is_some() {
        let after = runtime_push(before, field, child, observed_capacity).unwrap();
        let next = after.score.admission;
        let action = admission::ConcreteFrameActionModel::Push {
            field, child, frame_bytes: before.frame_bytes,
        };
        assert(match admission::concrete_message_driver_step(before.score.admission, action) {
            Some(admission::ConcreteFrameOutcomeModel::Pushed { state }) => state == next,
            _ => false,
        });
        assert(runtime_frame_image_wf(before));
        assert(admission::admission_state_wf(next));
        assert(after.score.admission.arena == before.score.admission.arena);
        assert(after.score.transcript == before.score.transcript);
        assert(after.score.admission.frames.len()
            == before.score.admission.frames.len() + 1);
        assert(after.score.admission.pushed as int
            == before.score.admission.pushed as int + 1);
        assert(after.capacity == observed_capacity);
        assert(after.frame_bytes == before.frame_bytes);
        assert(next.frames.len() <= observed_capacity as int);
        assert(observed_capacity as int * before.frame_bytes as int
            <= usize::MAX as int);
        let used = next.frames.len() * before.frame_bytes as int;
        let reserved = observed_capacity as int * before.frame_bytes as int;
        assert(0 <= used);
        assert(next.frames.len() * before.frame_bytes as int
            <= observed_capacity as int * before.frame_bytes as int) by(nonlinear_arith)
            requires
                next.frames.len() <= observed_capacity as int,
                0 <= before.frame_bytes as int,
        {}
        assert(used <= reserved);
        assert(reserved <= usize::MAX as int);
        assert(used as usize as int == used);
        assert(reserved as usize as int == reserved);
        assert(after.score.transcript.entries.len()
            == before.score.transcript.entries.len());
        assert forall|i: int| 0 <= i < next.arena.len()
            implies next.arena[i].encoded_len
                == after.score.transcript.entries[i].encoded_len
                && next.arena[i].transcript_bytes
                    == after.score.transcript.entries[i].transcript by {}
        assert(bridge::bridge_well_formed(after.score));
        assert(after.max_frames as int >= next.frames.len());
        assert(after.max_storage_bytes as int
            >= next.frames.len() * after.frame_bytes as int);
        assert(after.max_capacity_bytes as int
            >= observed_capacity as int * after.frame_bytes as int);
        assert(runtime_frame_image_wf(after));
    }
}

pub proof fn runtime_push_indexed_checked_storage(
    before: RuntimeFrameImage,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    observed_capacity: usize,
)
    ensures match runtime_push_indexed(
        before, field, child, parent_source, observed_capacity,
    ) {
        None => true,
        Some(after) => {
            &&& runtime_frame_image_wf(after)
            &&& after.score.admission.frames.len()
                == before.score.admission.frames.len() + 1
            &&& after.score.admission.pushed as int
                == before.score.admission.pushed as int + 1
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
            &&& after.capacity == observed_capacity
        },
    },
{
    let action = admission::ConcreteFrameActionModel::PushIndexed {
        field, child, parent_source, frame_bytes: before.frame_bytes,
    };
    admission::concrete_message_driver_step_simulates(before.score.admission, action);
    if runtime_frame_image_wf(before) {
        admission::concrete_message_driver_preserves_wf_and_arena(
            before.score.admission, action,
        );
    }
    if let Some(after) = runtime_push_indexed(
        before, field, child, parent_source, observed_capacity,
    ) {
        let next = after.score.admission;
        assert(match admission::concrete_message_driver_step(before.score.admission, action) {
            Some(admission::ConcreteFrameOutcomeModel::Pushed { state }) => state == next,
            _ => false,
        });
        assert(runtime_frame_image_wf(before));
        assert(admission::admission_state_wf(next));
        assert(next.frames.len() <= observed_capacity as int);
        assert(observed_capacity as int * before.frame_bytes as int
            <= usize::MAX as int);
        let used = next.frames.len() * before.frame_bytes as int;
        let reserved = observed_capacity as int * before.frame_bytes as int;
        assert(next.frames.len() * before.frame_bytes as int
            <= observed_capacity as int * before.frame_bytes as int) by(nonlinear_arith)
            requires
                next.frames.len() <= observed_capacity as int,
                0 <= before.frame_bytes as int,
        {}
        assert(used <= reserved);
        assert(used as usize as int == used);
        assert(reserved as usize as int == reserved);
        assert forall|i: int| 0 <= i < next.arena.len()
            implies next.arena[i].encoded_len
                == after.score.transcript.entries[i].encoded_len
                && next.arena[i].transcript_bytes
                    == after.score.transcript.entries[i].transcript by {}
        assert(bridge::bridge_well_formed(after.score));
        assert(runtime_frame_image_wf(after));
    }
}

pub proof fn source_checked_frame_request_push_indexed_refines(
    before: RuntimeFrameImage,
    request: SourceFrameRequestImage,
    parent_source: admission::AdmissionIndexedSourceImage,
    storage: SourceFramePushStorageImage,
)
    ensures match source_checked_frame_request_push_indexed(
        before, request, parent_source, storage,
    ) {
        None => true,
        Some(after) => {
            &&& source_frame_request_matches_child(request)
            &&& source_successful_push_storage(before, storage)
            &&& runtime_frame_image_wf(after)
            &&& after.score.admission.frames.len() == storage.after_len as int
            &&& after.capacity == storage.after_capacity
            &&& after.frame_bytes == storage.frame_variant_bytes
            &&& after.max_storage_bytes >= storage.used_bytes
            &&& after.max_capacity_bytes >= storage.reserved_bytes
            &&& after.score.admission.pushed as int
                == before.score.admission.pushed as int + 1
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
        },
    },
{
    runtime_push_indexed_checked_storage(
        before, request.field, request.child, parent_source,
        storage.after_capacity,
    );
    if let Some(after) = source_checked_frame_request_push_indexed(
        before, request, parent_source, storage,
    ) {
        assert(source_successful_push_storage(before, storage));
        assert(after.score.admission.frames.len() == storage.after_len as int);
        assert(after.frame_bytes == storage.frame_variant_bytes);
        assert(storage.used_bytes as int
            == after.score.admission.frames.len() * after.frame_bytes as int);
        assert(storage.reserved_bytes as int
            == after.capacity as int * after.frame_bytes as int);
        assert(after.max_storage_bytes >= storage.used_bytes);
        assert(after.max_capacity_bytes >= storage.reserved_bytes);
    }
}

pub proof fn source_checked_frame_request_push_refines(
    before: RuntimeFrameImage, request: SourceFrameRequestImage,
    storage: SourceFramePushStorageImage,
)
    ensures match source_checked_frame_request_push(before, request, storage) {
        None => true,
        Some(after) => {
            &&& source_frame_request_matches_child(request)
            &&& source_successful_push_storage(before, storage)
            &&& runtime_frame_image_wf(after)
            &&& after.score.admission.frames.len() == storage.after_len as int
            &&& after.capacity == storage.after_capacity
            &&& after.frame_bytes == storage.frame_variant_bytes
            &&& after.max_storage_bytes >= storage.used_bytes
            &&& after.max_capacity_bytes >= storage.reserved_bytes
            &&& after.score.admission.pushed as int
                == before.score.admission.pushed as int + 1
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
        },
    },
{
    runtime_push_checked_storage(
        before, request.field, request.child, storage.after_capacity,
    );
    if let Some(after) = source_checked_frame_request_push(before, request, storage) {
        assert(source_successful_push_storage(before, storage));
        assert(after.score.admission.frames.len() == storage.after_len as int);
        assert(after.frame_bytes == storage.frame_variant_bytes);
        assert(storage.used_bytes as int
            == after.score.admission.frames.len() * after.frame_bytes as int);
        assert(storage.reserved_bytes as int
            == after.capacity as int * after.frame_bytes as int);
        assert(after.max_storage_bytes >= storage.used_bytes);
        assert(after.max_capacity_bytes >= storage.reserved_bytes);
    }
}

pub proof fn source_checked_frame_request_push_fresh_refines(
    before: RuntimeFrameImage,
    request: SourceFrameRequestImage,
    storage: SourceFramePushStorageImage,
    observed: SourceFreshFrameObservation,
)
    ensures match source_checked_frame_request_push_fresh(
        before, request, storage, observed,
    ) {
        None => true,
        Some(after) => {
            &&& source_fresh_frame_matches(
                request, after.score.admission.frames.last(), observed,
            )
            &&& after.score.admission.frames.last() == observed.observed_frame
            &&& runtime_frame_image_wf(after)
            &&& after.score.admission.frames.len() == storage.after_len as int
        },
    },
{
    source_checked_frame_request_push_refines(before, request, storage);
}

pub proof fn source_checked_frame_request_push_indexed_fresh_refines(
    before: RuntimeFrameImage,
    request: SourceFrameRequestImage,
    parent_source: admission::AdmissionIndexedSourceImage,
    storage: SourceFramePushStorageImage,
    observed: SourceFreshFrameObservation,
)
    ensures match source_checked_frame_request_push_indexed_fresh(
        before, request, parent_source, storage, observed,
    ) {
        None => true,
        Some(after) => {
            &&& source_indexed_child_selected_view_matches(
                before, request, parent_source, observed.selected,
            )
            &&& source_fresh_frame_matches(
                request, after.score.admission.frames.last(), observed,
            )
            &&& after.score.admission.frames.last() == observed.observed_frame
            &&& runtime_frame_image_wf(after)
            &&& after.score.admission.frames.len() == storage.after_len as int
        },
    },
{
    source_checked_frame_request_push_indexed_refines(
        before, request, parent_source, storage,
    );
}

pub proof fn source_checked_fresh_root_refines(
    request: SourceFrameRequestImage,
    observed: SourceFreshRootObservation,
)
    ensures match source_checked_fresh_root(request, observed) {
        None => true,
        Some(state) => {
            &&& source_fresh_root_matches(request, observed)
            &&& state.frames.len() == 1
            &&& state.frames[0] == observed.frame.observed_frame
            &&& state.arena.len() == 0
            &&& state.pushed == 1
            &&& state.completed == 0
        },
    },
{}

pub proof fn source_initial_indexed_cursor_rejects_consumed(
    cursor: SourceInitialIndexedCursor,
    selected: SourceSelectedFrameView,
    enforce_numeric_order: bool,
)
    requires cursor.fields_read > 0 || cursor.reader_bytes_traversed > 0,
    ensures !source_initial_indexed_cursor_matches(
        cursor, selected, enforce_numeric_order,
    ),
{}

pub proof fn runtime_continue_retains_stack_and_score(
    before: RuntimeFrameImage, field: admission::IndexedCanonicalFieldModel,
)
    ensures match runtime_continue(before, field) {
        None => true,
        Some(after) => {
            &&& after.score.admission.frames.len()
                == before.score.admission.frames.len()
            &&& after.score.admission.completed
                == before.score.admission.completed
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
            &&& after.capacity == before.capacity
        },
    },
{
    admission::concrete_message_driver_step_simulates(
        before.score.admission,
        admission::ConcreteFrameActionModel::Continue { field },
    );
    if admission::admission_state_wf(before.score.admission) {
        admission::concrete_message_driver_preserves_wf_and_arena(
            before.score.admission,
            admission::ConcreteFrameActionModel::Continue { field },
        );
    }
}

pub proof fn runtime_continue_indexed_retains_stack_and_score(
    before: RuntimeFrameImage, field: admission::IndexedCanonicalFieldModel,
    parent_source: admission::AdmissionIndexedSourceImage,
)
    ensures match runtime_continue_indexed(before, field, parent_source) {
        None => true,
        Some(after) => {
            &&& after.score.admission.frames.len()
                == before.score.admission.frames.len()
            &&& after.score.admission.completed
                == before.score.admission.completed
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
            &&& after.capacity == before.capacity
        },
    },
{
    admission::concrete_message_driver_step_simulates(
        before.score.admission,
        admission::ConcreteFrameActionModel::ContinueIndexed {
            field, parent_source,
        },
    );
    if admission::admission_state_wf(before.score.admission) {
        admission::concrete_message_driver_preserves_wf_and_arena(
            before.score.admission,
            admission::ConcreteFrameActionModel::ContinueIndexed {
                field, parent_source,
            },
        );
    }
}

pub proof fn runtime_child_accept_is_derived(pre_pop: RuntimeFrameImage)
    ensures match runtime_pre_pop_child_accept(pre_pop) {
        None => true,
        Some(after) => {
            &&& after.score.admission.frames.len() + 1
                == pre_pop.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == pre_pop.score.admission.completed as int + 1
            &&& after.score.admission.arena == pre_pop.score.admission.arena
            &&& after.score.transcript == pre_pop.score.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    reveal(runtime_pre_pop_child_accept);
    admission::concrete_message_driver_step_simulates(
        pre_pop.score.admission,
        admission::ConcreteFrameActionModel::Complete,
    );
    if runtime_frame_image_wf(pre_pop) {
        admission::concrete_message_driver_preserves_wf_and_arena(
            pre_pop.score.admission,
            admission::ConcreteFrameActionModel::Complete,
        );
    }
}

pub proof fn runtime_noscore_child_eof_is_checked(before: RuntimeFrameImage)
    ensures match runtime_noscore_child_eof_accept(before) {
        None => true,
        Some(after) => {
            &&& runtime_noscore_child_kind(before).is_some()
            &&& bridge::bridge_score_finish_selected(before.score).is_none()
            &&& before.score.admission.frames.len() > 1
            &&& !admission::admission_summary_has_score(
                runtime_noscore_child_kind(before).unwrap(),
            )
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .returns.len()
                == before.score.admission.frames[before.score.admission.frames.len() - 2]
                    .returns.len() + 1
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .returns.last().kind == runtime_noscore_child_kind(before).unwrap()
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .returns.last().score_handle.is_none()
        },
    },
{
    runtime_child_accept_is_derived(before);
    runtime_noscore_child_selector_excluded(before);
    if runtime_noscore_child_kind(before).is_some() {
        let kind = runtime_noscore_child_kind(before).unwrap();
        match kind {
            admission::AdmissionReturnedKind::ReceiveBind { source_present } => {
                assert(!source_present);
            },
            admission::AdmissionReturnedKind::MatchCase { required_present, .. } => {
                assert(!required_present);
            },
            admission::AdmissionReturnedKind::BigRational { .. }
            | admission::AdmissionReturnedKind::FixedPoint { .. }
            | admission::AdmissionReturnedKind::Unit
            | admission::AdmissionReturnedKind::VarRef { .. }
            | admission::AdmissionReturnedKind::ConnectiveBody { .. }
            | admission::AdmissionReturnedKind::Bytes { .. } => {},
            admission::AdmissionReturnedKind::ExprPayload { score_present, .. } => {
                assert(!score_present);
            },
            admission::AdmissionReturnedKind::KeyValuePair { .. }
            | admission::AdmissionReturnedKind::Other => {},
            _ => assert(false),
        }
        reveal(runtime_pre_pop_child_accept);
        if let Some(after) = runtime_pre_pop_child_accept(before) {
            assert(admission::concrete_message_driver_step(
                before.score.admission, admission::ConcreteFrameActionModel::Complete,
            ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted {
                state: after.score.admission,
            }));
            if before.score.admission.frames.last().family == admission::AdmissionFamily::Other
                && before.score.admission.frames.last().program_type_id == 30
            {
                assert(admission::concrete_derived_summary_with_arena(
                    before.score.admission.frames[before.score.admission.frames.len() - 2],
                    before.score.admission.frames.last(),
                    before.score.admission.arena,
                ) == Some(kind));
            } else {
                assert(admission::concrete_derived_summary(
                    before.score.admission.frames[before.score.admission.frames.len() - 2],
                    before.score.admission.frames.last(),
                ) == Some(kind));
            }
        }
    }
}

pub proof fn runtime_noscore_emap_ezipper_eof_is_complete_image(before: RuntimeFrameImage)
    requires
        runtime_noscore_child_kind(before).is_some(),
        before.score.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.score.admission.frames.last().program_type_id == 30
            || before.score.admission.frames.last().program_type_id == 32,
        runtime_pre_pop_child_accept(before).is_some(),
    ensures
        runtime_noscore_child_kind(before) == admission::concrete_derived_summary_with_arena(
            before.score.admission.frames[before.score.admission.frames.len() - 2],
            before.score.admission.frames.last(),
            before.score.admission.arena,
        ),
        bridge::bridge_score_finish_selected(before.score).is_none(),
        runtime_message_driver_step(before, RuntimeMessageEvent::Eof)
            == Some(RuntimeMessageOutcome::ParentAccepted {
                image: runtime_pre_pop_child_accept(before).unwrap(),
            }),
{
    runtime_noscore_emap_ezipper_selected_quiet(before);
    let child = before.score.admission.frames.last();
    let parent = before.score.admission.frames[before.score.admission.frames.len() - 2];
    if child.program_type_id == 32 {
        assert(parent.waiting_tag == Some(33u32));
        assert(admission::concrete_derived_summary_with_arena(
            parent, child, before.score.admission.arena,
        ) == admission::concrete_derived_summary(parent, child));
    }
}

pub proof fn runtime_child_eof_gate_transports(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
)
    requires runtime_child_eof_gate(before, after_finish),
    ensures match runtime_pre_pop_child_accept(RuntimeFrameImage {
        score: after_finish, ..before
    }) {
        None => true,
        Some(after) => {
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == after_finish.admission.arena
            &&& after.score.transcript == after_finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    let pre_pop = RuntimeFrameImage { score: after_finish, ..before };
    runtime_child_accept_is_derived(pre_pop);
    runtime_child_eof_gate_depth(before, after_finish);
    assert(pre_pop.score == after_finish);
    assert(pre_pop.score.admission.frames.len()
        == before.score.admission.frames.len());
    assert(pre_pop.score.admission.completed
        == before.score.admission.completed);
    if let Some(after) = runtime_pre_pop_child_accept(pre_pop) {
        assert(after.score.admission.frames.len() + 1
            == pre_pop.score.admission.frames.len());
        assert(after.score.admission.completed as int
            == pre_pop.score.admission.completed as int + 1);
        assert(after.score.admission.arena == pre_pop.score.admission.arena);
        assert(after.score.transcript == pre_pop.score.transcript);
        assert(after.score.admission.frames[after.score.admission.frames.len() - 1]
            .waiting_id.is_none());
    }
}

pub proof fn runtime_child_eof_completes_after_source_score(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
)
    ensures match runtime_message_eof_child_accept(before, after_finish) {
        None => true,
        Some(after) => {
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == after_finish.admission.arena
            &&& after.score.transcript == after_finish.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    reveal(runtime_message_eof_child_accept);
    if runtime_child_eof_gate(before, after_finish) {
        runtime_child_eof_gate_transports(before, after_finish);
    }
}

pub proof fn source_message_eof_child_accept_refines(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
)
    ensures match source_message_eof_child_accept(before, result) {
        None => true,
        Some(after) => {
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    if source_message_finish_ready(before, result) {
        source_message_finish_discharges_child_gate(before, result);
        let after_finish = source_message_finish_bridge(before, result).unwrap();
        runtime_child_eof_completes_after_source_score(before, after_finish);
    }
}

pub proof fn source_noscore_message_eof_child_accept_refines(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
)
    ensures match source_noscore_message_eof_child_accept(before, result) {
        None => true,
        Some(after) => {
            &&& after.score.admission.frames.len() + 1
                == before.score.admission.frames.len()
            &&& after.score.admission.completed as int
                == before.score.admission.completed as int + 1
            &&& after.score.admission.arena == before.score.admission.arena
            &&& after.score.transcript == before.score.transcript
            &&& after.score.admission.frames[after.score.admission.frames.len() - 1]
                .waiting_id.is_none()
        },
    },
{
    runtime_child_accept_is_derived(before);
}

pub proof fn runtime_root_return_is_pre_pop_exact(
    before: RuntimeFrameImage, after_finish: bridge::ScoreAdmissionBridge,
)
    ensures match runtime_message_eof_root_return(before, after_finish) {
        None => true,
        Some(returned) => {
            &&& returned.remaining_frames == 0
            &&& returned.pre_pop.score == after_finish
            &&& returned.pre_pop.score.admission.frames.len() == 1
            &&& returned.summary.score_handle
                == returned.pre_pop.score.admission.frames[0].score_handle.unwrap()
            &&& returned.summary.family
                == returned.pre_pop.score.admission.frames[0].family
            &&& returned.summary.complete
                == admission::admission_family_complete(
                    returned.pre_pop.score.admission.frames[0])
            &&& returned.summary.body_bytes
                == returned.pre_pop.score.admission.frames[0].source_bytes
            &&& returned.score_bytes
                == returned.pre_pop.score.admission.arena[
                    returned.summary.score_handle as int].transcript_bytes
        },
    },
{
    finish::modeled_root_return_matches_pre_pop(before.score, after_finish);
    admission::concrete_message_driver_step_simulates(
        after_finish.admission, admission::ConcreteFrameActionModel::Complete,
    );
}

pub proof fn runtime_empty_send_return_is_reachable()
    ensures match runtime_empty_send_return() {
        None => false,
        Some(returned) => {
            &&& returned.remaining_frames == 0
            &&& returned.summary.family == admission::AdmissionFamily::Send
            &&& !returned.summary.complete
            &&& returned.summary.score_handle == 1
        },
    },
{
    finish::empty_send_finish_has_root_return();
    let before = runtime_empty_send_before();
    let root = finish::empty_send_finish_root();
    assert(admission::admission_root_initial(root));
    admission::admission_initial_wf(root);
    bridge::bridge_initial_exact(root);
    assert(runtime_frame_image_wf(before));
    let after_finish = finish::empty_send_finish_after().unwrap();
    finish::modeled_root_return_matches_pre_pop(before.score, after_finish);
    assert(admission::admission_complete_root(after_finish.admission));
    admission::concrete_message_view_rel(after_finish.admission);
    admission::concrete_message_driver_step_simulates(
        after_finish.admission, admission::ConcreteFrameActionModel::Complete,
    );
}

pub proof fn runtime_selected_empty_send_return_is_reachable()
    ensures match runtime_message_driver_step(
        runtime_empty_send_before(), RuntimeMessageEvent::Eof,
    ) {
        Some(RuntimeMessageOutcome::RootReturned { returned }) => {
            &&& returned.remaining_frames == 0
            &&& returned.summary.family == admission::AdmissionFamily::Send
            &&& !returned.summary.complete
            &&& returned.summary.score_handle == 1
        },
        _ => false,
    },
{
    finish::empty_send_finish_has_root_return();
    runtime_empty_send_return_is_reachable();
    let before = runtime_empty_send_before();
    let root = finish::empty_send_finish_root();
    assert(admission::admission_root_initial(root));
    assert(admission::admission_frame_wf(root));
    assert(bridge::bridge_primary_component_frame(before.score));
    bridge::bridge_primary_component_finish_selected_exact(before.score);
    assert(!admission::admission_family_complete(root));
    assert(bridge::bridge_send_step(before.score).is_none());
    assert(bridge::bridge_primary_component_finish(before.score).is_none());
    assert(bridge::bridge_nonsemantic_finish_candidate(before.score)
        == finish::empty_send_finish_after());
    assert(bridge::bridge_score_finish_selected(before.score)
        == finish::empty_send_finish_after());
    let returned = runtime_empty_send_return().unwrap();
    assert(runtime_message_eof_root_return(
        before, bridge::bridge_score_finish_selected(before.score).unwrap(),
    ) == Some(returned));
}

pub proof fn runtime_noscore_receive_bind_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_receive_bind_before()),
        runtime_noscore_child_kind(runtime_noscore_receive_bind_before())
            == Some(admission::AdmissionReturnedKind::ReceiveBind {
                source_present: false,
            }),
        bridge::bridge_score_finish_selected(
            runtime_noscore_receive_bind_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_receive_bind_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
            },
            _ => false,
        },
{
    let before = runtime_noscore_receive_bind_before();
    let state = before.score.admission;
    let child = state.frames[1];
    let parent = state.frames[0];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 19 || program_type_id == 16)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 19 || program_type_id == 16)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 19u16 } else { 16u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    admission::admission_fabricated_child_physical_contained(
        admission::AdmissionFamily::Receive, 0,
        admission::AdmissionFamily::ReceiveBindFrame, 1,
    );
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::ReceiveBind {
            source_present: false,
        }));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::ReceiveBind {
            source_present: false,
        },
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_match_case_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_match_case_before()),
        runtime_noscore_child_kind(runtime_noscore_match_case_before())
            == Some(admission::AdmissionReturnedKind::MatchCase {
                required_present: false, canonical_fixed_point: true,
            }),
        bridge::bridge_score_finish_selected(
            runtime_noscore_match_case_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_match_case_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
            },
            _ => false,
        },
{
    let before = runtime_noscore_match_case_before();
    let state = before.score.admission;
    let child = state.frames[1];
    let parent = state.frames[0];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 22 || program_type_id == 21)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 22 || program_type_id == 21)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 22u16 } else { 21u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    admission::admission_fabricated_child_physical_contained(
        admission::AdmissionFamily::Match, 1,
        admission::AdmissionFamily::MatchCaseFrame, 2,
    );
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::MatchCase {
            required_present: false, canonical_fixed_point: true,
        }));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::MatchCase {
            required_present: false, canonical_fixed_point: true,
        },
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_big_rational_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_big_rational_before()),
        runtime_noscore_child_kind(runtime_noscore_big_rational_before())
            == Some(admission::AdmissionReturnedKind::BigRational {
                numerator: Seq::empty(), denominator: Seq::empty(),
            }),
        bridge::bridge_score_finish_selected(
            runtime_noscore_big_rational_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_big_rational_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
            },
            _ => false,
        },
{
    let before = runtime_noscore_big_rational_before();
    let state = before.score.admission;
    let child = state.frames[1];
    let parent = state.frames[0];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 24 || program_type_id == 25)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 24 || program_type_id == 25)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 24u16 } else { 25u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    admission::admission_fabricated_child_physical_contained(
        admission::AdmissionFamily::Expr, 0,
        admission::AdmissionFamily::BigRationalFrame, 36,
    );
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::BigRational {
            numerator: Seq::empty(), denominator: Seq::empty(),
        }));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::BigRational {
            numerator: Seq::empty(), denominator: Seq::empty(),
        },
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_evar_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_evar_before()),
        runtime_noscore_child_kind(runtime_noscore_evar_before())
            == Some(admission::AdmissionReturnedKind::ExprPayload {
                score_present: false, canonical_fixed_point: true,
                canonical_path_stable: false,
            }),
        bridge::bridge_score_finish_selected(
            runtime_noscore_evar_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_evar_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
                &&& image.score.admission.frames[0].returns.last().kind
                    == admission::AdmissionReturnedKind::ExprPayload {
                        score_present: false, canonical_fixed_point: true,
                        canonical_path_stable: false,
                    }
            },
            _ => false,
        },
{
    let before = runtime_noscore_evar_before();
    let state = before.score.admission;
    let child = state.frames[1];
    let parent = state.frames[0];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 24 || program_type_id == 35)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 24 || program_type_id == 35)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 24u16 } else { 35u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    admission::admission_fabricated_child_physical_contained(
        admission::AdmissionFamily::Expr, 0,
        admission::AdmissionFamily::EVarFrame, 19,
    );
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        }));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        },
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub open spec fn runtime_noscore_expr_structural_before(binary: bool) -> RuntimeFrameImage {
    if binary { runtime_noscore_expr_binary_before() }
    else { runtime_noscore_expr_unary_before() }
}

pub proof fn runtime_noscore_expr_structural_modeled_case(binary: bool)
    ensures {
        let before = runtime_noscore_expr_structural_before(binary);
        &&& runtime_frame_image_wf(before)
        &&& runtime_noscore_child_kind(before)
            == Some(admission::AdmissionReturnedKind::ExprPayload {
                score_present: false, canonical_fixed_point: true,
                canonical_path_stable: false,
            })
        &&& bridge::bridge_score_finish_selected(before.score).is_none()
        &&& match runtime_message_driver_step(before, RuntimeMessageEvent::Eof) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
                &&& image.score.admission.frames[0].returns.last().kind
                    == admission::AdmissionReturnedKind::ExprPayload {
                        score_present: false, canonical_fixed_point: true,
                        canonical_path_stable: false,
                    }
            },
            _ => false,
        }
    },
{
    let before = runtime_noscore_expr_structural_before(binary);
    let state = before.score.admission;
    let child = state.frames[1];
    let parent = state.frames[0];
    let child_id = if binary { 38u16 } else { 36u16 };
    assert(child.program_type_id == child_id);
    assert(parent.waiting_tag == Some(if binary { 7u32 } else { 5u32 }));
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 24 || program_type_id == child_id)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 24 || program_type_id == child_id)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 24u16 } else { child_id }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(parent.physical_start == 0);
    assert(child.physical_start == 0);
    assert(parent.region_end == 0);
    assert(child.region_end == 0);
    assert(parent.source_bytes == Seq::<u8>::empty());
    assert(child.source_bytes == Seq::<u8>::empty());
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        }));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        },
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_expr_structural_modeled_witness()
    ensures
        runtime_noscore_child_kind(runtime_noscore_expr_unary_before())
            == Some(admission::AdmissionReturnedKind::ExprPayload {
                score_present: false, canonical_fixed_point: true,
                canonical_path_stable: false,
            }),
        runtime_noscore_child_kind(runtime_noscore_expr_binary_before())
            == Some(admission::AdmissionReturnedKind::ExprPayload {
                score_present: false, canonical_fixed_point: true,
                canonical_path_stable: false,
            }),
        match runtime_message_driver_step(
            runtime_noscore_expr_unary_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { .. }) => true,
            _ => false,
        },
        match runtime_message_driver_step(
            runtime_noscore_expr_binary_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { .. }) => true,
            _ => false,
        },
{
    runtime_noscore_expr_structural_modeled_case(false);
    runtime_noscore_expr_structural_modeled_case(true);
}

pub proof fn runtime_noscore_emethod_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_emethod_before()),
        runtime_noscore_child_kind(runtime_noscore_emethod_before())
            == Some(admission::AdmissionReturnedKind::ExprPayload {
                score_present: false, canonical_fixed_point: true,
                canonical_path_stable: false,
            }),
        bridge::bridge_score_finish_selected(
            runtime_noscore_emethod_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_emethod_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
                &&& image.score.admission.frames[0].returns.last().kind
                    == admission::AdmissionReturnedKind::ExprPayload {
                        score_present: false, canonical_fixed_point: true,
                        canonical_path_stable: false,
                    }
            },
            _ => false,
        },
{
    let before = runtime_noscore_emethod_before();
    let state = before.score.admission;
    let child = state.frames[1];
    let parent = state.frames[0];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 24 || program_type_id == 33)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 24 || program_type_id == 33)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 24u16 } else { 33u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(parent.physical_start == 0);
    assert(child.physical_start == 0);
    assert(parent.region_end == 0);
    assert(child.region_end == 0);
    assert(parent.source_bytes == Seq::<u8>::empty());
    assert(child.source_bytes == Seq::<u8>::empty());
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        }));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::ExprPayload {
            score_present: false, canonical_fixed_point: true,
            canonical_path_stable: false,
        },
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_key_value_pair_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_key_value_pair_before()),
        runtime_noscore_child_kind(runtime_noscore_key_value_pair_before())
            == Some(admission::AdmissionReturnedKind::KeyValuePair {
                key: None, value_par: None,
            }),
        bridge::bridge_score_finish_selected(
            runtime_noscore_key_value_pair_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_key_value_pair_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
                &&& image.score.admission.frames[0].returns.last().kind
                    == admission::AdmissionReturnedKind::KeyValuePair {
                        key: None, value_par: None,
                    }
            },
            _ => false,
        },
{
    let before = runtime_noscore_key_value_pair_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 30 || program_type_id == 34)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 30 || program_type_id == 34)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 30u16 } else { 34u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(parent.physical_start == 0);
    assert(child.physical_start == 0);
    assert(parent.region_end == 0);
    assert(child.region_end == 0);
    assert(parent.source_bytes == Seq::<u8>::empty());
    assert(child.source_bytes == Seq::<u8>::empty());
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::admission_key_value_pair_fields(child) == Some((None, None)));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::KeyValuePair {
            key: None, value_par: None,
        }));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::KeyValuePair {
            key: None, value_par: None,
        },
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_key_value_pair_present_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_key_value_pair_present_before()),
        match runtime_noscore_child_kind(runtime_noscore_key_value_pair_present_before()) {
            Some(admission::AdmissionReturnedKind::KeyValuePair { key, value_par }) => {
                &&& key.is_some()
                &&& value_par.is_some()
                &&& key.unwrap().score_handle == 0
                &&& value_par.unwrap().score_handle == 1
                &&& key.unwrap().body_id == 9
                &&& value_par.unwrap().body_id == 10
            },
            _ => false,
        },
        bridge::bridge_score_finish_selected(
            runtime_noscore_key_value_pair_present_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_key_value_pair_present_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 2
                &&& image.score.admission.frames[0].returns.last().kind
                    == runtime_noscore_child_kind(
                        runtime_noscore_key_value_pair_present_before(),
                    ).unwrap()
            },
            _ => false,
        },
{
    let before = runtime_noscore_key_value_pair_present_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 30 || program_type_id == 34)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {}
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 30 || program_type_id == 34)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 30u16 } else { 34u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    assert(score::integer_transcript(0).len() == 10);
    assert(score::integer_transcript(1).len() == 10);
    assert(score::arena_well_formed(before.score.transcript));
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::admission_key_value_pair_fields(child).is_some());
    let pair = admission::admission_key_value_pair_fields(child).unwrap();
    assert(pair.0.is_some() && pair.1.is_some());
    assert(admission::admission_key_value_pair_bindings(pair.0, pair.1, state.arena));
    assert(admission::concrete_derived_summary(parent, child).is_some());
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_opaque_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_opaque_before()),
        runtime_noscore_child_kind(runtime_noscore_opaque_before())
            == Some(admission::AdmissionReturnedKind::Other),
        bridge::bridge_score_finish_selected(
            runtime_noscore_opaque_before().score,
        ).is_none(),
        match runtime_message_driver_step(
            runtime_noscore_opaque_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
                &&& image.score.admission.frames[0].returns.last().kind
                    == admission::AdmissionReturnedKind::Other
            },
            _ => false,
        },
{
    let before = runtime_noscore_opaque_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    assert forall|program_type_id: u16, tag: u32|
        (program_type_id == 2 || program_type_id == 3)
        implies #[trigger] admission::admission_descriptor_not_pathmap(
            program_type_id, tag,
        ) by {
    }
    assert forall|program_type_id: u16, slot: int|
        (program_type_id == 2 || program_type_id == 3)
        implies #[trigger] admission::admission_no_required_descriptor_slot(
            program_type_id, slot,
        ) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
    assert forall|frame_index: int| 0 <= frame_index < state.frames.len()
        implies admission::admission_required_slots_generated(
            state.frames[frame_index],
        ) by {
        let frame = state.frames[frame_index];
        assert(frame.required == Set::empty());
        assert(frame.program_type_id == (if frame_index == 0 { 2u16 } else { 3u16 }));
        assert forall|slot: int| 0 <= slot < frame.slot_count as int
            implies frame.required.contains(slot)
                == (exists|tag: u32| admission::admission_required_descriptor_slot(
                    frame.program_type_id, tag, slot,
                )) by {
            assert(!frame.required.contains(slot));
            assert(!(exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )));
        }
    }
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(parent.physical_start == 0);
    assert(child.physical_start == 0);
    assert(parent.region_end == 0);
    assert(child.region_end == 0);
    assert(parent.source_bytes == Seq::<u8>::empty());
    assert(child.source_bytes == Seq::<u8>::empty());
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(admission::admission_opaque_child_edge(parent, child));
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::Other));
    assert(runtime_noscore_child_kind(before).is_some());
    runtime_noscore_child_eof_is_checked(before);
    assert(admission::admission_complete_child_step(
        state, 0, 1, admission::AdmissionReturnedKind::Other,
    ).is_some());
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_noscore_child_eof_accept(before).is_some());
}

pub proof fn runtime_noscore_witness_required_slots(frame: admission::AdmissionFrameState)
    requires
        frame.required == Set::<int>::empty(),
        runtime_noscore_witness_program(frame.program_type_id),
    ensures admission::admission_required_slots_generated(frame),
{
    let program_type_id = frame.program_type_id;
    assert forall|tag: u32|
        #[trigger] admission::admission_descriptor_not_pathmap(program_type_id, tag) by {
    }
    assert forall|slot: int| 0 <= slot < frame.slot_count as int
        implies frame.required.contains(slot)
            == (exists|tag: u32| admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(
                program_type_id, tag, slot,
            ) by {
            assert(admission::admission_descriptor_not_pathmap(program_type_id, tag));
        }
    }
}

pub proof fn runtime_noscore_leaf_image_wf(
    parent_family: admission::AdmissionFamily, field_tag: u32,
    child_family: admission::AdmissionFamily, child_program: u16, child_slots: usize,
)
    requires
        admission::admission_frame_wf(runtime_noscore_leaf_before(
            parent_family, field_tag, child_family, child_program, child_slots,
        ).score.admission.frames[0]),
        admission::admission_frame_wf(runtime_noscore_leaf_before(
            parent_family, field_tag, child_family, child_program, child_slots,
        ).score.admission.frames[1]),
    ensures
        runtime_frame_image_wf(runtime_noscore_leaf_before(
            parent_family, field_tag, child_family, child_program, child_slots,
        )),
{
    let before = runtime_noscore_leaf_before(
        parent_family, field_tag, child_family, child_program, child_slots,
    );
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
}

pub proof fn runtime_noscore_ezipper_absent_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_ezipper_absent_before()),
        runtime_noscore_child_kind(runtime_noscore_ezipper_absent_before())
            == Some(runtime_noscore_absent_expr_payload()),
        bridge::bridge_score_finish_selected(
            runtime_noscore_ezipper_absent_before().score,
        ).is_none(),
        runtime_message_driver_step(
            runtime_noscore_ezipper_absent_before(), RuntimeMessageEvent::Eof,
        ) == Some(RuntimeMessageOutcome::ParentAccepted {
            image: runtime_pre_pop_child_accept(runtime_noscore_ezipper_absent_before()).unwrap(),
        }),
        match runtime_message_driver_step(
            runtime_noscore_ezipper_absent_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
                &&& image.score.admission.frames[0].returns.last().kind
                    == runtime_noscore_absent_expr_payload()
            },
            _ => false,
        },
{
    let before = runtime_noscore_ezipper_absent_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    let kind = runtime_noscore_absent_expr_payload();
    runtime_noscore_witness_required_slots(parent);
    runtime_noscore_witness_required_slots(child);
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    runtime_noscore_leaf_image_wf(
        admission::AdmissionFamily::Expr, 33, admission::AdmissionFamily::Other, 32, 6,
    );
    assert(admission::admission_ezipper_expr_payload(child) == Some(kind));
    assert(admission::concrete_derived_summary(parent, child) == Some(kind));
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(kind));
    assert(runtime_noscore_child_kind(before) == Some(kind));
    reveal(runtime_pre_pop_child_accept);
    assert(admission::admission_complete_child_step(state, 0, 1, kind).is_some());
    assert(runtime_pre_pop_child_accept(before).is_some());
    runtime_noscore_emap_ezipper_eof_is_complete_image(before);
    runtime_noscore_child_eof_is_checked(before);
}

pub proof fn runtime_noscore_emap_incomplete_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_noscore_emap_incomplete_before()),
        admission::concrete_derived_summary(
            runtime_noscore_emap_incomplete_before().score.admission.frames[0],
            runtime_noscore_emap_incomplete_before().score.admission.frames[1],
        ).is_none(),
        runtime_noscore_child_kind(runtime_noscore_emap_incomplete_before())
            == Some(runtime_noscore_absent_expr_payload()),
        bridge::bridge_score_finish_selected(
            runtime_noscore_emap_incomplete_before().score,
        ).is_none(),
        runtime_message_driver_step(
            runtime_noscore_emap_incomplete_before(), RuntimeMessageEvent::Eof,
        ) == Some(RuntimeMessageOutcome::ParentAccepted {
            image: runtime_pre_pop_child_accept(runtime_noscore_emap_incomplete_before()).unwrap(),
        }),
        match runtime_message_driver_step(
            runtime_noscore_emap_incomplete_before(), RuntimeMessageEvent::Eof,
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.arena.len() == 0
                &&& image.score.admission.frames[0].returns.last().kind
                    == runtime_noscore_absent_expr_payload()
            },
            _ => false,
        },
{
    let before = runtime_noscore_emap_incomplete_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    let pair = runtime_noscore_emap_absent_pair();
    runtime_noscore_witness_required_slots(parent);
    runtime_noscore_witness_required_slots(child);
    assert(child.returns[0] == pair);
    assert(admission::admission_frame_wf(parent));
    assert(admission::admission_frame_wf(child));
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    let image = admission::AdmissionEMapPairImage {
        key: None, value_par: None, body_bytes: Seq::empty(),
    };
    assert(admission::admission_emap_pair_from_return(pair, state.arena) == Some(image));
    assert(child.returns.drop_first() =~= Seq::<admission::AdmissionReturnedSummary>::empty());
    assert(admission::admission_emap_pairs_from_returns(
        Seq::<admission::AdmissionReturnedSummary>::empty(), state.arena,
    ) == Some(Seq::<admission::AdmissionEMapPairImage>::empty()));
    assert(seq![image] + Seq::<admission::AdmissionEMapPairImage>::empty() =~= seq![image]);
    assert(admission::admission_emap_pairs_from_returns(child.returns, state.arena)
        == Some(seq![image]));
    assert(!admission::admission_emap_complete(seq![image]));
    assert(admission::admission_emap_source_fixed_point(seq![image], state.arena));
    assert(admission::admission_emap_source_summary(child, state.arena) == Some((false, true)));
    let kind = runtime_noscore_absent_expr_payload();
    assert(admission::concrete_derived_summary_with_arena(parent, child, state.arena)
        == Some(kind));
    assert(admission::concrete_derived_summary(parent, child).is_none());
    assert(runtime_noscore_child_kind(before) == Some(kind));
    reveal(runtime_pre_pop_child_accept);
    assert(admission::admission_complete_child_step(state, 0, 1, kind).is_some());
    assert(runtime_pre_pop_child_accept(before).is_some());
    runtime_noscore_emap_ezipper_eof_is_complete_image(before);
    runtime_noscore_child_eof_is_checked(before);
}

pub open spec fn runtime_noscore_expr_structural_tag(tag: u32) -> bool {
    tag == 5 || tag == 6 || tag == 20 || tag == 21 || tag == 24 || tag == 32 || tag == 33
        || (7 <= tag && tag <= 18) || (27 <= tag && tag <= 31)
}

pub proof fn runtime_noscore_structural_summary_selected(
    before: RuntimeFrameImage, kind: admission::AdmissionReturnedKind,
)
    requires
        runtime_frame_image_wf(before),
        before.score.admission.frames.len() > 1,
        before.score.admission.frames.last().frame_kind == 1,
        before.score.admission.frames.last().waiting_id.is_none(),
        before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end,
        before.score.admission.frames.last().score_handle.is_none(),
        before.score.admission.frames[before.score.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Expr,
        before.score.admission.frames[before.score.admission.frames.len() - 2]
            .program_type_id == 24,
        before.score.admission.frames[before.score.admission.frames.len() - 2].waiting_slot
            == Some(0usize),
        match before.score.admission.frames[before.score.admission.frames.len() - 2]
            .waiting_tag {
            Some(tag) => runtime_noscore_expr_structural_tag(tag),
            None => false,
        },
        admission::concrete_derived_summary(
            before.score.admission.frames[before.score.admission.frames.len() - 2],
            before.score.admission.frames.last(),
        ) == Some(kind),
        admission::admission_expr_structural_payload(
            before.score.admission.frames[before.score.admission.frames.len() - 2],
            before.score.admission.frames.last(),
        ) == Some(kind),
        !admission::admission_summary_has_score(kind),
    ensures runtime_noscore_child_kind(before) == Some(kind),
{
    let child = before.score.admission.frames.last();
    let parent = before.score.admission.frames[before.score.admission.frames.len() - 2];
    let tag = parent.waiting_tag.unwrap();
    if tag == 33 {
        assert(child.program_type_id == 32);
    } else if tag == 24 {
        assert(child.program_type_id == 33);
    } else if tag == 5 || tag == 6 {
        assert(36 <= child.program_type_id && child.program_type_id <= 37);
    } else {
        assert((7 <= tag && tag <= 18) || (27 <= tag && tag <= 31)
            || tag == 20 || tag == 21 || tag == 32);
        if (7 <= tag && tag <= 18) || (27 <= tag && tag <= 31) {
            assert(38 <= child.program_type_id && child.program_type_id <= 54);
            assert(bridge::bridge_expr_structural_constructor(child.program_type_id).is_some());
            assert(bridge::bridge_expr_structural_constructor(child.program_type_id).unwrap().1);
        }
    }
}

pub proof fn runtime_noscore_family_summary_selected(
    before: RuntimeFrameImage, kind: admission::AdmissionReturnedKind,
)
    requires
        runtime_frame_image_wf(before),
        before.score.admission.frames.len() > 1,
        before.score.admission.frames.last().frame_kind == 1,
        before.score.admission.frames.last().waiting_id.is_none(),
        before.score.admission.frames.last().cursor
            == before.score.admission.frames.last().region_end,
        before.score.admission.frames.last().score_handle.is_none(),
        admission::admission_frame_wf(before.score.admission.frames.last()),
        before.score.admission.frames[before.score.admission.frames.len() - 2].frame_kind == 1,
        before.score.admission.frames[before.score.admission.frames.len() - 2]
            .program_type_id != 1,
        before.score.admission.frames.last().program_type_id != 1,
        !(before.score.admission.frames[before.score.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Other
            && before.score.admission.frames[before.score.admission.frames.len() - 2]
                .program_type_id == 32
            && before.score.admission.frames[before.score.admission.frames.len() - 2]
                .waiting_slot == Some(0usize)
            && before.score.admission.frames[before.score.admission.frames.len() - 2]
                .waiting_tag == Some(1u32)),
        !(before.score.admission.frames[before.score.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Expr
            && before.score.admission.frames[before.score.admission.frames.len() - 2]
                .waiting_slot == Some(0usize)
            && match before.score.admission.frames[before.score.admission.frames.len() - 2]
                .waiting_tag {
                Some(tag) => runtime_noscore_expr_structural_tag(tag),
                None => false,
            }),
        before.score.admission.frames[before.score.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Expr
            ==> before.score.admission.frames[before.score.admission.frames.len() - 2]
                .program_type_id == 24
                && before.score.admission.frames[before.score.admission.frames.len() - 2]
                    .waiting_slot == Some(0usize),
        admission::concrete_derived_summary(
            before.score.admission.frames[before.score.admission.frames.len() - 2],
            before.score.admission.frames.last(),
        ) == Some(kind),
        admission::admission_child_summary_agrees(
            before.score.admission.frames.last(), kind, before.score.admission.arena,
        ),
        !admission::admission_summary_has_score(kind),
    ensures runtime_noscore_child_kind(before) == Some(kind),
{
    let child = before.score.admission.frames.last();
    let parent = before.score.admission.frames[before.score.admission.frames.len() - 2];
    assert(admission::concrete_derived_summary(parent, child) == match child.family {
        admission::AdmissionFamily::ReceiveBindFrame => Some(
            admission::AdmissionReturnedKind::ReceiveBind {
                source_present: admission::admission_returned_par(child, 1),
            }),
        admission::AdmissionFamily::MatchCaseFrame => Some(
            admission::AdmissionReturnedKind::MatchCase {
                required_present: admission::admission_returned_par(child, 0)
                    && admission::admission_returned_par(child, 1),
                canonical_fixed_point: !admission::admission_returned_default_par(child, 3),
            }),
        admission::AdmissionFamily::CostSignatureFrame
        | admission::AdmissionFamily::CostRegionFrame =>
            Some(admission::AdmissionReturnedKind::CostScore),
        admission::AdmissionFamily::BigRationalFrame =>
            match admission::admission_big_rational_fields(child) {
                Some((numerator, denominator)) => Some(
                    admission::AdmissionReturnedKind::BigRational { numerator, denominator }),
                None => None,
            },
        admission::AdmissionFamily::FixedPointFrame =>
            match admission::admission_fixed_point_fields(child) {
                Some((unscaled, scale)) => Some(
                    admission::AdmissionReturnedKind::FixedPoint { unscaled, scale }),
                None => None,
            },
        admission::AdmissionFamily::EVarFrame if parent.family
            == admission::AdmissionFamily::Expr && parent.waiting_tag == Some(19u32) => Some(
            admission::AdmissionReturnedKind::ExprPayload {
                score_present: admission::admission_returned_var(child, 0),
                canonical_fixed_point: true,
                canonical_path_stable: false,
            }),
        admission::AdmissionFamily::Other if child.frame_kind == 1
            && child.program_type_id == 34
            && parent.family == admission::AdmissionFamily::Other
            && parent.program_type_id == 30
            && parent.waiting_slot == Some(0usize)
            && parent.waiting_tag == Some(1u32) =>
            match admission::admission_key_value_pair_fields(child) {
                Some((key, value_par)) => Some(
                    admission::AdmissionReturnedKind::KeyValuePair { key, value_par }),
                None => None,
            },
        admission::AdmissionFamily::Other if admission::admission_opaque_child_edge(
            parent, child,
        ) => Some(admission::AdmissionReturnedKind::Other),
        admission::AdmissionFamily::Other if child.frame_kind == 2
            && parent.family == admission::AdmissionFamily::New
            && parent.waiting_slot == Some(3usize)
            && parent.waiting_tag == Some(4u32)
            && admission::admission_map_summary_complete(child) => Some(
            admission::AdmissionReturnedKind::Map {
                key_bytes: admission::admission_map_key_bytes(child),
                value_score_handle: admission::admission_map_value_score_handle(child),
            }),
        admission::AdmissionFamily::Other if child.frame_kind == 1 =>
            match child.program_type_id {
                6 => Some(admission::AdmissionReturnedKind::VarScore),
                7 | 64 => Some(admission::AdmissionReturnedKind::Unit),
                10 | 11 => Some(admission::AdmissionReturnedKind::CostScore),
                56 => match admission::admission_var_ref_fields(child) {
                    Some((index, depth)) => Some(
                        admission::AdmissionReturnedKind::VarRef { index, depth }),
                    None => None,
                },
                57 => match admission::admission_connective_body_handles(child) {
                    Some(par_handles) => Some(
                        admission::AdmissionReturnedKind::ConnectiveBody { par_handles }),
                    None => None,
                },
                61 | 62 | 63 => match admission::admission_bytes_leaf_payload(child) {
                    Some(value) => Some(admission::AdmissionReturnedKind::Bytes { value }),
                    None => None,
                },
                _ => None,
            },
        _ => None,
    });
    if child.family == admission::AdmissionFamily::Other
        && admission::admission_opaque_child_edge(parent, child)
        && !(child.program_type_id == 34
            && parent.family == admission::AdmissionFamily::Other
            && parent.program_type_id == 30
            && parent.waiting_slot == Some(0usize)
            && parent.waiting_tag == Some(1u32))
    {
        assert(child.program_type_id == 3 || child.program_type_id == 14
            || child.program_type_id == 17);
    }
}

pub proof fn runtime_noscore_child_kind_reads_pushed_summary(before: RuntimeFrameImage)
    requires
        runtime_frame_image_wf(before),
        before.score.admission.frames.len() > 1,
        before.score.admission.frames.last().frame_kind == 1,
        runtime_pre_pop_child_accept(before).is_some(),
    ensures
        admission::concrete_derived_summary_with_arena(
            before.score.admission.frames[before.score.admission.frames.len() - 2],
            before.score.admission.frames.last(),
            before.score.admission.arena,
        ).is_some(),
        runtime_noscore_child_kind(before) == if admission::admission_summary_has_score(
            admission::concrete_derived_summary_with_arena(
                before.score.admission.frames[before.score.admission.frames.len() - 2],
                before.score.admission.frames.last(),
                before.score.admission.arena,
            ).unwrap(),
        ) {
            None
        } else {
            admission::concrete_derived_summary_with_arena(
                before.score.admission.frames[before.score.admission.frames.len() - 2],
                before.score.admission.frames.last(),
                before.score.admission.arena,
            )
        },
{
    reveal(runtime_pre_pop_child_accept);
    let state = before.score.admission;
    let child = state.frames[state.frames.len() - 1];
    let parent = state.frames[state.frames.len() - 2];
    let arena = state.arena;
    assert(admission::admission_state_wf(state));
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_frame_wf(parent));
    assert(admission::concrete_message_complete_child(state).is_some());
    let kind = admission::concrete_derived_summary_with_arena(parent, child, arena).unwrap();
    assert(admission::admission_complete_child_step(
        state, child.return_id.unwrap(), child.frame_kind, kind,
    ).is_some());
    if parent.family == admission::AdmissionFamily::Expr
        && parent.waiting_slot == Some(0usize)
        && (parent.waiting_tag == Some(22u32) || parent.waiting_tag == Some(23u32))
        && parent.program_type_id == 24
    {
        assert(admission::concrete_derived_summary_with_arena(parent, child, arena)
            == admission::admission_expr_structural_payload_with_arena(parent, child, arena));
        if !(child.family == admission::AdmissionFamily::Other && child.frame_kind == 1
            && (child.program_type_id == 29 || child.program_type_id == 30))
        {
            assert(admission::admission_expr_structural_payload(parent, child).is_none());
        } else if child.program_type_id == 29 {
            if parent.waiting_tag == Some(23u32) {
                assert(admission::admission_expr_structural_payload(parent, child).is_none());
            }
            assert(admission::concrete_derived_summary(parent, child).is_none());
        } else if parent.waiting_tag == Some(22u32) {
            assert(admission::admission_expr_structural_payload(parent, child).is_none());
        } else {
            assert(admission::concrete_derived_summary(parent, child).is_none());
        }
    } else {
        assert(admission::concrete_derived_summary_with_arena(parent, child, arena)
            == admission::concrete_derived_summary(parent, child));
        if parent.family == admission::AdmissionFamily::Expr {
            assert(parent.program_type_id == 24);
            assert(parent.waiting_slot == Some(0usize));
        }
        if !admission::admission_summary_has_score(kind) {
            assert(child.score_handle.is_none());
            assert(parent.program_type_id != 1);
            assert(!(child.frame_kind == 1 && child.program_type_id == 1));
            assert(!(parent.family == admission::AdmissionFamily::Other
                && parent.program_type_id == 32
                && parent.waiting_slot == Some(0usize)
                && parent.waiting_tag == Some(1u32)));
            if parent.family == admission::AdmissionFamily::Expr
                && parent.waiting_slot == Some(0usize)
                && match parent.waiting_tag {
                    Some(tag) => runtime_noscore_expr_structural_tag(tag),
                    None => false,
                }
            {
                assert(admission::concrete_derived_summary(parent, child)
                    == admission::admission_expr_structural_payload(parent, child));
                runtime_noscore_structural_summary_selected(before, kind);
            } else {
                runtime_noscore_family_summary_selected(before, kind);
            }
        }
    }
}

pub proof fn source_noscore_route_agrees_with_runtime(
    before: RuntimeFrameImage, result: SourceMessageFinishResult,
)
    requires source_message_finish_bridge(before, result) == Some(before.score),
    ensures
        source_noscore_message_eof_child_accept(before, result)
            == runtime_noscore_child_eof_accept(before),
{
    reveal(source_noscore_message_finish_ready);
    if runtime_frame_image_wf(before)
        && before.score.admission.frames.len() > 1
        && before.score.admission.frames.last().frame_kind == 1
        && runtime_pre_pop_child_accept(before).is_some()
    {
        runtime_noscore_child_kind_reads_pushed_summary(before);
    }
}

pub proof fn runtime_emap_ezipper_source_steps_quiet(before: bridge::ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 30
            || before.admission.frames.last().program_type_id == 32,
        bridge::bridge_emap_finish_step(before).is_none(),
    ensures
        forall|after: bridge::ScoreAdmissionBridge|
            !bridge::bridge_score_source_step(before, after),
{
    assert(bridge::bridge_raw_par_step(before).is_none());
    assert(bridge::bridge_cost_atom_absent_step(before).is_none());
    assert(bridge::bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge::bridge_cost_bound_step(before).is_none());
    assert(bridge::bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge::bridge_cost_signature_step(before).is_none());
    assert(bridge::bridge_cost_region_step(before).is_none());
    assert(bridge::bridge_cost_token_stack_step(before).is_none());
    assert(bridge::bridge_bundle_step(before).is_none());
    assert(bridge::bridge_if_step(before).is_none());
    assert(bridge::bridge_send_step(before).is_none());
    assert(bridge::bridge_match_step(before).is_none());
    assert(bridge::bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge::bridge_absent_connective_step(before).is_none());
    assert(bridge::bridge_receive_bind_step(before).is_none());
    assert(bridge::bridge_match_case_step(before).is_none());
    assert(bridge::bridge_bool_oneof_step(before).is_none());
    assert(bridge::bridge_expr_atom_step(before).is_none());
    assert(bridge::bridge_expr_numeric_step(before).is_none());
    assert(bridge::bridge_expr_big_rational_step(before).is_none());
    assert(bridge::bridge_expr_fixed_point_step(before).is_none());
    assert(bridge::bridge_sys_auth_token_step(before).is_none());
    assert(bridge::bridge_new_empty_collections_step(before).is_none());
    assert(bridge::bridge_new_one_injection_step(before).is_none());
    assert(bridge::bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge::bridge_new_one_uri_step(before).is_none());
    assert(bridge::bridge_new_collections_step(before).is_none());
    assert(bridge::bridge_expr_payload_forward_step(before).is_none());
    assert(bridge::bridge_expr_structural_constructor(
        before.admission.frames.last().program_type_id,
    ).is_none());
    assert(bridge::bridge_expr_structural_step(before).is_none());
    assert(bridge::bridge_evar_step(before).is_none());
    assert(bridge::bridge_ordered_sequence_step(before).is_none());
    assert(bridge::bridge_emethod_step(before).is_none());
    assert(bridge::bridge_connective_not_step(before).is_none());
    assert(bridge::bridge_connective_varref_step(before).is_none());
    assert(bridge::bridge_connective_collection_step(before).is_none());
    assert(bridge::bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge::bridge_receive_present_condition_step(before).is_none());
    assert(bridge::bridge_receive_absent_condition_step(before).is_none());
    assert(bridge::bridge_nonsemantic_step(before).is_none());
}

pub open spec fn source_unchanged_finish_result(
    before: RuntimeFrameImage,
) -> SourceMessageFinishResult {
    SourceMessageFinishResult {
        updated_top: before.score.admission.frames.last(),
        score_arena: before.score.admission.arena,
        transcript: before.score.transcript,
    }
}

pub proof fn source_noscore_emap_incomplete_route_witness()
    ensures
        source_message_finish_bridge(
            runtime_noscore_emap_incomplete_before(),
            source_unchanged_finish_result(runtime_noscore_emap_incomplete_before()),
        ) == Some(runtime_noscore_emap_incomplete_before().score),
        runtime_pre_pop_child_accept(runtime_noscore_emap_incomplete_before()).is_some(),
        source_noscore_message_eof_child_accept(
            runtime_noscore_emap_incomplete_before(),
            source_unchanged_finish_result(runtime_noscore_emap_incomplete_before()),
        ) == runtime_pre_pop_child_accept(runtime_noscore_emap_incomplete_before()),
        source_message_finish_route(
            runtime_noscore_emap_incomplete_before(),
            source_unchanged_finish_result(runtime_noscore_emap_incomplete_before()),
        ) == Some(SourceMessageFinishRoute::Child {
            kind: runtime_noscore_absent_expr_payload(),
            image: runtime_pre_pop_child_accept(runtime_noscore_emap_incomplete_before()).unwrap(),
        }),
{
    let before = runtime_noscore_emap_incomplete_before();
    let result = source_unchanged_finish_result(before);
    let state = before.score.admission;
    let kind = runtime_noscore_absent_expr_payload();
    runtime_noscore_emap_incomplete_modeled_witness();
    assert(runtime_noscore_child_eof_accept(before).is_some());
    assert(runtime_pre_pop_child_accept(before).is_some());
    runtime_noscore_child_kind_reads_pushed_summary(before);
    assert(admission::concrete_derived_summary_with_arena(
        state.frames[state.frames.len() - 2], state.frames.last(), state.arena,
    ) == Some(kind));
    assert(state.frames.update(state.frames.len() - 1, state.frames.last()) =~= state.frames);
    assert(source_message_finish_bridge(before, result) == Some(before.score));
    reveal(source_noscore_message_finish_ready);
    assert(source_noscore_message_finish_ready(before, result));
    runtime_noscore_emap_ezipper_selected_quiet(before);
    runtime_emap_ezipper_source_steps_quiet(before.score);
    runtime_score_source_edge_exact(before.score, before.score);
    reveal(source_message_finish_ready);
    assert(!source_message_finish_ready(before, result));
    assert(source_message_eof_child_accept(before, result).is_none());
}

pub proof fn source_message_finish_route_reads_complete_summary(
    before: RuntimeFrameImage,
    result: SourceMessageFinishResult,
    after_finish: bridge::ScoreAdmissionBridge,
    image: RuntimeFrameImage,
    popped: admission::AdmissionState,
    summary: admission::AdmissionReturnedKind,
)
    requires
        source_message_finish_bridge(before, result) == Some(after_finish),
        before.score.admission.frames.len() > 1,
        after_finish.admission.frames.len() > 1,
        Some(image) == match source_message_eof_child_accept(before, result) {
            Some(accepted) => Some(accepted),
            None => source_noscore_message_eof_child_accept(before, result),
        },
        admission::concrete_message_driver_step(
            after_finish.admission, admission::ConcreteFrameActionModel::Complete,
        ) == Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state: popped }),
        popped.frames.last().returns.last().kind == summary,
    ensures
        source_message_finish_route(before, result)
            == Some(SourceMessageFinishRoute::Child { kind: summary, image }),
{
    let state = after_finish.admission;
    let child = state.frames.last();
    let parent = state.frames[state.frames.len() - 2];
    let pushed = admission::concrete_derived_summary_with_arena(parent, child, state.arena);
    assert(pushed.is_some());
    assert(admission::admission_complete_child_step(
        state, child.return_id.unwrap(), child.frame_kind, pushed.unwrap(),
    ) == Some(popped));
    assert(popped.frames.last().returns.last().kind == pushed.unwrap());
}

pub proof fn runtime_epm_root_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_epm_root_before()),
        runtime_epm_source_score_finish_selected(
            runtime_epm_root_before(), Seq::empty(),
        ).is_some(),
        match runtime_message_driver_step(
            runtime_epm_root_before(),
            RuntimeMessageEvent::EofEPathMap { snapshot_bytes: Seq::empty() },
        ) {
            Some(RuntimeMessageOutcome::RootReturned { returned }) => {
                &&& returned.remaining_frames == 0
                &&& returned.pre_pop.score.admission.frames[0].score_handle.is_some()
                &&& returned.summary.program_type_id == 31
                &&& returned.summary.complete
            },
            _ => false,
        },
{
    let before = runtime_epm_root_before();
    let state = before.score.admission;
    let root = state.frames[0];
    assert forall|slot: int| 0 <= slot < root.slot_count as int
        implies root.required.contains(slot)
            == (exists|tag: u32| admission::admission_required_descriptor_slot(
                root.program_type_id, tag, slot,
            )) by {
        if slot == 3 {
            assert(admission::admission_required_descriptor_slot(31, 9, 3));
        } else {
            assert forall|tag: u32|
                !#[trigger] admission::admission_required_descriptor_slot(31, tag, slot) by {}
        }
    }
    assert(admission::admission_frame_wf(root));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(bridge::bridge_bool_slot(root, 1, 4) == Some(false));
    assert(root.returns.last().body_bytes == Seq::<u8>::empty());
    assert(match root.returns.last().kind {
        admission::AdmissionReturnedKind::EPathMapSnapshot { .. } => true,
        _ => false,
    });
    assert(bridge::bridge_append_unattached(
        before.score, score::ScoreNode::BytesAtom(Seq::empty()),
    ).is_some());
    bridge::bridge_append_unattached_exact(
        before.score, score::ScoreNode::BytesAtom(Seq::empty()),
    );
    let (after_bytes, bytes_handle) = bridge::bridge_append_unattached(
        before.score, score::ScoreNode::BytesAtom(Seq::empty()),
    ).unwrap();
    assert(bridge::bridge_well_formed(after_bytes));
    bridge::bridge_append_unattached_exact(after_bytes, score::ScoreNode::Integer(0));
    assert(bridge::bridge_append_unattached(
        after_bytes, score::ScoreNode::Integer(0),
    ).is_some());
    let (after_flag, flag_handle) = bridge::bridge_append_unattached(
        after_bytes, score::ScoreNode::Integer(0),
    ).unwrap();
    assert(bridge::bridge_well_formed(after_flag));
    assert(score::child_handles_valid(
        after_flag.transcript.entries, seq![bytes_handle, flag_handle],
    ));
    assert(admission::admission_slots_complete(root));
    assert(admission::admission_family_admissible(root));
    assert(after_flag.admission.frames.last() == root);
    assert(bridge::bridge_append_unattached(
        after_flag, score::ScoreNode::Integer(13),
    ).is_some());
    bridge::bridge_append_unattached_exact(after_flag, score::ScoreNode::Integer(13));
    let (after_constructor, constructor_handle) = bridge::bridge_append_unattached(
        after_flag, score::ScoreNode::Integer(13),
    ).unwrap();
    assert(bridge::bridge_well_formed(after_constructor));
    assert(score::child_handles_valid(
        after_constructor.transcript.entries,
        seq![constructor_handle, bytes_handle, flag_handle],
    ));
    assert(score::kind_valid(
        after_constructor.transcript.entries,
        score::ScoreNode::Node(seq![constructor_handle, bytes_handle, flag_handle]),
    ));
    assert(bytes_handle == 0);
    assert(flag_handle == 1);
    assert(constructor_handle == 2);
    assert(after_constructor.transcript.entries[0].encoded_len == 4);
    assert(after_constructor.transcript.entries[1].encoded_len == 10);
    assert(after_constructor.transcript.entries[2].encoded_len == 10);
    assert(after_constructor.transcript.entries.len() == 3);
    score::node_transcript_exact(
        after_constructor.transcript.entries,
        seq![constructor_handle, bytes_handle, flag_handle],
    );
    assert(seq![constructor_handle, bytes_handle, flag_handle].drop_first()
        == seq![bytes_handle, flag_handle]);
    assert(seq![bytes_handle, flag_handle].drop_first() == seq![flag_handle]);
    assert(seq![flag_handle].drop_first() == Seq::<usize>::empty());
    assert(score::children_encoded_len(
        after_constructor.transcript.entries, Seq::<usize>::empty(),
    ) == 0);
    assert(score::children_encoded_len(
        after_constructor.transcript.entries, seq![flag_handle],
    ) == 10);
    assert(score::children_encoded_len(
        after_constructor.transcript.entries, seq![bytes_handle, flag_handle],
    ) == 14);
    assert(score::children_encoded_len(
        after_constructor.transcript.entries,
        seq![constructor_handle, bytes_handle, flag_handle],
    ) == 24);
    assert(score::children_transcript(
        after_constructor.transcript.entries,
        seq![constructor_handle, bytes_handle, flag_handle],
    ).len() == 24);
    assert(score::kind_transcript(
        after_constructor.transcript.entries,
        score::ScoreNode::Node(seq![constructor_handle, bytes_handle, flag_handle]),
    ).len() == 26);
    assert(bridge::bridge_append_unattached(
        after_constructor,
        score::ScoreNode::Node(seq![constructor_handle, bytes_handle, flag_handle]),
    ).is_some());
    bridge::bridge_append_unattached_exact(
        after_constructor,
        score::ScoreNode::Node(seq![constructor_handle, bytes_handle, flag_handle]),
    );
    let (after_node, score_handle) = bridge::bridge_append_unattached(
        after_constructor,
        score::ScoreNode::Node(seq![constructor_handle, bytes_handle, flag_handle]),
    ).unwrap();
    assert(admission::admission_attach_score_step(
        after_node.admission, score_handle, root.body_id,
    ).is_some());
    assert(seq![constructor_handle] + seq![bytes_handle, flag_handle]
        == seq![constructor_handle, bytes_handle, flag_handle]);
    assert(bridge::bridge_constructor_step(
        after_flag, 13i32, seq![bytes_handle, flag_handle],
    ).is_some());
    assert(runtime_epm_source_score_finish_selected(before, Seq::empty()).is_some());
    let after = runtime_epm_source_score_finish_selected(before, Seq::empty()).unwrap();
    assert(admission::admission_state_wf(after.admission));
    assert(admission::admission_complete_root(after.admission));
    assert(admission::concrete_message_driver_step(
        after.admission, admission::ConcreteFrameActionModel::Complete,
    ).is_some());
    assert(runtime_epm_source_eof(before, Seq::empty()).is_some());
}

pub proof fn runtime_epm_child_modeled_witness()
    ensures
        runtime_frame_image_wf(runtime_epm_child_before()),
        runtime_epm_source_score_finish_selected(
            runtime_epm_child_before(), Seq::empty(),
        ).is_some(),
        match runtime_message_driver_step(
            runtime_epm_child_before(),
            RuntimeMessageEvent::EofEPathMap { snapshot_bytes: Seq::empty() },
        ) {
            Some(RuntimeMessageOutcome::ParentAccepted { image }) => {
                &&& image.score.admission.frames.len() == 1
                &&& image.score.admission.completed == 1
                &&& image.score.admission.frames[0].waiting_id.is_none()
                &&& image.score.admission.frames[0].returns.last().kind
                    == admission::AdmissionReturnedKind::ExprPayload {
                        score_present: true,
                        canonical_fixed_point: true,
                        canonical_path_stable: true,
                    }
            },
            _ => false,
        },
{
    runtime_epm_root_modeled_witness();
    let before = runtime_epm_child_before();
    let state = before.score.admission;
    let parent = state.frames[0];
    let child = state.frames[1];
    assert(admission::admission_frame_wf(child));
    assert(admission::admission_frame_wf(parent));
    assert(parent.source_bytes.subrange(0, 0) == Seq::<u8>::empty());
    assert(admission::admission_child_physical_contained(parent, child));
    assert(admission::admission_state_wf(state));
    score::empty_arena_well_formed();
    assert(bridge::bridge_well_formed(before.score));
    assert(runtime_frame_image_wf(before));
    assert(runtime_epm_source_score_finish_selected(before, Seq::empty()).is_some());
    runtime_epm_source_score_finish_is_checked(before, Seq::empty());
    let after = runtime_epm_source_score_finish_selected(before, Seq::empty()).unwrap();
    assert(admission::admission_state_wf(after.admission));
    assert(admission::admission_epathmap_path_stability(after.admission.frames[1]) == Some(true));
    assert(admission::concrete_derived_summary(
        after.admission.frames[0], after.admission.frames[1],
    ) == Some(admission::AdmissionReturnedKind::ExprPayload {
        score_present: true,
        canonical_fixed_point: true,
        canonical_path_stable: true,
    }));
    assert(runtime_frame_image_wf(RuntimeFrameImage { score: after, ..before }));
    assert(match admission::concrete_message_driver_step(
        after.admission, admission::ConcreteFrameActionModel::Complete,
    ) {
        Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { .. }) => true,
        _ => false,
    });
    assert(before.score.admission.frames.len() == 2);
    let pre_pop = RuntimeFrameImage { score: after, ..before };
    assert(runtime_frame_image_wf(pre_pop));
    assert(pre_pop.score.admission.frames.len() == 2);
    assert(match admission::concrete_message_driver_step(
        pre_pop.score.admission, admission::ConcreteFrameActionModel::Complete,
    ) {
        Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { .. }) => true,
        _ => false,
    });
    reveal(runtime_pre_pop_child_accept);
    assert(runtime_pre_pop_child_accept(pre_pop).is_some());
    assert(runtime_epm_source_eof(before, Seq::empty()).is_some());
}

}

fn main() {}
