//! CanonicalPath's indexed child handoff, before admission-stack composition.
//!
//! This models the source facts at the point where the path reducer has
//! completed an opaque payload and `CanonicalPathFrame::step` is about to
//! return `Push`. The payload end is explicit because an empty child can use
//! the length-byte anchor instead of the next input byte's anchor. A checked
//! Vec push proves only scalar storage facts; the indexed payload and reducer
//! state are model inputs. No Rust-to-ghost extraction is asserted here.

use vstd::prelude::*;

// Focused standalone verification uses the same root module identities as
// the integrated gate. When included by that gate, these declarations vanish.
#[cfg(rpb3_canonical_path_standalone)]
#[path = "rpb3_admission_cursor.rs"]
mod rpb3_admission_cursor;
#[cfg(rpb3_canonical_path_standalone)]
#[path = "rpb3_score_transcript.rs"]
mod rpb3_score_transcript;
#[cfg(rpb3_canonical_path_standalone)]
#[path = "rpb3_score_admission_bridge.rs"]
mod rpb3_score_admission_bridge;
#[cfg(rpb3_canonical_path_standalone)]
#[path = "rpb3_finish_macro_refinement.rs"]
mod rpb3_finish_macro_refinement;
#[cfg(rpb3_canonical_path_standalone)]
#[path = "rpb3_epathmap_event_refinement.rs"]
mod rpb3_epathmap_event_refinement;
#[cfg(rpb3_canonical_path_standalone)]
#[path = "rpb3_path_parser_refinement.rs"]
pub mod rpb3_path_parser_refinement;
#[cfg(rpb3_canonical_path_standalone)]
#[path = "rpb3_runtime_frame_source_refinement.rs"]
mod rpb3_runtime_frame_source_refinement;

use crate::{
    rpb3_epathmap_event_refinement as epm, rpb3_runtime_frame_source_refinement as runtime_source,
};

verus! {

pub struct CanonicalPathPendingImage {
    pub id: u64,
    pub kind: epm::PathObligationKind,
    pub span: epm::ByteSpan,
    pub end: epm::SourceAnchor,
}

pub struct CanonicalPathWaitImage {
    pub id: u64,
    pub kind: epm::PathObligationKind,
    pub return_id: u64,
}

pub struct CanonicalPathFrameImage {
    pub ghost input: epm::IndexedRegionImage,
    pub cursor: usize,
    pub pending: Option<CanonicalPathPendingImage>,
    pub waiting: Option<CanonicalPathWaitImage>,
}

pub struct CanonicalPathChildRequestImage {
    pub kind: runtime_source::SourceFrameRequestKind,
    pub obligation_id: u64,
    pub return_id: u64,
    pub ghost body: epm::IndexedRegionImage,
}

pub open spec fn canonical_path_child_kind(
    kind: epm::PathObligationKind,
) -> runtime_source::SourceFrameRequestKind {
    match kind {
        epm::PathObligationKind::RawParEscape =>
            runtime_source::SourceFrameRequestKind::Message,
        epm::PathObligationKind::NestedEpm1 =>
            runtime_source::SourceFrameRequestKind::EPathMap,
    }
}

pub open spec fn canonical_path_frame_wf(frame: CanonicalPathFrameImage) -> bool {
    &&& epm::indexed_image_wf(frame.input)
    &&& frame.cursor as int <= frame.input.bytes.len()
    &&& match frame.pending {
        None => frame.waiting.is_none(),
        Some(pending) => {
            &&& pending.span.start <= pending.span.end
            &&& pending.span.end <= frame.cursor
            &&& match frame.waiting {
                None => true,
                Some(wait) => wait.id == pending.id && wait.kind == pending.kind,
            }
        },
    }
}

/// Select the child from the retained indexed input, including every byte's
/// physical position. The reducer-supplied terminal anchor remains explicit.
pub open spec fn canonical_path_selected_child(
    frame: CanonicalPathFrameImage,
    return_id: u64,
) -> Option<CanonicalPathChildRequestImage> {
    if !canonical_path_frame_wf(frame) || frame.waiting.is_some() {
        None
    } else {
        match frame.pending {
            None => None,
            Some(pending) => {
                let view = epm::indexed_subview(frame.input, pending.span);
                Some(CanonicalPathChildRequestImage {
                    kind: canonical_path_child_kind(pending.kind),
                    obligation_id: pending.id,
                    return_id,
                    body: epm::IndexedRegionImage {
                        bytes: view.bytes,
                        positions: view.positions,
                        end: pending.end,
                    },
                })
            },
        }
    }
}

pub proof fn canonical_path_selected_child_is_exact(
    frame: CanonicalPathFrameImage,
    return_id: u64,
)
    requires
        canonical_path_frame_wf(frame),
        frame.pending.is_some(),
        frame.waiting.is_none(),
    ensures
        canonical_path_selected_child(frame, return_id).is_some(),
        {
            let request = canonical_path_selected_child(frame, return_id).unwrap();
            let pending = frame.pending.unwrap();
            &&& request.kind == canonical_path_child_kind(pending.kind)
            &&& request.obligation_id == pending.id
            &&& request.return_id == return_id
            &&& request.body.bytes == frame.input.bytes.subrange(
                pending.span.start as int, pending.span.end as int,
            )
            &&& request.body.positions == frame.input.positions.subrange(
                pending.span.start as int, pending.span.end as int,
            )
            &&& forall|i: int| 0 <= i < request.body.bytes.len()
                ==> request.body.bytes[i]
                    == frame.input.bytes[pending.span.start as int + i]
                    && request.body.positions[i]
                    == frame.input.positions[pending.span.start as int + i]
            &&& request.body.end == pending.end
            &&& epm::indexed_image_wf(request.body)
        },
{
    let pending = frame.pending.unwrap();
    let request = canonical_path_selected_child(frame, return_id).unwrap();
    assert(epm::indexed_image_wf(frame.input));
    assert(request.body.bytes == frame.input.bytes.subrange(
        pending.span.start as int, pending.span.end as int,
    ));
    assert(request.body.positions == frame.input.positions.subrange(
        pending.span.start as int, pending.span.end as int,
    ));
    assert forall|i: int| 0 <= i < request.body.bytes.len()
        implies request.body.bytes[i]
            == frame.input.bytes[pending.span.start as int + i]
            && request.body.positions[i]
            == frame.input.positions[pending.span.start as int + i] by {
        assert(request.body.bytes[i]
            == frame.input.bytes.subrange(
                pending.span.start as int, pending.span.end as int,
            )[i]);
        assert(request.body.positions[i]
            == frame.input.positions.subrange(
                pending.span.start as int, pending.span.end as int,
            )[i]);
    }
}

/// The checked storage observation is composable with an admission-stack
/// transition once kind-4 parent push and return are modeled there. This
/// relation does not infer a ghost request from production's scalar fields.
pub open spec fn canonical_path_observed_push_step(
    frame_before: CanonicalPathFrameImage,
    frame_after: CanonicalPathFrameImage,
    runtime_before: runtime_source::RuntimeFrameImage,
    request: CanonicalPathChildRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
) -> bool {
    &&& runtime_source::runtime_frame_image_wf(runtime_before)
    &&& runtime_before.score.admission.frames.len() <= usize::MAX as int
    &&& runtime_before.score.admission.frames.last().frame_kind == 4
    &&& stats_frame_variant_bytes == runtime_before.frame_bytes
    &&& canonical_path_selected_child(frame_before, request.return_id) == Some(request)
    &&& match frame_before.pending {
        None => false,
        Some(pending) => frame_after == CanonicalPathFrameImage {
            waiting: Some(CanonicalPathWaitImage {
                id: pending.id,
                kind: pending.kind,
                return_id: request.return_id,
            }),
            ..frame_before
        },
    }
    &&& match runtime_source::source_frame_push_observe_after(
        request.kind,
        actual_top_kind,
        runtime_before.score.admission.frames.len() as usize,
        runtime_before.capacity,
        after_len,
        after_capacity,
        runtime_before.frame_bytes,
        stats_frame_variant_bytes,
    ) {
        runtime_source::SourceFramePushObservationResult::Success { .. } => true,
        _ => false,
    }
}

pub proof fn canonical_path_observed_push_is_exact(
    frame_before: CanonicalPathFrameImage,
    frame_after: CanonicalPathFrameImage,
    runtime_before: runtime_source::RuntimeFrameImage,
    request: CanonicalPathChildRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires canonical_path_observed_push_step(
        frame_before, frame_after, runtime_before, request,
        actual_top_kind, after_len, after_capacity, stats_frame_variant_bytes,
    ),
    ensures
        canonical_path_frame_wf(frame_after),
        frame_after.input == frame_before.input,
        frame_after.cursor == frame_before.cursor,
        frame_after.pending == frame_before.pending,
        frame_after.waiting.is_some(),
        frame_after.waiting.unwrap().id == request.obligation_id,
        frame_after.waiting.unwrap().return_id == request.return_id,
        request.body.bytes == frame_before.input.bytes.subrange(
            frame_before.pending.unwrap().span.start as int,
            frame_before.pending.unwrap().span.end as int,
        ),
        request.body.positions == frame_before.input.positions.subrange(
            frame_before.pending.unwrap().span.start as int,
            frame_before.pending.unwrap().span.end as int,
        ),
        request.body.end == frame_before.pending.unwrap().end,
        epm::indexed_image_wf(request.body),
        actual_top_kind == Some(request.kind),
        after_len as int == runtime_before.score.admission.frames.len() + 1,
        after_len <= after_capacity,
        after_capacity >= runtime_before.capacity,
        stats_frame_variant_bytes == runtime_before.frame_bytes,
        match runtime_source::source_frame_push_observe_after(
            request.kind,
            actual_top_kind,
            runtime_before.score.admission.frames.len() as usize,
            runtime_before.capacity,
            after_len,
            after_capacity,
            runtime_before.frame_bytes,
            stats_frame_variant_bytes,
        ) {
            runtime_source::SourceFramePushObservationResult::Success { storage } =>
                runtime_source::source_successful_push_storage(runtime_before, storage),
            _ => false,
        },
{
    canonical_path_selected_child_is_exact(frame_before, request.return_id);
    assert(frame_before.pending.is_some());
    runtime_source::source_frame_push_observe_after_exact(
        request.kind, actual_top_kind,
        runtime_before.score.admission.frames.len() as usize,
        runtime_before.capacity, after_len, after_capacity,
        runtime_before.frame_bytes, stats_frame_variant_bytes,
    );
    runtime_source::source_frame_push_observed_storage_is_checked(
        runtime_before, request.kind, actual_top_kind,
        after_len, after_capacity,
    );
    match (request.kind, actual_top_kind) {
        (
            runtime_source::SourceFrameRequestKind::Message,
            Some(runtime_source::SourceFrameRequestKind::Message),
        ) => {},
        (
            runtime_source::SourceFrameRequestKind::EPathMap,
            Some(runtime_source::SourceFrameRequestKind::EPathMap),
        ) => {},
        _ => assert(false),
    }
}

}

#[cfg(rpb3_canonical_path_standalone)]
fn main() {}
