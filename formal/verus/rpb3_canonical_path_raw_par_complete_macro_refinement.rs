//! RawParEscape Message Complete into a CanonicalPath obligation parent.
//!
//! The child's finished frame determines its Par summary; the kind-4 indexed
//! return selector determines the post-pop admission state. The score arena
//! is already present before this driver Complete and does not change during
//! the pop/parent `accept`. Binding an arbitrary Rust `MessageFrame::finish`,
//! `DriverCompleteSource`, and indexed push receipt to these modeled facts
//! remains a separate extraction obligation.

use vstd::prelude::*;

mod rpb3_admission_cursor;
mod rpb3_score_transcript;
mod rpb3_score_admission_bridge;

use {
    rpb3_admission_cursor as admission, rpb3_score_admission_bridge as score_bridge,
    rpb3_score_transcript as score,
};

verus! {

pub struct CanonicalPathRawParCompleteOutcome {
    pub ghost admission: admission::AdmissionState,
    pub ghost transcript: score::ScoreArena,
}

/// This selector never accepts a caller-supplied summary or poststate. It
/// derives Par flags from the finished Message child, requires the successful
/// RawParEscape unstable-path arm, then uses the kind-4 return function. Its
/// return source carries independently observed reducer and stack IDs; their
/// Rust extraction remains an explicit obligation.
pub open spec fn canonical_path_raw_par_complete_macro_selector(
    before: admission::AdmissionState,
    source: admission::AdmissionCanonicalPathReturnSource,
    transcript: score::ScoreArena,
) -> Option<CanonicalPathRawParCompleteOutcome> {
    if before.frames.len() < 2
        || source.push.kind != admission::AdmissionCanonicalPathObligationKind::RawParEscape
        || !admission::admission_state_wf(before)
        || !score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: before, transcript,
        })
    {
        None
    } else {
        let parent = before.frames[before.frames.len() - 2];
        let child = before.frames.last();
        match admission::concrete_derived_summary(parent, child) {
            Some(summary) => match summary {
                admission::AdmissionReturnedKind::Par {
                    canonical_path_stable: false, ..
                } => match admission::admission_canonical_path_return_indexed_step(
                    before, source, summary,
                ) {
                    Some(after) => Some(CanonicalPathRawParCompleteOutcome {
                        admission: after, transcript,
                    }),
                    None => None,
                },
                _ => None,
            },
            None => None,
        }
    }
}

pub proof fn canonical_path_raw_par_complete_macro_preserves_source_and_score(
    before: admission::AdmissionState,
    source: admission::AdmissionCanonicalPathReturnSource,
    transcript: score::ScoreArena,
)
    requires canonical_path_raw_par_complete_macro_selector(
        before, source, transcript,
    ).is_some(),
    ensures {
        let outcome = canonical_path_raw_par_complete_macro_selector(
            before, source, transcript,
        ).unwrap();
        let child = before.frames.last();
        let old_parent = before.frames[before.frames.len() - 2];
        let new_parent = outcome.admission.frames.last();
        let push = source.push;
        let summary = admission::concrete_derived_summary(old_parent, child).unwrap();
        let resolved = admission::admission_canonical_path_return_sidecar_after(
            source, summary,
        ).unwrap();
        &&& admission::admission_state_wf(outcome.admission)
        &&& score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: outcome.admission, transcript: outcome.transcript,
        })
        &&& outcome.admission.frames.len() + 1 == before.frames.len()
        &&& outcome.admission.completed as int == before.completed as int + 1
        &&& outcome.admission.pushed == before.pushed
        &&& outcome.admission.arena == before.arena
        &&& outcome.transcript == transcript
        &&& admission::admission_canonical_path_return_summary_from_source(
            before, source, summary,
        )
        &&& match summary {
            admission::AdmissionReturnedKind::Par {
                semantic_default, canonical_path_stable,
            } => semantic_default == admission::admission_raw_par_semantic_default(child)
                && !canonical_path_stable
                && admission::admission_raw_par_path_stability(child) == Some(false),
            _ => false,
        }
        &&& push.kind == admission::AdmissionCanonicalPathObligationKind::RawParEscape
        &&& child.return_id == Some(push.stack_return_id)
        &&& old_parent.waiting_id == Some(push.stack_return_id)
        &&& source.reducer_waiting_id == push.reducer_obligation_id
        &&& source.reducer_waiting_kind == push.kind
        &&& source.stack_return_id == push.stack_return_id
        &&& new_parent.frame_kind == 4
        &&& new_parent.waiting_id.is_none()
        &&& new_parent.waiting_kind == 0
        &&& new_parent.nested_resolved == old_parent.nested_resolved
        &&& new_parent.source_bytes == push.parent.bytes
        &&& new_parent.physical_source
            == admission::admission_indexed_source_position(push.parent, 0).unwrap().source
        &&& new_parent.physical_start
            == admission::admission_indexed_source_position(push.parent, 0).unwrap().offset
        &&& child.source_bytes == push.child.bytes
        &&& push.child.positions == push.parent.positions.subrange(
            push.payload_start as int, push.payload_end as int,
        )
        &&& push.child.end == push.terminal_anchor
        &&& resolved.waiting_reducer_id.is_none()
        &&& resolved.waiting_stack_id.is_none()
        &&& resolved.resolved_reducer_id == push.reducer_obligation_id
        &&& match resolved.resolution {
            admission::AdmissionCanonicalPathResolution::RawPar {
                canonical_path_stable,
            } => !canonical_path_stable,
            _ => false,
        }
        &&& forall|i: int| 0 <= i < outcome.admission.frames.len() - 1
            ==> outcome.admission.frames[i] == before.frames[i]
    },
{
    let outcome = canonical_path_raw_par_complete_macro_selector(
        before, source, transcript,
    ).unwrap();
    let child = before.frames.last();
    let old_parent = before.frames[before.frames.len() - 2];
    let new_parent = outcome.admission.frames.last();
    let push = source.push;
    let summary = admission::concrete_derived_summary(old_parent, child).unwrap();
    admission::concrete_derived_path_stability_is_child_derived(old_parent, child);
    admission::admission_canonical_path_return_indexed_preserves_wf(
        before, source, summary,
    );
    assert(outcome.admission.arena == before.arena);
    assert(score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
        admission: outcome.admission, transcript,
    }));
    assert(new_parent.source_bytes == push.parent.bytes);
    assert(new_parent.physical_source
        == admission::admission_indexed_source_position(push.parent, 0).unwrap().source);
    assert(new_parent.physical_start
        == admission::admission_indexed_source_position(push.parent, 0).unwrap().offset);
    assert forall|i: int| 0 <= i < outcome.admission.frames.len() - 1
        implies outcome.admission.frames[i] == before.frames[i] by {
        assert(outcome.admission.frames[i] == before.frames[i]);
    }
}

}

fn main() {}
