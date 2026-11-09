//! Retained CanonicalPath identity over the integrated live admission product.
//!
//! This module is included by the integrated Verus root. Its source-shaped
//! inputs describe checked production observations; no theorem here extracts
//! them from arbitrary `CanonicalPathFrame` or `Vec<Frame>` executions.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission,
    rpb3_canonical_path_driver_source_refinement as path,
    rpb3_epathmap_event_refinement as epm,
    rpb3_map_frame_refinement as map,
    rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as bridge,
    rpb3_score_transcript as score,
    rpb3_wire_cursor_source_refinement as wire,
};

verus! {

/// The outstanding source is retained on the kind-4 parent while an immediate
/// child (and possibly descendants of that child) is on the worklist. The
/// reducer obligation ID and stack return ID remain distinct fields.
pub struct LiveCanonicalPathState {
    pub ghost base: crate::LiveRuntimeProductState,
    pub ghost path_frames: Seq<Option<path::CanonicalPathFrameImage>>,
    pub ghost outstanding: Seq<Option<admission::AdmissionCanonicalPathPushSource>>,
}

pub open spec fn path_obligation_kind(
    kind: admission::AdmissionCanonicalPathObligationKind,
) -> epm::PathObligationKind {
    match kind {
        admission::AdmissionCanonicalPathObligationKind::RawParEscape =>
            epm::PathObligationKind::RawParEscape,
        admission::AdmissionCanonicalPathObligationKind::NestedEpm1 =>
            epm::PathObligationKind::NestedEpm1,
    }
}

pub open spec fn path_child_kind(
    kind: admission::AdmissionCanonicalPathObligationKind,
) -> runtime::SourceFrameRequestKind {
    match kind {
        admission::AdmissionCanonicalPathObligationKind::RawParEscape =>
            runtime::SourceFrameRequestKind::Message,
        admission::AdmissionCanonicalPathObligationKind::NestedEpm1 =>
            runtime::SourceFrameRequestKind::EPathMap,
    }
}

pub open spec fn indexed_from_admission(
    input: admission::AdmissionIndexedSourceImage,
) -> epm::IndexedRegionImage {
    epm::IndexedRegionImage {
        bytes: input.bytes,
        positions: Seq::new(input.positions.len(), |i: int| epm::SourceAnchor {
            source: input.positions[i].source,
            offset: input.positions[i].offset,
        }),
        end: epm::SourceAnchor { source: input.end.source, offset: input.end.offset },
    }
}

pub proof fn indexed_from_admission_is_exact(
    input: admission::AdmissionIndexedSourceImage,
)
    requires admission::admission_indexed_source_wf(input),
    ensures
        epm::indexed_image_wf(indexed_from_admission(input)),
        crate::product_admission_source_image(indexed_from_admission(input)) == input,
{
    let image = indexed_from_admission(input);
    assert(image.positions.len() == input.positions.len());
    assert forall|i: int| 0 <= i < input.positions.len()
        implies crate::product_admission_source_anchor(image.positions[i])
            == input.positions[i] by {}
    assert(crate::product_admission_source_image(image).positions == input.positions);
}

pub open spec fn path_frame_matches_live(
    state: LiveCanonicalPathState, i: int,
) -> bool {
    let frame = state.base.source.product.bridge.admission.frames[i];
    match state.path_frames[i] {
        None => frame.frame_kind != 4 && state.outstanding[i].is_none(),
        Some(path_frame) => {
            &&& frame.frame_kind == 4
            &&& frame.program_type_id == 0
            &&& admission::admission_canonical_path_slotless(frame)
            &&& path::canonical_path_frame_wf(path_frame)
            &&& path_frame.input == state.base.source.product.indexed_regions[i]
            &&& frame.cursor == path_frame.cursor
            &&& state.base.source.wire[i].is_none()
            &&& state.base.cursors[i].is_none()
            &&& match (path_frame.waiting, state.outstanding[i]) {
                (None, None) => frame.waiting_id.is_none() && frame.waiting_kind == 0,
                (Some(wait), Some(push)) => {
                    &&& i + 1 < state.path_frames.len()
                    &&& push.parent == crate::product_admission_source_image(path_frame.input)
                    &&& push.cursor_after_step == path_frame.cursor
                    &&& push.reducer_obligation_id == wait.id
                    &&& push.stack_return_id == wait.return_id
                    &&& path_obligation_kind(push.kind) == wait.kind
                    &&& frame.waiting_id == Some(push.stack_return_id)
                    &&& frame.waiting_kind
                        == runtime::source_frame_request_kind_code(path_child_kind(push.kind))
                    &&& state.base.source.product.bridge.admission.frames[i + 1].return_id
                        == Some(push.stack_return_id)
                    &&& state.base.source.product.bridge.admission.frames[i + 1].frame_kind
                        == runtime::source_frame_request_kind_code(path_child_kind(push.kind))
                    &&& crate::product_admission_source_image(
                        state.base.source.product.indexed_regions[i + 1],
                    ) == push.child
                },
                _ => false,
            }
        },
    }
}

pub open spec fn live_canonical_path_wf(state: LiveCanonicalPathState) -> bool {
    let n = state.base.source.product.bridge.admission.frames.len();
    &&& crate::live_runtime_product_wf(state.base)
    &&& state.path_frames.len() == n
    &&& state.outstanding.len() == n
    &&& forall|i: int| 0 <= i < n ==> path_frame_matches_live(state, i)
}

/// This constructor uses the kind-4 root source observation and builds every
/// modeled product layer. It does not use the kind-1 Message root predicate.
pub open spec fn live_canonical_path_root_from_source(
    source: admission::AdmissionCanonicalPathRootSource,
) -> Option<LiveCanonicalPathState> {
    if !admission::admission_canonical_path_root_source_wf(source) {
        None
    } else {
        let root = admission::admission_canonical_path_root_projected_frame(source);
        let state = admission::admission_initial_state(root);
        let transcript = score::empty_arena();
        let score_bridge = bridge::ScoreAdmissionBridge {
            admission: state, transcript,
        };
        let product = crate::ProductScoreState {
            bridge: score_bridge,
            map: map::IntegratedAdmissionState {
                admission: state,
                map_frames: seq![None],
                transcript,
            },
            epm_frames: seq![None],
            indexed_regions: seq![indexed_from_admission(source.indexed_input)],
            accepted_epm_sources: seq![None],
        };
        let runtime = runtime::RuntimeFrameImage {
            score: score_bridge,
            capacity: source.stack_capacity_after,
            frame_bytes: source.frame_variant_bytes,
            max_frames: 1,
            max_storage_bytes: source.used_bytes,
            max_capacity_bytes: source.reserved_bytes,
        };
        let base = crate::LiveRuntimeProductState {
            source: crate::LiveSourceProductState {
                product,
                runtime,
                arena: crate::product_runtime_arena_from_score(transcript),
                wire: seq![None],
            },
            cursors: seq![None],
        };
        let path_frame = path::CanonicalPathFrameImage {
            input: indexed_from_admission(source.indexed_input),
            cursor: 0,
            pending: None,
            waiting: None,
        };
        Some(LiveCanonicalPathState {
            base,
            path_frames: seq![Some(path_frame)],
            outstanding: seq![None],
        })
    }
}

pub proof fn live_canonical_path_root_is_wf(
    source: admission::AdmissionCanonicalPathRootSource,
)
    requires admission::admission_canonical_path_root_source_wf(source),
    ensures match live_canonical_path_root_from_source(source) {
        Some(root) => {
            &&& live_canonical_path_wf(root)
            &&& root.base.source.product.bridge.admission.frames.len() == 1
            &&& root.base.source.product.bridge.admission.frames[0].frame_kind == 4
            &&& root.outstanding[0].is_none()
            &&& root.base.source.product.bridge.admission.arena.len() == 0
        },
        None => false,
    },
{
    admission::admission_canonical_path_root_source_derives_initial_wf(source);
    indexed_from_admission_is_exact(source.indexed_input);
    score::empty_arena_well_formed();
    crate::product_runtime_arena_from_score_projects(score::empty_arena());
    let root = live_canonical_path_root_from_source(source).unwrap();
    assert(admission::admission_state_wf(root.base.source.product.bridge.admission));
    assert(bridge::bridge_well_formed(root.base.source.product.bridge));
    assert(map::integrated_admission_wf(root.base.source.product.map));
    assert(crate::product_frame_source_anchors_match(root.base.source.product));
    assert(crate::product_epm_source_receipts_wf(root.base.source.product));
    assert(crate::product_score_wf(root.base.source.product));
    assert(runtime::runtime_frame_image_wf(root.base.source.runtime));
    assert(crate::live_source_product_wf(root.base.source));
    assert(crate::live_runtime_product_wf(root.base));
    assert(path_frame_matches_live(root, 0));
}

pub open spec fn path_push_scanned_frame(
    frame: path::CanonicalPathFrameImage,
    source: admission::AdmissionCanonicalPathPushSource,
) -> path::CanonicalPathFrameImage {
    path::CanonicalPathFrameImage {
        cursor: source.cursor_after_step,
        pending: Some(path::CanonicalPathPendingImage {
            id: source.reducer_obligation_id,
            kind: path_obligation_kind(source.kind),
            span: epm::ByteSpan {
                start: source.payload_start, end: source.payload_end,
            },
            end: epm::SourceAnchor {
                source: source.terminal_anchor.source,
                offset: source.terminal_anchor.offset,
            },
        }),
        waiting: None,
        ..frame
    }
}

pub open spec fn path_push_waiting_frame(
    frame: path::CanonicalPathFrameImage,
    source: admission::AdmissionCanonicalPathPushSource,
) -> path::CanonicalPathFrameImage {
    path::CanonicalPathFrameImage {
        waiting: Some(path::CanonicalPathWaitImage {
            id: source.reducer_obligation_id,
            kind: path_obligation_kind(source.kind),
            return_id: source.stack_return_id,
        }),
        ..path_push_scanned_frame(frame, source)
    }
}

pub open spec fn path_push_storage_image(
    source: admission::AdmissionCanonicalPathPushStorage,
    kind: admission::AdmissionCanonicalPathObligationKind,
) -> runtime::SourceFramePushStorageImage {
    runtime::SourceFramePushStorageImage {
        request_kind: path_child_kind(kind),
        before_len: source.before_len,
        after_len: source.after_len,
        before_capacity: source.before_capacity,
        after_capacity: source.after_capacity,
        frame_variant_bytes: source.frame_variant_bytes,
        stats_frame_variant_bytes: source.stats_frame_variant_bytes,
        used_bytes: source.used_bytes,
        reserved_bytes: source.reserved_bytes,
    }
}

/// The supplied child wire cursor is an observed initial protobuf reader for
/// RawParEscape. NestedEpm1 has no protobuf reader; its EPM topology cursor is
/// established by the subsequent internal EPM handoff.
pub open spec fn path_push_child_reader_wf(
    child: admission::AdmissionFrameState,
    image: epm::IndexedRegionImage,
    cursor: Option<wire::WireRuntimeCursorImage>,
) -> bool {
    if child.frame_kind == 1 {
        match cursor {
            None => false,
            Some(reader) => {
                let concrete = wire::wire_runtime_derive_cursor(reader);
                let projected = wire::wire_runtime_cursor_project(reader);
                &&& concrete.is_some()
                &&& wire::wire_concrete_cursor_wf(concrete.unwrap())
                &&& projected.is_some()
                &&& wire::wire_cursor_shape(projected.unwrap().cursor)
                &&& projected.unwrap().cursor.region_start == child.region_start
                &&& projected.unwrap().cursor.region_end == child.region_end
                &&& projected.unwrap().cursor.next == child.cursor
                &&& projected.unwrap().cursor.source_bytes == child.source_bytes
                &&& projected.unwrap().terminal_error.is_none()
                &&& crate::product_wire_indexed_source_agrees(reader, image)
                &&& reader.reader_at == 0
                &&& reader.cursor_yielded == 0
                &&& reader.fields_read == 0
                &&& reader.terminal_error.is_none()
            },
        }
    } else {
        &&& child.frame_kind == 3
        &&& cursor.is_none()
    }
}

/// The driver action begins before CanonicalPath's internal byte loop. The
/// `scanned` image is the source-observed cursor/pending payload at the moment
/// `step` returns Push; the checked Vec observation and indexed child are
/// required separately. No final product or sidecar state is an input.
pub open spec fn live_canonical_path_push_source_ready(
    before: LiveCanonicalPathState,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
) -> bool {
    if !live_canonical_path_wf(before)
        || before.path_frames.len() == 0
        || before.path_frames.last().is_none()
    { false } else {
        let old_path = before.path_frames.last().unwrap();
        let scanned = path_push_scanned_frame(old_path, source);
        let waiting = path_push_waiting_frame(old_path, source);
        let request = path::canonical_path_selected_child(scanned, source.stack_return_id);
        &&& old_path.waiting.is_none()
        &&& before.outstanding.last().is_none()
        &&& admission::admission_canonical_path_push_source_wf(
            before.base.source.product.bridge.admission, source, child,
        )
        &&& admission::admission_canonical_path_push_storage_wf(
            before.base.source.product.bridge.admission, child, storage,
        )
        &&& source.parent == crate::product_admission_source_image(old_path.input)
        &&& source.child == crate::product_admission_source_image(child_image)
        &&& epm::indexed_image_wf(child_image)
        &&& path_push_child_reader_wf(child, child_image, child_reader)
        &&& runtime::source_successful_push_storage(
            before.base.source.runtime,
            path_push_storage_image(storage, source.kind),
        )
        &&& request.is_some()
        &&& request.unwrap().kind == path_child_kind(source.kind)
        &&& request.unwrap().obligation_id == source.reducer_obligation_id
        &&& request.unwrap().return_id == source.stack_return_id
        &&& request.unwrap().body == child_image
        &&& path::canonical_path_observed_push_step(
            scanned, waiting, before.base.source.runtime, request.unwrap(),
            Some(path_child_kind(source.kind)), storage.after_len,
            storage.after_capacity, storage.stats_frame_variant_bytes,
        )
        &&& admission::admission_canonical_path_push_indexed_step(
            before.base.source.product.bridge.admission, source, child, storage,
        ).is_some()
    }
}

pub open spec fn live_canonical_path_push_from_source(
    before: LiveCanonicalPathState,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
) -> Option<LiveCanonicalPathState> {
    if !live_canonical_path_push_source_ready(
        before, source, child, storage, child_image, child_reader,
    ) { None } else {
        let old_product = before.base.source.product;
        let old_runtime = before.base.source.runtime;
        let next = admission::admission_canonical_path_push_indexed_step(
            old_product.bridge.admission, source, child, storage,
        ).unwrap();
        let next_bridge = bridge::ScoreAdmissionBridge {
            admission: next,
            transcript: old_product.bridge.transcript,
        };
        let next_product = crate::ProductScoreState {
            bridge: next_bridge,
            map: map::IntegratedAdmissionState {
                admission: next,
                map_frames: old_product.map.map_frames.push(None),
                transcript: old_product.map.transcript,
            },
            epm_frames: old_product.epm_frames.push(None),
            indexed_regions: old_product.indexed_regions.push(child_image),
            accepted_epm_sources: old_product.accepted_epm_sources.push(None),
        };
        let next_runtime = runtime::RuntimeFrameImage {
            score: next_bridge,
            capacity: storage.after_capacity,
            frame_bytes: old_runtime.frame_bytes,
            max_frames: if old_runtime.max_frames as int >= next.frames.len() {
                old_runtime.max_frames
            } else { next.frames.len() as usize },
            max_storage_bytes: if old_runtime.max_storage_bytes >= storage.used_bytes {
                old_runtime.max_storage_bytes
            } else { storage.used_bytes },
            max_capacity_bytes: if old_runtime.max_capacity_bytes >= storage.reserved_bytes {
                old_runtime.max_capacity_bytes
            } else { storage.reserved_bytes },
        };
        let child_source_reader = match child_reader {
            None => None,
            Some(reader) => wire::wire_runtime_cursor_project(reader),
        };
        let next_base = crate::LiveRuntimeProductState {
            source: crate::LiveSourceProductState {
                product: next_product,
                runtime: next_runtime,
                arena: before.base.source.arena,
                wire: before.base.source.wire.push(child_source_reader),
            },
            cursors: before.base.cursors.push(child_reader),
        };
        let parent_index = before.path_frames.len() - 1;
        let old_path = before.path_frames[parent_index].unwrap();
        Some(LiveCanonicalPathState {
            base: next_base,
            path_frames: before.path_frames.update(
                parent_index, Some(path_push_waiting_frame(old_path, source)),
            ).push(None),
            outstanding: before.outstanding.update(parent_index, Some(source)).push(None),
        })
    }
}

/// Indexed Push edits only the top path frame and appends one child. Every
/// older admission frame, indexed image, wire view, and retained path source
/// is byte-for-byte unchanged by the selector.
pub proof fn live_canonical_path_push_preserves_ancestors(
    before: LiveCanonicalPathState,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
)
    requires live_canonical_path_push_source_ready(
        before, source, child, storage, child_image, child_reader,
    ),
    ensures {
        let after = live_canonical_path_push_from_source(
            before, source, child, storage, child_image, child_reader,
        ).unwrap();
        let n = before.path_frames.len();
        &&& forall|i: int| 0 <= i < n - 1 ==>
            after.base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]
            && after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]
            && after.base.source.wire[i] == before.base.source.wire[i]
            && after.base.cursors[i] == before.base.cursors[i]
            && after.path_frames[i] == before.path_frames[i]
            && after.outstanding[i] == before.outstanding[i]
    },
{
    let old = before.base.source.product.bridge.admission;
    admission::admission_canonical_path_push_indexed_preserves_wf(
        old, source, child, storage,
    );
    let after = live_canonical_path_push_from_source(
        before, source, child, storage, child_image, child_reader,
    ).unwrap();
    let n = before.path_frames.len();
    assert forall|i: int| 0 <= i < n - 1 implies
        after.base.source.product.bridge.admission.frames[i] == old.frames[i]
        && after.base.source.product.indexed_regions[i]
            == before.base.source.product.indexed_regions[i]
        && after.base.source.wire[i] == before.base.source.wire[i]
        && after.base.cursors[i] == before.base.cursors[i]
        && after.path_frames[i] == before.path_frames[i]
        && after.outstanding[i] == before.outstanding[i] by {}
}

pub proof fn live_canonical_path_push_preserves_wf(
    before: LiveCanonicalPathState,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
)
    requires live_canonical_path_push_source_ready(
        before, source, child, storage, child_image, child_reader,
    ),
    ensures match live_canonical_path_push_from_source(
        before, source, child, storage, child_image, child_reader,
    ) {
        Some(after) => {
            &&& live_canonical_path_wf(after)
            &&& after.outstanding[before.outstanding.len() - 1] == Some(source)
            &&& after.base.source.product.bridge.admission.frames.len()
                == before.base.source.product.bridge.admission.frames.len() + 1
            &&& after.base.source.product.bridge.admission.arena
                == before.base.source.product.bridge.admission.arena
            &&& after.base.source.product.bridge.transcript
                == before.base.source.product.bridge.transcript
        },
        None => false,
    },
{
    let old = before.base.source.product.bridge.admission;
    let next = admission::admission_canonical_path_push_indexed_step(
        old, source, child, storage,
    ).unwrap();
    admission::admission_canonical_path_push_indexed_preserves_wf(
        old, source, child, storage,
    );
    let old_path = before.path_frames.last().unwrap();
    let scanned = path_push_scanned_frame(old_path, source);
    let waiting = path_push_waiting_frame(old_path, source);
    let request = path::canonical_path_selected_child(scanned, source.stack_return_id).unwrap();
    path::canonical_path_observed_push_is_exact(
        scanned, waiting, before.base.source.runtime, request,
        Some(path_child_kind(source.kind)), storage.after_len,
        storage.after_capacity, storage.stats_frame_variant_bytes,
    );
    live_canonical_path_push_preserves_ancestors(
        before, source, child, storage, child_image, child_reader,
    );
    let after = live_canonical_path_push_from_source(
        before, source, child, storage, child_image, child_reader,
    ).unwrap();
    assert(admission::admission_state_wf(next));
    assert(bridge::bridge_well_formed(after.base.source.product.bridge));
    assert(map::integrated_admission_wf(after.base.source.product.map));
    assert(crate::product_frame_source_anchors_match(after.base.source.product));
    assert(crate::product_epm_source_receipts_wf(after.base.source.product));
    assert(crate::product_score_wf(after.base.source.product));
    assert(runtime::runtime_frame_image_wf(after.base.source.runtime));
    assert(crate::live_source_product_wf(after.base.source));
    assert(crate::live_runtime_product_wf(after.base));
    let n = before.path_frames.len();
    assert(after.path_frames.len() == n + 1);
    assert(after.outstanding.len() == n + 1);
    assert forall|i: int| 0 <= i < after.path_frames.len()
        implies path_frame_matches_live(after, i) by {
        if i < n - 1 {
            assert(path_frame_matches_live(before, i));
            assert(after.path_frames[i] == before.path_frames[i]);
            assert(after.outstanding[i] == before.outstanding[i]);
            assert(after.base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]);
            assert(after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]);
            assert(after.base.source.wire[i] == before.base.source.wire[i]);
            assert(after.base.cursors[i] == before.base.cursors[i]);
            if before.path_frames[i].is_some() && before.outstanding[i].is_some() {
                if i + 1 < n - 1 {
                    assert(after.base.source.product.bridge.admission.frames[i + 1]
                        == before.base.source.product.bridge.admission.frames[i + 1]);
                } else {
                    assert(i + 1 == n - 1);
                    assert(after.base.source.product.bridge.admission.frames[i + 1].return_id
                        == before.base.source.product.bridge.admission.frames[i + 1].return_id);
                    assert(after.base.source.product.bridge.admission.frames[i + 1].frame_kind
                        == before.base.source.product.bridge.admission.frames[i + 1].frame_kind);
                }
                assert(after.base.source.product.indexed_regions[i + 1]
                    == before.base.source.product.indexed_regions[i + 1]);
            }
            assert(path_frame_matches_live(after, i));
        } else if i == n - 1 {
            assert(path_frame_matches_live(before, i));
            assert(before.path_frames[i] == Some(old_path));
            assert(after.outstanding[i] == Some(source));
            assert(after.path_frames[i] == Some(waiting));
            assert(path::canonical_path_frame_wf(waiting));
            assert(waiting.input == old_path.input);
            assert(waiting.cursor == source.cursor_after_step);
            assert(waiting.waiting == Some(path::CanonicalPathWaitImage {
                id: source.reducer_obligation_id,
                kind: path_obligation_kind(source.kind),
                return_id: source.stack_return_id,
            }));
            assert(after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]);
            assert(waiting.input == after.base.source.product.indexed_regions[i]);
            assert(source.parent == crate::product_admission_source_image(waiting.input));
            assert(after.base.source.product.bridge.admission.frames[i].cursor
                == source.cursor_after_step);
            assert(after.base.source.product.bridge.admission.frames[i].waiting_id
                == Some(source.stack_return_id));
            assert(after.base.source.product.bridge.admission.frames[i].waiting_kind
                == child.frame_kind);
            assert(after.base.source.product.bridge.admission.frames[i].frame_kind == 4);
            assert(after.base.source.product.bridge.admission.frames[i].program_type_id == 0);
            assert(admission::admission_canonical_path_slotless(
                after.base.source.product.bridge.admission.frames[i],
            ));
            match source.kind {
                admission::AdmissionCanonicalPathObligationKind::RawParEscape => {
                    assert(child.frame_kind == 1);
                },
                admission::AdmissionCanonicalPathObligationKind::NestedEpm1 => {
                    assert(child.frame_kind == 3);
                },
            }
            assert(after.base.source.product.bridge.admission.frames[i].waiting_kind
                == runtime::source_frame_request_kind_code(path_child_kind(source.kind)));
            assert(after.base.source.wire[i].is_none());
            assert(after.base.cursors[i].is_none());
            assert(after.base.source.product.bridge.admission.frames[i + 1].return_id
                == Some(source.stack_return_id));
            assert(after.base.source.product.bridge.admission.frames[i + 1].frame_kind
                == child.frame_kind);
            assert(crate::product_admission_source_image(
                after.base.source.product.indexed_regions[i + 1],
            ) == source.child);
            assert(waiting.waiting.unwrap().id == source.reducer_obligation_id);
            assert(waiting.waiting.unwrap().kind == path_obligation_kind(source.kind));
            assert(waiting.waiting.unwrap().return_id == source.stack_return_id);
            assert(path_frame_matches_live(after, i));
        } else {
            assert(i == n);
            assert(after.path_frames[i].is_none());
            assert(after.outstanding[i].is_none());
            assert(after.base.source.product.bridge.admission.frames[i].frame_kind
                == child.frame_kind);
            assert(child.frame_kind == 1 || child.frame_kind == 3);
            assert(path_frame_matches_live(after, i));
        }
    }
    assert(live_canonical_path_wf(after));
}

/// The scalar reducer observation is separate from the retained indexed Push
/// source. The latter is always read from the live sidecar, never supplied by
/// the caller of the return transition.
pub struct LiveCanonicalPathReturnObservation {
    pub reducer_waiting_id: u64,
    pub reducer_waiting_kind: admission::AdmissionCanonicalPathObligationKind,
    pub stack_return_id: u64,
    pub ghost epm_entry_stability: Seq<bool>,
    pub ghost summary: admission::AdmissionReturnedKind,
}

pub struct LiveCanonicalPathReturnResult {
    pub ghost state: LiveCanonicalPathState,
    pub ghost resolved: admission::AdmissionCanonicalPathReturnAfter,
}

pub open spec fn live_canonical_path_retained_return_source(
    before: LiveCanonicalPathState,
    observed: LiveCanonicalPathReturnObservation,
) -> admission::AdmissionCanonicalPathReturnSource {
    let parent_index = before.outstanding.len() - 2;
    admission::AdmissionCanonicalPathReturnSource {
        push: before.outstanding[parent_index].unwrap(),
        reducer_waiting_id: observed.reducer_waiting_id,
        reducer_waiting_kind: observed.reducer_waiting_kind,
        stack_return_id: observed.stack_return_id,
        epm_entry_stability: observed.epm_entry_stability,
    }
}

pub open spec fn live_canonical_path_resolved_frame(
    parent: path::CanonicalPathFrameImage,
) -> path::CanonicalPathFrameImage {
    path::CanonicalPathFrameImage {
        pending: None,
        waiting: None,
        ..parent
    }
}

/// This guard is source-shaped: it checks the observed reducer IDs/summary
/// against the stored Push, the completed child, and the core indexed return
/// selector. It does not infer the observation from an arbitrary Rust run.
pub open spec fn live_canonical_path_return_source_ready(
    before: LiveCanonicalPathState,
    observed: LiveCanonicalPathReturnObservation,
) -> bool {
    if !live_canonical_path_wf(before)
        || before.path_frames.len() < 2
        || before.path_frames[before.path_frames.len() - 2].is_none()
        || before.outstanding[before.outstanding.len() - 2].is_none()
    { false } else {
        let parent_index = before.path_frames.len() - 2;
        let parent_path = before.path_frames[parent_index].unwrap();
        let source = live_canonical_path_retained_return_source(before, observed);
        &&& parent_path.waiting.is_some()
        &&& parent_path.pending.is_some()
        &&& observed.reducer_waiting_id == parent_path.waiting.unwrap().id
        &&& observed.stack_return_id == parent_path.waiting.unwrap().return_id
        &&& path_obligation_kind(observed.reducer_waiting_kind)
            == parent_path.waiting.unwrap().kind
        &&& admission::admission_canonical_path_return_source_wf(
            before.base.source.product.bridge.admission,
            source,
            observed.summary,
        )
        &&& admission::admission_canonical_path_return_sidecar_after(
            source, observed.summary,
        ).is_some()
        &&& admission::admission_canonical_path_return_indexed_step(
            before.base.source.product.bridge.admission,
            source,
            observed.summary,
        ).is_some()
    }
}

/// Deterministic return: the core selector produces the next admission state;
/// each integrated sidecar drops precisely its completed child entry. The
/// parent indexed image and its physical anchors are carried unchanged.
pub open spec fn live_canonical_path_return_from_source(
    before: LiveCanonicalPathState,
    observed: LiveCanonicalPathReturnObservation,
) -> Option<LiveCanonicalPathReturnResult> {
    if !live_canonical_path_return_source_ready(before, observed) { None } else {
        let old_product = before.base.source.product;
        let old_admission = old_product.bridge.admission;
        let parent_index = old_admission.frames.len() - 2;
        let source = live_canonical_path_retained_return_source(before, observed);
        let next = admission::admission_canonical_path_return_indexed_step(
            old_admission, source, observed.summary,
        ).unwrap();
        let resolved = admission::admission_canonical_path_return_sidecar_after(
            source, observed.summary,
        ).unwrap();
        let next_bridge = bridge::ScoreAdmissionBridge {
            admission: next,
            transcript: old_product.bridge.transcript,
        };
        let next_product = crate::ProductScoreState {
            bridge: next_bridge,
            map: map::IntegratedAdmissionState {
                admission: next,
                map_frames: old_product.map.map_frames.subrange(0, parent_index + 1),
                transcript: old_product.map.transcript,
            },
            epm_frames: old_product.epm_frames.subrange(0, parent_index + 1),
            indexed_regions: old_product.indexed_regions.subrange(0, parent_index + 1),
            accepted_epm_sources: old_product.accepted_epm_sources.subrange(
                0, parent_index + 1,
            ),
        };
        let next_base = crate::LiveRuntimeProductState {
            source: crate::LiveSourceProductState {
                product: next_product,
                runtime: runtime::runtime_after_admission_step(
                    before.base.source.runtime, next,
                ),
                arena: before.base.source.arena,
                wire: before.base.source.wire.subrange(0, parent_index + 1),
            },
            cursors: before.base.cursors.subrange(0, parent_index + 1),
        };
        let parent_path = before.path_frames[parent_index].unwrap();
        Some(LiveCanonicalPathReturnResult {
            state: LiveCanonicalPathState {
                base: next_base,
                path_frames: before.path_frames.subrange(0, parent_index + 1).update(
                    parent_index, Some(live_canonical_path_resolved_frame(parent_path)),
                ),
                outstanding: before.outstanding.subrange(0, parent_index + 1).update(
                    parent_index, None,
                ),
            },
            resolved,
        })
    }
}

pub proof fn live_canonical_path_return_preserves_ancestors(
    before: LiveCanonicalPathState,
    observed: LiveCanonicalPathReturnObservation,
)
    requires live_canonical_path_return_source_ready(before, observed),
    ensures {
        let result = live_canonical_path_return_from_source(before, observed).unwrap();
        let after = result.state;
        let parent_index = before.path_frames.len() - 2;
        &&& after.path_frames.len() + 1 == before.path_frames.len()
        &&& after.outstanding.len() + 1 == before.outstanding.len()
        &&& after.base.source.product.bridge.admission.frames.len() + 1
            == before.base.source.product.bridge.admission.frames.len()
        &&& after.base.source.product.indexed_regions[parent_index]
            == before.base.source.product.indexed_regions[parent_index]
        &&& forall|i: int| 0 <= i < parent_index ==>
            after.base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]
            && after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]
            && after.base.source.wire[i] == before.base.source.wire[i]
            && after.base.cursors[i] == before.base.cursors[i]
            && after.path_frames[i] == before.path_frames[i]
            && after.outstanding[i] == before.outstanding[i]
    },
{
    let old = before.base.source.product.bridge.admission;
    let source = live_canonical_path_retained_return_source(before, observed);
    admission::admission_canonical_path_return_indexed_preserves_wf(
        old, source, observed.summary,
    );
    let after = live_canonical_path_return_from_source(before, observed).unwrap().state;
    let parent_index = before.path_frames.len() - 2;
    assert forall|i: int| 0 <= i < parent_index implies
        after.base.source.product.bridge.admission.frames[i] == old.frames[i]
        && after.base.source.product.indexed_regions[i]
            == before.base.source.product.indexed_regions[i]
        && after.base.source.wire[i] == before.base.source.wire[i]
        && after.base.cursors[i] == before.base.cursors[i]
        && after.path_frames[i] == before.path_frames[i]
        && after.outstanding[i] == before.outstanding[i] by {}
}

pub proof fn live_canonical_path_return_preserves_wf(
    before: LiveCanonicalPathState,
    observed: LiveCanonicalPathReturnObservation,
)
    requires live_canonical_path_return_source_ready(before, observed),
    ensures match live_canonical_path_return_from_source(before, observed) {
        Some(result) => {
            let after = result.state;
            let parent_index = before.path_frames.len() - 2;
            let source = live_canonical_path_retained_return_source(before, observed);
            &&& live_canonical_path_wf(after)
            &&& after.outstanding[parent_index].is_none()
            &&& after.path_frames[parent_index].unwrap().waiting.is_none()
            &&& after.path_frames[parent_index].unwrap().pending.is_none()
            &&& after.base.source.product.bridge.admission.frames[parent_index]
                .waiting_id.is_none()
            &&& after.base.source.product.indexed_regions[parent_index]
                == before.base.source.product.indexed_regions[parent_index]
            &&& after.base.source.product.bridge.admission.arena
                == before.base.source.product.bridge.admission.arena
            &&& after.base.source.product.bridge.transcript
                == before.base.source.product.bridge.transcript
            &&& after.base.source.arena == before.base.source.arena
            &&& result.resolved.parent == source.push.parent
            &&& result.resolved.resolved_reducer_id == source.push.reducer_obligation_id
            &&& match result.resolved.resolution {
                admission::AdmissionCanonicalPathResolution::RawPar {
                    canonical_path_stable,
                } => source.push.kind
                    == admission::AdmissionCanonicalPathObligationKind::RawParEscape
                    && !canonical_path_stable,
                admission::AdmissionCanonicalPathResolution::NestedEpm1 {
                    mode, entries_canonical_path_stable,
                } => source.push.kind
                    == admission::AdmissionCanonicalPathObligationKind::NestedEpm1
                    && mode == 2 && entries_canonical_path_stable,
            }
        },
        None => false,
    },
{
    let old = before.base.source.product.bridge.admission;
    let source = live_canonical_path_retained_return_source(before, observed);
    admission::admission_canonical_path_return_indexed_preserves_wf(
        old, source, observed.summary,
    );
    live_canonical_path_return_preserves_ancestors(before, observed);
    let result = live_canonical_path_return_from_source(before, observed).unwrap();
    let after = result.state;
    let parent_index = before.path_frames.len() - 2;
    let next = admission::admission_canonical_path_return_indexed_step(
        old, source, observed.summary,
    ).unwrap();
    assert(crate::product_score_wf(before.base.source.product));
    assert(runtime::runtime_frame_image_wf(before.base.source.runtime));
    assert(after.base.source.product.bridge.admission == next);
    assert(after.base.source.runtime.score == after.base.source.product.bridge);
    assert(admission::admission_state_wf(after.base.source.product.bridge.admission));
    assert(bridge::bridge_well_formed(after.base.source.product.bridge));
    assert(map::integrated_admission_wf(after.base.source.product.map));
    assert(after.base.source.product.map.admission
        == after.base.source.product.bridge.admission);
    assert(after.base.source.product.map.transcript
        == after.base.source.product.bridge.transcript);
    assert(after.base.source.product.epm_frames.len() == next.frames.len());
    assert(after.base.source.product.indexed_regions.len() == next.frames.len());
    assert(crate::product_frame_source_anchors_match(after.base.source.product));
    assert(crate::product_epm_source_receipts_wf(after.base.source.product));
    assert forall|i: int| 0 <= i < after.base.source.product.epm_frames.len()
        implies {
            &&& epm::indexed_image_wf(after.base.source.product.indexed_regions[i])
            &&& after.base.source.product.indexed_regions[i].bytes
                == after.base.source.product.bridge.admission.frames[i].source_bytes
            &&& match after.base.source.product.epm_frames[i] {
                None => true,
                Some(frame) => {
                    &&& after.base.source.product.bridge.admission.frames[i].frame_kind == 3
                    &&& frame.snapshot == after.base.source.product.indexed_regions[i]
                    &&& epm::frame_layout_wf(frame)
                },
            }
        } by {
        if i < parent_index {
            assert(after.base.source.product.epm_frames[i]
                == before.base.source.product.epm_frames[i]);
            assert(after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]);
            assert(after.base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]);
        } else {
            assert(i == parent_index);
            assert(after.base.source.product.epm_frames[i].is_none());
            assert(after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]);
            assert(after.base.source.product.bridge.admission.frames[i].source_bytes
                == before.base.source.product.bridge.admission.frames[i].source_bytes);
        }
    }
    crate::product_score_wf_intro(after.base.source.product);
    assert(crate::product_score_wf(after.base.source.product));
    assert(next.frames.len() + 1 == old.frames.len());
    assert(after.base.source.runtime.capacity == before.base.source.runtime.capacity);
    assert(after.base.source.runtime.frame_bytes == before.base.source.runtime.frame_bytes);
    assert(after.base.source.runtime.max_frames == before.base.source.runtime.max_frames);
    assert(after.base.source.runtime.max_storage_bytes
        == before.base.source.runtime.max_storage_bytes);
    assert(after.base.source.runtime.max_capacity_bytes
        == before.base.source.runtime.max_capacity_bytes);
    assert(next.frames.len() > 0);
    assert(after.base.source.runtime.frame_bytes > 0);
    assert(next.frames.len() <= after.base.source.runtime.capacity as int);
    assert(after.base.source.runtime.capacity as int
        * after.base.source.runtime.frame_bytes as int <= usize::MAX as int);
    assert(after.base.source.runtime.max_frames as int >= next.frames.len());
    let old_len = old.frames.len();
    let new_len = next.frames.len();
    let frame_bytes = after.base.source.runtime.frame_bytes as int;
    let max_storage_bytes = after.base.source.runtime.max_storage_bytes as int;
    assert(max_storage_bytes >= old_len * frame_bytes);
    assert(new_len <= old_len);
    assert(frame_bytes > 0);
    assert(max_storage_bytes >= new_len * frame_bytes)
        by (nonlinear_arith)
        requires
            max_storage_bytes >= old_len * frame_bytes,
            new_len <= old_len,
            frame_bytes > 0;
    assert(after.base.source.runtime.max_capacity_bytes as int
        >= after.base.source.runtime.capacity as int
            * after.base.source.runtime.frame_bytes as int);
    assert(runtime::runtime_frame_image_wf(after.base.source.runtime));
    assert(crate::live_source_product_wf(after.base.source));
    assert(crate::live_runtime_product_wf(after.base));
    assert forall|i: int| 0 <= i < after.path_frames.len()
        implies path_frame_matches_live(after, i) by {
        if i < parent_index {
            assert(path_frame_matches_live(before, i));
            assert(after.path_frames[i] == before.path_frames[i]);
            assert(after.outstanding[i] == before.outstanding[i]);
            assert(after.base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]);
            assert(after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]);
            assert(after.base.source.wire[i] == before.base.source.wire[i]);
            assert(after.base.cursors[i] == before.base.cursors[i]);
            if before.outstanding[i].is_some() {
                if i + 1 < parent_index {
                    assert(after.base.source.product.bridge.admission.frames[i + 1]
                        == before.base.source.product.bridge.admission.frames[i + 1]);
                } else {
                    assert(i + 1 == parent_index);
                    assert(after.base.source.product.bridge.admission.frames[i + 1].return_id
                        == before.base.source.product.bridge.admission.frames[i + 1].return_id);
                    assert(after.base.source.product.bridge.admission.frames[i + 1].frame_kind
                        == before.base.source.product.bridge.admission.frames[i + 1].frame_kind);
                }
                assert(after.base.source.product.indexed_regions[i + 1]
                    == before.base.source.product.indexed_regions[i + 1]);
            }
            assert(path_frame_matches_live(after, i));
        } else {
            assert(i == parent_index);
            assert(path_frame_matches_live(before, i));
            let old_path = before.path_frames[i].unwrap();
            let resolved_path = live_canonical_path_resolved_frame(old_path);
            assert(after.outstanding[i].is_none());
            assert(after.path_frames[i] == Some(resolved_path));
            assert(path::canonical_path_frame_wf(resolved_path));
            assert(resolved_path.input == old_path.input);
            assert(resolved_path.cursor == old_path.cursor);
            assert(resolved_path.pending.is_none());
            assert(resolved_path.waiting.is_none());
            assert(after.base.source.product.bridge.admission.frames[i].frame_kind == 4);
            assert(after.base.source.product.bridge.admission.frames[i].program_type_id == 0);
            assert(admission::admission_canonical_path_slotless(
                after.base.source.product.bridge.admission.frames[i],
            ));
            assert(after.base.source.product.bridge.admission.frames[i].waiting_id.is_none());
            assert(after.base.source.product.bridge.admission.frames[i].waiting_kind == 0);
            assert(after.base.source.product.bridge.admission.frames[i].cursor
                == old_path.cursor);
            assert(after.base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]);
            assert(resolved_path.input == after.base.source.product.indexed_regions[i]);
            assert(after.base.source.wire[i].is_none());
            assert(after.base.cursors[i].is_none());
            assert(path_frame_matches_live(after, i));
        }
    }
    assert(live_canonical_path_wf(after));
}

}
