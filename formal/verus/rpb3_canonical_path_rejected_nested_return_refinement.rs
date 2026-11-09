//! Rejected NestedEpm1 child return into a CanonicalPath root.
//!
//! `CanonicalPathFrame::accept` applies the obligation resolution and pops the
//! completed child even when the nested EPM is Empty, Set, or an unstable Map.
//! The path reducer records a dead error; its later `value(true, end)` returns
//! that error instead of a CanonicalPath summary. The source-shaped EPM end,
//! reducer wait IDs, retained indexed Push, and tag offset still need binding
//! to each corresponding Rust observation.

use vstd::prelude::*;

mod rpb3_admission_cursor;
mod rpb3_score_transcript;
mod rpb3_score_admission_bridge;
pub mod rpb3_path_parser_refinement;
mod rpb3_epathmap_event_refinement;
mod rpb3_epm_source_accept_refinement;
mod rpb3_canonical_path_epm_return_producer;

use {
    rpb3_admission_cursor as admission, rpb3_canonical_path_epm_return_producer as producer,
    rpb3_epathmap_event_refinement as epm, rpb3_epm_source_accept_refinement as source_accept,
    rpb3_score_admission_bridge as bridge, rpb3_score_transcript as score,
};

verus! {

pub enum RejectedNestedEpmError {
    NestedEpmNotMap,
    NestedEpmNotStable,
}

pub struct RejectedNestedEpmReturnOutcome {
    pub ghost admission: admission::AdmissionState,
    pub ghost indexed_regions: Seq<epm::IndexedRegionImage>,
    pub ghost transcript: score::ScoreArena,
    pub resolution: admission::AdmissionCanonicalPathResolution,
    pub resolved_id: u64,
    pub error: RejectedNestedEpmError,
    pub error_source: admission::AdmissionSourceAnchor,
}

pub enum RejectedNestedEpmRootEof {
    Error { kind: RejectedNestedEpmError, at: admission::AdmissionSourceAnchor },
}

pub open spec fn rejected_nested_epm_error(
    mode: epm::EpmMode, entries_stable: bool,
) -> Option<RejectedNestedEpmError> {
    match mode {
        epm::EpmMode::Empty | epm::EpmMode::Set =>
            Some(RejectedNestedEpmError::NestedEpmNotMap),
        epm::EpmMode::Map => if entries_stable {
            None
        } else {
            Some(RejectedNestedEpmError::NestedEpmNotStable)
        },
    }
}

/// The production reducer's tag source is a separate observation. The guard
/// proves it is an indexed parent byte before the opaque payload; identifying
/// that byte as the reducer's tag still requires Rust-to-model extraction.
pub open spec fn rejected_nested_epm_return_source_ready(
    before: epm::LinkedEpmState,
    source: admission::AdmissionCanonicalPathReturnSource,
    transcript: score::ScoreArena,
    tag_offset: usize,
) -> bool {
    if before.admission.frames.len() != 2 {
        false
    } else {
        let parent = before.admission.frames[0];
        let child = before.admission.frames[1];
        let push = source.push;
        let finished = source_accept::epm_end_linked(before);
        &&& epm::linked_epm_complete(before, finished, epm::EpmAction::Complete {
            mode: before.epm.mode,
            entries_stable: before.epm.entries_stable,
        })
        &&& epm::frame_layout_wf(before.epm)
        &&& bridge::bridge_well_formed(bridge::ScoreAdmissionBridge {
            admission: before.admission, transcript,
        })
        &&& rejected_nested_epm_error(before.epm.mode, before.epm.entries_stable).is_some()
        &&& push.kind == admission::AdmissionCanonicalPathObligationKind::NestedEpm1
        &&& source.reducer_waiting_kind == push.kind
        &&& source.reducer_waiting_id == push.reducer_obligation_id
        &&& source.stack_return_id == push.stack_return_id
        &&& source.epm_entry_stability == seq![before.epm.entries_stable]
        &&& parent.frame_kind == 4
        &&& parent.program_type_id == 0
        &&& admission::admission_canonical_path_slotless(parent)
        &&& parent.waiting_id == Some(source.stack_return_id)
        &&& parent.waiting_kind == 3
        &&& parent.cursor == push.cursor_after_step
        &&& parent.nested_resolved < u64::MAX
        &&& child.frame_kind == 3
        &&& child.program_type_id == 1
        &&& admission::admission_canonical_path_slotless(child)
        &&& child.return_id == Some(source.stack_return_id)
        &&& child.body_id as int == source.stack_return_id as int + 1
        &&& child.score_handle.is_none()
        &&& before.admission.completed < usize::MAX
        &&& push.parent == producer::canonical_path_epm_indexed_as_admission(
            before.indexed_regions[0],
        )
        &&& push.child == producer::canonical_path_epm_indexed_as_admission(
            before.epm.snapshot,
        )
        &&& before.epm.snapshot == before.indexed_regions[1]
        &&& admission::admission_indexed_source_wf(push.parent)
        &&& admission::admission_indexed_source_wf(push.child)
        &&& push.payload_start <= push.payload_end
        &&& push.payload_end <= parent.cursor
        &&& push.child.bytes == push.parent.bytes.subrange(
            push.payload_start as int, push.payload_end as int,
        )
        &&& push.child.positions == push.parent.positions.subrange(
            push.payload_start as int, push.payload_end as int,
        )
        &&& push.child.end == push.terminal_anchor
        &&& tag_offset < push.payload_start
        &&& (tag_offset as int) < push.parent.positions.len()
    }
}

/// This is the post-accept image, not the successful kind-4 return selector.
/// The reducer error is retained as a separate sidecar while the admission
/// stack remains live with its CanonicalPath root.
pub open spec fn rejected_nested_epm_return_from_source(
    before: epm::LinkedEpmState,
    source: admission::AdmissionCanonicalPathReturnSource,
    transcript: score::ScoreArena,
    tag_offset: usize,
) -> Option<RejectedNestedEpmReturnOutcome> {
    if !rejected_nested_epm_return_source_ready(
        before, source, transcript, tag_offset,
    ) {
        None
    } else {
        let parent = before.admission.frames[0];
        let next_parent = admission::AdmissionFrameState {
            waiting_id: None,
            waiting_kind: 0,
            nested_resolved: (parent.nested_resolved as int + 1) as u64,
            ..parent
        };
        let next = admission::AdmissionState {
            frames: seq![next_parent],
            arena: before.admission.arena,
            root_end: before.admission.root_end,
            pushed: before.admission.pushed,
            completed: (before.admission.completed as int + 1) as usize,
            next_obligation_id: before.admission.next_obligation_id,
        };
        Some(RejectedNestedEpmReturnOutcome {
            admission: next,
            indexed_regions: before.indexed_regions.drop_last(),
            transcript,
            resolution: admission::AdmissionCanonicalPathResolution::NestedEpm1 {
                mode: producer::canonical_path_epm_mode_code(before.epm.mode),
                entries_canonical_path_stable: before.epm.entries_stable,
            },
            resolved_id: source.reducer_waiting_id,
            error: rejected_nested_epm_error(
                before.epm.mode, before.epm.entries_stable,
            ).unwrap(),
            error_source: source.push.parent.positions[tag_offset as int],
        })
    }
}

/// Once the now-unsuspended root reaches indexed EOF, the dead reducer
/// reports its retained error. A nonterminal cursor needs further byte edges;
/// this selector makes no claim about those intervening transitions.
pub open spec fn rejected_nested_epm_root_eof(
    outcome: RejectedNestedEpmReturnOutcome,
    at: epm::SourceAnchor,
) -> Option<RejectedNestedEpmRootEof> {
    if outcome.admission.frames.len() != 1
        || outcome.indexed_regions.len() != 1
        || outcome.admission.frames[0].cursor != outcome.admission.frames[0].region_end
        || at != outcome.indexed_regions[0].end
    {
        None
    } else {
        Some(RejectedNestedEpmRootEof::Error {
            kind: outcome.error,
            at: outcome.error_source,
        })
    }
}

pub proof fn rejected_nested_epm_return_applies_and_preserves(
    before: epm::LinkedEpmState,
    source: admission::AdmissionCanonicalPathReturnSource,
    transcript: score::ScoreArena,
    tag_offset: usize,
)
    requires rejected_nested_epm_return_source_ready(
        before, source, transcript, tag_offset,
    ),
    ensures match rejected_nested_epm_return_from_source(
        before, source, transcript, tag_offset,
    ) {
        Some(outcome) => {
            let old_parent = before.admission.frames[0];
            let new_parent = outcome.admission.frames[0];
            let summary = producer::canonical_path_epm_derived_summary(before);
            &&& admission::admission_state_wf(outcome.admission)
            &&& bridge::bridge_well_formed(bridge::ScoreAdmissionBridge {
                admission: outcome.admission, transcript: outcome.transcript,
            })
            &&& outcome.admission.frames.len() + 1 == before.admission.frames.len()
            &&& outcome.admission.completed as int == before.admission.completed as int + 1
            &&& outcome.admission.pushed == before.admission.pushed
            &&& outcome.admission.arena == before.admission.arena
            &&& outcome.transcript == transcript
            &&& outcome.indexed_regions == before.indexed_regions.drop_last()
            &&& outcome.indexed_regions[0] == before.indexed_regions[0]
            &&& epm::indexed_image_wf(outcome.indexed_regions[0])
            &&& new_parent.source_bytes == outcome.indexed_regions[0].bytes
            &&& new_parent.physical_source
                == epm::indexed_position_at(outcome.indexed_regions[0], 0).source
            &&& new_parent.physical_start
                == epm::indexed_position_at(outcome.indexed_regions[0], 0).offset
            &&& old_parent.source_bytes == new_parent.source_bytes
            &&& old_parent.cursor == new_parent.cursor
            &&& new_parent.waiting_id.is_none()
            &&& new_parent.waiting_kind == 0
            &&& new_parent.nested_resolved as int == old_parent.nested_resolved as int + 1
            &&& new_parent.nested_started == new_parent.nested_resolved
            &&& outcome.resolved_id == source.push.reducer_obligation_id
            &&& before.admission.frames[1].return_id == Some(source.stack_return_id)
            &&& source.stack_return_id == source.push.stack_return_id
            &&& source.reducer_waiting_id == source.push.reducer_obligation_id
            &&& source.push.child == producer::canonical_path_epm_indexed_as_admission(
                before.indexed_regions[1],
            )
            &&& outcome.error_source
                == source.push.parent.positions[tag_offset as int]
            &&& rejected_nested_epm_error(before.epm.mode, before.epm.entries_stable)
                == Some(outcome.error)
            &&& admission::admission_canonical_path_resolution_from_summary(
                source.push.kind, summary,
            ).is_none()
            &&& admission::admission_canonical_path_return_indexed_step(
                before.admission, source, summary,
            ).is_none()
        },
        None => false,
    },
{
    let outcome = rejected_nested_epm_return_from_source(
        before, source, transcript, tag_offset,
    ).unwrap();
    let old_parent = before.admission.frames[0];
    let new_parent = outcome.admission.frames[0];
    assert(admission::admission_state_wf(before.admission));
    assert(old_parent.waiting_kind == 3);
    assert(old_parent.nested_started as int == old_parent.nested_resolved as int + 1);
    assert(new_parent.nested_started == new_parent.nested_resolved);
    assert(admission::admission_frame_wf(old_parent));
    assert(admission::admission_frame_wf(new_parent));
    assert(admission::admission_state_wf(outcome.admission));
    assert(bridge::bridge_well_formed(bridge::ScoreAdmissionBridge {
        admission: outcome.admission, transcript,
    }));
    assert(epm::linked_indexed_sources(before));
    assert(outcome.indexed_regions[0] == before.indexed_regions[0]);
    assert(outcome.admission.frames[0].source_bytes == outcome.indexed_regions[0].bytes);
    assert(rejected_nested_epm_error(before.epm.mode, before.epm.entries_stable).is_some());
    assert(admission::admission_canonical_path_resolution_from_summary(
        source.push.kind, producer::canonical_path_epm_derived_summary(before),
    ).is_none());
}

pub proof fn rejected_nested_epm_root_eof_is_error(
    before: epm::LinkedEpmState,
    source: admission::AdmissionCanonicalPathReturnSource,
    transcript: score::ScoreArena,
    tag_offset: usize,
)
    requires
        rejected_nested_epm_return_source_ready(
            before, source, transcript, tag_offset,
        ),
        source.push.cursor_after_step == before.admission.frames[0].region_end,
    ensures {
        let outcome = rejected_nested_epm_return_from_source(
            before, source, transcript, tag_offset,
        ).unwrap();
        let at = outcome.indexed_regions[0].end;
        match rejected_nested_epm_root_eof(outcome, at) {
            Some(RejectedNestedEpmRootEof::Error { kind, at: source_at }) => {
                &&& kind == outcome.error
                &&& source_at == source.push.parent.positions[tag_offset as int]
                &&& outcome.admission.frames[0].waiting_id.is_none()
                &&& outcome.admission.frames[0].cursor
                    == outcome.admission.frames[0].region_end
            },
            None => false,
        }
    },
{
    rejected_nested_epm_return_applies_and_preserves(
        before, source, transcript, tag_offset,
    );
}

}

fn main() {}
