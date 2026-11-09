//! A modeled EPathMap driver Complete with a program-31 Message parent.
//!
//! One Rust `epathmap_step` can consume internal scalar topology events or
//! compact Forward/Unwind batches before it returns `FrameAction::Complete`.
//! The driver then pops that EPathMap child and accepts its summary in the
//! suspended Message parent. This module composes those *modeled* operations;
//! it does not derive their source projection from arbitrary Rust execution.
//! In particular, the Rust batch-to-scalar event expansion, the exact
//! `DriverCompleteSource` pre/post binding, and other parent kinds remain open.

use vstd::prelude::*;

mod rpb3_admission_cursor;
mod rpb3_score_transcript;
mod rpb3_score_admission_bridge;
pub mod rpb3_path_parser_refinement;
mod rpb3_epathmap_event_refinement;
mod rpb3_epm_source_accept_refinement;

use {
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_source_accept_refinement as source_accept,
    rpb3_score_admission_bridge as score_bridge, rpb3_score_transcript as score,
};

verus! {

pub struct Program31EpmCompleteOutcome {
    // End is an internal child state. It is not the returned driver state.
    pub ghost finished: epm::LinkedEpmState,
    pub ghost admission: admission::AdmissionState,
    pub ghost transcript: score::ScoreArena,
    pub ghost indexed_regions: Seq<epm::IndexedRegionImage>,
}

pub open spec fn program31_epm_internal_chain(
    states: Seq<epm::LinkedEpmState>,
    events: Seq<epm::EpmEvent>,
) -> bool {
    &&& states.len() == events.len() + 1
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger] epm::linked_epm_continue(
        states[i], states[i + 1], events[i],
    )
}

pub open spec fn program31_epm_source_stack_wf(
    admission_state: admission::AdmissionState,
    indexed_regions: Seq<epm::IndexedRegionImage>,
) -> bool {
    &&& indexed_regions.len() == admission_state.frames.len()
    &&& forall|i: int| 0 <= i < admission_state.frames.len() ==> {
        &&& epm::indexed_image_wf(indexed_regions[i])
        &&& indexed_regions[i].bytes == admission_state.frames[i].source_bytes
        &&& epm::indexed_position_at(indexed_regions[i], 0).source
            == admission_state.frames[i].physical_source
        &&& epm::indexed_position_at(indexed_regions[i], 0).offset
            == admission_state.frames[i].physical_start
    }
}

/// The terminal selector uses the linked EPM End transition and the existing
/// source-checked program-31 slot-3/tag-9 acceptance. No arbitrary poststate
/// is supplied by the caller. `None` includes every other parent kind.
pub open spec fn program31_epm_complete_macro(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    transcript: score::ScoreArena,
) -> Option<Program31EpmCompleteOutcome> {
    if !program31_epm_internal_chain(states, internal_events)
        || states.len() == 0
        || states[0].admission.frames.len() < 2
        || !score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: states[0].admission,
            transcript,
        })
    {
        None
    } else {
        let active = states.last();
        let finished = source_accept::epm_end_linked(active);
        match source_accept::epm_source_accept(active, finished) {
            None => None,
            Some(admission_state) => Some(Program31EpmCompleteOutcome {
                finished,
                admission: admission_state,
                transcript,
                indexed_regions: active.indexed_regions.drop_last(),
            }),
        }
    }
}

pub proof fn program31_epm_internal_prefix_preserves(
    states: Seq<epm::LinkedEpmState>,
    events: Seq<epm::EpmEvent>,
    upto: int,
)
    requires
        program31_epm_internal_chain(states, events),
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
        states[upto].admission.frames.last().source_bytes
            == states[0].admission.frames.last().source_bytes,
        states[0].admission.frames.last().cursor
            <= states[upto].admission.frames.last().cursor,
        forall|i: int| 0 <= i < states[0].admission.frames.len() - 1
            ==> states[upto].admission.frames[i] == states[0].admission.frames[i],
    decreases upto,
{
    if upto > 0 {
        program31_epm_internal_prefix_preserves(states, events, upto - 1);
        let previous = upto - 1;
        assert(0 <= previous < events.len());
        assert(program31_epm_internal_chain(states, events));
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

/// Program-31 only: internal Continue events, child End, pop, then Message
/// slot-3/tag-9 acceptance are one observable driver Complete macro-action.
/// This proves the model composition, not Rust trace extraction or coverage of
/// nested EPathMap, CanonicalPath, or other Message parent branches.
pub proof fn program31_epm_complete_macro_preserves_source_and_score(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    transcript: score::ScoreArena,
)
    requires program31_epm_complete_macro(states, internal_events, transcript).is_some(),
    ensures {
        let outcome = program31_epm_complete_macro(states, internal_events, transcript).unwrap();
        let active = states.last();
        let parent_before = states[0].admission.frames[states[0].admission.frames.len() - 2];
        let parent_after = outcome.admission.frames.last();
        &&& epm::linked_epm_complete(
            active, outcome.finished,
            epm::EpmAction::Complete {
                mode: active.epm.mode,
                entries_stable: active.epm.entries_stable,
            },
        )
        &&& admission::admission_state_wf(outcome.admission)
        &&& score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: outcome.admission,
            transcript: outcome.transcript,
        })
        &&& program31_epm_source_stack_wf(outcome.admission, outcome.indexed_regions)
        &&& outcome.admission.frames.len() + 1 == states[0].admission.frames.len()
        &&& outcome.admission.completed as int
            == states[0].admission.completed as int + 1
        &&& outcome.admission.pushed == states[0].admission.pushed
        &&& outcome.admission.arena == states[0].admission.arena
        &&& outcome.transcript == transcript
        &&& outcome.indexed_regions == states[0].indexed_regions.drop_last()
        &&& states[0].admission.frames.last().cursor
            <= active.admission.frames.last().cursor
        &&& parent_after.source_bytes == parent_before.source_bytes
        &&& parent_after.physical_source == parent_before.physical_source
        &&& parent_after.physical_start == parent_before.physical_start
        &&& parent_after.waiting_id.is_none()
        &&& parent_after.returns.last().slot == 3
        &&& parent_after.returns.last().tag == 9
        &&& parent_after.returns.last().body_bytes == active.epm.snapshot.bytes
        &&& match parent_after.returns.last().kind {
            admission::AdmissionReturnedKind::EPathMapSnapshot {
                mode, entries_canonical_path_stable,
            } => mode == source_accept::epm_mode_code(active.epm.mode)
                && entries_canonical_path_stable == active.epm.entries_stable,
            _ => false,
        }
        &&& forall|i: int| 0 <= i < outcome.admission.frames.len() - 1
            ==> outcome.admission.frames[i] == states[0].admission.frames[i]
    },
{
    let outcome = program31_epm_complete_macro(states, internal_events, transcript).unwrap();
    let last = states.len() - 1;
    let active = states[last];
    let accepted = outcome.admission;
    assert(program31_epm_internal_chain(states, internal_events));
    assert(states[0].admission.frames.len() >= 2);
    program31_epm_internal_prefix_preserves(states, internal_events, last);
    assert(source_accept::epm_source_accept(active, outcome.finished) == Some(accepted));
    source_accept::epm_source_accept_carries_end_summary(active, outcome.finished, accepted);
    source_accept::epm_source_finish_accept_preserves_wf(active);
    assert(accepted.arena == active.admission.arena);
    assert(outcome.indexed_regions == active.indexed_regions.drop_last());
    assert(accepted.frames.len() + 1 == active.admission.frames.len());
    assert(outcome.indexed_regions.len() == accepted.frames.len());
    assert forall|i: int| 0 <= i < accepted.frames.len()
        implies epm::indexed_image_wf(outcome.indexed_regions[i])
            && outcome.indexed_regions[i].bytes == accepted.frames[i].source_bytes
            && epm::indexed_position_at(outcome.indexed_regions[i], 0).source
                == accepted.frames[i].physical_source
            && epm::indexed_position_at(outcome.indexed_regions[i], 0).offset
                == accepted.frames[i].physical_start by {
        assert(epm::linked_indexed_sources(active));
        if i < accepted.frames.len() - 1 {
            assert(accepted.frames[i] == active.admission.frames[i]);
        } else {
            assert(i == accepted.frames.len() - 1);
            assert(accepted.frames[i].source_bytes == active.admission.frames[i].source_bytes);
            assert(accepted.frames[i].physical_source
                == active.admission.frames[i].physical_source);
            assert(accepted.frames[i].physical_start
                == active.admission.frames[i].physical_start);
        }
    }
    assert(program31_epm_source_stack_wf(accepted, outcome.indexed_regions));
    assert(score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
        admission: accepted,
        transcript,
    }));
    assert forall|i: int| 0 <= i < accepted.frames.len() - 1
        implies accepted.frames[i] == states[0].admission.frames[i] by {
        assert(accepted.frames[i] == active.admission.frames[i]);
        assert(active.admission.frames[i] == states[0].admission.frames[i]);
    }
    let parent_before = states[0].admission.frames[states[0].admission.frames.len() - 2];
    let parent_after = accepted.frames.last();
    assert(epm::linked_epm_complete(
        active, outcome.finished,
        epm::EpmAction::Complete {
            mode: active.epm.mode,
            entries_stable: active.epm.entries_stable,
        },
    ));
    assert(admission::admission_state_wf(accepted));
    assert(accepted.frames.len() + 1 == states[0].admission.frames.len());
    assert(accepted.completed as int == states[0].admission.completed as int + 1);
    assert(accepted.pushed == states[0].admission.pushed);
    assert(accepted.arena == states[0].admission.arena);
    assert(outcome.indexed_regions == states[0].indexed_regions.drop_last());
    assert(parent_after.source_bytes == parent_before.source_bytes);
    assert(parent_after.physical_source == parent_before.physical_source);
    assert(parent_after.physical_start == parent_before.physical_start);
    assert(parent_after.waiting_id.is_none());
    assert(parent_after.returns.last().slot == 3);
    assert(parent_after.returns.last().tag == 9);
    assert(parent_after.returns.last().body_bytes == active.epm.snapshot.bytes);
    assert(match parent_after.returns.last().kind {
        admission::AdmissionReturnedKind::EPathMapSnapshot {
            mode, entries_canonical_path_stable,
        } => mode == source_accept::epm_mode_code(active.epm.mode)
            && entries_canonical_path_stable == active.epm.entries_stable,
        _ => false,
    });
}

}

fn main() {}
