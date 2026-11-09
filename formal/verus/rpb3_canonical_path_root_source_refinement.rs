//! Standalone CanonicalPath root Complete, separate from scored Message EOF.
//!
//! The source boundary is the successful `CanonicalPathFrame::step` call to
//! `reducer.value(true, input.end_position().physical)` after its indexed
//! cursor is exhausted, followed by the no-parent branch of
//! `complete_admission_step`. The reducer value and its source binding are
//! observations, not consequences of this model. In particular this module
//! does not extract a reducer result or an indexed view from arbitrary Rust.

use vstd::prelude::*;

#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_admission_cursor.rs"]
mod rpb3_admission_cursor;
#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_score_transcript.rs"]
mod rpb3_score_transcript;
#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_score_admission_bridge.rs"]
mod rpb3_score_admission_bridge;
#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_finish_macro_refinement.rs"]
mod rpb3_finish_macro_refinement;
#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_epathmap_event_refinement.rs"]
mod rpb3_epathmap_event_refinement;
#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_path_parser_refinement.rs"]
pub mod rpb3_path_parser_refinement;
#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_runtime_frame_source_refinement.rs"]
mod rpb3_runtime_frame_source_refinement;
#[cfg(rpb3_canonical_path_root_standalone)]
#[path = "rpb3_canonical_path_driver_source_refinement.rs"]
mod rpb3_canonical_path_driver_source_refinement;

use crate::{
    rpb3_admission_cursor as admission,
    rpb3_canonical_path_driver_source_refinement as path,
    rpb3_epathmap_event_refinement as epm,
    rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_transcript as score,
};

verus! {

pub enum CanonicalPathFormImage {
    Bare,
    Split,
}

pub struct CanonicalPathSummaryImage {
    pub form: CanonicalPathFormImage,
    pub canonical_path_stable: bool,
}

/// `value_result` is the successful production reducer result, represented
/// here only after the caller has established the Rust-to-model observation.
/// `active_obligation` is separately observed because the generic frame
/// cursor has no reducer-state field.
pub struct CanonicalPathReducerEofObservation {
    pub has_value: bool,
    pub at: epm::SourceAnchor,
    pub active_obligation: bool,
    pub value_result: Option<CanonicalPathSummaryImage>,
}

/// Terminal output has no admission frame. The arena, transcript and Vec
/// capacity come from the pre-pop runtime image; no score handle is needed.
pub struct CanonicalPathRootReturnImage {
    pub summary: CanonicalPathSummaryImage,
    pub ghost arena: Seq<admission::AdmissionScoreBinding>,
    pub ghost transcript: score::ScoreArena,
    pub remaining_frames: usize,
    pub remaining_capacity: usize,
    pub frame_bytes: usize,
    pub max_frames: usize,
    pub max_storage_bytes: usize,
    pub max_capacity_bytes: usize,
    pub pushed: usize,
    pub completed: usize,
    pub nested_started: u64,
    pub nested_resolved: u64,
}

pub open spec fn indexed_root_start(input: epm::IndexedRegionImage) -> epm::SourceAnchor {
    if input.positions.len() == 0 { input.end } else { input.positions[0] }
}

pub open spec fn canonical_path_root_runtime_matches(
    before: runtime::RuntimeFrameImage,
    path_frame: path::CanonicalPathFrameImage,
) -> bool {
    if before.score.admission.frames.len() != 1 {
        false
    } else {
        let root = before.score.admission.frames[0];
        &&& root.frame_kind == 4
        &&& root.program_type_id == 0
        &&& admission::admission_canonical_path_slotless(root)
        &&& root.source_bytes == path_frame.input.bytes
        &&& root.region_start == 0
        &&& root.region_end as int == path_frame.input.bytes.len()
        &&& root.cursor == path_frame.cursor
        &&& root.physical_source == indexed_root_start(path_frame.input).source
        &&& root.physical_start == indexed_root_start(path_frame.input).offset
        &&& root.waiting_id.is_none()
        &&& root.waiting_kind == 0
        &&& root.return_id.is_none()
        &&& root.score_handle.is_none()
    }
}

/// These conditions must be bound to the real indexed cursor, reducer
/// `value(true, end)` call and returned `FrameSummary::CanonicalPath`. They
/// are not inferred from the numeric frame kind alone. The already-resolved
/// `pending` payload may remain recorded; no suspended wait may remain.
pub open spec fn canonical_path_root_source_ready(
    before: runtime::RuntimeFrameImage,
    path_frame: path::CanonicalPathFrameImage,
    reducer: CanonicalPathReducerEofObservation,
) -> bool {
    &&& runtime::runtime_frame_image_wf(before)
    &&& before.score.admission.frames.len() == 1
    &&& path::canonical_path_frame_wf(path_frame)
    &&& path_frame.waiting.is_none()
    &&& path_frame.cursor as int == path_frame.input.bytes.len()
    &&& reducer.has_value
    &&& reducer.at == path_frame.input.end
    &&& !reducer.active_obligation
    &&& reducer.value_result.is_some()
    &&& canonical_path_root_runtime_matches(before, path_frame)
}

/// This selector does not call Message `RootComplete`: that route requires a
/// score handle and frame kind 1. Here the only successful result comes from
/// the reducer's observed path summary, with no score-arena append.
pub open spec fn canonical_path_root_complete_from_source(
    before: runtime::RuntimeFrameImage,
    path_frame: path::CanonicalPathFrameImage,
    reducer: CanonicalPathReducerEofObservation,
) -> Option<CanonicalPathRootReturnImage> {
    if !canonical_path_root_source_ready(before, path_frame, reducer) {
        None
    } else {
        let root = before.score.admission.frames[0];
        Some(CanonicalPathRootReturnImage {
            summary: reducer.value_result.unwrap(),
            arena: before.score.admission.arena,
            transcript: before.score.transcript,
            remaining_frames: 0,
            remaining_capacity: before.capacity,
            frame_bytes: before.frame_bytes,
            max_frames: before.max_frames,
            max_storage_bytes: before.max_storage_bytes,
            max_capacity_bytes: before.max_capacity_bytes,
            pushed: before.score.admission.pushed,
            completed: before.score.admission.pushed,
            nested_started: root.nested_started,
            nested_resolved: root.nested_resolved,
        })
    }
}

/// The admission-frame invariant supplies balance once the observed root has
/// no wait. The reducer's active-obligation observation is a separate premise.
pub proof fn canonical_path_root_obligations_balanced(
    before: runtime::RuntimeFrameImage,
    path_frame: path::CanonicalPathFrameImage,
    reducer: CanonicalPathReducerEofObservation,
)
    requires canonical_path_root_source_ready(before, path_frame, reducer),
    ensures
        before.score.admission.frames[0].nested_started
            == before.score.admission.frames[0].nested_resolved,
        before.score.admission.frames[0].waiting_id.is_none(),
        path_frame.waiting.is_none(),
        !reducer.active_obligation,
{
    let root = before.score.admission.frames[0];
    assert(admission::admission_state_wf(before.score.admission));
    assert(admission::admission_frame_wf(root));
    assert(root.waiting_kind == 0);
}

/// This general theorem derives the exact no-score terminal image from the
/// source-matched EOF state. Its conclusion is not an arbitrary supplied
/// after-state: the remaining frame count, completion count, score arena,
/// transcript and storage fields are selected from `before`.
pub proof fn canonical_path_root_complete_is_exact(
    before: runtime::RuntimeFrameImage,
    path_frame: path::CanonicalPathFrameImage,
    reducer: CanonicalPathReducerEofObservation,
)
    requires canonical_path_root_source_ready(before, path_frame, reducer),
    ensures match canonical_path_root_complete_from_source(before, path_frame, reducer) {
        Some(returned) => {
            &&& returned.summary == reducer.value_result.unwrap()
            &&& returned.remaining_frames == 0
            &&& returned.arena == before.score.admission.arena
            &&& returned.transcript == before.score.transcript
            &&& returned.remaining_capacity == before.capacity
            &&& returned.frame_bytes == before.frame_bytes
            &&& returned.max_frames == before.max_frames
            &&& returned.max_storage_bytes == before.max_storage_bytes
            &&& returned.max_capacity_bytes == before.max_capacity_bytes
            &&& returned.pushed == before.score.admission.pushed
            &&& returned.completed as int
                == before.score.admission.completed as int + 1
            &&& returned.nested_started == returned.nested_resolved
        },
        None => false,
    },
{
    canonical_path_root_obligations_balanced(before, path_frame, reducer);
    assert(admission::admission_state_wf(before.score.admission));
    assert(before.score.admission.pushed as int
        == before.score.admission.completed as int + 1);
}

}

#[cfg(rpb3_canonical_path_root_standalone)]
fn main() {}
