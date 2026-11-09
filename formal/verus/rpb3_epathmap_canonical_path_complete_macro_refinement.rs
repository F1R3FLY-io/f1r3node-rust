//! EPathMap child Complete into a CanonicalPath NestedEpm1 obligation parent.
//!
//! The selector composes internal linked EPM topology steps, the child's End,
//! a producer-derived stable Map summary, and the kind-4 admission return.
//! It constructs the poststate rather than accepting an arbitrary one. Rust
//! Forward/Unwind batch expansion and concrete driver pre/post extraction are
//! separate obligations; this model covers only successful NestedEpm1 return.

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
    rpb3_score_admission_bridge as score_bridge, rpb3_score_transcript as score,
};

verus! {

pub struct CanonicalPathEpmCompleteOutcome {
    pub ghost finished_child: epm::LinkedEpmState,
    pub ghost admission: admission::AdmissionState,
    pub ghost indexed_regions: Seq<epm::IndexedRegionImage>,
    pub ghost transcript: score::ScoreArena,
}

pub open spec fn canonical_path_epm_internal_chain(
    states: Seq<epm::LinkedEpmState>,
    events: Seq<epm::EpmEvent>,
) -> bool {
    &&& states.len() == events.len() + 1
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger] epm::linked_epm_continue(
        states[i], states[i + 1], events[i],
    )
}

pub open spec fn canonical_path_epm_indexed_stack_wf(
    state: admission::AdmissionState,
    indexed_regions: Seq<epm::IndexedRegionImage>,
) -> bool {
    &&& indexed_regions.len() == state.frames.len()
    &&& forall|i: int| 0 <= i < state.frames.len() ==> {
        &&& epm::indexed_image_wf(indexed_regions[i])
        &&& indexed_regions[i].bytes == state.frames[i].source_bytes
        &&& epm::indexed_position_at(indexed_regions[i], 0).source
            == state.frames[i].physical_source
        &&& epm::indexed_position_at(indexed_regions[i], 0).offset
            == state.frames[i].physical_start
    }
}

/// `push` is the retained source-indexed CanonicalPath obligation request.
/// The producer derives both return source and summary from the terminal EPM
/// child. The core kind-4 selector then computes the only accepted poststate.
pub open spec fn canonical_path_epm_complete_macro_selector(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    push: admission::AdmissionCanonicalPathPushSource,
    transcript: score::ScoreArena,
) -> Option<CanonicalPathEpmCompleteOutcome> {
    if !canonical_path_epm_internal_chain(states, internal_events)
        || states.len() == 0
        || states[0].admission.frames.len() < 2
        || !score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: states[0].admission,
            transcript,
        })
    {
        None
    } else {
        let child = states.last();
        let finished = source_accept::epm_end_linked(child);
        if !producer::canonical_path_epm_successful_source_completion(child, finished, push) {
            None
        } else {
            let source = producer::canonical_path_epm_derived_return_source(child, push);
            let summary = producer::canonical_path_epm_derived_summary(child);
            match admission::admission_canonical_path_return_indexed_step(
                child.admission, source, summary,
            ) {
                None => None,
                Some(after) => Some(CanonicalPathEpmCompleteOutcome {
                    finished_child: finished,
                    admission: after,
                    indexed_regions: child.indexed_regions.drop_last(),
                    transcript,
                }),
            }
        }
    }
}

proof fn canonical_path_epm_internal_prefix_preserves(
    states: Seq<epm::LinkedEpmState>,
    events: Seq<epm::EpmEvent>,
    upto: int,
)
    requires
        canonical_path_epm_internal_chain(states, events),
        0 <= upto < states.len(),
        states[0].admission.frames.len() >= 2,
    ensures
        states[upto].admission.arena == states[0].admission.arena,
        states[upto].admission.pushed == states[0].admission.pushed,
        states[upto].admission.completed == states[0].admission.completed,
        states[upto].admission.next_obligation_id
            == states[0].admission.next_obligation_id,
        states[upto].indexed_regions == states[0].indexed_regions,
        states[upto].admission.frames.len() == states[0].admission.frames.len(),
        states[0].admission.frames.last().cursor
            <= states[upto].admission.frames.last().cursor,
        forall|i: int| 0 <= i < states[0].admission.frames.len() - 1
            ==> states[upto].admission.frames[i] == states[0].admission.frames[i],
    decreases upto,
{
    if upto > 0 {
        canonical_path_epm_internal_prefix_preserves(states, events, upto - 1);
        let previous = upto - 1;
        assert(0 <= previous < events.len());
        assert(canonical_path_epm_internal_chain(states, events));
        assert(epm::linked_epm_continue(
            states[previous], states[previous + 1], events[previous],
        ));
        epm::linked_epm_continue_preserves_source_and_progress(
            states[previous], states[upto], events[previous],
        );
        assert forall|i: int| 0 <= i < states[0].admission.frames.len() - 1
            implies states[upto].admission.frames[i] == states[0].admission.frames[i] by {
            assert(states[previous].admission.frames[i]
                == states[0].admission.frames[i]);
            assert(states[upto].admission.frames[i]
                == states[previous].admission.frames[i]);
        }
    }
}

pub proof fn canonical_path_epm_complete_macro_preserves_source_and_score(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    push: admission::AdmissionCanonicalPathPushSource,
    transcript: score::ScoreArena,
)
    requires canonical_path_epm_complete_macro_selector(
        states, internal_events, push, transcript,
    ).is_some(),
    ensures {
        let outcome = canonical_path_epm_complete_macro_selector(
            states, internal_events, push, transcript,
        ).unwrap();
        let child = states.last();
        let source = producer::canonical_path_epm_derived_return_source(child, push);
        let summary = producer::canonical_path_epm_derived_summary(child);
        let old_parent = states[0].admission.frames[states[0].admission.frames.len() - 2];
        let new_parent = outcome.admission.frames.last();
        let resolved = admission::admission_canonical_path_return_sidecar_after(
            source, summary,
        ).unwrap();
        &&& epm::linked_epm_complete(
            child, outcome.finished_child,
            epm::EpmAction::Complete {
                mode: child.epm.mode,
                entries_stable: child.epm.entries_stable,
            },
        )
        &&& producer::canonical_path_epm_successful_source_completion(
            child, outcome.finished_child, push,
        )
        &&& admission::admission_canonical_path_return_summary_from_source(
            child.admission, source, summary,
        )
        &&& admission::admission_state_wf(outcome.admission)
        &&& canonical_path_epm_indexed_stack_wf(outcome.admission, outcome.indexed_regions)
        &&& score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: outcome.admission,
            transcript: outcome.transcript,
        })
        &&& outcome.admission.frames.len() + 1 == states[0].admission.frames.len()
        &&& outcome.admission.completed as int
            == states[0].admission.completed as int + 1
        &&& outcome.admission.pushed == states[0].admission.pushed
        &&& outcome.admission.arena == states[0].admission.arena
        &&& outcome.transcript == transcript
        &&& outcome.indexed_regions == states[0].indexed_regions.drop_last()
        &&& states[0].admission.frames.last().cursor
            <= child.admission.frames.last().cursor
        &&& push.child == producer::canonical_path_epm_indexed_as_admission(
            child.epm.snapshot,
        )
        &&& child.epm.snapshot == child.indexed_regions.last()
        &&& push.kind == admission::AdmissionCanonicalPathObligationKind::NestedEpm1
        &&& push.child.bytes == child.epm.snapshot.bytes
        &&& push.child.end == (admission::AdmissionSourceAnchor {
            source: child.epm.snapshot.end.source,
            offset: child.epm.snapshot.end.offset,
        })
        &&& new_parent.frame_kind == 4
        &&& new_parent.source_bytes == old_parent.source_bytes
        &&& new_parent.physical_source == old_parent.physical_source
        &&& new_parent.physical_start == old_parent.physical_start
        &&& new_parent.waiting_id.is_none()
        &&& new_parent.waiting_kind == 0
        &&& new_parent.nested_resolved as int
            == old_parent.nested_resolved as int + 1
        &&& resolved.waiting_reducer_id.is_none()
        &&& resolved.waiting_stack_id.is_none()
        &&& resolved.resolved_reducer_id == push.reducer_obligation_id
        &&& match resolved.resolution {
            admission::AdmissionCanonicalPathResolution::NestedEpm1 {
                mode, entries_canonical_path_stable,
            } => mode == 2 && entries_canonical_path_stable,
            _ => false,
        }
        &&& forall|i: int| 0 <= i < outcome.admission.frames.len() - 1
            ==> outcome.admission.frames[i] == states[0].admission.frames[i]
    },
{
    let outcome = canonical_path_epm_complete_macro_selector(
        states, internal_events, push, transcript,
    ).unwrap();
    let last = states.len() - 1;
    let child = states[last];
    let source = producer::canonical_path_epm_derived_return_source(child, push);
    let summary = producer::canonical_path_epm_derived_summary(child);
    let resolved = admission::admission_canonical_path_return_sidecar_after(source, summary).unwrap();
    canonical_path_epm_internal_prefix_preserves(states, internal_events, last);
    producer::canonical_path_epm_completion_derives_return_summary(
        child, outcome.finished_child, push,
    );
    admission::admission_canonical_path_return_indexed_preserves_wf(
        child.admission, source, summary,
    );
    assert(outcome.admission.arena == child.admission.arena);
    assert(outcome.indexed_regions == child.indexed_regions.drop_last());
    assert(outcome.indexed_regions.len() == outcome.admission.frames.len());
    assert forall|i: int| 0 <= i < outcome.admission.frames.len()
        implies epm::indexed_image_wf(outcome.indexed_regions[i])
            && outcome.indexed_regions[i].bytes == outcome.admission.frames[i].source_bytes
            && epm::indexed_position_at(outcome.indexed_regions[i], 0).source
                == outcome.admission.frames[i].physical_source
            && epm::indexed_position_at(outcome.indexed_regions[i], 0).offset
                == outcome.admission.frames[i].physical_start by {
        assert(epm::linked_indexed_sources(child));
        if i < outcome.admission.frames.len() - 1 {
            assert(outcome.admission.frames[i] == child.admission.frames[i]);
        } else {
            assert(i == outcome.admission.frames.len() - 1);
            assert(outcome.admission.frames[i].source_bytes == child.admission.frames[i].source_bytes);
            assert(outcome.admission.frames[i].physical_source
                == child.admission.frames[i].physical_source);
            assert(outcome.admission.frames[i].physical_start
                == child.admission.frames[i].physical_start);
        }
    }
    assert(canonical_path_epm_indexed_stack_wf(outcome.admission, outcome.indexed_regions));
    assert(score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
        admission: outcome.admission,
        transcript,
    }));
    let old_parent = states[0].admission.frames[states[0].admission.frames.len() - 2];
    let new_parent = outcome.admission.frames.last();
    assert(new_parent.source_bytes == old_parent.source_bytes);
    assert(new_parent.physical_source == old_parent.physical_source);
    assert(new_parent.physical_start == old_parent.physical_start);
    assert(new_parent.nested_resolved as int == old_parent.nested_resolved as int + 1);
    assert(resolved.resolved_reducer_id == push.reducer_obligation_id);
    assert forall|i: int| 0 <= i < outcome.admission.frames.len() - 1
        implies outcome.admission.frames[i] == states[0].admission.frames[i] by {
        assert(outcome.admission.frames[i] == child.admission.frames[i]);
        assert(child.admission.frames[i] == states[0].admission.frames[i]);
    }
}

}

fn main() {}
