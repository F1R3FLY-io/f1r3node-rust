//! A nested EPathMap child Complete with an EPathMap obligation parent.
//!
//! One production `epathmap_step` may consume internal topology events before
//! returning `FrameAction::Complete`. The driver then pops the child and calls
//! the suspended EPathMap parent's `accept`. This model composes the linked
//! child End and source-checked nested-obligation return with that internal
//! chain. Rust batch-to-scalar extraction and concrete pre/post projection
//! remain separate obligations; this is not a universal driver simulation.

use vstd::prelude::*;

mod rpb3_admission_cursor;
mod rpb3_score_transcript;
mod rpb3_score_admission_bridge;
pub mod rpb3_path_parser_refinement;
mod rpb3_epathmap_event_refinement;
mod rpb3_epm_nested_source_refinement;

use {
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_nested_source_refinement as nested, rpb3_score_admission_bridge as score_bridge,
    rpb3_score_transcript as score,
};

verus! {

pub open spec fn nested_epm_internal_chain(
    states: Seq<epm::LinkedEpmState>, events: Seq<epm::EpmEvent>,
) -> bool {
    &&& states.len() == events.len() + 1
    &&& forall|i: int| 0 <= i < events.len() ==> #[trigger] epm::linked_epm_continue(
        states[i], states[i + 1], events[i],
    )
}

/// `parent_before.epm` is the suspended parent; `states.last().epm` is the
/// active child. `epm_frames` is the independently projected active stack.
/// The final relation is the existing source-derived parent acceptance,
/// including its child End event and one admission-stack pop.
pub open spec fn nested_epm_complete_macro(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    parent_before: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
    parent_after: epm::LinkedEpmState,
    transcript: score::ScoreArena,
) -> bool {
    &&& nested_epm_internal_chain(states, internal_events)
    &&& states.len() > 0
    &&& states[0].admission.frames.len() >= 2
    &&& score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
        admission: states[0].admission, transcript,
    })
    &&& parent_before.admission == states.last().admission
    &&& parent_before.indexed_regions == states.last().indexed_regions
    &&& epm_frames.len() == states.last().admission.frames.len()
    &&& epm_frames.last() == Some(states.last().epm)
    &&& epm_frames[epm_frames.len() - 2] == Some(parent_before.epm)
    &&& states.last().epm.mode == epm::EpmMode::Map
    &&& states.last().epm.entries_stable
    &&& match parent_before.epm.wait {
        Some(epm::EpmWait::Obligation {
            id, kind: epm::PathObligationKind::NestedEpm1, return_id,
        }) => {
            &&& states.last().admission.frames.last().return_id == Some(return_id)
            &&& match parent_before.epm.path.pending {
                Some(pending) => pending.id == id
                    && pending.kind == epm::PathObligationKind::NestedEpm1,
                None => false,
            }
        },
        _ => false,
    }
    &&& nested::source_linked_epm_child_return(
        parent_before, parent_after, epm_frames,
    )
}

pub proof fn nested_epm_internal_prefix_preserves(
    states: Seq<epm::LinkedEpmState>, events: Seq<epm::EpmEvent>, upto: int,
)
    requires
        nested_epm_internal_chain(states, events),
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
        nested_epm_internal_prefix_preserves(states, events, upto - 1);
        let previous = upto - 1;
        assert(0 <= previous < events.len());
        assert(nested_epm_internal_chain(states, events));
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

/// The End/Complete event belongs to the child and the resolved wait belongs
/// to its EPathMap parent. The theorem keeps those states distinct and proves
/// one driver-level pop and obligation return for any source-matched stable
/// Map child. `parent_after` remains an observed, relation-checked reducer
/// projection because the existing `accept_child` model is relational.
pub proof fn nested_epm_complete_macro_preserves_source_and_score(
    states: Seq<epm::LinkedEpmState>,
    internal_events: Seq<epm::EpmEvent>,
    parent_before: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
    parent_after: epm::LinkedEpmState,
    transcript: score::ScoreArena,
)
    requires nested_epm_complete_macro(
        states, internal_events, parent_before, epm_frames, parent_after, transcript,
    ),
    ensures {
        let child = states.last();
        let finished_child = nested::nested_end_frame(child.epm);
        let old_parent = states[0].admission.frames[states[0].admission.frames.len() - 2];
        let new_parent = parent_after.admission.frames.last();
        &&& epm::linked_epm_complete(
            child,
            epm::LinkedEpmState {
                epm: finished_child,
                admission: child.admission,
                indexed_regions: child.indexed_regions,
            },
            epm::EpmAction::Complete {
                mode: child.epm.mode,
                entries_stable: child.epm.entries_stable,
            },
        )
        &&& admission::admission_state_wf(parent_after.admission)
        &&& epm::linked_indexed_sources(parent_after)
        &&& score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: parent_after.admission, transcript,
        })
        &&& parent_after.admission.frames.len() + 1
            == states[0].admission.frames.len()
        &&& parent_after.admission.completed as int
            == states[0].admission.completed as int + 1
        &&& parent_after.admission.pushed == states[0].admission.pushed
        &&& parent_after.admission.arena == states[0].admission.arena
        &&& parent_after.indexed_regions == states[0].indexed_regions.drop_last()
        &&& child.epm.snapshot == child.indexed_regions.last()
        &&& parent_before.epm.snapshot
            == child.indexed_regions[child.indexed_regions.len() - 2]
        &&& parent_after.indexed_regions.last()
            == states[0].indexed_regions[states[0].indexed_regions.len() - 2]
        &&& states[0].admission.frames.last().cursor
            <= child.admission.frames.last().cursor
        &&& new_parent.frame_kind == 3
        &&& new_parent.source_bytes == old_parent.source_bytes
        &&& new_parent.physical_source == old_parent.physical_source
        &&& new_parent.physical_start == old_parent.physical_start
        &&& new_parent.waiting_id.is_none()
        &&& new_parent.waiting_kind == 0
        &&& new_parent.nested_resolved as int
            == old_parent.nested_resolved as int + 1
        &&& parent_after.epm.wait.is_none()
        &&& parent_after.epm.path.pending.is_none()
        &&& parent_after.epm.entries_stable == parent_before.epm.entries_stable
        &&& forall|i: int| 0 <= i < parent_after.admission.frames.len() - 1
            ==> parent_after.admission.frames[i] == states[0].admission.frames[i]
    },
{
    let last = states.len() - 1;
    let child = states[last];
    let finished_child = nested::nested_end_frame(child.epm);
    let derived = nested::source_derived_epm_return(parent_before, epm_frames).unwrap();
    nested_epm_internal_prefix_preserves(states, internal_events, last);
    nested::source_epm_return_has_derived_summary(
        parent_before, parent_after, epm_frames,
    );
    assert(epm::linked_epm_child_return(
        parent_before, parent_after, derived.summary, derived.admission_summary,
        derived.nested_before, derived.nested_after,
    ));
    assert(derived.nested_before == Some(child.epm));
    assert(derived.nested_after == Some(finished_child));
    assert(derived.summary == epm::ChildSummary::Epm1 {
        mode: child.epm.mode,
        entries_stable: child.epm.entries_stable,
    });
    assert(epm::linked_epm_complete(
        child,
        epm::LinkedEpmState {
            epm: finished_child,
            admission: child.admission,
            indexed_regions: child.indexed_regions,
        },
        epm::EpmAction::Complete {
            mode: child.epm.mode,
            entries_stable: child.epm.entries_stable,
        },
    ));
    assert(parent_after.admission.arena == parent_before.admission.arena);
    assert(parent_before.admission.arena == states[0].admission.arena);
    assert(score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
        admission: parent_after.admission, transcript,
    }));
    let old_parent = states[0].admission.frames[states[0].admission.frames.len() - 2];
    let new_parent = parent_after.admission.frames.last();
    assert(new_parent == epm::epm_parent_resolved(
        parent_before.admission.frames[parent_before.admission.frames.len() - 2],
    ));
    assert(new_parent.source_bytes == old_parent.source_bytes);
    assert(new_parent.physical_source == old_parent.physical_source);
    assert(new_parent.physical_start == old_parent.physical_start);
    assert(new_parent.nested_resolved as int
        == old_parent.nested_resolved as int + 1);
    assert forall|i: int| 0 <= i < parent_after.admission.frames.len() - 1
        implies parent_after.admission.frames[i] == states[0].admission.frames[i] by {
        assert(parent_after.admission.frames[i] == parent_before.admission.frames[i]);
        assert(parent_before.admission.frames[i] == states[0].admission.frames[i]);
    }
}

}

fn main() {}
